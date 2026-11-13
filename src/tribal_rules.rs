//! Retail Tribal-stage rules in one place: the original's tables and formulas
//! as named constants and pure functions that the Tribal modules call.
//!
//! Source: `notes\tribal-re-notes.md` sections T0-T3 (reverse-engineered by
//! `cell-re` from the original executable; addresses are the ModAPI march2017
//! (Steam) addresses, decompilations in `notes\tribal-re\*.c`, the dumps
//! `TribeTuning*.txt`, `tribe_tools.txt`, `tribe_abilities_table.txt`,
//! `tribe_archetypes.txt`). "VERIFIED" means the number or formula was read in
//! the executable code; "tuning" means the value comes from an installed
//! property record whose use was named but not traced. Whatever the executable
//! does not give us is marked `ADAPTED:` so it can be replaced later.
//!
//! Sections: T1 stage flow (progress, tribe level, hut health, start state,
//! chieftain, new tribes, the play-style card), T2 core rules (tools, weapons
//! with their raze damage, food, hunger, eating, members, healing, the six
//! consequence superpowers), T3 tribe interactions (relationship events,
//! stances, the ten NPC tribe archetypes and the nine five-tribe lineups, the
//! epic creature), T4 ecology (distance scales, placement markers, the herd
//! archetypes, herd casting) and the Bevy plugin with the systems the rules
//! need (new tribes appear, the chieftain respawns, superpower effects).
// Many named retail values are kept for the passes that will use them (HUD,
// cinematics, ecology) and are only exercised by the tests.
#![allow(dead_code)]

use crate::creature_rules::Difficulty;
use crate::stages::GameStage;
use crate::tribal_tools::ToolKind;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Pick the value for a difficulty (easy, medium, hard).
fn pick<T: Copy>(difficulty: Difficulty, easy: T, medium: T, hard: T) -> T {
    match difficulty {
        Difficulty::Easy => easy,
        Difficulty::Medium => medium,
        Difficulty::Hard => hard,
    }
}

// ================================================================= T1 stage flow

/// `numTribesToConquer` (tuning): tribes to ally or conquer; the progress bar
/// is `min(progress, goal) / goal` and the stage is complete at 1.0 (0xCD8C36).
pub const NUM_TRIBES_TO_CONQUER: usize = 5;
/// `goalProgressUpgrades` (tuning, 0xCDA4F0): the progress values at which the
/// tribe level rises (1 and 3 tribes allied or conquered).
pub const GOAL_PROGRESS_UPGRADES: [u32; 2] = [1, 3];
/// `tribeHutLevel1/2/3Health` (tuning; the same three values sit under three
/// unnamed props): hit points of a hut by tribe level 1..=3.
pub const HUT_HEALTH: [f32; 3] = [400.0, 750.0, 1000.0];
/// `tribePopulationStart` (tuning): members the tribe starts with.
pub const TRIBE_POPULATION_START: usize = 3;
/// `newTribeFood` (tuning): food the tribe starts with.
pub const NEW_TRIBE_FOOD: f32 = 15.0;
/// `tribeMaxFood` (tuning): cap of the food stock.
pub const TRIBE_MAX_FOOD: f32 = 9999.0;
/// `tribeStartingTools` (tuning): tool types 3 (spear) and 4 (horn). Whether
/// they start owned or only unlocked is not traced, so OpenSpore keeps its
/// own unlock tiers (`tribal_tools::TOOLS`).
pub const TRIBE_STARTING_TOOL_TYPES: [u8; 2] = [3, 4];

/// Tribe level 0..=2 from the stage progress (tribes allied or conquered),
/// VERIFIED (0xCDA4F0): the first `i` with `progress < goalProgressUpgrades[i]`,
/// level 2 when no entry is larger. With `[1, 3]`: 0 tribes -> 0, 1-2 -> 1, 3+ -> 2.
pub fn tribe_level(progress: usize) -> u8 {
    for (level, need) in GOAL_PROGRESS_UPGRADES.iter().enumerate() {
        if (progress as u32) < *need {
            return level as u8;
        }
    }
    2
}

/// Hut level 1..=3 (the look `he_canvas_1..3`, `trg_totem_pole1..3`, ...).
pub fn hut_level(progress: usize) -> usize {
    tribe_level(progress) as usize + 1
}

/// Hit points of a hut of `level` (clamped to 1..=3).
pub fn hut_health(level: usize) -> f32 {
    HUT_HEALTH[level.clamp(1, 3) - 1]
}

/// The new hut level when the progress moves from `before` to `after` crosses
/// an upgrade threshold (`TRG_Upgrade` plays and the hut look changes).
pub fn level_reached(before: usize, after: usize) -> Option<usize> {
    (hut_level(after) > hut_level(before)).then(|| hut_level(after))
}

/// Progress-bar fraction (0xCD8C36): `min(progress, goal) / goal`.
pub fn progress_fraction(progress: usize) -> f32 {
    progress.min(NUM_TRIBES_TO_CONQUER) as f32 / NUM_TRIBES_TO_CONQUER as f32
}

/// The stage is complete (the Evolve button appears) at fraction 1.0.
pub fn stage_complete(progress: usize) -> bool {
    progress >= NUM_TRIBES_TO_CONQUER
}

/// Food the stage starts with (0xCD4400: the tribe is created with
/// `newTribeFood`); every other stage starts with none.
pub fn start_food(stage: &GameStage) -> f32 {
    if *stage == GameStage::Tribal {
        NEW_TRIBE_FOOD
    } else {
        0.0
    }
}

/// Members a hut houses by tribe level. VERIFIED (0xC8EC70, the max population
/// of `cTribe`): a player-owned tribe returns 0xC00C10(`mCommunitySize`, the
/// tribe level +0x38), the `community` prop's `CommunityMax_*` table 6, 9, 12
/// (15, 30, 45, 60 follow for levels the stage never reaches); an NPC tribe
/// reads its archetype's `maxpopulation` (the same 6 / 9 / 12 for tiers
/// 1 / 2-4 / 5).
pub const POPULATION_CAP_BY_LEVEL: [usize; 3] = [6, 9, 12];

/// Members the home hut houses at this stage progress.
pub fn population_cap(progress: usize) -> usize {
    POPULATION_CAP_BY_LEVEL[tribe_level(progress) as usize]
}

// ---- chieftain

/// `chieftainScale` (tuning): the chieftain's body is 1.25x a member's.
pub const CHIEFTAIN_SCALE: f32 = 1.25;
/// `chieftainHealthScale` (tuning): 1.5x a member's hit points.
pub const CHIEFTAIN_HEALTH_SCALE: f32 = 1.5;
/// `chieftainDamageScale` (tuning): damage multiplier of the chieftain's staff.
pub const CHIEFTAIN_DAMAGE_SCALE: f32 = 1.0;
/// `chieftainRespawnTime` (tuning, cTribe +0x2C8): seconds before a fallen
/// chieftain returns at the hut.
pub const CHIEFTAIN_RESPAWN_TIME: f32 = 90.0;
/// `chieftainDefendDistanceMin/Max` (tuning): the chieftain defends the tribe
/// within 5..20 units.
pub const CHIEFTAIN_DEFEND_DISTANCES: (f32, f32) = (5.0, 20.0);

/// Chieftain hit points from a member's.
pub fn chieftain_health(member_health: f32) -> f32 {
    member_health * CHIEFTAIN_HEALTH_SCALE
}

/// Chieftain body scale from a member's.
pub fn chieftain_body_scale(member_scale: f32) -> f32 {
    member_scale * CHIEFTAIN_SCALE
}

/// One respawn tick: `(seconds left, true on the frame the chieftain returns)`.
/// A timer at zero is "not waiting".
pub fn chieftain_respawn_step(remaining: f32, dt: f32) -> (f32, bool) {
    if !remaining.is_finite() || remaining <= 0.0 || !dt.is_finite() {
        return (0.0, false);
    }
    let left = remaining - dt.max(0.0);
    if left <= 0.0 {
        (0.0, true)
    } else {
        (left, false)
    }
}

// ---- new NPC tribes over time (cNpcTribeController update 0xCC6A90, VERIFIED)

/// The NPC tribes come into view one at a time while the stage runs. The
/// controller (vtable 0x1476878; slot 4 = 0xCC6A90 updates, slot 9 = 0xCC6440
/// creates the next tribe from the next lineup entry at the nearest marker)
/// decides by how many tribes exist, the player's included (jump table
/// 0xCC70BC):
///
/// | NPC tribes in view | the next one appears |
/// | --- | --- |
/// | 0 | `first_tribe_baby_delay` (90 s, hard 60; prop 0x3D346A77) after the first baby, or `FIRST_TRIBE_FOOD_DELAY` (25 s; prop 0x1A09D79E, absent from the data) after the stock first reached `FOOD_TUTORIAL_FOOD`; with Settings > Tutorials off both timers start at once |
/// | 1 | a tribe has been allied or conquered, or the stock is above `SECOND_TRIBE_FOOD` (prop 0x4C87E711, default 100) |
/// | 2, 3 | at once (the controller waits one second after each creation) |
/// | 4 | at least `FIFTH_TRIBE_RESOLVED` tribes allied or conquered |
///
/// The tuning names `npcTribeAppearanceCheckTime`, `tribeTimeToNewTribe` and
/// `_foodForNewTribesAppear` are never read by the code (their ids occur
/// nowhere in the image), so the earlier 60 s check / 100-70 food reading of
/// them was wrong. The first creation also plays `TRG_PRE_FirstTribeAppears`
/// (0xCC686F) and shows the `TribeTutorial_Conquer` hint.
pub const TRIBES_AT_START: usize = 0;

/// The first NPC tribe after the first baby (property 0x3D346A77: 90 s, hard 60 s).
pub fn first_tribe_baby_delay(difficulty: Difficulty) -> f32 {
    pick(difficulty, 90.0, 90.0, 60.0)
}

/// The first NPC tribe after the tool-buying food level was reached (code
/// default of property 0x1A09D79E; the data has no entry).
pub const FIRST_TRIBE_FOOD_DELAY: f32 = 25.0;

/// Food of the player's tribe that sets the tutorial flag 0x14363974 (0xC91B00;
/// property 0xB79E0065, default 40).
pub const FOOD_TUTORIAL_FOOD: f32 = 40.0;

/// The second NPC tribe comes when the stock is above this (0xCC6D00; property
/// 0x4C87E711, default 100).
pub const SECOND_TRIBE_FOOD: f32 = 100.0;

/// The fifth NPC tribe waits for this many allied or conquered tribes (0xCC6D3A).
pub const FIFTH_TRIBE_RESOLVED: usize = 3;

/// Seconds the controller waits after creating a tribe (the global at
/// 0x157E940 is reset to 1.0 by the creation).
pub const APPEARANCE_SPACING: f32 = 1.0;

/// What the controller looks at each update.
#[derive(Debug, Clone, Copy)]
pub struct AppearanceInput {
    /// NPC tribes in view.
    pub npc_tribes: usize,
    /// Tribes allied or conquered (the length of the totem history, tribe game
    /// object + 0x40).
    pub resolved: usize,
    /// The player's tribe food.
    pub food: f32,
    /// The player has a baby (the Mating tutorial flag 0x11ADE9CE is set by the
    /// first baby, 0xCD7647).
    pub baby: bool,
    /// Settings > Tutorials (config option 0x4EA96CB).
    pub tutorials: bool,
    pub difficulty: Difficulty,
}

/// The controller's timers and the tutorial flags it reads (persisted).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct AppearanceState {
    /// Seconds since the baby timer (controller + 0x10) started.
    #[serde(default)]
    pub baby_timer: Option<f32>,
    /// Seconds since the food timer (controller + 0x30) started.
    #[serde(default)]
    pub food_timer: Option<f32>,
    /// The Mating tutorial flag (the first baby came).
    #[serde(default)]
    pub baby_seen: bool,
    /// The food tutorial flag (the stock reached `FOOD_TUTORIAL_FOOD`).
    #[serde(default)]
    pub food_seen: bool,
    /// Seconds before the next tribe may be created.
    #[serde(default)]
    pub spacing: f32,
}

impl AppearanceState {
    /// Advance by `dt` game seconds; true when the next NPC tribe is created now.
    pub fn step(&mut self, dt: f32, i: &AppearanceInput) -> bool {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.food_seen |= i.food >= FOOD_TUTORIAL_FOOD;
        self.baby_seen |= i.baby;
        for timer in [&mut self.baby_timer, &mut self.food_timer].into_iter().flatten() {
            *timer += dt;
        }
        self.spacing = (self.spacing - dt).max(0.0);
        if i.npc_tribes >= NUM_TRIBES_TO_CONQUER {
            return false;
        }
        let due = match i.npc_tribes {
            0 => {
                if (self.baby_seen || !i.tutorials) && self.baby_timer.is_none() {
                    self.baby_timer = Some(0.0);
                }
                if (self.food_seen || !i.tutorials) && self.food_timer.is_none() {
                    self.food_timer = Some(0.0);
                }
                self.baby_timer.is_some_and(|t| t > first_tribe_baby_delay(i.difficulty))
                    || self.food_timer.is_some_and(|t| t > FIRST_TRIBE_FOOD_DELAY)
            }
            1 => i.resolved >= 1 || i.food > SECOND_TRIBE_FOOD,
            2 | 3 => true,
            _ => i.resolved >= FIFTH_TRIBE_RESOLVED,
        };
        if due && self.spacing <= 0.0 {
            self.spacing = APPEARANCE_SPACING;
            return true;
        }
        false
    }
}

// ---- the city-hall play-style card

/// The Tribal play-style card shown at the stage end (`TRG2CVG_ShowCityHall`,
/// 0xCDD0F0; chosen by 0xCDBBD0 from the score of 0xCDBA30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayStyle {
    /// "MILI" (`trg_attack`).
    Military,
    /// "ECON" (`trg_mix`).
    Economic,
    /// "RELI" (`trg_social`).
    Religious,
}

impl PlayStyle {
    /// The card text the city hall shows.
    pub fn card(self) -> &'static str {
        match self {
            PlayStyle::Military => "MILI",
            PlayStyle::Economic => "ECON",
            PlayStyle::Religious => "RELI",
        }
    }

    /// The id 0xCDBBD0 returns: FNV-1 of `trg_attack`, `trg_mix`, `trg_social`.
    pub fn card_id(self) -> u32 {
        match self {
            PlayStyle::Military => 0x0CAC_124D,
            PlayStyle::Economic => 0xC2CA_9495,
            PlayStyle::Religious => 0x60A7_8928,
        }
    }

    /// The consequence trait of the Evolve click (0xCDBEA0): FNV-1 of
    /// `trg_earnedtraitattack`, `trg_earnedtraitmix`, `trg_earnedtraitsocial`.
    pub fn trait_id(self) -> u32 {
        match self {
            PlayStyle::Military => 0x6A9F_2620,
            PlayStyle::Economic => 0x8133_FB2E,
            PlayStyle::Religious => 0x2CFA_39DD,
        }
    }
}

/// VERIFIED (0xCDD0F0 / 0xCDBBD0): the card switches at +-1/3 of the score.
pub const PLAY_STYLE_THRESHOLD: f32 = 0.333_333_34;
/// The relationship range the attitude reads: 0xD00BD0 clamps the raw value to
/// -10..10 before it cuts it (VERIFIED); this crate keeps relationships in the
/// event table's x10 units, so the bound is +-100.
pub const RELATIONSHIP_BOUND: f32 = 100.0;
/// The bound of the play-style score (0xCDBA30, VERIFIED): each relationship is
/// clamped between the relationship manager's +0x10 (`RolloverRltnshpVBadCap`
/// -7) and +0x1C (`RolloverRltnshpGoodCap` 7) and the sum is divided by
/// `numTribesToConquer` x that cap (+0x1C), so the bound is the attitude
/// boundary, +-7 raw = +-70 in this crate's x10 units (not the +-10 clamp of the
/// attitude).
pub const PLAY_STYLE_RELATION_BOUND: f32 = 70.0;

/// The score of 0xCDBA30: `-sum(clamp(relationship, -bound, bound)) /
/// (numTribesToConquer x bound)` over the NPC tribes with `bound`
/// [`PLAY_STYLE_RELATION_BOUND`], clamped to [-1, 1]. Warm relations give a
/// negative score (the social card), hostile ones a positive one (the attack
/// card). The original adds one more unnamed modifier (the UI timeline value
/// 0xAD56080C, 0xE3F7A0) that is not traced and is left out.
pub fn play_style_score(relations: &[f32]) -> f32 {
    let sum: f32 = relations
        .iter()
        .map(|r| {
            if r.is_finite() {
                r.clamp(-PLAY_STYLE_RELATION_BOUND, PLAY_STYLE_RELATION_BOUND)
            } else {
                0.0
            }
        })
        .sum();
    (-sum / (NUM_TRIBES_TO_CONQUER as f32 * PLAY_STYLE_RELATION_BOUND)).clamp(-1.0, 1.0)
}

/// The card for a score (0xCDBBD0): `>= 1/3` attack, `>= -1/3` mix, else social.
pub fn play_style(score: f32) -> PlayStyle {
    if score >= PLAY_STYLE_THRESHOLD {
        PlayStyle::Military
    } else if score >= -PLAY_STYLE_THRESHOLD {
        PlayStyle::Economic
    } else {
        PlayStyle::Religious
    }
}

/// The card from the tribe's final relationships.
pub fn play_style_from_relations(relations: &[f32]) -> PlayStyle {
    play_style(play_style_score(relations))
}

/// Cinematic names of the stage flow, kept for the later cinematic work.
pub mod cinematics {
    /// 0xCD4400: stage start.
    pub const BEGIN: &str = "TRG_Begin";
    /// 0xCC686F: the first NPC tribe comes into view.
    pub const FIRST_TRIBE_APPEARS: &str = "TRG_PRE_FirstTribeAppears";
    /// 0xCDA4F0: the tribe level rises (new hut look).
    pub const UPGRADE: &str = "TRG_Upgrade";
    /// 0xCDAC90: after a tribe is allied or conquered.
    pub const TOTEM_POLE: &str = "TRG_TotemPole";
    pub const TOTEM_POLE_CONQUER: &str = "TRG_TotemPole_Conquer";
    pub const TOTEM_POLE_SOCIAL: &str = "TRG_TotemPole_Social";
    /// 0xCDBEA0: the Evolve button.
    pub const PRE_EDITOR: &str = "TRG2CVG_PreEditor";
    /// 0xCDD0F0: the city hall with the play-style card.
    pub const SHOW_CITY_HALL: &str = "TRG2CVG_ShowCityHall";
    /// 0xCFBD70: the Civilization stage starts.
    pub const CIV_START: &str = "TRG2CVG_CivStart";
}

// ======================================================================= T2 tools

/// `tribeToolHealth` of the attack, social and economy tools (tuning).
pub const TOOL_HEALTH: f32 = 200.0;
/// `defaulttool` health, also the camp fire's (it sets none of its own).
pub const DEFAULT_TOOL_HEALTH: f32 = 100.0;
/// `defaulttool` price.
pub const DEFAULT_TOOL_PRICE: f32 = 10.0;
/// `toolData_MaxNumSlots` (`defaulttool`): tools one rack hands out.
pub const MAX_TOOLS_PER_RACK: usize = 6;
/// `tribeToolHiDamage` / `tribeToolLoDamage` (and the same two for huts): the
/// health fractions below which the heavy / light damage model shows.
pub const TOOL_HI_DAMAGE: f32 = 0.33;
pub const TOOL_LO_DAMAGE: f32 = 0.66;
/// `tribeRepairDelay` (seconds untouched) and `tribeRepairRatePercent`.
pub const REPAIR_DELAY: f32 = 20.0;
pub const REPAIR_RATE_PERCENT: f32 = 0.5;

/// `toolData_ToolType` of a tool.
pub fn tool_type(kind: ToolKind) -> u8 {
    match kind {
        ToolKind::SpikedClubs => 1,
        ToolKind::StoneAxes => 2,
        ToolKind::ThrowingSpears => 3,
        ToolKind::WoodenHorns => 4,
        ToolKind::Maracas => 5,
        ToolKind::Aerophones => 6,
        ToolKind::FruitSticks => 7,
        ToolKind::FishingSpears => 8,
        ToolKind::HealingRods => 9,
        ToolKind::Campfire => 10,
    }
}

/// `modelPrice` in food: attack and social 25, gather / fish / heal 20, camp
/// fire 15 (`tribe_tools~`).
pub fn tool_price(kind: ToolKind) -> f32 {
    match kind {
        ToolKind::SpikedClubs
        | ToolKind::StoneAxes
        | ToolKind::ThrowingSpears
        | ToolKind::WoodenHorns
        | ToolKind::Maracas
        | ToolKind::Aerophones => 25.0,
        ToolKind::FruitSticks | ToolKind::FishingSpears | ToolKind::HealingRods => 20.0,
        ToolKind::Campfire => 15.0,
    }
}

/// `tribeToolHealth` of a tool rack.
pub fn tool_health(kind: ToolKind) -> f32 {
    if kind == ToolKind::Campfire {
        DEFAULT_TOOL_HEALTH
    } else {
        TOOL_HEALTH
    }
}

/// Which damage model a tool or hut shows at a health fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageModel {
    Intact,
    Light,
    Heavy,
}

pub fn damage_model(health_fraction: f32) -> DamageModel {
    if health_fraction <= TOOL_HI_DAMAGE {
        DamageModel::Heavy
    } else if health_fraction <= TOOL_LO_DAMAGE {
        DamageModel::Light
    } else {
        DamageModel::Intact
    }
}

/// A rack hands out a tool only while fewer than `MAX_TOOLS_PER_RACK` members
/// already carry that tool.
pub fn rack_has_room(holders: usize) -> bool {
    holders < MAX_TOOLS_PER_RACK
}

// ---- weapons (tribe_abilities_table)

/// A weapon's special attack (`tribe_hunt*_special`): an area hit with a long
/// refresh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Special {
    pub damage: f32,
    pub ranged_damage: f32,
    /// `creatureAbilityRazeDamage`: damage per hit to huts and tool racks.
    pub raze: f32,
    pub refresh: f32,
    pub range: f32,
    /// Cone length and radius of the area hit.
    pub cone_dist: f32,
    pub cone_radius: f32,
}

/// One tool ability: melee damage per hit, ranged damage (spear), seconds
/// between hits and the reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weapon {
    /// The ability `Type` of the table.
    pub ability: u32,
    pub damage: f32,
    pub ranged_damage: f32,
    /// `creatureAbilityRazeDamage` (decoded from the `tribeabilities` props by
    /// tribal-rules2): damage per hit to buildings (huts and tool racks).
    pub raze: f32,
    pub refresh: f32,
    pub range: f32,
    pub special: Option<Special>,
}

/// `tribe_hunt1` (torch / Spiked Clubs) with `tribe_hunt1_special`.
pub const TORCH: Weapon = Weapon {
    ability: 58,
    damage: 6.0,
    ranged_damage: 0.0,
    raze: 9.0,
    refresh: 1.5,
    range: 1.0,
    special: Some(Special {
        damage: 3.0,
        ranged_damage: 0.0,
        raze: 9.0,
        refresh: 12.0,
        range: 5.0,
        cone_dist: 2.0,
        cone_radius: 2.3,
    }),
};
/// `tribe_hunt2` (axe / Stone Axes) with `tribe_hunt2_special`.
pub const AXE: Weapon = Weapon {
    ability: 59,
    damage: 9.0,
    ranged_damage: 0.0,
    raze: 5.0,
    refresh: 2.0,
    range: 1.0,
    special: Some(Special {
        damage: 6.75,
        ranged_damage: 0.0,
        raze: 5.0,
        refresh: 12.0,
        range: 1.0,
        cone_dist: 1.0,
        cone_radius: 3.5,
    }),
};
/// `tribe_hunt3` (spear / Throwing Spears) with `tribe_hunt3_special` (a rush
/// of 30 units).
pub const SPEAR: Weapon = Weapon {
    ability: 43,
    damage: 5.0,
    ranged_damage: 10.0,
    raze: 4.0,
    refresh: 3.0,
    range: 25.0,
    special: Some(Special {
        damage: 3.75,
        ranged_damage: 10.0,
        raze: 4.0,
        refresh: 12.0,
        range: 1.0,
        cone_dist: 0.0,
        cone_radius: 0.0,
    }),
};
/// `tribe_chieftain_staff`.
pub const CHIEFTAIN_STAFF: Weapon = Weapon {
    ability: 42,
    damage: 12.0,
    ranged_damage: 0.0,
    raze: 7.0,
    refresh: 2.0,
    range: 1.0,
    special: None,
};
/// `tribe_shaman_staff` (Healing Rods).
pub const SHAMAN_STAFF: Weapon = Weapon {
    ability: 42,
    damage: 3.0,
    ranged_damage: 0.0,
    raze: 3.0,
    refresh: 2.5,
    range: 1.0,
    special: None,
};
/// ADAPTED: a member without an attack tool. The abilities of the other tools
/// carry no damage and `tribe_normaloutfit` has none, so retail members only
/// fight with a tool; OpenSpore lets them defend themselves and batter a hut
/// weakly (one point per `timeRangeBetweenAttacks`, 1.5 s).
pub const UNARMED: Weapon = Weapon {
    ability: 0,
    damage: 1.0,
    ranged_damage: 0.0,
    raze: 1.0,
    refresh: 1.5,
    range: 1.0,
    special: None,
};
/// `timeRangeBetweenAttacks` (tuning): seconds between a member's attacks.
pub const TIME_BETWEEN_ATTACKS: f32 = 1.5;
/// `attackWaitDistance` (tuning).
pub const ATTACK_WAIT_DISTANCE: f32 = 10.0;

// ---- citizen health (tribal-rules3; notes T8A)

/// The brain level a citizen reports (VERIFIED 0xC248B0, `cCreatureCitizen`
/// vtable slot 0xD8): 5, or 6 when the citizen field +0x1010 is set (not traced;
/// both levels read 100 HP).
pub const CITIZEN_BRAIN_LEVEL: i32 = 5;

/// Maximum hit points of a tribe member, rival tribe member or NPC tribe
/// citizen (VERIFIED: the citizen overrides the creature's base-HP slot 0x7C
/// with 0xC22880 = `creatureBaseHealth[brain level]` (speciestuning
/// 0x236F18F2: Level5 = 100) plus its species' `mAdditionalHealth` (+0x56C),
/// which 0x4D5080 sets to min(health parts x `creatureHealthIncrement` 2,
/// `creatureMaxHealth` 10) + two more cap sums; `GetMaxHitPoints` 0xC05D50
/// leaves an adult citizen at that value). Every tribe's members - the
/// player's and the NPC tribes' - use their own species' parts; the herd
/// `HitpointOverride` and the 10 HP level-0 base of wild creatures do not apply
/// to citizens.
pub fn citizen_max_hp(health_level: u8) -> f32 {
    crate::creature_rules::base_health(CITIZEN_BRAIN_LEVEL)
        + crate::creature_rules::parts_health_bonus(health_level)
}

/// ADAPTED: the health parts of an NPC tribe's species. OpenSpore's rival
/// species have no part list, so their members get the base 100 only (the
/// retail range is 100..110 plus rare extra caps).
pub const NPC_CITIZEN_HEALTH_LEVEL: u8 = 0;

/// Hit points of an NPC tribe's member (see [`citizen_max_hp`]).
pub fn npc_citizen_max_hp() -> f32 {
    citizen_max_hp(NPC_CITIZEN_HEALTH_LEVEL)
}

/// `nerfMultForBabies` (speciestuning 0x11074696, read into 0x150C888 by the
/// species tuning loader 0x4D24B0): `GetMaxHitPoints` (0xC05D50) multiplies a
/// baby's hit points by it. The installed value is 0.5 (the image's static
/// default is 0.25). Tribal babies are eggs in OpenSpore, so only the rule is
/// kept here.
pub const BABY_HIT_POINT_MULTIPLIER: f32 = 0.5;

// ---- self defence (tribal-rules3; notes T8B)

/// How far a member chases the creature that hurt it before it goes back to
/// its work: INFERRED from `attackWaitDistance` 10 (TakeDamage 0xBFCF10 keeps
/// the last attacker at `cCombatant` +0x28; the citizen's reaction itself is
/// not traced).
pub const RETALIATE_LEASH: f32 = ATTACK_WAIT_DISTANCE;

// ---- raids (tribal-rules3; notes T8C)

/// Food one raider carries off: a full carried bundle. INFERRED from the raid
/// action (0xDBCE4D..0xDBCEC3 tops the raider's bundle up to its capacity from
/// the raided tribe's stock) with `carryBundleAmountMax` 10.
pub const RAID_LOAD: f32 = CARRY_BUNDLE_AMOUNT_MAX;

/// Seconds a raided tribe stays alerted after the first raider reaches its food
/// (VERIFIED: the raid action calls 0xC8F600 on the raided tribe with flag
/// 0x4000 and 5.0 s; its effect - members turn on the raiders - INFERRED).
pub const RAID_ALERT_SECONDS: f32 = 5.0;

/// ADAPTED: an NPC tribe's stock grows by the archetype template's `FoodRate`
/// (0.1, read as food per second) back up to its `initialfood`. Retail's
/// income (the "food upkeep" behaviour tick 0xCE3CC0, decoded in full in
/// `notes	ribal-rows2-notes.md`) is different and NOT ported: every 2 s, while
/// a plan runs and `food <= plan desiredFood + the saved price of the next
/// wanted tool`, an accumulator gains `workers x max(plan FoodRate, archetype
/// FoodRate 0x1BBD28CF) x dt` (the code takes the larger of the two, not the
/// smaller) and is credited to the stock in whole lumps once it passes 1.0;
/// OpenSpore's NPC tribes run no gather / fish plans and buy no tools, so the
/// plan activity that gates it is not modelled.
pub const NPC_FOOD_RATE: f32 = 0.1;

/// Food an NPC tribe's stock has after `seconds` of regrowth from `food`.
pub fn npc_food_regrow(food: f32, initial: f32, seconds: f32) -> f32 {
    if food >= initial {
        food
    } else {
        (food + NPC_FOOD_RATE * seconds.max(0.0)).min(initial)
    }
}

/// What one raider takes from a stock of `stock` food: a bundle, or what is
/// left.
pub fn raid_take(stock: f32) -> f32 {
    stock.clamp(0.0, RAID_LOAD)
}

impl Weapon {
    /// Melee damage per second.
    pub fn melee_dps(&self) -> f32 {
        self.damage / self.refresh
    }

    /// Ranged damage per second (0 without a ranged attack).
    pub fn ranged_dps(&self) -> f32 {
        self.ranged_damage / self.refresh
    }

    /// Damage per second to buildings (`creatureAbilityRazeDamage` per hit):
    /// torch 9 / 1.5 s, axe 5 / 2 s, spear 4 / 3 s, chieftain staff 7 / 2 s,
    /// shaman staff 3 / 2.5 s.
    pub fn raze_dps(&self) -> f32 {
        self.raze / self.refresh
    }

    /// Damage per second at `distance`: melee while inside `melee_reach`, the
    /// ranged attack beyond it when the weapon has one.
    pub fn dps_at(&self, distance: f32, melee_reach: f32) -> f32 {
        if distance > melee_reach && self.ranged_damage > 0.0 {
            self.ranged_dps()
        } else {
            self.melee_dps()
        }
    }
}

/// The weapon a tool gives its carrier: the attack tools, the shaman staff for
/// the Healing Rods, otherwise bare hands.
pub fn weapon_of(tool: Option<ToolKind>) -> Weapon {
    match tool {
        Some(ToolKind::SpikedClubs) => TORCH,
        Some(ToolKind::StoneAxes) => AXE,
        Some(ToolKind::ThrowingSpears) => SPEAR,
        Some(ToolKind::HealingRods) => SHAMAN_STAFF,
        _ => UNARMED,
    }
}

/// Damage per second of the chieftain's staff (the chieftain scale applies).
pub fn chieftain_dps() -> f32 {
    CHIEFTAIN_STAFF.melee_dps() * CHIEFTAIN_DAMAGE_SCALE
}

/// Damage per second the chieftain's staff deals to buildings.
pub fn chieftain_raze_dps() -> f32 {
    CHIEFTAIN_STAFF.raze_dps() * CHIEFTAIN_DAMAGE_SCALE
}

/// Timer of a special attack: `(timer, true when the special fires)`. The
/// special refreshes every `refresh` seconds while the unit fights.
pub fn special_step(timer: f32, dt: f32, refresh: f32) -> (f32, bool) {
    if !dt.is_finite() || dt <= 0.0 {
        return (timer, false);
    }
    let timer = timer - dt;
    if timer <= 0.0 {
        (refresh, true)
    } else {
        (timer, false)
    }
}

// ======================================================================= T2 food

/// `gatherFoodPerFruit` (tuning): food in one fruit.
pub const GATHER_FOOD_PER_FRUIT: f32 = 1.0;
/// `gatherTreeSmackYield` (tuning) is a table keyed by the plant's size class:
/// the loader 0xCE7D2E fills the map at 0x15812A4 with 0x6D60A1CC (small
/// plants) -> 3, 0x029C388A (medium) -> 5, 0x3A8BE428 (large) -> 0 (the keys
/// are the planet's flora size classes, `notes\planet-flora-notes.md`), and
/// the gather behaviour 0xD75F70 (state 0xC, at the hit event of the smack
/// clip) looks the yield up by the tree's class (`tree +0x58`). VERIFIED; the
/// old reading "entry 0 by hand, entry 1 with the gather tool" was wrong.
pub const TREE_SMACK_YIELD: [f32; 3] = [3.0, 5.0, 0.0];
/// ADAPTED: the size class of OpenSpore's fruit bushes (a bush stands for a
/// fruit tree; which class the retail fruit trees are was not decoded): small.
pub const BUSH_PLANT_CLASS: usize = 0;
/// Food one unit of yield is worth when a tree is smacked: tuning 0x98FBEF73
/// (2.0, unnamed; 0xD75F70 state 0xC: `min(0x98FBEF73 x yield, room in the
/// bundle)` is added to the carried bundle).
pub const SMACK_FOOD_PER_YIELD: f32 = 2.0;
/// Length of `gathr_treesmacker` (TLSA 0x02DF4B12, ANIM 0x3F84D7A2: 55 frames
/// at 30 per second): one smack cycle with the gather tool (0xD75F70 state 2
/// plays it, state 0xC credits the fruit, state 0xE returns to state 0).
pub const SMACK_CLIP_SECONDS: f32 = 55.0 / 30.0;
/// `chief_gathr_treesmacker` (0x0579081C, ANIM 0x71002BD8): the chieftain's.
pub const CHIEF_SMACK_CLIP_SECONDS: f32 = 66.0 / 30.0;
/// `gathr_picktree` (0x02DF4B0A, ANIM 0x6AF668D3): the pick by hand
/// (0xD75F70 states 3..6: one fruit worth `gatherFoodPerFruit` per clip).
pub const PICK_CLIP_SECONDS: f32 = 49.0 / 30.0;
/// ADAPTED: the fruit is credited when the clip ends (retail credits it at an
/// event inside the clip, whose time was not read).
pub fn gather_cycle_seconds(fruit_sticks: bool, chieftain: bool) -> f32 {
    match (fruit_sticks, chieftain) {
        (true, true) => CHIEF_SMACK_CLIP_SECONDS,
        (true, false) => SMACK_CLIP_SECONDS,
        (false, _) => PICK_CLIP_SECONDS,
    }
}
/// Food one gather cycle brings with `room` left in the bundle: a smack gives
/// `2.0 x yield`, a pick by hand one fruit.
pub fn gather_cycle_food(fruit_sticks: bool, room: f32) -> f32 {
    let amount = if fruit_sticks {
        SMACK_FOOD_PER_YIELD * TREE_SMACK_YIELD[BUSH_PLANT_CLASS]
    } else {
        GATHER_FOOD_PER_FRUIT
    };
    amount.min(room.max(0.0))
}
/// `fruitPropMaxRegrowthTime` (tuning): seconds a fruit takes to regrow.
pub const FRUIT_REGROWTH_TIME: f32 = 25.0;
/// ADAPTED: fruit a bush carries (the original has fruit props on trees; their
/// count is not traced). Two bundles of five.
pub const FRUITS_PER_BUSH: f32 = 10.0;
/// `gatherFindTreeRadius` (70) and `maxForagingDistance` (150) (tuning): how
/// far from the tribe gatherers look for the next tree and may forage.
pub const GATHER_FIND_TREE_RADIUS: f32 = 70.0;
pub const MAX_FORAGING_DISTANCE: f32 = 150.0;
/// The next-tree search radius around the food mat (`gatherFindTreeRadius`).
pub fn forage_radius() -> f32 {
    GATHER_FIND_TREE_RADIUS * RANGE_SCALE
}
/// Below this a bush counts as bare and the gatherer moves on.
pub const BARE_BUSH: f32 = 1.0;
/// `fishingFoodPerFish` (tuning).
pub const FISHING_FOOD_PER_FISH: f32 = 1.5;
/// One catch of a fisher (0xD73FD0, states 8..0xD): `fish_idle_start`, a loop
/// of random length, `fish_spear` (the pull), a random `fish_succ_N` and
/// `fish_dump`, then the start again. The clips are installed data (ANIM
/// lengths, 30 frames per second; the chieftain's `chief_fish_*` clips are as
/// long).
pub const FISH_START_SECONDS: f32 = 16.0 / 30.0;
/// The loop's length is `random(tuning 0x069943CD, tuning 0xBCA45F07)`
/// (0xD73FD0 state 9). The installed `TribeTuning` sets them to 0.5 and 1.0;
/// the code's own defaults (1.9 / 3.9) apply only when the properties are
/// absent.
pub const FISH_LOOP_SECONDS: (f32, f32) = (0.5, 1.0);
/// `fish_spear` (0x02C3921B): the pull (ADAPTED: played in full; retail
/// switches to the catch clip at an event inside it).
pub const FISH_PULL_SECONDS: f32 = 121.0 / 30.0;
/// `fish_succ_1..3` (0x02C39220..22).
pub const FISH_SUCC_SECONDS: [f32; 3] = [61.0 / 30.0, 75.0 / 30.0, 92.0 / 30.0];
/// `fish_dump` (0x02C39224): the catch goes on the bundle (retail credits it
/// at an event inside the clip; ADAPTED here: when the dump starts).
pub const FISH_DUMP_SECONDS: f32 = 72.0 / 30.0;

/// The rolled parts of one catch cycle (`roll_loop`, `roll_succ` in 0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishCycle {
    pub loop_seconds: f32,
    pub succ: usize,
}

impl FishCycle {
    /// A cycle from two uniform rolls: the loop length and the `fish_succ`
    /// pick (`random(3)`).
    pub fn roll(roll_loop: f32, roll_succ: f32) -> Self {
        let r = roll_loop.clamp(0.0, 1.0);
        Self {
            loop_seconds: FISH_LOOP_SECONDS.0 + (FISH_LOOP_SECONDS.1 - FISH_LOOP_SECONDS.0) * r,
            succ: ((roll_succ.clamp(0.0, 0.999_9) * 3.0) as usize).min(2),
        }
    }

    /// Cycle time at which the pull starts.
    pub fn pull_at(&self) -> f32 {
        FISH_START_SECONDS + self.loop_seconds
    }

    /// Cycle time at which the pull ends and the catch clip starts.
    pub fn catch_at(&self) -> f32 {
        self.pull_at() + FISH_PULL_SECONDS
    }

    /// Cycle time at which the dump starts and the fish is credited.
    pub fn credit_at(&self) -> f32 {
        self.catch_at() + FISH_SUCC_SECONDS[self.succ]
    }

    /// Length of the whole cycle.
    pub fn end(&self) -> f32 {
        self.credit_at() + FISH_DUMP_SECONDS
    }
}

impl Default for FishCycle {
    fn default() -> Self {
        Self::roll(0.5, 0.0)
    }
}
/// `carryBundleAmountMax` (tuning): the most a member carries at once.
pub const CARRY_BUNDLE_AMOUNT_MAX: f32 = 10.0;
/// `giftFoodAmount` (tuning): food of one gift to a tribe.
pub const GIFT_FOOD_AMOUNT: f32 = 10.0;
/// What a member carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Haul {
    Fruit,
    Fish,
    Meat,
    Eggs,
}

/// `carryBundleAmountMax_Fruit/_Fish/_Meat/_Eggs` (tuning): items in one bundle.
pub fn bundle_items(haul: Haul) -> f32 {
    match haul {
        Haul::Fruit | Haul::Fish | Haul::Meat | Haul::Eggs => 5.0,
    }
}

/// Food of one item: a fish is worth 1.5, everything else 1.
pub fn food_per_item(haul: Haul) -> f32 {
    match haul {
        Haul::Fish => FISHING_FOOD_PER_FISH,
        _ => GATHER_FOOD_PER_FRUIT,
    }
}

/// Food in a full bundle (never above `carryBundleAmountMax`).
pub fn bundle_food(haul: Haul) -> f32 {
    (bundle_items(haul) * food_per_item(haul)).min(CARRY_BUNDLE_AMOUNT_MAX)
}

/// Food of a carcass: ADAPTED mapping of the two wild creature kinds to the
/// herd archetypes of T4 (large prey 15, large predator 20).
pub fn carcass_food(hostile: bool) -> f32 {
    if hostile {
        20.0
    } else {
        15.0
    }
}

// ----- hunger and eating

/// Hunger and health are full at 100 (`MotiveMax`).
pub const HUNGER_MAX: f32 = 100.0;
/// `eatHungerBoostPerFood` (tuning): hunger of one food.
pub const EAT_HUNGER_BOOST_PER_FOOD: f32 = 100.0;
/// `eatHealthBoostPerFood` (tuning): health points of one food.
pub const EAT_HEALTH_BOOST_PER_FOOD: f32 = 25.0;
/// `hungerDecayDelay` (MotiveTuning, Tribe): seconds hunger is held at the
/// maximum before it falls.
pub const HUNGER_DECAY_DELAY: f32 = 120.0;
/// `creatureStarvationPain` of the tribe's motive tuning: health lost per second
/// at zero hunger.
pub const STARVATION_PAIN: f32 = 0.5;

/// `TribeHungerDrain` (MotiveTuning, Tribe): hunger lost per second (easy
/// 0.278, hard 0.417).
pub fn tribe_hunger_drain(difficulty: Difficulty) -> f32 {
    pick(difficulty, 0.278, 0.333, 0.417)
}

/// One hunger tick (the Creature stage's 0xC0D5A0 stopwatch): held at the
/// maximum for `HUNGER_DECAY_DELAY`, then drained.
pub fn hunger_step(hunger: f32, hold: f32, dt: f32, difficulty: Difficulty) -> (f32, f32) {
    crate::creature_rules::hunger_tick(
        hunger,
        HUNGER_MAX,
        hold,
        dt,
        tribe_hunger_drain(difficulty),
        HUNGER_DECAY_DELAY,
    )
}

/// Health lost to starvation this tick (hunger below 1).
pub fn starvation_damage(hunger: f32, dt: f32) -> f32 {
    if hunger < 1.0 && dt.is_finite() && dt > 0.0 {
        STARVATION_PAIN * dt
    } else {
        0.0
    }
}

/// Eating one food: `(hunger, health)` after, both capped at their maximum.
pub fn eat_food(hunger: f32, health: f32, health_max: f32) -> (f32, f32) {
    (
        (hunger + EAT_HUNGER_BOOST_PER_FOOD).min(HUNGER_MAX),
        (health + EAT_HEALTH_BOOST_PER_FOOD).min(health_max),
    )
}

// ====================================================================== T2 members

/// Food a new member costs. VERIFIED: the buy click (0xCDBEA0, control 0x0625CD52)
/// reads the global 0x158133C, which the difficulty tuning loader 0xCE6C70 fills
/// from the Tribe tuning property 0xE8C2BCAE (+0x134 of the struct at 0x1581208;
/// 0 when the property is absent): 10.0 in `TribeTuning`, inherited by the easy
/// and hard lists.
pub const MEMBER_PRICE: f32 = 10.0;
/// `timeBeforeBabyGrowsUp` (tuning): seconds an egg takes to become a member.
pub const TIME_BEFORE_BABY_GROWS_UP: f32 = 60.0;
/// `babyGrowthBoost` (tuning, name only).
pub const BABY_GROWTH_BOOST: f32 = 30.0;
/// `matingTime` (tuning): seconds of mating before an egg (NPC tribes).
pub const MATING_TIME: f32 = 5.0;
/// `maxNumDomesticatedAnimals` (tuning).
pub const MAX_DOMESTICATED_ANIMALS: usize = 3;
/// `healPercentPerSecondWithTool` / `...NoTool` (tuning): percent of the
/// maximum health a shaman restores per second.
pub const HEAL_PERCENT_PER_SECOND_WITH_TOOL: f32 = 5.0;
pub const HEAL_PERCENT_PER_SECOND_NO_TOOL: f32 = 5.0;
/// `shamanHealingDistances` (tuning): the healer works from 1 to 5 units away.
pub const SHAMAN_HEALING_DISTANCES: (f32, f32) = (1.0, 5.0);
/// `shamanFindInjuredDistance` (25) and `shamanHealingMaxAngle` (135 degrees).
pub const SHAMAN_FIND_INJURED_DISTANCE: f32 = 25.0;
pub const SHAMAN_HEALING_MAX_ANGLE: f32 = 135.0;
/// `healthToolRegenRate` (tuning, name only).
pub const HEALTH_TOOL_REGEN_RATE: f32 = 1.0;

/// Health a shaman restores per second to a member with `max_health`.
pub fn heal_per_second(max_health: f32, with_tool: bool) -> f32 {
    max_health
        * if with_tool {
            HEAL_PERCENT_PER_SECOND_WITH_TOOL
        } else {
            HEAL_PERCENT_PER_SECOND_NO_TOOL
        }
        / 100.0
}

/// Whether a member `distance` away is inside a shaman's healing reach.
pub fn in_healing_reach(distance: f32) -> bool {
    distance <= SHAMAN_HEALING_DISTANCES.1
}

/// A new member can be bought: enough food and room in the hut.
pub fn can_buy_member(food: f32, members_and_eggs: usize, progress: usize) -> bool {
    food >= MEMBER_PRICE && members_and_eggs < population_cap(progress)
}

// ================================================================ T2 superpowers

/// Locale table of the consequence superpower names and descriptions.
pub const SUPERPOWER_LOCALE_TABLE: u32 = 0x9A5A_CF10;

/// One Tribal consequence superpower: the chieftain's ability earned with the
/// Cell card (`consequence_superpowers~!trg_clg_meat / _plant / _mix`) or the
/// Creature card (`trg_crg_attack / _social / _mix`). Ability props are
/// `tribeabilities~!superpower_*` (parent `tribe_verbs~!tribesuperpower`, decoded
/// by tribal-rules2); the names are the en-us strings of locale table
/// 0x9A5ACF10 (the HUD can look them up by `name_id` / `description_id`), the
/// descriptions here are short summaries of the localized ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Superpower {
    /// The consequence superpower record.
    pub consequence: &'static str,
    /// The ability prop.
    pub ability: &'static str,
    /// `creatureAbilityType` (ModAPI `kTRG_SuperPower_*`, 52..=57).
    pub ability_type: u32,
    /// Ids in [`SUPERPOWER_LOCALE_TABLE`].
    pub name_id: u32,
    pub description_id: u32,
    /// The en-us name.
    pub name: &'static str,
    pub description: &'static str,
    /// `creatureAbilityRefresh`: seconds before it can be used again.
    pub refresh: f32,
    /// `creatureAbilityRange` in retail units (1 is the template default).
    pub range: f32,
    /// `creatureAbilityDamage` (0 when the prop sets none).
    pub damage: f32,
}

/// `superpower_carnivore` (Cell carnivore card).
pub const SUPERPOWER_TRAPS: Superpower = Superpower {
    consequence: "trg_clg_meat",
    ability: "superpower_carnivore",
    ability_type: 52,
    name_id: 0x0002_0025,
    description_id: 0x0002_0026,
    name: "Traps",
    description: "The chieftain lays traps that ensnare wild animals.",
    refresh: 300.0,
    range: 1.0,
    damage: 0.0,
};
/// `superpower_herbivore` (Cell herbivore card).
pub const SUPERPOWER_REFRESHING_STORM: Superpower = Superpower {
    consequence: "trg_clg_plant",
    ability: "superpower_herbivore",
    ability_type: 53,
    name_id: 0x0001_0027,
    description_id: 0x0001_0028,
    name: "Refreshing Storm",
    description: "The chieftain calls a storm that regrows fruit and shakes it from the trees.",
    refresh: 300.0,
    range: 20.0,
    damage: 0.0,
};
/// `superpower_omnivore` (Cell omnivore card).
pub const SUPERPOWER_FLYING_FISH: Superpower = Superpower {
    consequence: "trg_clg_mix",
    ability: "superpower_omnivore",
    ability_type: 54,
    name_id: 0x0001_0029,
    description_id: 0x0001_002A,
    name: "Flying Fish",
    description: "The chieftain calls a sea serpent that frightens fish out of the water.",
    refresh: 300.0,
    range: 1.0,
    damage: 0.0,
};
/// `superpower_aggressive` (Creature attack card).
pub const SUPERPOWER_FIRE_BOMBS: Superpower = Superpower {
    consequence: "trg_crg_attack",
    ability: "superpower_aggressive",
    ability_type: 55,
    name_id: 0x0001_002B,
    description_id: 0x0001_002C,
    name: "Fire Bombs",
    description: "The chieftain throws fire bombs that hurt nearby enemies and buildings.",
    refresh: 240.0,
    range: 15.0,
    damage: 15.0,
};
/// `superpower_social` (Creature social card).
pub const SUPERPOWER_FIREWORKS: Superpower = Superpower {
    consequence: "trg_crg_social",
    ability: "superpower_social",
    ability_type: 56,
    name_id: 0x0001_002D,
    description_id: 0x0001_002E,
    name: "Fireworks",
    description: "The chieftain sets off fireworks that improve relations with neighbours.",
    refresh: 300.0,
    range: 25.0,
    damage: 0.0,
};
/// `superpower_versatile` (Creature mix card).
pub const SUPERPOWER_BEASTMASTER: Superpower = Superpower {
    consequence: "trg_crg_mix",
    ability: "superpower_versatile",
    ability_type: 57,
    name_id: 0x0001_002F,
    description_id: 0x0001_0030,
    name: "Beastmaster",
    description: "The chieftain enchants nearby animals to help the tribe socialize or fight.",
    refresh: 360.0,
    range: 50.0,
    damage: 0.0,
};

/// `superpower_carnivore` extra props (names only; their use is not traced):
/// `LifeTime` 25, `TrapRange` 60, `PoisonRange` 7, `triggerTime` 10, `FadeTime`
/// 10, unnamed 0x1070C664 = 4 and 0xBE3CE72D = 15.
pub const TRAP_LIFETIME: f32 = 25.0;
pub const TRAP_RANGE: f32 = 60.0;
pub const TRAP_POISON_RANGE: f32 = 7.0;
pub const TRAP_TRIGGER_TIME: f32 = 10.0;
pub const TRAP_FADE_TIME: f32 = 10.0;
/// ADAPTED reading of the trap props: a trap stays armed `LifeTime` seconds,
/// wild animals within `TrapRange` are lured to it and it springs on whoever
/// comes within `PoisonRange` (both scaled with [`RANGE_SCALE`]).
pub fn trap_lure_radius() -> f32 {
    TRAP_RANGE * RANGE_SCALE
}
pub fn trap_spring_radius() -> f32 {
    TRAP_POISON_RANGE * RANGE_SCALE
}
/// ADAPTED (OpenSpore 0.81): damage a sprung trap does to a raider or to a
/// creature too big to ensnare (an epic).
pub const TRAP_DAMAGE: f32 = 40.0;

/// `superpower_omnivore` `NumFish` 7 (also `searchRadius` 300, `EffectiveDist`
/// 100, unnamed 0x191DCAD8 = 45 and 0xED219018 = 15).
pub const FLYING_FISH_NUM_FISH: f32 = 7.0;
/// Food the fish of one Flying Fish bring: seven fish at `fishingFoodPerFish`.
/// ADAPTED: delivered to the stock instead of landing as fish on the shore.
pub fn flying_fish_food() -> f32 {
    FLYING_FISH_NUM_FISH * FISHING_FOOD_PER_FISH
}

/// `superpower_versatile` unnamed props 0x217997F4 = 5 and 0x83028513 = 120.
/// ADAPTED reading: up to five animals, charmed for 120 s.
pub const BEASTMASTER_MAX_ANIMALS: usize = 5;
pub const BEASTMASTER_DURATION: f32 = 120.0;

/// A superpower's reach in OpenSpore units (`creatureAbilityRange` scaled).
pub fn superpower_reach(power: &Superpower) -> f32 {
    power.range * RANGE_SCALE
}

// ======================================================================== T3 tribes

/// A relationship event between the tribe and an NPC tribe
/// (`space_npc_relationship_effects~!_DefaultPersonality`, ids 0x0530CF00-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationEvent {
    /// Hurting a member (`TribeAttack`).
    Attack,
    /// Killing a member (`TribeKill`).
    Kill,
    /// Stealing food (`TribeRaid`).
    Raid,
    /// Stealing a baby (`TribeStealBaby`).
    StealBaby,
    /// Damaging a tool rack or hut (`TribeAttackToolOrHut`).
    AttackToolOrHut,
    /// Destroying a tool rack (`TribeDestroyTool`).
    DestroyTool,
    /// A food gift (`TribeGift`).
    Gift,
    /// A social tool used for them (`TribeSocial`).
    Social,
    /// A social superpower used on them (`TribeSocialPower`, the Fireworks).
    SocialPower,
}

/// What one event does: the amount added and the limit it pushes towards (a
/// floor for negative events, `None` for the unbounded positive ones).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventEffect {
    pub add: f32,
    pub floor: Option<f32>,
}

impl RelationEvent {
    pub const ALL: [RelationEvent; 9] = [
        RelationEvent::Attack,
        RelationEvent::Kill,
        RelationEvent::Raid,
        RelationEvent::StealBaby,
        RelationEvent::AttackToolOrHut,
        RelationEvent::DestroyTool,
        RelationEvent::Gift,
        RelationEvent::Social,
        RelationEvent::SocialPower,
    ];

    /// The relationship property id (`TribeAttack` 0x0530CF00 ...).
    pub fn id(self) -> u32 {
        match self {
            RelationEvent::Attack => 0x0530_CF00,
            RelationEvent::Kill => 0x0530_CF01,
            RelationEvent::Raid => 0x0530_CF02,
            RelationEvent::StealBaby => 0x0530_CF03,
            RelationEvent::AttackToolOrHut => 0x0530_CF04,
            RelationEvent::DestroyTool => 0x0530_CF05,
            RelationEvent::Gift => 0x0530_CF06,
            RelationEvent::Social => 0x0530_CF07,
            RelationEvent::SocialPower => 0x54EA_B4B3,
        }
    }

    pub fn effect(self) -> EventEffect {
        let (add, floor) = match self {
            RelationEvent::Attack => (-10.0, Some(-60.0)),
            RelationEvent::Kill => (-60.0, Some(-100.0)),
            RelationEvent::Raid => (-30.0, Some(-60.0)),
            RelationEvent::StealBaby => (-30.0, Some(-100.0)),
            RelationEvent::AttackToolOrHut => (-10.0, Some(-100.0)),
            RelationEvent::DestroyTool => (-100.0, Some(-100.0)),
            RelationEvent::Gift => (GIFT_RELATIONSHIP, None),
            RelationEvent::Social => (SOCIAL_RELATIONSHIP_IMPROVEMENT, None),
            RelationEvent::SocialPower => (SOCIAL_POWER_RELATIONSHIP, None),
        };
        EventEffect { add, floor }
    }
}

/// `TribeGift` adds +10 (and the gift is `giftFoodAmount` food).
pub const GIFT_RELATIONSHIP: f32 = 10.0;
/// `socialRelationshipImprovement` (tuning) = `TribeSocial` +10.
pub const SOCIAL_RELATIONSHIP_IMPROVEMENT: f32 = 10.0;
/// `TribeSocialPower` (0x54EAB4B3): +10, unbounded.
pub const SOCIAL_POWER_RELATIONSHIP: f32 = 10.0;
/// `socialNeutralScoreThreshold` (tuning 0xE67DD8C2, 5): the mark on the 0..10
/// bar above which the song's ending is the happy one (0xDA3A03: `bar > global
/// 0x1581320` plays the cheer `0xD478051A` / `0x547462FF`, otherwise `0x14780312`).
/// It does not decide whether the song succeeds (that is the bar being full,
/// [`social_success`]). `socialPositiveScoreThreshold` (0xF2BC93C6, 8) is read
/// by no instruction of the image.
#[allow(dead_code)]
pub const SOCIAL_POSITIVE_SCORE_THRESHOLD: f32 = 8.0;
pub const SOCIAL_NEUTRAL_SCORE_THRESHOLD: f32 = 5.0;
/// The relationship range kept in the campaign: the range the attitude reads
/// (see [`RELATIONSHIP_BOUND`]; the original keeps larger values for gifts, which
/// the attitude cannot tell apart).
pub const RELATION_MIN: f32 = -RELATIONSHIP_BOUND;
pub const RELATION_MAX: f32 = RELATIONSHIP_BOUND;
/// ADAPTED: seconds between two repeated hostile events from one siege (a hit
/// is posted at most this often per village); the original's cadence is not
/// traced.
pub const HOSTILE_EVENT_INTERVAL: f32 = 4.0;

/// Apply one event to a relationship value. A negative event never pushes the
/// value below its floor (a value already below it stays); the event table has
/// no decay for tribe events. The result stays inside the kept range.
pub fn apply_event(value: f32, event: RelationEvent) -> f32 {
    let value = if value.is_finite() { value } else { 0.0 };
    let effect = event.effect();
    let next = match effect.floor {
        Some(floor) if effect.add < 0.0 => {
            if value <= floor {
                value
            } else {
                (value + effect.add).max(floor)
            }
        }
        _ => value + effect.add,
    };
    next.clamp(RELATION_MIN, RELATION_MAX)
}

/// How an NPC tribe stands with the player's tribe: the relationship manager's
/// attitude (VERIFIED 0xD00BD0: the relationship clamped to -10..10 raw is cut
/// at four thresholds, manager +0x10..+0x1C, into Furious 0 / Annoyed 1 /
/// Cautious 2 / Pleased 3 / Friendly 4). `Angry` = Furious or Annoyed,
/// `Neutral` = Cautious, `Friendly` = Pleased, `Allied` = joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    Angry,
    Neutral,
    Friendly,
    Allied,
}

/// The attitude thresholds in the campaign's x10 units. VERIFIED (tribal-final,
/// notes T9K): the relationship manager's loader 0xD00F10 (run by 0xD03C80 from
/// its Initialize 0xD04250) reads the global `ui` property list 0x5C770DB7 into
/// +0x10..+0x1C: `RolloverRltnshpVBadCap` -7, `RolloverRltnshpBadCap` -3,
/// `RolloverRltnshpNeutralCap` 3, `RolloverRltnshpGoodCap` 7 (raw; x10 here).
/// The tribe lineups start tribes at exactly -7 / -3 / 0 (Furious / Annoyed /
/// Cautious under these cuts).
pub const FURIOUS_RELATION_LIMIT: f32 = -70.0;
/// At or below this a tribe is Annoyed (or Furious) and raids the player.
pub const ANNOYED_RELATION_LIMIT: f32 = -30.0;
/// Tribes from this relationship are Pleased (friendly).
pub const FRIENDLY_RELATION_LIMIT: f32 = 30.0;
/// Tribes from this relationship are Friendly: the alliance condition (see
/// [`ALLY_RELATION_THRESHOLD`]).
pub const ALLY_RELATION_THRESHOLD: f32 = 70.0;
/// The lowest relationship at which a hostile tribe does not raid (just above
/// the Annoyed cut; kept for callers that need a peaceful value).
pub const RAID_RELATION_LIMIT: f32 = ANNOYED_RELATION_LIMIT + 1.0;

/// The stance for a relationship value (`allied` once the tribe has joined).
pub fn stance(relations: f32, allied: bool) -> Stance {
    if allied {
        Stance::Allied
    } else if relations >= FRIENDLY_RELATION_LIMIT {
        Stance::Friendly
    } else if relations > ANNOYED_RELATION_LIMIT {
        Stance::Neutral
    } else {
        Stance::Angry
    }
}

/// Whether a hostile tribe at this relationship is Annoyed or Furious (the
/// attitude bands of the NPC raid plans, `tribal_plans`; thresholds VERIFIED,
/// see [`FURIOUS_RELATION_LIMIT`]).
pub fn raids(relations: f32) -> bool {
    stance(relations, false) == Stance::Angry
}

/// Whether a tribe at this relationship joins the player (VERIFIED rule: the
/// social totem routine 0xD9CED0 and the defeat routine 0xC2F7F0 both ask the
/// attitude 0xD00BD0 and treat Friendly (4) as allied; threshold VERIFIED 70,
/// `RolloverRltnshpGoodCap`).
pub fn allies(relations: f32) -> bool {
    relations >= ALLY_RELATION_THRESHOLD
}

/// The attitude of 0xD008E0 (the relationship manager's four caps cut the
/// relationship, here in the campaign's x10 units): 0 Furious, 1 Annoyed, 2
/// Cautious, 3 Pleased, 4 Friendly.
pub fn attitude(relations: f32) -> u8 {
    let r = relations;
    if r > ANNOYED_RELATION_LIMIT && r < FRIENDLY_RELATION_LIMIT {
        2
    } else if r >= FRIENDLY_RELATION_LIMIT && r < ALLY_RELATION_THRESHOLD {
        3
    } else if r >= ALLY_RELATION_THRESHOLD {
        4
    } else if r <= ANNOYED_RELATION_LIMIT && r > FURIOUS_RELATION_LIMIT {
        1
    } else {
        0
    }
}

/// What a `TribeSocial` / `TribeGift` event does to the relationship in the
/// retail code (VERIFIED 0xDA3523..0xDA355E, 0xDAEAB6..0xDAEAFE, 0xDBC1F1..
/// 0xDBC257): the code never posts the event with its own amount, it passes a
/// `scale` to `ApplyRelationship` (0xD06280, "a multiplier over the effect the
/// relationship has") that is the DIFFERENCE to a target value, so the +1
/// event (`add` 10 in the campaign's x10 units) moves the relationship exactly
/// to the target, and only while the difference is positive. The target of a
/// social performance is the relationship manager's `RolloverRltnshpNeutralCap`
/// 3 (level 1) or `RolloverRltnshpGoodCap` 7 (level 2), of an answering
/// audience member NeutralCap, of a delivered gift 0 (Neutral).
pub fn lift_to(relations: f32, target: f32, add: f32) -> f32 {
    // `scale` is in the manager's units (the difference / 10); the event's
    // `add` is in the campaign's x10 units, so `add * scale` is the difference.
    let delta = target - relations;
    if delta > 0.0 {
        relations + delta * (add / 10.0)
    } else {
        relations
    }
}

/// The relationship the social tools aim for (the manager's NeutralCap /
/// GoodCap, x10 units).
pub const SOCIAL_NEUTRAL_CAP: f32 = FRIENDLY_RELATION_LIMIT;
pub const SOCIAL_GOOD_CAP: f32 = ALLY_RELATION_THRESHOLD;

/// The level of a social performance, fixed when it starts (0xDA183A..0xDA1863:
/// the attitude toward the player of the tribe it is for: Cautious -> 1,
/// Pleased -> 2, Furious / Annoyed / Friendly -> 0).
pub fn social_level(relations: f32) -> u8 {
    match attitude(relations) {
        2 => 1,
        3 => 2,
        _ => 0,
    }
}

/// The relationship a successful performance of `level` lifts the tribe to
/// (0xDA34FB: level 1 -> NeutralCap, level 2 -> GoodCap; level 0 nothing).
pub fn social_target(level: u8) -> Option<f32> {
    match level {
        1 => Some(SOCIAL_NEUTRAL_CAP),
        2 => Some(SOCIAL_GOOD_CAP),
        _ => None,
    }
}

/// A `TribeSocial` event with the retail scale (see [`lift_to`]): the
/// relationship rises to `target` unless it is there already.
pub fn social_lift(relations: f32, target: f32) -> f32 {
    lift_to(relations, target, SOCIAL_RELATIONSHIP_IMPROVEMENT).clamp(RELATION_MIN, RELATION_MAX)
}

/// The relationship after a gift is delivered to a tribe (0xDBC1F1..0xDBC25C):
/// only a tribe whose attitude is below Cautious (Furious / Annoyed) reacts,
/// the `TribeGift` event with scale `-relationship` moves it to 0 (Cautious).
/// A tribe at Cautious or better keeps its relationship (the gift only starts
/// the fading timer).
pub fn gift_effect(relations: f32) -> f32 {
    if attitude(relations) < 2 {
        lift_to(relations, 0.0, GIFT_RELATIONSHIP).clamp(RELATION_MIN, RELATION_MAX)
    } else {
        relations
    }
}

// ---------------------------------------------------------------------------
// The social rhythm game (the performance object, update 0xDA2F30)
//
// VERIFIED (march2017 addresses): the sendgame handler 0xDA1F20 (the patch's
// `trg_social_length` -> rounds, `trg_round_length`, the thought bubble
// 0xFF77507F, the beat 0xACEB7EA7), the round evaluation 0xDA0100, the score
// object 0xD9CC20 / 0xD9CCF0, the end 0xDA3490..0xDA3578 and the audience
// member 0xDAEA70. Read in `notes\tribal-song-notes.md`.
// ---------------------------------------------------------------------------

/// Rounds of a song: the patch `music_tribe_social` answers `trg_social_length`
/// 5 (0xDA20C7 stores it at +0x48).
pub const SOCIAL_ROUNDS: u32 = 5;
/// Seconds between two beats: `trg_round_length` 4 (stored at +0x4C by
/// 0xDA2098; also the time the start score fills its bar over).
pub const SOCIAL_ROUND_SECONDS: f32 = 4.0;
/// The village's thought bubble comes this long before its beat (the patch's
/// `trg_social_thought_bubble` send, measured on the installed patch).
pub const SOCIAL_BUBBLE_LEAD: f32 = 1.0;
/// Seconds after a beat the round is evaluated when the player gave no order
/// (0xDA1F87: the timer +0x54 = global 0x158132C = TribeTuning 0xE32237C9, 1.5
/// on Normal, 2 on Easy, 0.5 on Hard).
pub const SOCIAL_EVAL_DELAY: f32 = 1.5;
/// Refresh time of the three social abilities (`tribe_socialhorn` /
/// `tribe_socialmaraca` / `tribe_socialdidgeridoo`, ability types 5 / 6 / 7):
/// an instrument can be ordered again after this many seconds.
pub const SOCIAL_ORDER_REFRESH: f32 = 5.0;
/// `scoreContributionPerCreature` (global 0x1581300): every participant adds it
/// to the start score (0xD9CDC2). The chieftain's, the holding-tool and the
/// per-tool contributions (0xD9CD7D / 0xD9CDF0 / 0xD9CE3C) are 0 in the data.
pub const SCORE_PER_CREATURE: f32 = 25.0;
/// `scoreContributionPerAccessoryLevel` (global 0x1581304): the species' social
/// level times this is added to the start score (0xD9CEB3).
pub const SCORE_PER_ACCESSORY_LEVEL: f32 = 35.0;
/// The score a beat adds when the wanted instrument is played by enough
/// members (the performance's +0x3C, initial global 0x1581314 = TribeTuning
/// 0x459CE162).
pub const SCORE_PER_BEAT: f32 = 175.0;
/// What each member that played a not-wanted instrument adds when nobody played
/// the wanted one (0xDA0412: global 0x158131C = TribeTuning 0x58F922E2).
pub const SCORE_PER_EXTRA_PLAYER: f32 = 5.0;
/// The gift timer a failed song leaves running is at least this many seconds
/// (0xD9FE7C: `max(saved timer, global 0x158137C = TribeTuning 0xB6CE0861)`).
pub const GIFT_TIMER_AFTER_SONG_MIN: f32 = 30.0;

/// The instrument kinds of the social tools, in the order the troupe counts keep
/// them: horn (tool type 4, ability type 5), maracas (5, 6), didgeridoo (6, 7).
pub const SOCIAL_KINDS: usize = 3;

/// The social game's data of one NPC tribe archetype (`cTribeArchetype`, loaded by
/// 0xC9775D..0xC977E9 and 0xC97917).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SocialTuning {
    /// The `archetype` id.
    pub archetype: u8,
    /// +0x90 (property 0xE0BE991F): the score that fills the bar for a level 1
    /// song (a Cautious tribe).
    pub threshold_level1: f32,
    /// +0x94 (property 0x1570506E): the same for any other level (a Pleased tribe).
    pub threshold_level2: f32,
    /// +0x98 (property 0xD164F429): the range the number of members a bubble
    /// asks for is drawn from (0x4DF340, then `cvtss2si`).
    pub wanted_min: f32,
    pub wanted_max: f32,
    /// +0xA0 (property 0xA9A2D5E0): the ability types the village may ask for
    /// (5 horn, 6 maracas, 7 didgeridoo).
    pub instruments: &'static [u8],
}

const ALL_THREE: &[u8] = &[5, 6, 7];

/// The ten archetypes' social data (`tribe_archetypes.txt`); the parent
/// `TribeTypesTemplate` has 800 / 1000, (1, 3), [5, 6, 7].
pub const SOCIAL_TUNING: [SocialTuning; 10] = [
    SocialTuning { archetype: 1, threshold_level1: 795.0, threshold_level2: 880.0, wanted_min: 3.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 2, threshold_level1: 795.0, threshold_level2: 880.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 15, threshold_level1: 795.0, threshold_level2: 880.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 3, threshold_level1: 905.0, threshold_level2: 990.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 4, threshold_level1: 905.0, threshold_level2: 990.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 7, threshold_level1: 905.0, threshold_level2: 990.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 8, threshold_level1: 905.0, threshold_level2: 990.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 11, threshold_level1: 905.0, threshold_level2: 990.0, wanted_min: 2.0, wanted_max: 3.0, instruments: &[7] },
    SocialTuning { archetype: 12, threshold_level1: 905.0, threshold_level2: 990.0, wanted_min: 2.0, wanted_max: 3.0, instruments: ALL_THREE },
    SocialTuning { archetype: 14, threshold_level1: 1000.0, threshold_level2: 1090.0, wanted_min: 3.0, wanted_max: 4.0, instruments: ALL_THREE },
];

/// The social data of `archetype` (the template's when the archetype is unknown).
pub fn social_tuning(archetype: u8) -> SocialTuning {
    SOCIAL_TUNING.iter().copied().find(|t| t.archetype == archetype).unwrap_or(SocialTuning {
        archetype,
        threshold_level1: 800.0,
        threshold_level2: 1000.0,
        wanted_min: 1.0,
        wanted_max: 3.0,
        instruments: ALL_THREE,
    })
}

/// The score that fills the bar (0xD9CC20: `+0x90` for a level 1 song, `+0x94`
/// otherwise).
pub fn social_threshold(tuning: &SocialTuning, level: u8) -> f32 {
    if level == 1 {
        tuning.threshold_level1
    } else {
        tuning.threshold_level2
    }
}

/// The start score (0xD9CCF0, once when the troupe stands in place): the species'
/// social level times `scoreContributionPerAccessoryLevel`, plus
/// `scoreContributionPerCreature` for every participant.
pub fn social_start_score(social_level: u32, participants: u32) -> f32 {
    social_level as f32 * SCORE_PER_ACCESSORY_LEVEL + participants as f32 * SCORE_PER_CREATURE
}

/// What one evaluation adds to the score (0xDA0100). `wanted_played` members of
/// the wanted instrument played, `required` is what the bubble asked for,
/// `others` members of any instrument played: enough of the wanted one scores a
/// full beat, some of it the share, none of it a small credit per player.
pub fn social_beat_score(wanted_played: u32, required: u32, others: u32) -> f32 {
    if wanted_played >= required {
        SCORE_PER_BEAT
    } else if wanted_played > 0 {
        wanted_played as f32 / required as f32 * SCORE_PER_BEAT
    } else {
        others as f32 * SCORE_PER_EXTRA_PLAYER
    }
}

/// The bar of a social performance (0xD9CC20): `min(score / threshold x 10, 10)`;
/// an archetype without a threshold keeps the bar where it is (0).
pub fn social_bar(score: f32, threshold: f32) -> f32 {
    if threshold > 1.5e-5 {
        (score / threshold * 10.0).min(10.0)
    } else {
        0.0
    }
}

/// Whether the bar is full, which ends the song as a success (0xDA3490: the
/// bar object's fill `+0x68` reaches 1, 0xE310D0): the score reached the
/// threshold of the song's level.
pub fn social_success(score: f32, threshold: f32) -> bool {
    threshold > 1.5e-5 && score >= threshold
}

// ---------------------------------------------------------------------------
// Feeding a wild animal (FeedWild 0xD9DC40, the interaction counter 0xBA43E0)
// ---------------------------------------------------------------------------

/// The price of one feeding when the animal's species gives none: the loader
/// default of the species property 0xEB69140E (0x4E1CA0 reads it into the
/// species record +0x43C, VERIFIED T13C).
pub const FEED_COST_DEFAULT: f32 = 5.0;

/// The price of one feeding of an animal of the tribal herd `archetype` name:
/// the property 0xEB69140E of the installed archetypes (VERIFIED, notes
/// `tribal-social2-notes.md`: `TRG_NPC_Small_Prey` 5, `TRG_NPC_Medium_Prey` 10,
/// `TRG_NPC_Large_Prey` 15); a herd without it pays the default.
pub fn feed_cost_of(herd: &str) -> f32 {
    match herd {
        "TRG_NPC_Small_Prey" => 5.0,
        "TRG_NPC_Medium_Prey" => 10.0,
        "TRG_NPC_Large_Prey" => 15.0,
        _ => FEED_COST_DEFAULT,
    }
}

/// The interaction counts that turn an animal Curious / Friendly / a best friend
/// (species record +0x404 / +0x408 / +0x40C = properties `NumToImpressForCurious`
/// 0x2028AD2E, `...Friendly` 0xE9CAA7E5, `...BestFriends` 0x8A7F7999; the record's
/// defaults 1 / 3 / 6, 0x4DB0ED..0x4DB10E, apply to a species without them).
/// The three tribal prey herds carry 1 / 1 / 1 in their installed archetypes
/// (VERIFIED), so one accepted feeding is enough for them.
pub fn impress_counts(herd: &str) -> [u32; 3] {
    match herd {
        "TRG_NPC_Small_Prey" | "TRG_NPC_Medium_Prey" | "TRG_NPC_Large_Prey" => [1, 1, 1],
        _ => [1, 3, 6],
    }
}

/// The interaction state after `count` accepted interactions (0xBA43E0, called
/// by the interaction manager 0xBA4A00 for the "befriend" event 3): at the best
/// friends count 6 (FeedWild then ends the other feedings and starts Tame), at
/// the friendly count 4, from the curious count on 3; below it the animal's own
/// state (`base`, 0xBA40E0).
pub fn interaction_state(count: u32, counts: [u32; 3], base: u32) -> u32 {
    let [curious, friendly, best] = counts;
    if count >= best {
        6
    } else if count >= friendly {
        4
    } else if count < curious {
        base
    } else {
        3
    }
}

/// How many members the next bubble asks for (0xDA2162..0xDA216F): a float
/// drawn from the archetype's range (`roll` in 0..1), rounded to the nearest
/// integer (`cvtss2si`, ties to even).
pub fn social_required(tuning: &SocialTuning, roll: f32) -> u32 {
    let x = tuning.wanted_min + (tuning.wanted_max - tuning.wanted_min) * roll.clamp(0.0, 1.0);
    let floor = x.floor();
    let frac = x - floor;
    let up = frac > 0.5 || (frac == 0.5 && floor as i64 % 2 != 0);
    (floor as u32) + up as u32
}

/// The instrument the next bubble asks for (0xDA2195..0xDA21D8: a random entry
/// of the archetype's list; 5 horn, 6 maracas, 7 didgeridoo), as the troupe
/// index (0 horn, 1 maracas, 2 didgeridoo).
pub fn social_wanted_kind(tuning: &SocialTuning, pick: u32) -> usize {
    let list = if tuning.instruments.is_empty() { ALL_THREE } else { tuning.instruments };
    (list[pick as usize % list.len()] - 5) as usize
}

/// Whether the song's end is the happy one (0xDA3A03).
pub fn social_ending_cheers(bar: f32) -> bool {
    bar > SOCIAL_NEUTRAL_SCORE_THRESHOLD
}

// ---------------------------------------------------------------------------
// The troupe's walk to its spots and its ending clips (tribal-social3, notes T15):
// the start 0xDA1290 (spots, layout slots), the layout 0xDA22C0, the performance
// states 0..4 of 0xDA2F30 and the members' stages 0..9 (0xDA3656..0xDA3A44).
// ---------------------------------------------------------------------------

/// How far from the target, in front of the troupe, the near spots stand
/// (`2 x size + 15`; `.rdata` 0x147C0C4 = 15, VERIFIED 0xDA1481).
pub const SOCIAL_SPOT_NEAR: f32 = 15.0;
/// The far spot's distance (`2 x size + 22`; 0x147C0C8 = 22, VERIFIED 0xDA1489).
pub const SOCIAL_SPOT_FAR: f32 = 22.0;
/// The side spots swing the approach line by this angle about the planet's up
/// (a quaternion of half angle `0x1593470 x 0.5` = pi/8, VERIFIED 0xDA1555).
pub const SOCIAL_SPOT_SWING: f32 = std::f32::consts::FRAC_PI_4;
/// The chieftain's distance is `2 x (size + 1) + 3` (0x147C0CC = 3, 0xDA254C).
pub const SOCIAL_CHIEF_BASE: f32 = 3.0;
/// The layout's slot radius is this times the size (0x13EED94 = 1.5, 0xDA17D9).
pub const SOCIAL_SLOT_SCALE: f32 = 1.5;
/// The radius of the slot disc (0x13F5CB8 = 45, loaded at 0xDA16E5 for the
/// layout's parameters; the role of the value is INFERRED from
/// `cCommunityLayout::SetParameters`).
pub const SOCIAL_LAYOUT_RADIUS: f32 = 45.0;
/// ADAPTED: the size the spots are measured with. Retail takes the larger of the
/// two creatures' `0xCE6BC0(species)` (a tuning-scaled product of the species
/// profile's +0x554 / +0x558 bounds), which the rewrite does not evaluate.
pub const SOCIAL_CREATURE_SIZE: f32 = 1.0;
/// ADAPTED: seconds the troupe may take to reach its spots before the song starts
/// anyway (retail waits for every participant to have arrived or to have given up
/// walking, 0xDA3283; a member the rewrite cannot path to its slot would hold the
/// song forever).
pub const SOCIAL_GATHER_TIMEOUT: f32 = 45.0;
/// How near the stand a member must be to count as arrived (`WalkTo` finishing,
/// 0xC42F80; OpenSpore's walk ends within this distance of its goal).
pub const SOCIAL_ARRIVE_DISTANCE: f32 = 0.6;

/// The groups a participant belongs to by its tool type (0xDA253F: the jump table
/// of `type - 4`): 0 horn (4), 1 maracas (5), 2 didgeridoo (6), 3 everyone else
/// (types 7..10, no tool, animals). The chieftain (type 11) has no group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TroupeRole {
    Chieftain,
    Group(usize),
}

/// Pure: the five spots of the start 0xDA1290 around the `centre` (the target's
/// position), `dir` being the unit vector from the leader toward it: retail spot 1
/// at `centre - dir x (2 size + 15)`, spots 2 / 3 the same distance with the line
/// swung by +45 / -45 degrees about the up axis, spot 4 at `centre - dir x
/// (2 size + 22)` and spot 5 at the centre itself (never given to a group).
/// Index 0 here is retail spot 1.
pub fn social_spots(centre: Vec3, dir: Vec3, size: f32) -> [Vec3; 5] {
    let near = 2.0 * size + SOCIAL_SPOT_NEAR;
    let far = 2.0 * size + SOCIAL_SPOT_FAR;
    let swing = |angle: f32| Quat::from_rotation_y(angle) * dir;
    [
        centre - dir * near,
        centre - swing(SOCIAL_SPOT_SWING) * near,
        centre - swing(-SOCIAL_SPOT_SWING) * near,
        centre - dir * far,
        centre,
    ]
}

/// Pure: the slots of the troupe's layout (`cCommunityLayout`, 0xAFC5C0: slot 0
/// at the origin, then hexagonal rings whose neighbours touch, each slot a circle
/// of `radius`), every slot within [`SOCIAL_LAYOUT_RADIUS`] of the origin. The
/// origin is halfway between the target and the first spot (0xDA177B). ADAPTED:
/// the order of the slots inside a ring (it only breaks ties of the choice).
pub fn social_slots(origin: Vec3, dir: Vec3, radius: f32) -> Vec<Vec3> {
    let step = 2.0 * radius.max(1e-3);
    let rings = (((SOCIAL_LAYOUT_RADIUS - radius) / step) + 1.0).floor().max(1.0) as i32;
    let hex: Vec<Vec3> = (0..6).map(|k| Quat::from_rotation_y(k as f32 * std::f32::consts::FRAC_PI_3) * dir * step).collect();
    let mut slots = vec![origin];
    for m in 1..rings {
        let mut at = origin + hex[0] * m as f32;
        for side in 0..6 {
            for _ in 0..m {
                slots.push(at);
                at += hex[(side + 2) % 6];
            }
        }
    }
    slots
}

/// Pure: where each member of a troupe stands (0xDA22C0). `members` are the
/// participants' current positions and roles; `centre` is the target the troupe
/// plays for and `dir` the unit vector from the leader toward it. The chieftain
/// takes the free slot nearest to `centre - dir x (2 (size + 1) + 3)`; every group
/// takes the spot whose direction from the troupe's centroid best matches its own
/// (`(dot + 1) / distance`, 0xDA2A2F..0xDA2B6A, the groups in the order of their
/// sort key) and its members take, nearest-to-the-centroid first, the free slot
/// that maximises `(dot(member - group centroid, slot - spot) + 1) / |slot - spot|`
/// (0xDA2C0D..0xDA2E68). ADAPTED: the sort keys of the groups and of their
/// members (0xD9B440 / 0xDA0FC0: the group's key is not traced) are the distance
/// of the group's centroid to the target and of the member to its group's
/// centroid.
pub fn social_stands(centre: Vec3, dir: Vec3, size: f32, members: &[(Vec3, TroupeRole)]) -> Vec<Vec3> {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let dir = flat(dir).normalize_or(Vec3::X);
    let spots = social_spots(centre, dir, size);
    let origin = centre - dir * (2.0 * size + SOCIAL_SPOT_NEAR) * 0.5;
    let slots = social_slots(origin, dir, SOCIAL_SLOT_SCALE * size);
    let mut taken = vec![false; slots.len()];
    let mut out: Vec<Vec3> = members.iter().map(|(p, _)| *p).collect();
    if members.is_empty() {
        return out;
    }
    let nearest = |taken: &[bool], slots: &[Vec3], to: Vec3| -> Option<usize> {
        (0..slots.len())
            .filter(|&i| !taken[i])
            .min_by(|&a, &b| flat(slots[a] - to).length_squared().total_cmp(&flat(slots[b] - to).length_squared()))
    };
    // 0xDA254C: the chieftain stands in front of everyone.
    let chief_at = centre - dir * (2.0 * (size + 1.0) + SOCIAL_CHIEF_BASE);
    for (i, (_, role)) in members.iter().enumerate() {
        if *role == TroupeRole::Chieftain {
            if let Some(slot) = nearest(&taken, &slots, chief_at) {
                taken[slot] = true;
                out[i] = slots[slot];
            }
        }
    }
    let troupe_centre = members.iter().fold(Vec3::ZERO, |a, (p, _)| a + flat(*p)) / members.len() as f32;
    let mut groups: Vec<(usize, Vec3, Vec<usize>)> = (0..4)
        .map(|g| {
            let who: Vec<usize> =
                (0..members.len()).filter(|&i| members[i].1 == TroupeRole::Group(g)).collect();
            let c = if who.is_empty() {
                Vec3::ZERO
            } else {
                who.iter().fold(Vec3::ZERO, |a, &i| a + flat(members[i].0)) / who.len() as f32
            };
            (g, c, who)
        })
        .filter(|(_, _, who)| !who.is_empty())
        .collect();
    groups.sort_by(|a, b| flat(a.1 - centre).length_squared().total_cmp(&flat(b.1 - centre).length_squared()));
    let mut spot_taken = [false; 4];
    for (_, group_centre, who) in &groups {
        let from = (*group_centre - troupe_centre).normalize_or_zero();
        let mut best: Option<(usize, f32)> = None;
        for (k, spot) in spots.iter().take(4).enumerate() {
            if spot_taken[k] {
                continue;
            }
            let to = flat(*spot - troupe_centre);
            let score = (from.dot(to.normalize_or_zero()) + 1.0) / (to.length() + 1e-8);
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((k, score));
            }
        }
        let Some((k, _)) = best else { continue };
        spot_taken[k] = true;
        let spot = spots[k];
        let mut order = who.clone();
        order.sort_by(|&a, &b| {
            flat(members[a].0 - *group_centre)
                .length_squared()
                .total_cmp(&flat(members[b].0 - *group_centre).length_squared())
        });
        for i in order {
            let mut want = flat(members[i].0 - *group_centre).normalize_or_zero();
            if want == Vec3::ZERO {
                want = flat(members[i].0 - spot).normalize_or_zero();
            }
            let mut pick: Option<(usize, f32)> = None;
            for (s, slot) in slots.iter().enumerate() {
                if taken[s] {
                    continue;
                }
                let v = flat(*slot - spot);
                let score = (want.dot(v.normalize_or_zero()) + 1.0) / (v.length() + 1e-8);
                if pick.is_none_or(|(_, b)| score > b) {
                    pick = Some((s, score));
                }
            }
            let slot = pick.map(|(s, _)| s).or_else(|| nearest(&taken, &slots, spot));
            if let Some(s) = slot {
                taken[s] = true;
                out[i] = slots[s];
            }
        }
    }
    out
}

/// The troupe's cheer when the song ends, by tool type 4 / 5 / 6 (0xDA3991..0xDA39B3
/// `soc_flute_end`, `soc_shakern01_end`, `soc_didger_end`) and its length in
/// seconds (the installed ANIMs: 36 / 35 / 36 frames at 30 fps).
pub const SOCIAL_CHEER: [(u32, f32); 3] =
    [(0xD472_CE9C, 36.0 / 30.0), (0x5474_36F2, 35.0 / 30.0), (0x1474_36A3, 36.0 / 30.0)];
/// The performance's ending clips (0xDA3A03): `soc_sign_chief_01` for the
/// chieftain and `soc_sign_react_01` for the others when the bar passed
/// [`SOCIAL_NEUTRAL_SCORE_THRESHOLD`], `soc_fail_01` for everyone otherwise;
/// seconds from the installed ANIMs (251 / 228 / 193 frames at 30 fps).
pub const SOCIAL_ENDING_CHIEF: (u32, f32) = (0xD478_051A, 251.0 / 30.0);
pub const SOCIAL_ENDING_CROWD: (u32, f32) = (0x5474_62FF, 228.0 / 30.0);
pub const SOCIAL_ENDING_FAIL: (u32, f32) = (0x1478_0312, 193.0 / 30.0);
/// The waiting idles of a member that stands at its spot while the others arrive
/// (member stage 4, 0xDA3782..0xDA3826: `soc_chief_idle_wait01`, then by tool type
/// horn / maracas / didgeridoo / none `soc_flute_idle_wait01`,
/// `soc_shakern_idle_wait01`, `soc_didger_idle_wait01`, `soc_notool_idle_wait01`).
pub const SOCIAL_WAIT_CHIEF: u32 = 0xF477_F9B6;
pub const SOCIAL_WAIT: [u32; 4] = [0xF477_F9BA, 0xF477_F9BF, 0xF477_F9B8, 0xF477_F9BC];
/// The clip an instrument player plays when it reaches its spot (member stage 3,
/// 0xDA370A..0xDA376A: `soc_flute_start`, `soc_shakern01_start`,
/// `soc_didger_start`; the same ids as `outfit_crowd::tribe_clip::INSTRUMENT`).
pub const SOCIAL_START: [u32; 3] = [0x1472_CE9A, 0x5474_36F1, 0x1474_36A2];

/// The cheer an instrument player plays before the ending clip and the clip
/// every member then plays: `(cheer, ending)` for the member of `group`
/// (instrument 0..3 or `None`), the chieftain flag and the song's outcome
/// (`bar` as [`social_bar`] left it).
pub fn social_ending_clips(group: Option<usize>, chieftain: bool, bar: f32) -> (Option<(u32, f32)>, (u32, f32)) {
    let cheer = group.filter(|g| *g < 3 && !chieftain).map(|g| SOCIAL_CHEER[g]);
    let ending = if !social_ending_cheers(bar) {
        SOCIAL_ENDING_FAIL
    } else if chieftain {
        SOCIAL_ENDING_CHIEF
    } else {
        SOCIAL_ENDING_CROWD
    };
    (cheer, ending)
}

/// How long the members' ending runs, from the song's end to the stop 0xD9FE60
/// that hides the bar. ADAPTED: the instrument players' cheer and the longest
/// ending clip (the stop's caller, the creature callback 0xDA0550, was not
/// traced; retail runs it after the members' ending clips).
pub fn social_ending_seconds(bar: f32) -> f32 {
    let ending = if social_ending_cheers(bar) { SOCIAL_ENDING_CHIEF.1 } else { SOCIAL_ENDING_FAIL.1 };
    SOCIAL_CHEER.iter().map(|c| c.1).fold(0.0, f32::max) + ending
}

/// A beat's score fills the bar over this many seconds (0xDA039E / 0xDA0418 pass
/// `1.0` as the `seconds` of 0xD9CC20; the start score passes
/// `rounds x roundLength`).
pub const SOCIAL_BAR_BEAT_SECONDS: f32 = 1.0;
/// How fast a failed song's bar drains, bar widths per second (the multiplier
/// 0.4 of 0xE31380, `.rdata` 0x13EC414).
pub const SOCIAL_BAR_DRAIN_PER_SECOND: f32 = 0.4;
/// The bar is shown while the camera is closer than this to the troupe (the
/// per-frame test of 0xDA3040..0xDA309B against global 0x1581330, the
/// `TribeTuning` property 0xFA555876 = 200; 0xDA2F30 compares the squared distance
/// of the camera position, global 0x167EA30, with the bar's world point).
pub const SOCIAL_BAR_VISIBLE_DISTANCE: f32 = 200.0;
/// Fills the queue of the bar holds (retail: an unbounded vector; a song never
/// has more than the start score and two beats pending, a full queue merges
/// into its last fill).
pub const SOCIAL_BAR_QUEUE: usize = 8;

/// The social bar UI object (`SocialBar` layout, class 0xE312E0..0xE31630): the
/// bar's fill `+0x68` (0..1), the queued fills `(fraction left, fraction per
/// second)` of 0xE315B0 and the "failed" flag `+0x7C` of 0xE31610. Every fill of
/// the queue runs at the same time (0xE31380 walks the whole vector each frame).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct SocialBarMeter {
    /// The bar's fill (+0x68). It can pass 1 by the last step, as in retail.
    pub fill: f32,
    /// The song failed: the bar drains (+0x7C).
    pub failed: bool,
    queue: [(f32, f32); SOCIAL_BAR_QUEUE],
    len: u8,
}

impl SocialBarMeter {
    /// 0xD9CC20 -> 0xE315B0: queue `fraction` of the bar, filled over `seconds`
    /// (the pair is `(fraction, fraction / seconds)`).
    pub fn queue_fill(&mut self, fraction: f32, seconds: f32) {
        let rate = fraction / seconds;
        let n = self.len as usize;
        if n < SOCIAL_BAR_QUEUE {
            self.queue[n] = (fraction, rate);
            self.len += 1;
        } else {
            // ADAPTED: the retail vector grows; the last fill takes the rest.
            self.queue[n - 1].0 += fraction;
            self.queue[n - 1].1 += rate;
        }
    }

    /// 0xE31380, once per frame: a failed bar drains; a full one stands; else
    /// every queued fill gives `min(left, rate x dt)` to the bar and is dropped
    /// when nothing is left.
    pub fn update(&mut self, dt: f32) {
        if self.failed {
            self.fill -= dt * SOCIAL_BAR_DRAIN_PER_SECOND;
            if self.fill <= 0.0 {
                self.failed = false;
                self.fill = 0.0;
            }
            return;
        }
        if self.fill >= 1.0 {
            return;
        }
        let mut kept = 0;
        for i in 0..self.len as usize {
            let (left, rate) = self.queue[i];
            let step = left.min(rate * dt);
            self.fill += step;
            let left = left - step;
            if left > 0.0 {
                self.queue[kept] = (left, rate);
                kept += 1;
            }
        }
        self.len = kept as u8;
    }

    /// 0xE310D0: the fill reached the threshold (what ends a song as a success).
    pub fn is_full(&self) -> bool {
        self.fill >= 1.0
    }

    /// 0xE311D0: some fill is still on its way.
    pub fn animating(&self) -> bool {
        self.len > 0
    }

    /// 0xE31610 (the song failed): drop the queue and start draining.
    pub fn fail(&mut self) {
        self.len = 0;
        self.failed = true;
    }
}

/// The two faces of the bar by the attitude the song started at (0xE31170,
/// called with the attitude at 0xDA195E): Cautious (2) shows the neutral face
/// (frame 2) and aims at the blue one (3), Pleased (3) aims at the green one
/// (4). The faces are the five frames of the relationship face (angry, sad,
/// neutral, blue, green).
pub fn social_bar_faces(level: u8) -> Option<(u32, u32)> {
    match level {
        1 => Some((2, 3)),
        2 => Some((3, 4)),
        _ => None,
    }
}

/// Seconds of the gift timer of a tribe (cTribe +0x2CC, set by a `TribeGift`
/// 0xDBC28F or a `TribeSocial` 0xDAEB35 event): the difficulty tuning property
/// 0xA4775A7A (Easy 360, Normal 180, Hard 60); the byte property 0xB4853F28 (1
/// on every difficulty) switches the timer on.
pub fn gift_timer_seconds(difficulty: crate::creature_rules::Difficulty) -> f32 {
    match difficulty {
        crate::creature_rules::Difficulty::Easy => 360.0,
        crate::creature_rules::Difficulty::Medium => 180.0,
        crate::creature_rules::Difficulty::Hard => 60.0,
    }
}

/// What one expiry of a tribe's gift timer does (0xCC6D5A, the NPC tribe
/// controller's update): the gift effect fades one attitude step. A Friendly
/// tribe (an ally) keeps it and the timer stops; otherwise the relationship is
/// pulled down (a `TribeGift` event with the negative difference) to the floor
/// of its attitude (globals 0x158135C / 0x1581360 / 0x1581364 for attitude 1 / 2
/// / 3: `RolloverRltnshpVBadCap` -7, `RolloverRltnshpBadCap` -3, the constant
/// 0 the loader 0xCE6C70 stores at +0x15C; x10 here) and the timer restarts
/// while the tribe is still more friendly than its baseline attitude (0xC8EC60,
/// the attitude of the relationship it started with).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GiftDecay {
    pub relations: f32,
    pub restart: bool,
}

pub fn gift_decay(relations: f32, baseline: f32) -> GiftDecay {
    let attitude_now = attitude(relations);
    if attitude_now == 4 {
        return GiftDecay { relations, restart: false };
    }
    let floor = match attitude_now {
        1 => Some(FURIOUS_RELATION_LIMIT),
        2 => Some(ANNOYED_RELATION_LIMIT),
        3 => Some(0.0),
        _ => None,
    };
    let relations = match floor {
        Some(floor) if floor - relations < 0.0 => floor,
        _ => relations,
    };
    GiftDecay { relations, restart: attitude(relations) > attitude(baseline) }
}

/// Roles of an NPC tribe as shares of its members (the `percentage*` props).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Roles {
    pub hornist: f32,
    pub didgeridoo: f32,
    pub maraca: f32,
    pub healer: f32,
    pub spearman: f32,
    pub axeman: f32,
    /// `0x80A52AC0`: the torch carriers (inferred from the tool preferences).
    pub torchman: f32,
    pub fisherman: f32,
    pub gatherer: f32,
}

/// One NPC tribe archetype (`TribeArchetypes`, parent `TribeTypesTemplate`:
/// 3 members, 200 food, up to 12, `FoodRate` 0.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcTribe {
    pub name: &'static str,
    /// The `archetype` id the lineups list.
    pub archetype: u8,
    /// Tier 1..=5 (one tribe of each tier is in a game).
    pub tier: u8,
    pub initial_population: usize,
    pub max_population: usize,
    pub initial_food: f32,
    /// `toolprefs`: the tool types the tribe uses, in order.
    pub tools: &'static [u8],
    pub roles: Roles,
}

const NO_ROLES: Roles = Roles {
    hornist: 0.0,
    didgeridoo: 0.0,
    maraca: 0.0,
    healer: 0.0,
    spearman: 0.0,
    axeman: 0.0,
    torchman: 0.0,
    fisherman: 0.0,
    gatherer: 0.0,
};

/// The ten archetypes of `tribe_archetypes.txt`.
pub const NPC_TRIBES: [NpcTribe; 10] = [
    NpcTribe {
        name: "1a",
        archetype: 1,
        tier: 1,
        initial_population: 3,
        max_population: 6,
        initial_food: 125.0,
        tools: &[4, 8],
        roles: Roles {
            hornist: 0.333,
            fisherman: 0.667,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "1b",
        archetype: 2,
        tier: 1,
        initial_population: 3,
        max_population: 6,
        initial_food: 125.0,
        tools: &[6, 7],
        roles: Roles {
            didgeridoo: 0.333,
            gatherer: 0.667,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "1c",
        archetype: 15,
        tier: 1,
        initial_population: 3,
        max_population: 6,
        initial_food: 125.0,
        tools: &[5, 8],
        roles: Roles {
            fisherman: 0.667,
            maraca: 0.333,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "2a",
        archetype: 3,
        tier: 2,
        initial_population: 6,
        max_population: 9,
        initial_food: 200.0,
        tools: &[3, 7, 4],
        roles: Roles {
            hornist: 0.111,
            spearman: 0.778,
            gatherer: 0.111,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "2b",
        archetype: 4,
        tier: 2,
        initial_population: 6,
        max_population: 9,
        initial_food: 200.0,
        tools: &[9, 7, 3, 6],
        roles: Roles {
            healer: 0.222,
            spearman: 0.667,
            gatherer: 0.111,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "3a",
        archetype: 7,
        tier: 3,
        initial_population: 6,
        max_population: 9,
        initial_food: 200.0,
        tools: &[1, 8, 5],
        roles: Roles {
            torchman: 0.778,
            fisherman: 0.111,
            maraca: 0.111,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "3b",
        archetype: 8,
        tier: 3,
        initial_population: 6,
        max_population: 9,
        initial_food: 200.0,
        tools: &[1, 9, 8, 4],
        roles: Roles {
            healer: 0.222,
            torchman: 0.667,
            fisherman: 0.111,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "4a",
        archetype: 11,
        tier: 4,
        initial_population: 6,
        max_population: 9,
        initial_food: 200.0,
        tools: &[2, 9, 5],
        roles: Roles {
            healer: 0.222,
            maraca: 0.111,
            axeman: 0.667,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "4b",
        archetype: 12,
        tier: 4,
        initial_population: 6,
        max_population: 9,
        initial_food: 200.0,
        tools: &[2, 6],
        roles: Roles {
            didgeridoo: 0.222,
            axeman: 0.778,
            ..NO_ROLES
        },
    },
    NpcTribe {
        name: "5",
        archetype: 14,
        tier: 5,
        initial_population: 9,
        max_population: 12,
        initial_food: 300.0,
        tools: &[2, 1, 3, 9, 5, 6],
        roles: Roles {
            healer: 0.18,
            spearman: 0.18,
            torchman: 0.18,
            axeman: 0.27,
            ..NO_ROLES
        },
    },
];

/// The archetype with this id.
pub fn npc_tribe(archetype: u8) -> Option<&'static NpcTribe> {
    NPC_TRIBES.iter().find(|t| t.archetype == archetype)
}

/// Hut level (1..=3) of the NPC tribe of `tier`. VERIFIED (0xC9A7AF ->
/// 0xC00BE0): an NPC tribe's level is the first `CommunityMax_*` entry its
/// archetype's max population fits in (6 / 9 / 12 -> level 0 / 1 / 2, see
/// `tribal_village::community_level`), so tiers 1 / 2-4 / 5 live in level
/// 1 / 2 / 3 huts.
pub fn tribe_hut_level(tier: u8) -> usize {
    NPC_TRIBES
        .iter()
        .find(|t| t.tier == tier)
        .map(|t| crate::tribal_village::community_level(t.max_population as u32) as usize + 1)
        .unwrap_or(if tier > 5 { 3 } else { 1 })
        .min(3)
}

/// Hit points of one piece of an NPC village: slot 0 is the dwelling (a hut of
/// the tribe's level), slots 1 and 2 are tool racks (`tribeToolHealth`). ADAPTED
/// mapping of OpenSpore's three village pieces onto the hut and two racks.
pub fn village_piece_hp(tier: u8, slot: u32) -> f32 {
    if slot == 0 {
        hut_health(tribe_hut_level(tier))
    } else {
        TOOL_HEALTH
    }
}

/// Damage per second of an average member of `tribe`: each armed share fights
/// with its weapon, everybody else (instrument players, fishermen, gatherers)
/// bare-handed.
pub fn defender_dps(tribe: &NpcTribe) -> f32 {
    let r = tribe.roles;
    let armed = r.spearman + r.axeman + r.torchman + r.healer;
    r.spearman * SPEAR.melee_dps()
        + r.axeman * AXE.melee_dps()
        + r.torchman * TORCH.melee_dps()
        + r.healer * SHAMAN_STAFF.melee_dps()
        + (1.0 - armed).max(0.0) * UNARMED.melee_dps()
}

/// The nine five-tribe lineups (`TribeTuning` unnamed props 0x000DB6B9,
/// 0x1631B0C3, 0x4252B6FF, 0x8B4BF474, 0x8DA58D04, 0x97A0E4C2, 0xF18A299D,
/// 0xF7426B5E, 0xFB43C9A7 and their easy / hard overrides): per difficulty
/// (easy, medium, hard) and lineup, one `(archetype, initial relationship)` per
/// tier 1..=5. The numbers 0 / -3 / -7 are in the manager's unit (the attitude
/// caps -7 / -3 / 3 / 7 are the same unit: -7 Furious, -3 Annoyed, 0 Cautious);
/// `lineup_tribe` multiplies them by ten to meet the event table, which the
/// notes list "x10". ADAPTED: how the table's numbers map onto the manager's
/// unit (0xD05DD0 adds the table's `add` to the stored value) was not decoded.
const LINEUPS: [[[(u8, i8); 5]; 9]; 3] = [
    // easy
    [
        [(15, 0), (4, -7), (7, -3), (12, 0), (14, -3)], // 0x000DB6B9
        [(1, 0), (3, -7), (7, 0), (11, -3), (14, -3)],  // 0x1631B0C3
        [(1, 0), (3, -3), (7, 0), (11, -7), (14, -3)],  // 0x4252B6FF
        [(15, 0), (4, -7), (7, 0), (12, -3), (14, -3)], // 0x8B4BF474
        [(2, 0), (3, 0), (8, -7), (12, -3), (14, -3)],  // 0x8DA58D04
        [(2, 0), (3, -7), (8, -3), (12, 0), (14, -3)],  // 0x97A0E4C2
        [(15, 0), (4, -3), (7, -7), (12, 0), (14, -3)], // 0xF18A299D
        [(1, 0), (3, 0), (7, -3), (11, -7), (14, -3)],  // 0xF7426B5E
        [(2, 0), (3, -3), (8, -7), (12, 0), (14, -3)],  // 0xFB43C9A7
    ],
    // medium
    [
        [(15, 0), (4, -3), (7, -3), (12, -7), (14, -7)], // 0x000DB6B9
        [(1, 0), (3, -3), (7, -3), (11, -7), (14, -7)],  // 0x1631B0C3
        [(1, 0), (3, -3), (7, -7), (11, -3), (14, -7)],  // 0x4252B6FF
        [(15, 0), (4, -3), (7, -7), (12, -3), (14, -7)], // 0x8B4BF474
        [(2, 0), (3, -3), (8, -3), (12, -7), (14, -7)],  // 0x8DA58D04
        [(2, 0), (3, -3), (8, -7), (12, -3), (14, -7)],  // 0x97A0E4C2
        [(15, 0), (4, -7), (7, -3), (12, -3), (14, -7)], // 0xF18A299D
        [(1, 0), (3, -7), (7, -3), (11, -3), (14, -7)],  // 0xF7426B5E
        [(2, 0), (3, -7), (8, -3), (12, -3), (14, -7)],  // 0xFB43C9A7
    ],
    // hard
    [
        [(15, -3), (4, -7), (7, -7), (12, -3), (14, -7)], // 0x000DB6B9
        [(1, -3), (3, -7), (7, -3), (11, -7), (14, -7)],  // 0x1631B0C3
        [(1, -3), (3, -7), (7, -7), (11, -3), (14, -7)],  // 0x4252B6FF
        [(15, -3), (4, -3), (7, -7), (12, -7), (14, -7)], // 0x8B4BF474
        [(2, -3), (3, -7), (8, -3), (12, -7), (14, -7)],  // 0x8DA58D04
        [(2, -3), (3, -3), (8, -7), (12, -7), (14, -7)],  // 0x97A0E4C2
        [(15, -3), (4, -7), (7, -3), (12, -7), (14, -7)], // 0xF18A299D
        [(1, -3), (3, -7), (7, -7), (11, -3), (14, -7)],  // 0xF7426B5E
        [(2, -3), (3, -7), (8, -3), (12, -7), (14, -7)],  // 0xFB43C9A7
    ],
];

/// Number of lineups per difficulty.
pub const LINEUP_COUNT: usize = 9;
/// The world seed picks a lineup. Only the fallback of [`lineup_for_seed`]'s
/// callers that know nothing of the player (synthetic worlds, tests): the
/// retail pick is [`lineup_for_player`].
pub fn lineup_for_seed(seed: u64) -> u8 {
    ((seed ^ (seed >> 32)) % LINEUP_COUNT as u64) as u8
}

/// The player's diet as the lineup choice reads it (0xC8E9E0 / 0xC8EA00: the
/// species' herbivorous / carnivorous mouth ranks, +0x60C / +0x608).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineupDiet {
    /// Both mouths (`clg_mix`).
    Omnivore,
    /// Herbivorous mouth only (`clg_plant`).
    Herbivore,
    /// Anything else (`clg_meat`).
    Carnivore,
}

/// The Creature stage's play-style card the lineup choice reads (cPlayer
/// +0x1100: `crg_social` / `crg_attack` / `crg_mix`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineupStyle {
    Social,
    Attack,
    Mix,
}

/// The five-tribe lineup of a player (VERIFIED, the controller's start-up
/// 0xCC6160): the key is chosen by the diet group (`clg_mix` 0x5ECE4770,
/// `clg_plant` 0xA8EC6F99, `clg_meat` 0xCFB01B93) and the Creature card
/// (`crg_mix` 0xB9DE15F2, `crg_social` 0xFD159D91, `crg_attack` 0x17E5EF84;
/// cPlayer +0x1100, which falls back to mix / social / attack for an omnivore /
/// herbivore / carnivore when it is 0):
///
/// ```text
///            mix         social      attack
/// omnivore   0x000DB6B9  0xF18A299D  0x8B4BF474
/// herbivore  0x8DA58D04  0x97A0E4C2  0xFB43C9A7
/// carnivore  0x4252B6FF  0x1631B0C3  0xF7426B5E
/// ```
///
/// Returned as the index into [`LINEUPS`] (the nine keys in ascending order).
pub fn lineup_for_player(diet: LineupDiet, style: Option<LineupStyle>) -> u8 {
    let style = style.unwrap_or(match diet {
        LineupDiet::Omnivore => LineupStyle::Mix,
        LineupDiet::Herbivore => LineupStyle::Social,
        LineupDiet::Carnivore => LineupStyle::Attack,
    });
    match (diet, style) {
        (LineupDiet::Omnivore, LineupStyle::Mix) => 0,      // 0x000DB6B9
        (LineupDiet::Carnivore, LineupStyle::Social) => 1,  // 0x1631B0C3
        (LineupDiet::Carnivore, LineupStyle::Mix) => 2,     // 0x4252B6FF
        (LineupDiet::Omnivore, LineupStyle::Attack) => 3,   // 0x8B4BF474
        (LineupDiet::Herbivore, LineupStyle::Mix) => 4,     // 0x8DA58D04
        (LineupDiet::Herbivore, LineupStyle::Social) => 5,  // 0x97A0E4C2
        (LineupDiet::Omnivore, LineupStyle::Social) => 6,   // 0xF18A299D
        (LineupDiet::Carnivore, LineupStyle::Attack) => 7,  // 0xF7426B5E
        (LineupDiet::Herbivore, LineupStyle::Attack) => 8,  // 0xFB43C9A7
    }
}

/// The tribe of tier `village + 1` in `lineup`: `(archetype, initial
/// relationship)`. The relationship is the listed number times ten (the event
/// table's scale).
pub fn lineup_tribe(lineup: u8, village: usize, difficulty: Difficulty) -> (&'static NpcTribe, f32) {
    let d = match difficulty {
        Difficulty::Easy => 0,
        Difficulty::Medium => 1,
        Difficulty::Hard => 2,
    };
    let row = &LINEUPS[d][lineup as usize % LINEUP_COUNT];
    let (archetype, relation) = row[village.min(4)];
    (
        npc_tribe(archetype).expect("every lineup archetype is in the table"),
        relation as f32 * 10.0,
    )
}

/// The archetype of `village` in `lineup` (difficulty only changes the
/// relationships).
pub fn village_tribe(lineup: u8, village: usize) -> &'static NpcTribe {
    lineup_tribe(lineup, village, Difficulty::Medium).0
}

/// Seconds a replaced NPC member takes: mating plus growing up (ADAPTED: the
/// NPC tribes breed back to strength with the retail timers).
pub fn villager_replace_seconds() -> f32 {
    MATING_TIME + TIME_BEFORE_BABY_GROWS_UP
}

// ---- epic creature (TRG_NPC_Epic_Predator)

/// Hit points, damage multiplier and size of the epic predator; its carcass is
/// worth 300 food and it returns after 1200 s.
pub const EPIC_HEALTH: f32 = 1000.0;
pub const EPIC_DAMAGE_SCALE: f32 = 15.0;
pub const EPIC_SIZE: f32 = 25.0;
pub const EPIC_FOOD: f32 = 300.0;
pub const EPIC_RESPAWN_TIME: f32 = 1200.0;
/// Distance from the stage origin it is placed at (variant 2: 100-200).
pub const EPIC_PLACEMENT: (f32, f32) = (175.0, 240.0);
pub const EPIC_PLACEMENT_VARIANT: (f32, f32) = (100.0, 200.0);

// ================================================================ T4 distances

/// Distances around the tribe (ability ranges, the tribe's zoning radius)
/// are retail units: the stage plays on the original's R = 500 planet
/// (planet-re 4E step 5; the island-scale 0.24 is gone). Kept as a named
/// factor so the scaled call sites stay marked.
pub const RANGE_SCALE: f32 = 1.0;
/// World layout distances (placement rings, herd `OriginDistance`, herd
/// casting) are retail units on the planet (`tribal_planet`; the island's
/// 0.12 is gone).
pub const LAYOUT_SCALE: f32 = 1.0;
/// The tribe's zoning radius (cTribe +0x330 = 30): home (recruit, recover)
/// and a rival village's reach for performing, gifting and raiding.
pub const TRIBE_ZONING_RADIUS: f32 = 30.0;

// ================================================================ T4 placement

/// One `noun_placement_markers~` record of the Tribal level generation list:
/// `distanceFromOrigin` and `WaterDistance` (min, max; -1 = unbounded) and the
/// exclusive radius around it (large group).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementMarker {
    pub name: &'static str,
    pub id: u32,
    pub distance_from_origin: (f32, f32),
    pub water_distance: (f32, f32),
    pub exclusive_radius: f32,
}

/// `playertribe` (0x03D7F90A): 40..60 from the origin, water 30..50, altitude
/// 0.5..35, slope <= 20, within 30 degrees of the equator.
pub const PLAYER_TRIBE_MARKER: PlacementMarker = PlacementMarker {
    name: "playertribe",
    id: 0x03D7_F90A,
    distance_from_origin: (40.0, 60.0),
    water_distance: (30.0, 50.0),
    exclusive_radius: 0.0,
};
/// `nearbyNPCTribe` (0xE45C5BBB): 175..225, same continent, water >= 40.
pub const NEARBY_NPC_TRIBE_MARKER: PlacementMarker = PlacementMarker {
    name: "nearbyNPCTribe",
    id: 0xE45C_5BBB,
    distance_from_origin: (175.0, 225.0),
    water_distance: (40.0, -1.0),
    exclusive_radius: 115.0,
};
/// `farNPCTribe` (0x631674DF): 175..350.
pub const FAR_NPC_TRIBE_MARKER: PlacementMarker = PlacementMarker {
    name: "farNPCTribe",
    id: 0x6316_74DF,
    distance_from_origin: (175.0, 350.0),
    water_distance: (40.0, -1.0),
    exclusive_radius: 115.0,
};
/// `fillerNPCTribe` (0xF2659F20): 200..450.
pub const FILLER_NPC_TRIBE_MARKER: PlacementMarker = PlacementMarker {
    name: "fillerNPCTribe",
    id: 0xF265_9F20,
    distance_from_origin: (200.0, 450.0),
    water_distance: (30.0, -1.0),
    exclusive_radius: 115.0,
};
/// `failsafeNPCTribe` (0x99945D8D): 200..500.
pub const FAILSAFE_NPC_TRIBE_MARKER: PlacementMarker = PlacementMarker {
    name: "failsafeNPCTribe",
    id: 0x9994_5D8D,
    distance_from_origin: (200.0, 500.0),
    water_distance: (30.0, -1.0),
    exclusive_radius: 115.0,
};

/// `numNPCTribes` (TribeGameLevelGenerationList).
pub const NUM_NPC_TRIBES: usize = 5;
/// Roads (TribeGameLevelGenerationList): `maxNumTribeRoads` 2, `roadWidth` 13,
/// `roadStartDistanceFromTribe` 15, `maxTribeRoadDistance` 225; forests are marked
/// (`markForests` 1). OpenSpore's island has no road or forest generator yet.
pub const MAX_NUM_TRIBE_ROADS: usize = 2;
pub const ROAD_WIDTH: f32 = 13.0;
pub const ROAD_START_DISTANCE_FROM_TRIBE: f32 = 15.0;
pub const MAX_TRIBE_ROAD_DISTANCE: f32 = 225.0;

/// The marker that places village `index` (one NPC tribe per tier, tier 1
/// first). ADAPTED assignment (the generator's order is not traced): the first
/// tribe is the nearby one, the next two far, then filler and failsafe.
pub fn village_marker(index: usize) -> &'static PlacementMarker {
    match index {
        0 => &NEARBY_NPC_TRIBE_MARKER,
        1 | 2 => &FAR_NPC_TRIBE_MARKER,
        3 => &FILLER_NPC_TRIBE_MARKER,
        _ => &FAILSAFE_NPC_TRIBE_MARKER,
    }
}

/// Distance of village `index` from the origin: the middle of its marker's
/// ring (the flat layout's distance; on the planet `tribal_planet` places
/// the tribe anywhere in the ring).
pub fn village_distance(index: usize) -> f32 {
    let (lo, hi) = village_marker(index).distance_from_origin;
    (lo + hi) * 0.5 * LAYOUT_SCALE
}

// ===================================================================== T4 herds

/// A herd archetype of the Tribal stage (`herdtypes~!TRG_*`): creatures per
/// herd, `HitpointOverride`, `OverrideSize`, `foodValue` (food of a carcass),
/// `RespawnRate` and `OriginDistance`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HerdArchetype {
    pub name: &'static str,
    pub creatures: usize,
    pub health: f32,
    pub size: f32,
    pub food: f32,
    /// Seconds before a lost creature is replaced (0 = never).
    pub respawn: f32,
    pub origin_distance: (f32, f32),
    pub predator: bool,
    pub epic: bool,
    /// `DamageMultiplier`.
    pub damage_scale: f32,
    /// `TerritoryRadius` (cHerd +0x108, copied from the record at 0xC6E460):
    /// how far from its home the herd's creatures range.
    pub territory_radius: f32,
    /// `baseSightRadius` (-1 = the species' own).
    pub sight_radius: f32,
    /// `attacked_SpeciesHelpRadius`.
    pub help_radius: f32,
    /// Lounge groups and guards (T9E).
    pub groups: HerdGroups,
}

/// How a herd spreads around its nest (T9E, VERIFIED 0xD81FC0 / 0xD8DC30): a
/// member is a guard ("Guard" group 0x4472A59) at one of `guard_locations`
/// posts, or lounges in a group ("Lounge" 0x439E33E) of
/// `round(random(group_size))` members around the group's spot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HerdGroups {
    /// `groupSizeRange`.
    pub group_size: (f32, f32),
    /// `percentGuards`.
    pub percent_guards: f32,
    /// `NumGuardLocations`, `guardsPerLocation`.
    pub guard_locations: u32,
    pub guards_per_location: u32,
    /// `guardLocationRadiusRange`, `guardPositionDurationRange`.
    pub guard_radius: (f32, f32),
    pub guard_duration: (f32, f32),
}

const GROUPS_SMALL: HerdGroups = HerdGroups {
    group_size: (2.0, 2.0),
    percent_guards: 0.3,
    guard_locations: 3,
    guards_per_location: 1,
    guard_radius: (10.0, 15.0),
    guard_duration: (15.0, 30.0),
};
const GROUPS_MEDIUM: HerdGroups = HerdGroups {
    group_size: (2.0, 3.0),
    percent_guards: 0.3,
    guard_locations: 4,
    guards_per_location: 1,
    guard_radius: (10.0, 15.0),
    guard_duration: (15.0, 30.0),
};
const GROUPS_LARGE: HerdGroups = HerdGroups {
    group_size: (2.0, 4.0),
    percent_guards: 0.3,
    guard_locations: 4,
    guards_per_location: 2,
    guard_radius: (10.0, 15.0),
    guard_duration: (15.0, 30.0),
};
/// The epic predator (no guard posts) and the scavenger (one creature).
const GROUPS_SOLO: HerdGroups = HerdGroups {
    group_size: (4.0, 6.0),
    percent_guards: 0.3,
    guard_locations: 0,
    guards_per_location: 1,
    guard_radius: (10.0, 15.0),
    guard_duration: (60.0, 120.0),
};

/// `RespawnRate` of every Tribal prey and predator herd (VERIFIED: dumped
/// from the installed `herdtypes~` records TRG_NPC_Small_Prey / Medium_Prey /
/// Large_Prey / Large_Prey2 / Large_Predator / 2 / 3 incl. the easy and hard
/// variants: all 120 s; the epic 1200 s; TRG_Food_Scavenger 100000 s = never).
pub const TRIBAL_HERD_RESPAWN: f32 = 120.0;

pub const TRG_SMALL_PREY: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Small_Prey",
    creatures: 5,
    health: 15.0,
    size: 3.0,
    food: 5.0,
    respawn: 120.0,
    origin_distance: (70.0, 120.0),
    predator: false,
    epic: false,
    damage_scale: 1.0,
    territory_radius: 125.0,
    sight_radius: -1.0,
    help_radius: 0.0,
    groups: GROUPS_SMALL,
};
pub const TRG_MEDIUM_PREY: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Medium_Prey",
    creatures: 6,
    health: 25.0,
    size: 6.0,
    food: 10.0,
    respawn: TRIBAL_HERD_RESPAWN,
    origin_distance: (70.0, 120.0),
    predator: false,
    epic: false,
    damage_scale: 1.0,
    territory_radius: 200.0,
    sight_radius: -1.0,
    help_radius: 0.0,
    groups: GROUPS_MEDIUM,
};
pub const TRG_LARGE_PREY: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Large_Prey",
    creatures: 7,
    health: 40.0,
    size: 9.0,
    food: 15.0,
    respawn: TRIBAL_HERD_RESPAWN,
    origin_distance: (75.0, 105.0),
    predator: false,
    epic: false,
    damage_scale: 1.0,
    territory_radius: 200.0,
    sight_radius: -1.0,
    help_radius: 0.0,
    groups: GROUPS_LARGE,
};
/// `TRG_NPC_Large_Predator2` (90..200 from the origin).
pub const TRG_LARGE_PREDATOR: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Large_Predator2",
    creatures: 7,
    health: 40.0,
    size: 9.0,
    food: 20.0,
    respawn: TRIBAL_HERD_RESPAWN,
    origin_distance: (90.0, 200.0),
    predator: true,
    epic: false,
    damage_scale: 1.0,
    territory_radius: 200.0,
    sight_radius: -1.0,
    help_radius: 25.0,
    groups: GROUPS_LARGE,
};
/// `TRG_NPC_Epic_Predator` (T3C): HP 1000, damage x15, size 25, food 300,
/// respawn 1200 s, 175..240 from the origin (variant 2: 100..200), no nest.
pub const TRG_EPIC_PREDATOR: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Epic_Predator",
    creatures: 1,
    health: EPIC_HEALTH,
    size: EPIC_SIZE,
    food: EPIC_FOOD,
    respawn: EPIC_RESPAWN_TIME,
    origin_distance: EPIC_PLACEMENT_VARIANT,
    predator: true,
    epic: true,
    damage_scale: EPIC_DAMAGE_SCALE,
    territory_radius: 50.0,
    sight_radius: 10.0,
    help_radius: 10.0,
    groups: GROUPS_SOLO,
};
/// `TRG_Food_Scavenger`: one creature, HP 15, damage x0.75, food 5, never respawns.
pub const TRG_FOOD_SCAVENGER: HerdArchetype = HerdArchetype {
    name: "TRG_Food_Scavenger",
    creatures: 1,
    health: 15.0,
    size: 3.0,
    food: 5.0,
    respawn: 0.0,
    origin_distance: (10.0, 15.0),
    predator: false,
    epic: false,
    damage_scale: 0.75,
    territory_radius: 100.0,
    sight_radius: -1.0,
    help_radius: 0.0,
    groups: GROUPS_SOLO,
};

/// `trg_npc_large_predator3` (forestSpeciesRules 0x7DF62D7E): a large
/// predator herd whose nest stands 5..10 from a forest (T9F).
pub const TRG_LARGE_PREDATOR3: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Large_Predator3",
    creatures: 7,
    health: 40.0,
    size: 9.0,
    food: 20.0,
    respawn: TRIBAL_HERD_RESPAWN,
    origin_distance: (5.0, 10.0),
    predator: true,
    epic: false,
    damage_scale: 1.0,
    territory_radius: 200.0,
    sight_radius: -1.0,
    help_radius: 25.0,
    groups: GROUPS_LARGE,
};
/// `trg_npc_large_prey2` (forestSpeciesRules 0xFA99A31F): a large prey herd
/// 40..50 from a forest.
pub const TRG_LARGE_PREY2: HerdArchetype = HerdArchetype {
    name: "TRG_NPC_Large_Prey2",
    creatures: 7,
    health: 40.0,
    size: 9.0,
    food: 15.0,
    respawn: TRIBAL_HERD_RESPAWN,
    origin_distance: (40.0, 50.0),
    predator: false,
    epic: false,
    damage_scale: 1.0,
    territory_radius: 200.0,
    sight_radius: -1.0,
    help_radius: 0.0,
    groups: GROUPS_LARGE,
};

/// `forestSpeciesRules` of the Tribal generation list (VERIFIED 0xB9CBF0 ->
/// 0xB9C980 -> 0xB9AB60): at every forest within
/// `maxForestDistanceForNounGeneration` of the player's tribe on its
/// continent, each species places one nest with chance `PlacementPercentage`
/// x the difficulty factor 0x1EE4CAFD (1, 1, 1 for both), keeping its
/// `exclusiveAreaRadiusGroupSmall` (40 / 60) clear of other herds.
pub struct ForestRule {
    pub archetype: &'static HerdArchetype,
    pub chance: f32,
    pub exclusive_radius: f32,
}
pub const FOREST_SPECIES_RULES: [ForestRule; 2] = [
    ForestRule { archetype: &TRG_LARGE_PREDATOR3, chance: 0.3, exclusive_radius: 40.0 },
    ForestRule { archetype: &TRG_LARGE_PREY2, chance: 0.5, exclusive_radius: 60.0 },
];
/// `maxForestDistanceForNounGeneration`.
pub const MAX_FOREST_DISTANCE_FOR_NOUN_GENERATION: f32 = 500.0;
/// `forestRadiusIncrease_SmallGroup / _MediumGroup / _LargeGroup` (markForests:
/// a forest is a placement obstacle this much wider for each group size).
pub const FOREST_RADIUS_INCREASE: (f32, f32, f32) = (0.0, 1.0, 10.0);
/// INFERRED radius of a forest (the grove's large square half width, 24;
/// planet-flora F5).
pub const FOREST_RADIUS: f32 = 24.0;
/// Herd indices from this one are forest herds: `FOREST_HERD_BASE + 2 x
/// instance + rule`.
pub const FOREST_HERD_BASE: usize = 100;

/// The archetype of herd index `index` (the generation list's herds, then the
/// forest herds).
pub fn herd_archetype(index: usize) -> &'static HerdArchetype {
    if index >= FOREST_HERD_BASE {
        FOREST_SPECIES_RULES[(index - FOREST_HERD_BASE) % FOREST_SPECIES_RULES.len()].archetype
    } else {
        TRIBAL_HERDS[index.min(TRIBAL_HERDS.len() - 1)]
    }
}

/// The herds TribeGameLevelGenerationList places: two medium prey herds, two
/// epic predators, two large prey herds and one large predator herd.
pub const TRIBAL_HERDS: [&HerdArchetype; 7] = [
    &TRG_MEDIUM_PREY,
    &TRG_MEDIUM_PREY,
    &TRG_EPIC_PREDATOR,
    &TRG_EPIC_PREDATOR,
    &TRG_LARGE_PREY,
    &TRG_LARGE_PREY,
    &TRG_LARGE_PREDATOR,
];

/// Wild creatures of the generation list that are not epic (33).
pub const fn herd_creatures() -> usize {
    let mut total = 0;
    let mut i = 0;
    while i < TRIBAL_HERDS.len() {
        if !TRIBAL_HERDS[i].epic {
            total += TRIBAL_HERDS[i].creatures;
        }
        i += 1;
    }
    total
}

/// Distance of a herd's home from the tribe: the middle of its
/// `OriginDistance` at [`LAYOUT_SCALE`].
pub fn herd_distance(herd: &HerdArchetype) -> f32 {
    let (lo, hi) = herd.origin_distance;
    (lo + hi) * 0.5 * LAYOUT_SCALE
}

/// ADAPTED: the body scale of a herd creature from its `OverrideSize` (a size-6
/// grazer is drawn at about the 0.55 OpenSpore always used, the size-25 epic at
/// 2.25).
pub fn herd_body_scale(herd: &HerdArchetype) -> f32 {
    herd.size * 0.09
}

/// One respawn tick of a herd short of creatures: `(timer, true when one
/// creature returns)`. A herd with `respawn` 0 never refills.
pub fn herd_respawn_step(timer: f32, dt: f32, missing: usize, respawn: f32) -> (f32, bool) {
    if missing == 0 || respawn <= 0.0 || !dt.is_finite() || dt <= 0.0 {
        return (0.0, false);
    }
    let timer = timer + dt;
    if timer >= respawn {
        (0.0, true)
    } else {
        (timer, false)
    }
}

/// A herd member's place in its herd (T9E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HerdRole {
    /// Guard at post `post` (of `guard_locations`).
    Guard { post: u32 },
    /// Lounger in group `group`.
    Lounge { group: u32 },
}

/// Deterministic unit value for herd layouts.
fn herd_unit(key: u64, i: u64) -> f32 {
    let mut x = key ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The roles of a herd's `creatures` members: the first are the guards
/// (INFERRED count `round(percentGuards x creatures)`, at most
/// `guard_locations x guards_per_location`, filling the posts in turn), the
/// rest lounge in groups of `round(random(groupSizeRange))` (at least 1),
/// drawn in order (`key` makes the draws repeatable).
pub fn herd_layout(groups: &HerdGroups, creatures: usize, key: u64) -> Vec<HerdRole> {
    let cap = (groups.guard_locations * groups.guards_per_location) as usize;
    let guards = ((groups.percent_guards * creatures as f32).round() as usize).min(cap).min(creatures);
    let mut out = Vec::with_capacity(creatures);
    for g in 0..guards {
        out.push(HerdRole::Guard { post: g as u32 % groups.guard_locations.max(1) });
    }
    let (lo, hi) = groups.group_size;
    let mut group = 0u32;
    let mut left_in_group = 0usize;
    for _ in guards..creatures {
        if left_in_group == 0 {
            let size = (lo + (hi - lo) * herd_unit(key, group as u64)).round().max(1.0) as usize;
            left_in_group = size;
            if !out.is_empty() && matches!(out.last(), Some(HerdRole::Lounge { .. })) {
                group += 1;
            }
        }
        out.push(HerdRole::Lounge { group });
        left_in_group -= 1;
    }
    out
}

/// A guard's post `post` around the nest (evenly spaced, the radius drawn in
/// `guardLocationRadiusRange`) and how long it holds it
/// (`guardPositionDurationRange`); `roll` in 0..1.
pub fn guard_post(groups: &HerdGroups, nest: Vec3, post: u32, phase: f32, roll: f32) -> (Vec3, f32) {
    let n = groups.guard_locations.max(1) as f32;
    let angle = phase + post as f32 / n * std::f32::consts::TAU;
    let (lo, hi) = groups.guard_radius;
    let radius = (lo + (hi - lo) * roll.clamp(0.0, 1.0)) * LAYOUT_SCALE;
    let (wlo, whi) = groups.guard_duration;
    (
        nest + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius),
        wlo + (whi - wlo) * roll.clamp(0.0, 1.0),
    )
}

/// The species' personal-space factor of the group spot (species profile
/// +0x384; 0xD8D790 uses 1.25 when the species has no profile record).
pub const GROUP_PERSONAL_SPACE: f32 = 1.25;
/// ADAPTED: a herd creature's `GetFootprintRadius` (cSpatialObject +0x74) is
/// half its `OverrideSize`; the nest model's footprint radius is 3.
pub fn creature_footprint_radius(herd: &HerdArchetype) -> f32 {
    0.5 * herd.size
}
pub const NEST_FOOTPRINT_RADIUS: f32 = 3.0;

/// The ring a lounge group's spot is drawn from (VERIFIED 0xD8D790, the
/// group-join step of 0xD8DC30): a point at a random distance between `inner`
/// and `inner + outer` from the nest, where `inner` = nest footprint radius +
/// the creature's footprint radius and `outer` = `sqrt(members x (radius x
/// personal space)^2 + inner^2)`; random bearing (`cPlanetModel` 0xB818D0
/// takes the two distances and a centre). The footprint radii are ADAPTED
/// ([`creature_footprint_radius`], [`NEST_FOOTPRINT_RADIUS`]).
pub fn lounge_ring(herd: &HerdArchetype, members: usize) -> (f32, f32) {
    let a = creature_footprint_radius(herd);
    let inner = NEST_FOOTPRINT_RADIUS + a;
    let space = a * GROUP_PERSONAL_SPACE;
    let outer = (members.max(1) as f32 * space * space + inner * inner).sqrt();
    (inner * LAYOUT_SCALE, (inner + outer) * LAYOUT_SCALE)
}

/// Where lounge group `group` of `members` gathers: `roll_distance` and
/// `roll_bearing` in 0..1 pick the point on [`lounge_ring`].
pub fn lounge_spot(
    nest: Vec3,
    herd: &HerdArchetype,
    members: usize,
    roll_distance: f32,
    roll_bearing: f32,
) -> Vec3 {
    let (lo, hi) = lounge_ring(herd, members);
    let radius = lo + (hi - lo) * roll_distance.clamp(0.0, 1.0);
    let angle = roll_bearing.clamp(0.0, 1.0) * std::f32::consts::TAU;
    nest + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
}

/// The members of lounge group `group` in a herd's layout.
pub fn lounge_group_size(layout: &[HerdRole], group: u32) -> usize {
    layout.iter().filter(|r| matches!(r, HerdRole::Lounge { group: g } if *g == group)).count()
}

/// `CastingManager_DistanceThresholds` (100, 150): herds come alive within 100
/// of the player's units and freeze beyond 150 (`MaxAnimalPoolSize` 100).
pub const CASTING_DISTANCE_THRESHOLDS: (f32, f32) = (100.0, 150.0);
pub const MAX_ANIMAL_POOL_SIZE: usize = 100;

/// Activation / deactivation distances in OpenSpore units.
pub fn herd_activation_distances() -> (f32, f32) {
    (
        CASTING_DISTANCE_THRESHOLDS.0 * LAYOUT_SCALE,
        CASTING_DISTANCE_THRESHOLDS.1 * LAYOUT_SCALE,
    )
}

/// Whether a herd creature is active, given whether it was and the distance
/// to the nearest unit of the tribe (hysteresis between the two thresholds).
pub fn herd_active(was_active: bool, nearest_unit: f32) -> bool {
    let (on, off) = herd_activation_distances();
    if was_active {
        nearest_unit <= off
    } else {
        nearest_unit <= on
    }
}

/// ADAPTED: the bite of a Tribal herd predator. Its species is chosen with a
/// total attack of 11..20 (herd record 0x55014D21), a level-3 bite in the
/// Creature tables (`creature_rules::BITE_DAMAGE`: 2 damage every 1.5 s); the
/// herd `DamageMultiplier` scales it (x15 for the epics).
pub fn predator_dps(herd: &HerdArchetype) -> f32 {
    crate::creature_rules::BITE_DAMAGE[2] / crate::creature_rules::BITE_REFRESH * herd.damage_scale
}

/// The epic predator's blow: the predator bite times `DamageMultiplier` 15.
pub fn epic_dps() -> f32 {
    predator_dps(&TRG_EPIC_PREDATOR)
}

// ==================================================================== plugin

/// Marks the chieftain while he is down (hidden at the hut until he respawns).
#[derive(Component, Debug, Clone, Copy)]
pub struct ChieftainDown;

pub struct TribalRulesPlugin;

impl Plugin for TribalRulesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                crate::tribal::tribal_tribes_appear.after(crate::tribal::tribal_campaign_update),
                crate::tribal::tribal_chieftain_respawn.after(crate::tribal_rts::tribal_combat),
                crate::tribal::tribal_power_effects.after(crate::tribal::tribal_campaign_update),
                crate::tribal_path::village_ground_follow_level,
                crate::tribal_plans::npc_planner
                    .after(crate::tribal_rts::tribal_wildlife)
                    .before(crate::tribal_rts::tribal_rivals),
            )
                .in_set(crate::GameSystems::Simulation)
                .run_if(in_state(GameStage::Tribal)),
        )
        // Members and raiders over water float at the surface (tribal-swim):
        // after grounding, with grounding's own run conditions.
        .add_systems(
            Update,
            crate::tribal_swim::swim_units
                .after(crate::GameSystems::Grounding)
                .before(crate::GameSystems::Presentation)
                .run_if(crate::editor::simulation_active)
                .run_if(crate::mechanical_creator::simulation_active)
                .run_if(crate::pause_menu::simulation_active)
                .run_if(in_state(GameStage::Tribal)),
        )
        .add_systems(
            Update,
            crate::tribal_swim::swim_test
                .in_set(crate::GameSystems::Simulation)
                .run_if(in_state(GameStage::Tribal)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn herds_split_into_guards_and_lounge_groups() {
        // Medium prey: 6 creatures, 30% guards -> 2 guards on posts 0 and 1,
        // four loungers in groups of round(random(2, 3)).
        let layout = herd_layout(&TRG_MEDIUM_PREY.groups, 6, 7);
        assert_eq!(layout.len(), 6);
        assert_eq!(&layout[..2], &[HerdRole::Guard { post: 0 }, HerdRole::Guard { post: 1 }]);
        let mut sizes = std::collections::BTreeMap::new();
        for role in &layout[2..] {
            match role {
                HerdRole::Lounge { group } => *sizes.entry(*group).or_insert(0) += 1,
                other => panic!("{other:?}"),
            }
        }
        // Every group but the last has 2 or 3 members.
        let counts: Vec<usize> = sizes.values().copied().collect();
        for c in &counts[..counts.len() - 1] {
            assert!((2..=3).contains(c), "{counts:?}");
        }
        assert_eq!(counts.iter().sum::<usize>(), 4);
        // Large herds: 7 creatures, 2 guards, groups of 2..4.
        for key in 0..20 {
            let layout = herd_layout(&TRG_LARGE_PREY.groups, 7, key);
            assert_eq!(layout.iter().filter(|r| matches!(r, HerdRole::Guard { .. })).count(), 2);
        }
        // The epic has no guard posts and lounges alone.
        assert_eq!(herd_layout(&TRG_EPIC_PREDATOR.groups, 1, 3), vec![HerdRole::Lounge { group: 0 }]);
        // Guard posts ring the nest at 10..15 and are held 15..30 s.
        let nest = Vec3::new(50.0, 0.0, 0.0);
        for post in 0..4 {
            let (at, hold) = guard_post(&TRG_MEDIUM_PREY.groups, nest, post, 0.3, 0.5);
            assert!((dist(at, nest) - 12.5).abs() < 1e-3);
            assert_eq!(hold, 22.5);
        }
        // A lounge group gathers on the ring of 0xD8D790 around the nest: from
        // the nest's and the creature's radii out to that plus the group's
        // spread.
        let herd = &TRG_MEDIUM_PREY;
        let (lo, hi) = lounge_ring(herd, 3);
        assert!(lo > NEST_FOOTPRINT_RADIUS && hi > lo, "{lo} {hi}");
        for (rd, rb) in [(0.0, 0.0), (0.5, 0.3), (1.0, 0.9)] {
            let d = dist(lounge_spot(nest, herd, 3, rd, rb), nest);
            assert!(d >= lo - 1e-3 && d <= hi + 1e-3, "{d} not in {lo}..{hi}");
        }
        // More members spread the ring wider (sqrt of the group size).
        assert!(lounge_ring(herd, 4).1 > lounge_ring(herd, 2).1);
    }

    fn dist(a: Vec3, b: Vec3) -> f32 {
        Vec2::new(a.x - b.x, a.z - b.z).length()
    }

    #[test]
    fn tribe_level_rises_at_one_and_three_tribes() {
        // 0xCDA4F0 with goalProgressUpgrades [1, 3].
        let levels: Vec<u8> = (0..=5).map(tribe_level).collect();
        assert_eq!(levels, vec![0, 1, 1, 2, 2, 2]);
        assert_eq!(hut_level(0), 1);
        assert_eq!(hut_level(1), 2);
        assert_eq!(hut_level(2), 2);
        assert_eq!(hut_level(3), 3);
        assert_eq!(level_reached(0, 1), Some(2));
        assert_eq!(level_reached(1, 2), None);
        assert_eq!(level_reached(2, 3), Some(3));
        assert_eq!(level_reached(3, 4), None);
    }

    #[test]
    fn hut_health_follows_the_tribe_level() {
        assert_eq!(hut_health(1), 400.0);
        assert_eq!(hut_health(2), 750.0);
        assert_eq!(hut_health(3), 1000.0);
        // Out-of-range levels clamp instead of panicking.
        assert_eq!(hut_health(0), 400.0);
        assert_eq!(hut_health(9), 1000.0);
    }

    #[test]
    fn progress_counts_tribes_and_the_stage_ends_at_five() {
        assert_eq!(NUM_TRIBES_TO_CONQUER, 5);
        assert!(close(progress_fraction(0), 0.0));
        assert!(close(progress_fraction(2), 0.4));
        assert!(close(progress_fraction(5), 1.0));
        assert!(close(progress_fraction(9), 1.0), "min(progress, goal) / goal");
        assert!(!stage_complete(4));
        assert!(stage_complete(5));
    }

    #[test]
    fn the_tribe_starts_with_three_members_and_fifteen_food() {
        assert_eq!(TRIBE_POPULATION_START, 3);
        assert_eq!(NEW_TRIBE_FOOD, 15.0);
        assert_eq!(start_food(&GameStage::Tribal), 15.0);
        assert_eq!(start_food(&GameStage::Creature), 0.0);
        assert_eq!(start_food(&GameStage::Civilization), 0.0);
        assert_eq!(TRIBE_MAX_FOOD, 9999.0);
        assert_eq!(TRIBE_STARTING_TOOL_TYPES, [3, 4]);
    }

    #[test]
    fn the_hut_houses_more_members_at_each_level() {
        let caps: Vec<usize> = (0..=5).map(population_cap).collect();
        assert_eq!(caps, vec![6, 9, 9, 12, 12, 12]);
    }

    #[test]
    fn the_chieftain_is_bigger_tougher_and_returns_after_ninety_seconds() {
        assert_eq!(CHIEFTAIN_SCALE, 1.25);
        assert_eq!(CHIEFTAIN_HEALTH_SCALE, 1.5);
        assert_eq!(CHIEFTAIN_DAMAGE_SCALE, 1.0);
        assert_eq!(CHIEFTAIN_RESPAWN_TIME, 90.0);
        assert_eq!(CHIEFTAIN_DEFEND_DISTANCES, (5.0, 20.0));
        assert!(close(chieftain_health(80.0), 120.0));
        assert!(close(chieftain_body_scale(1.0), 1.25));
        // The respawn timer counts down and fires once.
        let (left, back) = chieftain_respawn_step(CHIEFTAIN_RESPAWN_TIME, 30.0);
        assert!(close(left, 60.0) && !back);
        let (left, back) = chieftain_respawn_step(0.5, 1.0);
        assert!(left == 0.0 && back);
        let (left, back) = chieftain_respawn_step(0.0, 1.0);
        assert!(left == 0.0 && !back, "not waiting");
        let (left, back) = chieftain_respawn_step(f32::NAN, 1.0);
        assert!(left == 0.0 && !back);
    }

    #[test]
    fn npc_tribes_appear_by_the_retail_controller_rules() {
        let input = |npc_tribes, resolved, food, baby, tutorials| AppearanceInput {
            npc_tribes,
            resolved,
            food,
            baby,
            tutorials,
            difficulty: Difficulty::Medium,
        };
        assert_eq!(TRIBES_AT_START, 0, "the first tribe appears after the start");
        assert_eq!(first_tribe_baby_delay(Difficulty::Medium), 90.0);
        assert_eq!(first_tribe_baby_delay(Difficulty::Hard), 60.0);
        // Tutorials on, nothing done yet: no timer runs.
        let mut s = AppearanceState::default();
        for _ in 0..1000 {
            assert!(!s.step(1.0, &input(0, 0, 15.0, false, true)));
        }
        assert_eq!(s.baby_timer, None);
        assert_eq!(s.food_timer, None);
        // 40 food starts the food timer; the tribe comes 25 s later.
        let mut s = AppearanceState::default();
        assert!(!s.step(1.0, &input(0, 0, 40.0, false, true)));
        assert!(s.food_seen && s.food_timer.is_some());
        let mut seconds = 0;
        while !s.step(1.0, &input(0, 0, 10.0, false, true)) {
            seconds += 1;
            assert!(seconds < 100);
        }
        assert!((24..=26).contains(&seconds), "{seconds}");
        // The first baby starts the 90 s timer.
        let mut s = AppearanceState::default();
        s.step(1.0, &input(0, 0, 0.0, true, true));
        assert!(s.baby_timer.is_some() && s.food_timer.is_none());
        let mut seconds = 0;
        while !s.step(1.0, &input(0, 0, 0.0, true, true)) {
            seconds += 1;
            assert!(seconds < 200);
        }
        assert!((89..=91).contains(&seconds), "{seconds}");
        // Tutorials off: both timers run from the start, the 25 s one wins.
        let mut s = AppearanceState::default();
        let mut seconds = 0;
        while !s.step(1.0, &input(0, 0, 0.0, false, false)) {
            seconds += 1;
            assert!(seconds < 100);
        }
        assert!((25..=27).contains(&seconds), "{seconds}");
        // The second tribe: a tribe settled, or more than 100 food.
        let mut s = AppearanceState::default();
        assert!(!s.step(1.0, &input(1, 0, 100.0, true, true)), "100 is not above 100");
        assert!(s.step(1.0, &input(1, 0, 101.0, true, true)));
        let mut s = AppearanceState::default();
        assert!(s.step(1.0, &input(1, 1, 0.0, true, true)));
        // The third and fourth follow one second apart.
        assert!(!s.step(0.5, &input(2, 1, 0.0, true, true)), "spacing after a creation");
        assert!(s.step(0.6, &input(2, 1, 0.0, true, true)));
        assert!(s.step(1.1, &input(3, 1, 0.0, true, true)));
        // The fifth waits for three settled tribes; five are the end.
        assert!(!s.step(2.0, &input(4, 2, 500.0, true, true)));
        assert!(s.step(1.0, &input(4, 3, 0.0, true, true)));
        assert!(!s.step(2.0, &input(5, 5, 500.0, true, true)), "all five are out");
        // The state survives a save.
        let json = serde_json::to_string(&s).expect("serialize");
        assert_eq!(serde_json::from_str::<AppearanceState>(&json).expect("load"), s);
        assert_eq!(serde_json::from_str::<AppearanceState>("{}").expect("old save"), AppearanceState::default());
    }

    #[test]
    fn the_lineup_follows_the_diet_and_the_creature_card() {
        // 0xCC6160: nine keys = three diet groups x three Creature cards; the
        // index is the key's place in ascending order (the LINEUPS rows).
        let keys = [
            0x000D_B6B9u32, 0x1631_B0C3, 0x4252_B6FF, 0x8B4B_F474, 0x8DA5_8D04, 0x97A0_E4C2,
            0xF18A_299D, 0xF742_6B5E, 0xFB43_C9A7,
        ];
        let mut sorted = keys;
        sorted.sort_unstable();
        assert_eq!(sorted, keys);
        use LineupDiet::*;
        use LineupStyle::*;
        let table = [
            (Omnivore, Mix, 0x000D_B6B9u32),
            (Omnivore, Social, 0xF18A_299D),
            (Omnivore, Attack, 0x8B4B_F474),
            (Herbivore, Mix, 0x8DA5_8D04),
            (Herbivore, Social, 0x97A0_E4C2),
            (Herbivore, Attack, 0xFB43_C9A7),
            (Carnivore, Mix, 0x4252_B6FF),
            (Carnivore, Social, 0x1631_B0C3),
            (Carnivore, Attack, 0xF742_6B5E),
        ];
        let mut seen = [false; LINEUP_COUNT];
        for (diet, style, key) in table {
            let index = lineup_for_player(diet, Some(style)) as usize;
            assert_eq!(keys[index], key, "{diet:?} {style:?}");
            assert!(!seen[index]);
            seen[index] = true;
        }
        // No card yet (cPlayer +0x1100 = 0): the diet's own card.
        assert_eq!(lineup_for_player(Omnivore, None), lineup_for_player(Omnivore, Some(Mix)));
        assert_eq!(lineup_for_player(Herbivore, None), lineup_for_player(Herbivore, Some(Social)));
        assert_eq!(lineup_for_player(Carnivore, None), lineup_for_player(Carnivore, Some(Attack)));
    }

    #[test]
    fn the_city_hall_card_follows_the_average_relationship() {
        // 0xCDBA30 / 0xCDBBD0: score = -sum(clamp) / (5 x bound), cut at +-1/3.
        let allied = [100.0; 5];
        let conquered = [-100.0; 5];
        assert!(close(play_style_score(&allied), -1.0));
        assert!(close(play_style_score(&conquered), 1.0));
        assert_eq!(play_style_from_relations(&allied), PlayStyle::Religious);
        assert_eq!(play_style_from_relations(&conquered), PlayStyle::Military);
        // Mixed endings are the economic card.
        let mixed = [100.0, 100.0, 100.0, -100.0, -100.0];
        assert!(close(play_style_score(&mixed), -0.2));
        assert_eq!(play_style_from_relations(&mixed), PlayStyle::Economic);
        // Four allied tribes still read as the social card, four conquered as attack.
        assert_eq!(
            play_style_from_relations(&[100.0, 100.0, 100.0, 100.0, -100.0]),
            PlayStyle::Religious
        );
        assert_eq!(
            play_style_from_relations(&[-100.0, -100.0, -100.0, -100.0, 100.0]),
            PlayStyle::Military
        );
        // Values past the bound clamp; non-finite values count as neutral.
        assert!(close(play_style_score(&[9999.0; 5]), -1.0));
        assert!(close(play_style_score(&[f32::NAN, 100.0]), -0.2));
        // 0xCDBA30 clamps each relationship between the attitude caps -7 / +7
        // (x10: +-70) and divides by 5 x 7: five tribes at +35 (3.5 raw) read
        // -0.5, a tribe at the attitude's own +-100 clamp counts as +-70.
        assert!(close(play_style_score(&[35.0; 5]), -0.5));
        assert!(close(play_style_score(&[70.0; 5]), -1.0));
        assert!(close(play_style_score(&[-100.0, 0.0, 0.0, 0.0, 0.0]), 70.0 / 350.0));
        // Exactly one third switches the card.
        assert_eq!(play_style(PLAY_STYLE_THRESHOLD), PlayStyle::Military);
        assert_eq!(play_style(-PLAY_STYLE_THRESHOLD), PlayStyle::Economic);
        assert_eq!(play_style(-0.34), PlayStyle::Religious);
        assert_eq!(play_style(0.0), PlayStyle::Economic);
    }

    #[test]
    fn card_ids_are_the_hashes_of_the_original_names() {
        use crate::world_objects::name_id;
        assert_eq!(PlayStyle::Military.card_id(), name_id("trg_attack"));
        assert_eq!(PlayStyle::Economic.card_id(), name_id("trg_mix"));
        assert_eq!(PlayStyle::Religious.card_id(), name_id("trg_social"));
        assert_eq!(PlayStyle::Military.trait_id(), name_id("trg_earnedtraitattack"));
        assert_eq!(PlayStyle::Economic.trait_id(), name_id("trg_earnedtraitmix"));
        assert_eq!(PlayStyle::Religious.trait_id(), name_id("trg_earnedtraitsocial"));
        assert_eq!(PlayStyle::Military.card(), "MILI");
        assert_eq!(PlayStyle::Economic.card(), "ECON");
        assert_eq!(PlayStyle::Religious.card(), "RELI");
        assert_eq!(cinematics::UPGRADE, "TRG_Upgrade");
        assert_eq!(cinematics::SHOW_CITY_HALL, "TRG2CVG_ShowCityHall");
    }

    #[test]
    fn tool_prices_health_and_rack_slots() {
        assert_eq!(tool_price(ToolKind::SpikedClubs), 25.0);
        assert_eq!(tool_price(ToolKind::StoneAxes), 25.0);
        assert_eq!(tool_price(ToolKind::ThrowingSpears), 25.0);
        assert_eq!(tool_price(ToolKind::WoodenHorns), 25.0);
        assert_eq!(tool_price(ToolKind::Maracas), 25.0);
        assert_eq!(tool_price(ToolKind::Aerophones), 25.0);
        assert_eq!(tool_price(ToolKind::FruitSticks), 20.0);
        assert_eq!(tool_price(ToolKind::FishingSpears), 20.0);
        assert_eq!(tool_price(ToolKind::HealingRods), 20.0);
        assert_eq!(tool_price(ToolKind::Campfire), 15.0);
        assert_eq!(DEFAULT_TOOL_PRICE, 10.0);
        assert_eq!(tool_health(ToolKind::StoneAxes), 200.0);
        assert_eq!(tool_health(ToolKind::HealingRods), 200.0);
        assert_eq!(tool_health(ToolKind::Campfire), 100.0, "the camp fire keeps the default");
        assert_eq!(MAX_TOOLS_PER_RACK, 6);
        assert!(rack_has_room(5));
        assert!(!rack_has_room(6));
        // The tool type numbers are the ten distinct values 1..=10.
        let mut types: Vec<u8> = crate::tribal_tools::TOOLS.iter().map(|t| tool_type(t.kind)).collect();
        types.sort();
        assert_eq!(types, (1..=10).collect::<Vec<u8>>());
        // The catalog of tribal_tools agrees on every price.
        for t in &crate::tribal_tools::TOOLS {
            assert_eq!(t.price, tool_price(t.kind), "{}", t.name);
        }
    }

    #[test]
    fn damaged_tools_switch_models_at_a_third_and_two_thirds() {
        assert_eq!(damage_model(1.0), DamageModel::Intact);
        assert_eq!(damage_model(0.67), DamageModel::Intact);
        assert_eq!(damage_model(0.66), DamageModel::Light);
        assert_eq!(damage_model(0.34), DamageModel::Light);
        assert_eq!(damage_model(0.33), DamageModel::Heavy);
        assert_eq!(damage_model(0.0), DamageModel::Heavy);
        assert_eq!(REPAIR_DELAY, 20.0);
        assert_eq!(REPAIR_RATE_PERCENT, 0.5);
    }

    #[test]
    fn weapons_deal_the_retail_damage() {
        // tribe_abilities_table: torch 6 / 1.5 s, axe 9 / 2 s, spear 5 or 10 / 3 s,
        // chieftain staff 12 / 2 s, shaman staff 3 / 2.5 s.
        assert!(close(TORCH.melee_dps(), 4.0));
        assert!(close(AXE.melee_dps(), 4.5));
        assert!(close(SPEAR.melee_dps(), 5.0 / 3.0));
        assert!(close(SPEAR.ranged_dps(), 10.0 / 3.0));
        assert!(close(CHIEFTAIN_STAFF.melee_dps(), 6.0));
        assert!(close(SHAMAN_STAFF.melee_dps(), 1.2));
        assert_eq!(SPEAR.range, 25.0);
        assert_eq!(TORCH.range, 1.0);
        assert!(close(chieftain_dps(), 6.0));
        // The spear throws beyond melee reach and stabs inside it.
        assert!(close(SPEAR.dps_at(10.0, 1.9), 10.0 / 3.0));
        assert!(close(SPEAR.dps_at(1.0, 1.9), 5.0 / 3.0));
        assert!(close(AXE.dps_at(10.0, 1.9), 4.5), "no ranged attack: melee numbers");
        // Tools map to their weapons.
        assert_eq!(weapon_of(Some(ToolKind::SpikedClubs)), TORCH);
        assert_eq!(weapon_of(Some(ToolKind::StoneAxes)), AXE);
        assert_eq!(weapon_of(Some(ToolKind::ThrowingSpears)), SPEAR);
        assert_eq!(weapon_of(Some(ToolKind::HealingRods)), SHAMAN_STAFF);
        assert_eq!(weapon_of(Some(ToolKind::WoodenHorns)), UNARMED);
        assert_eq!(weapon_of(None), UNARMED);
        assert_eq!(TIME_BETWEEN_ATTACKS, 1.5);
        assert_eq!(ATTACK_WAIT_DISTANCE, 10.0);
    }

    #[test]
    fn buildings_take_the_raze_damage_of_each_weapon() {
        // creatureAbilityRazeDamage of tribe_hunt1/2/3, the staffs and specials.
        assert_eq!((TORCH.raze, AXE.raze, SPEAR.raze), (9.0, 5.0, 4.0));
        assert_eq!((CHIEFTAIN_STAFF.raze, SHAMAN_STAFF.raze), (7.0, 3.0));
        assert!(close(TORCH.raze_dps(), 6.0), "the torch burns huts best");
        assert!(close(AXE.raze_dps(), 2.5));
        assert!(close(SPEAR.raze_dps(), 4.0 / 3.0));
        assert!(close(CHIEFTAIN_STAFF.raze_dps(), 3.5));
        assert!(close(SHAMAN_STAFF.raze_dps(), 1.2));
        assert!(close(chieftain_raze_dps(), 3.5));
        assert!(close(UNARMED.raze_dps(), 1.0 / 1.5));
        assert_eq!(TORCH.special.unwrap().raze, 9.0);
        assert_eq!(AXE.special.unwrap().raze, 5.0);
        assert_eq!(SPEAR.special.unwrap().raze, 4.0);
        // A level-1 hut (400) falls to three torches and the chieftain in under
        // 20 s; bare hands take minutes.
        let torches = 3.0 * TORCH.raze_dps() + chieftain_raze_dps();
        assert!(400.0 / torches < 20.0);
        assert!(400.0 / (3.0 * UNARMED.raze_dps() + chieftain_raze_dps()) > 60.0);
    }

    #[test]
    fn specials_refresh_every_twelve_seconds_as_area_hits() {
        let torch = TORCH.special.unwrap();
        assert_eq!((torch.damage, torch.refresh, torch.cone_dist, torch.cone_radius), (3.0, 12.0, 2.0, 2.3));
        let axe = AXE.special.unwrap();
        assert_eq!((axe.damage, axe.refresh, axe.cone_dist, axe.cone_radius), (6.75, 12.0, 1.0, 3.5));
        let spear = SPEAR.special.unwrap();
        assert_eq!((spear.damage, spear.ranged_damage, spear.refresh), (3.75, 10.0, 12.0));
        assert!(CHIEFTAIN_STAFF.special.is_none() && SHAMAN_STAFF.special.is_none());
        // The timer fires when it runs out and restarts at the refresh.
        let (t, fire) = special_step(12.0, 11.0, 12.0);
        assert!(close(t, 1.0) && !fire);
        let (t, fire) = special_step(t, 1.5, 12.0);
        assert!(close(t, 12.0) && fire);
    }

    #[test]
    fn the_fishing_cycle_is_the_installed_clips() {
        let c = FishCycle::roll(0.0, 0.0);
        assert!(close(c.loop_seconds, 0.5) && c.succ == 0);
        assert!(close(c.pull_at(), 16.0 / 30.0 + 0.5));
        assert!(close(c.catch_at(), c.pull_at() + 121.0 / 30.0));
        assert!(close(c.credit_at(), c.catch_at() + 61.0 / 30.0));
        assert!(close(c.end(), c.credit_at() + 2.4));
        let c = FishCycle::roll(1.0, 0.99);
        assert!(close(c.loop_seconds, 1.0) && c.succ == 2);
        assert!(close(c.credit_at() - c.catch_at(), 92.0 / 30.0));
        // A middle cycle takes about ten seconds per fish.
        assert!((FishCycle::roll(0.5, 0.5).end() - 10.2).abs() < 0.3);
    }

    #[test]
    fn herd_records_carry_the_territory_and_sight_data() {
        // Dumped from the installed `herdtypes~` records (all respawn 120 s
        // except the epic's 1200 s).
        assert_eq!(TRG_SMALL_PREY.territory_radius, 125.0);
        assert_eq!(TRG_MEDIUM_PREY.territory_radius, 200.0);
        assert_eq!(TRG_LARGE_PREDATOR.territory_radius, 200.0);
        assert_eq!(TRG_LARGE_PREDATOR3.help_radius, 25.0);
        assert_eq!(TRG_EPIC_PREDATOR.territory_radius, 50.0);
        assert_eq!(TRG_EPIC_PREDATOR.sight_radius, 10.0);
        assert_eq!(TRG_FOOD_SCAVENGER.territory_radius, 100.0);
        for h in [&TRG_SMALL_PREY, &TRG_MEDIUM_PREY, &TRG_LARGE_PREY, &TRG_LARGE_PREDATOR, &TRG_LARGE_PREDATOR3, &TRG_LARGE_PREY2] {
            assert_eq!(h.respawn, 120.0, "{}", h.name);
        }
        assert_eq!(TRG_EPIC_PREDATOR.respawn, 1200.0);
    }

    #[test]
    fn food_values_follow_the_tuning() {
        assert_eq!(GATHER_FOOD_PER_FRUIT, 1.0);
        assert_eq!(TREE_SMACK_YIELD, [3.0, 5.0, 0.0]);
        assert_eq!(FISHING_FOOD_PER_FISH, 1.5);
        assert_eq!(CARRY_BUNDLE_AMOUNT_MAX, 10.0);
        assert_eq!(GIFT_FOOD_AMOUNT, 10.0);
        assert_eq!(FRUIT_REGROWTH_TIME, 25.0);
        assert_eq!((GATHER_FIND_TREE_RADIUS, MAX_FORAGING_DISTANCE), (70.0, 150.0));
        assert!(close(forage_radius(), 70.0));
        // The gather cycles are the installed clips (0xD75F70): a smack with
        // the tool brings 2.0 x yield (capped by the room in the bundle), a
        // pick by hand one fruit.
        assert!(close(gather_cycle_seconds(true, false), 55.0 / 30.0));
        assert!(close(gather_cycle_seconds(true, true), 2.2));
        assert!(close(gather_cycle_seconds(false, false), 49.0 / 30.0));
        assert!(close(gather_cycle_food(true, 10.0), 6.0));
        assert!(close(gather_cycle_food(true, 5.0), 5.0), "capped by the bundle");
        assert!(close(gather_cycle_food(false, 5.0), 1.0));
        assert!(close(gather_cycle_food(false, 0.0), 0.0));
        // Bundles: five items each, a fish is worth 1.5, never above 10.
        assert!(close(bundle_food(Haul::Fruit), 5.0));
        assert!(close(bundle_food(Haul::Meat), 5.0));
        assert!(close(bundle_food(Haul::Eggs), 5.0));
        assert!(close(bundle_food(Haul::Fish), 7.5));
        assert!(Haul::Fish != Haul::Fruit);
        for haul in [Haul::Fruit, Haul::Fish, Haul::Meat, Haul::Eggs] {
            assert_eq!(bundle_items(haul), 5.0);
            assert!(bundle_food(haul) <= CARRY_BUNDLE_AMOUNT_MAX);
        }
        assert_eq!(carcass_food(true), 20.0);
        assert_eq!(carcass_food(false), 15.0);
    }

    #[test]
    fn eating_one_food_refills_hunger_and_heals_twenty_five() {
        assert_eq!(EAT_HUNGER_BOOST_PER_FOOD, 100.0);
        assert_eq!(EAT_HEALTH_BOOST_PER_FOOD, 25.0);
        let (hunger, health) = eat_food(20.0, 10.0, 80.0);
        assert!(close(hunger, 100.0) && close(health, 35.0));
        // Both are capped.
        let (hunger, health) = eat_food(90.0, 70.0, 80.0);
        assert!(close(hunger, 100.0) && close(health, 80.0));
    }

    #[test]
    fn tribe_hunger_holds_for_two_minutes_then_drains() {
        assert_eq!(HUNGER_DECAY_DELAY, 120.0);
        assert_eq!(tribe_hunger_drain(Difficulty::Easy), 0.278);
        assert_eq!(tribe_hunger_drain(Difficulty::Medium), 0.333);
        assert_eq!(tribe_hunger_drain(Difficulty::Hard), 0.417);
        // Full hunger is held for 120 s ...
        let (mut hunger, mut hold) = (100.0, 0.0);
        for _ in 0..(119 * 60) {
            (hunger, hold) = hunger_step(hunger, hold, 1.0 / 60.0, Difficulty::Medium);
        }
        assert!(close(hunger, 100.0), "still held at {hunger}");
        // ... then falls by 0.333 per second.
        for _ in 0..(2 * 60) {
            (hunger, hold) = hunger_step(hunger, hold, 1.0 / 60.0, Difficulty::Medium);
        }
        assert!(hunger < 100.0 && hunger > 99.0, "draining at {hunger}");
        let (hunger_after, _) = hunger_step(50.0, 0.0, 10.0, Difficulty::Medium);
        assert!(close(hunger_after, 50.0 - 3.33), "{hunger_after}");
        // Starvation hurts only at zero hunger.
        assert_eq!(starvation_damage(0.5, 2.0), 1.0);
        assert_eq!(starvation_damage(1.0, 2.0), 0.0);
        assert_eq!(STARVATION_PAIN, 0.5);
    }

    #[test]
    fn members_cost_ten_food_grow_in_sixty_seconds_and_the_hut_caps_them() {
        assert_eq!(MEMBER_PRICE, 10.0);
        assert_eq!(TIME_BEFORE_BABY_GROWS_UP, 60.0);
        assert_eq!(BABY_GROWTH_BOOST, 30.0);
        assert_eq!(MATING_TIME, 5.0);
        assert!(can_buy_member(10.0, 5, 0));
        assert!(!can_buy_member(9.9, 5, 0), "not enough food");
        assert!(!can_buy_member(100.0, 6, 0), "level 1 hut holds six");
        assert!(can_buy_member(100.0, 6, 1), "level 2 hut holds nine");
        assert!(!can_buy_member(100.0, 12, 5));
        assert_eq!(MAX_DOMESTICATED_ANIMALS, 3);
        assert!(close(villager_replace_seconds(), 65.0));
    }

    #[test]
    fn shamans_heal_five_percent_per_second_within_five_units() {
        assert!(close(heal_per_second(80.0, true), 4.0));
        assert!(close(heal_per_second(200.0, false), 10.0));
        assert_eq!(SHAMAN_HEALING_DISTANCES, (1.0, 5.0));
        assert!(in_healing_reach(5.0));
        assert!(!in_healing_reach(5.1));
        assert_eq!(SHAMAN_FIND_INJURED_DISTANCE, 25.0);
        assert_eq!(SHAMAN_HEALING_MAX_ANGLE, 135.0);
    }

    #[test]
    fn relationship_events_use_the_tuning_with_their_floors() {
        let expect = [
            (RelationEvent::Attack, -10.0, Some(-60.0), 0x0530_CF00),
            (RelationEvent::Kill, -60.0, Some(-100.0), 0x0530_CF01),
            (RelationEvent::Raid, -30.0, Some(-60.0), 0x0530_CF02),
            (RelationEvent::StealBaby, -30.0, Some(-100.0), 0x0530_CF03),
            (RelationEvent::AttackToolOrHut, -10.0, Some(-100.0), 0x0530_CF04),
            (RelationEvent::DestroyTool, -100.0, Some(-100.0), 0x0530_CF05),
            (RelationEvent::Gift, 10.0, None, 0x0530_CF06),
            (RelationEvent::Social, 10.0, None, 0x0530_CF07),
            (RelationEvent::SocialPower, 10.0, None, 0x54EA_B4B3),
        ];
        for (event, add, floor, id) in expect {
            let e = event.effect();
            assert_eq!((e.add, e.floor, event.id()), (add, floor, id), "{event:?}");
        }
        assert_eq!(RelationEvent::ALL.len(), 9);
        assert_eq!(GIFT_FOOD_AMOUNT, GIFT_RELATIONSHIP);
        // Attacks bottom out at -60, kills at -100, raids at -60.
        let mut v = 0.0;
        for _ in 0..20 {
            v = apply_event(v, RelationEvent::Attack);
        }
        assert_eq!(v, -60.0);
        v = apply_event(v, RelationEvent::Kill);
        assert_eq!(v, -100.0);
        assert_eq!(apply_event(0.0, RelationEvent::Raid), -30.0);
        assert_eq!(apply_event(-50.0, RelationEvent::Raid), -60.0);
        assert_eq!(apply_event(-80.0, RelationEvent::Raid), -80.0, "below the floor stays");
        assert_eq!(apply_event(30.0, RelationEvent::StealBaby), 0.0);
        assert_eq!(apply_event(50.0, RelationEvent::DestroyTool), -50.0);
        assert_eq!(apply_event(0.0, RelationEvent::DestroyTool), -100.0);
        // Gifts and social events add ten each and are only capped by the range.
        assert_eq!(apply_event(0.0, RelationEvent::Gift), 10.0);
        assert_eq!(apply_event(-60.0, RelationEvent::Social), -50.0);
        assert_eq!(apply_event(95.0, RelationEvent::Social), 100.0);
        assert_eq!(apply_event(f32::NAN, RelationEvent::Gift), 10.0);
    }

    #[test]
    fn the_ten_npc_tribes_match_the_archetype_table() {
        assert_eq!(NPC_TRIBES.len(), 10);
        let by_name = |n: &str| NPC_TRIBES.iter().find(|t| t.name == n).unwrap();
        // Tiers: members / maximum / food.
        for n in ["1a", "1b", "1c"] {
            let t = by_name(n);
            assert_eq!((t.tier, t.initial_population, t.max_population, t.initial_food), (1, 3, 6, 125.0));
        }
        for n in ["2a", "2b", "3a", "3b", "4a", "4b"] {
            let t = by_name(n);
            assert_eq!((t.initial_population, t.max_population, t.initial_food), (6, 9, 200.0), "{n}");
        }
        let five = by_name("5");
        assert_eq!((five.tier, five.initial_population, five.max_population, five.initial_food), (5, 9, 12, 300.0));
        // Archetype ids, tools and a few role shares.
        assert_eq!(by_name("1a").archetype, 1);
        assert_eq!(by_name("1b").archetype, 2);
        assert_eq!(by_name("1c").archetype, 15);
        assert_eq!(by_name("2a").tools, &[3, 7, 4]);
        assert_eq!(by_name("2b").tools, &[9, 7, 3, 6]);
        assert_eq!(by_name("3a").tools, &[1, 8, 5]);
        assert_eq!(by_name("4b").tools, &[2, 6]);
        assert_eq!(five.tools, &[2, 1, 3, 9, 5, 6]);
        assert_eq!(by_name("1a").roles.hornist, 0.333);
        assert_eq!(by_name("2a").roles.spearman, 0.778);
        assert_eq!(by_name("4b").roles.axeman, 0.778);
        assert_eq!(five.roles.axeman, 0.27);
        // Every role share set adds up to at most one tribe.
        for t in &NPC_TRIBES {
            let r = t.roles;
            let sum = r.hornist + r.didgeridoo + r.maraca + r.healer + r.spearman + r.axeman
                + r.torchman + r.fisherman + r.gatherer;
            assert!(sum <= 1.001, "{}: {sum}", t.name);
            assert!(t.initial_population <= t.max_population);
        }
        // Lookup by archetype id.
        assert_eq!(npc_tribe(14).unwrap().name, "5");
        assert!(npc_tribe(99).is_none());
        // Hut levels and village pieces: dwelling by tier, two racks of 200.
        assert_eq!(tribe_hut_level(1), 1);
        assert_eq!(tribe_hut_level(3), 2);
        assert_eq!(tribe_hut_level(5), 3);
        assert_eq!(village_piece_hp(1, 0), 400.0);
        assert_eq!(village_piece_hp(3, 0), 750.0);
        assert_eq!(village_piece_hp(5, 0), 1000.0);
        assert_eq!(village_piece_hp(5, 1), 200.0);
        assert_eq!(village_piece_hp(1, 2), 200.0);
    }

    #[test]
    fn defenders_fight_with_the_weapons_of_their_roles() {
        let dps = |name: &str| defender_dps(NPC_TRIBES.iter().find(|t| t.name == name).unwrap());
        // Instrument and fishing tribes only defend bare-handed.
        assert!(close(dps("1a"), UNARMED.melee_dps()));
        // Spear tribes stab for 5 / 3 s, torch tribes burn for 4, axe tribes chop for 4.5.
        assert!(dps("2a") > UNARMED.melee_dps() && dps("2a") < TORCH.melee_dps());
        assert!(dps("3a") > dps("2a"));
        assert!(dps("4b") > dps("3a"));
        assert!(dps("4b") <= AXE.melee_dps());
        let five = dps("5");
        assert!(five > dps("2a") && five < AXE.melee_dps(), "{five}");
    }

    #[test]
    fn every_lineup_has_one_tribe_of_each_tier_with_a_known_relationship() {
        for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
            for lineup in 0..LINEUP_COUNT as u8 {
                for village in 0..5 {
                    let (tribe, relation) = lineup_tribe(lineup, village, difficulty);
                    assert_eq!(tribe.tier as usize, village + 1, "one tribe per tier");
                    assert!([0.0, -30.0, -70.0].contains(&relation), "{relation}");
                }
                // Tier 1 starts neutral except on hard, tier 5 is never friendly.
                let (_, first) = lineup_tribe(lineup, 0, difficulty);
                assert_eq!(first, if difficulty == Difficulty::Hard { -30.0 } else { 0.0 });
                let (_, last) = lineup_tribe(lineup, 4, difficulty);
                assert!(last <= -30.0);
            }
        }
        // Medium lineup 0x000DB6B9: 1c, 2b, 3a, 4b, 5 at 0 / -3 / -3 / -7 / -7 (x10).
        let medium: Vec<(&str, f32)> = (0..5)
            .map(|v| {
                let (t, r) = lineup_tribe(0, v, Difficulty::Medium);
                (t.name, r)
            })
            .collect();
        assert_eq!(
            medium,
            vec![("1c", 0.0), ("2b", -30.0), ("3a", -30.0), ("4b", -70.0), ("5", -70.0)]
        );
        // Easy lineup 0x000DB6B9 overrides: 0, -7, -3, 0, -3.
        let easy: Vec<f32> = (0..5).map(|v| lineup_tribe(0, v, Difficulty::Easy).1).collect();
        assert_eq!(easy, vec![0.0, -70.0, -30.0, 0.0, -30.0]);
        // The seed picks a lineup deterministically inside the range.
        for seed in [0u64, 1, 12345, u64::MAX] {
            assert!((lineup_for_seed(seed) as usize) < LINEUP_COUNT);
            assert_eq!(lineup_for_seed(seed), lineup_for_seed(seed));
        }
        assert_eq!(village_tribe(0, 0).name, "1c");
    }

    #[test]
    fn the_six_superpowers_use_the_ability_props() {
        let all = [
            SUPERPOWER_TRAPS,
            SUPERPOWER_REFRESHING_STORM,
            SUPERPOWER_FLYING_FISH,
            SUPERPOWER_FIRE_BOMBS,
            SUPERPOWER_FIREWORKS,
            SUPERPOWER_BEASTMASTER,
        ];
        let types: Vec<u32> = all.iter().map(|p| p.ability_type).collect();
        assert_eq!(types, vec![52, 53, 54, 55, 56, 57], "ModAPI kTRG_SuperPower_*");
        let refresh: Vec<f32> = all.iter().map(|p| p.refresh).collect();
        assert_eq!(refresh, vec![300.0, 300.0, 300.0, 240.0, 300.0, 360.0]);
        let range: Vec<f32> = all.iter().map(|p| p.range).collect();
        assert_eq!(range, vec![1.0, 20.0, 1.0, 15.0, 25.0, 50.0]);
        assert_eq!(SUPERPOWER_FIRE_BOMBS.damage, 15.0);
        for p in &all {
            assert_eq!(p.description_id, p.name_id + 1, "{}", p.name);
            assert!(p.ability.starts_with("superpower_") && p.consequence.starts_with("trg_"));
        }
        assert_eq!(SUPERPOWER_TRAPS.name_id, 0x0002_0025);
        assert_eq!(SUPERPOWER_LOCALE_TABLE, 0x9A5A_CF10);
        // The trap, fish and beastmaster props.
        assert_eq!((TRAP_LIFETIME, TRAP_RANGE, TRAP_POISON_RANGE), (25.0, 60.0, 7.0));
        assert!(close(trap_lure_radius(), 60.0));
        assert!(close(trap_spring_radius(), 7.0));
        assert!(close(flying_fish_food(), 10.5), "seven fish at 1.5");
        assert_eq!((BEASTMASTER_MAX_ANIMALS, BEASTMASTER_DURATION), (5, 120.0));
        assert!(close(superpower_reach(&SUPERPOWER_BEASTMASTER), 50.0));
        assert!(close(superpower_reach(&SUPERPOWER_FIRE_BOMBS), 15.0));
    }

    #[test]
    fn the_attitude_follows_the_four_caps() {
        assert_eq!(attitude(-100.0), 0);
        assert_eq!(attitude(-70.0), 0, "at the very bad cap the tribe is Furious");
        assert_eq!(attitude(-69.0), 1);
        assert_eq!(attitude(-30.0), 1);
        assert_eq!(attitude(-29.0), 2);
        assert_eq!(attitude(0.0), 2);
        assert_eq!(attitude(30.0), 3);
        assert_eq!(attitude(69.0), 3);
        assert_eq!(attitude(70.0), 4);
        // The campaign's stances agree with the attitudes.
        for r in [-100.0, -70.0, -50.0, -30.0, 0.0, 29.0, 30.0, 69.0] {
            assert_eq!(stance(r, false) == Stance::Angry, attitude(r) <= 1, "{r}");
        }
    }

    #[test]
    fn a_gift_fades_one_attitude_step_per_timer_while_above_the_baseline() {
        // The timer lengths are the difficulty tuning's.
        use crate::creature_rules::Difficulty;
        assert_eq!(gift_timer_seconds(Difficulty::Easy), 360.0);
        assert_eq!(gift_timer_seconds(Difficulty::Medium), 180.0);
        assert_eq!(gift_timer_seconds(Difficulty::Hard), 60.0);
        // A tribe that started Cautious (0) and was warmed to Pleased (50):
        // the first expiry pulls it to 0 (Cautious), which is the baseline.
        let d = gift_decay(50.0, 0.0);
        assert_eq!(d, GiftDecay { relations: 0.0, restart: false });
        // One that started Annoyed (-30) and rose to Pleased (40) steps down twice.
        let d = gift_decay(40.0, -30.0);
        assert_eq!(d, GiftDecay { relations: 0.0, restart: true });
        let d = gift_decay(d.relations, -30.0);
        assert_eq!(d, GiftDecay { relations: -30.0, restart: false });
        // A tribe that started Furious (-70): Annoyed fades to the very bad cap.
        assert_eq!(gift_decay(-40.0, -70.0), GiftDecay { relations: -70.0, restart: false });
        // A Friendly tribe (an ally) keeps its relationship and the timer stops.
        assert_eq!(gift_decay(85.0, 0.0), GiftDecay { relations: 85.0, restart: false });
        // A Cautious tribe that started Furious drops to the bad cap (Annoyed)
        // and the timer runs on until it is Furious again.
        assert_eq!(gift_decay(0.0, -70.0), GiftDecay { relations: -30.0, restart: true });
        // A Furious tribe has no step.
        assert_eq!(gift_decay(-90.0, -90.0), GiftDecay { relations: -90.0, restart: false });
    }

    #[test]
    fn social_events_lift_the_relationship_to_a_target_like_the_retail_scale() {
        // 0xDA3523: the performance posts TribeSocial with scale `cap -
        // relationship`; the +1 event moves the tribe exactly to the cap.
        assert_eq!(social_lift(0.0, SOCIAL_NEUTRAL_CAP), 30.0);
        assert_eq!(social_lift(-80.0, SOCIAL_NEUTRAL_CAP), 30.0);
        assert_eq!(social_lift(30.0, SOCIAL_GOOD_CAP), 70.0);
        assert_eq!(social_lift(55.0, SOCIAL_GOOD_CAP), 70.0);
        // The scale must be positive: a tribe already at the cap (or above)
        // posts no event at all.
        assert_eq!(social_lift(30.0, SOCIAL_NEUTRAL_CAP), 30.0);
        assert_eq!(social_lift(45.0, SOCIAL_NEUTRAL_CAP), 45.0);
        assert_eq!(social_lift(95.0, SOCIAL_GOOD_CAP), 95.0);
        // 0xDA183A..0xDA1863: Cautious -> level 1, Pleased -> level 2, the
        // other attitudes level 0 (no target).
        assert_eq!(social_level(-80.0), 0);
        assert_eq!(social_level(-30.0), 0);
        assert_eq!(social_level(-29.0), 1);
        assert_eq!(social_level(0.0), 1);
        assert_eq!(social_level(29.0), 1);
        assert_eq!(social_level(30.0), 2);
        assert_eq!(social_level(69.0), 2);
        assert_eq!(social_level(70.0), 0);
        assert_eq!(social_target(0), None);
        assert_eq!(social_target(1), Some(30.0));
        assert_eq!(social_target(2), Some(70.0));
        // The bar (FUN_00D9CC20) is 10 x the share of the score, capped at 10.
        assert_eq!(social_bar(397.5, 795.0), 5.0);
        assert_eq!(social_bar(900.0, 795.0), 10.0);
        assert!(social_success(795.0, 795.0) && !social_success(794.0, 795.0));
        assert!(!social_success(10.0, 0.0), "no threshold, no success");
    }

    #[test]
    fn the_social_score_follows_the_retail_numbers() {
        // 0xD9CCF0: social level x 35 + 25 per participant.
        assert_eq!(social_start_score(0, 3), 75.0);
        assert_eq!(social_start_score(3, 6), 105.0 + 150.0);
        // 0xDA0100: enough of the wanted instrument is a full beat, some of it
        // its share, none of it 5 per player of another instrument.
        assert_eq!(social_beat_score(3, 3, 3), 175.0);
        assert_eq!(social_beat_score(4, 3, 4), 175.0);
        assert!((social_beat_score(1, 3, 1) - 175.0 / 3.0).abs() < 1e-3);
        assert_eq!(social_beat_score(0, 3, 2), 10.0);
        assert_eq!(social_beat_score(0, 3, 0), 0.0);
        // The thresholds of `tribe_archetypes.txt` (+0x90 level 1, +0x94 else).
        let first = social_tuning(1);
        assert_eq!((social_threshold(&first, 1), social_threshold(&first, 2)), (795.0, 880.0));
        assert_eq!((first.wanted_min, first.wanted_max), (3.0, 3.0));
        let last = social_tuning(14);
        assert_eq!((social_threshold(&last, 1), social_threshold(&last, 2)), (1000.0, 1090.0));
        assert_eq!(social_tuning(11).instruments, &[7]);
        assert_eq!(social_tuning(200).threshold_level1, 800.0, "the template's data");
        // A level 0 song (a Furious / Annoyed / Friendly tribe) uses +0x94.
        assert_eq!(social_threshold(&first, 0), 880.0);
        // cvtss2si rounds to nearest, ties to even.
        let two_three = social_tuning(2);
        assert_eq!(social_required(&two_three, 0.0), 2);
        assert_eq!(social_required(&two_three, 0.49), 2);
        assert_eq!(social_required(&two_three, 0.5), 2, "2.5 rounds to 2");
        assert_eq!(social_required(&two_three, 0.51), 3);
        assert_eq!(social_required(&two_three, 1.0), 3);
        assert_eq!(social_required(&first, 0.3), 3);
        // The list holds ability types 5 / 6 / 7 = horn / maracas / didgeridoo.
        assert_eq!((0..3).map(|i| social_wanted_kind(&first, i)).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(social_wanted_kind(&social_tuning(11), 5), 2);
        assert!(social_ending_cheers(5.1) && !social_ending_cheers(5.0));
    }

    #[test]
    fn feeding_a_wild_animal_follows_the_installed_herd_records() {
        // 0xEB69140E of the installed archetypes: small / medium / large prey.
        assert_eq!(feed_cost_of("TRG_NPC_Small_Prey"), 5.0);
        assert_eq!(feed_cost_of("TRG_NPC_Medium_Prey"), 10.0);
        assert_eq!(feed_cost_of("TRG_NPC_Large_Prey"), 15.0);
        assert_eq!(feed_cost_of("unknown"), FEED_COST_DEFAULT);
        // The prey herds need one interaction for every stage; the record
        // default needs 1 / 3 / 6 (0xBA43E0).
        assert_eq!(impress_counts("TRG_NPC_Medium_Prey"), [1, 1, 1]);
        assert_eq!(impress_counts("anything else"), [1, 3, 6]);
        assert_eq!(interaction_state(0, [1, 1, 1], 3), 3, "no interaction: the animal's own state");
        assert_eq!(interaction_state(1, [1, 1, 1], 3), 6, "one feeding makes a prey animal a friend");
        let default = impress_counts("x");
        assert_eq!(interaction_state(0, default, 2), 2);
        assert_eq!(interaction_state(1, default, 2), 3);
        assert_eq!(interaction_state(2, default, 2), 3);
        assert_eq!(interaction_state(3, default, 2), 4);
        assert_eq!(interaction_state(5, default, 2), 4);
        assert_eq!(interaction_state(6, default, 2), 6);
    }

    #[test]
    fn the_troupes_spots_follow_the_start_0xda1290() {
        // Spots 1..4 stand in front of the target on the leader's side: straight at
        // 2 size + 15, swung by +-45 degrees, and straight at 2 size + 22; spot 5 is
        // the target itself.
        let centre = Vec3::new(100.0, 0.0, -40.0);
        let dir = Vec3::new(1.0, 0.0, 0.0);
        let size = 1.5;
        let spots = social_spots(centre, dir, size);
        let near = 2.0 * size + 15.0;
        assert!((spots[0] - (centre - dir * near)).length() < 1e-4);
        assert!(((spots[3] - centre).length() - (2.0 * size + 22.0)).abs() < 1e-4);
        assert!(((spots[1] - centre).length() - near).abs() < 1e-4);
        assert!(((spots[2] - centre).length() - near).abs() < 1e-4);
        let angle = |a: Vec3| (a - centre).normalize().dot((spots[0] - centre).normalize()).acos();
        assert!((angle(spots[1]) - std::f32::consts::FRAC_PI_4).abs() < 1e-3, "{}", angle(spots[1]));
        assert!((angle(spots[2]) - std::f32::consts::FRAC_PI_4).abs() < 1e-3);
        assert!((spots[1] - spots[2]).length() > 1.0, "one swings left, the other right");
        assert_eq!(spots[4], centre);
        // The layout's slots touch (2 x radius apart), the first at the origin.
        let slots = social_slots(centre, dir, 1.5);
        assert_eq!(slots[0], centre);
        assert!(slots.len() > 100);
        let nearest = slots[1..].iter().map(|s| (*s - centre).length()).fold(f32::MAX, f32::min);
        assert!((nearest - 3.0).abs() < 1e-3, "{nearest}");
        let min_gap = (0..slots.len().min(60))
            .flat_map(|i| (0..i).map(move |j| (i, j)))
            .map(|(i, j)| (slots[i] - slots[j]).length())
            .fold(f32::MAX, f32::min);
        assert!((min_gap - 3.0).abs() < 1e-3, "no two slots closer than a slot's width: {min_gap}");
    }

    #[test]
    fn the_troupe_stands_in_groups_at_its_spots_like_0xda22c0() {
        let centre = Vec3::new(0.0, 0.0, 200.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);
        let size = SOCIAL_CREATURE_SIZE;
        // The troupe comes from the south (the leader's side), two members per
        // instrument group and two without an instrument, the chieftain in the middle.
        let mut members = vec![(Vec3::new(0.0, 0.0, 0.0), TroupeRole::Chieftain)];
        for g in 0..4usize {
            for k in 0..2 {
                members.push((Vec3::new(-6.0 + g as f32 * 4.0 + k as f32, 0.0, -3.0 - k as f32), TroupeRole::Group(g)));
            }
        }
        let stands = social_stands(centre, dir, size, &members);
        assert_eq!(stands.len(), members.len());
        let spots = social_spots(centre, dir, size);
        // Everyone has its own slot: no two stands closer than a slot's width.
        for i in 0..stands.len() {
            for j in 0..i {
                assert!((stands[i] - stands[j]).length() > 2.9, "{i} {j}: {:?} {:?}", stands[i], stands[j]);
            }
        }
        // The chieftain stands in front of the groups: nearer to the target.
        let d = |p: Vec3| (p - centre).length();
        let chief = d(stands[0]);
        assert!(chief < 2.0 * size + 15.0 - 5.0, "the chieftain's slot is near 2 (size + 1) + 3 = {chief}");
        // Each group stays together around one spot, and the groups use different spots.
        let mut used = std::collections::HashSet::new();
        for g in 0..4usize {
            let pair = [stands[1 + 2 * g], stands[2 + 2 * g]];
            let spot = (0..4)
                .min_by(|&a, &b| d_to(pair[0], spots[a]).total_cmp(&d_to(pair[0], spots[b])))
                .unwrap();
            assert!(d_to(pair[0], spots[spot]) < 12.0 && d_to(pair[1], spots[spot]) < 12.0, "group {g} at spot {spot}");
            used.insert(spot);
        }
        assert_eq!(used.len(), 4, "the four groups take four different spots");
    }

    fn d_to(a: Vec3, b: Vec3) -> f32 {
        (a - b).length()
    }

    #[test]
    fn the_members_ending_clips_follow_0xda3944() {
        // An instrument player cheers by its tool, then everyone plays the ending
        // clip: the chieftain's and the crowd's when the bar passed 5, the fail clip
        // otherwise (0xDA3991..0xDA3A3C).
        let (cheer, end) = social_ending_clips(Some(0), false, 7.0);
        assert_eq!((cheer.map(|c| c.0), end.0), (Some(0xD472_CE9C), 0x5474_62FF));
        let (cheer, end) = social_ending_clips(Some(1), false, 5.0);
        assert_eq!((cheer.map(|c| c.0), end.0), (Some(0x5474_36F2), 0x1478_0312), "5.0 is not above 5");
        let (cheer, _) = social_ending_clips(Some(2), false, 9.0);
        assert_eq!(cheer.map(|c| c.0), Some(0x1474_36A3));
        assert_eq!(social_ending_clips(Some(3), false, 9.0).0, None, "no instrument, no cheer");
        assert_eq!(social_ending_clips(None, false, 9.0).0, None);
        let (cheer, end) = social_ending_clips(None, true, 6.0);
        assert_eq!((cheer, end.0), (None, 0xD478_051A), "the chieftain's own clip");
        assert_eq!(social_ending_clips(None, true, 1.0).1 .0, 0x1478_0312, "a failed song ends the same for him");
        // The stop waits for the longest cheer and ending clip.
        assert!((social_ending_seconds(8.0) - (1.2 + 251.0 / 30.0)).abs() < 1e-4);
        assert!((social_ending_seconds(1.0) - (1.2 + 193.0 / 30.0)).abs() < 1e-4);
    }

    #[test]
    fn the_social_bar_fills_by_its_queue_like_0xe31380() {
        // 0xD9CC20 queues (delta / threshold, over `seconds`): a beat's whole
        // 175 on a 795 bar over one second.
        let mut bar = SocialBarMeter::default();
        assert!(!bar.is_full() && !bar.animating());
        bar.queue_fill(175.0 / 795.0, SOCIAL_BAR_BEAT_SECONDS);
        assert!(bar.animating());
        bar.update(0.5);
        assert!((bar.fill - 175.0 / 795.0 * 0.5).abs() < 1e-5, "half of it after half a second: {}", bar.fill);
        bar.update(0.6);
        assert!((bar.fill - 175.0 / 795.0).abs() < 1e-5, "the step never passes what is left");
        assert!(!bar.animating(), "a finished fill leaves the queue");
        // Two fills run side by side (the start score over 20 s, a beat over 1 s).
        let mut bar = SocialBarMeter::default();
        bar.queue_fill(0.2, 20.0);
        bar.queue_fill(0.22, 1.0);
        bar.update(1.0);
        assert!((bar.fill - (0.01 + 0.22)).abs() < 1e-5, "{}", bar.fill);
        assert!(bar.animating(), "the slow fill is still queued");
        for _ in 0..40 {
            bar.update(0.5);
        }
        assert!((bar.fill - 0.42).abs() < 1e-4 && !bar.animating());
        // Full: the bar stands and takes no more.
        let mut bar = SocialBarMeter::default();
        bar.queue_fill(1.0, 1.0);
        bar.update(1.0);
        assert!(bar.is_full());
        bar.queue_fill(0.3, 1.0);
        bar.update(1.0);
        assert_eq!(bar.fill, 1.0, "a full bar does not move (0xE31380)");
        // A failed song drains at 0.4 per second and is done at 0.
        let mut bar = SocialBarMeter::default();
        bar.queue_fill(0.6, 1.0);
        bar.update(1.0);
        bar.queue_fill(0.1, 1.0);
        bar.fail();
        assert!(!bar.animating() && bar.failed, "failing drops the queue");
        bar.update(1.0);
        assert!((bar.fill - 0.2).abs() < 1e-5);
        bar.update(1.0);
        assert!(!bar.failed && bar.fill == 0.0);
        // The queue is bounded: the last slot takes the rest.
        let mut bar = SocialBarMeter::default();
        for _ in 0..SOCIAL_BAR_QUEUE + 3 {
            bar.queue_fill(0.01, 1.0);
        }
        bar.update(1.0);
        assert!((bar.fill - 0.01 * (SOCIAL_BAR_QUEUE + 3) as f32).abs() < 1e-4, "nothing is lost: {}", bar.fill);
        // The faces: neutral -> blue for a Cautious tribe, blue -> green for a Pleased one.
        assert_eq!(social_bar_faces(1), Some((2, 3)));
        assert_eq!(social_bar_faces(2), Some((3, 4)));
        assert_eq!(social_bar_faces(0), None, "a level 0 song has no bar");
    }

    #[test]
    fn a_delivered_gift_calms_an_angry_tribe_to_neutral_and_does_nothing_else() {
        // 0xDBC1F1..0xDBC25C: attitude < Cautious -> TribeGift with scale
        // `-relationship` (to 0); any other attitude posts nothing.
        assert_eq!(gift_effect(-95.0), 0.0);
        assert_eq!(gift_effect(-70.0), 0.0);
        assert_eq!(gift_effect(-30.0), 0.0);
        assert_eq!(gift_effect(-29.0), -29.0);
        assert_eq!(gift_effect(0.0), 0.0);
        assert_eq!(gift_effect(40.0), 40.0);
        assert_eq!(gift_effect(80.0), 80.0);
    }

    #[test]
    fn stances_follow_the_relationship_attitudes() {
        assert_eq!(stance(-70.0, false), Stance::Angry);
        assert_eq!(stance(-30.0, false), Stance::Angry);
        assert_eq!(stance(-29.0, false), Stance::Neutral);
        assert_eq!(stance(0.0, false), Stance::Neutral);
        assert_eq!(stance(29.9, false), Stance::Neutral);
        assert_eq!(stance(30.0, false), Stance::Friendly);
        assert_eq!(stance(69.0, false), Stance::Friendly);
        assert_eq!(stance(10.0, true), Stance::Allied);
        assert!(raids(-30.0) && raids(-70.0) && raids(-100.0));
        assert!(!raids(RAID_RELATION_LIMIT) && !raids(0.0) && !raids(30.0));
        assert!(!allies(69.9) && allies(70.0) && allies(100.0));
        // The lineups start tribes at 0 / -30 / -70: the Annoyed and Furious
        // ones raid from the start, the Cautious ones only once angered.
        for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
            for village in 0..5 {
                let start = lineup_tribe(3, village, difficulty).1;
                assert_eq!(raids(start), start <= -30.0, "{start}");
            }
        }
    }

    #[test]
    fn citizens_have_the_level_five_base_health_plus_their_parts() {
        // creatureBaseHealth_Level5 100 + min(health x 2, 10).
        assert_eq!(citizen_max_hp(0), 100.0);
        assert_eq!(citizen_max_hp(3), 106.0);
        assert_eq!(citizen_max_hp(9), 110.0);
        assert_eq!(npc_citizen_max_hp(), 100.0);
        assert_eq!(chieftain_health(citizen_max_hp(0)), 150.0);
        assert_eq!(BABY_HIT_POINT_MULTIPLIER, 0.5);
        // A torch-armed member needs ~25 s to fell a rival member.
        let seconds = npc_citizen_max_hp() / TORCH.melee_dps();
        assert!((20.0..30.0).contains(&seconds), "{seconds}");
    }

    #[test]
    fn raids_take_a_bundle_and_stocks_regrow() {
        assert_eq!(raid_take(125.0), 10.0);
        assert_eq!(raid_take(4.0), 4.0);
        assert_eq!(raid_take(-1.0), 0.0);
        assert_eq!(npc_food_regrow(100.0, 125.0, 10.0), 101.0);
        assert_eq!(npc_food_regrow(124.99, 125.0, 60.0), 125.0);
        assert_eq!(npc_food_regrow(130.0, 125.0, 60.0), 130.0);
    }

    #[test]
    fn villages_stand_on_their_placement_rings_compressed_to_the_island() {
        assert_eq!(NEARBY_NPC_TRIBE_MARKER.distance_from_origin, (175.0, 225.0));
        assert_eq!(FAR_NPC_TRIBE_MARKER.distance_from_origin, (175.0, 350.0));
        assert_eq!(FILLER_NPC_TRIBE_MARKER.distance_from_origin, (200.0, 450.0));
        assert_eq!(FAILSAFE_NPC_TRIBE_MARKER.distance_from_origin, (200.0, 500.0));
        assert_eq!(PLAYER_TRIBE_MARKER.distance_from_origin, (40.0, 60.0));
        assert_eq!(PLAYER_TRIBE_MARKER.id, 0x03D7_F90A);
        assert_eq!(NUM_NPC_TRIBES, 5);
        assert_eq!((MAX_NUM_TRIBE_ROADS, ROAD_WIDTH), (2, 13.0));
        // Retail rings: the nearby tribe at 200, the others farther out.
        assert!(close(village_distance(0), 200.0));
        for i in 1..5 {
            assert!(village_distance(i) > village_distance(0));
            assert!(village_distance(i) <= 350.0);
        }
        assert_eq!(village_marker(0).name, "nearbyNPCTribe");
        assert_eq!(village_marker(4).name, "failsafeNPCTribe");
    }

    #[test]
    fn the_generation_list_places_the_tribal_herds() {
        assert_eq!(TRIBAL_HERDS.len(), 7);
        let count = |h: &HerdArchetype| TRIBAL_HERDS.iter().filter(|x| x.name == h.name).count();
        assert_eq!(count(&TRG_MEDIUM_PREY), 2);
        assert_eq!(count(&TRG_LARGE_PREY), 2);
        assert_eq!(count(&TRG_LARGE_PREDATOR), 1);
        assert_eq!(count(&TRG_EPIC_PREDATOR), 2);
        assert_eq!(herd_creatures(), 6 * 2 + 7 * 2 + 7);
        // Archetype numbers (T4): creatures / HP / size / food.
        let row = |h: &HerdArchetype| (h.creatures, h.health, h.size, h.food);
        assert_eq!(row(&TRG_SMALL_PREY), (5, 15.0, 3.0, 5.0));
        assert_eq!(row(&TRG_MEDIUM_PREY), (6, 25.0, 6.0, 10.0));
        assert_eq!(row(&TRG_LARGE_PREY), (7, 40.0, 9.0, 15.0));
        assert_eq!(row(&TRG_LARGE_PREDATOR), (7, 40.0, 9.0, 20.0));
        assert_eq!(row(&TRG_EPIC_PREDATOR), (1, 1000.0, 25.0, 300.0));
        assert_eq!(TRG_EPIC_PREDATOR.respawn, 1200.0);
        assert_eq!(TRG_EPIC_PREDATOR.damage_scale, 15.0);
        assert_eq!(TRG_SMALL_PREY.respawn, 120.0);
        assert_eq!(TRG_FOOD_SCAVENGER.respawn, 0.0);
        assert!(TRG_LARGE_PREDATOR.predator && !TRG_LARGE_PREY.predator);
        // Herd homes sit between home and the villages; epics farther out.
        assert!(close(herd_distance(&TRG_MEDIUM_PREY), 95.0 * LAYOUT_SCALE));
        assert!(herd_distance(&TRG_EPIC_PREDATOR) > herd_distance(&TRG_LARGE_PREY));
        assert!(herd_body_scale(&TRG_EPIC_PREDATOR) > 2.0);
        // Respawn: one creature per period while the herd is short.
        let (t, back) = herd_respawn_step(0.0, 119.0, 2, 120.0);
        assert!(!back && close(t, 119.0));
        let (t, back) = herd_respawn_step(t, 1.0, 2, 120.0);
        assert!(back && t == 0.0);
        assert_eq!(herd_respawn_step(50.0, 1.0, 0, 120.0), (0.0, false), "full herd");
        assert_eq!(herd_respawn_step(50.0, 1.0, 1, 0.0), (0.0, false), "never respawns");
        // A predator bites for 2 every 1.5 s; the epic fifteen times harder.
        assert!(close(predator_dps(&TRG_LARGE_PREDATOR), 2.0 / 1.5));
        assert!(close(epic_dps(), 20.0));
    }

    #[test]
    fn herds_wake_within_100_and_freeze_beyond_150() {
        assert_eq!(CASTING_DISTANCE_THRESHOLDS, (100.0, 150.0));
        let (on, off) = herd_activation_distances();
        assert!(close(on, 100.0) && close(off, 150.0));
        assert!(!herd_active(false, 101.0), "not yet woken");
        assert!(herd_active(false, 99.0));
        assert!(herd_active(true, 149.0), "stays awake until 150");
        assert!(!herd_active(true, 151.0));
        assert_eq!(MAX_ANIMAL_POOL_SIZE, 100);
    }

    #[test]
    fn the_epic_predator_is_a_thousand_hit_point_boss() {
        assert_eq!(EPIC_HEALTH, 1000.0);
        assert_eq!(EPIC_DAMAGE_SCALE, 15.0);
        assert_eq!(EPIC_SIZE, 25.0);
        assert_eq!(EPIC_FOOD, 300.0);
        assert_eq!(EPIC_RESPAWN_TIME, 1200.0);
        assert_eq!(EPIC_PLACEMENT, (175.0, 240.0));
        assert_eq!(EPIC_PLACEMENT_VARIANT, (100.0, 200.0));
    }
}
