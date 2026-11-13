//! Tribal stage: lead your tribe and unite the five rival villages.
//!
//! Spore's Tribal stage is a small real-time strategy game, and so is this
//! one. The chieftain (WASD, camera follows) and the tribe members are units:
//! click or drag-select them and click (left with a selection, or right) on
//! the world to command them - a fruit bush to gather, the shore to fish, a
//! wild creature to hunt, a rival village to attack or perform for, the food
//! mat to drop off, a tool shack to pick up its tool, the ground to move.
//! Gatherers and fishers loop between the source and the village food mat on
//! their own. See `tribal_rts` for selection/commands and `tribal_tools` for
//! the original tool shacks.
//!
//! Food is the tribe's spendable stockpile (`RunProgress::dna_this_stage`,
//! kept under that name so existing saves still load). It only counts once
//! it is carried to the food mat. Gathering never advances the stage on its
//! own: the only road to Civilization is resolving every rival village,
//! peacefully or by conquest:
//!
//! * **Alliance** - gifts of food (`G`) warm a village up to half way; the
//!   rest is earned with a performance (`E`): the village calls a song of
//!   notes and the tribe answers each with `1` Chant, `2` Dance, `3` Horn,
//!   `4` Maracas, `5` Didgeridoo. Later villages ask for instruments, which
//!   members only play when they carry them (built from social tool shacks).
//! * **Conquest** - send members to attack (`F`, or click the village) until
//!   its three pieces (a hut and two tool racks) fall. Weapons deal their
//!   retail damage; rival members defend.
//!
//! The numbers (hut health, tool damage, food values, relationship events, the
//! ten NPC tribe archetypes) come from the original and live in
//! `tribal_rules`; the constants below keep their names and read from there.
//!
//! Rival villages raid the food mat, wild creatures roam, allies send gifts.
//! Near home `R` spends food on a new member (the hut's size caps the tribe)
//! or patches the chieftain up. Food sources regrow, so there is no soft lock.
//!
//! Other controls: `Q` select the whole tribe, `Z` gather, `X` fish, `H`
//! hunt, `V` rally home, `T`/`B` pick and build/equip a tool, and
//! `Tab`/`,`/`.`/`N`/`Backspace` edit roles and outfits.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::asset_catalog;
use crate::creature::CreatureStatsRes;
use crate::game::*;
use crate::mechanical_assets;
use crate::stages::{GameStage, StageOwned};
use crate::tribal_tools::{self as tools, ToolKind};

// ---------------------------------------------------------------------------
// Tuning
//
// Values read from the installed `TribeTuning` PROP are marked "original";
// the rest are honest prototype numbers grouped here so a later parity pass
// can retune from retail data in one place.
// ---------------------------------------------------------------------------

/// How many rival villages the world holds (one NPC tribe per tier; the tribe
/// must ally or conquer `tribal_rules::NUM_TRIBES_TO_CONQUER` = 5 of them).
pub const RIVAL_VILLAGES: usize = 5;
/// Pieces a village starts with, destroyed from the last slot down: a weapons
/// shack and a music shack (tool racks, 200 hit points each) and the dwelling (a
/// hut of 400 / 750 / 1000 by the tribe's tier). Losing all of them is conquest.
pub const HUTS_PER_VILLAGE: u32 = 3;
/// Radius the chieftain must be inside to perform / gift / attack a village:
/// the tribe's zoning radius (30; ADAPTED use of it for a rival's reach).
pub const INTERACT_RANGE: f32 =
    crate::tribal_rules::TRIBE_ZONING_RADIUS * crate::tribal_rules::RANGE_SCALE;
/// Radius around the home hut where `R` recruits and recovers (the zoning
/// radius, 30).
pub const HOME_RANGE: f32 =
    crate::tribal_rules::TRIBE_ZONING_RADIUS * crate::tribal_rules::RANGE_SCALE;

/// Relations at which a village allies with you: the relationship attitude
/// Friendly (`tribal_rules::ALLY_RELATION_THRESHOLD`, 70; the alliance rule is
/// VERIFIED in 0xD9CED0 / 0xC2F7F0, the threshold VERIFIED: the `ui` prop
/// `RolloverRltnshpGoodCap` 7 read by 0xD00F10).
#[allow(dead_code)] // public name kept (tests, HUD helpers); the rule is tribal_rules::allies
pub const ALLY_RELATIONS: f32 = crate::tribal_rules::ALLY_RELATION_THRESHOLD;
/// The unit of a right note in the performance score: the `TribeSocial`
/// event's +10 (the social accessories' head start is counted in these,
/// `outfitter::social_head_start`). A note itself moves no relationship: the
/// retail social tools lift the tribe to a target when the audience answers and
/// when the song succeeds (`tribal_rules::social_lift`); a wrong or missed note
/// never subtracts.
pub const PERFORMANCE_SUCCESS: f32 = crate::tribal_rules::SOCIAL_RELATIONSHIP_IMPROVEMENT;
/// Food paid by a gift (`giftFoodAmount`). A delivered gift calms a Furious or
/// Annoyed tribe to Cautious and does nothing for a tribe that is at Cautious
/// or better (`tribal_rules::gift_effect`, 0xDBC1F1..0xDBC25C).
pub const GIFT_FOOD: f32 = crate::tribal_rules::GIFT_FOOD_AMOUNT;

/// Shared cooldown after a performance, gift or recruitment.
pub const ACTION_COOLDOWN: f32 = 2.0;
/// Seconds between two beats of the social game (the patch's `trg_round_length`).
pub const NOTE_WINDOW: f32 = crate::tribal_rules::SOCIAL_ROUND_SECONDS;
/// Longest possible song, in seconds (five rounds and the evaluation after the
/// last beat).
pub const PERFORMANCE_WINDOW: f32 = NOTE_WINDOW * (crate::tribal_rules::SOCIAL_ROUNDS as f32 + 1.0);

/// Below this fraction of max health the chieftain must recover first.
pub const WOUNDED_FRACTION: f32 = 0.15;

/// One-time food reward for resolving a village (paid once, ever). The original
/// pays none: the three handlers that run when a tribe is allied or conquered
/// (the conquest 0xC2F7F0, the alliance totem 0xD9CED0 and the totem history
/// 0xCDAC90, with the history / timeline calls 0xCD4230, 0xCDB520, 0xBA59E0) set
/// the tribe's state, play the `TRG_TotemPole_*` cinematic and record the
/// totem; none of them adds food (VERIFIED by reading them). OpenSpore's former
/// +60 / +40 food is gone; the constants stay (0) so the reward plumbing and the
/// one-time flag keep their shape.
pub const CONQUER_REWARD: f32 = 0.0;
pub const ALLY_REWARD: f32 = 0.0;

/// Food spent to recruit one tribe member (the member price) / to recover health.
pub const RECRUIT_FOOD: f32 = crate::tribal_rules::MEMBER_PRICE;
pub const RECOVER_FOOD: f32 = 25.0;
/// Share of max health restored by one recovery.
pub const RECOVER_FRACTION: f32 = 0.4;
/// Below this fraction of max health `R` patches the chieftain up before
/// spending anything on recruits.
pub const RECOVER_PRIORITY_FRACTION: f32 = 0.75;
/// Member hunger capacity (the retail motive maximum), persisted per stable
/// roster slot so a load cannot refill a working tribe for free.
pub const MEMBER_ENERGY_MAX: f32 = crate::tribal_rules::HUNGER_MAX;
/// Seconds an egg takes to grow into a member (`timeBeforeBabyGrowsUp`).
pub const BIRTH_GROWTH_SECONDS: f32 = crate::tribal_rules::TIME_BEFORE_BABY_GROWS_UP;
/// Upper bound on tribe members (also caps the editor's assignment list).
pub const MAX_MATES: usize = 12;
/// Members the starting hut houses; each tribe level enlarges it (see
/// `tribal_rules::POPULATION_CAP_BY_LEVEL`).
pub const START_POPULATION_CAP: usize = crate::tribal_rules::POPULATION_CAP_BY_LEVEL[0];
/// First `WorldId` handed to a tribe member. Member slots run
/// `MATE_ID_BASE..MATE_ID_BASE + MAX_MATES`, which is the range
/// `persistence::restore` treats as absent-when-dead.
pub const MATE_ID_BASE: u32 = 200;

/// The most food one member carries (original `carryBundleAmountMax`); each
/// kind of load has its own smaller bundle, see `tribal_rules::bundle_food`.
pub const CARRY_MAX: f32 = crate::tribal_rules::CARRY_BUNDLE_AMOUNT_MAX;
/// Seconds a full fruit bush takes to regrow (original
/// `fruitPropMaxRegrowthTime`).
pub const BUSH_REGROW_SECONDS: f32 = crate::tribal_rules::FRUIT_REGROWTH_TIME;
/// Fruit (one food each) on a full bush.
pub const BUSH_FOOD: f32 = crate::tribal_rules::FRUITS_PER_BUSH;

/// The five performance answers: two the whole tribe can do, three that need
/// an instrument in the troupe.
pub const ABILITIES: [&str; 5] = ["Chant", "Dance", "Horn", "Maracas", "Didgeridoo"];
/// Bit mask of every instrument note (3, 4, 5) - used by pure tests and by
/// callers that do not model who is standing where.
pub const ALL_INSTRUMENTS: u8 = 0b111;

// ---------------------------------------------------------------------------
// World layout (pure, so it is unit-testable)
// ---------------------------------------------------------------------------

/// Where the tribe's own hut stands; raids target it and `R` works near it.
pub fn home_position() -> Vec3 {
    Vec3::ZERO
}

/// The village food mat in front of the hut: deliveries land here and
/// raiders steal from it.
pub fn food_pile_position() -> Vec3 {
    home_position() + Vec3::new(-0.6, 0.0, 4.4)
}

/// The totem pole beside the food mat.
pub fn totem_position() -> Vec3 {
    home_position() + Vec3::new(3.4, 0.0, 3.6)
}

/// The egg pen where recruits hatch.
pub fn egg_pen_position() -> Vec3 {
    home_position() + Vec3::new(-4.4, 0.0, 2.4)
}

/// Where rival village `index` stands (zero-based: index 0 is `V1` on the
/// world map): its placement marker's ring from the stage origin (T4: nearby
/// 175..225, far 175..350, filler 200..450, failsafe 200..500), on land, as
/// `tribal_planet` laid the stage out (deterministic per save seed).
pub fn village_position(index: usize) -> Vec3 {
    crate::tribal_planet::current().village(index)
}

/// Pure: the prototype island's shoreline distance from the centre along the
/// direction of `dir`, for terrain `seed`. Mirrors the radial coastline in
/// `terrain::LandSurface::generate` (extent 45 for this stage); only the flat
/// layout (no planet) still fishes there.
pub fn coastline_radius(seed: u64, dir: Vec2) -> f32 {
    let dir = dir.normalize_or(Vec2::X);
    let phase = (seed ^ (seed >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28;
    let depth = |r: f32| crate::terrain::coast_depth(dir * r, 45.0, phase);
    let (mut land, mut sea) = (0.0, 45.0);
    for _ in 0..20 {
        let mid = (land + sea) * 0.5;
        if depth(mid) > 0.0 {
            land = mid;
        } else {
            sea = mid;
        }
    }
    (land + sea) * 0.5
}

/// A goal position kept on land (the island's movement square is gone: the
/// planet has no edges). Points out at sea are pulled back toward the hut;
/// without a planet every point is kept.
pub fn keep_on_land(pos: Vec3) -> Vec3 {
    crate::tribal_planet::current().keep_on_land(pos)
}

/// The fishing spots on the shore near the hut (the real shore on the
/// planet, the prototype island's coast without one). Each is a land point
/// at the waterline; `tribal_rts` puts its ripple 1.6 units farther out.
pub fn fishing_spots() -> Vec<Vec3> {
    crate::tribal_planet::current().fishing.clone()
}

/// Pure: display name of a performance answer (`1`..=`5`).
pub fn ability_name(ability: u32) -> &'static str {
    ABILITIES[((ability.max(1) - 1) as usize) % ABILITIES.len()]
}

/// Pure: bit for an instrument note in an instrument mask.
pub fn instrument_bit(note: u32) -> u8 {
    match note {
        3 => 0b001,
        4 => 0b010,
        5 => 0b100,
        _ => 0,
    }
}

/// Pure: the troupe index (0 horn, 1 maracas, 2 didgeridoo) of an instrument
/// note (`3`..=`5`).
pub fn instrument_kind(note: u32) -> Option<usize> {
    (3..=5).contains(&note).then(|| (note - 3) as usize)
}

/// Who takes part in a song, as the social command found them (the retail
/// performance object keeps the participants' vector at +0x64 and the village's
/// members at +0x78).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Troupe {
    /// Members sent to perform, the chieftain included: each adds
    /// `scoreContributionPerCreature` to the start score (0xD9CCF0).
    pub participants: u32,
    /// Members carrying each instrument (horn, maracas, didgeridoo).
    pub instruments: [u32; 3],
    /// Members of the village that watch; each answers once (0xDAEA70).
    pub audience: u32,
}

impl Troupe {
    /// A big troupe (twenty participants, four players per instrument) and a
    /// watching village of three: a fixture for pure tests and callers that do
    /// not model who stands where (the start score alone is 500).
    pub const FULL: Troupe = Troupe {
        participants: 20,
        instruments: [4, 4, 4],
        audience: 3,
    };
}

/// What the social game tells the HUD about the song (the hint manager's
/// `trg-socialtutorial` calls at 0xDA325E / 0xDA213F / 0xDA31A6 / 0xD9FFB5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocialEvent {
    /// The troupe is in place and the game starts (`ShowHint` while the tutorial
    /// flag 0x157BDF98 is unset: `seen` is the flag).
    Started { seen: bool },
    /// A bubble came up after more than one round without an order (`ShowHint`
    /// whatever the flag).
    Reminder,
    /// The first order of the game (sets the flag, `HideHint`).
    FirstOrder,
    /// The game ended (`HideHint`).
    Ended,
}

/// The events of the social game since the HUD last looked. Never part of a
/// save and never compared.
#[derive(Debug, Clone, Default)]
pub struct SocialEvents(pub Vec<SocialEvent>);

impl PartialEq for SocialEvents {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Pure: nearest *hostile* village within interaction range of `pos`.
pub fn nearest_hostile(pos: Vec3, campaign: &TribalCampaign) -> Option<usize> {
    (0..RIVAL_VILLAGES)
        .filter(|&i| {
            campaign
                .village(i)
                .is_some_and(|v| v.status == RivalStatus::Hostile)
        })
        .map(|i| (i, dist_xz(pos, village_position(i))))
        .filter(|&(_, d)| d <= INTERACT_RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Pure: the hostile village Fire Bombs land on from `pos`: the nearest one
/// whose centre is within the village radius plus the power's reach
/// (`creatureAbilityRange` 15, scaled).
pub fn fire_bomb_target(pos: Vec3, campaign: &TribalCampaign) -> Option<usize> {
    let reach = INTERACT_RANGE
        + crate::tribal_rules::superpower_reach(&crate::tribal_rules::SUPERPOWER_FIRE_BOMBS);
    (0..RIVAL_VILLAGES)
        .filter(|&i| {
            campaign
                .village(i)
                .is_some_and(|v| v.status == RivalStatus::Hostile)
        })
        .map(|i| (i, dist_xz(pos, village_position(i))))
        .filter(|&(_, d)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Pure: whether `pos` is close enough to home for recruitment and recovery.
pub fn is_home(pos: Vec3) -> bool {
    dist_xz(pos, home_position()) <= HOME_RANGE
}

/// Pure: how many members the home hut houses once `resolved` villages are
/// settled (the hut grows with the tribe level, 6 / 9 / 12).
pub fn population_cap(resolved: usize) -> usize {
    crate::tribal_rules::population_cap(resolved).min(MAX_MATES)
}

/// Pure: home hut level 1..=3 from the tribes allied or conquered: the level
/// rises at 1 and 3 (`goalProgressUpgrades`, 0xCDA4F0); health 400 / 750 / 1000.
pub fn hut_level(resolved: usize) -> usize {
    crate::tribal_rules::hut_level(resolved)
}

/// Pure: the tier (1..=5) of the NPC tribe living in village `index`: every
/// game holds one tribe of each tier, in order.
pub fn village_tier(index: usize) -> u8 {
    (index + 1).min(RIVAL_VILLAGES) as u8
}

// ---------------------------------------------------------------------------
// Campaign state (persisted through `RunProgress::tribal_campaign`)
// ---------------------------------------------------------------------------

/// Marker on each rival village's root entity. Zero-based; `navigation.rs`
/// labels these `V1`..`V5` on the world map.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivalVillage(pub usize);

/// Marker on a single hut inside a rival village (village index, hut slot).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RivalHut(pub usize, pub u32);

/// How a village stands with the tribe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RivalStatus {
    /// Still opposed; performances, gifts and attacks all apply.
    #[default]
    Hostile,
    /// Peaceful resolution (gifts + performances).
    Allied,
    /// Conquest resolution (every hut destroyed).
    Conquered,
}

impl RivalStatus {
    pub fn is_resolved(&self) -> bool {
        !matches!(self, RivalStatus::Hostile)
    }

    /// HUD label (uppercase only once resolved so the eye finds them).
    pub fn label(&self) -> &'static str {
        match self {
            RivalStatus::Hostile => "hostile",
            RivalStatus::Allied => "ALLIED",
            RivalStatus::Conquered => "CONQUERED",
        }
    }
}

fn default_huts() -> u32 {
    HUTS_PER_VILLAGE
}

/// One rival village's persisted state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RivalVillageState {
    #[serde(default)]
    pub status: RivalStatus,
    /// The relationship with the tribe, `tribal_rules::RELATION_MIN` (-100) ..=
    /// `ALLY_RELATIONS`; it starts at the lineup's value and moves with the
    /// relationship events (gift +10, social +10, attack -10, kill -60 ...).
    #[serde(default)]
    pub relations: f32,
    /// Pieces still standing (0..=3); 0 means conquered.
    #[serde(default = "default_huts")]
    pub huts: u32,
    /// Damage accumulated on the piece being destroyed, in hit points (persisted
    /// so a save mid-siege resumes rather than restarting).
    #[serde(default)]
    pub hurt: f32,
    /// The one-time resolve reward has been paid.
    #[serde(default)]
    pub rewarded: bool,
    /// The tribe has come into view. Saves from before tribes appeared over
    /// time have every tribe present.
    #[serde(default = "default_appeared")]
    pub appeared: bool,
    /// The tribe's food stock (raided by the player); `None` = untouched, i.e.
    /// its archetype's `initialfood` (125 / 200 / 300).
    #[serde(default)]
    pub food: Option<f32>,
    /// The relationship the tribe started with: its baseline attitude
    /// (`tribal_rules::attitude`, the retail 0xC8EC60) the gift effect fades
    /// back to.
    #[serde(default)]
    pub baseline: f32,
    /// Seconds until the effect of a gift or a song fades one attitude step
    /// (cTribe +0x2CC; 0 = not running), `tribal_rules::gift_decay`.
    #[serde(default)]
    pub gift_timer: f32,
}

fn default_appeared() -> bool {
    true
}

impl Default for RivalVillageState {
    fn default() -> Self {
        Self {
            status: RivalStatus::Hostile,
            relations: 0.0,
            huts: HUTS_PER_VILLAGE,
            hurt: 0.0,
            rewarded: false,
            appeared: true,
            food: None,
            baseline: 0.0,
            gift_timer: 0.0,
        }
    }
}

fn default_song() -> u32 {
    crate::tribal_rules::SOCIAL_ROUNDS
}

/// The bar and the troupe of a song that just ended (see
/// [`TribalCampaign::social_end`]): the members play their ending clips
/// (`tribal_rules::social_ending_clips`, 0xDA3944..0xDA3A44) and the bar stays
/// until the stop 0xD9FE60 hides it, `tribal_rules::social_ending_seconds` after
/// the end (ADAPTED trigger: the stop's caller was not traced).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SocialEnd {
    pub meter: crate::tribal_rules::SocialBarMeter,
    /// The village the song was for.
    pub village: usize,
    /// The song's level (selects the faces).
    pub level: u8,
    /// A success shows the star burst.
    pub success: bool,
    /// Seconds before the stop hides the bar.
    pub left: f32,
    /// The song had a bar object (levels 1 and 2; a level 0 song has none).
    pub has_bar: bool,
    /// The bar of the score object at the end, `min(score / threshold x 10, 10)`:
    /// above 5 the members play the happy ending clips (0xDA3A01).
    pub bar: f32,
}

/// An in-flight performance: the retail social rhythm game (the performance
/// object whose update is 0xDA2F30). The troupe plays in front of the village;
/// a beat ends a round every [`NOTE_WINDOW`] seconds, five rounds in all. One
/// second before each beat the village shows a thought bubble asking for some
/// players of one instrument; the player orders an instrument (the three social
/// abilities, refresh 5 s) and the round is scored at once, or 1.5 s after the
/// beat when no order came. The score starts with the participants and the
/// species' social level and fills a bar of the archetype's threshold: a full
/// bar ends the song as a success, running out of rounds as a failure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    /// Village being performed for.
    pub village: usize,
    /// The instrument the village asked for in its last bubble (`3` horn, `4`
    /// maracas, `5` didgeridoo: the ability type minus 2); 0 before the first.
    pub prompt: u32,
    /// Seconds to the next beat.
    pub time_left: f32,
    /// Beats counted so far (+0x44, the game's `round`).
    #[serde(default)]
    pub note: u32,
    /// Rounds in the song (+0x48, the patch's `trg_social_length`).
    #[serde(default = "default_song")]
    pub notes: u32,
    /// Evaluations in which at least one player of the wanted instrument played
    /// (0xDA0100 then sets `trg_social_success`).
    #[serde(default)]
    pub hits: u32,
    /// Evaluations in which none did.
    #[serde(default)]
    pub misses: u32,
    /// Song seed (the campaign round it started on).
    #[serde(default)]
    pub song: u32,
    /// Players the last bubble asked for (+0x1E0, `0` before the first bubble).
    #[serde(default)]
    pub required: u8,
    /// The song's social level, fixed when it starts from the attitude of the
    /// village (0xDA183A..0xDA1863: Cautious 1, Pleased 2, else 0): the target a
    /// successful song lifts the relationship to (`tribal_rules::social_target`)
    /// and which of the archetype's two thresholds fills the bar.
    #[serde(default)]
    pub level: u8,
    /// A member of the audience has answered (0xDAEA70, once per member).
    #[serde(default)]
    pub audience_answered: bool,
    /// The score (+0x34).
    #[serde(default)]
    pub score: f32,
    /// The score that fills the bar (the archetype's +0x90 / +0x94).
    #[serde(default)]
    pub threshold: f32,
    /// A bubble is up (it goes with the evaluation, 0xDA0488).
    #[serde(default)]
    pub bubble: bool,
    /// This round already had its bubble.
    #[serde(default)]
    pub bubbled: bool,
    /// The round has been evaluated (+0x58).
    #[serde(default)]
    pub evaluated: bool,
    /// Seconds to the evaluation a beat without an order sets (+0x54; 0 = none).
    #[serde(default)]
    pub eval_in: f32,
    /// Players ordered since the last evaluation, by instrument (horn, maracas,
    /// didgeridoo).
    #[serde(default)]
    pub ordered: [u32; 3],
    /// Seconds until each instrument can be ordered again.
    #[serde(default)]
    pub refresh: [f32; 3],
    /// Evaluations the timer made (no order in the round) since the reminder
    /// hint (+0x60).
    #[serde(default)]
    pub idle_beats: u32,
    /// The player has ordered an instrument in this song.
    #[serde(default)]
    pub first_order: bool,
    /// The village's `archetype` id (selects the thresholds and the bubbles).
    #[serde(default)]
    pub archetype: u8,
    /// Instruments whose tool is open (bit per kind); a bubble only asks for
    /// those. ADAPTED: retail asks for any of the archetype's list.
    #[serde(default)]
    pub unlocked: u8,
    /// The gift timer the song set aside when it began (+0x5C).
    #[serde(default)]
    pub saved_timer: f32,
    /// Orders given so far and the instrument note of the last one.
    #[serde(default)]
    pub orders: u32,
    #[serde(default)]
    pub last_order: u32,
    /// Audience members that answered.
    #[serde(default)]
    pub answers: u32,
    /// The bar of the social UI (the `SocialBar` object 0xE312E0): its fill, the
    /// fills the score queued (0xD9CC20 -> 0xE315B0) and the failed flag. The
    /// song succeeds when the bar's fill reaches 1 (0xDA349E), so a score counts
    /// as the bar animates it in; a song without a bar (level 0) cannot succeed.
    #[serde(default)]
    pub bar: crate::tribal_rules::SocialBarMeter,
    /// Counts the thought bubbles raised (the audience shows one each time) and the
    /// evaluations with their outcome (3 the bubble was answered, 2 partly, 1 not;
    /// 0xDA049D / 0xDA04B1 / 0xDA04BF), so the visuals see every event once.
    #[serde(default)]
    pub bubble_serial: u32,
    #[serde(default)]
    pub reaction: u8,
    #[serde(default)]
    pub reaction_serial: u32,
    /// The troupe is still walking to its spots (the performance states 0..4 of
    /// 0xDA2F30: every participant must stand in place before the music starts and
    /// the game begins, 0xDA3306 / 0xDA3461). The clock, the bubbles and the start
    /// score wait for [`TribalCampaign::troupe_ready`].
    #[serde(default)]
    pub gathering: bool,
    /// Seconds the troupe has been walking (the safety timeout,
    /// `tribal_rules::SOCIAL_GATHER_TIMEOUT`).
    #[serde(default)]
    pub gather_time: f32,
    /// The start score that goes through the score object when the game begins
    /// (0xD9CCF0, state 4 -> 5).
    #[serde(default)]
    pub start_score: f32,
}

impl Performance {
    /// 0xD9CC20: the score grows by `delta`; a song with a bar queues the fill
    /// `delta / threshold` over `seconds` (the start score over the whole song,
    /// a beat's score over one second).
    fn add_score(&mut self, delta: f32, seconds: f32) {
        self.score += delta;
        if self.level > 0 && self.threshold > 1.5e-5 && delta > 0.0 {
            self.bar.queue_fill(delta / self.threshold, seconds);
        }
    }

    /// The troupe is in place and the music runs (state 5): the start score fills
    /// the bar over the whole song and the first beat is a round length away.
    fn begin(&mut self) {
        self.gathering = false;
        self.time_left = NOTE_WINDOW;
        let score = std::mem::take(&mut self.start_score);
        self.add_score(score, self.notes as f32 * NOTE_WINDOW);
    }

    fn mix(&self) -> u32 {
        self.song
            .wrapping_mul(2_654_435_761)
            .wrapping_add(self.note.wrapping_mul(40_503))
            .rotate_left(7)
    }

    /// The thought bubble (the sendgame handler 0xDA1F20, style 0xFF77507F): the
    /// players it asks for are drawn from the archetype's range, the instrument
    /// from its list. Returns whether the tutorial reminder shows (0xDA213F: more
    /// than one evaluation by the timer since the last one).
    fn raise_bubble(&mut self) -> bool {
        let reminder = self.idle_beats > 1;
        if reminder {
            self.idle_beats = 0;
        }
        let tuning = crate::tribal_rules::social_tuning(self.archetype);
        let mix = self.mix();
        self.required = crate::tribal_rules::social_required(&tuning, ((mix >> 4) & 0xFFFF) as f32 / 65535.0).min(255) as u8;
        let open: Vec<usize> = tuning
            .instruments
            .iter()
            .map(|t| (*t - 5) as usize)
            .filter(|k| self.unlocked & (1 << *k) != 0)
            .collect();
        let kind = if open.is_empty() {
            crate::tribal_rules::social_wanted_kind(&tuning, mix >> 12)
        } else {
            open[(mix >> 12) as usize % open.len()]
        };
        self.prompt = 3 + kind as u32;
        self.bubble_serial = self.bubble_serial.wrapping_add(1);
        self.bubble = true;
        self.bubbled = true;
        self.evaluated = false;
        reminder
    }

    /// The round's evaluation (0xDA0100): the players ordered since the last one
    /// count against the bubble's instrument and number; enough of the wanted
    /// instrument scores a whole beat, some of it its share, none of it 5 per
    /// player. The bubble goes with it.
    fn evaluate(&mut self) {
        let played: u32 = self.ordered.iter().sum();
        let (wanted, required) = match instrument_kind(self.prompt).filter(|_| self.required > 0) {
            Some(kind) => (self.ordered[kind], u32::from(self.required)),
            // ADAPTED: before the first bubble nothing is wanted (the retail
            // required count is still 0 there, which would score a whole beat
            // for any order); those players count as players of other
            // instruments.
            None => (0, 1),
        };
        self.reaction = if wanted >= required {
            3
        } else if wanted > 0 {
            2
        } else {
            1
        };
        self.reaction_serial = self.reaction_serial.wrapping_add(1);
        let beat = crate::tribal_rules::social_beat_score(wanted, required, played);
        self.add_score(beat, crate::tribal_rules::SOCIAL_BAR_BEAT_SECONDS);
        if wanted > 0 {
            self.hits += 1;
        } else if played > 0 {
            self.misses += 1;
        }
        self.ordered = [0; 3];
        self.evaluated = true;
        self.bubble = false;
        // 0xDA01DB: the evaluation cancels the grace timer a beat set.
        self.eval_in = 0.0;
    }
}

/// What one campaign action changed. Pure data so the rules are testable
/// without a Bevy world.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TribalEvent {
    /// Player-facing feedback line (empty when nothing happened).
    pub message: String,
    /// Relations after the action.
    pub relations: f32,
    /// Village status after the action.
    pub status: RivalStatus,
    /// Food to add to the stockpile (negative when the action paid for it).
    pub food_delta: f32,
    /// One-time food reward granted by resolving the village (0 otherwise).
    pub reward: f32,
    /// Huts standing after the action.
    pub huts_left: u32,
    /// Huts knocked down by this action (the system despawns that many).
    pub huts_destroyed: u32,
    /// True only on the single frame the village changed resolved state.
    pub resolved_now: bool,
}

/// Persisted Tribal campaign: five rival villages plus every cooldown,
/// counter and roster slot the loop needs. `Default` and
/// `#[serde(default)]` on each field keep saves made before this stage
/// existed loading as five hostile villages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TribalCampaign {
    /// Always kept at `RIVAL_VILLAGES` entries by [`TribalCampaign::ensure_ready`].
    #[serde(default)]
    pub villages: Vec<RivalVillageState>,
    /// One stable roster slot (`WorldId - MATE_ID_BASE`) per living member,
    /// parallel to `RunProgress::tribe`. Slots are recycled so ids never
    /// grow past `MAX_MATES`.
    #[serde(default)]
    pub slots: Vec<u32>,
    /// Energy keyed by stable member slot, not dense roster index.
    #[serde(default)]
    pub member_energy: BTreeMap<u32, f32>,
    /// Paid eggs that are still growing at the home pen.
    #[serde(default)]
    pub births: Vec<TribalBirth>,
    /// Owned animal species, identities, journey and production progress.
    #[serde(default)]
    pub livestock: Vec<crate::tribal_rts::livestock::LivestockRecord>,
    /// Uncollected eggs at the pen, separate from the food mat stockpile.
    #[serde(default)]
    pub livestock_eggs: f32,
    /// Consumed at the Creature -> Tribal boundary, including an empty pack.
    /// Re-entering Tribal must never recreate a pet that has since died.
    #[serde(default)]
    pub pack_inherited: bool,
    /// Friendly world commands tame animals and perform for rival villages.
    #[serde(default)]
    pub friendly_stance: bool,
    /// Runtime-only cadence for committing growth progress to saves.
    #[serde(skip)]
    pub birth_save_timer: f32,
    /// Score the worn outfit's social level adds to a song's start score
    /// (`outfitter::social_head_start`: level x 35, set from the outfit each
    /// frame).
    #[serde(skip)]
    pub outfit_social_bonus: f32,
    /// What the social game tells the HUD (`trg-socialtutorial`); never saved.
    #[serde(skip)]
    pub social_events: SocialEvents,
    /// The social tutorial flag (profile flag 0x157BDF98, set by the first order
    /// of a song 0xDA3181): the game's start shows `trg-socialtutorial` only
    /// while it is unset. ADAPTED: per save (the other tutorial flags are too).
    #[serde(default)]
    pub social_tutorial_done: bool,
    /// In-flight performance, if the chieftain started one.
    #[serde(default)]
    pub performance: Option<Performance>,
    /// The social bar of the song that just ended, still on screen; never saved.
    #[serde(skip)]
    pub social_end: Option<SocialEnd>,
    /// Where each member of the troupe stands for the running (or just ended) song,
    /// by entity bits: the layout of 0xDA22C0 (`tribal_rules::social_stands`),
    /// laid out when the command starts the song; never saved.
    #[serde(skip)]
    pub song_stands: Vec<(u64, Vec3)>,
    /// Shared social/gift cooldown.
    #[serde(default)]
    pub cooldown: f32,
    /// Recruitment / recovery cooldown.
    #[serde(default)]
    pub recruit_cooldown: f32,
    /// Deterministic song counter (persisted, so prompts stay stable).
    #[serde(default)]
    pub round: u32,
    /// Last feedback line.
    #[serde(default)]
    pub message: String,
    /// Seconds left to show `message`.
    #[serde(default)]
    pub message_timer: f32,
    /// Tool shacks built beside the home hut, in slot order.
    #[serde(default)]
    pub shacks: Vec<ToolKind>,
    /// The tool each member carries, keyed by stable roster slot.
    #[serde(default)]
    pub tools: BTreeMap<u32, ToolKind>,
    /// Unused since the NPC gift plans (`tribal_plans`) replaced OpenSpore's
    /// 40 s ally gift; kept so old saves load unchanged.
    #[serde(default)]
    pub ally_gift_timer: f32,
    /// Active Cell consequence power.
    #[serde(default)]
    pub cell_power: Option<TribalPowerKind>,
    /// Active Creature consequence power.
    #[serde(default)]
    pub creature_power: Option<TribalPowerKind>,
    /// Cooldown for the Cell-stage consequence power (seconds).
    #[serde(default)]
    pub power_cell_cooldown: f32,
    /// Cooldown for the Creature-stage consequence power (seconds).
    #[serde(default)]
    pub power_creature_cooldown: f32,
    /// Placed traps currently active in the world (position, lifetime).
    #[serde(default)]
    pub active_traps: Vec<TribalTrapData>,
    /// Charmed wild animals currently fighting for the tribe (marker, timer).
    #[serde(default)]
    pub charmed_creatures: Vec<CharmedAnimalData>,
    /// Carvings added to the home totem pole as villages resolve.
    #[serde(default)]
    pub totem_carvings: Vec<TotemSegment>,
    /// Which of the nine five-tribe lineups this world uses
    /// (`tribal_rules::lineup_for_seed`); it decides the archetype of each
    /// village.
    #[serde(default)]
    pub lineup: u8,
    /// Seconds towards the next new-tribe check (every 60 s).
    #[serde(default)]
    pub appearance_timer: f32,
    /// The NPC tribe controller's timers and tutorial flags
    /// (`tribal_rules::AppearanceState`, 0xCC6A90): the first tribe comes after
    /// the stage starts, the rest by the settled-tribe / food rules. Replaces
    /// `appearance_timer` (kept so old saves load).
    #[serde(default)]
    pub appearance: crate::tribal_rules::AppearanceState,
    /// Seconds until a fallen chieftain returns at the hut (0 = not waiting).
    #[serde(default)]
    pub chieftain_respawn: f32,
    /// Hit points the home hut has lost to raiders (`RaidPhase::Raze`); at
    /// its health (400 / 750 / 1000) it is destroyed: `TRG_PRE_HutDestroyed`
    /// and the restart (0xCD81F0, message 0x58BADDD for the player's hut).
    #[serde(default)]
    pub home_hut_damage: f32,
    /// The city-hall play-style card chosen when the stage ended.
    #[serde(default)]
    pub city_hall_card: Option<crate::tribal_rules::PlayStyle>,
    /// The Evolve click (0xCDBEA0) also stores the consequence trait id on the
    /// species (+0x504, `PlayStyle::trait_id`) and the play-style id in the
    /// player (cPlayer +0x1104, `PlayStyle::card_id`). ADAPTED: here they are
    /// kept in the campaign (OpenSpore has no species record to put them on).
    #[serde(default)]
    pub earned_trait: Option<u32>,
    #[serde(default)]
    pub play_style_id: Option<u32>,
    /// The Tribal tutorial manager's state (`tribal_hud::tutorial`).
    #[serde(default)]
    pub tutorial: crate::tribal_hud::tutorial::TribalTutorial,
    /// Seconds of a gift timer on this difficulty (0 = the normal 180 s of
    /// saves that predate it), `tribal_rules::gift_timer_seconds`.
    #[serde(default)]
    pub gift_decay_seconds: f32,
    /// Runtime-only: seconds before the next hostile relationship event of a
    /// siege or fight may be posted for each village.
    #[serde(skip)]
    pub hostile_timers: [f32; RIVAL_VILLAGES],
    /// Runtime-only: fractions of a weapon blow the fighters of each village's
    /// foes have thrown since the last `TribeAttack` event (see
    /// [`Self::post_hostile_strikes`]).
    #[serde(skip)]
    pub hostile_strikes: [f32; RIVAL_VILLAGES],
    /// Runtime-only: seconds a raided village stays alerted
    /// (`tribal_rules::RAID_ALERT_SECONDS`); a raider arriving while it runs
    /// belongs to the same raid (one `TribeRaid` event).
    #[serde(skip)]
    pub raid_alerts: [f32; RIVAL_VILLAGES],
    /// Runtime-only: superpower effects waiting for `tribal_power_effects`.
    #[serde(skip)]
    pub pending_power_effects: Vec<PowerEffect>,
    /// The NPC tribes' planners (`tribal_plans`, T9), one per village.
    #[serde(default)]
    pub npc_plans: Vec<crate::tribal_plans::VillagePlanner>,
    /// Hit points the player's tool racks have lost to raid parties; a rack
    /// at its `tribeToolHealth` is destroyed (no tools until repaired).
    #[serde(default)]
    pub shack_damage: BTreeMap<ToolKind, f32>,
    /// Runtime-only: seconds since raiders last struck a building (repairs
    /// start after `tribal_rules::REPAIR_DELAY`).
    #[serde(skip)]
    pub building_quiet: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TotemSegment {
    /// Peaceful performance/diplomacy resolution.
    Allied,
    /// Violent conquest resolution.
    Conquered,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TribalTrapData {
    pub position: [f32; 3],
    pub timer: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CharmedAnimalData {
    pub position: [f32; 3],
    pub timer: f32,
}

/// The chieftain's consequence superpower (one from the Cell card, one from the
/// Creature card). The names, refresh times, ranges and damage are the retail
/// ones (`tribal_rules::SUPERPOWER_*`); the effects are OpenSpore's readings of
/// the descriptions where the original's code is not traced (see each arm of
/// `activate_cell_power` / `activate_creature_power`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TribalPowerKind {
    /// Cell carnivore card (`trg_clg_meat`): Traps.
    Traps,
    /// Cell herbivore card (`trg_clg_plant`): Refreshing Storm.
    RefreshingStorm,
    /// Cell omnivore card (`trg_clg_mix`): Flying Fish.
    FlyingFish,
    /// Creature attack card (`trg_crg_attack`): Fire Bombs.
    FireBombs,
    /// Creature social card (`trg_crg_social`): Fireworks.
    Fireworks,
    /// Creature mix card (`trg_crg_mix`): Beastmaster.
    Beastmaster,
}

impl TribalPowerKind {
    /// The retail superpower record behind this power.
    pub fn superpower(&self) -> &'static crate::tribal_rules::Superpower {
        use crate::tribal_rules as rules;
        match self {
            Self::Traps => &rules::SUPERPOWER_TRAPS,
            Self::RefreshingStorm => &rules::SUPERPOWER_REFRESHING_STORM,
            Self::FlyingFish => &rules::SUPERPOWER_FLYING_FISH,
            Self::FireBombs => &rules::SUPERPOWER_FIRE_BOMBS,
            Self::Fireworks => &rules::SUPERPOWER_FIREWORKS,
            Self::Beastmaster => &rules::SUPERPOWER_BEASTMASTER,
        }
    }

    /// The retail (en-us) name.
    pub fn name(&self) -> &'static str {
        self.superpower().name
    }

    /// A short summary of the retail description (the localized text is
    /// `description_text()` in locale table 0x9A5ACF10).
    pub fn description(&self) -> &'static str {
        self.superpower().description
    }

    /// `(locale table, id)` of the localized name, for HUDs that read the
    /// installed text.
    pub fn name_text(&self) -> (u32, u32) {
        (crate::tribal_rules::SUPERPOWER_LOCALE_TABLE, self.superpower().name_id)
    }

    /// `(locale table, id)` of the localized description.
    pub fn description_text(&self) -> (u32, u32) {
        (
            crate::tribal_rules::SUPERPOWER_LOCALE_TABLE,
            self.superpower().description_id,
        )
    }

    /// Seconds before the power can be used again (`creatureAbilityRefresh`:
    /// 300, Fire Bombs 240, Beastmaster 360).
    pub fn cooldown(&self) -> f32 {
        self.superpower().refresh
    }
}

/// A superpower effect the campaign cannot apply by itself (it needs the
/// world's bushes and creatures); `tribal_power_effects` applies and clears
/// them on the next frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerEffect {
    /// Refreshing Storm: every fruit bush within `radius` of `at` is full again.
    Storm { at: Vec3, radius: f32 },
    /// Fire Bombs: every rival member and wild creature within `radius` of `at`
    /// takes `damage`.
    Blast { at: Vec3, radius: f32, damage: f32 },
}

/// What activating a tribal chieftain power produced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TribalPowerEvent {
    pub message: String,
    pub food_delta: f32,
    pub village: usize,
    pub huts_destroyed: u32,
    pub huts_left: u32,
    pub reward: f32,
    pub resolved_now: bool,
}

/// Pure helper to resolve consequence powers from prior stage metrics.
pub fn resolve_consequence_powers(
    diet_carnivore: f32,
    diet_herbivore: f32,
    allied_species: usize,
    extinct_species: usize,
) -> (TribalPowerKind, TribalPowerKind) {
    let cell_power = if diet_carnivore > diet_herbivore * 1.25 {
        TribalPowerKind::Traps
    } else if diet_herbivore > diet_carnivore * 1.25 {
        TribalPowerKind::RefreshingStorm
    } else {
        TribalPowerKind::FlyingFish
    };

    let creature_power = if extinct_species > allied_species {
        TribalPowerKind::FireBombs
    } else if allied_species > extinct_species {
        TribalPowerKind::Fireworks
    } else if diet_carnivore > diet_herbivore {
        TribalPowerKind::FireBombs
    } else if diet_herbivore > diet_carnivore {
        TribalPowerKind::Fireworks
    } else {
        TribalPowerKind::Beastmaster
    };

    (cell_power, creature_power)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TribalBirth {
    pub slot: u32,
    pub remaining: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TribalStatusSummary {
    pub members: usize,
    pub average_energy_percent: f32,
    pub hungry_members: usize,
    pub pending_births: usize,
    pub next_birth_seconds: Option<f32>,
}

impl TribalCampaign {
    /// The five villages of a brand-new campaign: one NPC tribe per tier from
    /// the lineup, each starting at its lineup relationship. Only the first
    /// `TRIBES_AT_START` (none: the stage starts with the player's tribe alone)
    /// are in view; the others appear over time (`tribal_rules::AppearanceState`,
    /// the retail NPC tribe controller 0xCC6A90).
    pub fn staged_villages(lineup: u8, difficulty: crate::creature_rules::Difficulty) -> Vec<RivalVillageState> {
        (0..RIVAL_VILLAGES)
            .map(|i| {
                let relations = crate::tribal_rules::lineup_tribe(lineup, i, difficulty).1;
                RivalVillageState {
                    relations,
                    baseline: relations,
                    appeared: i < crate::tribal_rules::TRIBES_AT_START,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Start a brand-new campaign for this world: pick the lineup from the
    /// seed and stage the villages (only when the village list is still empty;
    /// a campaign in a save is left alone).
    pub fn ensure_ready_seeded(&mut self, seed: u64, difficulty: crate::creature_rules::Difficulty) {
        self.ensure_ready_lineup(crate::tribal_rules::lineup_for_seed(seed), difficulty);
    }

    /// [`Self::ensure_ready_seeded`] with the lineup chosen by the caller (the
    /// stage start picks it from the player's diet and Creature card,
    /// `tribal_rules::lineup_for_player`).
    pub fn ensure_ready_lineup(&mut self, lineup: u8, difficulty: crate::creature_rules::Difficulty) {
        if self.villages.is_empty() {
            self.lineup = lineup;
            self.villages = Self::staged_villages(self.lineup, difficulty);
            self.gift_decay_seconds = crate::tribal_rules::gift_timer_seconds(difficulty);
            // The camp fire is bound at the stage start: `TRG_Begin` (0xCD4400)
            // binds `TribeFire` to `GetToolByType(10)` and its vignette
            // `TRG_Begin2b` runs `trg_build` at it, so the fire pit stands from
            // the first frame (the 15 food of the `firepit` record are not spent).
            if !self.shacks.contains(&ToolKind::Campfire) {
                self.shacks.insert(0, ToolKind::Campfire);
            }
        }
        self.ensure_ready();
    }

    /// Start the gift timer of `village` (a `TribeGift` / `TribeSocial` event,
    /// 0xDBC28F / 0xDAEB35).
    fn start_gift_timer(&mut self, village: usize) {
        let seconds = if self.gift_decay_seconds > 0.0 { self.gift_decay_seconds } else { 180.0 };
        if let Some(v) = self.villages.get_mut(village) {
            v.gift_timer = seconds;
        }
    }

    /// Every tribe is in view (saves from before tribes appeared over time,
    /// tests of the pure rules).
    pub fn reveal_all(&mut self) {
        self.ensure_ready();
        for v in &mut self.villages {
            v.appeared = true;
        }
    }

    /// The village as the rest of the game sees it: `None` until its tribe has
    /// appeared, so nothing can talk to, attack or be raided by a tribe that
    /// has not come into view.
    pub fn village(&self, index: usize) -> Option<&RivalVillageState> {
        self.villages.get(index).filter(|v| v.appeared)
    }

    /// The village whether or not its tribe has appeared yet.
    pub fn village_raw(&self, index: usize) -> Option<&RivalVillageState> {
        self.villages.get(index)
    }

    /// Tribes in view.
    pub fn appeared_count(&self) -> usize {
        self.villages.iter().filter(|v| v.appeared).count()
    }

    /// Tribes in view that are not yet allied or conquered.
    pub fn unresolved_visible(&self) -> usize {
        self.villages
            .iter()
            .filter(|v| v.appeared && !v.status.is_resolved())
            .count()
    }

    /// The archetype of the tribe in village `index`.
    pub fn tribe_of(&self, index: usize) -> &'static crate::tribal_rules::NpcTribe {
        crate::tribal_rules::village_tribe(self.lineup, index)
    }

    /// The food stock of rival tribe `index` (its `initialfood` until raided).
    pub fn village_food(&self, index: usize) -> f32 {
        self.village(index)
            .map(|v| v.food.unwrap_or(self.tribe_of(index).initial_food))
            .unwrap_or(0.0)
    }

    /// One raider of the player's tribe reaches rival tribe `index`'s food mat:
    /// it takes a bundle (`tribal_rules::raid_take`) from a hostile tribe's
    /// stock, which is returned (0 for a settled or unseen tribe). The first
    /// raider of a raid posts its one `TribeRaid` event (-30, floor -60;
    /// VERIFIED once per raid, 0xDBCEDD..0xDBCF2F) and alerts the tribe for
    /// `RAID_ALERT_SECONDS`; raiders arriving while the alert runs belong to the
    /// same raid.
    pub fn raid_food(&mut self, index: usize) -> f32 {
        let stock = self.village_food(index);
        let Some(v) = self.villages.get_mut(index).filter(|v| v.appeared) else {
            return 0.0;
        };
        if v.status != RivalStatus::Hostile {
            return 0.0;
        }
        let take = crate::tribal_rules::raid_take(stock);
        v.food = Some(stock - take);
        let first = self.raid_alerts.get(index).is_some_and(|a| *a <= 0.0);
        if let Some(alert) = self.raid_alerts.get_mut(index) {
            *alert = crate::tribal_rules::RAID_ALERT_SECONDS;
        }
        if first {
            self.post_event(index, crate::tribal_rules::RelationEvent::Raid);
        }
        take
    }

    /// Whether rival tribe `index` is alerted by a raid.
    pub fn raid_alerted(&self, index: usize) -> bool {
        self.raid_alerts.get(index).is_some_and(|a| *a > 0.0)
    }

    /// Old saves default to an empty village list; refill it to five hostile
    /// villages without ever touching ones already in the save.
    pub fn ensure_ready(&mut self) {
        crate::tribal_rts::livestock::normalize(&mut self.livestock, &mut self.livestock_eggs);
        if self.villages.len() < RIVAL_VILLAGES {
            let old = std::mem::take(&mut self.villages);
            self.villages = (0..RIVAL_VILLAGES)
                .map(|i| old.get(i).cloned().unwrap_or_default())
                .collect();
        } else if self.villages.len() > RIVAL_VILLAGES {
            self.villages.truncate(RIVAL_VILLAGES);
        }
        // A duplicated shack in a hand-edited save would stack two models.
        let mut seen = Vec::new();
        self.shacks.retain(|k| {
            let fresh = !seen.contains(k);
            seen.push(*k);
            fresh
        });
        self.member_energy
            .retain(|slot, energy| *slot < MAX_MATES as u32 && energy.is_finite());
        for energy in self.member_energy.values_mut() {
            *energy = energy.clamp(0.0, MEMBER_ENERGY_MAX);
        }
        let active_slots = self.slots.clone();
        let mut birth_slots = Vec::new();
        self.births.retain(|birth| {
            let fresh = birth.slot < MAX_MATES as u32
                && !active_slots.contains(&birth.slot)
                && !birth_slots.contains(&birth.slot)
                && birth.remaining.is_finite()
                && birth.remaining >= 0.0;
            if fresh {
                birth_slots.push(birth.slot);
            }
            fresh
        });
        if self.totem_carvings.is_empty() {
            for v in &self.villages {
                match v.status {
                    RivalStatus::Allied => self.totem_carvings.push(TotemSegment::Allied),
                    RivalStatus::Conquered => self.totem_carvings.push(TotemSegment::Conquered),
                    RivalStatus::Hostile => {}
                }
            }
        }
    }

    /// Keep the persisted roster aligned with `RunProgress::tribe`: one
    /// stable slot per living member, every slot unique and inside
    /// `MATE_ID_BASE..MATE_ID_BASE + MAX_MATES`. Slots are recycled, so a
    /// member lost in battle frees its id for the next recruit and ids never
    /// run away from the range `persistence::restore` knows about.
    pub fn sync_roster(&mut self, tribe: &mut Vec<MateAssignment>) {
        if tribe.len() > MAX_MATES {
            tribe.truncate(MAX_MATES);
        }
        // Ids are unique by construction, so truncating back to the roster
        // length can never leave a duplicate behind.
        self.slots.truncate(MAX_MATES);
        while self.slots.len() > tribe.len() {
            self.slots.pop();
        }
        while self.slots.len() < tribe.len() {
            let used = self.slots.clone();
            let reserved: Vec<u32> = self.births.iter().map(|birth| birth.slot).collect();
            let Some(free) =
                (0..MAX_MATES as u32).find(|s| !used.contains(s) && !reserved.contains(s))
            else {
                break;
            };
            self.slots.push(free);
        }
        // Tools follow the member; a slot nobody holds carries nothing.
        let slots = self.slots.clone();
        self.tools.retain(|slot, _| slots.contains(slot));
        self.member_energy.retain(|slot, _| slots.contains(slot));
    }

    /// Claim the next free roster slot for a recruit. `None` when the valley
    /// already supports `MAX_MATES` members.
    pub fn take_slot(&mut self) -> Option<u32> {
        let free = (0..MAX_MATES as u32)
            .find(|s| !self.slots.contains(s) && !self.births.iter().any(|b| b.slot == *s))?;
        self.slots.push(free);
        // A recycled slot starts empty-handed.
        self.tools.remove(&free);
        Some(free)
    }

    /// Free the slot of a member that died, returning the roster index the
    /// caller must drop from `RunProgress::tribe` so the two stay parallel.
    pub fn release_slot(&mut self, world_id: u32) -> Option<usize> {
        let slot = world_id.checked_sub(MATE_ID_BASE)?;
        let index = self.slots.iter().position(|s| *s == slot)?;
        self.slots.swap_remove(index);
        self.tools.remove(&slot);
        self.member_energy.remove(&slot);
        Some(index)
    }

    pub fn energy_of(&self, slot: u32) -> f32 {
        self.member_energy
            .get(&slot)
            .copied()
            .unwrap_or(MEMBER_ENERGY_MAX)
            .clamp(0.0, MEMBER_ENERGY_MAX)
    }

    /// Update a member's saved energy; return true when a coarse save-worthy
    /// threshold was crossed, avoiding dirtying saves on every frame.
    pub fn set_energy(&mut self, slot: u32, energy: f32) -> bool {
        let energy = energy.clamp(0.0, MEMBER_ENERGY_MAX);
        let old = self.energy_of(slot);
        self.member_energy.insert(slot, energy);
        (old / 5.0).floor() != (energy / 5.0).floor()
    }

    pub fn pending_birth_count(&self) -> usize {
        self.births.len()
    }

    /// Stable HUD summary of the saved energy roster and growing eggs.
    pub fn status_summary(&self) -> TribalStatusSummary {
        let members = self.slots.len();
        let energy_total: f32 = self.slots.iter().map(|slot| self.energy_of(*slot)).sum();
        TribalStatusSummary {
            members,
            average_energy_percent: if members == 0 {
                0.0
            } else {
                energy_total / members as f32
            },
            hungry_members: self
                .slots
                .iter()
                .filter(|slot| self.energy_of(**slot) <= crate::tribal_rts::ENERGY_RETURN_THRESHOLD)
                .count(),
            pending_births: self.births.len(),
            next_birth_seconds: self
                .births
                .iter()
                .map(|birth| birth.remaining)
                .min_by(f32::total_cmp),
        }
    }

    pub fn member_capacity_used(&self, members: usize) -> usize {
        members.saturating_add(self.births.len())
    }

    /// Remove completed eggs, returning the slots ready to become members.
    pub fn tick_births(&mut self, dt: f32) -> Vec<u32> {
        for birth in &mut self.births {
            let remaining = birth.remaining - dt.max(0.0);
            // A decimal fixture such as 24.9 + 0.1 can leave a few ulps after
            // subtraction. Treat that representational residue as elapsed so
            // growth finishes at its advertised deadline.
            birth.remaining = if remaining <= 1.0e-4 { 0.0 } else { remaining };
        }
        let mut ready = Vec::new();
        self.births.retain(|birth| {
            if birth.remaining <= 0.0 {
                ready.push(birth.slot);
                false
            } else {
                true
            }
        });
        if !ready.is_empty() {
            // The first baby ends the Populate goal (`ShowTutorial(Mating)`, 0xCD764C).
            self.tutorial.baby_born();
        }
        ready
    }

    pub fn cancel_births(&mut self) -> usize {
        let count = self.births.len();
        self.births.clear();
        self.birth_save_timer = 0.0;
        count
    }

    /// Cancel newest eggs if a campaign change leaves fewer beds than the
    /// current living population plus its queued births.
    pub fn trim_births_to_capacity(&mut self, members: usize) -> usize {
        let cap = self.population_cap().min(MAX_MATES);
        let mut cancelled = 0;
        while members.saturating_add(self.births.len()) > cap && !self.births.is_empty() {
            self.births.pop();
            cancelled += 1;
        }
        cancelled
    }

    /// Roster index of a live member's `WorldId`, if it is still enlisted.
    pub fn roster_index(&self, world_id: u32) -> Option<usize> {
        let slot = world_id.checked_sub(MATE_ID_BASE)?;
        self.slots.iter().position(|s| *s == slot)
    }

    pub fn resolved_count(&self) -> usize {
        self.villages
            .iter()
            .filter(|v| v.status.is_resolved())
            .count()
    }

    /// Members the hut currently houses.
    pub fn population_cap(&self) -> usize {
        population_cap(self.resolved_count())
    }

    /// Stage progress as the bar shows it (0xCD8C36): tribes allied or
    /// conquered over `numTribesToConquer`.
    pub fn progress_fraction(&self) -> f32 {
        crate::tribal_rules::progress_fraction(self.resolved_count())
    }

    /// The home hut's level (1..=3, rising at 1 and 3 tribes settled).
    pub fn home_hut_level(&self) -> usize {
        hut_level(self.resolved_count())
    }

    /// The home hut's hit points at its level (400 / 750 / 1000).
    pub fn home_hut_health(&self) -> f32 {
        crate::tribal_rules::hut_health(self.home_hut_level())
    }

    /// Raiders have razed the home hut (its damage reached its health).
    pub fn home_hut_destroyed(&self) -> bool {
        self.home_hut_damage >= self.home_hut_health()
    }

    /// A raid party strikes the home hut (RaidHuts 15).
    pub fn damage_home_hut(&mut self, damage: f32) {
        if damage > 0.0 && damage.is_finite() {
            self.home_hut_damage = (self.home_hut_damage + damage).min(self.home_hut_health());
            self.building_quiet = 0.0;
        }
    }

    /// The player's rack for `kind` is destroyed (its damage reached its
    /// `tribeToolHealth`).
    pub fn shack_destroyed(&self, kind: ToolKind) -> bool {
        self.shack_damage
            .get(&kind)
            .is_some_and(|d| *d >= crate::tribal_rules::tool_health(kind))
    }

    /// A raid party strikes the rack for `kind`; true when this blow destroyed
    /// it.
    pub fn damage_shack(&mut self, kind: ToolKind, damage: f32) -> bool {
        if !(damage > 0.0 && damage.is_finite()) || self.shack_destroyed(kind) {
            return false;
        }
        let health = crate::tribal_rules::tool_health(kind);
        let d = self.shack_damage.entry(kind).or_insert(0.0);
        *d = (*d + damage).min(health);
        self.building_quiet = 0.0;
        *d >= health
    }

    /// Set rival tribe `index`'s food stock.
    pub fn set_village_food(&mut self, index: usize, food: f32) {
        if let Some(v) = self.villages.get_mut(index) {
            v.food = Some(food.clamp(0.0, crate::tribal_rules::TRIBE_MAX_FOOD));
        }
    }

    /// INFERRED repair (`tribeRepairDelay` 20 s, `tribeRepairRatePercent`
    /// 0.5 read as percent of the health per second; the repair code is not
    /// traced): damaged racks and the standing hut mend once no raider has
    /// struck a building for the delay. A destroyed hut stays destroyed (the
    /// stage restarts).
    fn repair_buildings(&mut self, dt: f32) {
        self.building_quiet += dt;
        if self.building_quiet < crate::tribal_rules::REPAIR_DELAY {
            return;
        }
        let rate = crate::tribal_rules::REPAIR_RATE_PERCENT / 100.0 * dt;
        for (kind, damage) in self.shack_damage.iter_mut() {
            *damage = (*damage - crate::tribal_rules::tool_health(*kind) * rate).max(0.0);
        }
        self.shack_damage.retain(|_, d| *d > 0.0);
        if self.home_hut_damage > 0.0 && !self.home_hut_destroyed() {
            self.home_hut_damage = (self.home_hut_damage - self.home_hut_health() * rate).max(0.0);
        }
    }

    /// How tribe `index` stands with the player (`None` until it appears).
    pub fn stance_of(&self, index: usize) -> Option<crate::tribal_rules::Stance> {
        self.village(index).map(|v| {
            crate::tribal_rules::stance(v.relations, v.status == RivalStatus::Allied)
        })
    }

    /// The single win condition: `numTribesToConquer` rival villages allied or
    /// conquered (all five).
    pub fn can_advance(&self) -> bool {
        crate::tribal_rules::stage_complete(self.resolved_count())
    }

    /// The city-hall play-style card for the relationships the tribe holds now
    /// (0xCDBA30 / 0xCDBBD0): military, economic or religious.
    pub fn play_style_card(&self) -> crate::tribal_rules::PlayStyle {
        let relations: Vec<f32> = self.villages.iter().map(|v| v.relations).collect();
        crate::tribal_rules::play_style_from_relations(&relations)
    }

    pub fn say(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.message_timer = 4.0;
    }

    // --- tools -----------------------------------------------------------

    pub fn has_shack(&self, kind: ToolKind) -> bool {
        self.shacks.contains(&kind)
    }

    /// The tool carried by the member under `world_id`.
    pub fn tool_of(&self, world_id: u32) -> Option<ToolKind> {
        let slot = world_id.checked_sub(MATE_ID_BASE)?;
        self.tools.get(&slot).copied()
    }

    /// Build the shack for `kind`, paying its authored price out of `food`.
    /// Returns the food spent, or why it cannot be built.
    pub fn build_shack(&mut self, kind: ToolKind, food: f32) -> Result<f32, String> {
        let info = tools::info(kind);
        if self.has_shack(kind) {
            return Err(format!("{} are already built.", info.name));
        }
        if !tools::is_unlocked(kind, self.resolved_count()) {
            return Err(format!(
                "{} unlock after settling {} village{}.",
                info.name,
                info.tier,
                if info.tier == 1 { "" } else { "s" }
            ));
        }
        if food < info.price {
            return Err(format!(
                "{} cost {:.0} food - gather {:.0} more.",
                info.name,
                info.price,
                info.price - food
            ));
        }
        self.shacks.push(kind);
        self.say(format!("Built {} ({:.0} food).", info.name, info.price));
        Ok(info.price)
    }

    /// Hand the member under `world_id` the tool from a built shack.
    pub fn equip(&mut self, world_id: u32, kind: ToolKind) -> Result<(), String> {
        if kind == ToolKind::Campfire {
            return Err("The campfire stays at home.".into());
        }
        if !self.has_shack(kind) {
            return Err(format!("Build the {} shack first.", tools::info(kind).name));
        }
        if self.shack_destroyed(kind) {
            return Err(format!("Raiders destroyed the {} rack.", tools::info(kind).name));
        }
        let slot = world_id
            .checked_sub(MATE_ID_BASE)
            .filter(|s| self.slots.contains(s))
            .ok_or("Only tribe members carry tools.")?;
        // A rack hands out `toolData_MaxNumSlots` (6) tools; a member who
        // already holds this tool does not count against it.
        let holders = self
            .tools
            .iter()
            .filter(|(s, k)| **k == kind && **s != slot)
            .count();
        if !crate::tribal_rules::rack_has_room(holders) {
            return Err(format!(
                "The {} rack only holds {} tools.",
                tools::info(kind).name,
                crate::tribal_rules::MAX_TOOLS_PER_RACK
            ));
        }
        self.tools.insert(slot, kind);
        Ok(())
    }

    // --- powers and timers -----------------------------------------------

    pub fn cell_power(&self) -> TribalPowerKind {
        self.cell_power.unwrap_or(TribalPowerKind::Traps)
    }

    pub fn creature_power(&self) -> TribalPowerKind {
        self.creature_power.unwrap_or(TribalPowerKind::FireBombs)
    }

    /// Activate the Cell-stage consequence power.
    pub fn activate_cell_power(&mut self, chief_pos: Vec3) -> Result<TribalPowerEvent, String> {
        let power = self.cell_power();
        if self.power_cell_cooldown > 0.0 {
            return Err(format!(
                "{} is on cooldown ({:.0}s left).",
                power.name(),
                self.power_cell_cooldown
            ));
        }
        self.power_cell_cooldown = power.cooldown();
        let mut ev = TribalPowerEvent::default();
        match power {
            // ADAPTED reading of `superpower_carnivore` (LifeTime 25, TrapRange
            // 60, PoisonRange 7): the trap stays armed 25 s, lures wild animals
            // from its range and springs on the first that comes close; the
            // catch is left as a carcass for the hunters (`tribal_rts`).
            TribalPowerKind::Traps => {
                self.active_traps.push(TribalTrapData {
                    position: [chief_pos.x, chief_pos.y, chief_pos.z],
                    timer: crate::tribal_rules::TRAP_LIFETIME,
                });
                ev.message = format!(
                    "{}: the chieftain lays a trap - wild animals nearby are lured to it.",
                    power.name()
                );
            }
            // ADAPTED: the storm reaches `creatureAbilityRange` 20 (scaled)
            // around the chieftain and fills every fruit bush there again.
            TribalPowerKind::RefreshingStorm => {
                self.pending_power_effects.push(PowerEffect::Storm {
                    at: chief_pos,
                    radius: crate::tribal_rules::superpower_reach(power.superpower()),
                });
                ev.message = format!(
                    "{}: rain makes the fruit around the chieftain grow back.",
                    power.name()
                );
            }
            // `NumFish` 7 at `fishingFoodPerFish` 1.5 (ADAPTED: straight to the
            // stock instead of fish landing on the shore).
            TribalPowerKind::FlyingFish => {
                ev.food_delta = crate::tribal_rules::flying_fish_food();
                ev.message = format!(
                    "{}: a sea serpent scares {:.0} fish out of the water (+{:.1} food).",
                    power.name(),
                    crate::tribal_rules::FLYING_FISH_NUM_FISH,
                    ev.food_delta
                );
            }
            _ => {
                ev.message = format!("Activated {}", power.name());
            }
        }
        self.say(ev.message.clone());
        Ok(ev)
    }

    /// Activate the Creature-stage consequence power.
    pub fn activate_creature_power(
        &mut self,
        chief_pos: Vec3,
        target_village: Option<usize>,
    ) -> Result<TribalPowerEvent, String> {
        let power = self.creature_power();
        if self.power_creature_cooldown > 0.0 {
            return Err(format!(
                "{} is on cooldown ({:.0}s left).",
                power.name(),
                self.power_creature_cooldown
            ));
        }
        self.power_creature_cooldown = power.cooldown();
        let mut ev = TribalPowerEvent::default();
        let reach = crate::tribal_rules::superpower_reach(power.superpower());
        match power {
            // `superpower_aggressive`: damage 15, range 15. ADAPTED: one volley
            // hits the target village's buildings and every enemy within reach
            // of its centre for the ability damage (bomb count and area are not
            // traced).
            TribalPowerKind::FireBombs => {
                let damage = power.superpower().damage;
                if let Some(v) = target_village.filter(|v| self.village(*v).is_some()) {
                    let attack_ev = self.attack(v, damage);
                    self.pending_power_effects.push(PowerEffect::Blast {
                        at: village_position(v),
                        radius: reach,
                        damage,
                    });
                    ev.village = v;
                    ev.huts_destroyed = attack_ev.huts_destroyed;
                    ev.huts_left = attack_ev.huts_left;
                    ev.reward = attack_ev.reward;
                    ev.resolved_now = attack_ev.resolved_now;
                    ev.message = format!(
                        "{}: the chieftain bombs V{}! {}",
                        power.name(),
                        v + 1,
                        attack_ev.message
                    );
                } else {
                    self.pending_power_effects.push(PowerEffect::Blast {
                        at: chief_pos,
                        radius: reach,
                        damage,
                    });
                    ev.message = format!("{}: the chieftain throws fire bombs!", power.name());
                }
            }
            // `superpower_social` (range 25): every hostile tribe whose village
            // is within reach of the chieftain gets one `TribeSocialPower`
            // relationship event (+10).
            TribalPowerKind::Fireworks => {
                let mut touched = 0;
                for i in 0..RIVAL_VILLAGES {
                    let hostile = self
                        .village(i)
                        .is_some_and(|v| v.status == RivalStatus::Hostile);
                    if !hostile || dist_xz(chief_pos, village_position(i)) > INTERACT_RANGE + reach {
                        continue;
                    }
                    touched += 1;
                    self.post_event(i, crate::tribal_rules::RelationEvent::SocialPower);
                    if self.village(i).is_some_and(|v| v.status == RivalStatus::Allied) {
                        ev.reward += self.pay_resolve_reward(i);
                        ev.resolved_now = true;
                    }
                }
                ev.message = if ev.resolved_now {
                    format!("{}: the display wins a new ally!", power.name())
                } else if touched > 0 {
                    format!(
                        "{}: {} neighbouring tribe{} enjoyed the show (+{:.0}).",
                        power.name(),
                        touched,
                        if touched == 1 { "" } else { "s" },
                        crate::tribal_rules::SOCIAL_POWER_RELATIONSHIP
                    )
                } else {
                    format!("{}: no tribe is close enough to see them.", power.name())
                };
            }
            // `superpower_versatile` (range 50; unnamed 120 read as the charm
            // time, ADAPTED): animals within reach fight for the tribe.
            TribalPowerKind::Beastmaster => {
                self.charmed_creatures.push(CharmedAnimalData {
                    position: [chief_pos.x, chief_pos.y, chief_pos.z],
                    timer: crate::tribal_rules::BEASTMASTER_DURATION,
                });
                ev.message = format!(
                    "{}: the animals around the chieftain rally to the tribe.",
                    power.name()
                );
            }
            _ => {
                ev.message = format!("Activated {}", power.name());
            }
        }
        self.say(ev.message.clone());
        Ok(ev)
    }

    /// Count the gift timers down (0xCC6D5A): when one runs out the tribe's
    /// relationship fades one attitude step and the timer restarts while the
    /// tribe is still friendlier than the attitude it started with.
    fn tick_gift_timers(&mut self, dt: f32) {
        let seconds = if self.gift_decay_seconds > 0.0 { self.gift_decay_seconds } else { 180.0 };
        // The tribe a song is for does not fade while it plays (0xDA1A3E: the
        // performance sets the timer aside when it starts).
        let performing = self.performance.map(|p| p.village);
        for (index, v) in self.villages.iter_mut().enumerate() {
            if v.gift_timer <= 0.0 || performing == Some(index) {
                continue;
            }
            if v.status != RivalStatus::Hostile {
                v.gift_timer = 0.0;
                continue;
            }
            v.gift_timer -= dt;
            if v.gift_timer > 0.0 {
                continue;
            }
            let decay = crate::tribal_rules::gift_decay(v.relations, v.baseline);
            v.relations = decay.relations;
            v.gift_timer = if decay.restart { seconds } else { 0.0 };
        }
    }

    /// Advance every persisted timer. An unanswered note counts as a miss
    /// and the song moves on; a forgotten song ends by itself.
    pub fn tick(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.recruit_cooldown = (self.recruit_cooldown - dt).max(0.0);
        self.message_timer = (self.message_timer - dt).max(0.0);
        self.power_cell_cooldown = (self.power_cell_cooldown - dt).max(0.0);
        self.power_creature_cooldown = (self.power_creature_cooldown - dt).max(0.0);
        self.active_traps.retain_mut(|t| {
            t.timer -= dt;
            t.timer > 0.0
        });
        self.charmed_creatures.retain_mut(|c| {
            c.timer -= dt;
            c.timer > 0.0
        });
        for timer in &mut self.hostile_timers {
            *timer = (*timer - dt).max(0.0);
        }
        for timer in &mut self.raid_alerts {
            *timer = (*timer - dt).max(0.0);
        }
        self.tick_gift_timers(dt);
        self.repair_buildings(dt);
        // Raided stocks grow back (ADAPTED flat `FoodRate`; the retail upkeep
        // 0xCE3CC0 needs the NPC gather / fish plans, see `tribal_rules::NPC_FOOD_RATE`).
        for index in 0..self.villages.len() {
            let initial = self.tribe_of(index).initial_food;
            if let Some(food) = self.villages[index].food {
                self.villages[index].food =
                    Some(crate::tribal_rules::npc_food_regrow(food, initial, dt));
            }
        }
        self.tick_performance(dt);
        self.tick_social_end(dt);
    }

    /// Resolve a village once and only once. Returns the food reward the
    /// first time and nothing ever again.
    fn pay_resolve_reward(&mut self, index: usize) -> f32 {
        let Some(v) = self.villages.get_mut(index) else {
            return 0.0;
        };
        if v.rewarded {
            return 0.0;
        }
        v.rewarded = true;
        if v.status == RivalStatus::Allied {
            ALLY_REWARD
        } else {
            CONQUER_REWARD
        }
    }

    fn set_relations(&mut self, index: usize, relations: f32) {
        if let Some(v) = self.villages.get_mut(index) {
            v.relations = relations.clamp(
                crate::tribal_rules::RELATION_MIN,
                crate::tribal_rules::RELATION_MAX,
            );
            if crate::tribal_rules::allies(v.relations) && v.status == RivalStatus::Hostile {
                v.status = RivalStatus::Allied;
                self.totem_carvings.push(TotemSegment::Allied);
            }
        }
    }

    /// Post a relationship event with a tribe (T3A: attack -10 floor -60, kill
    /// -60 floor -100, raid -30, steal a baby -30, attack a tool or hut -10,
    /// destroy a tool -100, gift +10, social +10). Only a tribe still in play
    /// reacts: allied and conquered villages are settled for good. Returns the
    /// relationship afterwards.
    pub fn post_event(&mut self, village: usize, event: crate::tribal_rules::RelationEvent) -> f32 {
        let Some(v) = self.village(village) else {
            return 0.0;
        };
        if v.status != RivalStatus::Hostile {
            return v.relations;
        }
        let next = crate::tribal_rules::apply_event(v.relations, event);
        self.set_relations(village, next);
        if matches!(event, crate::tribal_rules::RelationEvent::Gift | crate::tribal_rules::RelationEvent::Social) {
            self.start_gift_timer(village);
        }
        next
    }

    /// Strikes at a village's members. The original posts `TribeAttack` for
    /// every blow that hurts a citizen of another tribe and leaves it alive
    /// (0xC26940 -> 0xC26C85, the citizen's damage handler: `ApplyRelationship(
    /// TribeAttack, 1.0)`, no cadence of its own). Members here strike
    /// continuously (`dps x dt`), so `strikes` is the fraction of a blow thrown
    /// this frame (`dt / weapon refresh`); one event is posted per whole blow,
    /// the fractions carrying over. Returns the events posted.
    pub fn post_hostile_strikes(
        &mut self,
        village: usize,
        event: crate::tribal_rules::RelationEvent,
        strikes: f32,
    ) -> u32 {
        let Some(carry) = self.hostile_strikes.get_mut(village) else {
            return 0;
        };
        if !strikes.is_finite() || strikes <= 0.0 {
            return 0;
        }
        *carry += strikes;
        let mut posted = 0;
        while *carry >= 1.0 && posted < 8 {
            *carry -= 1.0;
            posted += 1;
        }
        *carry = carry.min(1.0);
        for _ in 0..posted {
            self.post_event(village, event);
        }
        posted
    }

    /// A repeated hostile hit on a piece of the village (the retail
    /// `TribeAttackToolOrHut`, 0xC9CAFB / 0xC9D899, is posted for every blow
    /// too): posts `event` at most once per `HOSTILE_EVENT_INTERVAL` seconds per
    /// village (ADAPTED cadence: the siege damage is a continuous rate here, not
    /// blows).
    pub fn post_hostile_hit(
        &mut self,
        village: usize,
        event: crate::tribal_rules::RelationEvent,
    ) -> bool {
        let Some(timer) = self.hostile_timers.get_mut(village) else {
            return false;
        };
        if *timer > 0.0 {
            return false;
        }
        *timer = crate::tribal_rules::HOSTILE_EVENT_INTERVAL;
        self.post_event(village, event);
        true
    }

    // --- performances ----------------------------------------------------

    /// Start a song for `village` with a troupe that carries every instrument.
    pub fn start_performance(&mut self, village: usize) -> Option<()> {
        self.start_performance_troupe(village, Troupe::FULL).ok()
    }

    /// Start the social game at `village` with `troupe` at once (the headless
    /// callers: tests, the outfitter's fixtures). The game's own command goes
    /// through [`Self::start_performance_gathering`].
    pub fn start_performance_troupe(&mut self, village: usize, troupe: Troupe) -> Result<(), String> {
        self.start_performance_with(village, troupe, false)
    }

    /// Start the social game at `village` with `troupe` the way retail does
    /// (states 0..4 of 0xDA2F30): the song exists, its bar is up, but the music,
    /// the clock and the start score wait until every participant stands at its
    /// spot ([`Self::troupe_ready`], called by the Tribal simulation).
    pub fn start_performance_gathering(&mut self, village: usize, troupe: Troupe) -> Result<(), String> {
        self.start_performance_with(village, troupe, true)
    }

    fn start_performance_with(&mut self, village: usize, troupe: Troupe, gathering: bool) -> Result<(), String> {
        if self.performance.is_some() {
            return Err("A performance is already under way.".into());
        }
        if self.cooldown > 0.0 {
            return Err("Give them a moment before performing again.".into());
        }
        match self.village(village).map(|v| v.status) {
            None => return Err("No such village.".into()),
            Some(RivalStatus::Hostile) => {}
            Some(status) => return Err(format!("V{} is already {}.", village + 1, status.label())),
        }
        let song = self.round;
        self.round = self.round.wrapping_add(1);
        let relations = self.village(village).map(|v| v.relations).unwrap_or_default();
        let level = crate::tribal_rules::social_level(relations);
        let archetype = self.tribe_of(village).archetype;
        let tuning = crate::tribal_rules::social_tuning(archetype);
        let resolved = self.resolved_count();
        let unlocked = [ToolKind::WoodenHorns, ToolKind::Maracas, ToolKind::Aerophones]
            .iter()
            .enumerate()
            .fold(0u8, |m, (i, k)| if tools::is_unlocked(*k, resolved) { m | (1 << i) } else { m });
        // 0xD9CCF0: the start score. The social level is the outfit's
        // (`outfit_social_bonus` already holds level x 35).
        let score = crate::tribal_rules::social_start_score(0, troupe.participants) + self.outfit_social_bonus;
        // 0xDA1A3E: the tribe's gift timer is set aside for the song.
        let saved_timer = self.villages.get(village).map_or(0.0, |v| v.gift_timer);
        if let Some(v) = self.villages.get_mut(village) {
            v.gift_timer = 0.0;
        }
        let mut performance = Performance {
            village,
            prompt: 0,
            time_left: NOTE_WINDOW,
            note: 0,
            notes: crate::tribal_rules::SOCIAL_ROUNDS,
            hits: 0,
            misses: 0,
            song,
            required: 0,
            level,
            audience_answered: false,
            score: 0.0,
            threshold: crate::tribal_rules::social_threshold(&tuning, level),
            bubble: false,
            bubbled: false,
            evaluated: true,
            eval_in: 0.0,
            ordered: [0; 3],
            refresh: [0.0; 3],
            idle_beats: 0,
            first_order: false,
            archetype,
            unlocked,
            saved_timer,
            orders: 0,
            last_order: 0,
            answers: 0,
            bar: Default::default(),
            bubble_serial: 0,
            reaction: 0,
            reaction_serial: 0,
            gathering,
            gather_time: 0.0,
            start_score: if gathering { score } else { 0.0 },
        };
        // 0xD9CCF0 -> 0xD9CC20: the start score goes through the score object like
        // every other score, so the bar fills with it over the whole song
        // (`rounds x roundLength` seconds). Retail runs it when the troupe is in
        // place and the music has set the rounds (state 4 -> 5).
        if !gathering {
            performance.add_score(score, performance.notes as f32 * NOTE_WINDOW);
        }
        self.performance = Some(performance);
        self.social_end = None;
        self.song_stands.clear();
        // 0xDAEA70: every member of the village that watches answers once.
        for _ in 0..troupe.audience {
            self.audience_answer(village);
        }
        self.social_events.0.push(SocialEvent::Started { seen: self.social_tutorial_done });
        let threshold = self.performance.map_or(0.0, |p| p.threshold);
        self.say(format!(
            "The troupe strikes up for V{} - fill the bar ({:.0}/{:.0}): order the instrument the crowd calls for!",
            village + 1,
            score,
            threshold
        ));
        Ok(())
    }

    /// One audience member of the village answers (0xDAEA70, state 0, once per
    /// member): `TribeSocial` with scale `NeutralCap - relationship` while that
    /// is positive (it lifts the tribe to Pleased, never beyond), then the gift
    /// timer starts (0xDAEB18..0xDAEB35).
    fn audience_answer(&mut self, village: usize) {
        let relations = self.village(village).map(|v| v.relations).unwrap_or_default();
        let lifted = crate::tribal_rules::social_lift(relations, crate::tribal_rules::SOCIAL_NEUTRAL_CAP);
        self.set_relations(village, lifted);
        self.start_gift_timer(village);
        if let Some(p) = self.performance.as_mut() {
            p.audience_answered = true;
            p.answers += 1;
        }
    }

    /// Every participant stands at its spot (the performance state 3 -> 4 -> 5,
    /// 0xDA3306 / 0xDA3461: the counts go to the music patch, which sets the rounds
    /// and the round length at once, and 0xD9CCF0 scores the start): the clock
    /// starts, the start score fills the bar over the whole song. Returns whether a
    /// gathering song began.
    pub fn troupe_ready(&mut self) -> bool {
        let Some(mut p) = self.performance else {
            return false;
        };
        if !p.gathering {
            return false;
        }
        p.begin();
        self.performance = Some(p);
        true
    }

    /// The spot the song's layout gave the member `who` (entity bits). It stays
    /// until the member notices the song is over ([`Self::forget_stand`]) and goes
    /// back to idling where it stands.
    pub fn song_stand(&self, who: u64) -> Option<Vec3> {
        self.song_stands.iter().find(|(e, _)| *e == who).map(|(_, p)| *p)
    }

    /// The member `who` has left the song.
    pub fn forget_stand(&mut self, who: u64) {
        self.song_stands.retain(|(e, _)| *e != who);
    }

    /// A performance is answered from where the troupe is standing, so the
    /// chieftain has to stay within reach of the village it is for.
    pub fn answer_in_range(&self, pos: Vec3) -> bool {
        self.performance
            .is_some_and(|p| dist_xz(pos, village_position(p.village)) <= INTERACT_RANGE)
    }

    /// The chieftain wandered off mid-performance: the game stops (0xDA34BE
    /// `trg_social_stop_on_beat`) without a result; the gift timer it set aside
    /// runs again.
    pub fn abandon_performance(&mut self, reason: impl Into<String>) {
        if let Some(p) = self.performance.take() {
            self.restore_timer(&p);
            self.social_events.0.push(SocialEvent::Ended);
        }
        self.say(reason);
    }

    /// Give the gift timer back (0xD9FE60: `max(saved, 30)` when something was set
    /// aside).
    fn restore_timer(&mut self, p: &Performance) {
        if p.saved_timer > 0.0 {
            if let Some(v) = self.villages.get_mut(p.village) {
                v.gift_timer = p.saved_timer.max(crate::tribal_rules::GIFT_TIMER_AFTER_SONG_MIN);
            }
        }
    }

    /// Order an instrument (the social ability button: horn `3`, maracas `4`,
    /// didgeridoo `5`; ability types 5 / 6 / 7, refresh 5 s). `available` is the
    /// number of troupe members carrying each instrument. Every member of that
    /// instrument plays and the round is evaluated at once (0xDA35D0..0xDA3625
    /// marks the members, 0xDA0100 scores them). Returns whether the order was
    /// given.
    pub fn order_instrument(&mut self, note: u32, available: [u32; 3]) -> bool {
        let Some(mut p) = self.performance else {
            self.say("Nothing to play - press E near a hostile village.");
            return false;
        };
        if p.gathering {
            self.say("The troupe is still taking its places.");
            return false;
        }
        let Some(kind) = instrument_kind(note) else {
            return false;
        };
        if available[kind] == 0 {
            self.say(format!("Nobody in the troupe plays the {}", ability_name(note)));
            return false;
        }
        if p.refresh[kind] > 0.0 {
            return false;
        }
        p.refresh[kind] = crate::tribal_rules::SOCIAL_ORDER_REFRESH;
        p.ordered[kind] += available[kind];
        p.orders += 1;
        p.last_order = note;
        let first = !p.first_order;
        p.first_order = true;
        p.evaluate();
        self.performance = Some(p);
        if first {
            self.social_event_first_order();
        }
        true
    }

    /// The first order of a song sets the tutorial flag and ends its hint
    /// (0xDA3181..0xDA31B1).
    fn social_event_first_order(&mut self) {
        self.social_tutorial_done = true;
        self.social_events.0.push(SocialEvent::FirstOrder);
    }

    /// The ended song's bar keeps animating (a failed bar drains) until it hides.
    fn tick_social_end(&mut self, dt: f32) {
        if let Some(end) = self.social_end.as_mut() {
            end.meter.update(dt);
            end.left -= dt;
            if end.left <= 0.0 {
                self.social_end = None;
            }
        }
    }

    /// Advance the song by `dt` seconds. Sub-steps keep the events in order when
    /// a caller advances by more than a frame.
    fn tick_performance(&mut self, dt: f32) {
        let mut left = dt.max(0.0);
        while left > 0.0 && self.performance.is_some() {
            let h = left.min(0.05);
            left -= h;
            self.step_performance(h);
        }
    }

    fn step_performance(&mut self, h: f32) {
        let Some(mut p) = self.performance else {
            return;
        };
        for r in &mut p.refresh {
            *r = (*r - h).max(0.0);
        }
        // The bar object's own update (0xE31380 from the Tribal UI's update).
        p.bar.update(h);
        if p.gathering {
            // States 0..4: the troupe walks to its spots; the game begins when the
            // simulation says everybody is there (or the safety timeout runs out).
            p.gather_time += h;
            if p.gather_time >= crate::tribal_rules::SOCIAL_GATHER_TIMEOUT {
                p.begin();
            }
            self.performance = Some(p);
            return;
        }
        p.time_left -= h;
        // The bubble one second before the beat (the patch's
        // `trg_social_thought_bubble`), while rounds remain (0xDA20E8).
        let mut reminder = false;
        if p.note < p.notes && !p.bubbled && p.time_left <= crate::tribal_rules::SOCIAL_BUBBLE_LEAD {
            reminder = p.raise_bubble();
        }
        // The evaluation a beat without an order set (0xDA31B6..0xDA31E0).
        if p.eval_in > 0.0 {
            p.eval_in -= h;
            if p.eval_in <= 0.0 {
                p.eval_in = 0.0;
                p.evaluate();
                p.idle_beats += 1;
            }
        }
        // The beat (0xDA1F67): a round counted, the grace timer when the round
        // still waits for its evaluation.
        if p.time_left <= 0.0 {
            p.time_left += NOTE_WINDOW;
            if p.note < p.notes {
                p.note += 1;
                if p.bubble && !p.evaluated {
                    p.eval_in = crate::tribal_rules::SOCIAL_EVAL_DELAY;
                }
                p.bubbled = false;
            }
        }
        self.performance = Some(p);
        if reminder {
            self.social_events.0.push(SocialEvent::Reminder);
        }
        // 0xDA3490: the bar object's fill reaching 1 (0xE310D0) is a success at once
        // (a song without a bar has none); no rounds left, nothing waiting for its
        // evaluation and no fill still on its way is a failure (0xDA357A..0xDA35AA).
        // A level 0 song has no bar object (0xDA1874 skips it, and every performance
        // is a fresh object: the default constructor 0xDA4E60 clears `+0xE8`), so
        // its end path 0xDA35B1 has no success branch: it ends after the last
        // round, never as a success (VERIFIED in tribal-social3; the earlier port
        // let it succeed by its score).
        let has_bar = p.level > 0;
        let full = has_bar && p.bar.is_full();
        if full {
            self.finish_performance(true);
        } else if p.note >= p.notes && p.eval_in <= 0.0 && !(has_bar && p.bar.animating()) {
            self.finish_performance(false);
        }
    }

    /// The end of the game (0xDA3490..0xDA3578, 0xD9FE60). A success lifts the
    /// tribe to the target of the song's level and clears the gift timer for
    /// good; a failure does nothing and gives back the timer set aside.
    fn finish_performance(&mut self, success: bool) -> TribalEvent {
        let Some(p) = self.performance.take() else {
            return TribalEvent::default();
        };
        let before = self
            .village(p.village)
            .map(|v| v.status)
            .unwrap_or(RivalStatus::Hostile);
        let mut relations = self.village(p.village).map(|v| v.relations).unwrap_or_default();
        if success {
            if let Some(target) = crate::tribal_rules::social_target(p.level) {
                relations = crate::tribal_rules::social_lift(relations, target);
            }
            if let Some(v) = self.villages.get_mut(p.village) {
                v.gift_timer = 0.0;
            }
        } else {
            self.restore_timer(&p);
        }
        self.set_relations(p.village, relations);
        let after = self
            .village(p.village)
            .map(|v| v.status)
            .unwrap_or(RivalStatus::Hostile);
        let relations = self.village(p.village).map(|v| v.relations).unwrap_or(0.0);
        let resolved_now = before == RivalStatus::Hostile && after.is_resolved();
        let reward = if resolved_now {
            self.pay_resolve_reward(p.village)
        } else {
            0.0
        };
        let bar = crate::tribal_rules::social_bar(p.score, p.threshold);
        // The bar stays up for the ending (the stop 0xD9FE60 hides it once the
        // members' ending clips are done): a success shows the star burst
        // (0xE31150), a failure starts the drain (0xE31610). A level 0 song has no
        // bar but its members play the same ending clips.
        self.social_end = Some({
            let mut meter = p.bar;
            if !success && p.level > 0 {
                meter.fail();
            }
            SocialEnd {
                meter,
                village: p.village,
                level: p.level,
                success,
                left: crate::tribal_rules::social_ending_seconds(bar),
                has_bar: p.level > 0,
                bar,
            }
        });
        let message = if resolved_now {
            format!("V{} joins the dance - now {}!", p.village + 1, after.label())
        } else {
            format!(
                "Song over: {:.0}/{:.0} for V{}{}. Relations {:.0}",
                p.score,
                p.threshold,
                p.village + 1,
                if success {
                    " - they warm to you"
                } else if crate::tribal_rules::social_ending_cheers(bar) {
                    " - the crowd cheers anyway"
                } else {
                    " - the crowd shrugs"
                },
                relations
            )
        };
        self.cooldown = ACTION_COOLDOWN;
        self.social_events.0.push(SocialEvent::Ended);
        self.say(message.clone());
        TribalEvent {
            message,
            relations,
            status: after,
            food_delta: 0.0,
            reward,
            huts_left: self.village(p.village).map(|v| v.huts).unwrap_or(0),
            huts_destroyed: 0,
            resolved_now,
        }
    }

    /// Gift food to `village`. Needs a real stockpile; `food` is what the
    /// caller currently holds. The relationship rule is the retail one
    /// (`tribal_rules::gift_effect`, 0xDBC1F1..0xDBC25C): a delivered gift
    /// calms a Furious or Annoyed tribe to Cautious (0) and does nothing for a
    /// tribe that is at Cautious or better; the food is gone either way and the
    /// gift starts the timer that fades the effect again (0xDBC277..0xDBC28F).
    pub fn gift(&mut self, village: usize, food: f32) -> TribalEvent {
        let Some(v) = self.village(village) else {
            return TribalEvent::default();
        };
        if v.status != RivalStatus::Hostile {
            self.say(format!("V{} is already {}.", village + 1, v.status.label()));
            return TribalEvent::default();
        }
        if food < GIFT_FOOD {
            self.say(format!(
                "A gift costs {:.0} food - gather {:.0} more.",
                GIFT_FOOD,
                GIFT_FOOD - food
            ));
            return TribalEvent::default();
        }
        if self.cooldown > 0.0 {
            return TribalEvent::default();
        }
        let before = v.relations;
        self.deliver_gift(village);
        self.cooldown = ACTION_COOLDOWN;
        let relations = self.village(village).map(|v| v.relations).unwrap_or(0.0);
        let message = if relations > before {
            format!(
                "V{} accepts the gift and calms down. Relations {:.0} - now perform for them!",
                village + 1,
                relations
            )
        } else {
            format!(
                "V{} takes the food. Relations {:.0} - perform for them!",
                village + 1,
                relations
            )
        };
        self.say(message.clone());
        TribalEvent {
            message,
            relations,
            status: RivalStatus::Hostile,
            food_delta: -GIFT_FOOD,
            reward: 0.0,
            huts_left: self.village(village).map(|v| v.huts).unwrap_or(0),
            huts_destroyed: 0,
            resolved_now: false,
        }
    }

    /// The food of a gift reaches the village's mat (`TribeGift`, 0xDBC1B2): the
    /// relationship rule of [`tribal_rules::gift_effect`] and the fading timer.
    /// Used by [`Self::gift`] and by a carrier that hands the bundle over.
    pub fn deliver_gift(&mut self, village: usize) {
        let Some(v) = self.village(village) else {
            return;
        };
        if v.status != RivalStatus::Hostile {
            return;
        }
        let next = crate::tribal_rules::gift_effect(v.relations);
        self.set_relations(village, next);
        self.start_gift_timer(village);
    }

    /// Apply `damage` (hit points) to `village`. Its pieces fall from the last
    /// slot down: the two tool racks (200 hit points each), then the dwelling (a
    /// hut of 400 / 750 / 1000 by the tribe's tier). Resolves it once the last
    /// piece falls; a resolved village takes no further damage and pays nothing.
    /// A blow posts `TribeAttackToolOrHut` (at most every few seconds) and every
    /// piece that falls `TribeDestroyTool`.
    pub fn attack(&mut self, village: usize, damage: f32) -> TribalEvent {
        let tier = village_tier(village);
        let Some(v) = self.villages.get_mut(village).filter(|v| v.appeared) else {
            return TribalEvent::default();
        };
        if v.status != RivalStatus::Hostile || damage <= 0.0 || !damage.is_finite() {
            let huts_left = v.huts;
            return TribalEvent {
                huts_left,
                ..default_event()
            };
        }
        v.hurt += damage;
        let mut destroyed = 0;
        while v.huts > 0 {
            let hp = crate::tribal_rules::village_piece_hp(tier, v.huts - 1);
            if v.hurt < hp {
                break;
            }
            v.hurt -= hp;
            v.huts -= 1;
            destroyed += 1;
        }
        let huts_left = v.huts;
        let resolved_now = huts_left == 0;
        // Relationship events come before the village is marked conquered, so
        // the last piece still counts.
        self.post_hostile_hit(village, crate::tribal_rules::RelationEvent::AttackToolOrHut);
        for _ in 0..destroyed {
            self.post_event(village, crate::tribal_rules::RelationEvent::DestroyTool);
        }
        if resolved_now {
            if let Some(v) = self.villages.get_mut(village) {
                v.status = RivalStatus::Conquered;
                v.hurt = 0.0;
            }
            self.totem_carvings.push(TotemSegment::Conquered);
            // A conquered village has no relations left to perform for.
            if let Some(p) = self.performance {
                if p.village == village {
                    self.performance = None;
                }
            }
        }
        let reward = if resolved_now {
            self.pay_resolve_reward(village)
        } else {
            0.0
        };
        let message = if resolved_now {
            format!("V{} is CONQUERED!", village + 1)
        } else if destroyed > 0 {
            format!("V{} loses a piece ({} left)!", village + 1, huts_left)
        } else {
            let hurt = self.village(village).map(|v| v.hurt).unwrap_or(0.0);
            let piece = crate::tribal_rules::village_piece_hp(tier, huts_left.saturating_sub(1));
            format!(
                "The tribe batters V{} ({:.0}% to the next piece).",
                village + 1,
                100.0 * hurt / piece
            )
        };
        self.say(message.clone());
        TribalEvent {
            message,
            relations: self.village(village).map(|v| v.relations).unwrap_or(0.0),
            status: RivalStatus::Conquered,
            food_delta: 0.0,
            reward,
            huts_left,
            huts_destroyed: destroyed,
            resolved_now,
        }
    }

    /// Spend food for a paid egg. It becomes a member after its growth timer.
    pub fn recruit(&mut self, food: f32, members: usize) -> bool {
        let used = self.member_capacity_used(members);
        if used >= MAX_MATES {
            self.say("The tribe is as big as the valley supports.");
            return false;
        }
        let cap = self.population_cap();
        if used >= cap {
            self.say(format!(
                "The hut houses {cap} members - settle another village to grow it."
            ));
            return false;
        }
        if self.recruit_cooldown > 0.0 {
            return false;
        }
        if food < RECRUIT_FOOD {
            self.say(format!(
                "A new member costs {:.0} food - gather {:.0} more.",
                RECRUIT_FOOD,
                RECRUIT_FOOD - food
            ));
            return false;
        }
        let Some(slot) = (0..MAX_MATES as u32)
            .find(|s| !self.slots.contains(s) && !self.births.iter().any(|b| b.slot == *s))
        else {
            self.say("There is no room for another egg.");
            return false;
        };
        self.recruit_cooldown = ACTION_COOLDOWN;
        self.births.push(TribalBirth {
            slot,
            remaining: BIRTH_GROWTH_SECONDS,
        });
        self.say(format!(
            "A new egg is growing at the pen ({:.0}s; {} food paid).",
            BIRTH_GROWTH_SECONDS, RECRUIT_FOOD
        ));
        true
    }

    /// Spend `RECOVER_FOOD` to heal the chieftain. Returns true when paid.
    pub fn recover(&mut self, food: f32, missing: f32) -> bool {
        if missing <= 0.0 {
            return false;
        }
        if self.recruit_cooldown > 0.0 {
            return false;
        }
        if food < RECOVER_FOOD {
            self.say(format!(
                "Recovery costs {:.0} food - gather {:.0} more.",
                RECOVER_FOOD,
                RECOVER_FOOD - food
            ));
            return false;
        }
        self.recruit_cooldown = ACTION_COOLDOWN;
        self.say(format!("The tribe patches you up ({} food).", RECOVER_FOOD));
        true
    }

    /// Multi-line status shown by the HUD ahead of the shared objective line.
    pub fn hud(&self) -> String {
        let resolved = self.resolved_count();
        let mut head = match self.performance {
            Some(p) => format!(
                "V{} song: round {}/{}  score {:.0}/{:.0}{}",
                p.village + 1,
                (p.note + 1).min(p.notes),
                p.notes,
                p.score,
                p.threshold,
                if p.bubble {
                    format!("  - V{} asks for {} x{} [{}]", p.village + 1, ability_name(p.prompt), p.required, p.prompt)
                } else {
                    String::new()
                }
            ),
            None if resolved >= RIVAL_VILLAGES => {
                "All five villages united - the tribe marches on!".to_string()
            }
            None => format!(
                "Unite all {} villages: gift (G) + perform (E), or attack (F)  [{}/{}]",
                RIVAL_VILLAGES, resolved, RIVAL_VILLAGES
            ),
        };
        let mut row = String::new();
        for (i, v) in self.villages.iter().enumerate() {
            if !row.is_empty() {
                row.push_str("  ");
            }
            row.push_str(&format!("V{} ", i + 1));
            if !v.appeared {
                row.push_str("not yet seen");
            } else if v.status == RivalStatus::Hostile {
                row.push_str(&format!(
                    "hostile {} piece{} rel {:.0}",
                    v.huts,
                    if v.huts == 1 { "" } else { "s" },
                    v.relations
                ));
            } else {
                row.push_str(v.status.label());
            }
        }
        head.push('\n');
        head.push_str(&row);
        if let Some(next) = self
            .births
            .iter()
            .map(|birth| birth.remaining)
            .min_by(f32::total_cmp)
        {
            head.push_str(&format!(
                "\n{} egg{} growing at the pen; next hatches in {:.0}s.",
                self.births.len(),
                if self.births.len() == 1 { "" } else { "s" },
                next.max(0.0)
            ));
        }
        let p1 = self.cell_power();
        let p2 = self.creature_power();
        let p1_cd = if self.power_cell_cooldown <= 0.0 {
            "Ready".to_string()
        } else {
            format!("{:.0}s", self.power_cell_cooldown)
        };
        let p2_cd = if self.power_creature_cooldown <= 0.0 {
            "Ready".to_string()
        } else {
            format!("{:.0}s", self.power_creature_cooldown)
        };
        head.push_str(&format!(
            "\nPowers: [1] {} [{}]  |  [2] {} [{}]",
            p1.name(),
            p1_cd,
            p2.name(),
            p2_cd
        ));
        if !self.totem_carvings.is_empty() {
            let carvings = self
                .totem_carvings
                .iter()
                .map(|c| match c {
                    TotemSegment::Allied => "Allied",
                    TotemSegment::Conquered => "Conquered",
                })
                .collect::<Vec<_>>()
                .join(", ");
            head.push_str(&format!("\nTotem: [{carvings}]"));
        }
        if !self.message.is_empty() && self.message_timer > 0.0 {
            head.push('\n');
            head.push_str(&self.message);
        }
        head
    }
}

fn default_event() -> TribalEvent {
    TribalEvent::default()
}

// ---------------------------------------------------------------------------
// Pure combat / economy helpers
// ---------------------------------------------------------------------------

/// Hit points of damage per second of a chieftain-led siege with `living_mates`
/// bare-handed members at the walls. Warrior roles raise it.
pub fn attack_power(roles: &[u32], living_mates: u32) -> f32 {
    siege_damage(true, &vec![None; living_mates as usize], roles)
}

/// Hit points of damage per second to a village: the chieftain's staff (when he
/// leads with `F`) plus every besieging member, each with the raze damage and
/// refresh of the weapon its tool gives (`creatureAbilityRazeDamage`: torch 9
/// per 1.5 s, axe 5 per 2 s, spear 4 per 3 s, chieftain staff 7 per 2 s, shaman
/// staff 3 per 2.5 s; bare hands ADAPTED 1 per 1.5 s).
pub fn siege_damage(chieftain: bool, attackers: &[Option<ToolKind>], roles: &[u32]) -> f32 {
    let members: f32 = attackers
        .iter()
        .map(|t| crate::tribal_rules::weapon_of(*t).raze_dps())
        .sum();
    (if chieftain {
        crate::tribal_rules::chieftain_raze_dps()
    } else {
        0.0
    } + members)
        * defense_multiplier(roles)
}

/// Damage per second one defender of `tribe` deals (the average of its roles'
/// weapons), softened by warrior roles.
pub fn retaliation_dps(tribe: &crate::tribal_rules::NpcTribe, roles: &[u32]) -> f32 {
    crate::tribal_rules::defender_dps(tribe) / defense_multiplier(roles).max(0.5)
}

// ---------------------------------------------------------------------------
// Scene: villages, raiders, the tribe and stage setup
// ---------------------------------------------------------------------------

/// Rival raider states (the NPC plans of `tribal_plans`, T9): the workers
/// gather at their camp fire (`Assemble`); a food thief congregates outside
/// the village, walks to the mat (`Approach`), takes a bundle (`Pillage`) and
/// goes home (`Retreat`); a raid party strikes the player's tool racks and hut
/// (`Raze`) or members (`Fight`). `Done` = home again, released by the planner.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RaidPhase {
    #[default]
    Approach,
    Pillage,
    Raze,
    Retreat,
    Assemble,
    Congregate,
    Fight,
    Done,
}

/// A rival villager working a raid or steal-food plan (`tribal_plans`).
#[derive(Component, Debug, Clone)]
pub struct Raider {
    pub hp: f32,
    pub phase: RaidPhase,
    pub timer: f32,
    /// Which plan sent it.
    pub kind: crate::tribal_plans::RaidKind,
    /// What a raid party member strikes.
    pub target: crate::tribal_plans::RaidTarget,
    /// The tool it grabbed for the raid (`_membersToolsProb`).
    pub tool: Option<ToolKind>,
}

impl Default for Raider {
    fn default() -> Self {
        Self {
            hp: crate::tribal_rules::npc_citizen_max_hp(),
            phase: RaidPhase::Approach,
            timer: 0.0,
            kind: crate::tribal_plans::RaidKind::StealFood,
            target: crate::tribal_plans::RaidTarget::None,
            tool: None,
        }
    }
}

/// Pure seek step on the XZ plane toward `target` (returns new position).
pub fn raider_seek(pos: Vec3, target: Vec3, speed: f32, dt: f32) -> Vec3 {
    crate::tribal_rts::step_toward(pos, target, speed, dt).0
}

/// Kept for the stage's resource list: the NPC raids, once a 45 s timer, are
/// the planners of `tribal_plans` (T9).
#[derive(Resource, Default)]
pub struct TribalTimer;

/// Marker on a destroyed hut / shack left where a rival piece burned down.
#[derive(Component, Debug, Clone, Copy)]
pub struct RivalRubble(pub usize);

/// Marker on a conquered village's burned-out main hut.
#[derive(Component, Debug, Clone, Copy)]
pub struct VillageRuin;

/// Distinct readable tint per village, shifted by diplomacy so conquest and
/// alliance are visible on the ground and on the world map.
pub fn village_tint(status: RivalStatus, index: usize) -> Color {
    let (r, g, b) = match index % RIVAL_VILLAGES {
        0 => (0.10, 0.55, 0.50),
        1 => (0.60, 0.50, 0.12),
        2 => (0.55, 0.20, 0.55),
        3 => (0.18, 0.32, 0.68),
        _ => (0.65, 0.32, 0.12),
    };
    match status {
        RivalStatus::Hostile => Color::srgb(r, g, b),
        RivalStatus::Allied => Color::srgb(r * 0.5 + 0.15, g + 0.25, b * 0.5 + 0.15),
        RivalStatus::Conquered => Color::srgb(0.30, 0.30, 0.34),
    }
}

/// Hut placement inside a village (slot 0..`HUTS_PER_VILLAGE`), on the side
/// facing home so the pieces read from the default camera.
pub fn hut_offset(index: usize, slot: u32) -> Vec3 {
    let toward_home = (-village_position(index)).normalize_or(Vec3::Z);
    let base = toward_home.z.atan2(toward_home.x);
    let angle =
        base + (slot as f32 - 1.0) * 1.15 + if slot == 1 { std::f32::consts::PI } else { 0.0 };
    Vec3::new(angle.cos() * 4.0, 0.0, angle.sin() * 4.0)
}

/// Pure: the original model of hut piece `slot` of village `index`
/// (`(group, name)`): the camp fire, a weapons shack and a music shack.
pub fn hut_piece(index: usize, slot: u32) -> (u32, &'static str) {
    match slot {
        // Retail NPC tribes have one hut (the village root, drawn by
        // `tribal_village` at the village centre) and a camp fire (tool type 10,
        // `tt_firepit_home_0<level>`, 0xC91BD0). ADAPTED: the piece that carries
        // the hut's health and falls last is drawn as the fire; the root hut
        // shows its damage (`tribal_village`). Not changed: the retail frame
        // (MasterTribeLayout: `ToolPositions` out to 21.5 x 20.5 around the
        // fire, `foodMatPosition` 15.83, `totemPolePosition` 13.63) is the
        // player's tribe layout at 30 units radius; OpenSpore's three pieces of
        // an NPC village stand 4 units from the root (rows 48 / 57 of the audit).
        0 => (
            tools::TRIBAL_ART_GROUP,
            crate::tribal_village::firepit_name((village_hut_level(index).clamp(1, 3) - 1) as u8),
        ),
        1 => (
            tools::TRIBAL_ART_GROUP,
            ["tt_hunt_home_01", "tt_hunt_home_02", "tt_hunt_home_03"][index % 3],
        ),
        _ => (
            tools::TRIBAL_ART_GROUP,
            ["tt_Horn_home", "tt_Maraca_home", "tt_Didgeridoo_home"][index % 3],
        ),
    }
}

/// The hut level of NPC tribe `index` (`tribal_rules::tribe_hut_level` of its
/// tier: 1, 2, 2, 2, 3).
pub fn village_hut_level(index: usize) -> usize {
    crate::tribal_rules::tribe_hut_level(village_tier(index))
}

/// The hut model of village `index` at hut `level` (1..=3). VERIFIED
/// (0xC99DC0): every tribe that is not the player's builds from
/// `MasterHutList!npcHutList`, the canvas huts (the yurt / timber / simple
/// lists are the player's by diet, `tribal_village`).
pub fn village_hut_name(_index: usize, level: usize) -> &'static str {
    crate::tribal_village::npc_hut_name((level.clamp(1, 3) - 1) as u8)
}

/// The PROP a village root shows when spawned: the hut of its level, the
/// ruin once conquered.
pub fn village_root_name(index: usize, status: RivalStatus) -> String {
    if status == RivalStatus::Conquered {
        "he_destroyed_hut".into()
    } else {
        village_hut_name(index, village_hut_level(index)).into()
    }
}

/// Spawn one standing village piece, or its rubble once burned.
#[allow(clippy::too_many_arguments)]
fn spawn_hut(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    index: usize,
    slot: u32,
    tint: Color,
    originals: &mut crate::Originals,
    images: &mut Assets<Image>,
    standing: bool,
) {
    let pos = village_position(index) + hut_offset(index, slot);
    let (group, name, width) = if standing {
        let (g, n) = hut_piece(index, slot);
        (g, n, if slot == 0 { 2.2 } else { 3.0 })
    } else if slot == 0 {
        // The burnt-out fire (`TribeTuning.modelPreloads` lists
        // `tt_pile_destroyed` beside the destroyed hut and shed).
        (tools::TRIBAL_ART_GROUP, "tt_pile_destroyed", 2.2)
    } else {
        (tools::TRIBAL_ART_GROUP, "tt_Shed_Destroyed", 2.8)
    };
    if let Some(entity) = tools::spawn_prop(
        commands,
        originals,
        group,
        name,
        meshes,
        images,
        materials,
        Vec3::new(pos.x, -0.6, pos.z),
        width,
    ) {
        if standing {
            commands.entity(entity).insert(RivalHut(index, slot));
        } else {
            commands.entity(entity).insert(RivalRubble(index));
        }
        return;
    }
    let (mesh, color): (Mesh, Color) = if standing {
        (Cone::new(1.1, 1.7).into(), tint)
    } else {
        (
            Cuboid::new(1.6, 0.3, 1.4).into(),
            Color::srgb(0.18, 0.15, 0.12),
        )
    };
    let mut e = commands.spawn((
        StageOwned,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(pos.x, if standing { 0.1 } else { -0.45 }, pos.z),
    ));
    if standing {
        e.insert(RivalHut(index, slot));
    } else {
        e.insert(RivalRubble(index));
    }
}

/// Spawn a village's main hut (or its ruin once conquered).
#[allow(clippy::too_many_arguments)]
fn spawn_village_root(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    index: usize,
    status: RivalStatus,
) {
    let pos = village_position(index);
    let tint = village_tint(status, index);
    let ruined = status == RivalStatus::Conquered;
    let name = village_root_name(index, status);
    if respawn_village_root(
        commands,
        originals,
        meshes,
        images,
        materials,
        index,
        status,
        &name,
        Vec3::new(pos.x, -0.6, pos.z),
    )
    .is_some()
    {
        return;
    }
    let mut e = commands.spawn((
        StageOwned,
        RivalVillage(index),
        Name::new(format!("rival-village-{}", index + 1)),
        Mesh3d(meshes.add(Cone::new(2.0, if ruined { 0.8 } else { 2.6 }))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: tint,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(pos.x, if ruined { -0.2 } else { 0.4 }, pos.z),
    ));
    if ruined {
        e.insert(VillageRuin);
    }
}

/// Spawn the original hut PROP `name` as village `index`'s root (the
/// `RivalVillage` marker the world map and the stage key off). `None` without
/// the installation or when the PROP does not import.
#[allow(clippy::too_many_arguments)]
pub fn respawn_village_root(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    index: usize,
    status: RivalStatus,
    name: &str,
    at: Vec3,
) -> Option<Entity> {
    let ruined = status == RivalStatus::Conquered;
    let entity = tools::spawn_prop(
        commands,
        originals,
        crate::world_objects::HUT_GROUP,
        name,
        meshes,
        images,
        materials,
        at,
        if ruined { 3.6 } else { 3.8 },
    )?;
    commands.entity(entity).insert((
        RivalVillage(index),
        Name::new(format!("rival-village-{}", index + 1)),
    ));
    if ruined {
        commands.entity(entity).insert(VillageRuin);
    }
    Some(entity)
}

/// Spawn the scenery of village `index`: its root carrying the `RivalVillage`
/// marker, its pieces (burned ones stay as rubble) and its food mat.
#[allow(clippy::too_many_arguments)]
pub fn spawn_village_scene(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    index: usize,
    state: &RivalVillageState,
) {
    let tint = village_tint(state.status, index);
    crate::tribal_path::spawn_village_ground(
        commands,
        originals,
        meshes,
        images,
        materials,
        village_position(index),
        crate::tribal_rules::tribe_hut_level(village_tier(index)),
        Some(index),
    );
    spawn_village_root(
        commands,
        originals,
        meshes,
        images,
        materials,
        index,
        state.status,
    );
    for slot in 0..HUTS_PER_VILLAGE {
        spawn_hut(
            commands,
            meshes,
            materials,
            index,
            slot,
            tint,
            originals,
            images,
            slot < state.huts,
        );
    }
    // VERIFIED: only the player's tribe has a totem pole (0xC992D0 is called
    // by the player-tribe creation 0xC99F50 alone), and the food mat of a
    // tribe without one is `toolgather1!tt_food_mat` (0xE8F530).
    let food = village_food_mat(index);
    let (group, name) = crate::tribal_village::food_mat(None);
    tools::spawn_prop(
        commands,
        originals,
        group,
        name,
        meshes,
        images,
        materials,
        Vec3::new(food.x, -0.6, food.z),
        2.2,
    );
}

/// Resolve the shared player-build body for every tribal member. On a cold
/// asset cache the stage setup prepares `baked_body` first; later recruits
/// read that same cache, so their body mesh, painted material and source stay
/// identical to the members present on entry.
fn tribal_body_appearance(
    model: Option<&crate::OriginalModel>,
    profile: &crate::body::BodyProfile,
    role: u32,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> (
    Handle<Mesh>,
    Handle<StandardMaterial>,
    Option<crate::dbpf::ResourceKey>,
) {
    if let Some(model) = model {
        let material = model.material.clone().unwrap_or_else(|| {
            materials.add(StandardMaterial {
                base_color: role_color(role),
                ..default()
            })
        });
        return (model.draw_mesh(), material, model.source_definition);
    }
    (
        meshes.add(crate::body::mesh(profile)),
        materials.add(StandardMaterial {
            base_color: role_color(role),
            ..default()
        }),
        None,
    )
}

/// Members walk a little slower than the chieftain.
pub fn member_speed(stats: &CreatureStats) -> f32 {
    (7.5 * (stats.speed / 8.0) * 0.8).max(4.5)
}

/// Hit points of a tribe member whose species' best health part has level
/// `health_level`: `tribal_rules::citizen_max_hp` (VERIFIED 0xC22880: 100 +
/// the parts bonus; was OpenSpore's 80 x size); the chieftain has 1.5x this.
pub fn member_health(health_level: u8) -> f32 {
    crate::tribal_rules::citizen_max_hp(health_level)
}

/// The player species' health level (the best health part), for
/// [`member_health`].
pub fn tribe_health_level(run: &RunProgress, originals: &crate::Originals) -> u8 {
    crate::creature_actions::player_abilities(run, originals).health
}

/// Where rival village `index` keeps its food mat (the raid target): in front of
/// its hut, toward the player's village.
pub fn village_food_mat(index: usize) -> Vec3 {
    let centre = village_position(index);
    let toward_home = (-centre).normalize_or(Vec3::Z);
    let side = Vec3::new(-toward_home.z, 0.0, toward_home.x);
    centre + toward_home * 3.0 - side * 1.6
}

/// Spawn an initial or recruited member through one path so a cold reload
/// cannot give recruits a procedural body or a role tint when the rest of the
/// tribe uses the original painted creature body.
#[allow(clippy::too_many_arguments)]
fn spawn_tribe_mate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    originals: &crate::Originals,
    run: &RunProgress,
    world_id: u32,
    role: u32,
    member_index: usize,
    position: Vec3,
    size: f32,
    energy: f32,
) -> Entity {
    let model = originals.baked_body.as_ref().map(|body| &body.model);
    let (mesh, material, source) =
        tribal_body_appearance(model, &run.body, role, meshes, materials);
    let root_scale = if source.is_some() {
        size
    } else {
        size.max(0.9)
    };
    let speed = member_speed(&run.creature_stats());
    let mate = commands
        .spawn((
            StageOwned,
            TribeMate,
            crate::persistence::WorldId(world_id),
            Health::new(member_health(tribe_health_level(run, originals))),
            TribeRole(role),
            {
                let mut unit = crate::tribal_rts::TribeUnit::new(false, speed);
                unit.energy = energy.clamp(0.0, MEMBER_ENERGY_MAX);
                unit
            },
            crate::tribal_rts::UnitTask::Idle(position),
            tools::HeldToolState::default(),
            Name::new(format!(
                "mate-{} ({})",
                world_id - MATE_ID_BASE,
                role_name(role)
            )),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position).with_scale(Vec3::splat(root_scale)),
        ))
        .id();
    if let Some(source) = source {
        commands.entity(mate).insert((
            crate::baked_model::NativeCreature,
            crate::baked_model::source(source),
        ));
    }
    crate::creature::mount_saved_parts(
        commands,
        materials,
        mate,
        originals,
        run,
        member_index,
        root_scale,
    );
    mate
}

/// Where member `i` waits at home when the stage begins.
pub fn home_spot(i: usize) -> Vec3 {
    let o = crate::tribal_rts::formation_offset(i + 1);
    home_position() + Vec3::new(1.5 + o.x, 0.45, 7.0 + o.y)
}

#[allow(clippy::too_many_arguments)]
pub fn setup_tribal(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut run: ResMut<RunProgress>,
    mut outfits: ResMut<TribalOutfitSync>,
    mut originals: ResMut<crate::Originals>,
    mut images: ResMut<Assets<Image>>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
) {
    run.enter_stage(GameStage::Tribal);
    // The stage plays on the homeworld: lay the tribes out on it before
    // anything is spawned (`terrain::prepare` reuses the cache and the
    // levelled planet).
    let seed = run.species_world.seed;
    let generated = crate::creature_planet::ensure_site_cache(&originals.resource_index, cache.as_deref(), seed);
    let layout = crate::tribal_planet::prepare(generated.as_ref().or(cache.as_deref()), seed);
    commands.insert_resource(crate::tribal_planet::StageLayout(layout.clone()));
    let _layout = crate::tribal_planet::scope_arc(Some(layout.clone()));
    if let Some(generated) = generated {
        commands.insert_resource(generated);
    }
    clear.0 = GameStage::Tribal.accent();
    // Leaving and re-entering respawns every mate under the same stable
    // `WorldId`s, so remembered mounts would make the first sync skip members
    // that in fact have no outfit child any more. Start the bookkeeping from
    // empty; the sync below rebuilds it against the fresh entities.
    outfits.clear();
    // A save from before this stage existed starts with no villages; fill
    // them in before anything reads the campaign. The roster is rebuilt to
    // one stable slot per assignment before the first member spawns, so
    // ids stay inside the range persistence can restore.
    let run = &mut *run;
    // A new campaign picks its five-tribe lineup from the world seed and only
    // shows the first tribe; a campaign in a save is left as it was.
    let difficulty = crate::creature_rules::Difficulty::from_index(run.cell.difficulty);
    let world_seed = run.species_world.seed;
    if run.tribal_campaign.villages.is_empty() {
        // The lineup is chosen by the player's diet and the Creature card
        // (0xCC6160, `tribal_rules::lineup_for_player`), not by the seed.
        let diet = match crate::tribal_village::player_hut_diet(run, &originals) {
            crate::tribal_village::HutDiet::Omnivore => crate::tribal_rules::LineupDiet::Omnivore,
            crate::tribal_village::HutDiet::Herbivore => crate::tribal_rules::LineupDiet::Herbivore,
            crate::tribal_village::HutDiet::Carnivore => crate::tribal_rules::LineupDiet::Carnivore,
        };
        let creature = run.tribal_campaign.creature_power.unwrap_or_else(|| {
            resolve_consequence_powers(
                run.diet_carnivore,
                run.diet_herbivore,
                run.species_world
                    .species
                    .iter()
                    .filter(|s| run.species_world.species_allied(s.id))
                    .count(),
                run.species_world
                    .species
                    .iter()
                    .filter(|s| run.species_world.species_extinct(s.id))
                    .count(),
            )
            .1
        });
        let style = match creature {
            TribalPowerKind::FireBombs => Some(crate::tribal_rules::LineupStyle::Attack),
            TribalPowerKind::Fireworks => Some(crate::tribal_rules::LineupStyle::Social),
            TribalPowerKind::Beastmaster => Some(crate::tribal_rules::LineupStyle::Mix),
            _ => None,
        };
        let lineup = crate::tribal_rules::lineup_for_player(diet, style);
        run.tribal_campaign.ensure_ready_lineup(lineup, difficulty);
    }
    run.tribal_campaign
        .ensure_ready_seeded(world_seed, difficulty);
    run.tribal_campaign.sync_roster(&mut run.tribe);
    // The creature you built still leads: its stats scale the chieftain,
    // and the whole tribe wears its body + parts (same assembler as the
    // creature stage mates).
    let stats = run.creature_stats();
    commands.insert_resource(CreatureStatsRes(stats));
    let original_body = crate::baked_model::prepare_body(
        &mut originals,
        &run.body,
        &mut meshes,
        &mut images,
        &mut materials,
    )
    .ok();
    // Same avatar geometry as the creature stage (body ~1.5 units tall).
    let body_scale = stats.size;
    let mesh_scale = if original_body.is_some() {
        body_scale
    } else {
        stats.size.max(0.9)
    };
    commands.spawn((
        StageOwned,
        Mesh3d(meshes.add(Cuboid::new(56.0, 1.0, 56.0))),
        crate::terrain::WorldGround,
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.24, 0.12),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -1.1, 0.0),
    ));
    let resolved = run.tribal_campaign.resolved_count();
    let cap = run.tribal_campaign.population_cap();
    // tribal-rules3: the village ground decal of the tribe's level and the
    // roads to the nearest tribes (tribal_path).
    crate::tribal_path::spawn_village_ground(
        &mut commands,
        &originals,
        &mut meshes,
        &mut images,
        &mut materials,
        home_position(),
        run.tribal_campaign.home_hut_level(),
        None,
    );
    crate::tribal_path::spawn_roads(&mut commands, &mut meshes, &mut materials, &layout);
    // The home village: hut, food mat with its pile, totem pole, egg pen and
    // every tool shack already built. The hut is the player's diet style at
    // the tribe level, the food mat matches the totem (`tribal_village`).
    let look = crate::tribal_village::HomeLook::of(run, &originals);
    for kind in [
        crate::tribal_rts::HomePropKind::Hut,
        crate::tribal_rts::HomePropKind::Totem,
        crate::tribal_rts::HomePropKind::EggPen,
        crate::tribal_rts::HomePropKind::FoodMat,
        crate::tribal_rts::HomePropKind::FoodPile,
    ] {
        crate::tribal_rts::spawn_home_prop(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            kind,
            resolved,
            run.dna_this_stage,
            run.tribe.len(),
            cap,
            &look,
        );
    }
    let level = crate::tribal_rules::tribe_level(resolved);
    for (slot, kind) in run.tribal_campaign.shacks.clone().into_iter().enumerate() {
        tools::spawn_shack(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            kind,
            slot,
            level,
        );
    }
    // The rival villages in view, each a visible root carrying the
    // `RivalVillage` marker the world map and this stage both key off, plus its
    // pieces, a totem and a food mat. Tribes that have not appeared yet get
    // their scenery when they do (`tribal_tribes_appear`).
    for index in 0..RIVAL_VILLAGES {
        let Some(state) = run.tribal_campaign.village(index).cloned() else {
            continue;
        };
        spawn_village_scene(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            index,
            &state,
        );
    }
    // Chieftain (player): the original creature body + the saved parts,
    // carrying the chieftain's staff.
    let chief_mesh = original_body
        .as_ref()
        .map(|m| m.draw_mesh())
        .unwrap_or_else(|| meshes.add(Capsule3d::new(0.6, 1.3)));
    let chief_speed = 7.5 * (stats.speed / 8.0);
    // The chieftain is 1.5x a member's hit points and 1.25x his size
    // (`chieftainHealthScale`, `chieftainScale`).
    let chief_scale = crate::tribal_rules::chieftain_body_scale(mesh_scale);
    let chief = commands
        .spawn((
            StageOwned,
            Player,
            Mover { speed: chief_speed },
            Health::new(crate::tribal_rules::chieftain_health(member_health(
                tribe_health_level(&run, &originals),
            ))),
            Hunger::new(150.0),
            crate::tribal_rts::TribeUnit::new(true, chief_speed),
            crate::tribal_rts::UnitTask::Idle(Vec3::new(0.0, 0.7, 7.0)),
            tools::HeldToolState::default(),
            Mesh3d(chief_mesh),
            MeshMaterial3d(
                original_body
                    .as_ref()
                    .and_then(|m| m.material.clone())
                    .unwrap_or_else(|| {
                        materials.add(StandardMaterial {
                            base_color: Color::srgb(1.0, 0.8, 0.4),
                            emissive: Color::srgb(0.3, 0.2, 0.05).into(),
                            ..default()
                        })
                    }),
            ),
            Transform::from_xyz(0.0, 0.7, 7.0).with_scale(Vec3::splat(chief_scale)),
            // Explicit so the respawn can hide him in every app (the render
            // plugins add it to meshes, the headless harness does not).
            Visibility::Inherited,
        ))
        .id();
    if let Some(source) = original_body.as_ref().and_then(|m| m.source_definition) {
        commands.entity(chief).insert((
            crate::baked_model::NativeCreature,
            crate::baked_model::source(source),
        ));
    }
    crate::creature::mount_saved_parts(
        &mut commands,
        &mut materials,
        chief,
        &originals,
        &*run,
        0,
        chief_scale, // the chief's transform is scaled; compensate the children
    );
    // Starting mates: your species with per-creature variation, each in the
    // saved role (gatherer/warrior/shaman/idler) from the tribe editor. Each
    // keeps the stable roster slot it enlisted under, so a save/reload (and
    // the checkpoint that restores living members) always matches the same
    // member to the same id, and ids never leave the restorable range.
    let roster: Vec<(u32, MateAssignment)> = run
        .tribe
        .iter()
        .take(MAX_MATES)
        .enumerate()
        .map(|(i, mate)| {
            let slot = run
                .tribal_campaign
                .slots
                .get(i)
                .copied()
                .unwrap_or(i as u32);
            (MATE_ID_BASE + slot, mate.clone())
        })
        .collect();
    for (world_id, mate) in &roster {
        let slot = world_id - MATE_ID_BASE;
        let mate_ent = spawn_tribe_mate(
            &mut commands,
            &mut meshes,
            &mut materials,
            &originals,
            &*run,
            *world_id,
            mate.role,
            slot as usize + 1,
            home_spot(slot as usize),
            stats.size,
            run.tribal_campaign.energy_of(slot),
        );
        // A save can carry an outfit, and the definition cache is cold on
        // entry, so import the exact saved definition here and mount it. This
        // runs once; the running sync sees the mount as already applied and
        // leaves it alone, so the wearer never gets a second mesh.
        let worn = worn_outfit_of(mate);
        if !worn.is_none() {
            match mount_outfit_child(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                mate_ent,
                &mut originals,
                worn,
                slot as usize + 1,
                mesh_scale,
            ) {
                Ok(()) => {
                    outfits.applied.insert(*world_id, (mate_ent, worn));
                }
                // An assignment this build cannot serve stays saved and is
                // reported by the HUD and the running sync instead.
                Err(err) => {
                    info!("mate {world_id:08X} cannot wear its saved outfit: {err}");
                }
            }
        }
    }
    // Fruit bushes in groves around home and wild ones further out. They
    // regrow where they stand, so the stockpile can always be rebuilt.
    let seed = run.species_world.seed;
    for (i, pos) in crate::tribal_rts::bush_positions(seed, 14)
        .into_iter()
        .enumerate()
    {
        crate::tribal_rts::spawn_bush(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            100 + i as u32,
            pos,
        );
    }
    // Scenery only on land, off the hut and the village pads; ADAPTED spread
    // x2 on the planet (as Creature; terrainDistribute flora is not placed yet).
    let layout = crate::tribal_planet::current();
    crate::baked_model::scenery_where(
        &mut commands,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
        run.species_world.seed,
        if layout.on_planet() { 2.0 } else { 1.0 },
        &|p: Vec3| {
            !layout.on_planet()
                || (layout.land.is_land(p.xz())
                    && p.xz().length() > 12.0
                    && layout.villages.iter().all(|v| v.xz().distance(p.xz()) > 10.0))
        },
    );
    let mut icons = tools::ToolIcons::default();
    tools::load_icons(&mut icons, &originals, &mut images);
    crate::tribal_rts::spawn_village_life(
        &mut commands,
        &mut originals,
        &mut meshes,
        &mut images,
        &mut materials,
        run,
        &icons,
    );
    commands.insert_resource(icons);
}

/// A tribe member's role (drives gather/defense multipliers + tint).
#[derive(Component)]
pub struct TribeRole(pub u32);

/// Role colour: gatherers warm, warriors red, shamans pale blue.
pub fn role_color(role: u32) -> Color {
    match role_name(role) {
        "warrior" => Color::srgb(0.85, 0.25, 0.2),
        "shaman" => Color::srgb(0.6, 0.8, 0.95),
        "idler" => Color::srgb(0.5, 0.5, 0.5),
        _ => Color::srgb(0.8, 0.6, 0.4),
    }
}

/// Marker on the outfit child a mate wears, so outfit replacement/removal
/// never touches the mate's inherited Creature parts (those carry
/// `creature::MountedPart`, which this stage never despawns here).
#[derive(Component)]
pub struct TribalOutfit;

/// A worn outfit as a save sees it. `part` keeps its historical meaning (the
/// RW4 model instance, so saves written before outfits still load) and
/// `definition` is the exact TOOL PROP identity, added later to tell apart
/// definitions that share one model. Neither alone is ambiguous: `None` means
/// "legacy record, resolve the model canonically", `Some` means "this exact
/// definition, and no silent substitute if it is unavailable".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WornOutfit {
    pub part: u32,
    pub definition: Option<u32>,
}

impl WornOutfit {
    /// No outfit at all.
    pub fn is_none(self) -> bool {
        self.part == 0 && self.definition.is_none()
    }
}

/// The pair this stage persists on a member, straight out of the save.
pub fn worn_outfit_of(assignment: &MateAssignment) -> WornOutfit {
    WornOutfit {
        part: assignment.part,
        definition: assignment.outfit_definition,
    }
}

/// Last outfit a sync mounted, per mate stable `WorldId`. Both fields are part
/// of the identity, so swapping between definitions that share one model still
/// remounts instead of being skipped as "unchanged". Runtime state only: a
/// reload starts empty and the first sync rebuilds every child.
#[derive(Resource, Default)]
pub struct TribalOutfitSync {
    applied: HashMap<u32, (Entity, WornOutfit)>,
}

impl TribalOutfitSync {
    /// Forget every remembered mount. Called on stage entry: the mate entities
    /// are about to be respawned under the same stable `WorldId`s, so the
    /// previous mounts no longer describe anything that exists.
    pub fn clear(&mut self) {
        self.applied.clear();
    }
}

/// Upper bound on one outfit's largest axis, in world units, after the
/// mate's own scale. Large garments still read as worn rather than as a body
/// engulfing the mate.
pub const OUTFIT_CAP: f32 = 1.1;

/// The eligible TOOL definition matching `worn`, following the save contract:
/// `Some(definition)` resolves that exact PROP and never substitutes another;
/// `None` resolves the model canonically (the first eligible definition that
/// uses it), which is how legacy records and records written before outfits
/// existed are read. `Err` is a reason worth showing the player.
pub fn tribal_outfit_resolve<'a>(
    originals: &'a crate::Originals,
    worn: WornOutfit,
) -> Result<Option<&'a asset_catalog::PartDefinition>, String> {
    if worn.is_none() {
        return Ok(None);
    }
    let eligible = mechanical_assets::eligible(originals, asset_catalog::TOOL_PARTS);
    if let Some(definition) = worn.definition {
        // Exact identity: unavailable is an error, never a silent switch.
        return match eligible.iter().find(|d| d.key.instance_id == definition) {
            Some(found) => Ok(Some(*found)),
            None => Err(format!("outfit {definition:08X} is not in this catalog")),
        };
    }
    // Legacy record: only the model instance survived, so resolve it through
    // the definitions that share it, canonically first.
    eligible
        .iter()
        .find(|d| d.model.instance_id == worn.part)
        .copied()
        .map(Some)
        .ok_or_else(|| format!("outfit model {:08X} is not in this catalog", worn.part))
}

/// The authored name of a worn outfit, or `None` when it cannot be shown.
pub fn tribal_outfit_name(originals: &crate::Originals, worn: WornOutfit) -> Option<String> {
    tribal_outfit_resolve(originals, worn)
        .ok()
        .flatten()
        .map(|d| d.name.clone())
}

/// Spawn the outfit child for `worn` under `body`, or `Err` with a reason
/// worth showing the player. The mesh, authored material and name come from
/// the exact-definition cache (`mechanical_assets::import`), so a definition
/// that shares its RW4 model with another never receives the wrong texture.
/// The child carries `TribalOutfit`, never `creature::MountedPart`, so
/// removing it can never despawn the mate's inherited Creature parts.
#[allow(clippy::too_many_arguments)]
fn mount_outfit_child(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    body: Entity,
    originals: &mut crate::Originals,
    worn: WornOutfit,
    npc_index: usize,
    body_scale: f32,
) -> Result<(), String> {
    // Resolve through the save contract: exact definition when the save has
    // one, canonical model otherwise. Never substitutes when it has one.
    let Some(definition) = tribal_outfit_resolve(originals, worn)?.cloned() else {
        return Ok(());
    };
    let label = worn.definition.unwrap_or(worn.part);
    let imported = mechanical_assets::import(originals, &definition, meshes, images, materials)?;
    let model = imported.model.clone();
    let (jitter, yaw) = crate::creature::npc_variation(npc_index, 0);
    let local = crate::creature::slot_offset(originals, 0, 1.0);
    let (s, c) = yaw.sin_cos();
    let local = Vec3::new(
        local.x * c - local.z * s,
        local.y,
        local.x * s + local.z * c,
    );
    // Bound the authored garment to a worn size using the real mesh bounds,
    // then apply the mate's body scale and the same jitter the old path used.
    let fitted = mechanical_assets::capped_scale(meshes, &model, OUTFIT_CAP);
    let child_scale = (fitted * 0.35 * jitter) / body_scale.max(1e-3);
    let material = model
        .material
        .clone()
        .unwrap_or_else(|| materials.add(StandardMaterial::default()));
    let name = if imported.name.is_empty() {
        format!("outfit {label:08X}")
    } else {
        imported.name.clone()
    };
    commands.entity(body).with_children(|parent| {
        parent.spawn((
            StageOwned,
            TribalOutfit,
            Name::new(name),
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(material),
            Transform::from_translation(local)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(child_scale)),
        ));
    });
    Ok(())
}

/// Tribe editor: `Tab` focus mate, `[`/`]` cycle role, `N` give the focused
/// mate an original outfit, `Backspace` take it back. Persisted; the tribe's
/// roles drive the gather/defense multipliers immediately.
///
/// `N` cycles the focused member through the original TOOL catalog's outfits
/// (see `tribal_outfits_sync` for the remount). The walk visits each unique
/// candidate once, skips definitions whose import fails, and only commits a
/// definition that actually imported, so a broken original can never be
/// written into a save. Outfits are a prototype cosmetic: they cost nothing
/// and carry no unlock gate, and the catalog's decoded `abilities` are empty
/// for this group, so no gameplay effect is invented for them here.
pub fn tribal_editor(
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut outfits: ResMut<TribalOutfitSync>,
    mut notices: Local<Vec<String>>,
    mut commands: Commands,
    mates: Query<(Entity, &crate::persistence::WorldId), With<TribeMate>>,
    outfit_children: Query<(Entity, &ChildOf), With<TribalOutfit>>,
) {
    notices.clear();
    // The roster is authoritative: `run.tribe` is kept dense and parallel to
    // the campaign's slots, so its real length is the only member count that
    // exists. Never invent members by resizing it - a phantom entry would
    // desync the roster from the slots and charge roles/parts onto entities
    // that were never spawned.
    let count = run.tribe.len();
    if count == 0 {
        // No one to focus with an empty tribe; nothing may be edited.
        return;
    }
    // Clamp the persisted focus onto the living roster so a stale pointer
    // from a bigger save can never index out of range.
    let focus = (run.focused_part as usize).min(count - 1);
    run.focused_part = focus as u32;
    let mut changed = false;
    if keys.just_pressed(KeyCode::Tab) {
        run.focused_part = ((focus + 1) % count) as u32;
        changed = true;
    }
    // tribal_controls: `,` and `.` rotate the camera; roles are `[` and `]`.
    if keys.just_pressed(crate::tribal_controls::KEY_ROLE_PREV)
        || keys.just_pressed(crate::tribal_controls::KEY_ROLE_NEXT)
    {
        let dir = if keys.just_pressed(crate::tribal_controls::KEY_ROLE_NEXT) {
            1
        } else {
            -1
        };
        let cur = run.tribe[focus].role as i32;
        run.tribe[focus].role = ((cur + dir).rem_euclid(TRIBE_ROLES.len() as i32)) as u32;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        // Owned clones, so no borrow of `originals` survives into the import
        // calls below (each import needs `&mut Originals`).
        let candidates: Vec<asset_catalog::PartDefinition> =
            mechanical_assets::eligible(&originals, asset_catalog::TOOL_PARTS)
                .into_iter()
                .cloned()
                .collect();
        if candidates.is_empty() {
            notices.push("No original outfits are indexed in this install".into());
        } else {
            // Walk each distinct definition identity once, starting after the
            // one this member already wears, and stop at the first that
            // imports. A failed import is skipped, never committed, and never
            // retried within this one press.
            let current = worn_outfit_of(&run.tribe[focus]);
            let start = candidates
                .iter()
                .position(|d| Some(d.key.instance_id) == current.definition)
                .map(|p| p + 1)
                .unwrap_or(0);
            let mut picked = None;
            for offset in 0..candidates.len() {
                let definition = &candidates[(start + offset) % candidates.len()];
                let definition_instance = definition.key.instance_id;
                if Some(definition_instance) == current.definition {
                    // Every distinct definition was already tried.
                    break;
                }
                match mechanical_assets::import(
                    &mut originals,
                    definition,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                ) {
                    Ok(imported) => {
                        // `part` keeps its model-id meaning for old saves;
                        // `outfit_definition` pins the exact PROP so two
                        // definitions sharing a model stay distinguishable.
                        picked = Some((
                            WornOutfit {
                                part: imported.model.instance,
                                definition: Some(definition_instance),
                            },
                            imported.name,
                        ));
                        break;
                    }
                    Err(err) => {
                        info!("skipping outfit {definition_instance:08X}: {err}");
                    }
                }
            }
            match picked {
                Some((worn, name)) => {
                    run.tribe[focus].part = worn.part;
                    run.tribe[focus].outfit_definition = worn.definition;
                    changed = true;
                    notices.push(format!("Outfit: {name}"));
                }
                None => notices.push("No original outfit could be loaded from this install".into()),
            }
        }
    }
    if keys.just_pressed(KeyCode::Backspace) {
        if worn_outfit_of(&run.tribe[focus]).is_none() {
            notices.push("That member wears no outfit".into());
        } else {
            run.tribe[focus].part = 0;
            run.tribe[focus].outfit_definition = None;
            changed = true;
            notices.push("Outfit removed".into());
        }
    }
    if let Some(notice) = notices.last() {
        run.tribal_campaign.message = notice.clone();
        run.tribal_campaign.message_timer = 5.0;
    }
    if !changed {
        return;
    }
    run.request_save();
    let roles: Vec<u32> = run.tribe.iter().map(|m| m.role).collect();
    info!(
        "mate {} role {} (gather x{:.2}, defense x{:.2})",
        focus,
        role_name(run.tribe[focus].role),
        gather_multiplier(&roles),
        defense_multiplier(&roles)
    );
    // Apply the new assignment to the living member right away; the shared
    // system keeps doing this after `tribal_campaign_update` as well.
    sync_outfits(
        &mut commands,
        &mut run,
        &mut originals,
        &mut outfits,
        &mut meshes,
        &mut images,
        &mut materials,
        &mates,
        &outfit_children,
        &mut notices,
    );
    if let Some(notice) = notices.last() {
        run.tribal_campaign.message = notice.clone();
        run.tribal_campaign.message_timer = 5.0;
    }
}

/// Keep every living mate's outfit child in step with the saved assignment.
///
/// Public so it can be scheduled right after `tribal_campaign_update` (which
/// claims and releases roster slots), so both systems share one pass over the
/// roster. A member is resolved through its stable `WorldId` -> campaign
/// roster index, never through query enumeration order, so death and
/// recruitment keep their existing slot semantics. Only a change to the
/// assignment (either field) or to what the cache can serve costs a remount.
pub fn tribal_outfits_sync(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut outfits: ResMut<TribalOutfitSync>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mates: Query<(Entity, &crate::persistence::WorldId), With<TribeMate>>,
    outfit_children: Query<(Entity, &ChildOf), With<TribalOutfit>>,
    mut notices: Local<Vec<String>>,
) {
    notices.clear();
    sync_outfits(
        &mut commands,
        &mut run,
        &mut originals,
        &mut outfits,
        &mut meshes,
        &mut images,
        &mut materials,
        &mates,
        &outfit_children,
        &mut notices,
    );
    if let Some(notice) = notices.last() {
        run.tribal_campaign.message = notice.clone();
        run.tribal_campaign.message_timer = 5.0;
    }
}

/// Shared body of `tribal_editor` and `tribal_outfits_sync`.
#[allow(clippy::too_many_arguments)]
fn sync_outfits(
    commands: &mut Commands,
    run: &mut RunProgress,
    originals: &mut crate::Originals,
    outfits: &mut TribalOutfitSync,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    mates: &Query<(Entity, &crate::persistence::WorldId), With<TribeMate>>,
    outfit_children: &Query<(Entity, &ChildOf), With<TribalOutfit>>,
    notices: &mut Vec<String>,
) {
    let body_scale = run.creature_stats().size;
    outfits
        .applied
        .retain(|_, (entity, _)| mates.get(*entity).is_ok());
    for (entity, world_id) in mates.iter() {
        // A dead or recycled slot has no roster entry, so its entity keeps
        // whatever it wears until it is despawned with the roster.
        let Some(index) = run.tribal_campaign.roster_index(world_id.0) else {
            continue;
        };
        let Some(assignment) = run.tribe.get(index) else {
            continue;
        };
        let wanted = worn_outfit_of(assignment);
        // Both fields are the identity, so swapping between definitions that
        // share one model still remounts.
        if outfits.applied.get(&world_id.0).copied() == Some((entity, wanted)) {
            continue;
        }
        // Remove the previous outfit child only; the mate's inherited
        // Creature parts live on the mate entity and are never touched here.
        for (child, child_of) in outfit_children.iter() {
            if child_of.parent() == entity {
                commands.entity(child).despawn();
            }
        }
        outfits.applied.insert(world_id.0, (entity, wanted));
        if wanted.is_none() {
            continue;
        }
        // An assignment this build cannot serve stays saved and stays visible.
        if let Err(err) = mount_outfit_child(
            commands,
            meshes,
            materials,
            images,
            entity,
            originals,
            wanted,
            world_id.0 as usize,
            body_scale,
        ) {
            let label = wanted.definition.unwrap_or(wanted.part);
            info!("mate {} cannot wear outfit {label:08X}: {err}", world_id.0);
            notices.push(format!("Outfit {label:08X} is unavailable in this install"));
        }
    }
}

/// Player-facing Tribal editor line: the focused member, the authored name of
/// the outfit it wears (or why it cannot be shown), the controls, and the
/// outcome of the last edit.
pub fn tribal_editor_hud(run: &RunProgress, originals: &crate::Originals) -> String {
    let count = run.tribe.len();
    if count == 0 {
        return "Tribe editor: no members yet".to_string();
    }
    let focus = (run.focused_part as usize).min(count - 1);
    let world_id = run
        .tribal_campaign
        .slots
        .get(focus)
        .map(|slot| MATE_ID_BASE + *slot);
    let worn = worn_outfit_of(&run.tribe[focus]);
    let outfit = if worn.is_none() {
        "none".to_string()
    } else {
        // An exact definition this build cannot serve stays visible as
        // unsupported, and a legacy record whose model no longer resolves
        // shows the same way rather than silently reading as "no outfit".
        match tribal_outfit_name(originals, worn) {
            Some(name) => name,
            None => {
                let label = worn.definition.unwrap_or(worn.part);
                format!("unsupported outfit {label:08X}")
            }
        }
    };
    format!(
        "Tribe editor - member {}/{} ({}) [{}]\nOutfit: {}\nTab: next member   [ ]: role   N: outfit   Backspace: remove",
        focus + 1,
        count,
        role_name(run.tribe[focus].role),
        world_id.map(|id| format!("{id:08X}")).unwrap_or_else(|| "-".into()),
        outfit,
    )
}

/// Dead members leave the roster; stripped bushes regrow where they stand.
pub fn tribal_cleanup(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut mates: Query<
        (
            Entity,
            &Health,
            &crate::persistence::WorldId,
            &mut TribeRole,
        ),
        With<TribeMate>,
    >,
    mut bushes: Query<&mut Bush, Without<crate::tribal_rts::livestock::EggPile>>,
) {
    let mut lost = false;
    for (entity, health, id, _role) in &mates {
        if health.dead() {
            commands.entity(entity).despawn();
            // Free the stable roster slot and the assignment sitting next to
            // it, so the next recruit reuses the id instead of growing the
            // roster past `MAX_MATES` and out of the restorable range.
            if let Some(index) = run.tribal_campaign.release_slot(id.0) {
                if index < run.tribe.len() {
                    run.tribe.swap_remove(index);
                    lost = true;
                }
            }
        }
    }
    if lost {
        // The assignment vec is dense again; survivors re-read the role at
        // their recycled index so the editor and the world stay in step.
        for (_entity, _health, id, mut role) in &mut mates {
            if let Some(index) = run.tribal_campaign.roster_index(id.0) {
                if let Some(assignment) = run.tribe.get(index) {
                    role.0 = assignment.role;
                }
            }
        }
        run.tribal_campaign.say("A tribe member has fallen.");
        run.request_save();
    }
    // Fruit grows back on the bush, so a spent stockpile can always be
    // rebuilt - there is no way to soft-lock the stage.
    let regrow = BUSH_FOOD / BUSH_REGROW_SECONDS * time.delta_secs();
    for mut b in &mut bushes {
        if b.food_left < BUSH_FOOD {
            b.food_left = (b.food_left + regrow).min(BUSH_FOOD);
        }
    }
}

/// Colour a village root once, so conquest and alliance are visible on the
/// ground and on the world map (which reads the root's material). The read
/// comes first so an unchanged tint never marks the asset modified.
fn retint_village(
    materials: &mut ResMut<Assets<StandardMaterial>>,
    material: &MeshMaterial3d<StandardMaterial>,
    status: RivalStatus,
    index: usize,
) {
    let want = village_tint(status, index);
    if materials
        .get(&material.0)
        .is_some_and(|m| m.base_color == want || m.base_color_texture.is_some())
    {
        return;
    }
    if let Some(mut m) = materials.get_mut(&material.0) {
        m.base_color = want;
    }
}

/// Members carrying each instrument (horn, maracas, didgeridoo) among those
/// standing near `village`.
fn instrument_counts_near(
    village: usize,
    campaign: &TribalCampaign,
    mates: &[(Entity, Vec3, Option<u32>, bool)],
) -> [u32; 3] {
    let centre = village_position(village);
    let mut counts = [0u32; 3];
    for (_, p, id, _) in mates {
        if dist_xz(*p, centre) > INTERACT_RANGE + 4.0 {
            continue;
        }
        if let Some(kind) = tools::instrument_note(id.and_then(|id| campaign.tool_of(id))).and_then(instrument_kind) {
            counts[kind] += 1;
        }
    }
    counts
}

/// The troupe a social command sends: the members it is given to (and the
/// chieftain, who conducts: 0xDA3857) as participants, the instruments they
/// carry, and the village members that watch.
fn troupe_for(
    campaign: &TribalCampaign,
    living: &[(Entity, Vec3, Option<u32>, bool)],
    acting: &[Entity],
    audience: u32,
) -> Troupe {
    let mut troupe = Troupe { participants: 1, instruments: [0; 3], audience };
    for (e, _, id, _) in living {
        if !acting.contains(e) {
            continue;
        }
        troupe.participants += 1;
        if let Some(kind) = tools::instrument_note(id.and_then(|id| campaign.tool_of(id))).and_then(instrument_kind) {
            troupe.instruments[kind] += 1;
        }
    }
    troupe
}

/// The Tribal loop: diplomacy, performance, gifting, conquest, recruitment
/// and recovery, plus the single advance gate for the whole stage.
#[allow(clippy::too_many_arguments)]
pub fn tribal_campaign_update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut next: ResMut<NextState<GameStage>>,
    mut player: Query<
        (
            &Transform,
            &mut Health,
            Option<&mut crate::tribal_rts::UnitTask>,
        ),
        With<Player>,
    >,
    mut mates: Query<
        (
            Entity,
            &Transform,
            &mut crate::tribal_rts::UnitTask,
            &mut crate::tribal_rts::TribeUnit,
            &Health,
            Option<&crate::persistence::WorldId>,
            Option<&crate::tribal_rts::Selected>,
        ),
        (With<TribeMate>, Without<Player>),
    >,
    villages: Query<
        (
            Entity,
            &RivalVillage,
            Option<&MeshMaterial3d<StandardMaterial>>,
            Option<&VillageRuin>,
        ),
        Without<Player>,
    >,
    huts: Query<(Entity, &RivalHut), Without<Player>>,
    mut evolve_said: Local<bool>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
    rivals: Query<&crate::tribal_rts::RivalMember, Without<Raider>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    use crate::tribal_rts::{formation_offset, UnitTask};
    let dt = time.delta_secs();
    let run = &mut *run;
    run.tribal_campaign.ensure_ready();
    // Keeps one stable roster slot per assignment, recycling the ids of
    // members lost in battle so they never outgrow the restorable range.
    run.tribal_campaign.sync_roster(&mut run.tribe);
    let settled_before_tick = run.tribal_campaign.resolved_count();
    run.tribal_campaign.tick(dt);
    if run.tribal_campaign.resolved_count() != settled_before_tick {
        // A song that filled its bar allied a village.
        announce_level_up(&mut run.tribal_campaign, settled_before_tick);
        run.request_save();
    }

    if run.tribal_campaign.cell_power.is_none() || run.tribal_campaign.creature_power.is_none() {
        let (cp, crp) = resolve_consequence_powers(
            run.diet_carnivore,
            run.diet_herbivore,
            run.species_world
                .species
                .iter()
                .filter(|s| run.species_world.species_allied(s.id))
                .count(),
            run.species_world
                .species
                .iter()
                .filter(|s| run.species_world.species_extinct(s.id))
                .count(),
        );
        if run.tribal_campaign.cell_power.is_none() {
            run.tribal_campaign.cell_power = Some(cp);
        }
        if run.tribal_campaign.creature_power.is_none() {
            run.tribal_campaign.creature_power = Some(crp);
        }
    }

    let Ok((pt, mut health, mut chief_task)) = player.single_mut() else {
        return;
    };
    let ready_births = run.tribal_campaign.tick_births(dt);
    if run.tribal_campaign.births.is_empty() {
        run.tribal_campaign.birth_save_timer = 0.0;
    } else {
        run.tribal_campaign.birth_save_timer += dt;
        if run.tribal_campaign.birth_save_timer >= 5.0 {
            run.tribal_campaign.birth_save_timer %= 5.0;
            run.request_save();
        }
    }
    let pos = pt.translation;
    if run.tribal_campaign.trim_births_to_capacity(run.tribe.len()) > 0 {
        run.tribal_campaign
            .say("A growing egg was returned; the tribe has no spare bed.");
        run.request_save();
    }
    for slot in ready_births {
        // A bed may have disappeared while the egg was growing.
        if run.tribe.len() + run.tribal_campaign.births.len()
            >= run.tribal_campaign.population_cap()
        {
            run.tribal_campaign
                .say("The egg did not hatch; the village has no spare bed.");
            run.request_save();
            continue;
        }
        let assignment = MateAssignment::default();
        let member_index = run.tribe.len();
        let o = formation_offset(member_index + 1) * 0.5;
        let at = egg_pen_position() + Vec3::new(1.4 + o.x, 0.45, 1.2 + o.y);
        run.tribal_campaign.slots.push(slot);
        run.tribal_campaign
            .member_energy
            .insert(slot, MEMBER_ENERGY_MAX);
        spawn_tribe_mate(
            &mut commands,
            &mut meshes,
            &mut materials,
            &originals,
            &*run,
            MATE_ID_BASE + slot,
            assignment.role,
            member_index + 1,
            at,
            run.creature_stats().size,
            MEMBER_ENERGY_MAX,
        );
        run.tribe.push(assignment);
        run.request_save();
    }
    let roles: Vec<u32> = run.tribe.iter().map(|m| m.role).collect();
    let target = nearest_hostile(pos, &run.tribal_campaign);
    // Snapshot of living members: (entity, position, world id, selected).
    let living: Vec<(Entity, Vec3, Option<u32>, bool)> = mates
        .iter()
        .filter(|m| !m.4.dead())
        .map(|m| (m.0, m.1.translation, m.5.map(|id| id.0), m.6.is_some()))
        .collect();
    // Orders go to the selected members, or to everyone when none is.
    let acting: Vec<Entity> = if living.iter().any(|m| m.3) {
        living.iter().filter(|m| m.3).map(|m| m.0).collect()
    } else {
        living.iter().map(|m| m.0).collect()
    };
    let mut order = |task: &dyn Fn(usize) -> UnitTask, who: &[Entity]| {
        for (i, e) in who.iter().enumerate() {
            if let Ok((_, _, mut t, mut u, ..)) = mates.get_mut(*e) {
                u.offset = formation_offset(i + 1);
                *t = task(i);
            }
        }
    };

    // --- 1..5: answer the called note -----------------------------------
    let answer = [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 2),
        (KeyCode::Digit3, 3),
        (KeyCode::Digit4, 4),
        (KeyCode::Digit5, 5),
    ]
    .iter()
    .find(|(k, _)| keys.just_pressed(*k))
    .map(|(_, a)| *a);
    if let Some(answer) = answer {
        if let Some(p) = run.tribal_campaign.performance {
            // The troupe plays for whoever is standing there; answering from
            // across the valley drops the performance instead of scoring it.
            let troupe_there = living
                .iter()
                .any(|m| dist_xz(m.1, village_position(p.village)) <= INTERACT_RANGE);
            if run.tribal_campaign.answer_in_range(pos) || troupe_there {
                // 3 / 4 / 5 order the horns / maracas / didgeridoos (the social
                // ability buttons); 1 and 2 have no meaning in the game.
                let available = instrument_counts_near(p.village, &run.tribal_campaign, &living);
                let settled_before = run.tribal_campaign.resolved_count();
                if run.tribal_campaign.order_instrument(answer, available) {
                    run.request_save();
                }
                if run.tribal_campaign.resolved_count() != settled_before {
                    announce_level_up(&mut run.tribal_campaign, settled_before);
                }
            } else {
                run.tribal_campaign.abandon_performance(format!(
                    "Too far from V{} to answer - stay within {} units.",
                    p.village + 1,
                    INTERACT_RANGE
                ));
            }
        } else if answer == 1 {
            match run.tribal_campaign.activate_cell_power(pos) {
                Ok(ev) => {
                    if ev.food_delta > 0.0 {
                        run.award_dna(ev.food_delta, &GameStage::Tribal);
                    }
                    run.request_save();
                }
                Err(reason) => run.tribal_campaign.say(reason),
            }
        } else if answer == 2 {
            let bomb_target = fire_bomb_target(pos, &run.tribal_campaign);
            match run.tribal_campaign.activate_creature_power(pos, bomb_target) {
                Ok(ev) => {
                    if ev.reward > 0.0 {
                        run.award_dna(ev.reward, &GameStage::Tribal);
                    }
                    if ev.huts_destroyed > 0 {
                        let tint = village_tint(RivalStatus::Hostile, ev.village);
                        for (entity, hut) in &huts {
                            if hut.0 == ev.village && hut.1 >= ev.huts_left {
                                commands.entity(entity).despawn();
                                spawn_hut(
                                    &mut commands,
                                    &mut meshes,
                                    &mut materials,
                                    ev.village,
                                    hut.1,
                                    tint,
                                    &mut originals,
                                    &mut images,
                                    false,
                                );
                            }
                        }
                    }
                    run.request_save();
                }
                Err(reason) => run.tribal_campaign.say(reason),
            }
        }
    } else if run.tribal_campaign.performance.is_none() {
        if keys.just_pressed(KeyCode::KeyY) {
            match run.tribal_campaign.activate_cell_power(pos) {
                Ok(ev) => {
                    if ev.food_delta > 0.0 {
                        run.award_dna(ev.food_delta, &GameStage::Tribal);
                    }
                    run.request_save();
                }
                Err(reason) => run.tribal_campaign.say(reason),
            }
        } else if keys.just_pressed(KeyCode::KeyU) {
            let bomb_target = fire_bomb_target(pos, &run.tribal_campaign);
            match run.tribal_campaign.activate_creature_power(pos, bomb_target) {
                Ok(ev) => {
                    if ev.reward > 0.0 {
                        run.award_dna(ev.reward, &GameStage::Tribal);
                    }
                    if ev.huts_destroyed > 0 {
                        let tint = village_tint(RivalStatus::Hostile, ev.village);
                        for (entity, hut) in &huts {
                            if hut.0 == ev.village && hut.1 >= ev.huts_left {
                                commands.entity(entity).despawn();
                                spawn_hut(
                                    &mut commands,
                                    &mut meshes,
                                    &mut materials,
                                    ev.village,
                                    hut.1,
                                    tint,
                                    &mut originals,
                                    &mut images,
                                    false,
                                );
                            }
                        }
                    }
                    run.request_save();
                }
                Err(reason) => run.tribal_campaign.say(reason),
            }
        }
    }

    // --- perform for the nearest hostile village ---------------------------
    // tribal_controls: was E (a camera strafe key in the original); the right
    // click on a village in the social stance presses it on arrival.
    if keys.just_pressed(crate::tribal_controls::KEY_PERFORM) {
        if let Some(village) = target {
            let audience = rivals.iter().filter(|m| m.village == village).count() as u32;
            let troupe = troupe_for(&run.tribal_campaign, &living, &acting, audience);
            let started = run.tribal_campaign.start_performance_gathering(village, troupe);
            // The troupe gathers in front of the village either way.
            order(&|_| UnitTask::Perform(village), &acting);
            match started {
                Ok(()) => {
                    // 0xDA22C0: the layout gives every participant its spot; the
                    // leader is the chieftain, the target the village.
                    let centre = village_position(village);
                    let dir = Vec3::new(centre.x - pos.x, 0.0, centre.z - pos.z)
                        .normalize_or(Vec3::new(-centre.x, 0.0, -centre.z).normalize_or(Vec3::X));
                    let mut members = vec![(pos, crate::tribal_rules::TroupeRole::Chieftain)];
                    let mut who: Vec<u64> = Vec::new();
                    for (e, p, id, _) in &living {
                        if !acting.contains(e) {
                            continue;
                        }
                        let group = tools::instrument_note(id.and_then(|id| run.tribal_campaign.tool_of(id)))
                            .and_then(instrument_kind)
                            .unwrap_or(3);
                        members.push((*p, crate::tribal_rules::TroupeRole::Group(group)));
                        who.push(e.to_bits());
                    }
                    let stands = crate::tribal_rules::social_stands(
                        centre,
                        dir,
                        crate::tribal_rules::SOCIAL_CREATURE_SIZE,
                        &members,
                    );
                    run.tribal_campaign.song_stands =
                        who.into_iter().zip(stands.into_iter().skip(1)).collect();
                    run.request_save()
                }
                Err(reason) => {
                    if run.tribal_campaign.performance.is_none() {
                        run.tribal_campaign.say(reason);
                    }
                }
            }
        } else if run.tribal_campaign.cooldown <= 0.0 {
            run.tribal_campaign.say(format!(
                "Walk within {} units of a hostile village first.",
                INTERACT_RANGE
            ));
        }
    }

    // --- G: gift food for relations -------------------------------------
    // ADAPTED: G, F and R (and Y U 1-5) are extras the original's trigger
    // files do not bind; the original gifts and attacks through the commands
    // and the HUD.
    if keys.just_pressed(KeyCode::KeyG) {
        if let Some(village) = target {
            let food = run.dna_this_stage;
            let ev = run.tribal_campaign.gift(village, food);
            if ev.food_delta < 0.0 {
                run.dna_this_stage = (run.dna_this_stage + ev.food_delta).max(0.0);
                run.request_save();
            }
        } else {
            run.tribal_campaign
                .say("Gift a village: stand near one first (G).");
        }
    }

    // --- F: attack ---------------------------------------------------------
    // Pressing F sends the members at the nearest hostile village; holding
    // it near one has the chieftain fight at the walls too.
    let mut chief_attacking: Option<usize> = None;
    if keys.just_pressed(KeyCode::KeyF) {
        if let Some(village) = target {
            order(&|_| UnitTask::Siege(village), &acting);
            run.tribal_campaign
                .say(format!("The tribe attacks V{}!", village + 1));
        }
    }
    if keys.pressed(KeyCode::KeyF) {
        if let Some(village) = target {
            if health.frac() < WOUNDED_FRACTION {
                run.tribal_campaign
                    .say("Too wounded to fight. Recover at home with R first.");
            } else {
                chief_attacking = Some(village);
                // The chieftain leads from the walls, where defenders find him.
                if let Some(task) = chief_task.as_mut() {
                    if **task != UnitTask::Siege(village) {
                        **task = UnitTask::Siege(village);
                    }
                }
            }
        } else if run.tribal_campaign.performance.is_none() && keys.just_pressed(KeyCode::KeyF) {
            run.tribal_campaign
                .say("Nothing to attack - stand near a hostile village.");
        }
    }
    // Siege damage: the chieftain plus every member at the walls, each with
    // the tool it carries.
    for village in 0..RIVAL_VILLAGES {
        if run
            .tribal_campaign
            .village(village)
            .is_none_or(|v| v.status != RivalStatus::Hostile)
        {
            continue;
        }
        let centre = village_position(village);
        let attackers: Vec<Option<ToolKind>> = mates
            .iter()
            .filter(|m| {
                !m.4.dead()
                    && !m.3.fighting
                    && *m.2 == UnitTask::Siege(village)
                    && dist_xz(m.1.translation, centre) <= 4.8
            })
            .map(|m| m.5.and_then(|id| run.tribal_campaign.tool_of(id.0)))
            .collect();
        let chief = chief_attacking == Some(village);
        if attackers.is_empty() && !chief {
            continue;
        }
        let damage = siege_damage(chief, &attackers, &roles) * dt.max(0.0);
        let settled_before = run.tribal_campaign.resolved_count();
        let ev = run.tribal_campaign.attack(village, damage);
        if ev.resolved_now {
            announce_level_up(&mut run.tribal_campaign, settled_before);
        }
        if ev.huts_destroyed > 0 {
            // Burn the highest standing pieces first and leave rubble.
            let tint = village_tint(RivalStatus::Hostile, village);
            for (entity, hut) in &huts {
                if hut.0 == village && hut.1 >= ev.huts_left {
                    commands.entity(entity).despawn();
                    spawn_hut(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        village,
                        hut.1,
                        tint,
                        &mut originals,
                        &mut images,
                        false,
                    );
                }
            }
            // A hut falling is real progress; the siege frames in
            // between are not, so the save only rides the hit.
            run.request_save();
            info!("{}", ev.message);
        }
        if ev.reward > 0.0 {
            run.award_dna(ev.reward, &GameStage::Tribal);
            run.request_save();
        }
    }

    // --- R: spend food near home to patch up or to hatch a member ---------
    if keys.just_pressed(KeyCode::KeyR) {
        if is_home(pos) {
            let members = run.tribe.len().min(MAX_MATES);
            let food = run.dna_this_stage;
            // A fallen chieftain waits for his respawn; R cannot patch him up.
            let missing = if run.tribal_campaign.chieftain_respawn > 0.0 {
                0.0
            } else {
                health.max - health.hp
            };
            let hurt_badly = missing > 0.0 && health.frac() < RECOVER_PRIORITY_FRACTION;
            let cap = run.tribal_campaign.population_cap();
            let used = run.tribal_campaign.member_capacity_used(members);
            // A wounded chieftain is seen to first: recruiting until the
            // stockpile is empty would leave nobody well enough to lead.
            let recoverable = missing > 0.0 && (hurt_badly || food < RECRUIT_FOOD || used >= cap);
            if recoverable {
                if run.tribal_campaign.recover(food, missing) {
                    let amount = missing.min(health.max * RECOVER_FRACTION);
                    health.heal(amount);
                    run.dna_this_stage = (run.dna_this_stage - RECOVER_FOOD).max(0.0);
                    run.request_save();
                }
            } else if used < cap {
                if run.tribal_campaign.recruit(food, members) {
                    run.dna_this_stage = (run.dna_this_stage - RECRUIT_FOOD).max(0.0);
                    run.request_save();
                }
            } else if missing > 0.0 {
                run.tribal_campaign
                    .say("The hut is full; use R to patch yourself up.");
            } else {
                run.tribal_campaign.recruit(food, members);
            }
        } else {
            run.tribal_campaign.say(format!(
                "Come within {} units of home to recruit or recover.",
                HOME_RANGE
            ));
        }
    }

    // Keep the village tints in step with diplomacy, and burn conquered
    // villages' main huts down to a ruin.
    for (entity, village, material, ruin) in &villages {
        let Some(state) = run.tribal_campaign.village(village.0) else {
            continue;
        };
        if let Some(material) = material {
            retint_village(&mut materials, material, state.status, village.0);
        }
        if state.status == RivalStatus::Conquered && ruin.is_none() {
            commands.entity(entity).despawn();
            spawn_village_root(
                &mut commands,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                village.0,
                RivalStatus::Conquered,
            );
        }
    }

    // The single win condition for the whole stage. Once every tribe is
    // united the retail Evolve button (`0645_4B47`, T5C) appears; the stage
    // ends when it is clicked (`crate::tribal_hud::ADVANCE_KEY` is its key).
    if run.tribal_campaign.can_advance() {
        if !*evolve_said {
            *evolve_said = true;
            run.tribal_campaign.say("Every tribe is united: press Evolve (F4) to advance to Civilization.");
        }
    } else {
        *evolve_said = false;
    }
    if run.tribal_campaign.can_advance() && keys.just_pressed(crate::tribal_hud::ADVANCE_KEY) {
        if run.tribal_campaign.cancel_births() > 0 {
            run.tribal_campaign
                .say("The growing egg was canceled as the tribe advanced.");
        }
        // The city hall shows the play-style card the tribe earned
        // (`TRG2CVG_ShowCityHall`, 0xCDD0F0): the warmer the final relationships
        // with the five tribes, the more social; the colder, the more military.
        let card = run.tribal_campaign.play_style_card();
        run.tribal_campaign.city_hall_card = Some(card);
        // The Evolve click also stores the trait on the species and the play
        // style on the player (0xCDBEA0: 0xE39890 with the earned-trait id,
        // cPlayer +0x1104 = the card id).
        run.tribal_campaign.earned_trait = Some(card.trait_id());
        run.tribal_campaign.play_style_id = Some(card.card_id());
        info!(
            "every village is united - the tribe advances to Civilization ({} city hall card)",
            card.card()
        );
        run.request_save();
        run.advance();
        next.set(GameStage::Civilization);
    }
}

/// Add the tribe level-up to the line on screen when the progress crossed an
/// upgrade threshold (`TRG_Upgrade`: the hut and its totem get their next look).
fn announce_level_up(campaign: &mut TribalCampaign, settled_before: usize) {
    if let Some(level) = crate::tribal_rules::level_reached(settled_before, campaign.resolved_count()) {
        let line = format!(
            "The tribe grows: level {level} hut ({:.0} health)!",
            crate::tribal_rules::hut_health(level)
        );
        info!("{line}");
        let joined = if campaign.message.is_empty() {
            line
        } else {
            format!("{} {line}", campaign.message)
        };
        campaign.say(joined);
    }
}

/// Drop unpaid growth progress if the stage is exited through an external
/// transition rather than the normal five-village advance gate.
pub fn tribal_stage_exit(mut run: ResMut<RunProgress>) {
    // Continue/menu/save transitions keep the Tribal stage current and must
    // preserve eggs. Only a permanent transition to another stage cancels.
    let current = run.current_stage();
    let cancelled = cancel_births_on_stage_exit(&mut run.tribal_campaign, current);
    if cancelled > 0 {
        run.tribal_campaign
            .say("Growing eggs were canceled when the tribe left this stage.");
        run.request_save();
    }
}

pub fn cancel_births_on_stage_exit(campaign: &mut TribalCampaign, current: GameStage) -> usize {
    if current == GameStage::Tribal {
        0
    } else {
        campaign.cancel_births()
    }
}

/// New NPC tribes come into view over time by the retail NPC tribe controller
/// (0xCC6A90, `tribal_rules::AppearanceState`): the first one 25 s after the
/// tribe's food first reaches 40 (or 90 s after its first baby; both at once
/// with Tutorials off), the second when a tribe is settled or the stock is above
/// 100, the third and fourth at once, the fifth after three settled tribes. The
/// scenery and members of every tribe in view that has none yet are spawned here.
#[allow(clippy::too_many_arguments)]
pub fn tribal_tribes_appear(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<&RivalVillage>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let run = &mut *run;
    run.tribal_campaign.ensure_ready();
    let difficulty = crate::creature_rules::Difficulty::from_index(run.cell.difficulty);
    let food = run.dna_this_stage;
    // The first baby (the Mating tutorial flag, 0xCD7647): an egg was bought or
    // the tribe is larger than its start.
    let baby = !run.tribal_campaign.births.is_empty()
        || run.tribe.len() > crate::tribal_rules::TRIBE_POPULATION_START;
    let mut revealed: Option<usize> = None;
    {
        let resolved = run.tribal_campaign.resolved_count();
        let campaign = &mut run.tribal_campaign;
        if campaign.appeared_count() < RIVAL_VILLAGES {
            let input = crate::tribal_rules::AppearanceInput {
                npc_tribes: campaign.appeared_count(),
                resolved,
                food,
                baby,
                tutorials: crate::front_end::settings::tutorials_enabled(),
                difficulty,
            };
            let mut state = campaign.appearance;
            let due = state.step(time.delta_secs(), &input);
            campaign.appearance = state;
            if due {
                if let Some(next) = campaign.villages.iter().position(|v| !v.appeared) {
                    campaign.villages[next].appeared = true;
                    campaign.say(format!("A new tribe has appeared: V{}!", next + 1));
                    revealed = Some(next);
                }
            }
        }
    }
    if let Some(next) = revealed {
        info!("a new tribe appears: V{}", next + 1);
        run.request_save();
    }
    for index in 0..RIVAL_VILLAGES {
        let Some(state) = run.tribal_campaign.village(index).cloned() else {
            continue;
        };
        if roots.iter().any(|root| root.0 == index) {
            continue;
        }
        spawn_village_scene(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            index,
            &state,
        );
        if state.status != RivalStatus::Conquered {
            crate::tribal_rts::spawn_village_members(
                &mut commands,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                &run.tribal_campaign,
                index,
            );
        }
    }
}

/// A fallen chieftain waits at the hut, hidden and untargetable, for
/// `chieftainRespawnTime` (90 s) and then returns with full health.
pub fn tribal_chieftain_respawn(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut player: Query<
        (
            &mut Health,
            &mut Transform,
            Option<&mut Visibility>,
            &mut crate::tribal_rts::TribeUnit,
            &mut crate::tribal_rts::UnitTask,
        ),
        With<Player>,
    >,
) {
    let remaining = run.tribal_campaign.chieftain_respawn;
    if remaining <= 0.0 {
        return;
    }
    let Ok((mut health, mut t, mut visibility, mut unit, mut task)) = player.single_mut() else {
        return;
    };
    // Down: out of sight and out of reach (units at zero health are nobody's target).
    if let Some(v) = visibility.as_mut() {
        **v = Visibility::Hidden;
    }
    health.hp = 0.0;
    let (left, back) = crate::tribal_rules::chieftain_respawn_step(remaining, time.delta_secs());
    run.tribal_campaign.chieftain_respawn = left;
    if back {
        health.hp = health.max;
        if let Some(v) = visibility.as_mut() {
            **v = Visibility::Inherited;
        }
        t.translation = home_position() + Vec3::new(2.0, t.translation.y, 6.0);
        unit.carrying = 0.0;
        *task = crate::tribal_rts::UnitTask::Idle(t.translation);
        run.tribal_campaign.say("The chieftain returns to the village.");
        run.request_save();
    }
}

/// Apply the superpower effects the campaign queued: a Refreshing Storm fills
/// the fruit bushes it reaches, Fire Bombs hurt every rival member and wild
/// creature in the blast (each hurt tribe posts a `TribeAttack` event).
#[allow(clippy::type_complexity)]
pub fn tribal_power_effects(
    mut run: ResMut<RunProgress>,
    mut bushes: Query<(&Transform, &mut Bush)>,
    mut rivals: Query<
        (&Transform, &crate::tribal_rts::RivalMember, &mut Health),
        (Without<crate::tribal_rts::WildAnimal>, Without<Bush>),
    >,
    mut animals: Query<
        (&Transform, &mut Health),
        (
            With<crate::tribal_rts::WildAnimal>,
            Without<crate::tribal_rts::RivalMember>,
            Without<Bush>,
        ),
    >,
) {
    if run.tribal_campaign.pending_power_effects.is_empty() {
        return;
    }
    let effects = std::mem::take(&mut run.tribal_campaign.pending_power_effects);
    for effect in effects {
        match effect {
            PowerEffect::Storm { at, radius } => {
                let mut refreshed = 0;
                for (t, mut bush) in &mut bushes {
                    if dist_xz(t.translation, at) <= radius && bush.food_left < BUSH_FOOD {
                        bush.food_left = BUSH_FOOD;
                        refreshed += 1;
                    }
                }
                info!("refreshing storm refills {refreshed} bushes");
            }
            PowerEffect::Blast { at, radius, damage } => {
                for (t, rival, mut h) in &mut rivals {
                    if !h.dead() && dist_xz(t.translation, at) <= radius {
                        h.hurt(damage);
                        run.tribal_campaign.post_hostile_strikes(
                            rival.village,
                            crate::tribal_rules::RelationEvent::Attack,
                            1.0,
                        );
                    }
                }
                for (t, mut h) in &mut animals {
                    if !h.dead() && dist_xz(t.translation, at) <= radius {
                        h.hurt(damage);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::headless_app;
    use crate::tribal_rts::{TribeUnit, UnitTask};
    use bevy::time::TimeUpdateStrategy;

    fn step(app: &mut App) {
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        step(app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    /// Hold a key for `frames` frames (re-pressed every frame because the
    /// headless harness clears input after each update).
    fn hold(app: &mut App, key: KeyCode, frames: usize) {
        for _ in 0..frames {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            step(app);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    fn stage_of(app: &App) -> GameStage {
        app.world().resource::<State<GameStage>>().get().clone()
    }

    fn enter(app: &mut App, stage: GameStage) {
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(stage.clone());
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(stage);
        step(app);
    }

    fn teleport(app: &mut App, position: Vec3) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Transform, With<Player>>();
        players.single_mut(world).expect("one player").translation = position;
    }

    /// Move the chieftain and every member to `position` (a troupe in place).
    fn teleport_tribe(app: &mut App, position: Vec3) {
        let world = app.world_mut();
        let mut units = world.query_filtered::<&mut Transform, With<TribeUnit>>();
        for (i, mut t) in units.iter_mut(world).enumerate() {
            let o = crate::tribal_rts::formation_offset(i);
            t.translation.x = position.x + o.x;
            t.translation.z = position.z + o.y;
        }
    }

    /// `World::query_filtered` needs a mutable world (it builds a
    /// `QueryState`), so this takes `&mut App` like `teleport` does.
    fn player_hp(app: &mut App) -> f32 {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&Health, With<Player>>();
        players.single(world).expect("one player").hp
    }

    fn campaign(app: &App) -> &TribalCampaign {
        &app.world().resource::<RunProgress>().tribal_campaign
    }

    fn prompt_of(app: &App) -> Option<u32> {
        campaign(app).performance.map(|p| p.prompt)
    }

    fn first_hostile(app: &App) -> Option<usize> {
        let c = campaign(app);
        (0..RIVAL_VILLAGES).find(|&i| {
            c.village(i)
                .is_some_and(|v| v.status == RivalStatus::Hostile)
        })
    }

    fn digit(ability: u32) -> KeyCode {
        match ability {
            1 => KeyCode::Digit1,
            2 => KeyCode::Digit2,
            3 => KeyCode::Digit3,
            4 => KeyCode::Digit4,
            _ => KeyCode::Digit5,
        }
    }

    /// Play a started song to its end with four players per instrument: every
    /// bubble is answered with the instrument it asks for, as soon as it comes.
    fn sing(c: &mut TribalCampaign) {
        let mut guard = 0;
        while c.performance.is_some() && guard < 4000 {
            guard += 1;
            if let Some(p) = c.performance {
                // As soon as the wanted instrument has recharged (a bubble that
                // was already evaluated still counts: the retail evaluation reads
                // the last bubble).
                if let Some(kind) = instrument_kind(p.prompt).filter(|_| p.required > 0) {
                    if p.refresh[kind] <= 0.0 {
                        c.order_instrument(p.prompt, [4, 4, 4]);
                    }
                }
            }
            c.tick(0.05);
        }
        assert!(c.performance.is_none(), "the song ends");
    }

    /// Ally `village` the peaceful way: perform full songs, clearing the
    /// cooldown in between.
    fn ally_by_performance(c: &mut TribalCampaign, village: usize) {
        for _ in 0..4 {
            c.cooldown = 0.0;
            if c.village(village).is_some_and(|v| v.status.is_resolved()) {
                return;
            }
            if c.start_performance(village).is_none() {
                return;
            }
            sing(c);
        }
    }

    // --- world layout ----------------------------------------------------

    #[test]
    fn five_villages_sit_distinctly_at_their_retail_rings() {
        let mut seen: Vec<Vec3> = Vec::new();
        for i in 0..RIVAL_VILLAGES {
            let p = village_position(i);
            let (lo, hi) = crate::tribal_rules::village_marker(i).distance_from_origin;
            assert!(
                p.length() >= lo && p.length() <= hi,
                "village {i} at {p:?} leaves its ring {lo}..{hi}"
            );
            assert!(
                seen.iter().all(|q| dist_xz(p, *q) > INTERACT_RANGE * 2.0),
                "village {i} at {p:?} overlaps a neighbour"
            );
            assert!(
                dist_xz(p, home_position()) > INTERACT_RANGE * 2.0,
                "a rival must not sit on the home hut"
            );
            seen.push(p);
        }
        assert_eq!(seen.len(), RIVAL_VILLAGES);
    }

    #[test]
    fn the_flat_layout_fishes_at_the_island_shore() {
        for seed in [0u64, 7, 0xDEAD_BEEF] {
            for i in 0..36 {
                let a = i as f32 * std::f32::consts::TAU / 36.0;
                let dir = Vec2::new(a.cos(), a.sin());
                let coast = coastline_radius(seed, dir);
                assert!((33.0..44.0).contains(&coast), "coast {coast} at {a}");
                // No planet: nothing bounds a goal any more.
                let far = Vec3::new(dir.x * 600.0, 0.0, dir.y * 600.0);
                assert_eq!(crate::tribal_planet::flat_layout(seed).keep_on_land(far), far);
            }
            let spots = crate::tribal_planet::flat_layout(seed).fishing;
            assert_eq!(spots.len(), RIVAL_VILLAGES);
            for s in spots {
                let anchor = Vec2::new(s.x, s.z);
                let visible_ripple = anchor + anchor.normalize_or(Vec2::X) * 1.6;
                let coast = coastline_radius(seed, visible_ripple);
                let ripple_radius = visible_ripple.length();
                assert!(
                    (ripple_radius - coast - 0.4).abs() < 0.02,
                    "visible ripple at {ripple_radius} should track the waterline near {coast}"
                );
            }
        }
    }

    #[test]
    fn bubbles_are_deterministic_and_ask_for_open_instruments() {
        assert_eq!(ability_name(1), "Chant");
        assert_eq!(ability_name(3), "Horn");
        assert_eq!(ability_name(4), "Maracas");
        assert_eq!(ability_name(5), "Didgeridoo");
        assert_eq!(instrument_kind(3), Some(0));
        assert_eq!(instrument_kind(5), Some(2));
        assert_eq!(instrument_kind(2), None);
        // Two songs with the same seed ask for the same things.
        let ask = |resolved: usize, song: u32| {
            let mut c = TribalCampaign::default();
            c.ensure_ready();
            for v in c.villages.iter_mut().take(resolved) {
                v.status = RivalStatus::Allied;
            }
            c.round = song;
            c.start_performance(4).expect("starts");
            let mut asked = Vec::new();
            while let Some(p) = c.performance {
                if p.bubble && p.note < p.notes && asked.len() as u32 == p.note {
                    asked.push((p.prompt, p.required));
                }
                c.tick(0.05);
            }
            asked
        };
        assert_eq!(ask(0, 3), ask(0, 3));
        // Five bubbles a song; with no village settled only the horn is open,
        // with one the maracas, with two the didgeridoo (OpenSpore's tool tiers).
        for song in 0..8 {
            let first = ask(0, song);
            assert_eq!(first.len(), 5, "{first:?}");
            assert!(first.iter().all(|(note, _)| *note == 3), "{first:?}");
            let second = ask(1, song);
            assert!(second.iter().all(|(note, _)| matches!(note, 3 | 4)), "{second:?}");
            let third = ask(2, song);
            assert!(third.iter().all(|(note, required)| (3..=5).contains(note) && *required >= 1), "{third:?}");
        }
    }

    #[test]
    fn nearest_hostile_picks_the_closest_unresolved_village_in_range() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        // Standing on the home hut, no village is in range.
        assert_eq!(nearest_hostile(home_position(), &c), None);
        // Standing on an allied village ignores it and finds nothing else.
        c.villages[0].status = RivalStatus::Allied;
        assert_eq!(nearest_hostile(village_position(0), &c), None);
        // Just inside range of village 3.
        let near = village_position(3) + Vec3::new(INTERACT_RANGE - 0.5, 0.0, 0.0);
        assert_eq!(nearest_hostile(near, &c), Some(3));
    }

    // --- save compatibility ---------------------------------------------

    #[test]
    fn the_first_hatched_baby_ends_the_populate_goal() {
        use crate::tribal_hud::tutorial::{flag, TutorialGoal};
        let mut c = TribalCampaign::default();
        c.tutorial.add(TutorialGoal::Populate);
        c.births.push(TribalBirth { slot: 5, remaining: 2.0 });
        assert!(c.tick_births(1.0).is_empty());
        assert_eq!(c.tutorial.goals(), [TutorialGoal::Populate]);
        assert_eq!(c.tick_births(1.0), [5]);
        assert!(c.tutorial.has(flag::BORN));
        assert!(c.tutorial.goals().is_empty());
    }

    #[test]
    fn a_new_campaign_starts_with_the_camp_fire_built() {
        use crate::creature_rules::Difficulty;
        // `TRG_Begin` binds the fire at the stage start: it stands from the
        // first frame and is not paid for.
        let mut c = TribalCampaign::default();
        c.ensure_ready_seeded(7, Difficulty::Medium);
        assert_eq!(c.shacks, [ToolKind::Campfire]);
        assert!(c.has_shack(ToolKind::Campfire));
        assert!(c.build_shack(ToolKind::Campfire, 100.0).is_err(), "it cannot be bought again");
        // Seeding again (a reload of the stage) keeps one fire.
        c.ensure_ready_seeded(7, Difficulty::Medium);
        assert_eq!(c.shacks, [ToolKind::Campfire]);
        // A campaign already under way is left alone.
        let mut running = TribalCampaign::default();
        running.ensure_ready();
        running.ensure_ready_seeded(7, Difficulty::Medium);
        assert!(running.shacks.is_empty(), "a campaign with its villages already staged is not restarted");
    }

    #[test]
    fn a_gift_effect_fades_one_attitude_step_per_timer() {
        use crate::creature_rules::Difficulty;
        let mut c = TribalCampaign::default();
        c.ensure_ready_seeded(7, Difficulty::Medium);
        c.reveal_all();
        assert_eq!(c.gift_decay_seconds, 180.0);
        // A tribe that started Annoyed (-30) and has been warmed to 25 (Cautious).
        c.villages[0].baseline = -30.0;
        c.villages[0].relations = 25.0;
        assert_eq!(c.villages[0].gift_timer, 0.0);
        // A gift (+10) lifts it to Pleased and starts the timer.
        let relations = c.post_event(0, crate::tribal_rules::RelationEvent::Gift);
        assert_eq!(relations, 35.0);
        assert_eq!(c.villages[0].gift_timer, 180.0);
        c.tick(179.0);
        assert_eq!(c.villages[0].relations, 35.0);
        // The timer runs out: Pleased fades to Cautious (0), still above the
        // Annoyed baseline, so the timer restarts.
        c.tick(1.0);
        assert_eq!(c.villages[0].relations, 0.0);
        assert_eq!(c.villages[0].gift_timer, 180.0);
        // The next step reaches the baseline attitude and stops.
        c.tick(180.0);
        assert_eq!(c.villages[0].relations, -30.0);
        assert_eq!(c.villages[0].gift_timer, 0.0);
        c.tick(500.0);
        assert_eq!(c.villages[0].relations, -30.0);
        // A song note starts the timer too; a missed note and the other
        // events do not.
        c.villages[1].relations = 0.0;
        c.villages[1].baseline = -70.0;
        c.post_event(1, crate::tribal_rules::RelationEvent::Social);
        assert_eq!(c.villages[1].gift_timer, 180.0);
        c.villages[2].relations = 0.0;
        c.post_event(2, crate::tribal_rules::RelationEvent::Attack);
        assert_eq!(c.villages[2].gift_timer, 0.0);
        // A tribe that joined (allied) keeps its relationship: the timer stops.
        c.villages[1].status = RivalStatus::Allied;
        c.villages[1].relations = 80.0;
        c.tick(200.0);
        assert_eq!(c.villages[1].relations, 80.0);
        assert_eq!(c.villages[1].gift_timer, 0.0);
    }

    #[test]
    fn the_gift_timer_follows_the_difficulty() {
        use crate::creature_rules::Difficulty;
        for (difficulty, seconds) in [(Difficulty::Easy, 360.0), (Difficulty::Medium, 180.0), (Difficulty::Hard, 60.0)] {
            let mut c = TribalCampaign::default();
            c.ensure_ready_seeded(3, difficulty);
            c.reveal_all();
            c.villages[0].relations = 0.0;
            c.post_event(0, crate::tribal_rules::RelationEvent::Gift);
            assert_eq!(c.villages[0].gift_timer, seconds, "{difficulty:?}");
            // The lineup relationship is the baseline the effect fades to.
            assert_eq!(c.villages[1].baseline, c.villages[1].relations);
        }
        // A save from before the timer existed uses the normal 180 s.
        let mut old = TribalCampaign::default();
        old.ensure_ready();
        old.reveal_all();
        old.post_event(0, crate::tribal_rules::RelationEvent::Gift);
        assert_eq!(old.villages[0].gift_timer, 180.0);
    }

    #[test]
    fn a_pre_campaign_save_gains_five_hostile_villages() {
        let mut c: TribalCampaign = serde_json::from_str("{}").expect("old save");
        assert!(c.villages.is_empty());
        c.ensure_ready();
        assert_eq!(c.villages.len(), RIVAL_VILLAGES);
        assert!(c
            .villages
            .iter()
            .all(|v| v.status == RivalStatus::Hostile && v.huts == HUTS_PER_VILLAGE));
        assert_eq!(c.resolved_count(), 0);
        assert!(!c.can_advance());
        assert!(c.shacks.is_empty() && c.tools.is_empty());
        assert!(c.member_energy.is_empty() && c.births.is_empty());
        assert_eq!(c.energy_of(0), MEMBER_ENERGY_MAX);
        // A performance saved before songs existed still loads.
        let p: Performance =
            serde_json::from_str(r#"{"village":1,"prompt":2,"time_left":3.0}"#).expect("old");
        assert_eq!(p.notes, 5);
        assert_eq!(p.note, 0);
    }

    #[test]
    fn a_pre_campaign_save_syncs_an_empty_roster_onto_its_assignments() {
        // A save written before the roster existed has assignments but no
        // slots; syncing hands them out from the bottom of the range.
        let mut c: TribalCampaign = serde_json::from_str("{}").expect("old save");
        let mut tribe = vec![MateAssignment::default(); 3];
        c.sync_roster(&mut tribe);
        assert_eq!(c.slots, vec![0, 1, 2]);
        // Re-syncing is a no-op, so ids stay stable across frames.
        c.sync_roster(&mut tribe);
        assert_eq!(c.slots, vec![0, 1, 2]);
        // An oversized save is trimmed to the roster cap.
        let mut big = vec![MateAssignment::default(); MAX_MATES + 4];
        c.sync_roster(&mut big);
        assert_eq!(big.len(), MAX_MATES);
        assert_eq!(c.slots.len(), MAX_MATES);
    }

    #[test]
    fn roster_slots_are_recycled_and_stay_inside_the_restorable_range() {
        let mut c = TribalCampaign::default();
        let mut tribe = vec![MateAssignment::default(); 3];
        c.sync_roster(&mut tribe);
        assert_eq!(c.slots, vec![0, 1, 2]);

        // The member under slot 1 falls in battle; cleanup releases it and
        // drops the assignment sitting next to it.
        let index = c.release_slot(MATE_ID_BASE + 1).expect("enlisted");
        tribe.swap_remove(index);
        c.sync_roster(&mut tribe);
        assert_eq!(c.slots, vec![0, 2]);
        assert_eq!(tribe.len(), 2);

        // The next recruit reuses the freed id instead of growing the range.
        let slot = c.take_slot().expect("a slot is free again");
        assert_eq!(slot, 1);
        assert!(c.slots.contains(&slot));
        tribe.push(MateAssignment::default());
        c.sync_roster(&mut tribe);
        // Every id is unique, dense and inside what persistence can restore.
        let mut sorted = c.slots.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..MAX_MATES as u32).take(3).collect::<Vec<_>>());
        assert!(c.slots.iter().all(|s| (0..MAX_MATES as u32).contains(s)));

        // Full roster: no further slot can be claimed.
        c.slots = (0..MAX_MATES as u32).collect();
        assert!(c.take_slot().is_none());
        // An id outside the roster is not enlisted.
        assert!(c.release_slot(MATE_ID_BASE + MAX_MATES as u32).is_none());
        assert!(c.release_slot(0).is_none());
        // The lookup used to re-key survivors ignores unknown ids.
        assert_eq!(c.roster_index(MATE_ID_BASE + 99), None);
        assert_eq!(c.roster_index(MATE_ID_BASE + 3), Some(3));
    }

    #[test]
    fn ordering_without_a_performance_or_out_of_range_does_nothing() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(2).expect("starts");
        // In range: the troupe hears the order.
        assert!(c.answer_in_range(village_position(2)));
        // Out of range: not scored, and the performance is dropped.
        c.abandon_performance("too far");
        assert!(c.performance.is_none());
        assert_eq!(c.resolved_count(), 0);
        assert!(!c.order_instrument(3, [1, 1, 1]));
        // The gift timer set aside comes back (at least 30 s) when it stops.
        c.cooldown = 0.0;
        c.villages[2].gift_timer = 5.0;
        c.start_performance(2).expect("starts");
        assert_eq!(c.performance.unwrap().saved_timer, 5.0);
        c.abandon_performance("too far");
        assert_eq!(c.villages[2].gift_timer, 30.0);
        // Range is measured to the village the performance belongs to, so
        // standing near a different village does not count.
        c.cooldown = 0.0;
        c.start_performance(3).expect("starts");
        assert!(!c.answer_in_range(village_position(4)));
        assert!(!c.answer_in_range(home_position()));
        assert!(c.answer_in_range(village_position(3) + Vec3::new(INTERACT_RANGE - 0.25, 0.0, 0.0)));
    }

    #[test]
    fn campaign_state_survives_a_save_roundtrip() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[1].status = RivalStatus::Allied;
        c.villages[1].relations = ALLY_RELATIONS;
        c.villages[1].rewarded = true;
        c.villages[2].huts = 1;
        c.villages[2].hurt = 0.4;
        c.cooldown = 0.0;
        c.recruit_cooldown = 0.5;
        c.round = 7;
        c.shacks = vec![ToolKind::WoodenHorns, ToolKind::FishingSpears];
        c.tools.insert(2, ToolKind::FishingSpears);
        c.member_energy.insert(2, 42.5);
        c.births.push(TribalBirth {
            slot: 3,
            remaining: 8.0,
        });
        c.start_performance(3).expect("song");
        // Preserve a nonzero saved cooldown independently of the active song.
        c.cooldown = 1.25;
        c.say("saved");
        let json = serde_json::to_string(&c).expect("serialize");
        let back: TribalCampaign = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, c);
        assert_eq!(back.resolved_count(), 1);
        assert_eq!(back.round, 8);
        assert_eq!(back.villages[2].hurt, 0.4);
        assert_eq!(
            back.tool_of(MATE_ID_BASE + 2),
            Some(ToolKind::FishingSpears)
        );
        assert_eq!(back.performance.unwrap().village, 3);
    }

    #[test]
    fn a_forgotten_song_runs_its_five_rounds_and_fails_without_resolving_anything() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(0).expect("hostile village accepts");
        let start = c.performance.unwrap().score;
        assert_eq!(start, Troupe::FULL.participants as f32 * 25.0, "the start score: 25 per participant");
        c.tick(NOTE_WINDOW * 0.5);
        assert!(c.performance.is_some(), "the first beat is 4 s away");
        assert_eq!(c.performance.unwrap().note, 0);
        assert!(!c.performance.unwrap().bubble, "the bubble comes 1 s before the beat");
        // The bubble (3 s), then the beat (4 s): a round counted, the
        // evaluation waits 1.5 s for an order.
        c.tick(NOTE_WINDOW * 0.3);
        let p = c.performance.unwrap();
        assert!(p.bubble && (3..=5).contains(&p.prompt) && p.required >= 1, "{p:?}");
        c.tick(NOTE_WINDOW * 0.25);
        let p = c.performance.unwrap();
        assert_eq!(p.note, 1);
        assert!(p.eval_in > 0.0, "the timer waits for an order");
        c.tick(PERFORMANCE_WINDOW);
        assert!(c.performance.is_none(), "a forgotten song ends");
        // Nothing resolved, the score never grew, the audience's lift stays.
        assert_eq!(c.resolved_count(), 0);
        assert_eq!(c.village(0).unwrap().relations, 30.0);
        assert_eq!(c.villages[0].gift_timer, 180.0, "a failed song leaves the fading timer running");
        assert!(c.cooldown > 0.0);
    }

    #[test]
    fn the_social_game_scores_orders_like_the_retail_evaluation() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(0).expect("starts");
        // Before the first bubble nothing is wanted: the players count 5 each.
        let start = c.performance.unwrap().score;
        assert!(c.order_instrument(5, [2, 2, 2]));
        assert_eq!(c.performance.unwrap().score, start + 10.0);
        // The same instrument is recharging for 5 s.
        assert!(!c.order_instrument(5, [2, 2, 2]));
        c.tick(NOTE_WINDOW * 0.76);
        let p = c.performance.unwrap();
        assert!(p.bubble);
        let wanted = p.prompt;
        let other = if wanted == 3 { 4 } else { 3 };
        // A player of an instrument nobody asked for earns only the small credit.
        let before = c.performance.unwrap().score;
        assert!(c.order_instrument(other, [1, 1, 1]));
        assert_eq!(c.performance.unwrap().score, before + 5.0);
        assert!(!c.performance.unwrap().bubble, "the evaluation takes the bubble");
        // Enough players of the wanted one: a whole beat.
        let before = c.performance.unwrap().score;
        assert!(c.order_instrument(wanted, [9, 9, 9]));
        assert_eq!(c.performance.unwrap().score, before + 175.0);
        assert_eq!(c.performance.unwrap().hits, 1);
        // Too few: the share of a beat (one of the players asked for).
        let mut d = TribalCampaign::default();
        d.ensure_ready();
        d.start_performance(0).expect("starts");
        d.tick(NOTE_WINDOW * 0.76);
        let p = d.performance.unwrap();
        let before = p.score;
        assert!(p.required >= 2);
        assert!(d.order_instrument(p.prompt, {
            let mut a = [0; 3];
            a[instrument_kind(p.prompt).unwrap()] = 1;
            a
        }));
        let gained = d.performance.unwrap().score - before;
        assert!((gained - 175.0 / p.required as f32).abs() < 1e-3, "{gained}");
        // Nobody carrying the instrument: no order at all.
        assert!(!d.order_instrument(4, [3, 0, 3]));
    }

    #[test]
    fn a_full_bar_ends_the_song_early_and_the_rounds_running_out_ends_it_too() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(0).expect("starts");
        sing(&mut c);
        // 225 + whole beats fill 795 on the fourth round (a level 1 song).
        assert!(c.villages[0].gift_timer == 0.0, "a success clears the fading timer for good");
        // A song with a threshold that cannot be reached runs all five beats.
        c.cooldown = 0.0;
        c.start_performance(1).expect("starts");
        c.performance.as_mut().unwrap().threshold = 1.0e9;
        let mut seconds = 0.0;
        while let Some(p) = c.performance {
            if p.bubble && !p.evaluated {
                c.order_instrument(p.prompt, [4, 4, 4]);
            }
            c.tick(0.05);
            seconds += 0.05;
        }
        // Five rounds of 4 s, then the grace evaluation of the last one at most.
        assert!(
            (5.0 * NOTE_WINDOW - 0.1..=5.0 * NOTE_WINDOW + crate::tribal_rules::SOCIAL_EVAL_DELAY + 0.2).contains(&seconds),
            "five rounds: {seconds}"
        );
        assert_eq!(c.villages[1].relations, 30.0, "a failed song only keeps the audience's lift");
    }

    // --- routes and costs ------------------------------------------------

    #[test]
    fn perfect_songs_ally_a_village_exactly_once() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(0).expect("performance starts");
        // A Cautious tribe: a level-1 song. Its three watching members answer
        // at once (TribeSocial with scale NeutralCap - relationship, 0xDAEA70),
        // which lifts the tribe to Pleased (30) and starts the gift timer.
        let p = c.performance.unwrap();
        assert_eq!((p.level, p.notes, p.answers), (1, 5, 3));
        assert_eq!(c.villages[0].relations, 30.0);
        assert_eq!(c.villages[0].gift_timer, 180.0, "the answer starts the gift timer");
        sing(&mut c);
        // A song that succeeds lifts to its target (30 again) and clears the
        // gift timer, so the effect does not fade (0xDA356B).
        assert_eq!(c.villages[0].status, RivalStatus::Hostile, "one song is not enough");
        assert_eq!(c.villages[0].relations, 30.0);
        assert_eq!(c.villages[0].gift_timer, 0.0);
        assert!(c.cooldown > 0.0, "the cooldown is real");

        // The cooldown is real and only time clears it.
        assert!(
            c.start_performance(0).is_none(),
            "a performance respects the shared cooldown"
        );
        c.tick(ACTION_COOLDOWN);
        assert_eq!(c.cooldown, 0.0, "the cooldown expires");

        // The second song is a level-2 song (Pleased): its success lifts the
        // tribe to Friendly (70): allied.
        c.start_performance(0).expect("after the cooldown");
        assert_eq!(c.performance.unwrap().level, 2);
        sing(&mut c);
        assert_eq!(c.villages[0].status, RivalStatus::Allied, "a level-2 song that succeeds reaches 70");
        assert!(c.villages[0].relations >= ALLY_RELATIONS, "{}", c.villages[0].relations);
        assert!(c.villages[0].rewarded);

        // Once resolved: no more performances, no more rewards.
        c.cooldown = 0.0;
        assert!(c.start_performance(0).is_none());
        assert_eq!(c.attack(0, 10_000.0).reward, 0.0);
        assert_eq!(c.attack(0, 10_000.0).huts_destroyed, 0);
    }

    #[test]
    fn a_song_without_enough_players_does_not_succeed_and_never_subtracts() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[0].relations = 20.0;
        // A troupe of three with no instrument player: the start score is 75 and
        // nothing can be played for a bubble.
        let troupe = Troupe { participants: 3, instruments: [0; 3], audience: 2 };
        c.start_performance_troupe(0, troupe).expect("starts");
        // The two watching members lift the tribe to Pleased (30), not by +10.
        assert_eq!(c.villages[0].relations, 30.0);
        assert_eq!(c.performance.unwrap().score, 75.0);
        // Orders for instruments nobody carries are refused.
        for note in [3, 4, 5] {
            assert!(!c.order_instrument(note, troupe.instruments));
        }
        let mut guard = 0;
        while c.performance.is_some() && guard < 4000 {
            c.tick(0.05);
            guard += 1;
        }
        assert!(c.performance.is_none());
        // A failed song does not lift further and is not made permanent.
        assert_eq!(c.villages[0].relations, 30.0);
        assert_eq!(c.villages[0].gift_timer, 180.0, "a failed song leaves the fading timer running");
        assert_eq!(c.villages[0].status, RivalStatus::Hostile);
    }

    #[test]
    fn the_bar_animates_the_score_in_and_the_song_ends_with_it() {
        // 0xDA3490: the song succeeds when the bar object's fill reaches 1, not when
        // the score does: the start score fills over the whole song (20 s), a
        // beat's score over 1 s.
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[0].relations = 0.0;
        c.start_performance(0).expect("starts");
        let p = c.performance.unwrap();
        assert_eq!(p.level, 1);
        assert!(p.score > 0.0 && p.bar.fill == 0.0 && p.bar.animating(), "the start score is queued, not in the bar yet");
        c.tick(1.0);
        let p = c.performance.unwrap();
        let per_second = p.score / p.threshold / (p.notes as f32 * NOTE_WINDOW);
        assert!((p.bar.fill - per_second).abs() < 1e-3, "{} vs {per_second}", p.bar.fill);
        // A whole threshold arrives as a beat's score: the score is there at once,
        // the bar a second later.
        let threshold = p.threshold;
        if let Some(p) = c.performance.as_mut() {
            p.add_score(threshold, crate::tribal_rules::SOCIAL_BAR_BEAT_SECONDS);
        }
        assert!(c.performance.unwrap().score >= threshold);
        c.tick(0.5);
        assert!(c.performance.is_some(), "half a second later the bar is not full yet");
        c.tick(0.6);
        assert!(c.performance.is_none(), "the bar is full: success");
        let end = c.social_end.expect("the bar stays up for the ending");
        assert!(end.success && end.meter.is_full() && end.level == 1);
        assert_eq!(c.villages[0].relations, 30.0, "a level 1 song lifts the tribe to Pleased");
        c.tick(end.left + 0.1);
        assert!(c.social_end.is_none(), "the stop hides the bar");
    }

    #[test]
    fn a_gathering_song_waits_for_the_troupe_to_stand_at_its_spots() {
        // States 0..4 of 0xDA2F30: the song exists and its bar is up, but the clock,
        // the bubbles and the start score wait until every participant is in place.
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[0].relations = 0.0;
        c.start_performance_gathering(0, Troupe::FULL).expect("starts");
        let p = c.performance.unwrap();
        assert!(p.gathering && p.start_score > 0.0 && p.score == 0.0 && !p.bar.animating());
        c.tick(20.0);
        let p = c.performance.unwrap();
        assert!(p.gathering && p.note == 0 && p.bubble_serial == 0 && p.time_left == NOTE_WINDOW, "{p:?}");
        // Orders wait for the music.
        assert!(!c.order_instrument(3, [4, 4, 4]));
        assert!(c.troupe_ready(), "everybody arrived");
        let p = c.performance.unwrap();
        assert!(!p.gathering && p.start_score == 0.0 && p.score > 0.0 && p.bar.animating());
        assert_eq!(p.time_left, NOTE_WINDOW);
        assert!(!c.troupe_ready(), "only once");
        c.tick(NOTE_WINDOW - 0.9);
        assert!(c.performance.unwrap().bubble_serial == 1, "the first bubble one second before the first beat");
        // A song nobody could walk to the spots for starts after the safety timeout.
        c.performance = None;
        c.cooldown = 0.0;
        c.start_performance_gathering(0, Troupe::FULL).expect("starts");
        c.tick(crate::tribal_rules::SOCIAL_GATHER_TIMEOUT + 0.1);
        assert!(!c.performance.unwrap().gathering);
    }

    #[test]
    fn a_level_0_song_has_no_bar_and_never_succeeds_0xda35b1() {
        // The Furious / Annoyed / Friendly tribes: no bar object (+0xE8 stays null,
        // every performance is a fresh object), so the end path has no success branch.
        for relations in [-80.0, -30.0] {
            let mut c = TribalCampaign::default();
            c.ensure_ready();
            c.villages[0].relations = relations;
            c.start_performance(0).expect("starts");
            let p = c.performance.unwrap();
            assert_eq!(p.level, 0);
            // Answer everything a bubble asks for: the score passes the threshold.
            let mut guard = 0;
            while c.performance.is_some() && guard < 4000 {
                guard += 1;
                let prompt = c.performance.unwrap().prompt;
                c.order_instrument(prompt, [4, 4, 4]);
                c.tick(0.05);
            }
            assert!(c.performance.is_none(), "the rounds run out");
            let end = c.social_end.expect("the members still play their ending");
            assert!(!end.success && !end.has_bar, "{end:?}");
            assert!(c.villages[0].relations <= 30.0, "no permanent lift");
            assert_ne!(c.villages[0].gift_timer, 0.0, "the audience's answer started the fading timer and a failed song does not clear it");
        }
    }

    #[test]
    fn the_ending_waits_for_the_members_clips() {
        // The bar stays until the members' cheer and ending clip are done
        // (`social_ending_seconds`), and the bar value picks their clips.
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[0].relations = 0.0;
        c.start_performance(0).expect("starts");
        let mut guard = 0;
        while c.performance.is_some() && guard < 2000 {
            guard += 1;
            c.tick(0.05);
        }
        let end = c.social_end.expect("ending");
        assert!(end.has_bar && end.bar > 0.0 && end.bar <= 10.0);
        // (The tick that ended the song already ran 50 ms of the ending.)
        let total = crate::tribal_rules::social_ending_seconds(end.bar);
        assert!(end.left <= total && end.left > total - 0.06, "{} vs {total}", end.left);
        assert!(end.left > 7.0, "longer than the 3 s of the earlier port: {}", end.left);
        c.tick(end.left - 0.5);
        assert!(c.social_end.is_some());
        c.tick(1.0);
        assert!(c.social_end.is_none());
    }

    #[test]
    fn a_song_nobody_answers_fails_and_its_bar_drains() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[0].relations = 0.0;
        c.start_performance(0).expect("starts");
        let mut guard = 0;
        while c.performance.is_some() && guard < 2000 {
            guard += 1;
            c.tick(0.05);
        }
        assert!(c.performance.is_none(), "the rounds run out");
        let end = c.social_end.expect("the bar stays up for the ending");
        assert!(!end.success && end.meter.failed, "a failed song drains its bar (0xE31610)");
        let before = end.meter.fill;
        assert!(before > 0.0 && before < 1.0, "the start score alone is in the bar: {before}");
        c.tick(1.0);
        let after = c.social_end.unwrap().meter.fill;
        assert!(after < before, "{after} < {before}");
        // A song that was abandoned leaves no bar.
        c.cooldown = 0.0;
        c.start_performance(0).expect("starts");
        c.abandon_performance("left");
        assert!(c.social_end.is_none());
    }

    #[test]
    fn the_song_level_follows_the_attitude_when_it_starts() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        for (relations, level) in [(-80.0, 0), (-30.0, 0), (0.0, 1), (29.0, 1), (30.0, 2), (69.0, 2)] {
            c.villages[0].relations = relations;
            c.cooldown = 0.0;
            c.performance = None;
            c.start_performance(0).expect("starts");
            assert_eq!(c.performance.unwrap().level, level, "{relations}");
            // The level picks the archetype's threshold (+0x90 for level 1).
            let tuning = crate::tribal_rules::social_tuning(c.tribe_of(0).archetype);
            assert_eq!(
                c.performance.unwrap().threshold,
                crate::tribal_rules::social_threshold(&tuning, level as u8)
            );
        }
        // A Furious tribe: the audience still answers (lifting it to Pleased)
        // but a level-0 song has no bar and no success branch (0xDA35B1): it ends
        // after the last round and does not clear the timer (0xDA3565 is the
        // success path), so the lift fades again.
        c.villages[0].relations = -80.0;
        c.cooldown = 0.0;
        c.performance = None;
        c.start_performance(0).expect("starts");
        sing(&mut c);
        assert_eq!(c.villages[0].relations, 30.0);
        assert_eq!(c.villages[0].status, RivalStatus::Hostile);
        assert!(!c.social_end.expect("ending").success, "a level 0 song cannot succeed");
        assert!(c.villages[0].gift_timer > 0.0, "the fading timer keeps running");
        // The gift timer of the tribe does not run while its song plays.
        c.cooldown = 0.0;
        c.start_performance(0).expect("starts");
        c.tick(2.0);
        assert_eq!(c.villages[0].gift_timer, 180.0, "paused during the song");
    }

    #[test]
    fn an_instrument_nobody_carries_cannot_be_ordered() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance_troupe(0, Troupe { participants: 4, instruments: [2, 0, 0], audience: 3 })
            .expect("starts");
        assert!(!c.order_instrument(4, [2, 0, 0]), "no maraca player");
        assert!(c.order_instrument(3, [2, 0, 0]), "two horn players");
        assert_eq!(c.performance.unwrap().orders, 1);
        assert_eq!(c.performance.unwrap().last_order, 3);
        // The first order of a song ends the tutorial hint and sets its flag.
        assert!(c.social_tutorial_done);
        let events = std::mem::take(&mut c.social_events.0);
        assert_eq!(events, vec![SocialEvent::Started { seen: false }, SocialEvent::FirstOrder]);
    }

    #[test]
    fn the_tutorial_hint_comes_back_after_two_unanswered_rounds() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(0).expect("starts");
        c.social_events.0.clear();
        // Rounds 1 and 2 pass without an order (the timer evaluates each);
        // the third bubble shows the reminder (0xDA213F: more than one).
        let mut seen = Vec::new();
        for _ in 0..(NOTE_WINDOW * 2.5 / 0.05) as usize + 40 {
            c.tick(0.05);
            seen.extend(std::mem::take(&mut c.social_events.0));
            if seen.contains(&SocialEvent::Reminder) {
                break;
            }
        }
        assert!(seen.contains(&SocialEvent::Reminder), "{seen:?}");
        assert!(c.performance.unwrap().note >= 2);
        // Ordering ends the song's hint.
        let p = c.performance.unwrap();
        c.order_instrument(p.prompt.max(3), [2, 2, 2]);
        assert!(c.social_events.0.contains(&SocialEvent::FirstOrder));
    }

    #[test]
    fn a_stopped_song_tells_the_hud() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.start_performance(0).expect("starts");
        c.abandon_performance("away");
        assert_eq!(
            std::mem::take(&mut c.social_events.0),
            vec![SocialEvent::Started { seen: false }, SocialEvent::Ended]
        );
    }

    #[test]
    fn a_gift_calms_an_angry_tribe_to_neutral_and_does_nothing_for_the_rest() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        // A gift is `giftFoodAmount` (10) food.
        assert_eq!(GIFT_FOOD, 10.0);
        let broke = c.gift(0, GIFT_FOOD - 1.0);
        assert_eq!(broke.food_delta, 0.0, "no food, no gift");
        assert_eq!(c.village(0).unwrap().relations, 0.0);
        // A Cautious tribe (0) keeps its relationship: the food is gone, the
        // fading timer runs (0xDBC1F1..0xDBC28F: attitude >= 2 posts nothing).
        let paid = c.gift(0, GIFT_FOOD);
        assert_eq!(paid.food_delta, -GIFT_FOOD);
        assert_eq!(c.village(0).unwrap().relations, 0.0);
        assert_eq!(c.village(0).unwrap().gift_timer, 180.0);
        // An Annoyed or Furious tribe is lifted to exactly 0 (Cautious).
        for before in [-30.0, -55.0, -90.0] {
            c.cooldown = 0.0;
            c.villages[0].relations = before;
            let next = c.gift(0, GIFT_FOOD);
            assert_eq!(next.food_delta, -GIFT_FOOD);
            assert_eq!(c.village(0).unwrap().relations, 0.0, "{before}");
        }
        // Gifts alone never ally, however many are given.
        for _ in 0..8 {
            c.cooldown = 0.0;
            let more = c.gift(0, GIFT_FOOD);
            assert!(!more.resolved_now);
        }
        assert_eq!(c.village(0).unwrap().status, RivalStatus::Hostile);
        assert_eq!(c.village(0).unwrap().relations, 0.0);
        // A Pleased tribe is not warmed any further either.
        c.cooldown = 0.0;
        c.villages[0].relations = 40.0;
        c.gift(0, GIFT_FOOD);
        assert_eq!(c.village(0).unwrap().relations, 40.0);
        // Two songs ally it (Cautious -> Pleased -> Friendly).
        c.villages[0].relations = 0.0;
        ally_by_performance(&mut c, 0);
        assert_eq!(c.village(0).unwrap().status, RivalStatus::Allied);
        // A resolved village takes no gifts.
        c.cooldown = 0.0;
        assert_eq!(c.gift(0, GIFT_FOOD), TribalEvent::default());
    }

    #[test]
    fn conquest_destroys_pieces_by_hit_points_and_pays_once() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        // Village 0 is a tier-1 tribe: two tool racks of 200 hit points fall
        // first, then the dwelling, a level-1 hut of 400.
        // Damage accumulates instead of vanishing.
        let nick = c.attack(0, 100.0);
        assert_eq!(nick.huts_destroyed, 0);
        assert_eq!(c.village(0).unwrap().huts, HUTS_PER_VILLAGE);
        assert_eq!(c.village(0).unwrap().hurt, 100.0);

        let first = c.attack(0, 100.0);
        assert_eq!((first.huts_destroyed, first.huts_left), (1, 2), "a 200 point rack falls");
        let second = c.attack(0, 200.0);
        assert_eq!((second.huts_destroyed, second.huts_left), (1, 1));
        let almost = c.attack(0, 399.0);
        assert!(!almost.resolved_now, "the 400 point hut still stands");
        let last = c.attack(0, 1.0);
        assert!(last.resolved_now);
        let reward = first.reward + second.reward + almost.reward + last.reward;
        assert_eq!(c.village(0).unwrap().huts, 0);
        assert_eq!(c.village(0).unwrap().status, RivalStatus::Conquered);
        assert_eq!(reward, 0.0, "the original pays no food for a conquest");
        assert_eq!(c.village(0).unwrap().relations, -100.0, "destroyed racks end any friendship");
        // A conquered village takes no more damage and pays nothing more.
        let after = c.attack(0, 10_000.0);
        assert_eq!(after.huts_destroyed, 0);
        assert_eq!(after.reward, 0.0);
        assert_eq!(c.village(0).unwrap().huts, 0);

        // A tier-5 tribe's dwelling is a level-3 hut of 1000 hit points.
        c.attack(4, 400.0);
        assert_eq!(c.village(4).unwrap().huts, 1);
        let big = c.attack(4, 999.0);
        assert!(!big.resolved_now);
        assert!(c.attack(4, 1.0).resolved_now);
    }

    #[test]
    fn recruitment_and_recovery_both_need_and_spend_food() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert!(!c.recruit(RECRUIT_FOOD - 1.0, 3), "no food, no recruit");
        assert!(c.recruit(RECRUIT_FOOD, 3), "affordable recruit");
        assert_eq!(c.pending_birth_count(), 1);
        assert_eq!(c.slots.len(), 0, "an egg is not a living roster member");
        assert!(c.recruit_cooldown > 0.0, "recruiting is rate limited");
        // The rate limit is a real timer: a second recruit is refused until
        // it has ticked down, and it then only clears the cooldown.
        assert!(!c.recruit(RECRUIT_FOOD, 3), "still within the rate limit");
        c.tick(ACTION_COOLDOWN);
        assert_eq!(c.recruit_cooldown, 0.0, "the rate limit expires");
        // Pending eggs occupy beds and slots before they hatch.
        assert!(
            c.recruit(RECRUIT_FOOD, 3),
            "an affordable second egg queues"
        );
        assert_eq!(c.pending_birth_count(), 2);
        assert_eq!(c.births[0].slot, 0);
        assert_eq!(c.births[1].slot, 1);
        c.recruit_cooldown = 0.0;
        assert!(
            !c.recruit(RECRUIT_FOOD, MAX_MATES),
            "the valley has a limit"
        );
        c.recruit_cooldown = 0.0;
        assert!(
            !c.recruit(RECRUIT_FOOD, START_POPULATION_CAP),
            "pending eggs count against hut capacity"
        );
        // Hatching only occurs after the complete saved growth interval.
        assert!(c.tick_births(BIRTH_GROWTH_SECONDS - 0.1).is_empty());
        let hatched = c.tick_births(0.1);
        assert_eq!(hatched, vec![0, 1]);
        c.slots.extend(hatched);
        assert_eq!(c.slots.len(), 2);
        c.recruit_cooldown = 0.0;
        c.villages[0].status = RivalStatus::Allied;
        assert!(c.recruit(RECRUIT_FOOD, 2), "alliance adds beds");
        assert_eq!(population_cap(RIVAL_VILLAGES), MAX_MATES);
        assert_eq!(hut_level(0), 1);
        assert_eq!(hut_level(5), 3);

        let mut h = TribalCampaign::default();
        assert!(!h.recover(RECOVER_FOOD, 0.0), "nothing to heal");
        assert!(!h.recover(RECOVER_FOOD - 1.0, 40.0), "no food, no recovery");
        assert!(h.recover(RECOVER_FOOD, 40.0), "affordable recovery");
    }

    #[test]
    fn queued_births_are_unique_cancelable_and_trimmed_when_beds_shrink() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert!(c.recruit(RECRUIT_FOOD, 3));
        c.recruit_cooldown = 0.0;
        assert!(c.recruit(RECRUIT_FOOD, 3));
        assert_eq!(
            c.births.iter().map(|b| b.slot).collect::<Vec<_>>(),
            vec![0, 1]
        );
        c.slots.extend([2, 3, 4]);
        assert_eq!(c.trim_births_to_capacity(5), 1);
        assert_eq!(c.births.len(), 1);
        assert_eq!(c.births[0].slot, 0, "the oldest egg is preserved");
        assert_eq!(c.cancel_births(), 1);
        assert!(
            c.births.is_empty(),
            "explicit cancellation clears the remaining egg"
        );
        assert_eq!(
            c.trim_births_to_capacity(MAX_MATES + 1),
            0,
            "an over-cap living roster cannot make trimming loop without eggs"
        );
    }

    #[test]
    fn menu_reload_exit_preserves_eggs_but_permanent_stage_advance_cancels_them() {
        let mut c = TribalCampaign::default();
        c.births.push(TribalBirth {
            slot: 0,
            remaining: 12.0,
        });
        assert_eq!(
            cancel_births_on_stage_exit(&mut c, GameStage::Tribal),
            0,
            "continuing or returning to menu still resumes Tribal"
        );
        assert_eq!(c.births.len(), 1);
        assert_eq!(
            cancel_births_on_stage_exit(&mut c, GameStage::Civilization),
            1
        );
        assert!(c.births.is_empty());
    }

    #[test]
    fn tribal_hud_summary_reports_average_energy_and_next_birth() {
        let mut c = TribalCampaign::default();
        c.slots = vec![0, 2];
        c.member_energy.insert(0, 20.0);
        c.member_energy.insert(2, 80.0);
        c.births.push(TribalBirth {
            slot: 1,
            remaining: 7.5,
        });
        let summary = c.status_summary();
        assert_eq!(summary.members, 2);
        assert_eq!(summary.average_energy_percent, 50.0);
        assert_eq!(summary.hungry_members, 1);
        assert_eq!(summary.pending_births, 1);
        assert_eq!(summary.next_birth_seconds, Some(7.5));
    }

    #[test]
    fn shacks_cost_their_authored_price_and_unlock_in_tiers() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.sync_roster(&mut vec![MateAssignment::default(); 3]);
        assert!(
            c.build_shack(ToolKind::FishingSpears, 10.0).is_err(),
            "too poor"
        );
        assert_eq!(c.build_shack(ToolKind::FishingSpears, 100.0), Ok(20.0));
        assert!(
            c.build_shack(ToolKind::FishingSpears, 100.0).is_err(),
            "once"
        );
        assert!(c.build_shack(ToolKind::Maracas, 100.0).is_err(), "locked");
        c.villages[0].status = RivalStatus::Conquered;
        assert_eq!(c.build_shack(ToolKind::Maracas, 100.0), Ok(25.0));
        // Members take tools only from built shacks, the campfire stays put.
        assert!(c.equip(MATE_ID_BASE, ToolKind::StoneAxes).is_err());
        assert!(c.equip(MATE_ID_BASE, ToolKind::FishingSpears).is_ok());
        assert_eq!(c.tool_of(MATE_ID_BASE), Some(ToolKind::FishingSpears));
        assert!(
            c.equip(MATE_ID_BASE + 9, ToolKind::Maracas).is_err(),
            "not enlisted"
        );
        // A fallen member's tool is forgotten; its recycled slot starts bare.
        c.release_slot(MATE_ID_BASE);
        assert_eq!(c.tool_of(MATE_ID_BASE), None);
    }

    // --- combat scaling --------------------------------------------------

    #[test]
    fn attack_power_and_retaliation_answer_to_roles_tools_and_attackers() {
        let base = attack_power(&[0, 0, 0], 3);
        assert!(base > 0.0, "a starting tribe can still conquer");
        assert!(attack_power(&[1, 1, 1], 3) > base, "warriors hit harder");
        assert!(
            attack_power(&[0, 0, 0], 8) > base,
            "a bigger tribe hits harder"
        );
        let armed = siege_damage(true, &[Some(ToolKind::SpikedClubs); 3], &[0, 0, 0]);
        assert!(armed > base, "torches burn huts faster");
        assert!(siege_damage(false, &[], &[0, 0, 0]) == 0.0);
        // The retail raze damage (`creatureAbilityRazeDamage`): the chieftain's
        // staff 7 per 2 s, a torch 9 per 1.5 s, an axe 5 per 2 s, a spear 4 per
        // 3 s, the shaman staff 3 per 2.5 s, bare hands (ADAPTED) 1 per 1.5 s.
        let per_second = |chief: bool, who: &[Option<ToolKind>]| siege_damage(chief, who, &[0]);
        assert!((per_second(true, &[]) - 3.5).abs() < 1e-4);
        assert!((per_second(false, &[Some(ToolKind::SpikedClubs)]) - 6.0).abs() < 1e-4);
        assert!((per_second(false, &[Some(ToolKind::StoneAxes)]) - 2.5).abs() < 1e-4);
        assert!((per_second(false, &[Some(ToolKind::ThrowingSpears)]) - 4.0 / 3.0).abs() < 1e-4);
        assert!((per_second(false, &[Some(ToolKind::HealingRods)]) - 1.2).abs() < 1e-4);
        assert!((per_second(false, &[None]) - 1.0 / 1.5).abs() < 1e-4);
        // Warriors also blunt the retaliation of a tribe's defenders.
        let tribe = crate::tribal_rules::village_tribe(0, 3);
        assert!(retaliation_dps(tribe, &[1, 1, 1, 1]) < retaliation_dps(tribe, &[0, 0, 0]));
        assert!(retaliation_dps(tribe, &[0, 0, 0]) > 0.0);
    }

    // --- the win condition ----------------------------------------------

    #[test]
    fn a_full_food_stockpile_alone_cannot_skip_the_stage() {
        let mut run = RunProgress::default();
        run.tribal_campaign.ensure_ready();
        run.dna_this_stage = 100_000.0;
        assert!(
            !run.tribal_campaign.can_advance(),
            "food is a stockpile, not a win condition"
        );
    }

    #[test]
    fn every_village_must_resolve_before_the_tribe_advances() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert!(!c.can_advance());
        for i in 0..RIVAL_VILLAGES - 1 {
            ally_by_performance(&mut c, i);
            assert_eq!(c.resolved_count(), i + 1);
            assert!(!c.can_advance(), "{i} allies is not enough");
        }
        ally_by_performance(&mut c, RIVAL_VILLAGES - 1);
        assert_eq!(c.resolved_count(), RIVAL_VILLAGES);
        assert!(c.can_advance());
    }

    #[test]
    fn hud_names_every_village_and_shows_the_prompt() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        let hud = c.hud();
        for i in 1..=RIVAL_VILLAGES {
            assert!(hud.contains(&format!("V{i}")), "missing V{i} in:\n{hud}");
        }
        assert!(hud.contains("0/5"), "no progress shown in:\n{hud}");
        c.start_performance(2).expect("starts");
        let hud = c.hud();
        assert!(hud.contains("V3"), "{hud}");
        assert!(hud.contains("song: round 1/5"), "the song's round missing from:\n{hud}");
        c.tick(NOTE_WINDOW * 0.8);
        let hud = c.hud();
        assert!(hud.contains("asks for"), "the bubble is missing from:\n{hud}");
        sing(&mut c);
        // A village allies after two songs (Pleased, then Friendly).
        for _ in 0..1 {
            c.tick(ACTION_COOLDOWN);
            c.start_performance(2)
                .expect("cooled-down further performance");
            sing(&mut c);
        }
        let hud = c.hud();
        // Once resolved the head reports progress and the village row carries
        // the uppercase status; the head only ever shows a live prompt, so it
        // deliberately never repeats the status word.
        assert!(
            hud.contains(RivalStatus::Allied.label()),
            "resolved village must show its status in:\n{hud}"
        );
        assert!(
            !hud.contains("asks for"),
            "no prompt should be left:\n{hud}"
        );
        assert!(hud.contains("1/5"), "{hud}");
    }

    // --- runtime (headless, real schedule) -------------------------------

    fn food(app: &App) -> f32 {
        app.world().resource::<RunProgress>().dna_this_stage
    }

    #[test]
    fn gatherers_carry_fruit_to_the_food_mat_but_never_leave_the_stage() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);
        assert_eq!(food(&app), crate::tribal_rules::NEW_TRIBE_FOOD, "newTribeFood");
        // The tribe is selected (HUD key; Q is a camera key), Z sends it to
        // the nearest bushes.
        tap(&mut app, crate::tribal_controls::KEY_SELECT_TRIBE);
        tap(&mut app, KeyCode::KeyZ);
        let mut carried = false;
        for _ in 0..240 {
            step(&mut app);
            let world = app.world_mut();
            carried |= world
                .query::<&TribeUnit>()
                .iter(world)
                .any(|u| u.carrying > 0.0);
        }
        assert!(carried, "members fill their arms at the bushes");
        // Bundles are five fruit (one food each); the stockpile piles up anyway.
        let goal = 120.0;
        for _ in 0..12_000 {
            if stage_of(&app) != GameStage::Tribal || food(&app) > goal {
                break;
            }
            step(&mut app);
        }
        assert_eq!(
            stage_of(&app),
            GameStage::Tribal,
            "gathering must never advance the tribal stage"
        );
        assert!(
            food(&app) > goal,
            "food piled up as a stockpile ({:.0})",
            food(&app)
        );
        assert_eq!(campaign(&app).resolved_count(), 0, "and it settled nothing");
        // The gatherers are still at it on their own.
        let world = app.world_mut();
        assert!(world
            .query::<&UnitTask>()
            .iter(world)
            .any(|t| matches!(t, UnitTask::Gather(_) | UnitTask::Deliver(_))));
    }

    #[test]
    fn idle_chieftain_holds_a_restored_pose_but_explicit_move_still_works() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);

        // A checkpoint can restore the player away from the spawn anchor,
        // while the chief's fresh setup task still contains that old anchor.
        let restored = village_position(0);
        teleport(&mut app, restored);
        for _ in 0..10 {
            step(&mut app);
        }
        let (position, task) = {
            let world = app.world_mut();
            let (position, task) = world
                .query_filtered::<(&Transform, &UnitTask), With<Player>>()
                .single(world)
                .expect("one chieftain");
            (position.translation, *task)
        };
        assert!(
            crate::game::dist_xz(position, restored) < 0.1,
            "an idle chieftain should hold the restored position: {position:?}"
        );
        assert!(
            matches!(task, UnitTask::Idle(anchor) if crate::game::dist_xz(anchor, position) < 0.1)
        );

        // The hold behavior must not swallow a deliberate RTS move order.
        let destination = restored + Vec3::new(3.0, 0.0, 0.0);
        {
            let world = app.world_mut();
            let mut chief = world.query_filtered::<&mut UnitTask, With<Player>>();
            *chief.single_mut(world).expect("one chieftain") = UnitTask::MoveTo(destination);
        }
        for _ in 0..180 {
            step(&mut app);
            let world = app.world_mut();
            let position = world
                .query_filtered::<&Transform, With<Player>>()
                .single(world)
                .expect("one chieftain")
                .translation;
            if crate::game::dist_xz(position, destination) < 0.35 {
                break;
            }
        }
        let world = app.world_mut();
        let position = world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .expect("one chieftain")
            .translation;
        assert!(
            crate::game::dist_xz(position, destination) < 0.35,
            "an explicit MoveTo task should still move the chieftain: {position:?}"
        );
    }

    #[test]
    fn fishers_need_spears_from_a_built_shack() {
        let run = RunProgress::default();
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Tribal);
        // Entering a new stage resets its food stockpile; seed the test after
        // the transition so the shack purchase exercises the normal build path.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 100.0;
        tap(&mut app, KeyCode::KeyX);
        assert!(campaign(&app).message.contains("Fishing Spears"));
        // T walks the palette to Fishing Spears, B builds the shack, B again
        // hands the spears out.
        while app
            .world()
            .resource::<crate::tribal_rts::TribalRts>()
            .palette
            != ToolKind::FishingSpears
        {
            tap(&mut app, KeyCode::KeyT);
        }
        tap(&mut app, KeyCode::KeyB);
        assert!(campaign(&app).has_shack(ToolKind::FishingSpears));
        assert_eq!(food(&app), 80.0, "the authored price is paid");
        step(&mut app);
        tap(&mut app, KeyCode::KeyB);
        for _ in 0..900 {
            step(&mut app);
            if campaign(&app).tools.len() == 3 {
                break;
            }
        }
        assert_eq!(campaign(&app).tools.len(), 3, "every member took spears");
        tap(&mut app, KeyCode::KeyX);
        let before = food(&app);
        // A bundle is five fish at 1.5 food each: 7.5 food per catch trip.
        for _ in 0..6000 {
            step(&mut app);
            if food(&app) >= before + 15.0 {
                break;
            }
        }
        assert!(food(&app) >= before + 15.0, "fish reach the food mat");
    }

    #[test]
    fn attacking_sends_the_tribe_and_the_village_fights_back() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);
        // The first NPC tribe appears after the start (`tribal_rules`): bring V1 into view.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[0].appeared = true;
        step(&mut app);
        let before_hp = player_hp(&mut app);
        teleport(&mut app, village_position(0));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyF);
        step(&mut app);
        let world = app.world_mut();
        let sieging = world
            .query_filtered::<&UnitTask, With<TribeMate>>()
            .iter(world)
            .filter(|t| **t == UnitTask::Siege(0))
            .count();
        assert_eq!(sieging, 3, "F sends every member at the village");
        // Keep holding F: the chieftain fights at the walls with them. A tool
        // rack has 200 hit points and the chieftain's staff deals 6 a second,
        // so the first piece takes a while (the defenders keep him busy).
        for _ in 0..6000 {
            hold(&mut app, KeyCode::KeyF, 1);
            if campaign(&app).village(0).unwrap().huts < HUTS_PER_VILLAGE {
                break;
            }
        }
        let huts = campaign(&app).village(0).unwrap().huts;
        assert!(
            huts < HUTS_PER_VILLAGE,
            "the siege burns pieces ({huts} left)"
        );
        assert!(
            player_hp(&mut app) < before_hp,
            "the village must fight back"
        );
    }

    #[test]
    fn performances_carry_the_tribe_all_the_way_to_civilization() {
        let mut run = RunProgress::default();
        run.dna_this_stage = 500.0;
        // A full tribe: the retail songs ask for two to four players of one
        // instrument and fill a bar of 795..1090 points (25 per participant,
        // 175 per whole beat).
        run.tribe = (0..MAX_MATES).map(|_| MateAssignment::default()).collect();
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Tribal);
        reveal_all(&mut app);
        let kinds = [ToolKind::WoodenHorns, ToolKind::Maracas, ToolKind::Aerophones];
        let mut frames = 0;
        while stage_of(&app) == GameStage::Tribal && frames < 120_000 {
            frames += 1;
            let Some(target) = first_hostile(&app) else {
                break;
            };
            // Build whatever music is unlocked and hand it round the troupe
            // (what is still locked fails and is tried again next song).
            {
                let mut run = app.world_mut().resource_mut::<RunProgress>();
                let run = &mut *run;
                for kind in kinds {
                    let _ = run.tribal_campaign.build_shack(kind, 999.0);
                }
                for (i, slot) in run.tribal_campaign.slots.clone().into_iter().enumerate() {
                    let _ = run.tribal_campaign.equip(MATE_ID_BASE + slot, kinds[i % 3]);
                }
            }
            let stand = village_position(target) + Vec3::new(0.0, 0.0, 5.0);
            teleport_tribe(&mut app, stand);
            tap(&mut app, crate::tribal_controls::KEY_PERFORM);
            if campaign(&app).performance.is_none() {
                // Still cooling down; let time pass.
                step(&mut app);
                continue;
            }
            // Play the song: every bubble is answered with its instrument.
            let mut song_frames = 0;
            while campaign(&app).performance.is_some() && song_frames < 3000 {
                song_frames += 1;
                teleport_tribe(&mut app, stand);
                match campaign(&app).performance {
                    // The instrument the village asked for, whenever it is ready.
                    Some(p) if p.required > 0 && p.prompt >= 3 && p.refresh[instrument_kind(p.prompt).unwrap()] <= 0.0 => {
                        tap(&mut app, digit(p.prompt))
                    }
                    _ => step(&mut app),
                }
            }
        }
        // The last song resolved the fifth village: the stage waits for the
        // Evolve button, whose key then ends it (`NextState` is applied by
        // the next frame's state transition - commit it before asserting).
        step(&mut app);
        assert_eq!(
            stage_of(&app),
            GameStage::Tribal,
            "five allied villages wait for the Evolve button"
        );
        tap(&mut app, crate::tribal_hud::ADVANCE_KEY);
        step(&mut app);
        assert_eq!(
            stage_of(&app),
            GameStage::Civilization,
            "five allied villages must advance the tribe"
        );
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.tribal_campaign.resolved_count(), RIVAL_VILLAGES);
        assert_eq!(run.stage_order_index, 3);
        assert_eq!(
            run.dna_this_stage, 0.0,
            "the stockpile is banked on advance"
        );
    }

    #[test]
    fn conquest_resolves_villages_at_runtime_too() {
        // Eight warrior mates: high attack power, softened retaliation.
        let mut run = RunProgress::default();
        run.tribe = (0..8)
            .map(|_| MateAssignment {
                role: 1,
                ..default()
            })
            .collect();
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Tribal);
        reveal_all(&mut app);
        // Retail members only fight well with a weapon: build the spears (open
        // from the start) and hand them to the six a rack holds.
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            let run = &mut *run;
            run.tribal_campaign
                .build_shack(ToolKind::ThrowingSpears, 999.0)
                .expect("spears are open from the start");
            for i in 0..crate::tribal_rules::MAX_TOOLS_PER_RACK {
                let id = MATE_ID_BASE + run.tribal_campaign.slots[i];
                run.tribal_campaign
                    .equip(id, ToolKind::ThrowingSpears)
                    .expect("the rack hands out six");
            }
        }
        // The roaming herds are outside this bounded siege scenario.
        {
            let world = app.world_mut();
            let mut wild =
                world.query::<(&mut Transform, &mut crate::tribal_rts::WildAnimal)>();
            for (mut t, mut a) in wild.iter_mut(world) {
                t.translation = Vec3::new(28.0, 0.0, -28.0);
                a.home = t.translation;
                a.target = t.translation;
                a.wait = 1_000.0;
            }
        }
        for _ in 0..RIVAL_VILLAGES {
            let target = first_hostile(&app).expect("a hostile village remains");
            // The war party arrives together.
            teleport_tribe(&mut app, village_position(target) + Vec3::new(0.0, 0.0, 4.0));
            teleport(&mut app, village_position(target));
            // Each village has 800-1400 hit points of racks and hut.
            for _ in 0..9000 {
                if campaign(&app)
                    .village(target)
                    .is_some_and(|v| v.status.is_resolved())
                {
                    break;
                }
                hold(&mut app, KeyCode::KeyF, 1);
            }
            let v = campaign(&app).village(target).unwrap().clone();
            let members = app.world().resource::<RunProgress>().tribe.len();
            let chief = player_hp(&mut app);
            assert_eq!(
                v.status,
                RivalStatus::Conquered,
                "V{} after the siege: {} pieces, {:.0} damage on the next, {} members, chieftain {:.0} hp, respawn {:.0}",
                target + 1,
                v.huts,
                v.hurt,
                members,
                chief,
                campaign(&app).chieftain_respawn
            );
        }
        // The fifth conquest makes the Evolve button appear; its key arms
        // `NextState`, which the state machine applies on the following update.
        step(&mut app);
        assert_eq!(stage_of(&app), GameStage::Tribal, "the stage waits for Evolve");
        tap(&mut app, crate::tribal_hud::ADVANCE_KEY);
        step(&mut app);
        assert_eq!(stage_of(&app), GameStage::Civilization);
    }

    #[test]
    fn the_editor_never_invents_roster_members_on_a_small_tribe() {
        // Regression: the editor used to assume at least three members and
        // resize `run.tribe` up to its focus, which desynced the roster from
        // the campaign slots and charged roles onto entities that were never
        // spawned. It must follow the real roster and never grow it.
        for start in 0..=1usize {
            let mut run = RunProgress::default();
            run.tribe = vec![MateAssignment::default(); start];
            let mut app = headless_app(run);
            enter(&mut app, GameStage::Tribal);
            app.world_mut().insert_resource(crate::Originals::default());

            // A stale focus left over from a bigger save must be clamped,
            // not honoured as an out-of-range index.
            app.world_mut().resource_mut::<RunProgress>().focused_part = 7;
            for key in [
                KeyCode::KeyE,
                KeyCode::Digit1,
                KeyCode::Tab,
                KeyCode::Comma,
                KeyCode::Period,
                KeyCode::Backspace,
            ] {
                tap(&mut app, key);
            }
            let run = app.world().resource::<RunProgress>();
            assert_eq!(
                run.tribe.len(),
                start,
                "editing a {start}-member tribe must not invent members"
            );
            assert!(
                run.tribal_campaign.slots.len() <= run.tribe.len(),
                "slots must never outnumber the roster ({}/{})",
                run.tribal_campaign.slots.len(),
                run.tribe.len()
            );
            assert!(
                run.tribe.is_empty() || run.focused_part < run.tribe.len() as u32,
                "focus {} must stay inside the {start}-member roster",
                run.focused_part
            );
        }
    }

    #[test]
    fn the_editor_wraps_focus_around_the_real_roster() {
        let mut run = RunProgress::default();
        run.tribe = vec![MateAssignment::default(); 2];
        run.focused_part = 1;
        let mut app = headless_app(run);
        enter(&mut app, GameStage::Tribal);
        app.world_mut().insert_resource(crate::Originals::default());

        // Two mates: focus 1 -> 0 -> 1, wrapping on the real length of 2.
        tap(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<RunProgress>().focused_part, 0);
        tap(&mut app, KeyCode::Tab);
        assert_eq!(app.world().resource::<RunProgress>().focused_part, 1);
        // Role cycling mutates only the persisted assignment for the focused
        // entry; `tribal_cleanup` re-keys the matching `TribeRole` from the
        // roster, so no live component is written here and the count is fixed.
        let before = app.world().resource::<RunProgress>().tribe[0].role;
        tap(&mut app, crate::tribal_controls::KEY_ROLE_PREV);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.tribe.len(), 2, "role edits must not change the count");
        assert_ne!(run.tribe[1].role, before, "focused mate 1 changed role");
    }

    fn mate_ids(app: &mut App) -> Vec<u32> {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&crate::persistence::WorldId, With<TribeMate>>();
        let mut ids: Vec<u32> = q.iter(world).map(|w| w.0).collect();
        ids.sort_unstable();
        ids
    }

    fn kill_mate(app: &mut App, world_id: u32) {
        let world = app.world_mut();
        let mut q =
            world.query_filtered::<(&mut Health, &crate::persistence::WorldId), With<TribeMate>>();
        for (mut health, id) in q.iter_mut(world) {
            if id.0 == world_id {
                health.hp = 0.0;
            }
        }
    }

    #[test]
    fn a_lost_mate_frees_its_stable_slot_for_the_next_recruit() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);
        assert_eq!(mate_ids(&mut app), vec![200, 201, 202]);
        assert_eq!(campaign(&app).slots, vec![0, 1, 2]);
        {
            let world = app.world_mut();
            {
                let mut run = world.resource_mut::<RunProgress>();
                run.tribal_campaign.member_energy.insert(0, 90.0);
                run.tribal_campaign.member_energy.insert(1, 35.0);
                run.tribal_campaign.member_energy.insert(2, 60.0);
            }
            let mut mates = world.query_filtered::<(
                &mut crate::tribal_rts::TribeUnit,
                &crate::persistence::WorldId,
            ), With<TribeMate>>();
            for (mut unit, id) in mates.iter_mut(world) {
                unit.energy = match id.0 {
                    200 => 90.0,
                    201 => 35.0,
                    202 => 60.0,
                    _ => unit.energy,
                };
            }
        }

        // The member under slot 1 dies; cleanup despawns it, releases the id
        // and drops the assignment sitting beside it.
        kill_mate(&mut app, MATE_ID_BASE + 1);
        step(&mut app);
        assert_eq!(mate_ids(&mut app), vec![200, 202]);
        assert_eq!(campaign(&app).slots, vec![0, 2]);
        // Hunger now falls on its own (0.333 a second), so compare loosely.
        let energy_near = |app: &App, slot: u32, wanted: f32| {
            campaign(app)
                .member_energy
                .get(&slot)
                .is_some_and(|e| (e - wanted).abs() < 0.5)
        };
        assert!(energy_near(&app, 0, 90.0));
        assert_eq!(
            campaign(&app).member_energy.get(&1),
            None,
            "death clears only that stable slot"
        );
        assert!(
            energy_near(&app, 2, 60.0),
            "swap-remove preserves survivor energy by slot"
        );
        assert_eq!(
            app.world().resource::<RunProgress>().tribe.len(),
            2,
            "a death must not leave a ghost assignment behind"
        );

        // Paid recruitment queues an egg and reserves, but does not claim, the
        // freed id until the growth timer finishes.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 200.0;
        teleport(&mut app, home_position());
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(
            mate_ids(&mut app),
            vec![200, 202],
            "a paid egg is not selectable before it grows"
        );
        assert_eq!(
            app.world().resource::<RunProgress>().tribe.len(),
            2,
            "pending eggs are not adult assignments"
        );
        assert_eq!(campaign(&app).births.len(), 1);
        assert_eq!(campaign(&app).births[0].slot, 1);
        assert_eq!(
            app.world().resource::<RunProgress>().dna_this_stage,
            200.0 - RECRUIT_FOOD,
            "starting an egg costs food immediately"
        );

        // The roster round-trips through a save and reloads into the same
        // members with the same ids, all inside the restorable range.
        let saved =
            serde_json::to_string(app.world().resource::<RunProgress>()).expect("serialize run");
        let loaded: RunProgress = serde_json::from_str(&saved).expect("deserialize run");
        assert_eq!(loaded.tribal_campaign.slots, vec![0, 2]);
        assert_eq!(loaded.tribal_campaign.births.len(), 1);
        assert!((loaded.tribal_campaign.member_energy[&0] - 90.0).abs() < 0.5);
        assert!((loaded.tribal_campaign.member_energy[&2] - 60.0).abs() < 0.5);
        let mut resumed = headless_app(loaded);
        enter(&mut resumed, GameStage::Tribal);
        assert_eq!(mate_ids(&mut resumed), vec![200, 202]);
        for _ in 0..((BIRTH_GROWTH_SECONDS * 60.0).ceil() as usize + 5) {
            step(&mut resumed);
        }
        assert_eq!(mate_ids(&mut resumed), vec![200, 201, 202]);
        assert_eq!(resumed.world().resource::<RunProgress>().tribe.len(), 3);
        assert!(resumed
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .births
            .is_empty());
        assert_eq!(
            resumed
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .slots,
            vec![0, 2, 1]
        );
        assert!(mate_ids(&mut resumed)
            .iter()
            .all(|id| (MATE_ID_BASE..MATE_ID_BASE + MAX_MATES as u32).contains(id)));
    }

    #[test]
    fn resolve_consequence_powers_by_prior_stage_history() {
        // Cell stage diet mappings:
        let (cell_carn, _) = resolve_consequence_powers(100.0, 10.0, 0, 0);
        assert_eq!(cell_carn, TribalPowerKind::Traps);

        let (cell_herb, _) = resolve_consequence_powers(10.0, 100.0, 0, 0);
        assert_eq!(cell_herb, TribalPowerKind::RefreshingStorm);

        let (cell_omni, _) = resolve_consequence_powers(50.0, 50.0, 0, 0);
        assert_eq!(cell_omni, TribalPowerKind::FlyingFish);

        // Creature stage social vs predator mappings:
        let (_, creature_pred) = resolve_consequence_powers(50.0, 50.0, 1, 4);
        assert_eq!(creature_pred, TribalPowerKind::FireBombs);

        let (_, creature_soc) = resolve_consequence_powers(50.0, 50.0, 4, 1);
        assert_eq!(creature_soc, TribalPowerKind::Fireworks);

        let (_, creature_adapt) = resolve_consequence_powers(50.0, 50.0, 2, 2);
        assert_eq!(creature_adapt, TribalPowerKind::Beastmaster);
    }

    #[test]
    fn consequence_powers_activate_and_cooldown() {
        use crate::tribal_rules as rules;
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.cell_power = Some(TribalPowerKind::Traps);
        c.creature_power = Some(TribalPowerKind::FireBombs);

        let chief_pos = Vec3::new(0.0, 0.0, 0.0);

        // Traps: one trap at the chieftain, armed for `LifeTime` 25 s.
        let ev = c.activate_cell_power(chief_pos).expect("activates trap");
        assert!(ev.message.contains("trap"));
        assert_eq!(c.active_traps.len(), 1);
        assert_eq!(c.active_traps[0].position, [0.0, 0.0, 0.0]);
        assert_eq!(c.active_traps[0].timer, rules::TRAP_LIFETIME);
        assert_eq!(c.power_cell_cooldown, 300.0, "creatureAbilityRefresh 300");

        // Reject activation during cooldown:
        assert!(c.activate_cell_power(chief_pos).is_err());

        // The trap runs out and the cooldown ticks down:
        c.tick(26.0);
        assert!(c.active_traps.is_empty());
        c.tick(300.0);
        assert_eq!(c.power_cell_cooldown, 0.0);

        // Refreshing Storm: fills the bushes within reach (20, scaled) of the
        // chieftain; no food appears out of thin air.
        c.cell_power = Some(TribalPowerKind::RefreshingStorm);
        let ev = c.activate_cell_power(chief_pos).expect("activates storm");
        assert_eq!(ev.food_delta, 0.0);
        assert_eq!(
            c.pending_power_effects,
            vec![PowerEffect::Storm {
                at: chief_pos,
                radius: 20.0 * rules::RANGE_SCALE
            }]
        );
        c.pending_power_effects.clear();

        // Flying Fish: seven fish at 1.5 food each.
        c.power_cell_cooldown = 0.0;
        c.cell_power = Some(TribalPowerKind::FlyingFish);
        let ev = c.activate_cell_power(chief_pos).expect("activates fish");
        assert!((ev.food_delta - 10.5).abs() < 1e-4);

        // Fire Bombs on village 0: 15 damage to the buildings and a blast of 15
        // around the village for its defenders; refresh 240 s.
        let ev = c.activate_creature_power(chief_pos, Some(0)).expect("activates bombs");
        assert!(ev.message.contains("Fire Bombs"));
        assert_eq!(c.villages[0].hurt, 15.0);
        assert_eq!(c.power_creature_cooldown, 240.0);
        assert!(matches!(
            c.pending_power_effects[..],
            [PowerEffect::Blast { damage, .. }] if damage == 15.0
        ));

        // Fireworks: one TribeSocialPower event (+10) for the tribes within
        // reach of the chieftain, nobody else.
        c.power_creature_cooldown = 0.0;
        c.creature_power = Some(TribalPowerKind::Fireworks);
        let at_v2 = village_position(1);
        let ev = c.activate_creature_power(at_v2, None).expect("activates fireworks");
        assert!(ev.message.contains("Fireworks"));
        assert_eq!(c.villages[1].relations, 10.0);
        assert_eq!(c.villages[3].relations, 0.0, "too far to see them");
        assert_eq!(c.power_creature_cooldown, 300.0);

        // Beastmaster: charms for 120 s, refresh 360 s.
        c.power_creature_cooldown = 0.0;
        c.creature_power = Some(TribalPowerKind::Beastmaster);
        c.activate_creature_power(chief_pos, None).expect("activates beastmaster");
        assert_eq!(c.charmed_creatures[0].timer, rules::BEASTMASTER_DURATION);
        assert_eq!(c.power_creature_cooldown, 360.0);
    }

    #[test]
    fn superpowers_carry_the_retail_names_refresh_and_locale_ids() {
        use TribalPowerKind::*;
        let expect = [
            (Traps, "Traps", 300.0, 0x0002_0025, 52),
            (RefreshingStorm, "Refreshing Storm", 300.0, 0x0001_0027, 53),
            (FlyingFish, "Flying Fish", 300.0, 0x0001_0029, 54),
            (FireBombs, "Fire Bombs", 240.0, 0x0001_002B, 55),
            (Fireworks, "Fireworks", 300.0, 0x0001_002D, 56),
            (Beastmaster, "Beastmaster", 360.0, 0x0001_002F, 57),
        ];
        for (power, name, refresh, name_id, ability) in expect {
            assert_eq!(power.name(), name);
            assert_eq!(power.cooldown(), refresh);
            assert_eq!(power.name_text(), (0x9A5A_CF10, name_id));
            assert_eq!(power.description_text().1, name_id + 1);
            assert_eq!(power.superpower().ability_type, ability);
            assert!(!power.description().is_empty());
        }
        // The Cell cards give the first three, the Creature cards the others.
        assert_eq!(Traps.superpower().consequence, "trg_clg_meat");
        assert_eq!(RefreshingStorm.superpower().consequence, "trg_clg_plant");
        assert_eq!(FlyingFish.superpower().consequence, "trg_clg_mix");
        assert_eq!(FireBombs.superpower().consequence, "trg_crg_attack");
        assert_eq!(Fireworks.superpower().consequence, "trg_crg_social");
        assert_eq!(Beastmaster.superpower().consequence, "trg_crg_mix");
    }

    #[test]
    fn the_food_store_holds_tribe_max_food_and_the_hut_reports_its_health() {
        let mut run = RunProgress::default();
        run.award_dna(20_000.0, &GameStage::Tribal);
        assert_eq!(run.dna_this_stage, crate::tribal_rules::TRIBE_MAX_FOOD);
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert_eq!((c.home_hut_level(), c.home_hut_health()), (1, 400.0));
        c.villages[0].status = RivalStatus::Conquered;
        assert_eq!((c.home_hut_level(), c.home_hut_health()), (2, 750.0));
        assert!((c.progress_fraction() - 0.2).abs() < 1e-6);
        assert_eq!(c.stance_of(1), Some(crate::tribal_rules::Stance::Neutral));
        c.villages[1].relations = -30.0;
        assert_eq!(c.stance_of(1), Some(crate::tribal_rules::Stance::Angry));
        c.villages[1].relations = 65.0;
        assert_eq!(c.stance_of(1), Some(crate::tribal_rules::Stance::Friendly));
    }

    #[test]
    fn fire_bombs_reach_a_village_from_its_edge_only() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        let reach = INTERACT_RANGE + 15.0 * crate::tribal_rules::RANGE_SCALE;
        let v = village_position(2);
        let toward_home = (-v).normalize();
        assert_eq!(fire_bomb_target(v + toward_home * (reach - 0.5), &c), Some(2));
        assert_eq!(fire_bomb_target(v + toward_home * (reach + 0.5), &c), None);
        c.villages[2].status = RivalStatus::Allied;
        assert_eq!(fire_bomb_target(v, &c), None, "allies are not bombed");
    }

    #[test]
    fn totem_carvings_grow_with_alliances_and_conquests() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert!(c.totem_carvings.is_empty());

        // Conquer village 0 (its racks and hut together hold 800 hit points):
        c.attack(0, 10_000.0);
        assert_eq!(c.villages[0].status, RivalStatus::Conquered);
        assert_eq!(c.totem_carvings, vec![TotemSegment::Conquered]);

        // Ally village 1:
        c.set_relations(1, ALLY_RELATIONS);
        assert_eq!(c.villages[1].status, RivalStatus::Allied);
        assert_eq!(
            c.totem_carvings,
            vec![TotemSegment::Conquered, TotemSegment::Allied]
        );

        // HUD contains totem information and power readout:
        let hud = c.hud();
        assert!(hud.contains("Totem:"));
        assert!(hud.contains("Powers:"));
        assert!(hud.contains("Conquered"));
        assert!(hud.contains("Allied"));
    }

    // --- retail rules (tribal_rules): T1 flow, T2 tools, T3 relations -----

    /// Every tribe in view, with its scenery, after the stage was entered.
    fn reveal_all(app: &mut App) {
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .reveal_all();
        step(app);
        step(app);
    }

    #[test]
    fn the_tribe_level_and_the_hut_follow_one_and_three_tribes() {
        assert_eq!(
            (0..=5).map(hut_level).collect::<Vec<_>>(),
            vec![1, 2, 2, 3, 3, 3]
        );
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert_eq!(c.population_cap(), 6, "level 1 hut");
        c.villages[0].status = RivalStatus::Allied;
        assert_eq!(c.population_cap(), 9, "the first tribe raises the hut to level 2");
        c.villages[1].status = RivalStatus::Allied;
        assert_eq!(c.population_cap(), 9);
        c.villages[2].status = RivalStatus::Conquered;
        assert_eq!(c.population_cap(), 12, "three tribes make it level 3");
        // The level-up line is added to what the village just announced.
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.say("V1 joins the dance - now ALLIED!");
        c.villages[0].status = RivalStatus::Allied;
        announce_level_up(&mut c, 0);
        assert!(c.message.contains("V1 joins the dance"), "{}", c.message);
        assert!(c.message.contains("level 2 hut (750 health)"), "{}", c.message);
        c.say("later");
        announce_level_up(&mut c, 1);
        assert_eq!(c.message, "later", "level 2 stays level 2");
    }

    #[test]
    fn a_new_campaign_shows_no_tribe_and_keeps_the_lineup_relationships() {
        use crate::creature_rules::Difficulty;
        let mut c = TribalCampaign::default();
        c.ensure_ready_seeded(7, Difficulty::Medium);
        assert_eq!(c.villages.len(), RIVAL_VILLAGES);
        assert_eq!(c.appeared_count(), crate::tribal_rules::TRIBES_AT_START);
        for i in 0..RIVAL_VILLAGES {
            assert!(c.village(i).is_none(), "V{} has not appeared", i + 1);
            assert!(c.village_raw(i).is_some());
        }
        // One tribe of each tier, each at the relationship its lineup gives.
        for i in 0..RIVAL_VILLAGES {
            let (tribe, relation) =
                crate::tribal_rules::lineup_tribe(c.lineup, i, Difficulty::Medium);
            assert_eq!(tribe.tier as usize, i + 1);
            assert_eq!(c.villages[i].relations, relation);
            assert_eq!(c.tribe_of(i).archetype, tribe.archetype);
        }
        // A campaign that already has its villages keeps them.
        let lineup = c.lineup;
        c.ensure_ready_seeded(99_999, Difficulty::Hard);
        assert_eq!(c.lineup, lineup);
        // Nothing can talk to, attack or hear a tribe that has not appeared.
        assert_eq!(c.gift(1, 100.0), TribalEvent::default());
        assert!(c.start_performance(1).is_none());
        assert_eq!(c.attack(1, 10_000.0), TribalEvent::default());
        assert_eq!(c.post_event(1, crate::tribal_rules::RelationEvent::Kill), 0.0);
        assert_eq!(nearest_hostile(village_position(1), &c), None);
        assert_eq!(c.unresolved_visible(), 0);
        assert!(c.hud().contains("V2 not yet seen"), "{}", c.hud());
        c.reveal_all();
        assert_eq!(c.appeared_count(), RIVAL_VILLAGES);
        assert!(c.village(1).is_some());
        // Saves from before tribes appeared over time show them all.
        let old: RivalVillageState = serde_json::from_str(r#"{"huts":2}"#).expect("old village");
        assert!(old.appeared);
    }

    #[test]
    fn every_blow_at_a_member_is_one_attack_event() {
        use crate::tribal_rules::RelationEvent::Attack;
        // 0xC26C85: one `TribeAttack` per blow, no cadence of its own. A torch
        // (refresh 1.5 s) fighting for 5 s throws three blows (and a third of one).
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[3].relations = 0.0;
        let mut posted = 0;
        for _ in 0..100 {
            posted += c.post_hostile_strikes(3, Attack, (1.0 / 20.0) / 1.5);
        }
        assert_eq!(posted, 3);
        assert_eq!(c.villages[3].relations, -30.0);
        // Two more fighters, six blows in all: the event stops at its floor.
        for _ in 0..200 {
            c.post_hostile_strikes(3, Attack, 0.1);
        }
        assert_eq!(c.villages[3].relations, -60.0, "a TribeAttack stops at -60");
        assert_eq!(c.post_hostile_strikes(9, Attack, 1.0), 0, "no such village");
        assert_eq!(c.post_hostile_strikes(3, Attack, f32::NAN), 0);
    }

    #[test]
    fn relationship_events_move_relations_with_their_floors() {
        use crate::tribal_rules::RelationEvent::*;
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        assert_eq!(c.post_event(0, Gift), 10.0);
        assert_eq!(c.post_event(0, Social), 20.0);
        assert_eq!(c.post_event(0, Attack), 10.0);
        assert_eq!(c.post_event(0, Kill), -50.0);
        assert_eq!(c.post_event(0, Raid), -60.0, "a raid stops at its floor");
        assert_eq!(c.post_event(0, Attack), -60.0, "an attack cannot go lower than -60");
        assert_eq!(c.post_event(0, StealBaby), -90.0);
        assert_eq!(c.post_event(0, DestroyTool), -100.0);
        assert_eq!(c.post_event(0, Kill), -100.0);
        // Settled tribes no longer react.
        c.villages[1].status = RivalStatus::Allied;
        c.villages[1].relations = ALLY_RELATIONS;
        assert_eq!(c.post_event(1, Kill), ALLY_RELATIONS);
        // Repeated hostile hits are posted at most every few seconds.
        assert!(c.post_hostile_hit(2, Attack));
        assert!(!c.post_hostile_hit(2, Attack));
        c.tick(crate::tribal_rules::HOSTILE_EVENT_INTERVAL);
        assert!(c.post_hostile_hit(2, Attack));
        assert_eq!(c.villages[2].relations, -20.0);
        // A hostile relationship is a state, not a debt: a gift still warms it up.
        assert_eq!(c.post_event(0, Gift), -90.0);
    }

    #[test]
    fn sieges_post_attack_and_destroy_events() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.villages[0].relations = 30.0;
        c.attack(0, 50.0);
        assert_eq!(c.villages[0].relations, 20.0, "a blow at the village is -10");
        c.attack(0, 50.0);
        assert_eq!(c.villages[0].relations, 20.0, "...but not every frame");
        // A rack falling is a destroyed tool: -100.
        c.attack(0, 200.0);
        assert_eq!(c.villages[0].relations, -80.0);
    }

    #[test]
    fn a_rack_hands_out_six_tools() {
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        c.sync_roster(&mut vec![MateAssignment::default(); MAX_MATES]);
        c.shacks.push(ToolKind::FishingSpears);
        for slot in 0..crate::tribal_rules::MAX_TOOLS_PER_RACK as u32 {
            assert!(c.equip(MATE_ID_BASE + slot, ToolKind::FishingSpears).is_ok());
        }
        let seventh = c.equip(MATE_ID_BASE + 6, ToolKind::FishingSpears);
        assert!(seventh.is_err(), "the rack holds six tools");
        assert!(seventh.unwrap_err().contains("only holds 6"));
        // A holder may take its own tool again.
        assert!(c.equip(MATE_ID_BASE, ToolKind::FishingSpears).is_ok());
        // A member who falls frees its tool for the next one.
        c.release_slot(MATE_ID_BASE + 5);
        assert!(c.equip(MATE_ID_BASE + 6, ToolKind::FishingSpears).is_ok());
    }

    #[test]
    fn the_city_hall_card_follows_how_the_tribes_were_won() {
        use crate::tribal_rules::PlayStyle;
        let mut c = TribalCampaign::default();
        c.ensure_ready();
        for v in &mut c.villages {
            v.status = RivalStatus::Allied;
            v.relations = ALLY_RELATIONS;
        }
        assert_eq!(c.play_style_card(), PlayStyle::Religious);
        for v in &mut c.villages {
            v.status = RivalStatus::Conquered;
            v.relations = -100.0;
        }
        assert_eq!(c.play_style_card(), PlayStyle::Military);
        for (i, v) in c.villages.iter_mut().enumerate() {
            let allied = i < 3;
            v.status = if allied { RivalStatus::Allied } else { RivalStatus::Conquered };
            v.relations = if allied { ALLY_RELATIONS } else { -100.0 };
        }
        assert_eq!(c.play_style_card(), PlayStyle::Economic);
        assert!(c.can_advance());
    }

    #[test]
    fn a_new_run_starts_with_fifteen_food_and_a_tougher_bigger_chieftain() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);
        assert_eq!(food(&app), crate::tribal_rules::NEW_TRIBE_FOOD);
        assert_eq!(mate_ids(&mut app).len(), crate::tribal_rules::TRIBE_POPULATION_START);
        let world = app.world_mut();
        let chief_health = world
            .query_filtered::<&Health, With<Player>>()
            .single(world)
            .expect("one chieftain")
            .max;
        let member_health = world
            .query_filtered::<&Health, With<TribeMate>>()
            .iter(world)
            .next()
            .expect("a member")
            .max;
        assert!(
            (chief_health / member_health - crate::tribal_rules::CHIEFTAIN_HEALTH_SCALE).abs() < 1e-3,
            "{chief_health} vs {member_health}"
        );
        let chief_scale = world
            .query_filtered::<&Transform, With<Player>>()
            .single(world)
            .expect("one chieftain")
            .scale
            .x;
        let member_scale = world
            .query_filtered::<&Transform, With<TribeMate>>()
            .iter(world)
            .next()
            .expect("a member")
            .scale
            .x;
        // Members are drawn at the body size, the chieftain 1.25 times that.
        assert!(
            chief_scale > member_scale * 1.2 && chief_scale < member_scale * 1.3,
            "{chief_scale} vs {member_scale}"
        );
        // No tribe is in view yet: the first one appears after the start.
        let world = app.world_mut();
        let villages = world.query::<&RivalVillage>().iter(world).count();
        assert_eq!(villages, 0);
    }

    #[test]
    fn new_tribes_appear_by_the_retail_controller_rules() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);
        // The stage starts with the player's tribe alone.
        assert_eq!(campaign(&app).appeared_count(), 0);
        let world = app.world_mut();
        assert_eq!(world.query::<&RivalVillage>().iter(world).count(), 0);
        // 15 food (the start) never brings a tribe by itself.
        for _ in 0..(60 * 40) {
            step(&mut app);
        }
        assert_eq!(campaign(&app).appeared_count(), 0, "no tutorial flag, no timer");
        // Reaching 40 food starts the 25 s timer; the first tribe walks into view.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 40.0;
        for _ in 0..(20 * 60) {
            step(&mut app);
        }
        assert_eq!(campaign(&app).appeared_count(), 0, "25 s have not passed");
        for _ in 0..(10 * 60) {
            step(&mut app);
            if campaign(&app).appeared_count() > 0 {
                break;
            }
        }
        assert_eq!(campaign(&app).appeared_count(), 1);
        assert!(campaign(&app).message.contains("new tribe has appeared") || campaign(&app).village(0).is_some());
        // A neutral tribe sends no raiders, so the stock stays put.
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .villages[0]
            .relations = crate::tribal_rules::RAID_RELATION_LIMIT;
        step(&mut app);
        let world = app.world_mut();
        let roots: Vec<usize> = world.query::<&RivalVillage>().iter(world).map(|r| r.0).collect();
        assert!(roots.contains(&0), "V1 got its scenery: {roots:?}");
        let members = world
            .query::<&crate::tribal_rts::RivalMember>()
            .iter(world)
            .filter(|m| m.village == 0)
            .count();
        assert_eq!(members, crate::tribal_rules::village_tribe(campaign(&app).lineup, 0).initial_population);
        // The second tribe needs a settled tribe or more than 100 food.
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 100.0;
        for _ in 0..(5 * 60) {
            step(&mut app);
        }
        assert_eq!(campaign(&app).appeared_count(), 1, "100 food is not above 100");
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 0.0;
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[0].status = RivalStatus::Conquered;
        for _ in 0..(2 * 60) {
            step(&mut app);
            if campaign(&app).appeared_count() > 1 {
                break;
            }
        }
        assert_eq!(campaign(&app).appeared_count(), 2, "a settled tribe brings the second");
        // The third and fourth follow a second apart; the fifth waits for three
        // settled tribes.
        for _ in 0..(4 * 60) {
            step(&mut app);
        }
        assert_eq!(campaign(&app).appeared_count(), 4);
        for _ in 0..(5 * 60) {
            step(&mut app);
        }
        assert_eq!(campaign(&app).appeared_count(), 4, "one settled tribe is not enough");
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            for i in 1..3 {
                run.tribal_campaign.villages[i].status = RivalStatus::Conquered;
            }
        }
        for _ in 0..(2 * 60) {
            step(&mut app);
            if campaign(&app).appeared_count() > 4 {
                break;
            }
        }
        assert_eq!(campaign(&app).appeared_count(), RIVAL_VILLAGES);
    }

    #[test]
    fn a_fallen_chieftain_returns_after_ninety_seconds_with_full_health() {
        let mut app = headless_app(RunProgress::default());
        enter(&mut app, GameStage::Tribal);
        let max = {
            let world = app.world_mut();
            let mut chief = world.query_filtered::<&mut Health, With<Player>>();
            let mut health = chief.single_mut(world).expect("one chieftain");
            let max = health.max;
            health.hp = 0.0;
            max
        };
        step(&mut app);
        assert!(
            (campaign(&app).chieftain_respawn - crate::tribal_rules::CHIEFTAIN_RESPAWN_TIME).abs() < 1.0,
            "the timer starts at 90 s: {}",
            campaign(&app).chieftain_respawn
        );
        let hidden = |app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Visibility, With<Player>>();
            *q.single(world).expect("one chieftain") == Visibility::Hidden
        };
        assert!(hidden(&mut app), "a fallen chieftain is out of sight");
        for _ in 0..(80 * 60) {
            step(&mut app);
        }
        assert!(campaign(&app).chieftain_respawn > 5.0, "still waiting at 80 s");
        assert_eq!(player_hp(&mut app), 0.0);
        // The timer runs out around 90 s and he returns healthy (checked on
        // that frame: raiders from the rival villages, 200 units away, may be
        // fighting at the hut by then).
        for _ in 0..(12 * 60) {
            step(&mut app);
            if campaign(&app).chieftain_respawn == 0.0 {
                break;
            }
        }
        assert_eq!(campaign(&app).chieftain_respawn, 0.0);
        let hp = player_hp(&mut app);
        assert!((hp - max).abs() < 1e-3, "full health: {hp} of {max}");
        assert!(!hidden(&mut app));
    }
}
