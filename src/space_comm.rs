//! Space communication screen: the retail conversation engine (`.cnv`), the
//! comm-screen session (greeting, tabs, responses, diplomacy actions) and the
//! NPC store of the Trade tab.
//!
//! Sources (see `notes\space-comm-notes.md`):
//! * File format: `space_conversations~` 0x055ADA23, type `cnv` 0x055ADA24,
//!   read as SporeModder-FX `CnvUnit` / ModAPI `ConversationResource`
//!   (VERIFIED: all 56 installed files parse to their last byte).
//! * Text choice 0xAEE210 (VERIFIED): every text whose required flags are all
//!   set and whose excluded flags are all clear is a candidate; one is drawn
//!   with `RandomInt(n)`.
//! * Responses 0xAEE440 / 0xAED500 (VERIFIED): shown when the required flags
//!   are set and the excluded flags clear; enabled when the second pair
//!   (enable-require / enable-exclude) holds; the label is drawn like a text.
//! * Flags 0xAED680 (VERIFIED, the subset OpenSpore can know): see
//!   [`space_flags`].
//! * Actions 0xAEB730 + 0x102DF30 (VERIFIED): goto dialog 0x02D5DCEC, main menu
//!   0xCAFF0F72, exit 0x01994745, gifts 0x102CAF0, alliance 0x102CE40 / break
//!   0x102CDA0, peace offers 0x102CF20, system purchase offers 0x102D1C0, trade
//!   route request (inline), NPC incoming messages (0xFE9410).
//! * The tabs (Trade / Repair / Recharge / Missions / Diplomacy) show only on
//!   the main menu dialog `default` of `space_conversation_main_menu`
//!   (ShowCommEvent 0xAEBE10 -> 0xDD4AB0, VERIFIED); response buttons are made
//!   by code in window 0x04ADC3BF, 18 px rows, orange 0xFFFFB400, disabled grey
//!   0xFF7F7F7F (0xDD3E60, VERIFIED).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;

use crate::{
    game::RunProgress,
    space::{empire_archetype, AlienMission, SpaceArchetype, SpaceCampaign},
    space_galaxy::EmpireId,
    space_rules::{self as rules, RelationshipEvent},
};

/// Group and type of the conversation resources.
pub const CNV_GROUP: u32 = 0x055A_DA23;
pub const CNV_TYPE: u32 = 0x055A_DA24;
/// `space_conversation_main_menu`: the comm screen of an empire.
pub const MAIN_MENU_FILE: u32 = 0xC8FB_F7D7;
/// `incoming_messages`: what empires send on their own.
pub const INCOMING_FILE: u32 = 0x95BB_BBFC;
/// Raid events (`event_raid*`, `event_grob_*`): their `mission_assign` dialog.
pub const EVENT_RAID_FILE: u32 = 0x2E27_078E;
pub const EVENT_RAID_WAR_FILE: u32 = 0x7C39_2D99;
pub const EVENT_RAID_ALLY_FILE: u32 = 0xF0B6_2B43;
pub const EVENT_GROB_WAR_RAID_FILE: u32 = 0x9862_2544;
pub const EVENT_GROB_TEASER_RAID_FILE: u32 = 0xC524_3674;

/// The wallet cap (`space::SPICE_CAP`, int32 Sporebucks).
pub const WALLET_CAP: f32 = 2_000_000_000.0;

/// FNV-1 of a lower-cased name (the resource / action id of a name).
pub const fn fnv(name: &str) -> u32 {
    let b = name.as_bytes();
    let mut h = 0x811C_9DC5u32;
    let mut i = 0;
    while i < b.len() {
        h = h.wrapping_mul(0x0100_0193) ^ (b[i].to_ascii_lowercase() as u32);
        i += 1;
    }
    h
}

/// Dialog ids (FNV of the dialog names in the files).
#[allow(dead_code)]
pub mod dialog {
    pub const DEFAULT: u32 = 0x2EA8_FB98;
    pub const GREETINGS: u32 = 0xB485_84E3;
    pub const FIRST_GREETINGS: u32 = 0xA99D_2018;
    pub const DIPLOMACY: u32 = 0xF6F6_B0A5;
    pub const MISSION_MAIN: u32 = 0xD38E_52BF;
    pub const MISSION_UNAVAILABLE: u32 = 0x8053_662E;
    pub const GIVE_GIFT_RELATIONSHIP: u32 = 0x8EFA_19DC;
    pub const GIVE_GIFT_NO_RELATIONSHIP: u32 = 0x661D_5B89;
    pub const CREATE_ALLIANCE_SUCCESSFUL: u32 = 0x1251_0968;
    pub const CREATE_ALLIANCE_FAILURE: u32 = 0xB151_9E40;
    pub const BREAK_ALLIANCE_SUCCESSFUL: u32 = 0x2053_4E67;
    pub const GET_POSSE_ACCEPTED: u32 = 0xE914_6E3C;
    pub const TRADE_ROUTE_ACCEPTED: u32 = 0x917B_EE91;
    pub const TRADE_ROUTE_DECLINED_COUNT: u32 = 0xDC68_EAA0;
    pub const TRADE_ROUTE_DECLINED_RELATIONSHIP: u32 = 0x4584_B3CD;
    pub const TRADE_ROUTE_DESTROYED: u32 = 0x80E1_5BCF;
    pub const PEACE_BIG_SUCCESS: u32 = 0x38C5_12E1;
    pub const PEACE_SUCCESS: u32 = 0x6C5F_070E;
    pub const PEACE_FAILURE: u32 = 0xB885_EC95;
    pub const PEACE_BIG_FAILURE: u32 = 0x1273_C3F6;
    pub const PURCHASE_BIG_SUCCESS: u32 = 0xE8A5_68EC;
    pub const PURCHASE_SUCCESS: u32 = 0x57C9_8667;
    pub const PURCHASE_FAILURE: u32 = 0x8103_4EE0;
    pub const PURCHASE_BIG_FAILURE: u32 = 0x92A9_5B57;
    pub const REQUEST_ATTACK_DECLINED_COUNT: u32 = 0x1748_90AC;
    pub const SELECT_ATTACK_REQUEST_TARGET: u32 = 0xAC4E_8B42;
    // incoming_messages
    pub const WELCOME: u32 = 0xAF30_1A2C;
    pub const ECONOMIC_PURCHASE_READY: u32 = 0xDD33_319C;
    pub const WARN_TRESPASS: u32 = 0xDE4B_97ED;
    pub const OFFER_ALLIANCE: u32 = 0x0943_1B1F;
    pub const OFFER_GIFT: u32 = 0x4FC0_8A9E;
    pub const DECLARE_WAR: u32 = 0x5663_AB52;
    pub const DEMAND_TRIBUTE: u32 = 0x7FC0_2FBC;
    // special greetings of the main menu
    pub const FIRST_GREETINGS_AFTER_UPLIFT: u32 = 0x4CF2_2E16;
    pub const MYSPECIES_AFTER_DEATH: u32 = 0x6B9F_BCCF;
    pub const AFTER_DISASTER_HELPED: u32 = 0x305A_4A16;
    pub const AFTER_DISASTER_NOT_HELPED: u32 = 0xA789_391D;
    pub const FIRST_COLONY_GREETINGS: u32 = 0x0AA2_20A3;
    // The dialogs of `mission_multi_delivery` the mission class picks while it
    // runs (`func110h` 0xC57E90) and the buyer's replies to a delivery.
    pub const DYNAMIC_RESPONSE_TO_SOURCE: u32 = super::fnv("dynamic_response_to_source");
    pub const DYNAMIC_RESPONSE_TO_BUYER_WITH_ITEM: u32 = super::fnv("dynamic_response_to_buyer_with_item");
    pub const DYNAMIC_RESPONSE_TO_BUYER_WITHOUT_ITEM: u32 = super::fnv("dynamic_response_to_buyer_without_item");
    pub const BUYER_COMPLETE_DELIVERY: u32 = super::fnv("buyer_conversation_complete_delivery");
    pub const BUYER_INCOMPLETE_DELIVERY: u32 = super::fnv("buyer_conversation_incomplete_delivery");
    pub const AFTER_MEETING_GROB: u32 = 0xE2F7_711C;
    // event_* and mission_* files
    pub const MISSION_ASSIGN: u32 = 0x8B75_6F4B;
    pub const MISSION_ACCEPT: u32 = 0x1FCE_3DE8;
    pub const MISSION_REJECT: u32 = 0xE2AE_C541;
    pub const MISSION_COMPLETE: u32 = 0x1196_5A4D;
}

/// Response action ids (0xAEB730 / 0x102DF30; names from the brute-forced FNV).
pub mod action {
    use super::fnv;
    pub const NONE: u32 = 0xFFFF_FFFF;
    pub const GOTO: u32 = 0x02D5_DCEC;
    pub const MAIN_MENU: u32 = 0xCAFF_0F72;
    pub const EXIT: u32 = 0x0199_4745;
    pub const BACK_TO_MENU: u32 = 0x2027_BC64;
    pub const GIFT_LARGE: u32 = 0xDAC9_A912;
    pub const GIFT_MEDIUM: u32 = 0xD13E_4CA2;
    pub const GIFT_SMALL: u32 = 0x12D9_113A;
    pub const TRY_CREATE_ALLIANCE: u32 = 0x3148_4F2C;
    pub const BREAK_ALLIANCE: u32 = 0xD950_19BD;
    pub const GET_POSSE: u32 = 0xCA66_7561;
    pub const CREATE_TRADE_ROUTE: u32 = 0x1355_94F5;
    pub const DESTROY_TRADE_ROUTE: u32 = fnv("action_destroy_trade_route");
    pub const REQUEST_ATTACK: u32 = 0xDAFF_3CE0;
    pub const PEACE_OFFER: [u32; 5] = [
        fnv("action_peace_offer_1"),
        fnv("action_peace_offer_2"),
        fnv("action_peace_offer_3"),
        fnv("action_peace_offer_4"),
        fnv("action_peace_offer_5"),
    ];
    pub const PURCHASE_OFFER: [u32; 5] = [0x1229_3374, 0x1229_3377, 0x1229_3376, 0x1229_3371, 0x1229_3370];
    pub const CONTACT_NICE: u32 = 0xAE44_B32E;
    pub const CONTACT_NEUTRAL: u32 = 0x5776_14C6;
    pub const CONTACT_MEAN: u32 = fnv("action_contact_mean");
    pub const TRY_MISSION: u32 = fnv("action_try_mission");
    pub const ACCEPT_GIFT: u32 = fnv("action_accept_gift");
    pub const ACCEPT_ALLIANCE: u32 = 0x6C70_B992;
    pub const DECLINE_ALLIANCE: u32 = 0xC049_E8B5;
    pub const PAY_TRIBUTE: u32 = 0x5C2E_FCB0;
    pub const CANNOT_PAY_TRIBUTE: u32 = 0x4C18_2387;
    pub const WITHHOLD_TRIBUTE: u32 = fnv("action_withold_tribute");
    pub const ACCEPT_SURRENDER: u32 = fnv("action_accept_surrender");
    pub const CONTINUE: u32 = fnv("continue");
    pub const MISSION_ACCEPT: u32 = fnv("action_mission_accept");
    pub const MISSION_REJECT: u32 = fnv("action_mission_reject");
    pub const MISSION_COMPLETE: u32 = fnv("action_mission_complete");
    pub const MISSION_ABORT: u32 = fnv("action_mission_abort");
    pub const MISSION_DELIVERY: u32 = fnv("action_mission_delivery");
    // The multi-step missions (`mission_scan_fetch`) use their own ids for the dialogs that
    // accept / complete / fail a step (they have no registered names).
    pub const STEP_ACCEPT: u32 = 0xA624_E3FA;
    pub const STEP_COMPLETE: u32 = 0x07D5_647D;
    pub const STEP_FAILED: u32 = 0xACC5_0317;
    pub const DECLINE_SURRENDER: u32 = fnv("action_decline_surrender");
}

/// The 154 conversation flags (SporeModder `reg_cnv.txt`, set by 0xAED680).
#[allow(dead_code)]
pub mod flag {
    pub const GROB_ENCOUNTER: usize = 0;
    pub const MY_SPECIES: usize = 1;
    pub const HOMEWORLD: usize = 2;
    pub const COLONY: usize = 3;
    pub const COLLECTIBLE_SPICE: usize = 4;
    pub const ALLIANCE: usize = 5;
    pub const AT_WAR: usize = 6;
    pub const UFO_DAMAGED: usize = 7;
    pub const UNDER_ATTACK: usize = 8;
    pub const MILITARISTIC: usize = 9;
    pub const CULTURAL: usize = 10;
    pub const DIPLOMATIC: usize = 11;
    pub const EXPLORATIVE: usize = 12;
    pub const SPORTY: usize = 13;
    pub const TRADE: usize = 14;
    pub const GROB_GOAL: usize = 15;
    pub const WARRIOR: usize = 16;
    pub const TRADER: usize = 17;
    pub const SCIENTIST: usize = 18;
    pub const SHAMAN: usize = 19;
    pub const BARD: usize = 20;
    pub const ZEALOT: usize = 21;
    pub const DIPLOMAT: usize = 22;
    pub const ECOLOGIST: usize = 23;
    pub const GROB: usize = 24;
    pub const PLAYER_SAVE_EMPIRE: usize = 35;
    pub const GROB_SHOCK: usize = 36;
    pub const OPEN_TO_TRADES: usize = 37;
    pub const LARGE_GIFT_MONEY: usize = 38;
    pub const MEDIUM_GIFT_MONEY: usize = 39;
    pub const SMALL_GIFT_MONEY: usize = 40;
    pub const FIRST_CONTACT: usize = 44;
    pub const HAS_MISSION_CONVERSATION: usize = 50;
    pub const HAS_ASSIGNED_MISSION: usize = 51;
    /// `Step1` .. `Step5` of a multi-step mission.
    pub const STEP1: usize = 58;
    pub const ARCHETYPE_MISSION_AVAILABLE: usize = 54;
    pub const ATTITUDE_FURIOUS: usize = 63;
    pub const ATTITUDE_ANNOYED: usize = 64;
    pub const ATTITUDE_CAUTIOUS: usize = 65;
    pub const ATTITUDE_PLEASED: usize = 66;
    pub const ATTITUDE_FRIENDLY: usize = 67;
    pub const ANNOYED_OR_BETTER: usize = 68;
    pub const CAUTIOUS_OR_BETTER: usize = 69;
    pub const PLEASED_OR_BETTER: usize = 70;
    pub const ANNOYED_OR_WORSE: usize = 71;
    pub const CAUTIOUS_OR_WORSE: usize = 72;
    pub const PLEASED_OR_WORSE: usize = 73;
    pub const HAS_POSSE: usize = 112;
    pub const CAN_HAVE_POSSE: usize = 113;
    pub const HAS_TRADE_ROUTE: usize = 114;
    pub const TRADE_CAPTURE_COMPLETE: usize = 115;
    pub const PURCHASE_OFFER_MONEY_1: usize = 116;
    pub const PEACE_OFFER_MONEY_1: usize = 121;
    pub const TRIBUTE_MONEY: usize = 126;
    pub const HAS_TALKED_TO_ALIENS: usize = 127;
    pub const MASTER_BADGE_LEVEL_0: usize = 128;
    pub const ENTERED_GALACTIC_CORE: usize = 139;
    pub const LARGE_EMPIRE: usize = 148;
    pub const MEDIUM_EMPIRE: usize = 149;
    pub const SMALL_EMPIRE: usize = 150;
}

pub const FLAG_COUNT: usize = 154;

/// A set of conversation flags (eastl::bitset<154>: five 32-bit words).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags(pub [u32; 5]);

#[allow(dead_code)]
impl Flags {
    pub fn set(&mut self, i: usize) {
        if i < FLAG_COUNT {
            self.0[i >> 5] |= 1 << (i & 31);
        }
    }
    pub fn has(&self, i: usize) -> bool {
        i < FLAG_COUNT && self.0[i >> 5] & (1 << (i & 31)) != 0
    }
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|w| *w == 0)
    }
    /// Every flag of `require` is set and none of `exclude` (0xAEE210).
    pub fn passes(&self, require: &Flags, exclude: &Flags) -> bool {
        (0..5).all(|w| self.0[w] & exclude.0[w] == 0 && self.0[w] & require.0[w] == require.0[w])
    }
    pub fn from_list(list: &[usize]) -> Self {
        let mut f = Flags::default();
        for &i in list {
            f.set(i);
        }
        f
    }
}

// ---------------------------------------------------------------------------
// The file format.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CnvText {
    pub require: Flags,
    pub exclude: Flags,
    /// Locale string id and table (the table is the file's own instance).
    pub id: u32,
    pub table: u32,
    /// Animation (emotion) of the speaker.
    pub emotion: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CnvResponse {
    pub require: Flags,
    pub exclude: Flags,
    pub enable_require: Flags,
    pub enable_exclude: Flags,
    pub action: u32,
    /// group, type, instance (goto: file and dialog).
    pub key: (u32, u32, u32),
    pub texts: Vec<CnvText>,
    /// Badges / tools the response requires (`ints1` / `ints2`, 0xAED500).
    pub ints: [Vec<u32>; 4],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CnvDialog {
    pub id: u32,
    pub name: String,
    /// value1, showAccept, showDecline, value4, showStatic.
    pub flags: [bool; 5],
    pub action: u32,
    pub key: (u32, u32, u32),
    pub texts: Vec<CnvText>,
    pub responses: Vec<CnvResponse>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversation {
    pub dialogs: HashMap<u32, CnvDialog>,
    pub logs: HashMap<u32, Vec<CnvText>>,
    /// The animation table (emotion id -> variants by speaker); only the
    /// `animations` file fills it (`ANIMATIONS_FILE`).
    pub anims: HashMap<u32, Vec<CnvAnim>>,
}

/// One speaker variant of an emotion: its idle and action animation ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CnvAnim {
    pub name: String,
    pub idle: Vec<u32>,
    pub action: Vec<u32>,
}

/// `space_conversations~!animations` (0x055ADA23!0x7C6DC224): the 8 emotions
/// of the texts (Afraid, Laugh, Sad, Neutral 0x27F8B66A, Gasp, Urgent, Angry,
/// Happy) x 12 speaker variants (VERIFIED data, space-endgame notes E5).
pub const ANIMATIONS_FILE: u32 = 0x7C6D_C224;
pub const EMOTION_NEUTRAL: u32 = 0x27F8_B66A;

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.p.checked_add(n).filter(|e| *e <= self.b.len()).ok_or("cnv: truncated")?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn le(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn be(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = self.le()? as usize;
        if n > 100_000 {
            return Err("cnv: bad count".into());
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.be()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn flags(&mut self) -> Result<Flags, String> {
        let mut f = Flags::default();
        for (i, b) in self.take(FLAG_COUNT)?.iter().enumerate() {
            if *b != 0 {
                f.set(i);
            }
        }
        Ok(f)
    }
    fn key(&mut self) -> Result<(u32, u32, u32), String> {
        Ok((self.be()?, self.be()?, self.be()?))
    }
    fn text(&mut self) -> Result<CnvText, String> {
        Ok(CnvText {
            require: self.flags()?,
            exclude: self.flags()?,
            id: self.be()?,
            table: self.be()?,
            emotion: self.be()?,
        })
    }
    fn texts(&mut self) -> Result<Vec<CnvText>, String> {
        (0..self.count()?).map(|_| self.text()).collect()
    }
    fn ints(&mut self) -> Result<Vec<u32>, String> {
        (0..self.count()?).map(|_| self.be()).collect()
    }
}

/// Parse a `.cnv` file (animations, dialogs, event logs, mission records).
pub fn parse_cnv(bytes: &[u8]) -> Result<Conversation, String> {
    let mut r = Reader { b: bytes, p: 0 };
    let mut out = Conversation::default();
    for _ in 0..r.count()? {
        let id = r.le()?;
        r.string()?;
        let mut variants = Vec::new();
        for _ in 0..r.count()? {
            r.le()?;
            let name = r.string()?;
            let idle = r.ints()?;
            let action = r.ints()?;
            variants.push(CnvAnim { name, idle, action });
        }
        out.anims.insert(id, variants);
    }
    for _ in 0..r.count()? {
        let id = r.le()?;
        let name = r.string()?;
        let mut flags = [false; 5];
        for f in &mut flags {
            *f = r.take(1)?[0] != 0;
        }
        let action = r.le()?;
        let key = r.key()?;
        let texts = r.texts()?;
        let mut responses = Vec::new();
        for _ in 0..r.count()? {
            let require = r.flags()?;
            let exclude = r.flags()?;
            let enable_require = r.flags()?;
            let enable_exclude = r.flags()?;
            let action = r.le()?;
            let key = r.key()?;
            let texts = r.texts()?;
            let ints = [r.ints()?, r.ints()?, r.ints()?, r.ints()?];
            responses.push(CnvResponse { require, exclude, enable_require, enable_exclude, action, key, texts, ints });
        }
        out.dialogs.insert(id, CnvDialog { id, name, flags, action, key, texts, responses });
    }
    for _ in 0..r.count()? {
        let id = r.le()?;
        r.string()?;
        let texts = r.texts()?;
        out.logs.insert(id, texts);
    }
    for _ in 0..r.count()? {
        r.take(12)?;
        r.count().and_then(|n| r.take(n * 4).map(|_| ()))?;
        r.count().and_then(|n| r.take(n * 4).map(|_| ()))?;
    }
    if r.p != bytes.len() {
        return Err(format!("cnv: {} trailing bytes", bytes.len() - r.p));
    }
    Ok(out)
}

/// The candidates of a text list for `flags` (0xAEE210).
pub fn text_candidates<'a>(texts: &'a [CnvText], flags: &Flags) -> Vec<&'a CnvText> {
    texts.iter().filter(|t| flags.passes(&t.require, &t.exclude)).collect()
}

/// One text drawn among the candidates (`RandomInt(n)` with `roll`).
pub fn pick_text<'a>(texts: &'a [CnvText], flags: &Flags, roll: u32) -> Option<&'a CnvText> {
    let c = text_candidates(texts, flags);
    if c.is_empty() {
        None
    } else {
        Some(c[roll as usize % c.len()])
    }
}

/// Visible responses and whether each is enabled (0xAEE440 / 0xAED500).
pub fn visible_responses<'a>(dialog: &'a CnvDialog, flags: &Flags) -> Vec<(&'a CnvResponse, bool)> {
    dialog
        .responses
        .iter()
        .filter(|r| flags.passes(&r.require, &r.exclude))
        .map(|r| (r, flags.passes(&r.enable_require, &r.enable_exclude)))
        .collect()
}

// ---------------------------------------------------------------------------
// Flags from the campaign (0xAED680).
// ---------------------------------------------------------------------------

/// What the flags builder reads (one comm session).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommContext {
    /// The speaking empire (None: the player's own species at its colony).
    pub empire: Option<EmpireId>,
    pub archetype: Option<SpaceArchetype>,
    pub grox: bool,
    pub homeworld: bool,
    pub colony: bool,
    pub relationship: f32,
    pub at_war: bool,
    pub allied: bool,
    pub ufo_damaged: bool,
    pub collectible_spice: bool,
    pub money: f32,
    pub peace_offers: [f32; 5],
    pub tribute: f32,
    pub has_trade_route: bool,
    pub trade_capture_complete: bool,
    pub has_posse: bool,
    pub can_have_posse: bool,
    pub empire_tier: usize,
    pub rank: u8,
    pub reached_core: bool,
    pub first_contact: bool,
    pub talked_to_aliens: bool,
    pub has_assigned_mission: bool,
    pub under_attack: bool,
    /// The step of a multi-step mission (1 = Step1 ..., 0 = none): the conversation
    /// flags Step1 / Step2 of `mission_scan_fetch`.
    pub mission_step: u8,
}

/// Attitude index 0 Furious .. 4 Friendly (0xD00BD0).
pub fn stance(relationship: f32) -> usize {
    match rules::attitude(relationship) {
        rules::Attitude::Furious => 0,
        rules::Attitude::Annoyed => 1,
        rules::Attitude::Cautious => 2,
        rules::Attitude::Pleased => 3,
        rules::Attitude::Friendly => 4,
    }
}

/// The conversation flags of a session (0xAED680).
pub fn space_flags(c: &CommContext) -> Flags {
    use flag::*;
    let mut f = Flags::default();
    if c.mission_step > 0 {
        f.set(STEP1 + usize::from(c.mission_step - 1).min(4));
    }
    if c.ufo_damaged {
        f.set(UFO_DAMAGED);
    }
    if c.collectible_spice {
        f.set(COLLECTIBLE_SPICE);
    }
    let Some(_) = c.empire else {
        // The player's own species (source == player empire).
        f.set(MY_SPECIES);
        if c.homeworld {
            f.set(HOMEWORLD);
        } else if c.colony {
            f.set(COLONY);
        }
        common_flags(c, &mut f);
        return f;
    };
    // Attitude rows (the switch on the stance).
    let attitude: &[usize] = match stance(c.relationship) {
        0 => &[ATTITUDE_FURIOUS, ANNOYED_OR_WORSE, CAUTIOUS_OR_WORSE, PLEASED_OR_WORSE],
        1 => &[ATTITUDE_ANNOYED, ANNOYED_OR_BETTER, ANNOYED_OR_WORSE, CAUTIOUS_OR_WORSE, PLEASED_OR_WORSE],
        2 => &[ATTITUDE_CAUTIOUS, ANNOYED_OR_BETTER, CAUTIOUS_OR_BETTER, CAUTIOUS_OR_WORSE, PLEASED_OR_WORSE],
        3 => &[ATTITUDE_PLEASED, ANNOYED_OR_BETTER, CAUTIOUS_OR_BETTER, PLEASED_OR_BETTER, PLEASED_OR_WORSE],
        _ => &[ATTITUDE_FRIENDLY, ANNOYED_OR_BETTER, CAUTIOUS_OR_BETTER, PLEASED_OR_BETTER],
    };
    for &a in attitude {
        f.set(a);
    }
    if c.at_war {
        f.set(AT_WAR);
    }
    if c.allied {
        f.set(ALLIANCE);
    }
    if c.has_posse {
        f.set(HAS_POSSE);
    }
    if c.can_have_posse {
        f.set(CAN_HAVE_POSSE);
    }
    if c.has_trade_route {
        f.set(HAS_TRADE_ROUTE);
    }
    if c.trade_capture_complete {
        f.set(TRADE_CAPTURE_COMPLETE);
    }
    for (i, price) in rules::SYSTEM_PURCHASE_OFFERS.iter().enumerate() {
        if c.money >= *price {
            f.set(PURCHASE_OFFER_MONEY_1 + i);
        }
    }
    for (i, price) in c.peace_offers.iter().enumerate() {
        if c.money >= *price {
            f.set(PEACE_OFFER_MONEY_1 + i);
        }
    }
    if c.money >= c.tribute {
        f.set(TRIBUTE_MONEY);
    }
    // Archetype flags of the source empire (personality + archetype).
    let pair: [usize; 2] = if c.grox {
        [GROB_GOAL, GROB]
    } else {
        match c.archetype.unwrap_or_default() {
            SpaceArchetype::Warrior | SpaceArchetype::Knight => [MILITARISTIC, WARRIOR],
            SpaceArchetype::Trader => [TRADE, TRADER],
            SpaceArchetype::Scientist => [EXPLORATIVE, SCIENTIST],
            SpaceArchetype::Shaman => [CULTURAL, SHAMAN],
            SpaceArchetype::Bard => [SPORTY, BARD],
            SpaceArchetype::Zealot => [MILITARISTIC, ZEALOT],
            SpaceArchetype::Diplomat | SpaceArchetype::Wanderer => [DIPLOMATIC, DIPLOMAT],
            SpaceArchetype::Ecologist => [EXPLORATIVE, ECOLOGIST],
        }
    };
    f.set(pair[0]);
    f.set(pair[1]);
    // ADAPTED: `OpenToTrades` reads an empire state and a positive relationship.
    if c.relationship > 0.0 {
        f.set(OPEN_TO_TRADES);
    }
    // Empire size class (0xC31900 tier: 0-2 small, 3 medium, 4-5 large).
    f.set(match c.empire_tier {
        0..=2 => SMALL_EMPIRE,
        3 => MEDIUM_EMPIRE,
        _ => LARGE_EMPIRE,
    });
    if c.first_contact {
        f.set(FIRST_CONTACT);
    }
    if c.has_assigned_mission {
        f.set(HAS_ASSIGNED_MISSION);
        f.set(HAS_MISSION_CONVERSATION);
    }
    if c.under_attack {
        f.set(UNDER_ATTACK);
    }
    common_flags(c, &mut f);
    f
}

fn common_flags(c: &CommContext, f: &mut Flags) {
    use flag::*;
    for (i, amount) in [rules::GiftSize::Large, rules::GiftSize::Medium, rules::GiftSize::Small].iter().enumerate() {
        if c.money >= amount.amount() {
            f.set(LARGE_GIFT_MONEY + i);
        }
    }
    // The master badge level is the rank count 0xFE4290 (VERIFIED, space-re-notes S5B: the rank bar's
    // `rank count`), which `captain_rank` reproduces from the badge points.
    f.set(MASTER_BADGE_LEVEL_0 + c.rank.min(10) as usize);
    if c.reached_core {
        f.set(ENTERED_GALACTIC_CORE);
    }
    if c.talked_to_aliens {
        f.set(HAS_TALKED_TO_ALIENS);
    }
}

/// The five peace offers an empire takes (0x1030940): `peaceOfferPrices` x the
/// personality's multiplier for the player's `rank`, rounded down to hundreds.
pub fn peace_offers(rank: usize, difficulty: rules::Difficulty) -> [f32; 5] {
    let m = rules::size_multiplier(rank, difficulty);
    rules::PEACE_OFFER_PRICES.map(|p| ((p * m * 0.01).floor() * 100.0).max(100.0))
}

/// The tribute an empire demands (0x10407E0): `npcTributeAmount` x the
/// multiplier for the player's `rank`, rounded down to hundreds.
pub fn tribute_amount(rank: usize, difficulty: rules::Difficulty) -> f32 {
    ((rules::NPC_TRIBUTE_AMOUNT * rules::size_multiplier(rank, difficulty) * 0.01).floor() * 100.0).max(100.0)
}

/// The comm context of `empire` (or the player's world when `None`).
pub fn context_for(campaign: &SpaceCampaign, money: f32, empire: Option<EmpireId>, world: u32, ufo_damaged: bool) -> CommContext {
    let difficulty = campaign.difficulty();
    let mut c = CommContext {
        empire,
        ufo_damaged,
        money,
        rank: campaign.captain_rank(),
        reached_core: campaign.reached_core,
        talked_to_aliens: !campaign.universe.contacted.is_empty(),
        has_posse: !campaign.fleet.is_empty(),
        can_have_posse: campaign.fleet.len() < campaign.max_fleet_size(),
        ..Default::default()
    };
    match empire {
        None => {
            c.homeworld = world == 0;
            c.colony = world != 0;
            c.collectible_spice = campaign.colony_spice_stored(world) > 0;
        }
        Some(e) => {
            let tier = rules::empire_tier(campaign.empire_size(e));
            c.grox = e == 0;
            c.archetype = Some(empire_archetype(e));
            c.relationship = campaign.relationship(e);
            c.at_war = campaign.wars.contains(&e);
            c.allied = campaign.allies.contains(&e);
            c.empire_tier = tier;
            // Prices scale with the player's rank (0x10307B0), not the empire's size.
            let rank = usize::from(campaign.captain_rank());
            c.peace_offers = peace_offers(rank, difficulty);
            c.tribute = campaign
                .tribute_demand
                .filter(|d| d.empire == e)
                .map_or_else(|| tribute_amount(rank, difficulty), |d| d.amount);
            let system = campaign.current_system;
            let routes: Vec<f32> = campaign
                .trade_routes
                .iter()
                .filter(|(w, _)| crate::space_galaxy::system_of_world(**w) == system)
                .map(|(_, p)| *p)
                .collect();
            c.has_trade_route = !routes.is_empty();
            c.trade_capture_complete = routes.iter().any(|p| *p >= 1.0);
            c.first_contact = !campaign.universe.contacted.contains(&e);
            c.has_assigned_mission = campaign.active_alien_mission.as_ref().is_some_and(|m| m.empire == e);
            c.mission_step = campaign.active_alien_mission.as_ref().map_or(0, mission_step);
            c.under_attack = campaign.universe.attacks.iter().any(|a| a.ally == Some(e));
        }
    }
    c
}

// ---------------------------------------------------------------------------
// Text tokens.
// ---------------------------------------------------------------------------

/// Values of the `~token~` substitutions of one session.
#[derive(Clone, Debug, Default)]
pub struct Tokens {
    pub player_title: String,
    pub player_name: String,
    pub player_homeworld: String,
    pub race: String,
    pub race_homeworld: String,
    pub star_name: String,
    pub planet_name: String,
    pub cost: f32,
    pub peace_offers: [f32; 5],
    /// The empire raiding (`~mission_attacker_empire~` of the event files).
    pub attacker: String,
    /// Mission tokens (`mission_rewards`, `mission_time`, ...).
    pub extra: Vec<(&'static str, String)>,
}

pub fn money_text(amount: f32) -> String {
    crate::space_hud::format_money(amount.round() as i64)
}

/// Replace the `~token~` markers (`~p~` is a paragraph break).
pub fn substitute(text: &str, t: &Tokens) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut rest = text;
    while let Some(start) = rest.find('~') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('~') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after[..end];
        let value = match token {
            "p" => "\n\n".to_string(),
            // `~^p^name^one^many~` (the plural form of the mission strings, e.g.
            // "~^p^mission_num_kills_needed^animal^animals~"): the first word
            // when the token `name` is 1, the second otherwise.
            s if s.starts_with("^p^") => {
                let mut parts = s[3..].split('^');
                let (name, one, many) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let count = t.extra.iter().find(|(k, _)| *k == name).and_then(|(_, v)| v.trim().parse::<i64>().ok());
                if count == Some(1) { one.to_string() } else { many.to_string() }
            }
            "player_title" => t.player_title.clone(),
            "player_creature_name" | "player_species" => t.player_name.clone(),
            "player_homeworld" => t.player_homeworld.clone(),
            "race" => t.race.clone(),
            "race_homeworld" => t.race_homeworld.clone(),
            "star_name" => t.star_name.clone(),
            "planet_name" => t.planet_name.clone(),
            // The mission's target (`mission_tokens`), else the session's own star / planet.
            "mission_target_star" => t
                .extra
                .iter()
                .find(|(k, _)| *k == "mission_target_star")
                .map_or_else(|| t.star_name.clone(), |(_, v)| v.clone()),
            "mission_target_planet" => t
                .extra
                .iter()
                .find(|(k, _)| *k == "mission_target_planet")
                .map_or_else(|| t.planet_name.clone(), |(_, v)| v.clone()),
            "ufo_name" => "UFO".into(),
            "mission_attacker_empire" => t.attacker.clone(),
            "cost" | "attack_cost" => money_text(t.cost),
            "small_gift_amount" => money_text(rules::GiftSize::Small.amount()),
            "medium_gift_amount" => money_text(rules::GiftSize::Medium.amount()),
            "large_gift_amount" => money_text(rules::GiftSize::Large.amount()),
            s if s.starts_with("peace_offer_") => {
                let i = s[12..].parse::<usize>().unwrap_or(1).clamp(1, 5) - 1;
                money_text(t.peace_offers[i])
            }
            s if s.starts_with("purchase_offer_") => {
                let i = s[15..].parse::<usize>().unwrap_or(1).clamp(1, 5) - 1;
                money_text(rules::SYSTEM_PURCHASE_OFFERS[i])
            }
            s => t.extra.iter().find(|(k, _)| *k == s).map(|(_, v)| v.clone()).unwrap_or_default(),
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// The conversation library (installed files + their locale tables).
// ---------------------------------------------------------------------------

/// Conversations and strings read from the install on first use.
#[derive(Resource, Default)]
pub struct CnvLibrary {
    convs: HashMap<u32, Option<Arc<Conversation>>>,
    strings: HashMap<(u32, u32), String>,
    locale: crate::cell_editor_data::Locale,
}

impl CnvLibrary {
    /// A library with given conversations and strings (tests).
    #[allow(dead_code)]
    pub fn with(convs: Vec<(u32, Conversation)>, strings: Vec<((u32, u32), String)>) -> Self {
        Self {
            convs: convs.into_iter().map(|(k, v)| (k, Some(Arc::new(v)))).collect(),
            strings: strings.into_iter().collect(),
            locale: Default::default(),
        }
    }

    pub fn conversation(&mut self, index: Option<&crate::asset_catalog::ResourceIndex>, file: u32) -> Option<Arc<Conversation>> {
        if let Some(c) = self.convs.get(&file) {
            return c.clone();
        }
        let index = index?;
        let key = crate::dbpf::ResourceKey { group_id: CNV_GROUP, instance_id: file, type_id: CNV_TYPE };
        let parsed = index.read(key).ok().and_then(|b| parse_cnv(&b).ok()).map(Arc::new);
        self.convs.insert(file, parsed.clone());
        parsed
    }

    pub fn string(&mut self, index: Option<&crate::asset_catalog::ResourceIndex>, table: u32, id: u32) -> String {
        if let Some(s) = self.strings.get(&(table, id)) {
            return s.clone();
        }
        let s = index.and_then(|i| self.locale.get(i, table, id)).unwrap_or_default();
        self.strings.insert((table, id), s.clone());
        s
    }
}

// ---------------------------------------------------------------------------
// The session.
// ---------------------------------------------------------------------------

/// One response button of the shown dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownResponse {
    pub label: String,
    pub action: u32,
    pub key: (u32, u32, u32),
    pub enabled: bool,
}

/// Which side of the Trade tab is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TradeSide {
    #[default]
    Buy,
    Sell,
}

/// The Trade tab (trading controls 0x05E62A48).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeView {
    pub stock: Vec<StockItem>,
    pub side: TradeSide,
    pub selected: usize,
    pub count: u32,
    pub message: String,
    /// The labels carry the retail tool names (set by the HUD from the tool
    /// property files).
    pub named: bool,
}

/// An item of an NPC store (Trade tab) or of the player's hold.
#[derive(Clone, Debug, PartialEq)]
pub struct StockItem {
    pub id: String,
    pub tool: String,
    pub label: String,
    pub price: f32,
    pub consumable: bool,
    /// Why the player cannot buy it yet (shown greyed out), if locked.
    pub locked: Option<String>,
}

/// What the comm screen shows.
#[derive(Resource, Clone, Debug, Default)]
pub struct CommSession {
    pub open: bool,
    pub empire: Option<EmpireId>,
    pub world: u32,
    pub file: u32,
    pub dialog: u32,
    pub text: String,
    pub emotion: u32,
    pub responses: Vec<ShownResponse>,
    pub show_tabs: bool,
    pub show_accept: bool,
    pub show_decline: bool,
    pub trade: Option<TradeView>,
    /// A mission offered by `action_try_mission`, waiting for Accept / Decline.
    pub offered_mission: Option<AlienMission>,
    /// The incoming message (inbox index) this session answers.
    pub incoming: Option<crate::space_universe::Incoming>,
    pub tokens: Tokens,
    pub roll: u32,
    /// A UI sound to play (taken by the presentation).
    pub sound: Option<crate::game_audio::Cue>,
}

/// Inputs of a session step that are not in the campaign.
pub struct CommEnv<'a> {
    pub index: Option<&'a crate::asset_catalog::ResourceIndex>,
    pub ufo_damaged: bool,
}

impl CommSession {
    fn next_roll(&mut self) -> u32 {
        // LCG (deterministic per session; RandomInt of the retail RNG).
        self.roll = self.roll.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.roll >> 8
    }

    /// Open the screen with `empire` at `world` (None: the player's colony).
    pub fn open_with(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv, empire: Option<EmpireId>, world: u32, seed: u32) {
        *self = CommSession { open: true, empire, world, roll: seed ^ 0x9E37_79B9, ..Default::default() };
        self.tokens = tokens_for(run, empire, world);
        // An incoming message from this empire is answered first.
        let pending = empire.and_then(|e| {
            run.space_campaign.universe.inbox.iter().position(|m| m.empire == Some(e))
        });
        if let Some(i) = pending {
            let msg = run.space_campaign.universe.inbox.remove(i);
            self.tokens.cost = msg.amount;
            self.show(lib, run, env, msg.file, msg.dialog);
            self.incoming = Some(msg);
            return;
        }
        // A finished contract of this empire: its mission conversation's
        // `mission_complete` (its own action pays).
        if let Some(m) = run.space_campaign.active_alien_mission.clone() {
            if Some(m.empire) == empire && m.is_complete(&run.space_campaign) {
                if let Some(file) = m.retail.map(mission_file) {
                    if lib.conversation(env.index, file).is_some_and(|c| c.dialogs.contains_key(&dialog::MISSION_COMPLETE)) {
                        self.tokens.extra = mission_tokens(&m, &self.tokens.planet_name, &self.tokens.star_name);
                        self.show(lib, run, env, file, dialog::MISSION_COMPLETE);
                        return;
                    }
                }
            }
        }
        // A running multi-delivery mission answers in its own conversation
        // (`func110h` 0xC57E90): at the buyer's planet with or without the items,
        // at the giver as `dynamic_response_to_source`.
        if let Some(m) = run.space_campaign.active_alien_mission.clone() {
            if m.kind == crate::space::AlienMissionKind::MultiDelivery && !m.is_complete(&run.space_campaign) {
                let d = if m.extra.buyer_world == Some(world) {
                    Some(if m.extra.carried > 0 { dialog::DYNAMIC_RESPONSE_TO_BUYER_WITH_ITEM } else { dialog::DYNAMIC_RESPONSE_TO_BUYER_WITHOUT_ITEM })
                } else if Some(m.empire) == empire {
                    Some(dialog::DYNAMIC_RESPONSE_TO_SOURCE)
                } else {
                    None
                };
                if let (Some(file), Some(d)) = (m.retail.map(mission_file), d) {
                    if lib.conversation(env.index, file).is_some_and(|c| c.dialogs.contains_key(&d)) {
                        self.tokens.extra = mission_tokens(&m, &self.tokens.planet_name, &self.tokens.star_name);
                        self.show(lib, run, env, file, d);
                        return;
                    }
                }
            }
        }
        let start = greeting_for(&run.space_campaign, empire, world);
        note_greeting(&mut run.space_campaign, start, empire, world);
        self.show(lib, run, env, MAIN_MENU_FILE, start);
    }

    /// Open an incoming transmission (the comm button while it glows).
    pub fn open_incoming(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv, msg: crate::space_universe::Incoming, seed: u32) {
        *self = CommSession { open: true, empire: msg.empire, world: msg.world, roll: seed ^ 0x51ED_270B, ..Default::default() };
        self.tokens = tokens_for(run, msg.empire, msg.world);
        self.tokens.cost = msg.amount;
        self.tokens.attacker = attacker_name(&run.space_campaign, msg.world);
        self.show(lib, run, env, msg.file, msg.dialog);
        self.incoming = Some(msg);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.trade = None;
        self.offered_mission = None;
        self.incoming = None;
    }

    /// Show a dialog: its text, its responses, the tabs on the main menu.
    pub fn show(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv, file: u32, id: u32) {
        let Some(conv) = lib.conversation(env.index, file) else {
            self.close();
            return;
        };
        let Some(d) = conv.dialogs.get(&id) else {
            self.close();
            return;
        };
        let money = run.spice;
        let mut ctx = context_for(&run.space_campaign, money, self.empire, self.world, env.ufo_damaged);
        if let Some(m) = &self.offered_mission {
            ctx.mission_step = mission_step(m);
        }
        let flags = space_flags(&ctx);
        self.file = file;
        self.dialog = id;
        self.trade = None;
        self.tokens.peace_offers = ctx.peace_offers;
        if self.tokens.cost == 0.0 {
            self.tokens.cost = ctx.tribute;
        }
        let roll = self.next_roll();
        let (text, emotion) = match pick_text(&d.texts, &flags, roll) {
            Some(t) => (lib.string(env.index, t.table, t.id), t.emotion),
            None => (String::new(), 0x27F8_B66A),
        };
        self.text = substitute(&text, &self.tokens);
        self.emotion = emotion;
        self.responses.clear();
        for (r, enabled) in visible_responses(d, &flags) {
            let roll = self.next_roll();
            let label = pick_text(&r.texts, &flags, roll)
                .map(|t| lib.string(env.index, t.table, t.id))
                .unwrap_or_default();
            self.responses.push(ShownResponse {
                label: substitute(&label, &self.tokens),
                action: r.action,
                key: r.key,
                enabled,
            });
        }
        self.show_tabs = file == MAIN_MENU_FILE && id == dialog::DEFAULT;
        self.show_accept = d.flags[1];
        self.show_decline = d.flags[2];
        // A dialog's own action runs when it is shown (ShowCommEvent).
        if d.action != action::NONE && d.action != 0 {
            let key = d.key;
            self.apply(lib, run, env, d.action, key);
        }
    }

    /// Click on response `i`.
    pub fn choose(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv, i: usize) {
        let Some(r) = self.responses.get(i).cloned() else {
            return;
        };
        if !r.enabled {
            self.sound = Some(crate::game_audio::Cue::SpgError);
            return;
        }
        self.sound = Some(crate::game_audio::Cue::UfoComSelect);
        self.apply(lib, run, env, r.action, r.key);
    }

    /// The tabs of the main menu.
    pub fn tab_missions(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv) {
        self.sound = Some(crate::game_audio::Cue::UfoComSwap);
        self.show(lib, run, env, MAIN_MENU_FILE, dialog::MISSION_MAIN);
    }

    pub fn tab_diplomacy(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv) {
        self.sound = Some(crate::game_audio::Cue::UfoComSwap);
        self.show(lib, run, env, MAIN_MENU_FILE, dialog::DIPLOMACY);
    }

    pub fn tab_trade(&mut self, run: &RunProgress) {
        self.sound = Some(crate::game_audio::Cue::UfoComSwap);
        let stock = match self.empire {
            Some(e) => npc_stock(&run.space_campaign, e),
            None => Vec::new(),
        };
        self.trade = Some(TradeView { stock, count: 1, ..Default::default() });
    }

    /// Accept / Decline (an offered mission).
    pub fn accept(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv) {
        if let Some(m) = self.offered_mission.take() {
            let empire = m.empire;
            run.space_campaign.active_alien_mission = Some(m);
            run.space_campaign.apply_relationship(empire, RelationshipEvent::MissionStarted, 1.0);
            run.request_save();
            self.sound = Some(crate::game_audio::Cue::UfoComSelect);
            self.show(lib, run, env, MAIN_MENU_FILE, dialog::DEFAULT);
        }
    }

    pub fn decline(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv) {
        if let Some(m) = self.offered_mission.take() {
            // `missionRejectPunishmentRelation` (-0.5); easy has no penalty.
            if run.space_campaign.difficulty() != rules::Difficulty::Easy {
                run.space_campaign.apply_relationship(m.empire, RelationshipEvent::MissionRejected, 0.5);
            }
            self.show(lib, run, env, MAIN_MENU_FILE, dialog::DEFAULT);
        } else {
            self.close();
        }
    }

    /// Apply a response action (0xAEB730 -> 0x102DF30).
    pub fn apply(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv, id: u32, key: (u32, u32, u32)) {
        // Mission conversation actions (they run as the dialog's own action
        // when `mission_accept` / `_reject` / `_complete` is shown).
        match id {
            // A goto without a file stays in the shown file.
            action::GOTO => {
                let file = if key.0 == 0 { self.file } else { key.0 };
                return self.show(lib, run, env, file, key.2);
            }
            action::CONTINUE => return self.show(lib, run, env, MAIN_MENU_FILE, dialog::DEFAULT),
            action::MISSION_ACCEPT | action::STEP_ACCEPT => {
                if let Some(m) = self.offered_mission.take() {
                    let empire = m.empire;
                    run.space_campaign.active_alien_mission = Some(m);
                    run.space_campaign.apply_relationship(empire, RelationshipEvent::MissionStarted, 1.0);
                    run.request_save();
                }
                return;
            }
            action::MISSION_REJECT => {
                if let Some(m) = self.offered_mission.take() {
                    if run.space_campaign.difficulty() != rules::Difficulty::Easy {
                        run.space_campaign.apply_relationship(m.empire, RelationshipEvent::MissionRejected, 0.5);
                    }
                }
                return;
            }
            action::STEP_FAILED => return,
            action::MISSION_COMPLETE | action::STEP_COMPLETE => {
                // The campaign's turn-in pays the reward and the relationship.
                let mut campaign = std::mem::take(&mut run.space_campaign);
                campaign.try_communicate(self.world, run);
                run.space_campaign = campaign;
                return;
            }
            // The buyer takes the items carried (`try_multi_delivery`), then answers
            // with the complete / incomplete delivery dialog.
            action::MISSION_DELIVERY => {
                let mut campaign = std::mem::take(&mut run.space_campaign);
                campaign.try_multi_delivery(self.world, run);
                let mission = campaign.active_alien_mission.clone();
                let complete = mission.as_ref().is_some_and(|m| m.is_complete(&campaign));
                run.space_campaign = campaign;
                if let Some(m) = mission {
                    if let Some(file) = m.retail.map(mission_file) {
                        self.tokens.extra = mission_tokens(&m, &self.tokens.planet_name, &self.tokens.star_name);
                        let d = if complete { dialog::BUYER_COMPLETE_DELIVERY } else { dialog::BUYER_INCOMPLETE_DELIVERY };
                        return self.show(lib, run, env, file, d);
                    }
                }
                return;
            }
            action::MISSION_ABORT => {
                if let Some(m) = run.space_campaign.active_alien_mission.take() {
                    // `missionAbortPunishmentRelation` -1.2; easy and medium none.
                    if run.space_campaign.difficulty() == rules::Difficulty::Hard {
                        run.space_campaign.apply_relationship(m.empire, RelationshipEvent::MissionAborted, 1.2);
                    }
                    run.request_save();
                }
                return;
            }
            _ => {}
        }
        let next = apply_action(&mut run.space_campaign, &mut run.spice, self.empire, self.world, id, key);
        if next != Next::Stay {
            run.request_save();
        }
        match next {
            Next::Show(file, d) => self.show(lib, run, env, file, d),
            Next::Close => self.close(),
            Next::Stay => {}
            Next::Mission => {
                let campaign = &run.space_campaign;
                match self.empire {
                    Some(e) if campaign.active_alien_mission.is_none() && rules::offers_missions(campaign.relationship(e)) => {
                        let m = campaign.generate_alien_mission(e);
                        self.offer(lib, run, env, m);
                    }
                    _ => self.show(lib, run, env, MAIN_MENU_FILE, dialog::MISSION_UNAVAILABLE),
                }
            }
        }
    }

    /// Offer the mission `m`: the mission kind's own conversation shows its
    /// `mission_assign` dialog (Accept / Decline start or refuse it).
    pub fn offer(&mut self, lib: &mut CnvLibrary, run: &mut RunProgress, env: &CommEnv, m: AlienMission) {
        if let Some(file) = m.retail.map(mission_file) {
            if lib.conversation(env.index, file).is_some_and(|c| c.dialogs.contains_key(&dialog::MISSION_ASSIGN)) {
                self.tokens.extra = mission_tokens(&m, &self.tokens.planet_name, &self.tokens.star_name);
                self.offered_mission = Some(m);
                self.show(lib, run, env, file, dialog::MISSION_ASSIGN);
                return;
            }
        }
        let time = m.time_left.map(|t| format!(" ({t:.0} s)")).unwrap_or_default();
        self.text = format!("{}\n\nReward: {}{time}", m.description, money_text(m.reward_spice));
        self.responses.clear();
        self.show_tabs = false;
        self.show_accept = true;
        self.show_decline = true;
        self.offered_mission = Some(m);
    }

    /// Trade tab: select an item of the NPC stock (Buy) or of the hold (Sell).
    pub fn trade_select(&mut self, side: TradeSide, i: usize) {
        if let Some(t) = self.trade.as_mut() {
            t.side = side;
            t.selected = i;
            t.count = 1;
            self.sound = Some(crate::game_audio::Cue::UfoComSelect);
        }
    }

    /// Add / remove one or many (10) of the selected item.
    pub fn trade_count(&mut self, delta: i32, run: &RunProgress) {
        let max = self.trade_max(run);
        if let Some(t) = self.trade.as_mut() {
            t.count = (t.count as i32 + delta).clamp(1, max.max(1) as i32) as u32;
            self.sound = Some(crate::game_audio::Cue::UfoComTradeIncrement);
        }
    }

    fn trade_max(&self, run: &RunProgress) -> u32 {
        let Some(t) = self.trade.as_ref() else {
            return 1;
        };
        match t.side {
            TradeSide::Buy => t.stock.get(t.selected).map_or(1, |i| if i.consumable { 99 } else { 1 }),
            TradeSide::Sell => sell_list(&run.space_campaign).get(t.selected).map_or(1, |s| s.1),
        }
    }

    /// Buy the selected stock item `count` times.
    pub fn trade_buy(&mut self, run: &mut RunProgress) {
        let Some(t) = self.trade.as_mut() else {
            return;
        };
        let Some(item) = t.stock.get(t.selected).cloned() else {
            return;
        };
        let count = if item.consumable { t.count.max(1) } else { 1 };
        let total = item.price * count as f32;
        let campaign = &mut run.space_campaign;
        if let Some(reason) = item.locked.as_ref() {
            t.message = format!("{}: {reason}.", item.label);
            self.sound = Some(crate::game_audio::Cue::SpgError);
            return;
        }
        if !item.consumable && campaign.has_tool(&item.tool) {
            t.message = format!("You already own the {}.", item.label);
            self.sound = Some(crate::game_audio::Cue::SpgError);
            return;
        }
        if run.spice < total {
            t.message = format!("Not enough Sporebucks: {} needed.", money_text(total));
            self.sound = Some(crate::game_audio::Cue::SpgError);
            return;
        }
        run.spice -= total;
        // `ToolsPurchased` (0x103F000: every unit bought).
        campaign.endgame.tools_purchased += count;
        if item.consumable {
            campaign.add_charges(&item.tool, count);
        } else {
            campaign.grant_tool(&item.tool);
        }
        t.message = format!("Bought {count} x {} for {}.", item.label, money_text(total));
        if let Some(e) = self.empire {
            campaign.apply_relationship(e, RelationshipEvent::TradeComplete, 1.0);
        }
        // Upgrades leave the store once owned; their next level appears.
        if let Some(e) = self.empire {
            t.stock = npc_stock(campaign, e);
            t.named = false;
            t.selected = t.selected.min(t.stock.len().saturating_sub(1));
        }
        run.request_save();
        self.sound = Some(crate::game_audio::Cue::UfoComTradeBuy);
    }

    /// Sell `count` of the selected hold item (spice) to the empire.
    pub fn trade_sell(&mut self, run: &mut RunProgress) {
        let Some(empire) = self.empire else {
            return;
        };
        let Some(t) = self.trade.as_mut() else {
            return;
        };
        let list = sell_list(&run.space_campaign);
        let Some((color, held, price)) = list.get(t.selected).copied() else {
            t.message = "Your cargo hold has nothing to sell.".into();
            self.sound = Some(crate::game_audio::Cue::SpgError);
            return;
        };
        let count = t.count.clamp(1, held).min(rules::NPC_MAX_SPICE_BOUGHT);
        let campaign = &mut run.space_campaign;
        let left = held - count;
        if left == 0 {
            campaign.cargo_spice.remove(&color);
        } else {
            campaign.cargo_spice.insert(color, left as u8);
        }
        let revenue = price * count as f32;
        run.spice = (run.spice + revenue).min(WALLET_CAP);
        campaign.badge_stats.trade_volume += revenue as f64;
        campaign.apply_relationship(empire, RelationshipEvent::TradeComplete, 1.0);
        t.message = format!("Sold {count} {} spice for {}.", color.label(), money_text(revenue));
        t.count = 1;
        // A sale is the Tradespice teaching mission's goal.
        let mut campaign = std::mem::take(&mut run.space_campaign);
        campaign.advance_tutorial(rules::TutorialStep::TeachTradeSpice, run);
        run.space_campaign = campaign;
        run.request_save();
        self.sound = Some(crate::game_audio::Cue::UfoComTradeSell);
    }
}

/// The player's sellable cargo: (spice colour, units, price per unit).
pub fn sell_list(campaign: &SpaceCampaign) -> Vec<(crate::space::SpiceColor, u32, f32)> {
    let difficulty = campaign.difficulty();
    crate::space::SpiceColor::ALL
        .iter()
        .filter_map(|c| {
            let held = campaign.cargo_spice.get(c).copied().unwrap_or(0) as u32;
            (held > 0).then(|| (*c, held, c.sell_price(difficulty)))
        })
        .collect()
}

/// The empire raiding `world`, for `~mission_attacker_empire~`.
pub fn attacker_name(campaign: &SpaceCampaign, world: u32) -> String {
    campaign
        .universe
        .attacks
        .iter()
        .find(|a| a.world == world)
        .and_then(|a| a.empire)
        .map_or_else(|| "An unknown assailant".to_string(), |e| format!("The {}", SpaceCampaign::empire_name(e)))
}

/// The token values of a session.
pub fn tokens_for(run: &RunProgress, empire: Option<EmpireId>, world: u32) -> Tokens {
    let campaign = &run.space_campaign;
    let name = if run.creature_creation.name.trim().is_empty() {
        "Captain".to_string()
    } else {
        run.creature_creation.name.trim().to_string()
    };
    let star = crate::space_galaxy::registered_galaxy()
        .map(|g| g.star_name(campaign.current_system))
        .unwrap_or_else(|| "this".into());
    let planet = crate::space_galaxy::world_info(world).map(|w| w.name).unwrap_or_else(|| star.clone());
    let race = empire.map_or_else(|| name.clone(), |e| SpaceCampaign::empire_name(e).to_string());
    Tokens {
        player_title: format!("{} {name}", rules::rank_title(campaign.captain_rank())),
        player_name: name,
        player_homeworld: crate::space_galaxy::world_info(0).map(|w| w.name).unwrap_or_else(|| "home".into()),
        race_homeworld: planet.clone(),
        race,
        star_name: star,
        planet_name: planet,
        cost: 0.0,
        peace_offers: [0.0; 5],
        attacker: String::new(),
        extra: Vec::new(),
    }
}

/// The conversation of a mission kind (`mission_<kind>.cnv`).
pub fn mission_file(kind: rules::MissionKind) -> u32 {
    use rules::MissionKind as K;
    fnv(match kind {
        K::FetchArtifact => "mission_fetch_artifact",
        K::EradicateAnimals => "mission_eradicate_animals",
        K::EradicateCityCitizens => "mission_eradicate_city_citizens",
        K::War => "mission_war",
        K::SampleCollection => "mission_sample_collection",
        K::Terraform => "mission_terraform",
        K::MultiDelivery => "mission_multi_delivery",
        K::FetchCommodity => "mission_fetch_commodity",
        K::DestroyTurrets => "mission_destroy_turrets",
        K::FetchPlant => "mission_fetch_plant",
        K::FetchAnimal => "mission_fetch_animal",
        K::Balance => "mission_balance",
        K::FetchTribeCitizen => "mission_fetch_tribe_citizen",
        K::FetchCivCitizen => "mission_fetch_civ_citizen",
        K::FetchEmpireCitizen => "mission_fetch_empire_citizen",
        K::ScanFetch => "mission_scan_fetch",
        K::RaidAlly => "mission_raid_ally",
    })
}

/// The step of a multi-step mission for the conversation flags (`Step1` = 1):
/// the scan sub-mission, then the fetch sub-mission of a scan-fetch.
pub fn mission_step(m: &AlienMission) -> u8 {
    if m.kind == crate::space::AlienMissionKind::ScanFetch {
        m.extra.stage + 1
    } else {
        0
    }
}

/// The mission tokens of `m` (`~mission_rewards~`, `~mission_time~`, ...).
pub fn mission_tokens(m: &AlienMission, planet: &str, star: &str) -> Vec<(&'static str, String)> {
    use crate::space::AlienMissionKind as Kind;
    let world_name = |w: u32| crate::space_galaxy::world_info(w).map(|i| i.name).unwrap_or_else(|| planet.to_string());
    let target = m.target_world.map(world_name).unwrap_or_else(|| planet.to_string());
    // A mission whose target lies at another star names that star
    // (`~mission_target_star~`), and a fetch names its item (`~mission_item~`:
    // "take a member of ~mission_item~", "This ~mission_item~ ...").
    // ADAPTED: retail names the target planet's species; the port has the
    // artifact's name, the commodity, "native" for a citizen and "specimen" for
    // a scan-fetch.
    let target_star = match m.kind {
        Kind::Fetch | Kind::ScanFetch | Kind::Eradicate | Kind::Balance => m
            .target_world
            .zip(crate::space_galaxy::registered_galaxy())
            .map(|(w, g)| g.star_name(crate::space_galaxy::system_of_world(w)))
            .unwrap_or_else(|| star.to_string()),
        _ => star.to_string(),
    };
    let item = match (m.kind, m.retail, m.target_world) {
        (Kind::Fetch, Some(rules::MissionKind::FetchArtifact), Some(w)) => rules::fetch_artifact_item(w).to_string(),
        (Kind::Fetch, Some(rules::MissionKind::FetchCommodity), _) => m.extra.item.clone(),
        (Kind::Fetch, _, _) => "native".to_string(),
        _ => "specimen".to_string(),
    };
    // The multi-delivery strings: the buyer, the items still to deliver and where
    // the items lie.
    let buyer = m.extra.buyer_world.map(world_name).unwrap_or_default();
    let needed = m.target_count.saturating_sub(m.progress);
    let item_planets = m.extra.item_worlds.iter().map(|w| world_name(*w)).collect::<Vec<_>>().join("\n");
    vec![
        ("mission_rewards", money_text(m.reward_spice)),
        ("mission_time", m.time_left.or(m.extra.duration).map(|t| format!("{t:.0}")).unwrap_or_default()),
        ("mission_target_planet", target),
        ("mission_target_star", target_star),
        ("mission_source_planet", planet.to_string()),
        ("mission_source_star", star.to_string()),
        ("mission_source_empire", SpaceCampaign::empire_name(m.empire).to_string()),
        ("mission_num_kills_needed", m.target_count.to_string()),
        ("mission_terraform_target_score", "1".into()),
        ("mission_item", item),
        ("mission_target_animal_species", "animal".into()),
        ("mission_multi_delivery_item", m.extra.item.clone()),
        ("mission_multi_delivery_buyer_planet", buyer),
        ("mission_multi_delivery_buyer_num_needed_items", needed.to_string()),
        ("mission_multi_delivery_item_planet_list", item_planets),
    ]
}

/// The greeting a session opens with (main menu dialogs; triggers from the
/// campaign, ADAPTED where retail's trigger was not traced): first contact
/// (`first_greetings`, or `first_greetings_after_uplift` for an empire the
/// player uplifted), the colonists after the UFO was destroyed within
/// `recentDeathConversationDuration` (120 s), after a disaster on that colony
/// (helped / not helped), on the first visit to a colony, after meeting the
/// Grox; else `greetings`.
pub fn greeting_for(campaign: &SpaceCampaign, empire: Option<EmpireId>, world: u32) -> u32 {
    let u = &campaign.universe;
    match empire {
        Some(e) if !u.contacted.contains(&e) => {
            if campaign.uplifted_empires.values().any(|x| *x == e) {
                dialog::FIRST_GREETINGS_AFTER_UPLIFT
            } else {
                dialog::FIRST_GREETINGS
            }
        }
        Some(_) => dialog::GREETINGS,
        None => {
            if u.last_death.is_some_and(|t| campaign.universe_clock - t <= crate::space_universe::RECENT_DEATH_CONVERSATION) {
                dialog::MYSPECIES_AFTER_DEATH
            } else if let Some((_, helped)) = u.disaster_greeting.filter(|(w, _)| *w == world) {
                if helped { dialog::AFTER_DISASTER_HELPED } else { dialog::AFTER_DISASTER_NOT_HELPED }
            } else if world != 0 && !u.greeted_colonies.contains(&world) {
                dialog::FIRST_COLONY_GREETINGS
            } else if !u.grox_news_told && campaign.relationships.get(&0).is_some_and(|r| r.met) {
                dialog::AFTER_MEETING_GROB
            } else {
                dialog::GREETINGS
            }
        }
    }
}

/// Record that a special greeting was said (each is said once).
fn note_greeting(campaign: &mut SpaceCampaign, d: u32, empire: Option<EmpireId>, world: u32) {
    let u = &mut campaign.universe;
    match d {
        dialog::FIRST_GREETINGS_AFTER_UPLIFT => {
            if let Some(e) = empire {
                u.contacted.insert(e);
            }
        }
        dialog::MYSPECIES_AFTER_DEATH => u.last_death = None,
        dialog::AFTER_DISASTER_HELPED | dialog::AFTER_DISASTER_NOT_HELPED => u.disaster_greeting = None,
        dialog::FIRST_COLONY_GREETINGS => {
            u.greeted_colonies.insert(world);
        }
        dialog::AFTER_MEETING_GROB => u.grox_news_told = true,
        _ => {}
    }
}

/// What follows an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Show(u32, u32),
    Close,
    Stay,
    /// `action_try_mission`: offer a mission (Accept / Decline).
    Mission,
}

fn menu(d: u32) -> Next {
    Next::Show(MAIN_MENU_FILE, d)
}

/// The effect of a response action (0x102DF30 and its helpers).
pub fn apply_action(
    campaign: &mut SpaceCampaign,
    money: &mut f32,
    empire: Option<EmpireId>,
    world: u32,
    id: u32,
    key: (u32, u32, u32),
) -> Next {
    use action as a;
    match id {
        a::GOTO => {
            let file = if key.0 == 0 { MAIN_MENU_FILE } else { key.0 };
            return Next::Show(file, key.2);
        }
        a::MAIN_MENU | a::BACK_TO_MENU => return menu(dialog::DEFAULT),
        a::EXIT | a::DECLINE_ALLIANCE | a::CANNOT_PAY_TRIBUTE | a::DECLINE_SURRENDER => return Next::Close,
        a::TRY_MISSION => return Next::Mission,
        _ => {}
    }
    let Some(e) = empire else {
        return Next::Close;
    };
    match id {
        // 0x102CAF0: pay, GiveGift x the gift's scale; the dialog tells
        // whether the relationship moved.
        a::GIFT_LARGE | a::GIFT_MEDIUM | a::GIFT_SMALL => {
            let gift = match id {
                a::GIFT_LARGE => rules::GiftSize::Large,
                a::GIFT_MEDIUM => rules::GiftSize::Medium,
                _ => rules::GiftSize::Small,
            };
            if *money < gift.amount() {
                return Next::Stay;
            }
            *money -= gift.amount();
            let before = campaign.relationship(e);
            campaign.apply_relationship(e, RelationshipEvent::GiveGift, gift.relationship_scale());
            let moved = (campaign.relationship(e) - before).abs() > 1e-6;
            menu(if moved { dialog::GIVE_GIFT_RELATIONSHIP } else { dialog::GIVE_GIFT_NO_RELATIONSHIP })
        }
        // 0x102CE40: alliance when the relationship reaches createAllianceBase
        // (chance 1.0 above it, 0 below).
        a::TRY_CREATE_ALLIANCE => {
            if campaign.relationship(e) >= rules::CREATE_ALLIANCE_THRESHOLD {
                declare_alliance(campaign, e);
                menu(dialog::CREATE_ALLIANCE_SUCCESSFUL)
            } else {
                menu(dialog::CREATE_ALLIANCE_FAILURE)
            }
        }
        a::ACCEPT_ALLIANCE => {
            declare_alliance(campaign, e);
            Next::Close
        }
        // 0x102CDA0: BreakAlliance + the BreakAlliance event.
        a::BREAK_ALLIANCE => {
            campaign.allies.retain(|x| *x != e);
            campaign.fleet.retain(|s| s.empire != Some(e));
            campaign.apply_relationship(e, RelationshipEvent::BreakAlliance, 1.0);
            menu(dialog::BREAK_ALLIANCE_SUCCESSFUL)
        }
        // 0x1002650: an allied ship joins the posse.
        a::GET_POSSE => {
            if campaign.fleet.len() < campaign.max_fleet_size() {
                let tier = rules::empire_tier(campaign.empire_size(e));
                let hp = rules::UFO_HEALTH_ALLY[tier] * campaign.difficulty().pick(rules::DIFFICULTY_ALLY_HEALTH);
                let n = campaign.fleet.len() + 1;
                campaign.fleet.push(crate::space::EscortShip {
                    name: format!("{} Escort #{n}", SpaceCampaign::empire_name(e)),
                    empire: Some(e),
                    hp,
                    max_hp: hp,
                });
            }
            menu(dialog::GET_POSSE_ACCEPTED)
        }
        // Trade route request (inline in 0x102DF30): refused below Pleased or
        // at the route limit, else the route opens (TradeRoutes badge).
        a::CREATE_TRADE_ROUTE => {
            if stance(campaign.relationship(e)) < 3 {
                return menu(dialog::TRADE_ROUTE_DECLINED_RELATIONSHIP);
            }
            if campaign.trade_routes.len() >= rules::TRADE_ROUTE_MAX {
                return menu(dialog::TRADE_ROUTE_DECLINED_COUNT);
            }
            let target = trade_world(campaign, e, world);
            campaign.trade_routes.entry(target).or_insert(0.0);
            if !campaign.docked.contains(&target) {
                campaign.docked.push(target);
            }
            menu(dialog::TRADE_ROUTE_ACCEPTED)
        }
        a::DESTROY_TRADE_ROUTE => {
            let system = campaign.current_system;
            campaign.trade_routes.retain(|w, _| crate::space_galaxy::system_of_world(*w) != system);
            menu(dialog::TRADE_ROUTE_DESTROYED)
        }
        // 0x102CF20: the offer is graded against the asking price; the money
        // is paid in every case; peace on success (relationship reset).
        x if a::PEACE_OFFER.contains(&x) => {
            let i = a::PEACE_OFFER.iter().position(|p| *p == x).unwrap_or(0);
            let rank = usize::from(campaign.captain_rank());
            let difficulty = campaign.difficulty();
            let offers = peace_offers(rank, difficulty);
            let offer = offers[i];
            if *money < offer {
                return Next::Stay;
            }
            *money -= offer;
            let asked = rules::peace_price(campaign.relationship(e)) * rules::size_multiplier(rank, difficulty);
            let (d, peace) = peace_verdict(offer, asked, i == 4, rules::size_multiplier(rank, difficulty));
            if peace {
                campaign.relationships.remove(&e);
                campaign.meet_empire(e);
                campaign.wars.retain(|w| *w != e);
            }
            let event = if d == dialog::PEACE_BIG_SUCCESS {
                RelationshipEvent::GoodPeaceOffer
            } else {
                RelationshipEvent::BadPeaceOffer
            };
            campaign.apply_relationship(e, event, 1.0);
            if peace {
                // The stance settles as peace even when the event row is low.
                campaign.wars.retain(|w| *w != e);
            }
            menu(d)
        }
        // 0x102D1C0: buying the system (economic capture).
        x if a::PURCHASE_OFFER.contains(&x) => {
            let i = a::PURCHASE_OFFER.iter().position(|p| *p == x).unwrap_or(0);
            let offer = rules::SYSTEM_PURCHASE_OFFERS[i];
            if *money < offer {
                return Next::Stay;
            }
            let worlds = empire_worlds_here(campaign, e);
            let tier = worlds.iter().map(|w| campaign.ecology_for(*w).t_score()).max().unwrap_or(0);
            let price = rules::system_purchase_cost(tier);
            let (d, bought) = purchase_verdict(offer, price, i == 4);
            match d {
                dialog::PURCHASE_BIG_SUCCESS => {
                    campaign.apply_relationship(e, RelationshipEvent::GoodSystemPurchaseOffer, 1.0);
                }
                dialog::PURCHASE_BIG_FAILURE => {
                    campaign.apply_relationship(e, RelationshipEvent::BadSystemPurchaseOffer, 1.0);
                }
                _ => {}
            }
            if bought {
                *money -= offer;
                // `SystemsPurchased` (0x102D1C0), kept apart from conquest.
                campaign.endgame.systems_purchased += 1;
                for w in worlds {
                    if !campaign.conquered_worlds.contains(&w) {
                        campaign.endgame.purchased_worlds += 1;
                        campaign.conquered_worlds.push(w);
                    }
                    campaign.trade_routes.remove(&w);
                    campaign.own(w);
                }
            }
            menu(d)
        }
        // First contact (shared code of nice / neutral): CommunicatedNice,
        // contact recorded, then the main menu.
        a::CONTACT_NICE | a::CONTACT_NEUTRAL => {
            campaign.universe.contacted.insert(e);
            campaign.meet_empire(e);
            campaign.apply_relationship(e, RelationshipEvent::CommunicatedNice, 1.0);
            menu(dialog::DEFAULT)
        }
        a::CONTACT_MEAN => {
            campaign.universe.contacted.insert(e);
            campaign.meet_empire(e);
            campaign.apply_relationship(e, RelationshipEvent::CommunicatedMean, 1.0);
            Next::Close
        }
        // Incoming offer_gift: +npcGiftAmount and AcceptGift.
        a::ACCEPT_GIFT => {
            *money = (*money + rules::NPC_GIFT_AMOUNT).min(WALLET_CAP);
            campaign.apply_relationship(e, RelationshipEvent::AcceptGift, 1.0);
            Next::Close
        }
        // demand_tribute: pay (0x5C2EFCB0) or withhold (WitholdTribute).
        a::PAY_TRIBUTE => {
            let amount = campaign
                .tribute_demand
                .filter(|d| d.empire == e)
                .map_or_else(|| tribute_amount(usize::from(campaign.captain_rank()), campaign.difficulty()), |d| d.amount);
            if *money >= amount {
                *money -= amount;
                campaign.tribute_demand = None;
            }
            Next::Close
        }
        a::WITHHOLD_TRIBUTE => {
            campaign.tribute_demand = None;
            campaign.apply_relationship(e, RelationshipEvent::WitholdTribute, 1.0);
            Next::Close
        }
        // Allies attack on request (0x102DAB0): OpenSpore keeps no list of the
        // ally's enemies, so the request is declined (ADAPTED).
        a::REQUEST_ATTACK => menu(dialog::REQUEST_ATTACK_DECLINED_COUNT),
        a::ACCEPT_SURRENDER => {
            campaign.wars.retain(|w| *w != e);
            campaign.apply_relationship(e, RelationshipEvent::WasAtWar, 1.0);
            Next::Close
        }
        _ => Next::Close,
    }
}

fn declare_alliance(campaign: &mut SpaceCampaign, e: EmpireId) {
    if !campaign.allies.contains(&e) {
        campaign.allies.push(e);
        campaign.badge_stats.alliances_formed += 1;
        campaign.wars.retain(|w| *w != e);
        campaign.apply_relationship(e, RelationshipEvent::CreateAlliance, 1.0);
        if e == 0 {
            // Allying with the Grox: AlliedWithGrob on every other empire.
            campaign.danced_with_the_devil = true;
            for other in campaign.galactic_witnesses().into_iter().filter(|x| *x != 0) {
                campaign.apply_relationship(other, RelationshipEvent::AlliedWithGrob, 1.0);
            }
            campaign.allies.retain(|a| *a == 0);
        }
    }
}

/// Peace offer verdict (0x102CF20): `(dialog, peace)`. Above the asking price
/// by the good delta: big success; above it less the bad delta: success; the
/// largest offer always succeeds; down to the very bad delta: failure, else
/// big failure. VERIFIED (space-audit-a, 0x1030AB0 / 0x10309D0 / 0x1030A40): the
/// good / bad / very bad deltas are `peaceOffer*Delta` (defaults 99 / ...) x the
/// same multiplier as the prices (`size_multiplier`, truncated to an int).
pub fn peace_verdict(offer: f32, asked: f32, largest: bool, scale: f32) -> (u32, bool) {
    let diff = offer - asked;
    if diff > rules::PEACE_GOOD_OFFER_DELTA * scale {
        (dialog::PEACE_BIG_SUCCESS, true)
    } else if diff > rules::PEACE_BAD_OFFER_DELTA * scale || largest {
        (dialog::PEACE_SUCCESS, true)
    } else if diff > rules::PEACE_VERY_BAD_OFFER_DELTA * scale {
        (dialog::PEACE_FAILURE, false)
    } else {
        (dialog::PEACE_BIG_FAILURE, false)
    }
}

/// System purchase verdict (0x102D1C0): `(dialog, bought)`.
pub fn purchase_verdict(offer: f32, price: f32, largest: bool) -> (u32, bool) {
    let diff = offer - price;
    if diff > rules::PURCHASE_GOOD_OFFER_DELTA {
        (dialog::PURCHASE_BIG_SUCCESS, true)
    } else if diff > 0.0 || largest {
        (dialog::PURCHASE_SUCCESS, true)
    } else if diff > rules::PURCHASE_BAD_OFFER_DELTA {
        (dialog::PURCHASE_FAILURE, false)
    } else {
        (dialog::PURCHASE_BIG_FAILURE, false)
    }
}

/// The empire's worlds in the current system.
fn empire_worlds_here(campaign: &SpaceCampaign, e: EmpireId) -> Vec<u32> {
    (0..crate::space_galaxy::WORLDS_PER_SYSTEM)
        .map(|slot| crate::space_galaxy::world_id(campaign.current_system, slot))
        .filter(|w| campaign.world_empire(*w) == Some(e))
        .collect()
}

/// The world a new trade route goes to (the one the UFO talks to, else the
/// empire's first world here).
fn trade_world(campaign: &SpaceCampaign, e: EmpireId, world: u32) -> u32 {
    if campaign.world_empire(world) == Some(e) {
        world
    } else {
        empire_worlds_here(campaign, e).first().copied().unwrap_or(world)
    }
}

// ---------------------------------------------------------------------------
// NPC store (spacetrading~ per-archetype stock and price multipliers).
// ---------------------------------------------------------------------------

/// Per-store-item `spaceTradingChance<Arch>` and `spaceTradingCostMultiplier<Arch>`
/// for Warrior, Trader, Scientist, Shaman, Bard, Zealot, Diplomat, Ecologist,
/// Grox (missing multipliers are 1), plus abundance, chance and allies-only.
/// Generated from `notes\space-re\space_trading_table.txt`.
#[rustfmt::skip]
pub const STORE_STOCK: &[(&str, [f32; 9], [f32; 9], f32, f32, bool)] = &[
    ("toolinterplanetarydrive", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], 100.0, 0.0, false),
    ("toolplacecolony", [0.0, 100.0, 100.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 0.65, 0.65, 0.5, 1.0, 0.6, 0.75, 1.0, 1.0], 100.0, 3.0, false),
    ("toolinterstellardrive2", [0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 0.5, 0.5, 1.0, 1.0, 0.5, 0.5, 1.0, 1.0], 100.0, 1.0, false),
    ("toolinterstellardrive3", [0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 0.5, 0.5, 1.0, 1.0, 0.5, 0.5, 1.0, 1.0], 100.0, 1.0, false),
    ("toolinterstellardrive4", [0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 0.5, 0.5, 1.0, 1.0, 0.5, 0.5, 1.0, 1.0], 100.0, 1.0, false),
    ("toolinterstellardrive5", [0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 0.5, 0.5, 1.0, 1.0, 0.5, 0.5, 1.0, 1.0], 100.0, 1.0, false),
    ("toolhealth2", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolhealth3", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolhealth4", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolhealth5", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolenergy2", [100.0, 0.0, 100.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0], [0.5, 1.0, 0.5, 1.0, 0.5, 1.0, 1.0, 0.5, 1.0], 100.0, 1.0, false),
    ("toolenergy3", [100.0, 0.0, 100.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0], [0.5, 1.0, 0.5, 1.0, 0.5, 1.0, 1.0, 0.5, 1.0], 100.0, 1.0, false),
    ("toolenergy4", [100.0, 0.0, 100.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0], [0.5, 1.0, 0.5, 1.0, 0.5, 1.0, 1.0, 0.5, 1.0], 100.0, 1.0, false),
    ("toolenergy5", [100.0, 0.0, 100.0, 0.0, 100.0, 0.0, 0.0, 100.0, 100.0], [0.5, 1.0, 0.5, 1.0, 0.5, 1.0, 1.0, 0.5, 1.0], 100.0, 1.0, false),
    ("toolcargoupgrade1", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolcargoupgrade2", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolcargoupgrade3", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toollaser1", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 30.0, 5.0, false),
    ("toollaser2", [100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 30.0, 5.0, false),
    ("toollaser3", [100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 10.0, 5.0, false),
    ("toolprotonmissilelevel1", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 25.0, 5.0, false),
    ("toolprotonmissilelevel2", [100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 25.0, 5.0, false),
    ("toolprotonmissilelevel3", [100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 25.0, 5.0, false),
    ("toolpulse1", [100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 30.0, 5.0, false),
    ("toolshield", [100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 10.0, 5.0, false),
    ("toolcloak", [100.0, 100.0, 0.0, 0.0, 100.0, 0.0, 100.0, 0.0, 100.0], [0.5, 0.5, 1.0, 1.0, 0.5, 1.0, 0.5, 1.0, 1.0], 10.0, 5.0, false),
    ("toolrallycall", [100.0, 0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.6, 1.0, 0.5, 0.6, 1.0, 1.0], 20.0, 3.0, false),
    ("toolrepairpotion", [100.0, 0.0, 0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0], [0.5, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], 100.0, 1.0, false),
    ("toolenergypotion", [0.0, 100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 100.0, 100.0], [1.0, 0.5, 0.5, 1.0, 1.0, 1.0, 0.5, 0.5, 1.0], 5.0, 1.0, false),
    ("toolheatray", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 100.0, 100.0], [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0], 20.0, 1.0, false),
    ("toolcoldray", [0.0, 100.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0], [1.0, 0.6, 0.6, 0.6, 0.6, 1.0, 1.0, 0.5, 1.0], 20.0, 1.0, false),
    ("toolcloudray", [0.0, 100.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0], [1.0, 0.6, 0.6, 0.6, 0.6, 1.0, 1.0, 0.5, 1.0], 20.0, 1.0, false),
    ("toolcloudvacuum", [0.0, 100.0, 100.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0], [1.0, 0.6, 0.6, 0.6, 0.6, 1.0, 1.0, 0.5, 1.0], 20.0, 1.0, false),
    ("toolatmospheregenerator", [0.0, 0.0, 100.0, 100.0, 0.0, 100.0, 100.0, 100.0, 100.0], [1.0, 1.0, 0.6, 0.6, 1.0, 0.6, 1.0, 0.5, 1.0], 20.0, 3.0, false),
    ("tooldroughtgenerator", [0.0, 0.0, 100.0, 100.0, 0.0, 100.0, 0.0, 100.0, 100.0], [1.0, 1.0, 0.6, 0.6, 1.0, 0.6, 1.0, 0.5, 1.0], 20.0, 3.0, false),
    ("tooliceStorm", [0.0, 0.0, 100.0, 100.0, 0.0, 100.0, 0.0, 100.0, 100.0], [1.0, 1.0, 0.6, 0.6, 1.0, 0.6, 1.0, 0.5, 1.0], 20.0, 3.0, false),
    ("toolasteroid", [0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0, 100.0], [1.0, 1.0, 0.6, 1.0, 0.6, 0.6, 1.0, 0.5, 1.0], 20.0, 3.0, false),
    ("toolhappyray1", [0.0, 0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 1.0, 1.0, 0.6, 1.0, 0.5, 0.6, 1.0, 1.0], 30.0, 5.0, false),
    ("toolplaceembassy", [0.0, 100.0, 0.0, 100.0, 0.0, 0.0, 100.0, 0.0, 100.0], [1.0, 0.65, 1.0, 0.5, 1.0, 1.0, 0.5, 1.0, 1.0], 10.0, 1.0, false),
    ("toolplacespicestorage", [0.0, 100.0, 100.0, 100.0, 100.0, 0.0, 0.0, 0.0, 100.0], [1.0, 0.5, 0.65, 0.6, 0.5, 1.0, 1.0, 1.0, 1.0], 10.0, 1.0, false),
    ("toolplaceuberturret", [100.0, 100.0, 100.0, 0.0, 0.0, 100.0, 0.0, 0.0, 100.0], [0.5, 0.6, 0.6, 1.0, 1.0, 0.6, 1.0, 1.0, 1.0], 10.0, 1.0, false),
    ("toolplanetbuster", [100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 100.0], [0.5, 0.65, 1.0, 1.0, 1.0, 0.75, 1.0, 1.0, 1.0], 1.0, 1.0, false),
    ("toolwormhole", [0.0, 100.0, 100.0, 0.0, 0.0, 100.0, 100.0, 0.0, 100.0], [1.0, 0.5, 0.5, 1.0, 1.0, 0.5, 0.5, 1.0, 1.0], 10.0, 1.0, false),
    ("toolmonolith", [0.0, 100.0, 100.0, 100.0, 0.0, 100.0, 0.0, 0.0, 100.0], [1.0, 0.6, 0.5, 0.5, 1.0, 0.5, 1.0, 1.0, 1.0], 5.0, 1.0, false),
    ("toolcreatureedit", [0.0, 100.0, 100.0, 100.0, 0.0, 0.0, 0.0, 100.0, 100.0], [1.0, 0.65, 0.5, 0.5, 1.0, 1.0, 1.0, 0.5, 1.0], 5.0, 1.0, false),
    ("toolcreaturecreate", [0.0, 100.0, 100.0, 0.0, 0.0, 0.0, 0.0, 100.0, 100.0], [1.0, 0.65, 0.5, 1.0, 1.0, 1.0, 1.0, 0.5, 1.0], 20.0, 1.0, false),
];

/// Index of an NPC archetype in [`STORE_STOCK`] rows.
pub fn stock_column(archetype: SpaceArchetype, grox: bool) -> usize {
    if grox {
        return 8;
    }
    match archetype {
        SpaceArchetype::Warrior | SpaceArchetype::Knight => 0,
        SpaceArchetype::Trader => 1,
        SpaceArchetype::Scientist => 2,
        SpaceArchetype::Shaman => 3,
        SpaceArchetype::Bard => 4,
        SpaceArchetype::Zealot => 5,
        SpaceArchetype::Diplomat | SpaceArchetype::Wanderer => 6,
        SpaceArchetype::Ecologist => 7,
    }
}

/// Why a store item is locked for the player (None: it can be bought).
pub fn lock_reason(campaign: &SpaceCampaign, item: &rules::StoreItem, seller_tier: usize) -> Option<String> {
    if item_unlocked(campaign, item, seller_tier) {
        return None;
    }
    if let Some(req) = item.requires.filter(|r| !campaign.has_tool(r)) {
        return Some(format!("requires the {}", tool_label(req)));
    }
    let badge = |b: &str| {
        let (family, level) = b.rsplit_once('-').unwrap_or((b, "1"));
        format!("{} badge level {level}", tool_label(family))
    };
    Some(match (item.badge, item.badge_or) {
        (Some(a), Some(b)) => format!("requires the {} or the {}", badge(a), badge(b)),
        (Some(a), None) | (None, Some(a)) => format!("requires the {}", badge(a)),
        (None, None) => "locked".into(),
    })
}

/// Whether the player meets a store item's gates (VERIFIED 0x10399A0 and
/// 0x1039530): the previous item owned (`spaceTradingRequires`), and
/// `RequiresBadge` earned (or none) or else `RequiresBadgeOr` earned.
/// `spaceTradingRequiresTier` gates nothing: no code reads 0x0567DDD9 (no
/// immediate, no table entry, no string in the image; space-endgame notes E3).
pub fn item_unlocked(campaign: &SpaceCampaign, item: &rules::StoreItem, _seller_tier: usize) -> bool {
    if let Some(req) = item.requires {
        if !campaign.has_tool(req) {
            return false;
        }
    }
    let badge_ok = |b: Option<&str>| {
        b.is_none_or(|b| {
            let (family, level) = b.rsplit_once('-').unwrap_or((b, "1"));
            let level: u8 = level.parse().unwrap_or(1);
            rules::badge_family(family).map(|f| campaign.badge_family_level(f) >= level).unwrap_or(false)
        })
    };
    // (RequiresBadge absent or earned) or (RequiresBadgeOr present and earned).
    badge_ok(item.badge) || (item.badge_or.is_some() && badge_ok(item.badge_or))
}

fn tool_label(tool: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in tool.chars() {
        if c == '_' {
            out.push(' ');
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The stock of `empire`'s store: items its archetype sells
/// (`spaceTradingChance<Arch>` > 0) and the player does not own yet
/// (upgrades), at base x `spaceTradingCostMultiplier<Arch>` x difficulty;
/// items whose gates are not met are listed locked with the reason.
/// ADAPTED: every eligible item is in stock (the abundance / chance draw and
/// the re-offer timer are not traced).
pub fn npc_stock(campaign: &SpaceCampaign, empire: EmpireId) -> Vec<StockItem> {
    let col = stock_column(empire_archetype(empire), empire == 0);
    let tier = rules::empire_tier(campaign.empire_size(empire));
    let difficulty = campaign.difficulty();
    let allied = campaign.allies.contains(&empire);
    let mut out = Vec::new();
    for item in rules::STORE {
        let Some(row) = STORE_STOCK.iter().find(|r| r.0.eq_ignore_ascii_case(item.id)) else {
            continue;
        };
        if row.1[col] <= 0.0 || (row.5 && !allied) {
            continue;
        }
        if !item.consumable && campaign.has_tool(item.tool) {
            continue;
        }
        out.push(StockItem {
            locked: lock_reason(campaign, item, tier),
            id: item.id.to_string(),
            tool: item.tool.to_string(),
            label: tool_label(item.tool),
            price: rules::store_price_at(item.cost, row.2[col], difficulty, false),
            consumable: item.consumable,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn text(id: u32, req: &[usize], exc: &[usize]) -> CnvText {
        CnvText { require: Flags::from_list(req), exclude: Flags::from_list(exc), id, table: 7, emotion: 0 }
    }

    fn response(action: u32, key: (u32, u32, u32), req: &[usize], ereq: &[usize], label: u32) -> CnvResponse {
        CnvResponse {
            require: Flags::from_list(req),
            enable_require: Flags::from_list(ereq),
            action,
            key,
            texts: vec![text(label, &[], &[])],
            ..Default::default()
        }
    }

    /// A small main menu: greetings -> default (tabs) -> diplomacy with a gift menu.
    fn library() -> CnvLibrary {
        let mut conv = Conversation::default();
        let mut add = |id: u32, texts: Vec<CnvText>, responses: Vec<CnvResponse>| {
            conv.dialogs.insert(id, CnvDialog { id, texts, responses, action: action::NONE, ..Default::default() });
        };
        add(
            dialog::GREETINGS,
            vec![text(1, &[flag::WARRIOR, flag::PLEASED_OR_BETTER], &[]), text(2, &[flag::WARRIOR, flag::CAUTIOUS_OR_WORSE], &[])],
            vec![response(action::GOTO, (MAIN_MENU_FILE, 0, dialog::DEFAULT), &[], &[], 10), response(action::EXIT, (0, 0, 0), &[], &[], 11)],
        );
        add(
            dialog::FIRST_GREETINGS,
            vec![text(3, &[], &[])],
            vec![response(action::CONTACT_NICE, (0, 0, 0), &[], &[], 12)],
        );
        add(dialog::DEFAULT, vec![text(4, &[], &[])], vec![]);
        add(
            dialog::DIPLOMACY,
            vec![text(5, &[], &[])],
            vec![
                response(action::GIFT_SMALL, (0, 0, 0), &[], &[flag::SMALL_GIFT_MONEY], 13),
                response(action::TRY_CREATE_ALLIANCE, (0, 0, 0), &[], &[], 14),
                response(action::BREAK_ALLIANCE, (0, 0, 0), &[flag::ALLIANCE], &[], 15),
            ],
        );
        for d in [
            dialog::GIVE_GIFT_RELATIONSHIP,
            dialog::GIVE_GIFT_NO_RELATIONSHIP,
            dialog::CREATE_ALLIANCE_SUCCESSFUL,
            dialog::CREATE_ALLIANCE_FAILURE,
            dialog::MISSION_UNAVAILABLE,
            dialog::MISSION_MAIN,
        ] {
            add(d, vec![text(d & 0xFF, &[], &[])], vec![response(action::GOTO, (MAIN_MENU_FILE, 0, dialog::DEFAULT), &[], &[], 16)]);
        }
        let strings = vec![
            ((7, 1), "Hail ~player_title~, friend of the ~race~!~p~Welcome.".to_string()),
            ((7, 2), "State your business.".to_string()),
            ((7, 3), "Who are you?".to_string()),
            ((7, 4), "How may we help?".to_string()),
            ((7, 10), "Continue".to_string()),
            ((7, 13), "A gift of ~small_gift_amount~".to_string()),
        ];
        CnvLibrary::with(vec![(MAIN_MENU_FILE, conv)], strings)
    }

    fn warrior_empire() -> EmpireId {
        (1..60).find(|e| empire_archetype(*e) == SpaceArchetype::Warrior).expect("a warrior empire")
    }

    #[test]
    fn fnv_names_match_the_ids_decoded_from_the_files() {
        assert_eq!(fnv("default"), dialog::DEFAULT);
        assert_eq!(fnv("greetings"), dialog::GREETINGS);
        assert_eq!(fnv("exit"), action::EXIT);
        assert_eq!(fnv("action_peace_offer_4"), 0x9320_DBBA);
        assert_eq!(fnv("action_give_large_gift"), action::GIFT_LARGE);
        assert_eq!(fnv("action_try_create_alliance"), action::TRY_CREATE_ALLIANCE);
        assert_eq!(fnv("space_conversation_main_menu"), MAIN_MENU_FILE);
        assert_eq!(fnv("incoming_messages"), INCOMING_FILE);
        assert_eq!(fnv("demand_tribute"), dialog::DEMAND_TRIBUTE);
        assert_eq!(crate::world_objects::name_id("Mission_Info"), fnv("mission_info"));
    }

    #[test]
    fn a_cnv_round_trips_through_the_binary_reader() {
        // Hand-built file: 0 animations, 1 dialog with 1 text and 1 response.
        let mut b = Vec::new();
        b.extend(0u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(dialog::DEFAULT.to_le_bytes());
        b.extend(7u32.to_be_bytes());
        b.extend(b"default");
        b.extend([0, 1, 0, 1, 0]);
        b.extend(action::NONE.to_le_bytes());
        b.extend([0u8; 12]);
        b.extend(1u32.to_le_bytes());
        let mut req = [0u8; FLAG_COUNT];
        req[flag::WARRIOR] = 1;
        b.extend(req);
        b.extend([0u8; FLAG_COUNT]);
        b.extend(0x22u32.to_be_bytes());
        b.extend(MAIN_MENU_FILE.to_be_bytes());
        b.extend(0x27F8_B66Au32.to_be_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend([0u8; FLAG_COUNT * 4]);
        b.extend(action::EXIT.to_le_bytes());
        b.extend([0u8; 12]);
        b.extend(0u32.to_le_bytes());
        for _ in 0..4 {
            b.extend(0u32.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        let c = parse_cnv(&b).expect("parses");
        let d = &c.dialogs[&dialog::DEFAULT];
        assert_eq!(d.name, "default");
        assert_eq!(d.flags, [false, true, false, true, false]);
        assert_eq!(d.texts[0].id, 0x22);
        assert!(d.texts[0].require.has(flag::WARRIOR));
        assert_eq!(d.responses[0].action, action::EXIT);
        assert!(parse_cnv(&b[..b.len() - 1]).is_err(), "truncated files are refused");
    }

    #[test]
    fn texts_and_responses_follow_the_required_and_excluded_flags() {
        let texts = vec![text(1, &[flag::WARRIOR], &[flag::AT_WAR]), text(2, &[flag::WARRIOR, flag::AT_WAR], &[]), text(3, &[], &[])];
        let peace = Flags::from_list(&[flag::WARRIOR]);
        let war = Flags::from_list(&[flag::WARRIOR, flag::AT_WAR]);
        let ids = |f: &Flags| text_candidates(&texts, f).iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(&peace), vec![1, 3]);
        assert_eq!(ids(&war), vec![2, 3]);
        assert_eq!(pick_text(&texts, &peace, 0).unwrap().id, 1);
        assert_eq!(pick_text(&texts, &peace, 1).unwrap().id, 3);
        let d = CnvDialog {
            responses: vec![
                response(1, (0, 0, 0), &[flag::ALLIANCE], &[], 0),
                response(2, (0, 0, 0), &[], &[flag::SMALL_GIFT_MONEY], 0),
            ],
            ..Default::default()
        };
        let shown = visible_responses(&d, &Flags::default());
        assert_eq!(shown.len(), 1, "the alliance-only response is hidden");
        assert_eq!((shown[0].0.action, shown[0].1), (2, false), "shown but disabled without the money");
        let rich = Flags::from_list(&[flag::SMALL_GIFT_MONEY, flag::ALLIANCE]);
        assert_eq!(visible_responses(&d, &rich).iter().map(|r| r.1).collect::<Vec<_>>(), vec![true, true]);
    }

    #[test]
    fn the_flags_carry_attitude_archetype_money_and_size() {
        let ctx = CommContext {
            empire: Some(3),
            archetype: Some(SpaceArchetype::Trader),
            relationship: 4.0,
            money: 60_000.0,
            peace_offers: [10_000.0, 20_000.0, 30_000.0, 40_000.0, 80_000.0],
            tribute: 20_000.0,
            empire_tier: 3,
            rank: 2,
            ..Default::default()
        };
        let f = space_flags(&ctx);
        for i in [
            flag::ATTITUDE_PLEASED,
            flag::PLEASED_OR_BETTER,
            flag::CAUTIOUS_OR_BETTER,
            flag::PLEASED_OR_WORSE,
            flag::TRADE,
            flag::TRADER,
            flag::MEDIUM_EMPIRE,
            flag::SMALL_GIFT_MONEY,
            flag::MEDIUM_GIFT_MONEY,
            flag::TRIBUTE_MONEY,
            flag::PEACE_OFFER_MONEY_1 + 3,
            flag::MASTER_BADGE_LEVEL_0 + 2,
            flag::OPEN_TO_TRADES,
        ] {
            assert!(f.has(i), "flag {i}");
        }
        for i in [flag::LARGE_GIFT_MONEY, flag::PEACE_OFFER_MONEY_1 + 4, flag::MY_SPECIES, flag::AT_WAR, flag::ATTITUDE_FRIENDLY] {
            assert!(!f.has(i), "flag {i}");
        }
        let furious = space_flags(&CommContext { empire: Some(0), grox: true, relationship: -9.0, at_war: true, ..Default::default() });
        for i in [flag::ATTITUDE_FURIOUS, flag::ANNOYED_OR_WORSE, flag::GROB, flag::GROB_GOAL, flag::AT_WAR, flag::SMALL_EMPIRE] {
            assert!(furious.has(i), "flag {i}");
        }
        assert!(!furious.has(flag::ANNOYED_OR_BETTER));
        let home = space_flags(&CommContext { homeworld: true, ..Default::default() });
        assert!(home.has(flag::MY_SPECIES) && home.has(flag::HOMEWORLD) && !home.has(flag::COLONY));
    }

    #[test]
    fn tokens_are_substituted_and_paragraphs_break() {
        let t = Tokens { player_title: "Admiral Zorb".into(), race: "Avorni".into(), ..Default::default() };
        assert_eq!(substitute("Hail ~player_title~ of ~race~!~p~Go.", &t), "Hail Admiral Zorb of Avorni!\n\nGo.");
        assert_eq!(substitute("~unknown~x", &t), "x");
        assert_eq!(substitute("a ~small_gift_amount~", &t), format!("a {}", money_text(10_000.0)));
        assert_eq!(substitute("open ~ tilde", &t), "open ~ tilde");
    }

    #[test]
    fn plural_tokens_pick_by_the_count() {
        let mut t = Tokens::default();
        t.extra = vec![("mission_num_kills_needed", "1".into())];
        let text = "Remove ~mission_num_kills_needed~ more ~^p^mission_num_kills_needed^animal^animals~.";
        assert_eq!(substitute(text, &t), "Remove 1 more animal.");
        t.extra = vec![("mission_num_kills_needed", "5".into())];
        assert_eq!(substitute(text, &t), "Remove 5 more animals.");
        // An unknown count reads as plural.
        t.extra.clear();
        assert_eq!(substitute("~^p^n^city^cities~", &t), "cities");
    }

    #[test]
    fn a_session_walks_greeting_menu_diplomacy_and_a_gift() {
        let mut lib = library();
        let mut run = RunProgress::default();
        run.spice = 50_000.0;
        let e = warrior_empire();
        let env = CommEnv { index: None, ufo_damaged: false };
        let mut s = CommSession::default();
        // Never contacted: first greetings, then contact -> main menu.
        s.open_with(&mut lib, &mut run, &env, Some(e), 0, 7);
        assert!(s.open);
        assert_eq!(s.dialog, dialog::FIRST_GREETINGS);
        assert_eq!(s.text, "Who are you?");
        s.choose(&mut lib, &mut run, &env, 0);
        assert!(run.space_campaign.universe.contacted.contains(&e));
        assert_eq!(s.dialog, dialog::DEFAULT);
        assert!(s.show_tabs, "the tabs show on the main menu only");
        // Greetings now depend on the attitude.
        s.open_with(&mut lib, &mut run, &env, Some(e), 0, 9);
        assert_eq!(s.dialog, dialog::GREETINGS);
        assert!(!s.show_tabs);
        let rel = run.space_campaign.relationship(e);
        let expected = if stance(rel) >= 3 { "Hail" } else { "State your business." };
        assert!(s.text.starts_with(expected), "{rel}: {}", s.text);
        // Diplomacy: the gift is shown and enabled with the money.
        s.tab_diplomacy(&mut lib, &mut run, &env);
        assert_eq!(s.responses.len(), 2, "no break-alliance response without an alliance");
        assert!(s.responses[0].enabled);
        assert!(s.responses[0].label.contains("10,000"));
        let before = run.space_campaign.relationship(e);
        s.choose(&mut lib, &mut run, &env, 0);
        assert_eq!(run.spice, 40_000.0);
        assert!(run.space_campaign.relationship(e) > before);
        assert_eq!(s.dialog, dialog::GIVE_GIFT_RELATIONSHIP);
        // Not enough money: shown but disabled, the click is refused.
        run.spice = 5_000.0;
        s.tab_diplomacy(&mut lib, &mut run, &env);
        assert!(!s.responses[0].enabled);
        s.choose(&mut lib, &mut run, &env, 0);
        assert_eq!(run.spice, 5_000.0);
        assert_eq!(s.sound, Some(crate::game_audio::Cue::SpgError));
        // The alliance needs createAllianceBase (7).
        s.choose(&mut lib, &mut run, &env, 1);
        let allied = run.space_campaign.allies.contains(&e);
        assert_eq!(allied, run.space_campaign.relationship(e) >= rules::CREATE_ALLIANCE_THRESHOLD);
        assert_eq!(s.dialog, if allied { dialog::CREATE_ALLIANCE_SUCCESSFUL } else { dialog::CREATE_ALLIANCE_FAILURE });
    }

    #[test]
    fn diplomacy_actions_follow_the_retail_rules() {
        let mut c = SpaceCampaign::default();
        let e = warrior_empire();
        let mut money = 1_000_000.0;
        // Trade route: refused below Pleased (a warrior starts at -3; gifts
        // cap at GiveGift's +5).
        c.meet_empire(e);
        assert_eq!(apply_action(&mut c, &mut money, Some(e), 0, action::CREATE_TRADE_ROUTE, (0, 0, 0)), menu(dialog::TRADE_ROUTE_DECLINED_RELATIONSHIP));
        let before = c.relationship(e);
        for _ in 0..3 {
            apply_action(&mut c, &mut money, Some(e), 0, action::GIFT_LARGE, (0, 0, 0));
        }
        assert_eq!(money, 700_000.0);
        assert!(c.relationship(e) > before);
        // A diplomat (PersonalityNice +3) is pleased at once and opens the route.
        let d = (1..80).find(|x| empire_archetype(*x) == SpaceArchetype::Diplomat).expect("diplomat");
        c.meet_empire(d);
        assert!(stance(c.relationship(d)) >= 3, "{}", c.relationship(d));
        assert_eq!(apply_action(&mut c, &mut money, Some(d), 0, action::CREATE_TRADE_ROUTE, (0, 0, 0)), menu(dialog::TRADE_ROUTE_ACCEPTED));
        assert_eq!(c.trade_routes.len(), 1);
        // Breaking an alliance.
        c.allies.push(e);
        assert_eq!(apply_action(&mut c, &mut money, Some(e), 0, action::BREAK_ALLIANCE, (0, 0, 0)), menu(dialog::BREAK_ALLIANCE_SUCCESSFUL));
        assert!(!c.allies.contains(&e));
        // Goto / exit.
        assert_eq!(apply_action(&mut c, &mut money, Some(e), 0, action::GOTO, (MAIN_MENU_FILE, 0, dialog::DIPLOMACY)), menu(dialog::DIPLOMACY));
        assert_eq!(apply_action(&mut c, &mut money, Some(e), 0, action::EXIT, (0, 0, 0)), Next::Close);
        assert_eq!(apply_action(&mut c, &mut money, Some(e), 0, action::TRY_MISSION, (0, 0, 0)), Next::Mission);
        // Tribute: paid when affordable, withheld costs the relationship.
        c.tribute_demand = Some(crate::space::TributeDemand { empire: e, amount: 20_000.0, time_left: 100.0 });
        let m = money;
        assert_eq!(apply_action(&mut c, &mut money, Some(e), 0, action::PAY_TRIBUTE, (0, 0, 0)), Next::Close);
        assert_eq!(money, m - 20_000.0);
        assert!(c.tribute_demand.is_none());
        let before = c.relationship(e);
        apply_action(&mut c, &mut money, Some(e), 0, action::WITHHOLD_TRIBUTE, (0, 0, 0));
        assert!(c.relationship(e) < before);
        // An NPC gift pays npcGiftAmount.
        let m = money;
        apply_action(&mut c, &mut money, Some(e), 0, action::ACCEPT_GIFT, (0, 0, 0));
        assert_eq!(money, m + rules::NPC_GIFT_AMOUNT);
    }

    #[test]
    fn peace_and_purchase_offers_grade_against_the_asking_price() {
        assert_eq!(peace_verdict(60_000.0, 40_000.0, false, 1.0), (dialog::PEACE_BIG_SUCCESS, true));
        assert_eq!(peace_verdict(40_000.0, 43_000.0, false, 1.0), (dialog::PEACE_SUCCESS, true));
        assert_eq!(peace_verdict(40_000.0, 50_000.0, false, 1.0), (dialog::PEACE_FAILURE, false));
        assert_eq!(peace_verdict(40_000.0, 60_000.0, false, 1.0), (dialog::PEACE_BIG_FAILURE, false));
        assert_eq!(peace_verdict(40_000.0, 90_000.0, true, 1.0), (dialog::PEACE_SUCCESS, true), "the largest offer always works");
        assert_eq!(purchase_verdict(1_000_000.0, 400_000.0, false), (dialog::PURCHASE_BIG_SUCCESS, true));
        assert_eq!(purchase_verdict(500_000.0, 400_000.0, false), (dialog::PURCHASE_SUCCESS, true));
        assert_eq!(purchase_verdict(1_000_000.0, 1_100_000.0, false), (dialog::PURCHASE_FAILURE, false));
        assert_eq!(purchase_verdict(500_000.0, 5_000_000.0, false), (dialog::PURCHASE_BIG_FAILURE, false));
        // Offers scale with the player's rank (normal difficulty halves the hard row).
        let small = peace_offers(0, rules::Difficulty::Normal);
        assert_eq!(small, [10_000.0, 12_500.0, 15_000.0, 17_500.0, 20_000.0]);
        assert_eq!(tribute_amount(1, rules::Difficulty::Normal), 10_000.0);
        // War ends on a successful peace offer; the money is spent either way.
        let mut c = SpaceCampaign::default();
        let e = warrior_empire();
        c.meet_empire(e);
        c.wars.push(e);
        let mut money = 1_000_000.0;
        let next = apply_action(&mut c, &mut money, Some(e), 0, action::PEACE_OFFER[4], (0, 0, 0));
        assert!(matches!(next, Next::Show(_, d) if d == dialog::PEACE_SUCCESS || d == dialog::PEACE_BIG_SUCCESS));
        assert!(!c.wars.contains(&e));
        assert!(money < 1_000_000.0);
    }

    #[test]
    fn npc_stores_follow_archetype_stock_multipliers_and_gates() {
        let mut c = SpaceCampaign::default();
        c.grant_tool("interplanetarydrive");
        c.grant_tool("healthupgrade1");
        // A trader sells the Colony Incredi-Pak at x0.65; a warrior does not stock it.
        let trader = (1..80).find(|e| empire_archetype(*e) == SpaceArchetype::Trader).expect("trader");
        let warrior = warrior_empire();
        c.empire_sizes = vec![20; 100];
        let ts = npc_stock(&c, trader);
        let pack = ts.iter().find(|i| i.tool == "placecolony").expect("traders sell colony packs");
        assert_eq!(pack.price, (150_000.0f32 * 0.65).round());
        assert!(!npc_stock(&c, warrior).iter().any(|i| i.tool == "placecolony"));
        // Health 2 needs conquerer-1 or colonist-1: locked for a new captain.
        let h2 = npc_stock(&c, warrior).into_iter().find(|i| i.tool == "healthupgrade2").expect("warriors stock health 2");
        assert!(h2.locked.as_deref().is_some_and(|r| r.contains("Conquerer")), "{:?}", h2.locked);
        let item = rules::store_item_for_tool("healthupgrade2").unwrap();
        assert!(!item_unlocked(&c, item, 5));
        // Owned upgrades leave the store.
        c.grant_tool("laserlevel1");
        assert!(!npc_stock(&c, warrior).iter().any(|i| i.tool == "laserlevel1"));
        assert_eq!(STORE_STOCK.len(), rules::STORE.len(), "every store item has its stock row");
    }

    #[test]
    fn buying_and_selling_on_the_trade_tab() {
        let mut run = RunProgress::default();
        run.spice = 1_000_000.0;
        run.space_campaign.grant_tool("interplanetarydrive");
        run.space_campaign.empire_sizes = vec![20; 100];
        let trader = (1..80).find(|e| empire_archetype(*e) == SpaceArchetype::Trader).expect("trader");
        let mut s = CommSession { open: true, empire: Some(trader), ..Default::default() };
        s.tab_trade(&run);
        let i = s.trade.as_ref().unwrap().stock.iter().position(|x| x.tool == "placecolony").expect("pack");
        s.trade_select(TradeSide::Buy, i);
        s.trade_count(2, &run);
        assert_eq!(s.trade.as_ref().unwrap().count, 3);
        let price = s.trade.as_ref().unwrap().stock[i].price;
        s.trade_buy(&mut run);
        assert_eq!(run.space_campaign.charges("placecolony"), 3);
        assert_eq!(run.spice, 1_000_000.0 - 3.0 * price);
        assert_eq!(run.space_campaign.endgame.tools_purchased, 3, "ToolsPurchased counts every unit");
        assert_eq!(s.sound, Some(crate::game_audio::Cue::UfoComTradeBuy));
        // Selling spice from the hold.
        run.space_campaign.cargo_spice.insert(crate::space::SpiceColor::Red, 5);
        let money = run.spice;
        s.trade_select(TradeSide::Sell, 0);
        s.trade_count(10, &run);
        assert_eq!(s.trade.as_ref().unwrap().count, 5, "no more than the hold carries");
        s.trade_sell(&mut run);
        assert!(run.space_campaign.cargo_spice.is_empty());
        assert!(run.spice > money);
    }

    #[test]
    fn special_greetings_follow_their_triggers() {
        let mut c = SpaceCampaign::default();
        let e = warrior_empire();
        assert_eq!(greeting_for(&c, Some(e), 0), dialog::FIRST_GREETINGS);
        c.uplifted_empires.insert(99, e);
        assert_eq!(greeting_for(&c, Some(e), 0), dialog::FIRST_GREETINGS_AFTER_UPLIFT);
        note_greeting(&mut c, dialog::FIRST_GREETINGS_AFTER_UPLIFT, Some(e), 0);
        assert_eq!(greeting_for(&c, Some(e), 0), dialog::GREETINGS);
        // The colonists: first visit, after a disaster, after the UFO's loss.
        let colony = crate::space_galaxy::world_id(4, 1);
        assert_eq!(greeting_for(&c, None, colony), dialog::FIRST_COLONY_GREETINGS);
        note_greeting(&mut c, dialog::FIRST_COLONY_GREETINGS, None, colony);
        assert_eq!(greeting_for(&c, None, colony), dialog::GREETINGS);
        c.universe.disaster_greeting = Some((colony, false));
        assert_eq!(greeting_for(&c, None, colony), dialog::AFTER_DISASTER_NOT_HELPED);
        c.universe_clock = 500.0;
        c.universe.last_death = Some(450.0);
        assert_eq!(greeting_for(&c, None, colony), dialog::MYSPECIES_AFTER_DEATH);
        c.universe_clock = 700.0;
        assert_eq!(greeting_for(&c, None, colony), dialog::AFTER_DISASTER_NOT_HELPED, "the death news is stale after 120 s");
        // The universe notes the loss of the UFO and the end of a disaster.
        let mut d = SpaceCampaign::default();
        d.universe_clock = 10.0;
        d.ships_lost = 1;
        crate::space_universe::tick(&mut d, 1.0, None);
        assert_eq!(d.universe.last_death, Some(10.0));
        d.active_distress = Some(crate::space::DistressSignal { system_id: 4, world_id: colony, kind: crate::space::DistressKind::BiohazardOutbreak, time_remaining: 10.0 });
        crate::space_universe::tick(&mut d, 1.0, None);
        d.active_distress = None;
        d.biohazards_cleansed += 1;
        crate::space_universe::tick(&mut d, 1.0, None);
        assert_eq!(d.universe.disaster_greeting, Some((colony, true)));
    }

    #[test]
    fn missions_are_offered_through_their_own_conversation() {
        // Headless: only the kinds of the three conversations below are offerable.
        let _headless = crate::space_galaxy::pin_registered_galaxy(None);
        let mut lib = library();
        let mut mission = Conversation::default();
        let goto = |d: u32| response(action::GOTO, (0, 0, d), &[], &[], 20);
        mission.dialogs.insert(dialog::MISSION_ASSIGN, CnvDialog {
            id: dialog::MISSION_ASSIGN,
            action: action::NONE,
            texts: vec![CnvText { id: 30, table: 8, ..Default::default() }],
            responses: vec![goto(dialog::MISSION_ACCEPT), goto(dialog::MISSION_REJECT)],
            ..Default::default()
        });
        mission.dialogs.insert(dialog::MISSION_ACCEPT, CnvDialog {
            id: dialog::MISSION_ACCEPT,
            action: action::MISSION_ACCEPT,
            texts: vec![CnvText { id: 31, table: 8, ..Default::default() }],
            responses: vec![response(action::CONTINUE, (0, 0, 0), &[], &[], 21)],
            ..Default::default()
        });
        for kind in [rules::MissionKind::FetchAnimal, rules::MissionKind::Terraform, rules::MissionKind::DestroyTurrets] {
            lib.convs.insert(mission_file(kind), Some(Arc::new(mission.clone())));
        }
        lib.strings.insert((8, 30), "Bring us one, for ~mission_rewards~.".into());
        let mut run = RunProgress::default();
        let e = warrior_empire();
        run.space_campaign.universe.contacted.insert(e);
        run.space_campaign.meet_empire(e);
        let env = CommEnv { index: None, ufo_damaged: false };
        let mut s = CommSession { open: true, empire: Some(e), ..Default::default() };
        s.apply(&mut lib, &mut run, &env, action::TRY_MISSION, (0, 0, 0));
        assert_eq!(s.dialog, dialog::MISSION_ASSIGN);
        let reward = s.offered_mission.as_ref().expect("offered").reward_spice;
        assert_eq!(s.text, format!("Bring us one, for {}.", money_text(reward)));
        assert!(run.space_campaign.active_alien_mission.is_none());
        s.choose(&mut lib, &mut run, &env, 0);
        assert_eq!(s.dialog, dialog::MISSION_ACCEPT);
        assert!(run.space_campaign.active_alien_mission.is_some(), "accepting starts the mission");
        s.choose(&mut lib, &mut run, &env, 0);
        assert_eq!(s.dialog, dialog::DEFAULT, "continue returns to the main menu");
    }

    #[test]
    fn a_fetch_missions_texts_name_its_target_star_and_item() {
        let galaxy = crate::space_galaxy::Galaxy::generate(None);
        let _pinned = crate::space_galaxy::pin_registered_galaxy(Some(galaxy.clone()));
        let world = crate::space_galaxy::world_id(12, 1);
        let mut m = AlienMission {
            empire: 1,
            kind: crate::space::AlienMissionKind::Fetch,
            reward_spice: 20_000.0,
            progress: 0,
            target_count: 1,
            target_world: Some(world),
            description: String::new(),
            retail: Some(rules::MissionKind::FetchArtifact),
            time_left: Some(180.0),
            ..Default::default()
        };
        let get = |m: &AlienMission, key: &str| {
            mission_tokens(m, "Home", "HomeStar").into_iter().find(|(k, _)| *k == key).unwrap().1
        };
        assert_eq!(get(&m, "mission_target_star"), galaxy.star_name(12), "the target's star, not the current one");
        assert_eq!(get(&m, "mission_item"), rules::fetch_artifact_item(world));
        assert!(rules::FETCH_ARTIFACT_ITEMS.contains(&get(&m, "mission_item").as_str()));
        m.retail = Some(rules::MissionKind::FetchTribeCitizen);
        assert_eq!(get(&m, "mission_item"), "native");
        m.kind = crate::space::AlienMissionKind::PirateHunt;
        assert_eq!(get(&m, "mission_target_star"), "HomeStar");
        assert_eq!(get(&m, "mission_item"), "specimen");
    }

    /// Every installed conversation parses and the main menu has the dialogs
    /// the session uses.
    #[test]
    #[ignore]
    fn installed_conversations_parse() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut lib = CnvLibrary::default();
        let main = lib.conversation(Some(&index), MAIN_MENU_FILE).expect("main menu");
        for d in [dialog::DEFAULT, dialog::GREETINGS, dialog::FIRST_GREETINGS, dialog::DIPLOMACY, dialog::MISSION_MAIN, dialog::GIVE_GIFT_RELATIONSHIP, dialog::PEACE_SUCCESS, dialog::PURCHASE_SUCCESS] {
            assert!(main.dialogs.contains_key(&d), "{d:08X}");
        }
        assert_eq!(main.dialogs.len(), 58);
        let inc = lib.conversation(Some(&index), INCOMING_FILE).expect("incoming");
        assert!(inc.dialogs.contains_key(&dialog::DEMAND_TRIBUTE));
        let s = lib.string(Some(&index), MAIN_MENU_FILE, 0x7D);
        assert!(s.contains("Captain"), "{s}");
        // The emotion animation table (space-endgame notes E5).
        let anims = lib.conversation(Some(&index), ANIMATIONS_FILE).expect("animations");
        assert_eq!(anims.anims.len(), 8);
        let neutral = &anims.anims[&EMOTION_NEUTRAL];
        assert_eq!(neutral.len(), 12);
        assert_eq!(neutral[0].idle, vec![0x058C_964A, 0x058C_964E, 0x058C_9652, 0x058C_9656]);
    }
}
