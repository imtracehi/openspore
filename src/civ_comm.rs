//! The Civilization communication screen's conversations: the installed
//! `civ_main_menu` conversation (`space_conversations~` 0xDBF385BF, the same
//! `.cnv` format and evaluation as Space, `space_comm`), the Civ comm actions
//! and the rivals' own calls (`notes\civ-rules3-re.md` W2).
//!
//! VERIFIED (march2017 addresses):
//! * actions 0xCFF010: compliment (Compliment x1 -> `npc_complimented`),
//!   insult (Insult x1 -> `npc_insulted`), gifts of 4000 / 2000 / 1000 (paid
//!   when affordable; Gift x amount / 100, a third of it when the player is
//!   Friendly with another nation (0xCFEA90) -> `npc_receive_gift`), buying a
//!   city (five offers graded against the asked price: [ask - 120, ask +
//!   1200) accepted, above that accepted with BuyCityOver x2, [ask - 1200,
//!   ask - 120) rejected with BuyCityUnder x1, below too low with x3),
//!   demands (paid: Gift x amount / 400, half when Friendly elsewhere; refused:
//!   DemandRejected x1), alliance / embargo requests, help against a city
//!   (price 0xCFE980), the trade route offers, surrender (every city joins);
//! * the rivals' calls from `cCivilization::Update` 0xBFBD30: a war warning
//!   when the attitude drops below Cautious (once until it recovers), demands
//!   0xBF75E0, embargo 0xBF13B0 and alliance 0xBF15F0 requests (each needs a
//!   nation of at least two cities, so OpenSpore's one-city nations never
//!   make them), surrender 0xBF32C0 (1 in 20 per second once the player is
//!   developed with vehicle tech above 2: the last rival with one city when
//!   the player holds every other city);
//! * HUD entry 0xBF0950: the comm state 0 / 1 / 2 / other opens the rival's
//!   greeting / `npc_war_warning` / `npc_trade_route_sever_city` /
//!   `player_contact_npc`.
//!
//! INFERRED / ADAPTED (marked where used): the conversation flag builder of
//! the Civ mode was not found (the Space one, 0xAED680, gives the attitude
//! flags); the five buy offers (0.8 / 0.6 / 0.4 / 2 / 1 x the asked price:
//! the flag names `AtLeast3200..4000Money` for a 4000 price); `AcceptsGifts`
//! is always set.
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::civ::{self, Approach, CivCampaign, CITY_COUNT, HOME_INDEX};
use crate::civ_rules::{self, Difficulty, RelationKind};
use crate::space_comm::{self as sc, CnvLibrary, Flags};

/// `civ_main_menu.cnv`.
pub const CIV_FILE: u32 = 0xDBF3_85BF;

/// Dialog ids of `civ_main_menu` (FNV of the names).
pub mod dialog {
    pub const TRADE_ROUTE_OFFER_NPC: u32 = 0x832B_3AA4;
    pub const NPC_ALLIANCE_COUNTERED: u32 = 0x92C5_EBBC;
    pub const PLAYER_CONTACT_NPC: u32 = 0x942B_FB93;
    pub const NPC_EMBARGO_COUNTERED: u32 = 0x9C22_B3AE;
    pub const TRADE_ROUTE_ACCEPTED: u32 = 0x9CEC_AD2C;
    pub const NPC_WAR_WARNING: u32 = 0xA156_6B04;
    pub const CAPTURE_CITY_NPC_GLOAT: u32 = 0xA917_C1C3;
    pub const CAPTURE_FROM_PLAYER_RELIGIOUS: u32 = 0xAEE7_49F0;
    pub const GREETINGS: u32 = 0xB485_84E3;
    pub const NPC_COMPLIMENTED: u32 = 0xCB8B_690E;
    pub const BUY_REJECTED: u32 = 0xCCD9_0D2A;
    pub const CAPTURE_FROM_NPC_RELIGIOUS: u32 = 0xCE44_C220;
    pub const NPC_INSULTED: u32 = 0xCF16_A127;
    pub const NPC_RECEIVE_GIFT: u32 = 0xD297_847D;
    pub const NPC_ATTACK_CITY: u32 = 0xD683_8A11;
    pub const NPC_SURRENDER: u32 = 0xD959_7CC9;
    pub const BUY_OFFER_FROM_PLAYER: u32 = 0xE05D_0481;
    pub const CAPTURE_FROM_PLAYER_MILITARY: u32 = 0xE093_357A;
    pub const BUY_ACCEPTED: u32 = 0xE095_755F;
    pub const NPC_DEMAND_REJECTED: u32 = 0xE883_5715;
    pub const NPC_GIFT: u32 = 0xEE4F_8F7F;
    pub const NPC_DEMAND_MET: u32 = 0xFD02_ED1B;
    pub const BUY_OFFER_FROM_NPC: u32 = 0x0EFA_1507;
    pub const ATTACK_COUNTER_ACCEPTED: u32 = 0x1583_D650;
    pub const NPC_EMBARGO_ACCEPTED: u32 = 0x1695_2A08;
    pub const TRADE_ROUTE_OFFER_NPC_ACCEPT: u32 = 0x17D7_EC7F;
    pub const NPC_ALLIANCE: u32 = 0x1837_1572;
    pub const NPC_EMBARGO: u32 = 0x1AA5_BFF0;
    pub const NPC_ALLIANCE_REJECTED: u32 = 0x1C18_780B;
    pub const NPC_ALLIANCE_ACCEPTED: u32 = 0x1C89_B8DA;
    pub const TRADE_ROUTE_REJECTED: u32 = 0x2654_6F31;
    pub const BUY_TOO_LOW: u32 = 0x28A6_6E39;
    pub const NPC_TRADE_ROUTE_SEVER_CITY: u32 = 0x2AB3_E3AD;
    pub const CAPTURE_FROM_NPC_MILITARY: u32 = 0x2AF3_064A;
    pub const ATTACK_COUNTER: u32 = 0x2FC9_5198;
    pub const NPC_NODE_BOUGHT: u32 = 0x4CE5_B975;
    pub const NPC_EMBARGO_REJECTED: u32 = 0x4DB1_BA15;
    pub const TRADE_ROUTE_OFFER_NPC_REJECT: u32 = 0x5260_EC76;
    pub const BUY_TOO_HIGH: u32 = 0x647F_97AD;
    pub const NPC_SURRENDER_ALLY: u32 = 0x6B6A_D65E;
    pub const NPC_DEMAND: u32 = 0x6EEC_EEF0;
    pub const BUY_CANCELED: u32 = 0x7B10_D7BD;
}

/// Response actions (0xCFF010 compares these FNV ids).
pub mod action {
    pub const GOTO: u32 = 0x02D5_DCEC;
    pub const EXIT: u32 = 0x0199_4745;
    pub const COMPLIMENT: u32 = 0x70C1_4C34;
    pub const INSULT: u32 = 0x26B1_4441;
    /// Large / medium / small gift (4000 / 2000 / 1000).
    pub const GIFTS: [u32; 3] = [0xD648_ED4D, 0x5EA6_D3ED, 0x410C_4A65];
    /// `~offer_1~` .. `~offer_5~` of `buy_city_offer_from_player`.
    pub const BUY_OFFERS: [u32; 5] = [0x7583_30B8, 0x7583_30BB, 0x7583_30BA, 0x7583_30BD, 0x7583_30BC];
    pub const BUY_CANCEL: u32 = 0xA3C1_D501;
    pub const BUY_DONE: u32 = 0x8650_BD5B;
    pub const BUY_DONE_HIGH: u32 = 0x6668_560E;
    pub const BUY_OOPS_LOW: u32 = 0x9650_8A98;
    pub const BUY_OOPS_REJECTED: u32 = 0xBF3B_803A;
    /// `AttackCity1..10` targets.
    pub const ATTACK_CITY: [u32; 10] = [
        0x1864_6F3F, 0x1864_6F3C, 0x1864_6F3D, 0x1864_6F3A, 0x1864_6F3B, 0x1864_6F38, 0x1864_6F39, 0x1864_6F36,
        0x1864_6F37, 0xA51B_201D,
    ];
    pub const ATTACK_PAY: u32 = 0x9138_6D6C;
    pub const ACCEPT_SURRENDER: u32 = 0x3EF0_9234;
    pub const CLOSE_NOTICE: u32 = 0x6BA6_4250;
    pub const TRADE_ACCEPT: u32 = 0xCADA_C8A0;
    pub const TRADE_REJECT: u32 = 0x1125_D6A9;
    pub const ALLIANCE_YES: u32 = 0x24E1_525D;
    pub const ALLIANCE_COUNTER: u32 = 0x9F0F_F4CB;
    pub const ALLIANCE_NO: u32 = 0x0764_C336;
    pub const EMBARGO_YES: u32 = 0xD50A_1A6D;
    pub const EMBARGO_COUNTER: u32 = 0xE797_7907;
    pub const EMBARGO_NO: u32 = 0x7894_18DC;
    pub const DEMAND_PAY: u32 = 0x23A2_D684;
    pub const DEMAND_REFUSE: u32 = 0x764E_A37C;
}

/// Conversation flags the Civ file uses (SporeModder `reg_cnv.txt` order).
pub mod flag {
    pub const ALLIANCE: usize = 5;
    pub const AT_WAR: usize = 6;
    pub const UNDER_ATTACK: usize = 8;
    pub const ATTITUDE_FURIOUS: usize = 63;
    pub const NOT_ENOUGH_MONEY_GIFT: [usize; 3] = [74, 75, 76];
    /// `AtLeast3200/2400/1600/8000/4000Money`: offers 1..5 affordable.
    pub const AT_LEAST_OFFER: [usize; 5] = [77, 78, 79, 80, 81];
    /// PlayerCivMilitary / Religious / Economic.
    pub const PLAYER_CIV: [usize; 3] = [82, 83, 84];
    /// SourceCivMilitary / Religious / Economic.
    pub const SOURCE_CIV: [usize; 3] = [85, 86, 87];
    pub const SOURCE_0_TO_3: usize = 88;
    pub const SOURCE_4_TO_6: usize = 89;
    pub const SOURCE_7_TO_9: usize = 90;
    pub const SOURCE_MORE_THAN_1: usize = 91;
    pub const SOURCE_MORE_THAN_9: usize = 92;
    pub const ATTACK_CITY_1: usize = 101;
    pub const ATTACK_CITY_MONEY: usize = 111;
    pub const ACCEPTS_GIFTS: usize = 147;
}

/// Gift amounts of `npc_gift` (0xCFF010).
pub const GIFT_AMOUNTS: [f32; 3] = [4000.0, 2000.0, 1000.0];
/// INFERRED: the five offers of `buy_city_offer_from_player` as fractions of
/// the asked price (from the enabling flags' names for a 4000 price).
pub const BUY_OFFER_FRACTIONS: [f32; 5] = [0.8, 0.6, 0.4, 2.0, 1.0];
/// Seconds between a rival's demands (0xBF75E0: 70,001 ms), embargo
/// (0xBF13B0: 50,001 ms) and alliance (0xBF15F0: 60,001 ms) requests, and
/// between two trade route refusals shown (0xDCA090: 40,001 ms).
pub const DEMAND_INTERVAL: f32 = 70.0;
pub const EMBARGO_INTERVAL: f32 = 50.0;
pub const ALLIANCE_INTERVAL: f32 = 60.0;
pub const TRADE_REFUSAL_INTERVAL: f32 = 40.0;
/// 0xBF32C0: one chance in 20 per civilization update (1 s).
pub const SURRENDER_ODDS: u32 = 20;
/// 0xBFA7A0: a military target is re-chosen 40 s (40,001 ms) after the
/// last, a cultural one after 50 s; 0xBFB160 sends economic vehicles every
/// 60 s; 0xBF9FB0 raids hostile derricks every 20 s.
pub const ATTACK_INTERVAL_MILITARY: f32 = 40.0;
pub const ATTACK_INTERVAL_CULTURAL: f32 = 50.0;
pub const ECONOMIC_INTERVAL: f32 = 60.0;
pub const DERRICK_RAID_INTERVAL: f32 = 20.0;

/// Seconds between a rival's vehicle builds by its type (the interval of
/// the decision that orders vehicles of that type; INFERRED mapping).
pub fn build_interval(kind: Approach) -> f32 {
    match kind {
        Approach::Military => ATTACK_INTERVAL_MILITARY,
        Approach::Religious => ATTACK_INTERVAL_CULTURAL,
        Approach::Economic => ECONOMIC_INTERVAL,
    }
}

/// 0xCEEEB0: the chance a city wins one citizen back per 30 s.
pub fn counter_conversion_chance(happiness: i32) -> f32 {
    (happiness.clamp(0, 100) as f32 * 0.01).powi(3) + 0.2
}

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

/// Grade of a buy offer (0xCFF010).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuyVerdict {
    Accepted,
    TooHigh,
    Rejected,
    TooLow,
}

impl BuyVerdict {
    /// The dialog the nation answers with.
    pub fn dialog(self) -> u32 {
        match self {
            BuyVerdict::Accepted => dialog::BUY_ACCEPTED,
            BuyVerdict::TooHigh => dialog::BUY_TOO_HIGH,
            BuyVerdict::Rejected => dialog::BUY_REJECTED,
            BuyVerdict::TooLow => dialog::BUY_TOO_LOW,
        }
    }
}

/// 0xCFF010: [ask - 120, ask + 1200) accepted, from ask + 1200 accepted
/// gladly, [ask - 1200, ask - 120) rejected, below that too low.
pub fn buy_verdict(offer: f32, asked: f32) -> BuyVerdict {
    if offer >= asked - 120.0 && offer < asked + 1200.0 {
        BuyVerdict::Accepted
    } else if offer >= asked + 1200.0 {
        BuyVerdict::TooHigh
    } else if offer >= asked - 1200.0 {
        BuyVerdict::Rejected
    } else {
        BuyVerdict::TooLow
    }
}

/// The relationship event of a verdict: BuyCityOver x2, BuyCityUnder x1 /
/// x3 (the original applies the counts -1 / -3 to a row that rises with the
/// count; here the row falls, same result).
pub fn buy_relation(verdict: BuyVerdict) -> Option<(RelationKind, f32)> {
    match verdict {
        BuyVerdict::Accepted => None,
        BuyVerdict::TooHigh => Some((RelationKind::BuyCityOver, 2.0)),
        BuyVerdict::Rejected => Some((RelationKind::BuyCityUnder, 1.0)),
        BuyVerdict::TooLow => Some((RelationKind::BuyCityUnder, 3.0)),
    }
}

/// INFERRED offers for an asked price, rounded to 100.
pub fn buy_offers(asked: f32) -> [f32; 5] {
    BUY_OFFER_FRACTIONS.map(|f| ((asked * f) / 100.0).round() * 100.0)
}

/// 0xCFF010: a gift counts amount / 100 Gift events (integer division), a
/// third of the amount when the player is Friendly with another nation.
pub fn gift_count(amount: f32, friend_elsewhere: bool) -> f32 {
    let mut a = amount.max(0.0) as i64;
    if friend_elsewhere {
        a /= 3;
    }
    (a / 100) as f32
}

/// 0xCFF010: a paid demand counts amount / 400 Gift events, half the amount
/// first when the player is Friendly with another nation.
pub fn demand_count(amount: f32, friend_elsewhere: bool) -> f32 {
    let mut a = amount.max(0.0) as i64;
    if friend_elsewhere {
        a /= 2;
    }
    (a / 400) as f32
}

/// 0xBF75E0: the amount a rival demands: (player derricks + its derricks / 2
/// + two rolls of 0..4 + its cities + the player's cities) x 200 + a tenth of
/// the player's money - 200, at most the player's money, floored to 100;
/// none below 100. (Which derrick count is halved: INFERRED.)
pub fn demand_amount(
    player_derricks: usize,
    npc_derricks: usize,
    rolls: (u32, u32),
    npc_cities: usize,
    player_cities: usize,
    wealth: f32,
) -> Option<f32> {
    let base = (player_derricks as i64
        + npc_derricks as i64 / 2
        + rolls.0 as i64
        + rolls.1 as i64
        + npc_cities as i64
        + player_cities as i64)
        * 200
        + wealth as i64 / 10
        - 200;
    let a = base.min(wealth as i64);
    let a = (a / 100) * 100;
    (a >= 100).then_some(a as f32)
}

/// 0xBF75E0: one chance in N per update: military 16, religious 8, economic
/// 12, x4 on easy, x3 on normal, x2 on hard.
pub fn demand_odds(kind: Approach, difficulty: Difficulty) -> u32 {
    let base = match kind {
        Approach::Military => 16,
        Approach::Religious => 8,
        Approach::Economic => 12,
    };
    base * match difficulty {
        Difficulty::Easy => 4,
        Difficulty::Medium => 3,
        Difficulty::Hard => 2,
    }
}

/// 0xBF15F0 / 0xBF13B0: alliance requests 16 / 12 / 8, embargo requests
/// 12 / 9 / 6 (easy / normal / hard).
pub fn alliance_odds(difficulty: Difficulty) -> u32 {
    match difficulty {
        Difficulty::Easy => 16,
        Difficulty::Medium => 12,
        Difficulty::Hard => 8,
    }
}

pub fn embargo_odds(difficulty: Difficulty) -> u32 {
    match difficulty {
        Difficulty::Easy => 12,
        Difficulty::Medium => 9,
        Difficulty::Hard => 6,
    }
}

/// 0xCFE980: the price of a rival's help against a city: (target cities + 2)
/// x 800 + 400 per attitude step of the helper toward the target, + 800 when
/// the two share a type, + 1600 when the helper's type beats the target's
/// (military > economic > religious > military; which side is which:
/// INFERRED). The extra 1600 for a target with allies is not modelled.
pub fn attack_help_price(target_cities: usize, stance_to_target: usize, helper: Approach, target: Approach) -> f32 {
    let mut p = (target_cities as i64 + 2) * 800 + 400 * stance_to_target.min(4) as i64;
    let beats = matches!(
        (helper, target),
        (Approach::Military, Approach::Economic)
            | (Approach::Religious, Approach::Military)
            | (Approach::Economic, Approach::Religious)
    );
    if helper == target {
        p += 800;
    } else if beats {
        p += 1600;
    }
    p as f32
}

/// 0xCEEF90 for the player's economic vehicle at a city of another nation
/// (VERIFIED, civ-audit-a; it returns 0.0 or 1.0): a nation that is Pleased or
/// Friendly with the player (attitude index >= 3, 0xD00BD0) accepts a trade
/// route; a Cautious one (index 2) only when it is not neutral and no other
/// non-neutral nation likes the player more (the loop over the civilizations,
/// 0xB25C80, refuses when another's 0xD00950 value toward the player is higher
/// than its own: `best_other` is the highest of them); Annoyed and Furious
/// refuse. (The 0xBEF9F0 test, 120 s since the nation's last proposal, belongs
/// to a rival's vehicle at one of the player's cities.)
pub fn accepts_trade_route(stance: usize, value: f32, best_other: f32) -> bool {
    stance >= 3 || (stance == 2 && best_other <= value)
}

/// [`accepts_trade_route`] for a nation of the campaign (the player's economic
/// vehicle at one of its cities).
pub fn trade_route_accepted(campaign: &CivCampaign, nation: usize) -> bool {
    let value = campaign.nations.get(nation).map_or(0.0, |n| n.relation.value());
    accepts_trade_route(stance(value), value, best_other_relation(campaign, nation))
}

/// The highest relationship value with the player among the other nations on
/// the map (the comparison of 0xCEEF90's loop; OpenSpore has no neutral
/// nations, every rival is a non-neutral civilization).
pub fn best_other_relation(campaign: &CivCampaign, nation: usize) -> f32 {
    (1..CITY_COUNT)
        .filter(|n| *n != nation && campaign.nation_alive(*n) && campaign.present(*n))
        .filter_map(|n| campaign.nations.get(n))
        .map(|n| n.relation.value())
        .fold(f32::NEG_INFINITY, f32::max)
}

/// Attitude index 0 Furious .. 4 Friendly of a relationship value (the
/// Space thresholds, `space_comm::stance`).
pub fn stance(value: f32) -> usize {
    sc::stance(value)
}

/// Whether the player is Friendly with a nation other than `nation`
/// (0xCFEA90).
pub fn friend_elsewhere(campaign: &CivCampaign, nation: usize) -> bool {
    (1..campaign.nations.len().min(CITY_COUNT))
        .filter(|n| *n != nation && campaign.nation_alive(*n))
        .any(|n| stance(campaign.nations[n].relation.value()) > 3)
}

// ---------------------------------------------------------------------------
// Rival expansion to the spice geysers (0xBF9960)
// ---------------------------------------------------------------------------

/// 0xBF9960 runs again 10 s (10,001 ms) after the last choice.
pub const EXPANSION_INTERVAL: f32 = 10.0;
/// ADAPTED: geysers farther than this from the capital are ignored (the
/// original picks a building city on the geyser's continent, not modelled).
pub const EXPANSION_REACH: f32 = 2.0 * civ_rules::MIN_DISTANCE_BETWEEN_CITIES;

/// 0xBF9960: (2 - distance / (planet radius x pi)) x (200 for the current
/// target, else 100) x 8 / (claim weight + 1); other non-neutral nations
/// aiming at the geyser weigh 2 each.
pub fn expansion_score(distance: f32, radius: f32, current: bool, weight: u32) -> f32 {
    (2.0 - distance / (radius.max(1.0) * std::f32::consts::PI))
        * if current { 200.0 } else { 100.0 }
        * 8.0
        / (weight as f32 + 1.0)
}

/// The best free geyser of `(owner, distance from the capital, claim
/// weight)`; geysers with a claim weight above 2 are skipped.
pub fn expansion_pick(geysers: &[(Option<usize>, f32, u32)], current: Option<usize>, radius: f32) -> Option<usize> {
    geysers
        .iter()
        .enumerate()
        .filter(|(_, (owner, d, w))| owner.is_none() && *w <= 2 && *d <= EXPANSION_REACH)
        .map(|(i, (_, d, w))| (i, expansion_score(*d, radius, current == Some(i), *w)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// 0xBF9960 on easy: a rival expands only while the player has more
/// derricks than it (which count is which: INFERRED).
pub fn expands_on_easy(player_derricks: usize, npc_derricks: usize) -> bool {
    player_derricks > npc_derricks
}

// ---------------------------------------------------------------------------
// The rivals' attack decision (0xBFA7A0)
// ---------------------------------------------------------------------------
//
// VERIFIED (march2017, `swarm-volume\target\re\dis.py`; `notes\civ-rows3-notes.md` 1):
// `cCivilization::Update` 0xBFBD30 (once a second, non-player civilizations) calls 0xBFA7A0(0) and
// 0xBFA7A0(1) (0xBFBFE1 / 0xBFBFEA), military then cultural. Per call:
// * nothing until the player has developed (+0x8D); on Easy / Medium a rival with more cities than the
//   player (Medium: while the player has never had two) clears both targets and restarts both
//   stopwatches (0xBFA7E9 .. 0xBFA884; `civ_rules::attack_held_back`);
// * the current target (+0x458 military, +0x45C cultural) is dropped when it became the rival's own city or
//   its owner is Cautious or better and not neutral (0xBFA8A1 .. 0xBFA8F8); otherwise it stays until the
//   stopwatch (+0x148 / +0x168) exceeds 40 000 / 50 000 ms (0xBFA96A .. 0xBFA9A8);
// * the choice (0xBFA9AE ..): every city that is not the rival's own, whose owner it is Annoyed or worse with
//   (a neutral owner always qualifies), on Easy a player's city only while the player holds more cities,
//   reachable (tech level 3 reaches everything, below that an own city of the purpose's style stands on the
//   target's continent, 0xBF0250 / 0xBF00D0 / 0xBF01E0), and not already under the vehicles of another
//   civilization with none of the rival's own (0xBE4150) is scored
//     (RandomInt(50) + RandomInt(50) + 50
//        + military (10 - live turrets) x 20 | cultural (100 - mood) x 4
//        - 10 x relationship value (0xD00950))
//     x 10 neutral owner, x 6 reachable, x 4 position valid, x 3 the owner's own target civilization (+0x46C)
//     is the rival, x 2 the current target
//     x (2 - distance / (planet radius x pi)) x 8                       (distance: nearest own city, 0xBF5B10)
//     x 16 on Easy when the owner is not the human;
//   the highest score above 0 wins; the target is stored (also none) and the stopwatch restarts.
// ADAPTED: a rival's attitude toward another rival and neutral civilizations are not modelled (OpenSpore keeps
// one relationship, the player's), so the player's cities are the only candidates; the owner's own target civilization
// (+0x46C, chosen by 0xBF8580) scales every candidate of one owner alike and is left out; the launching
// city and the squad (0xBFAD48 .. 0xBFB0D3: the nearest own city of the purpose's style spawns vehicles while the
// rival can pay) stay with `civ_rts::rts_ai`'s builder; the forced target +0x460 (set by the comm action
// 0xCFF010, cleared 1 in 1000 per second) has no OpenSpore counterpart.

/// The two purposes of 0xBFA7A0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackPurpose {
    /// 0: military, re-chosen 40 s after the last choice.
    Military,
    /// 1: cultural, re-chosen after 50 s.
    Cultural,
}

impl AttackPurpose {
    /// The index of the target slot (`NationState::attack_target`).
    pub fn index(self) -> usize {
        match self {
            AttackPurpose::Military => 0,
            AttackPurpose::Cultural => 1,
        }
    }

    /// The stopwatch limit in seconds (0xBFA983: 0x9C40 ms, 0xBFA9A3: 0xC350 ms).
    pub fn interval(self) -> f32 {
        match self {
            AttackPurpose::Military => ATTACK_INTERVAL_MILITARY,
            AttackPurpose::Cultural => ATTACK_INTERVAL_CULTURAL,
        }
    }

    /// The purpose of a nation's vehicles: military 0, religious (cultural) 1; economic nations send
    /// trade vehicles (0xBFB160), not attackers.
    pub fn of(kind: Approach) -> Option<Self> {
        match kind {
            Approach::Military => Some(AttackPurpose::Military),
            Approach::Religious => Some(AttackPurpose::Cultural),
            Approach::Economic => None,
        }
    }

    /// The city style that launches the purpose (`cCity` +0x540 == the purpose: 0 castle = military,
    /// 1 shrine = religious; 0xBD8320 at 0xBFADA1).
    pub fn style(self) -> Approach {
        match self {
            AttackPurpose::Military => Approach::Military,
            AttackPurpose::Cultural => Approach::Religious,
        }
    }
}

/// What 0xBFA7A0 reads of one candidate city.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackFacts {
    /// The city's owner is the human player (+0x89).
    pub owner_is_human: bool,
    /// The owner is a neutral civilization (+0x8A).
    pub owner_neutral: bool,
    /// Live turrets (0xBD9CC0 counts the turrets whose combatant state is 0).
    pub live_turrets: usize,
    /// The city's mood (`cCity` +0x304, 0xBD7E50).
    pub happiness: f32,
    /// The rival's relationship value toward the owner (0xD00950).
    pub relationship: f32,
    /// An own city of the purpose's style stands on the target's continent (0xBF0250).
    pub reachable: bool,
    /// The owner's own target civilization is the rival (`+0x46C`).
    pub owner_targets_me: bool,
    /// Distance from the nearest own city (0xBF5CD0, the map 0xBF5B10 fills).
    pub distance: f32,
    /// The city is the current target.
    pub current: bool,
}

/// The score of 0xBFA7A0 (0xBFAB4B .. 0xBFAD1E); `rolls` are the two `RandomInt(50)` draws in order.
pub fn attack_score(
    f: &AttackFacts,
    purpose: AttackPurpose,
    radius: f32,
    difficulty: Difficulty,
    rolls: (u32, u32),
) -> f32 {
    let mut s = (rolls.0 + rolls.1 + 50) as f32;
    match purpose {
        AttackPurpose::Military => s += (10.0 - f.live_turrets as f32) * 20.0,
        AttackPurpose::Cultural => s += (100.0 - f.happiness) * 4.0,
    }
    s -= 10.0 * f.relationship;
    if f.owner_neutral {
        s *= 10.0;
    }
    if f.reachable {
        s *= 6.0;
    }
    // 0x41DCF0(position, 0x168C514): the position differs from the "none" vector: always.
    s *= 4.0;
    if f.owner_targets_me {
        s *= 3.0;
    }
    if f.current {
        s *= 2.0;
    }
    s = (2.0 - f.distance / (radius.max(1.0) * std::f32::consts::PI)) * s * 8.0;
    if difficulty == Difficulty::Easy && !f.owner_is_human {
        s *= 16.0;
    }
    s
}

/// The decision of one call of 0xBFA7A0 for `nation`. `rolls(n)` is `RandomInt(n)`; `orders` lists the
/// `(nation, order)` of every vehicle (the "already under attack" test of 0xBE4150). Returns the target
/// after the call. The caller decrements `NationState::raid` (the stopwatch) every second.
pub fn attack_decision(
    campaign: &mut CivCampaign,
    nation: usize,
    purpose: AttackPurpose,
    difficulty: Difficulty,
    orders: &[(usize, civ::UnitOrder)],
    rolls: &mut dyn FnMut(u32) -> u32,
) -> Option<usize> {
    let slot = purpose.index();
    if nation == HOME_INDEX || nation >= campaign.nations.len() || !campaign.nation_alive(nation) {
        return None;
    }
    // 0xBFA7D1: nothing starts until the player has developed.
    if !campaign.player_developed {
        return campaign.nations[nation].attack_target[slot];
    }
    let player_cities = campaign.owned_count();
    let rival_cities = campaign.cities.iter().filter(|c| !c.owned() && c.nation() == nation).count();
    let had_two = player_cities >= 2
        || campaign.captures.military + campaign.captures.religious + campaign.captures.economic > 0;
    // 0xBFA7FF .. 0xBFA884: the hold-back clears both targets and restarts both stopwatches.
    if civ_rules::attack_held_back(difficulty, player_cities, rival_cities, had_two, false) {
        let n = &mut campaign.nations[nation];
        n.attack_target = [None, None];
        n.raid = purpose.interval();
        return None;
    }
    // The attitude toward a city's owner: the player's from the relationship; another rival's is
    // Cautious (ADAPTED: rival-to-rival attitudes are not modelled).
    let attitude = |c: &CivCampaign, owner: usize| -> usize {
        if owner == HOME_INDEX {
            stance(c.nations[nation].relation.value())
        } else {
            2
        }
    };
    // 0xBFA8A1 .. 0xBFA8F8: the target goes when it is the rival's own city or its owner is Cautious+.
    if let Some(t) = campaign.nations[nation].attack_target[slot] {
        let owner = campaign.cities.get(t).map(|c| c.nation());
        if owner.is_none_or(|o| o == nation || attitude(campaign, o) > 1) {
            campaign.nations[nation].attack_target[slot] = None;
        } else if campaign.nations[nation].raid > 0.0 {
            return Some(t);
        }
    } else if campaign.nations[nation].raid > 0.0 {
        // No target and the stopwatch still running: nothing to decide yet.
        return None;
    }
    let current = campaign.nations[nation].attack_target[slot];
    let radius = campaign.sphere.map_or(500.0, |s| s[3]);
    let level = civ_rules::vehicle_tech_level(rival_cities, CITY_COUNT);
    let own: Vec<usize> = campaign.cities.iter().filter(|c| c.nation() == nation).map(|c| c.index).collect();
    let mut best: Option<(usize, f32)> = None;
    for c in campaign.cities.iter() {
        if c.nation() == nation || !campaign.present(c.index) {
            continue;
        }
        let owner = c.nation();
        // 0xBFAAC1 .. 0xBFAAD9: an owner it is not Annoyed with qualifies only when neutral (none here).
        if attitude(campaign, owner) > 1 {
            continue;
        }
        // 0xBFAA5F .. 0xBFAA9A: on Easy the player's city only while the player holds more cities.
        if owner == HOME_INDEX && difficulty == Difficulty::Easy && player_cities <= rival_cities {
            continue;
        }
        // 0xBF0250: an own city of the purpose's style on the target's continent.
        let continent = campaign.continents.get(c.index).copied();
        let reachable = own.iter().any(|i| {
            campaign.cities[*i].kind == purpose.style()
                && (campaign.continents.get(*i).copied() == continent)
        });
        // 0xBFAADF .. 0xBFAB25: tech level 3 reaches every continent, below it only the reachable.
        if !reachable && level < civ_rules::AIR_TECH_LEVEL {
            continue;
        }
        // 0xBFAB2B: a city that vehicles of another civilization are already working on, and none of
        // the rival's own, is left alone (0xBE4150 with 1, then 0).
        let on_city = |mine: bool| {
            orders
                .iter()
                .any(|(n, o)| (*n == nation) == mine && matches!(o, civ::UnitOrder::City(t) if *t == c.index))
        };
        if !on_city(true) && on_city(false) {
            continue;
        }
        let distance = own
            .iter()
            .map(|i| campaign.distance(campaign.cities[*i].center(), c.center()))
            .fold(f32::INFINITY, f32::min);
        let facts = AttackFacts {
            owner_is_human: owner == HOME_INDEX,
            owner_neutral: false,
            live_turrets: c.live_turrets(),
            happiness: c.happiness() as f32,
            relationship: if owner == HOME_INDEX { campaign.nations[nation].relation.value() } else { 0.0 },
            reachable,
            owner_targets_me: false,
            distance: if distance.is_finite() { distance } else { 0.0 },
            current: current == Some(c.index),
        };
        let a = rolls(50);
        let b = rolls(50);
        let score = attack_score(&facts, purpose, radius, difficulty, (a, b));
        if score > best.map_or(0.0, |(_, s)| s) {
            best = Some((c.index, score));
        }
    }
    let n = &mut campaign.nations[nation];
    n.attack_target[slot] = best.map(|(i, _)| i);
    n.raid = purpose.interval();
    n.attack_target[slot]
}

// ---------------------------------------------------------------------------
// Rival hut hunting (0xBF8850)
// ---------------------------------------------------------------------------
//
// VERIFIED (`notes\civ-rows3-notes.md` 3): `cCivilization::Update` calls 0xBF8850 (0xBFBFF5) for every non-player
// civilization once a second. With `RandomInt(10) == 0` it walks every goodie hut (the tribes of type 0x1E4DAAE with
// the taken flag +0x557 clear): a hut whose continent holds one of the civilization's cities (0xBF01E0(continent, -1))
// and that none of its vehicles is already driving to (a type 8 order whose target is the hut) gets the first of
// its vehicles that is not busy defending (0xC9F5A0), can reach the hut (0xC9EAF0) and has no order (type 0): the
// capture order `0xCAC1A0(vehicle, hut, 8, 0)`. One vehicle per hut per call.
// ADAPTED: 0xC9F5A0 (the vehicle's home object is under attack) is not modelled; a vehicle that is idle for
// `HUNT_IDLE` seconds counts as free (the idle gate of the other rival orders).

/// 1 in `HUT_HUNT_ODDS` calls (once a second) hunts (0xBF8864 `RandomInt(10)`).
pub const HUT_HUNT_ODDS: u32 = 10;

/// The orders of one hunt call: `(vehicle id, hut index)`. `huts` lists the standing huts with whether a city
/// of `nation` stands on the hut's continent; `vehicles` are the rival's own vehicles; `can_reach(id, hut)` is
/// the vehicle's reach test.
pub fn hut_hunt(
    huts: &[(usize, bool)],
    vehicles: &[(u32, bool, Option<usize>)],
    can_reach: &dyn Fn(u32, usize) -> bool,
) -> Vec<(u32, usize)> {
    let mut orders: Vec<(u32, usize)> = Vec::new();
    let mut taken: Vec<u32> = Vec::new();
    for (hut, on_continent) in huts {
        if !on_continent {
            continue;
        }
        // 0xBF8920 .. 0xBF893C: a vehicle already on its way (or ordered earlier in this call).
        if vehicles.iter().any(|(_, _, h)| *h == Some(*hut)) || orders.iter().any(|(_, h)| h == hut) {
            continue;
        }
        // 0xBF8960 .. 0xBF89A5: the first free vehicle that reaches it.
        if let Some((id, _, _)) = vehicles
            .iter()
            .find(|(id, free, _)| *free && !taken.contains(id) && can_reach(*id, *hut))
        {
            taken.push(*id);
            orders.push((*id, *hut));
        }
    }
    orders
}

// ---------------------------------------------------------------------------
// Persisted state and the rivals' calls
// ---------------------------------------------------------------------------

/// A conversation a rival starts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Incoming {
    pub nation: usize,
    pub dialog: u32,
    pub offer: f32,
}

/// What the Civ comm code keeps between saves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommLedger {
    /// `mCommNotifiedWarning` per nation.
    pub warned: Vec<bool>,
    /// `mSurrendered` per nation.
    pub surrendered: Vec<bool>,
    /// Seconds since each nation's last demand.
    pub demand_wait: Vec<f32>,
    /// Calls waiting for the screen.
    pub incoming: Vec<Incoming>,
    /// Owned cities and capture counts at the last look (capture calls).
    pub owned: Vec<bool>,
    pub captures: [u32; 3],
    /// Pseudo-random state of the rivals' rolls.
    pub seed: u32,
    /// Seconds until a trade refusal may be shown again (0xDCA090: 40 s).
    pub refusal_wait: f32,
    /// Seconds since each city's last counter-conversion (0xBE1340).
    pub counter_wait: Vec<f32>,
}

impl CommLedger {
    fn ensure(&mut self) {
        for v in [&mut self.warned, &mut self.surrendered, &mut self.owned] {
            if v.len() < CITY_COUNT {
                v.resize(CITY_COUNT, false);
            }
        }
        if self.demand_wait.len() < CITY_COUNT {
            self.demand_wait.resize(CITY_COUNT, DEMAND_INTERVAL);
        }
        if self.counter_wait.len() < CITY_COUNT {
            self.counter_wait.resize(CITY_COUNT, 0.0);
        }
        if self.seed == 0 {
            self.seed = 0x2545_F491;
        }
    }

    fn roll(&mut self, n: u32) -> u32 {
        // xorshift32
        let mut x = self.seed.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        if n == 0 {
            0
        } else {
            x % n
        }
    }

    pub fn queue(&mut self, call: Incoming) {
        if !self.incoming.iter().any(|c| c.nation == call.nation && c.dialog == call.dialog) {
            self.incoming.push(call);
        }
    }
}

/// One civilization update (1 s) of the rivals' calls toward the player
/// (0xBFBD30 tail): war warnings, demands, surrender, plus the capture notes.
pub fn rivals_call(campaign: &mut CivCampaign, money: f32, difficulty: Difficulty) {
    let mut ledger = std::mem::take(&mut campaign.comm);
    ledger.ensure();
    let player_cities = campaign.owned_count();
    let total = campaign.cities.len();
    // Capture notes: the city's former nation speaks (by the method used).
    let counts = [campaign.captures.military, campaign.captures.economic, campaign.captures.religious];
    for (i, city) in campaign.cities.iter().enumerate() {
        if city.owned() && !ledger.owned.get(i).copied().unwrap_or(true) && i != HOME_INDEX {
            let religious = counts[2] > ledger.captures[2];
            let dialog = if religious { dialog::CAPTURE_FROM_NPC_RELIGIOUS } else { dialog::CAPTURE_FROM_NPC_MILITARY };
            if counts[1] <= ledger.captures[1] {
                ledger.queue(Incoming { nation: i, dialog, offer: 0.0 });
            }
        }
    }
    ledger.refusal_wait = (ledger.refusal_wait - civ_rules::CIV_UPDATE_SECONDS).max(0.0);
    // Counter-conversion (0xBE1340): every `CounterConversionInterval` (30 s,
    // the stopwatch of every city runs on) the owner of each city wins back, with
    // the chance (happiness / 100)^3 + 0.2 (0xCEEEB0), one citizen from each
    // faith holding some (`CityState::counter_conversion`): the same for your
    // cities and the rivals'.
    for (i, city) in campaign.cities.iter_mut().enumerate().take(CITY_COUNT) {
        ledger.counter_wait[i] += civ_rules::CIV_UPDATE_SECONDS;
        if ledger.counter_wait[i] >= civ_rules::COUNTER_CONVERSION_INTERVAL {
            ledger.counter_wait[i] = 0.0;
            let mood = city.happiness();
            city.counter_conversion(mood, || ledger.roll(10_000) as f32 / 10_000.0);
        }
    }
    ledger.owned = campaign.cities.iter().map(|c| c.owned()).collect();
    ledger.owned.resize(CITY_COUNT, false);
    ledger.captures = counts;
    let tech = campaign.vehicle_tech_level();
    for nation in 1..total.min(CITY_COUNT) {
        if !campaign.nation_alive(nation) {
            continue;
        }
        let value = campaign.nations.get(nation).map_or(0.0, |n| n.relation.value());
        let st = stance(value);
        // War warning: Annoyed or worse, once until the attitude recovers.
        if st < 2 {
            if !ledger.warned[nation] {
                ledger.warned[nation] = true;
                ledger.queue(Incoming { nation, dialog: dialog::NPC_WAR_WARNING, offer: 0.0 });
            }
        } else {
            ledger.warned[nation] = false;
        }
        // Demands (0xBF75E0): two cities at least, the player developed, not
        // at war, not Friendly.
        ledger.demand_wait[nation] += civ_rules::CIV_UPDATE_SECONDS;
        let npc_cities = campaign.cities.iter().filter(|c| c.nation() == nation).count();
        if npc_cities >= 2
            && campaign.player_developed
            && !campaign.at_war(nation)
            && st != 4
            && ledger.demand_wait[nation] > DEMAND_INTERVAL
        {
            let rolls = (ledger.roll(5), ledger.roll(5));
            let kind = campaign.cities[nation].kind;
            if let Some(amount) = demand_amount(
                campaign.derricks(HOME_INDEX),
                campaign.derricks(nation),
                rolls,
                npc_cities,
                player_cities,
                money,
            ) {
                ledger.demand_wait[nation] = 0.0;
                if ledger.roll(demand_odds(kind, difficulty)) == 0 {
                    ledger.queue(Incoming { nation, dialog: dialog::NPC_DEMAND, offer: amount });
                }
            }
        }
        // Surrender (0xBF32C0): the last rival city once the player holds
        // every other one, developed and past vehicle tech level 2.
        if !ledger.surrendered[nation]
            && campaign.player_developed
            && tech > 2
            && !campaign.superweapon_launched
            && npc_cities == 1
            && player_cities + 1 == total
            && ledger.roll(SURRENDER_ODDS) == 0
        {
            ledger.surrendered[nation] = true;
            ledger.queue(Incoming { nation, dialog: dialog::NPC_SURRENDER, offer: 0.0 });
        }
    }
    campaign.comm = ledger;
}

// ---------------------------------------------------------------------------
// The open conversation
// ---------------------------------------------------------------------------

/// The shown conversation (the HUD draws it, `civ_hud::comm`).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CivComm {
    pub open: bool,
    pub nation: usize,
    pub dialog: u32,
    pub text: String,
    pub emotion: u32,
    pub responses: Vec<sc::ShownResponse>,
    /// `~offer_1~` .. `~offer_5~`.
    pub offers: [f32; 5],
    /// The asked price of the city being bought (`cCivilization` +0x448).
    pub asked: f32,
    /// The city spoken about (bought, traded with, attacked).
    pub city: Option<usize>,
    /// Cities a helper could attack (`AttackCity1..10`, 0xCFEF60).
    pub targets: Vec<usize>,
    /// A rolling number for the text draws.
    pub roll: u32,
}

/// What a session needs from the game besides the campaign.
pub struct CommEnv<'a> {
    pub index: Option<&'a crate::asset_catalog::ResourceIndex>,
    pub difficulty: Difficulty,
}

/// Flags of the session (see the module notes for what is INFERRED).
pub fn civ_flags(campaign: &CivCampaign, nation: usize, money: f32, comm: &CivComm) -> Flags {
    use flag::*;
    let mut f = Flags::default();
    let value = campaign.nations.get(nation).map_or(0.0, |n| n.relation.value());
    // Attitude flags as the Space builder sets them (0xAED680).
    let attitude: &[usize] = match stance(value) {
        0 => &[63, 71, 72, 73],
        1 => &[64, 68, 71, 72, 73],
        2 => &[65, 68, 69, 72, 73],
        3 => &[66, 68, 69, 70, 73],
        _ => &[67, 68, 69, 70],
    };
    for i in attitude {
        f.set(*i);
    }
    let at_war = campaign.at_war(nation);
    if at_war {
        f.set(AT_WAR);
    }
    let kind_index = |k: Approach| match k {
        Approach::Military => 0,
        Approach::Religious => 1,
        Approach::Economic => 2,
    };
    f.set(PLAYER_CIV[kind_index(campaign.approach)]);
    if let Some(city) = campaign.cities.get(nation) {
        f.set(SOURCE_CIV[kind_index(city.kind)]);
    }
    let cities = campaign.cities.iter().filter(|c| c.nation() == nation).count();
    f.set(match cities {
        0..=3 => SOURCE_0_TO_3,
        4..=6 => SOURCE_4_TO_6,
        7..=9 => SOURCE_7_TO_9,
        _ => SOURCE_MORE_THAN_9,
    });
    if cities > 1 {
        f.set(SOURCE_MORE_THAN_1);
    }
    for (i, amount) in GIFT_AMOUNTS.iter().enumerate() {
        if money < *amount {
            f.set(NOT_ENOUGH_MONEY_GIFT[i]);
        }
    }
    for (i, offer) in comm.offers.iter().enumerate() {
        if *offer > 0.0 && money >= *offer {
            f.set(AT_LEAST_OFFER[i]);
        }
    }
    for i in 0..comm.targets.len().min(10) {
        f.set(ATTACK_CITY_1 + i);
    }
    if comm.offers[0] > 0.0 && money >= comm.offers[0] {
        f.set(ATTACK_CITY_MONEY);
    }
    // ADAPTED: every nation takes gifts (a gift is the way out of a war).
    f.set(ACCEPTS_GIFTS);
    f
}

/// Group thousands (the texts say "Sporebucks" after the number).
pub fn amount_text(amount: f32) -> String {
    let digits = (amount.max(0.0).round() as u64).to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Replace the Civ `~token~` markers.
pub fn substitute(text: &str, campaign: &CivCampaign, comm: &CivComm) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut rest = text;
    let nation_name = |n: usize| format!("{} nation", civ::city_label(n));
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
            "target_city_name" => civ::city_label(comm.city.unwrap_or(comm.nation)),
            "source_city_name" => civ::city_label(comm.nation),
            "player_name_nation" => nation_name(HOME_INDEX),
            "npc1_name_nation" => comm.targets.first().map(|c| nation_name(*c)).unwrap_or_default(),
            s if s.starts_with("offer_") => {
                let i = s[6..].parse::<usize>().unwrap_or(1).clamp(1, 5) - 1;
                amount_text(comm.offers[i])
            }
            s if s.starts_with("city_name_") => {
                let i = s[10..].parse::<usize>().unwrap_or(1).clamp(1, 10) - 1;
                comm.targets.get(i).map(|c| civ::city_label(*c)).unwrap_or_default()
            }
            s if s.starts_with("source_") => civ::city_label(comm.city.unwrap_or(comm.nation)),
            _ => String::new(),
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    let _ = campaign;
    out
}

/// What a choice leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Show(u32),
    Close,
    Stay,
}

impl CivComm {
    /// Open a conversation with a rival at a dialog.
    pub fn open(&mut self, lib: &mut CnvLibrary, campaign: &CivCampaign, money: f32, env: &CommEnv, nation: usize, dialog: u32) {
        let roll = self.roll;
        *self = CivComm { open: true, nation, roll, ..Default::default() };
        self.show(lib, campaign, money, env, dialog);
    }

    /// Open the player's call to a rival (0xBF0950, `player_contact_npc`).
    pub fn contact(&mut self, lib: &mut CnvLibrary, campaign: &CivCampaign, money: f32, env: &CommEnv, nation: usize) {
        self.open(lib, campaign, money, env, nation, dialog::PLAYER_CONTACT_NPC);
    }

    /// Open a rival's call.
    pub fn incoming(&mut self, lib: &mut CnvLibrary, campaign: &CivCampaign, money: f32, env: &CommEnv, call: Incoming) {
        let roll = self.roll;
        *self = CivComm { open: true, nation: call.nation, roll, ..Default::default() };
        self.offers[0] = call.offer;
        self.show(lib, campaign, money, env, call.dialog);
    }

    /// Open the city purchase haggle (`buy_city_offer_from_player`): only
    /// with a trade route (R6: no trade entry, not for sale).
    pub fn buy(&mut self, lib: &mut CnvLibrary, campaign: &CivCampaign, money: f32, env: &CommEnv, city: usize) -> Result<(), String> {
        let Some(c) = campaign.cities.get(city) else {
            return Err("no such city".into());
        };
        if c.owned() {
            return Err(format!("{} is already yours", c.label()));
        }
        if !c.trade {
            return Err(format!("{} is not for sale: open a trade route first", c.label()));
        }
        let asked = c.buy_price();
        let roll = self.roll;
        *self = CivComm { open: true, nation: c.nation(), roll, asked, city: Some(city), ..Default::default() };
        self.offers = buy_offers(asked);
        self.show(lib, campaign, money, env, dialog::BUY_OFFER_FROM_PLAYER);
        Ok(())
    }

    pub fn close(&mut self) {
        let roll = self.roll;
        *self = CivComm { roll, ..Default::default() };
    }

    /// Show a dialog of the Civ file: draw its text with the flags, list its
    /// visible responses.
    pub fn show(&mut self, lib: &mut CnvLibrary, campaign: &CivCampaign, money: f32, env: &CommEnv, id: u32) {
        self.dialog = id;
        self.roll = self.roll.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let Some(conv) = lib.conversation(env.index, CIV_FILE) else {
            self.text.clear();
            self.responses.clear();
            return;
        };
        let Some(d) = conv.dialogs.get(&id) else {
            self.text.clear();
            self.responses.clear();
            return;
        };
        let flags = civ_flags(campaign, self.nation, money, self);
        let picked = sc::pick_text(&d.texts, &flags, self.roll >> 16).cloned();
        self.text = picked
            .as_ref()
            .map(|t| substitute(&lib.string(env.index, t.table, t.id), campaign, self))
            .unwrap_or_default();
        self.emotion = picked.map_or(0, |t| t.emotion);
        let shown: Vec<(sc::CnvResponse, bool)> =
            sc::visible_responses(d, &flags).into_iter().map(|(r, on)| (r.clone(), on)).collect();
        self.responses = shown
            .into_iter()
            .map(|(r, enabled)| {
                let label = sc::pick_text(&r.texts, &flags, self.roll >> 8)
                    .map(|t| substitute(&lib.string(env.index, t.table, t.id), campaign, self))
                    .unwrap_or_default();
                sc::ShownResponse { label, action: r.action, key: r.key, enabled }
            })
            .collect();
    }

    /// Choose a response: run its action, then show what follows.
    pub fn choose(&mut self, lib: &mut CnvLibrary, campaign: &mut CivCampaign, money: &mut f32, env: &CommEnv, i: usize) -> Next {
        let Some(r) = self.responses.get(i).cloned().filter(|r| r.enabled) else {
            return Next::Stay;
        };
        let next = self.apply(campaign, money, env, r.action, r.key);
        match next {
            Next::Show(d) => self.show(lib, campaign, *money, env, d),
            Next::Close => self.close(),
            Next::Stay => {}
        }
        next
    }

    /// 0xCFF010 (and the generic goto / exit of 0xAEB810).
    pub fn apply(&mut self, campaign: &mut CivCampaign, money: &mut f32, env: &CommEnv, id: u32, key: (u32, u32, u32)) -> Next {
        let nation = self.nation;
        let relation = |campaign: &mut CivCampaign, n: usize, kind: RelationKind, count: f32| {
            if let Some(state) = campaign.nations.get_mut(n) {
                state.relation.apply_count(kind, count);
            }
        };
        match id {
            action::GOTO => {
                if key.0 == CIV_FILE || key.0 == 0 {
                    // The request-attack menu lists the cities a helper could hit.
                    if key.2 == dialog::NPC_ATTACK_CITY {
                        self.targets = attack_targets(campaign, nation);
                    }
                    Next::Show(key.2)
                } else {
                    Next::Close
                }
            }
            action::EXIT => Next::Close,
            action::COMPLIMENT => {
                relation(campaign, nation, RelationKind::Compliment, 1.0);
                Next::Show(dialog::NPC_COMPLIMENTED)
            }
            action::INSULT => {
                relation(campaign, nation, RelationKind::Insult, 1.0);
                Next::Show(dialog::NPC_INSULTED)
            }
            a if action::GIFTS.contains(&a) => {
                let i = action::GIFTS.iter().position(|g| *g == a).unwrap_or(0);
                let amount = GIFT_AMOUNTS[i];
                if *money >= amount {
                    *money -= amount;
                    let friend = friend_elsewhere(campaign, nation);
                    if let Some(n) = campaign.nations.get_mut(nation) {
                        n.spice = (n.spice + amount).min(civ_rules::WEALTH_CAP);
                    }
                    relation(campaign, nation, RelationKind::Gift, gift_count(amount, friend));
                    campaign.message = format!("{} accepts your gift of {}", civ::city_label(nation), amount_text(amount));
                }
                Next::Show(dialog::NPC_RECEIVE_GIFT)
            }
            a if action::BUY_OFFERS.contains(&a) => {
                let i = action::BUY_OFFERS.iter().position(|g| *g == a).unwrap_or(0);
                let offer = self.offers[i];
                if *money < offer {
                    return Next::Close;
                }
                let verdict = buy_verdict(offer, self.asked);
                if let Some((kind, count)) = buy_relation(verdict) {
                    relation(campaign, nation, kind, count);
                }
                // The offer made is kept for the accepting answer.
                self.offers = [offer; 5];
                Next::Show(verdict.dialog())
            }
            action::BUY_DONE | action::BUY_DONE_HIGH => {
                let offer = self.offers[0];
                if let Some(city) = self.city.and_then(|c| campaign.cities.get_mut(c)) {
                    if *money >= offer && !city.owned() {
                        *money -= offer;
                        // The campaign hands a fully paid city over (economic
                        // capture, `conquest_of`).
                        city.spent = city.buy_price();
                        campaign.message = format!("{} is bought for {}", city.label(), amount_text(offer));
                    }
                }
                Next::Close
            }
            action::BUY_CANCEL | action::BUY_OOPS_LOW | action::BUY_OOPS_REJECTED | action::CLOSE_NOTICE => Next::Close,
            a if action::ATTACK_CITY.contains(&a) => {
                let i = action::ATTACK_CITY.iter().position(|g| *g == a).unwrap_or(0);
                let Some(&target) = self.targets.get(i) else {
                    return Next::Close;
                };
                let helper = campaign.cities.get(nation).map_or(Approach::Military, |c| c.kind);
                let target_kind = campaign.cities.get(target).map_or(Approach::Military, |c| c.kind);
                let target_nation = campaign.cities.get(target).map_or(target, |c| c.nation());
                let target_cities = campaign.cities.iter().filter(|c| c.nation() == target_nation).count();
                let st = stance(campaign.nations.get(target_nation).map_or(0.0, |n| n.relation.value()));
                self.offers[0] = attack_help_price(target_cities, st, helper, target_kind);
                self.city = Some(target);
                Next::Show(dialog::ATTACK_COUNTER)
            }
            action::ATTACK_PAY => {
                let price = self.offers[0];
                if *money >= price {
                    *money -= price;
                    if let Some(target) = self.city {
                        let target_nation = campaign.cities.get(target).map_or(target, |c| c.nation());
                        relation(campaign, target_nation, RelationKind::DeclaredWar, 1.0);
                    }
                    Next::Show(dialog::ATTACK_COUNTER_ACCEPTED)
                } else {
                    Next::Close
                }
            }
            action::ACCEPT_SURRENDER => {
                // Every city of the nation joins the player (economic capture
                // path of the campaign, 0xBE9E00).
                for city in campaign.cities.iter_mut().filter(|c| c.nation() == nation && !c.owned()) {
                    city.trade = true;
                    city.spent = city.buy_price();
                }
                campaign.message = format!("{} surrenders and joins you", civ::city_label(nation));
                Next::Close
            }
            action::TRADE_ACCEPT | action::TRADE_REJECT => Next::Close,
            action::ALLIANCE_YES => {
                relation(campaign, nation, RelationKind::JoinedAlliance, 5.0);
                if let Some(&enemy) = self.targets.first() {
                    relation(campaign, enemy, RelationKind::DeclaredWar, 1.0);
                }
                Next::Show(dialog::NPC_ALLIANCE_ACCEPTED)
            }
            action::ALLIANCE_COUNTER => {
                let paid = campaign.nations.get(nation).map_or(0.0, |n| n.spice.min(2000.0));
                if let Some(n) = campaign.nations.get_mut(nation) {
                    n.spice -= paid;
                }
                *money = (*money + paid).min(civ_rules::WEALTH_CAP);
                relation(campaign, nation, RelationKind::JoinedAlliance, 1.0);
                if let Some(&enemy) = self.targets.first() {
                    relation(campaign, enemy, RelationKind::DeclaredWar, 1.0);
                }
                Next::Show(dialog::NPC_ALLIANCE_COUNTERED)
            }
            action::ALLIANCE_NO => {
                relation(campaign, nation, RelationKind::DemandRejected, 1.0);
                Next::Show(dialog::NPC_ALLIANCE_REJECTED)
            }
            action::EMBARGO_YES | action::EMBARGO_COUNTER => {
                if let Some(&partner) = self.targets.first() {
                    if let Some(c) = campaign.cities.get_mut(partner) {
                        c.trade = false;
                    }
                    relation(campaign, partner, RelationKind::BrokeDeal, 1.0);
                }
                if id == action::EMBARGO_YES {
                    relation(campaign, nation, RelationKind::JoinedAlliance, 2.0);
                    Next::Show(dialog::NPC_EMBARGO_ACCEPTED)
                } else {
                    let paid = campaign.nations.get(nation).map_or(0.0, |n| n.spice.min(4000.0));
                    if let Some(n) = campaign.nations.get_mut(nation) {
                        n.spice -= paid;
                    }
                    *money = (*money + paid).min(civ_rules::WEALTH_CAP);
                    Next::Show(dialog::NPC_EMBARGO_COUNTERED)
                }
            }
            action::EMBARGO_NO => {
                relation(campaign, nation, RelationKind::DemandRejected, 1.0);
                Next::Show(dialog::NPC_EMBARGO_REJECTED)
            }
            action::DEMAND_PAY => {
                let amount = self.offers[0];
                if *money >= amount {
                    *money -= amount;
                    let friend = friend_elsewhere(campaign, nation);
                    if let Some(n) = campaign.nations.get_mut(nation) {
                        n.spice = (n.spice + amount).min(civ_rules::WEALTH_CAP);
                    }
                    relation(campaign, nation, RelationKind::Gift, demand_count(amount, friend));
                }
                Next::Show(dialog::NPC_DEMAND_MET)
            }
            action::DEMAND_REFUSE => {
                relation(campaign, nation, RelationKind::DemandRejected, 1.0);
                Next::Show(dialog::NPC_DEMAND_REJECTED)
            }
            _ => {
                let _ = env;
                Next::Close
            }
        }
    }
}

/// 0xCFEF60: the cities neither the player's nor the helper's, in order.
pub fn attack_targets(campaign: &CivCampaign, helper: usize) -> Vec<usize> {
    campaign
        .cities
        .iter()
        .filter(|c| !c.owned() && c.nation() != helper)
        .map(|c| c.index)
        .take(10)
        .collect()
}

/// Run the rivals' calls once per civilization second and open the next
/// waiting call when no conversation is up.
pub fn civ_comm_update(
    time: Res<Time>,
    stage: Res<State<crate::stages::GameStage>>,
    originals: Option<Res<crate::Originals>>,
    mut lib: Option<ResMut<CnvLibrary>>,
    mut comm: ResMut<CivComm>,
    mut run: ResMut<crate::game::RunProgress>,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
    mut clock: Local<f32>,
) {
    if *stage.get() != crate::stages::GameStage::Civilization {
        if comm.open {
            comm.close();
        }
        return;
    }
    let difficulty = Difficulty::from_index(run.cell.difficulty);
    *clock += time.delta_secs().min(0.25);
    let run = &mut *run;
    while *clock >= civ_rules::CIV_UPDATE_SECONDS {
        *clock -= civ_rules::CIV_UPDATE_SECONDS;
        rivals_call(&mut run.civ_campaign, run.spice, difficulty);
    }
    // `OPENSPORE_CIV_CALLS=0` keeps the calls waiting (captures of a fight).
    // Calls wait without a window (headless runs, offscreen captures): the
    // screen pauses the game and nobody would answer.
    let calls = std::env::var("OPENSPORE_CIV_CALLS").map_or(true, |v| v != "0") && !windows.is_empty();
    if comm.open || run.civ_campaign.comm.incoming.is_empty() || !calls {
        return;
    }
    let Some(lib) = lib.as_deref_mut() else {
        return;
    };
    let call = run.civ_campaign.comm.incoming.remove(0);
    let env = CommEnv { index: originals.as_deref().map(|o| &o.resource_index), difficulty };
    comm.incoming(lib, &run.civ_campaign, run.spice, &env, call);
    if comm.text.is_empty() && comm.responses.is_empty() {
        // No installed conversation (headless runs): drop the call.
        comm.close();
    }
}

/// The rivals' calls and the vehicle stats of the player's designs.
pub struct CivCommPlugin;

impl Plugin for CivCommPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivComm>().add_systems(
            Update,
            (civ_comm_update, crate::civ_combat::refresh_vehicle_stats)
                .run_if(in_state(crate::stages::GameStage::Civilization)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign() -> CivCampaign {
        let mut c = CivCampaign::default();
        c.ensure_cities();
        c
    }

    fn facts() -> AttackFacts {
        AttackFacts {
            owner_is_human: true,
            owner_neutral: false,
            live_turrets: 4,
            happiness: 40.0,
            relationship: -3.0,
            reachable: true,
            owner_targets_me: false,
            distance: 0.0,
            current: false,
        }
    }

    #[test]
    fn the_attack_score_multiplies_the_retail_factors() {
        // 0xBFAB4B ..: (R50 + R50 + 50 + (10 - turrets) x 20 - 10 x relationship) x 6 reach x 4 x
        // (2 - d / (r pi)) x 8: (10 + 20 + 50 + 120 + 30) = 230 x 6 x 4 x 2 x 8.
        let base = attack_score(&facts(), AttackPurpose::Military, 100.0, Difficulty::Medium, (10, 20));
        assert_eq!(base, 230.0 * 6.0 * 4.0 * 2.0 * 8.0);
        // The cultural purpose scores the mood instead: (80 + (100 - 40) x 4 + 30) = 350.
        let cultural = attack_score(&facts(), AttackPurpose::Cultural, 100.0, Difficulty::Medium, (10, 20));
        assert_eq!(cultural, 350.0 * 6.0 * 4.0 * 2.0 * 8.0);
        // Neutral owner x10, the owner targeting the rival x3, the current target x2, Easy against a
        // rival city (not the human's) x16.
        let mut f = facts();
        f.owner_neutral = true;
        f.owner_targets_me = true;
        f.current = true;
        f.owner_is_human = false;
        let all = attack_score(&f, AttackPurpose::Military, 100.0, Difficulty::Easy, (10, 20));
        assert_eq!(all, base * 10.0 * 3.0 * 2.0 * 16.0);
        // The distance factor 2 - d / (r pi): half the planet away leaves 1 of the 2.
        f = facts();
        f.distance = 100.0 * std::f32::consts::PI;
        let far = attack_score(&f, AttackPurpose::Military, 100.0, Difficulty::Medium, (10, 20));
        assert!((far - 230.0 * 6.0 * 4.0 * 1.0 * 8.0).abs() < 0.5);
        f = facts();
        f.reachable = false;
        assert_eq!(attack_score(&f, AttackPurpose::Military, 100.0, Difficulty::Medium, (10, 20)) * 6.0, base);
    }

    fn hostile_campaign() -> CivCampaign {
        use crate::civ::CityOwner;
        let mut c = campaign();
        c.hidden.clear();
        c.player_developed = true;
        c.continents = vec![0; CITY_COUNT];
        c.cities[1].kind = Approach::Military;
        c.cities[2].owner = CityOwner::Player;
        // Two cities of the player against the rival's one (no hold-back on any difficulty) and an
        // Annoyed rival.
        c.nations[1].relation.apply_count(RelationKind::Hostility, 1.0);
        assert!(stance(c.nations[1].relation.value()) < 2);
        // The stopwatch (0xBFA96A) has run out.
        c.nations[1].raid = 0.0;
        c
    }

    #[test]
    fn a_rival_chooses_the_weakest_reachable_city_and_keeps_it_until_the_stopwatch_ends() {
        let mut c = hostile_campaign();
        c.cities[2].turret_hp.iter_mut().for_each(|hp| *hp = 0.0);
        assert!(c.cities[HOME_INDEX].live_turrets() > c.cities[2].live_turrets());
        let mut zero = |_: u32| 0u32;
        // Not before the player has developed (0xBFA7D1): nothing happens.
        c.player_developed = false;
        c.nations[1].raid = 0.0;
        assert_eq!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &[], &mut zero), None);
        assert_eq!(c.nations[1].raid, 0.0, "the stopwatch is untouched");
        c.player_developed = true;
        // The stopwatch ran out: the city without turrets scores most (its (10 - 0) x 20).
        let target = attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &[], &mut zero);
        assert_eq!(target, Some(2));
        assert_eq!(c.nations[1].attack_target, [Some(2), None]);
        assert_eq!(c.nations[1].raid, 40.0, "restarted: 40 s for the military purpose");
        // With the stopwatch running and the attitude unchanged the target stays.
        c.nations[1].raid = 10.0;
        assert_eq!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &[], &mut zero), Some(2));
        assert_eq!(c.nations[1].raid, 10.0);
        // The owner is Cautious or better: the target goes, no city qualifies, the stopwatch restarts.
        c.nations[1].relation = crate::civ_rules::Relationship::default();
        c.nations[1].raid = 10.0;
        assert_eq!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &[], &mut zero), None);
        assert_eq!(c.nations[1].attack_target, [None, None]);
        assert_eq!(c.nations[1].raid, 40.0);
        // A cultural nation re-chooses after 50 s.
        let mut c = hostile_campaign();
        c.cities[1].kind = Approach::Religious;
        let t = attack_decision(&mut c, 1, AttackPurpose::Cultural, Difficulty::Hard, &[], &mut zero);
        assert!(t.is_some());
        assert_eq!(c.nations[1].raid, 50.0);
        assert_eq!(c.nations[1].attack_target[1], t);
    }

    #[test]
    fn a_city_other_nations_attack_and_the_rival_does_not_is_left_alone() {
        let mut c = hostile_campaign();
        c.cities[2].turret_hp.iter_mut().for_each(|hp| *hp = 0.0);
        let mut zero = |_: u32| 0u32;
        // Another nation's vehicle is already on city 2 (0xBE4150): the rival takes the other one.
        let orders = [(3usize, crate::civ::UnitOrder::City(2))];
        let t = attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &orders, &mut zero);
        assert_eq!(t, Some(HOME_INDEX));
        // With one of its own vehicles there as well it is a candidate again.
        c.nations[1].raid = 0.0;
        c.nations[1].attack_target = [None, None];
        let both = [(3usize, crate::civ::UnitOrder::City(2)), (1usize, crate::civ::UnitOrder::City(2))];
        assert_eq!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &both, &mut zero), Some(2));
    }

    #[test]
    fn an_unreachable_city_is_skipped_below_tech_level_three_and_easy_skips_a_weaker_player() {
        let mut zero = |_: u32| 0u32;
        // The rival's city is on another continent and it holds one city (tech level 2): nothing reachable.
        let mut c = hostile_campaign();
        c.continents = vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Medium, &[], &mut zero), None);
        // Easy: the player holds two cities against the rival's one, so its cities are candidates.
        let mut c = hostile_campaign();
        assert!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Easy, &[], &mut zero).is_some());
        // Easy with one player city against one rival city: equal counts are no hold-back, but the
        // player's city is no candidate either (0xBFAA98: only while the player holds more).
        let mut c = hostile_campaign();
        c.cities[2].owner = crate::civ::CityOwner::Rival;
        assert_eq!(attack_decision(&mut c, 1, AttackPurpose::Military, Difficulty::Easy, &[], &mut zero), None);
    }

    #[test]
    fn rival_hut_hunting_sends_one_free_vehicle_to_each_untargeted_hut_on_its_continent() {
        // Hut 0 is already the goal of vehicle 7; hut 1 is on another continent; hut 2 gets the first
        // free vehicle that reaches it; hut 3 the next one.
        let huts = [(0usize, true), (1, false), (2, true), (3, true)];
        let fleet = [(5u32, false, None), (6, true, None), (7, false, Some(0usize)), (8, true, None)];
        let orders = hut_hunt(&huts, &fleet, &|_, _| true);
        assert_eq!(orders, vec![(6, 2), (8, 3)]);
        // A vehicle that cannot reach a hut is passed over for it.
        let orders = hut_hunt(&huts, &fleet, &|id, hut| !(id == 6 && hut == 2));
        assert_eq!(orders, vec![(8, 2), (6, 3)], "vehicle 6 takes the next hut it reaches");
        assert!(hut_hunt(&huts, &[], &|_, _| true).is_empty());
        assert_eq!(HUT_HUNT_ODDS, 10);
    }

    #[test]
    fn buy_offers_are_graded_like_the_original() {
        assert_eq!(buy_verdict(4000.0, 4000.0), BuyVerdict::Accepted);
        assert_eq!(buy_verdict(3880.0, 4000.0), BuyVerdict::Accepted);
        assert_eq!(buy_verdict(5199.0, 4000.0), BuyVerdict::Accepted);
        assert_eq!(buy_verdict(5200.0, 4000.0), BuyVerdict::TooHigh);
        assert_eq!(buy_verdict(3200.0, 4000.0), BuyVerdict::Rejected);
        assert_eq!(buy_verdict(2799.0, 4000.0), BuyVerdict::TooLow);
        assert_eq!(buy_offers(4000.0), [3200.0, 2400.0, 1600.0, 8000.0, 4000.0]);
        assert_eq!(buy_relation(BuyVerdict::TooLow), Some((RelationKind::BuyCityUnder, 3.0)));
    }

    #[test]
    fn gifts_demands_and_help_follow_the_comm_actions() {
        assert_eq!(gift_count(4000.0, false), 40.0);
        assert_eq!(gift_count(1000.0, true), 3.0, "a third of 1000, then / 100");
        assert_eq!(demand_count(1200.0, false), 3.0);
        assert_eq!(demand_count(1200.0, true), 1.0);
        // (2 + 2/2 + 1 + 3 + 2 + 3) x 200 + 500 - 200 = 2700.
        assert_eq!(demand_amount(2, 2, (1, 3), 2, 3, 5000.0), Some(2700.0));
        assert_eq!(demand_amount(0, 0, (0, 0), 0, 0, 50.0), None, "below 100");
        assert_eq!(demand_amount(9, 9, (4, 4), 9, 9, 1234.0), Some(1200.0), "capped by the money");
        assert_eq!(demand_odds(Approach::Religious, Difficulty::Hard), 16);
        assert_eq!(demand_odds(Approach::Military, Difficulty::Easy), 64);
        assert_eq!(alliance_odds(Difficulty::Medium), 12);
        assert_eq!(embargo_odds(Difficulty::Hard), 6);
        // (1 + 2) x 800 + 2 x 400 + 1600 (military beats economic).
        assert_eq!(attack_help_price(1, 2, Approach::Military, Approach::Economic), 4800.0);
        assert_eq!(attack_help_price(1, 0, Approach::Economic, Approach::Economic), 3200.0);
        // Pleased accepts; Annoyed refuses; Cautious only as the nation that likes the player most.
        assert!(accepts_trade_route(3, 3.0, 9.0) && accepts_trade_route(4, 8.0, 9.0));
        assert!(!accepts_trade_route(1, -4.0, -9.0) && !accepts_trade_route(0, -8.0, -9.0));
        assert!(accepts_trade_route(2, 0.0, 0.0), "a tie passes");
        assert!(accepts_trade_route(2, 1.0, f32::NEG_INFINITY));
        assert!(!accepts_trade_route(2, 0.0, 0.3), "another nation likes the player more");
        let mut c = CivCampaign::default();
        c.ensure_cities();
        for n in 1..CITY_COUNT {
            c.hidden.retain(|h| *h != n);
        }
        c.nations[3].relation.apply(RelationKind::Trade);
        assert!(best_other_relation(&c, 2) > 0.0);
        assert_eq!(best_other_relation(&c, 3), 0.0, "the others are all at 0");
    }

    #[test]
    fn actions_pay_and_move_the_relationship() {
        let mut c = campaign();
        let env = CommEnv { index: None, difficulty: Difficulty::Medium };
        let mut comm = CivComm { open: true, nation: 2, ..Default::default() };
        let mut money = 5000.0;
        let before = c.nations[2].relation.value();
        assert_eq!(comm.apply(&mut c, &mut money, &env, action::GIFTS[1], (0, 0, 0)), Next::Show(dialog::NPC_RECEIVE_GIFT));
        assert_eq!(money, 3000.0);
        assert_eq!(c.nations[2].relation.count(RelationKind::Gift), 20.0);
        assert!(c.nations[2].relation.value() > before);
        // A gift too big for the purse is not paid.
        assert_eq!(comm.apply(&mut c, &mut money, &env, action::GIFTS[0], (0, 0, 0)), Next::Show(dialog::NPC_RECEIVE_GIFT));
        assert_eq!(money, 3000.0);
        // Insults hurt, compliments help once.
        comm.apply(&mut c, &mut money, &env, action::INSULT, (0, 0, 0));
        assert_eq!(c.nations[2].relation.count(RelationKind::Insult), 1.0);
        assert_eq!(comm.apply(&mut c, &mut money, &env, action::EXIT, (0, 0, 0)), Next::Close);
        assert_eq!(
            comm.apply(&mut c, &mut money, &env, action::GOTO, (CIV_FILE, 0, dialog::NPC_GIFT)),
            Next::Show(dialog::NPC_GIFT)
        );
    }

    #[test]
    fn buying_a_city_haggles_then_hands_it_over() {
        let mut c = campaign();
        let env = CommEnv { index: None, difficulty: Difficulty::Medium };
        let mut lib = CnvLibrary::default();
        let mut comm = CivComm::default();
        assert!(comm.buy(&mut lib, &c, 1e6, &env, 3).is_err(), "no trade route, not for sale");
        c.cities[3].trade = true;
        comm.buy(&mut lib, &c, 1e6, &env, 3).unwrap();
        let asked = c.cities[3].buy_price();
        assert_eq!(comm.asked, asked);
        assert_eq!(comm.offers, buy_offers(asked));
        let mut money = 1e6;
        // The asked price (offer 5) is accepted without a relationship change.
        assert_eq!(comm.apply(&mut c, &mut money, &env, action::BUY_OFFERS[4], (0, 0, 0)), Next::Show(dialog::BUY_ACCEPTED));
        assert_eq!(comm.apply(&mut c, &mut money, &env, action::BUY_DONE, (0, 0, 0)), Next::Close);
        assert!((money - (1e6 - buy_offers(asked)[4])).abs() < 1e-3);
        assert!(civ::conquest_of(&c.cities[3]).is_some(), "the paid city changes hands");
    }

    #[test]
    fn counter_conversion_and_build_timers() {
        assert!((counter_conversion_chance(100) - 1.2).abs() < 1e-6);
        assert!((counter_conversion_chance(50) - 0.325).abs() < 1e-6);
        assert!((counter_conversion_chance(0) - 0.2).abs() < 1e-6);
        assert_eq!(build_interval(Approach::Religious), 50.0);
        // A converting city drifts back over time (a happy city always wins one).
        let mut c = campaign();
        c.cities[2].conversion = 0.5;
        c.cities[2].ensure_buckets();
        assert_eq!(c.cities[2].leader, Some(crate::civ::HOME_INDEX));
        for _ in 0..901 {
            rivals_call(&mut c, 0.0, Difficulty::Medium);
        }
        assert!(c.cities[2].conversion < 0.5, "{}", c.cities[2].conversion);
    }

    #[test]
    fn your_cities_win_citizens_back_from_a_rival_faith() {
        let mut c = campaign();
        c.cities[0].rival_conversion = 0.5;
        c.cities[0].converter = Some(2);
        c.cities[0].ensure_buckets();
        assert_eq!(c.cities[0].leader, Some(2));
        for _ in 0..901 {
            rivals_call(&mut c, 0.0, Difficulty::Medium);
        }
        assert!(c.cities[0].rival_conversion < 0.5, "{}", c.cities[0].rival_conversion);
        // A captured city converted to the full goes back after 2 s; the
        // capital never does.
        c.cities[3].owner = crate::civ::CityOwner::Player;
        c.cities[3].rival_conversion = 1.0;
        c.cities[3].converter = Some(2);
        c.cities[0].rival_conversion = 1.0;
        assert_eq!(c.convert_away(1.0), None);
        let lost = c.convert_away(1.1).expect("the city leaves");
        assert!(lost.contains("faith"), "{lost}");
        assert!(!c.cities[3].owned() && c.cities[3].rival_conversion == 0.0);
        assert!(c.cities[0].owned() && c.cities[0].rival_conversion < 1.0);
    }

    #[test]
    fn rivals_expand_to_the_best_geyser() {
        // Nearer wins; a current target counts double; a geyser two others
        // aim at is skipped.
        let list = [(None, 150.0, 0), (None, 100.0, 0), (Some(3), 10.0, 0), (None, 20.0, 4)];
        assert_eq!(expansion_pick(&list, None, 500.0), Some(1));
        assert_eq!(expansion_pick(&list, Some(0), 500.0), Some(0));
        assert!((expansion_score(0.0, 500.0, false, 0) - 1600.0).abs() < 1e-3);
        assert!((expansion_score(0.0, 500.0, false, 2) - 1600.0 / 3.0).abs() < 1e-3);
        assert!(expands_on_easy(2, 1) && !expands_on_easy(1, 1));
    }

    #[test]
    fn rivals_warn_and_the_last_one_surrenders() {
        let mut c = campaign();
        c.nations[2].relation.apply_count(RelationKind::DeclaredWar, 1.0);
        rivals_call(&mut c, 0.0, Difficulty::Medium);
        assert!(c.comm.incoming.iter().any(|i| i.nation == 2 && i.dialog == dialog::NPC_WAR_WARNING));
        let queued = c.comm.incoming.len();
        rivals_call(&mut c, 0.0, Difficulty::Medium);
        assert_eq!(c.comm.incoming.len(), queued, "warned once");
        // Every city but the last is the player's: it surrenders sooner or later.
        let mut c = campaign();
        let last = civ::CITY_COUNT - 1;
        for i in 1..last {
            c.cities[i].owner = civ::CityOwner::Player;
        }
        c.player_developed = true;
        c.tech_level = 3;
        for _ in 0..400 {
            rivals_call(&mut c, 0.0, Difficulty::Medium);
        }
        assert!(c.comm.incoming.iter().any(|i| i.nation == last && i.dialog == dialog::NPC_SURRENDER));
        // One-city nations never demand (two cities needed).
        assert!(!c.comm.incoming.iter().any(|i| i.dialog == dialog::NPC_DEMAND));
    }

    #[test]
    fn installed_civ_conversation_greets_by_attitude_and_lists_the_menu() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let c = campaign();
        let env = CommEnv { index: Some(&index), difficulty: Difficulty::Medium };
        let mut lib = CnvLibrary::default();
        let mut comm = CivComm::default();
        comm.contact(&mut lib, &c, 5000.0, &env, 2);
        assert!(!comm.text.is_empty(), "a greeting is drawn");
        // Compliment, insult, gifts, goodbye (help needs two cities).
        let actions: Vec<u32> = comm.responses.iter().map(|r| r.action).collect();
        assert!(actions.contains(&action::COMPLIMENT) && actions.contains(&action::INSULT), "{actions:x?}");
        assert!(actions.contains(&action::EXIT));
        assert!(comm.responses.iter().all(|r| !r.label.is_empty()));
        // The gift menu: 4000 is out of reach with 3000.
        comm.show(&mut lib, &c, 3000.0, &env, dialog::NPC_GIFT);
        let large = comm.responses.iter().find(|r| r.action == action::GIFTS[0]).unwrap();
        assert!(!large.enabled);
        assert!(comm.responses.iter().find(|r| r.action == action::GIFTS[1]).unwrap().enabled);
    }
}
