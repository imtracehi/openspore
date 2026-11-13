//! The NPC tribes' plans (retail cTribePlanner, `notes\tribal-re-notes.md` T9):
//! when a rival tribe raids the player's village, sends a food thief, or
//! brings a gift.
//!
//! VERIFIED (decompilations `notes\tribal-re\TR9_*`):
//! - The plan records are the installed `TribePlans` props (group 0x72D02C25),
//!   chosen per archetype and difficulty (`TribeArchetypes`, easy / hard
//!   variants): the tables below are copied from them.
//! - Planner update 0xCE4A70, once per second: per plan type the cooldown
//!   counts down; once it is below zero every waiting plan's check timer counts
//!   down and a plan is "ready" for the one second its timer wraps (it restarts
//!   at `CheckTime`; the first wrap comes after `InitialDelay + CheckTime`,
//!   0xCE63E0).
//! - Selection 0xCE3080: a ready plan with executions left whose attitude band
//!   holds the tribe's attitude toward the PLAYER, with enough members (or a
//!   population cap below the requirement), enough food, the required tools and
//!   the player's population in its band. The only target is the player.
//! - End 0xCE31A0: the plan type's cooldown becomes `timeBetweenExecution`.
//! - Recruit 0xCE4500: `_numberMembersToUse` workers, `ceil(n x share)` of them
//!   with each tool of `_membersToolsProb`; the chieftain stays home.
//! - Assemble 0xCE4030: the workers gather at their own camp fire (within 0.75
//!   of the tribe radius).
//! - Raid 0x058B724D: start 0xCE1310 ("Incoming raiders!", the raid horn and
//!   music); every 2 s 0xCE5310 stops the raid once the attitude is above
//!   Annoyed and sends the workers at one of the player's standing tool racks
//!   (not the camp fire), else at the hut, else at the nearest member.
//! - Steal food 0x05931FEC (after the congregate step 0x05DCE234): RaidFood on
//!   the player's mat (T8C), stopped above Annoyed.
//! - Gift 0x058B7254: GiftFood on the player's tribe.
//!
//! INFERRED / ADAPTED (marked at the use): the behaviour tree's order (one plan
//! per village at a time, raid > steal food > gift), the congregate spot and
//! time, which rack is attacked first, the gift load (`giftFoodAmount` per
//! worker), gifts leave the relationship alone, repairs of damaged buildings.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::creature_rules::Difficulty;
use crate::game::{dist_xz, Health, RunProgress};
use crate::tribal::{self, RaidPhase, Raider, RivalStatus, RIVAL_VILLAGES};
use crate::tribal_rts::{RivalMember, RivalMode, TribeUnit};
use crate::tribal_tools::{self as tools, ToolKind};

// ===================================================================== attitude

/// The relationship manager's attitude (VERIFIED cut 0xD00BD0; thresholds
/// VERIFIED -70 / -30 / 30 / 70 in the campaign's x10 units: the `ui` props'
/// `RolloverRltnshp*Cap` read by 0xD00F10, see
/// `tribal_rules::FURIOUS_RELATION_LIMIT` and T9K).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Attitude {
    Furious = 0,
    Annoyed = 1,
    Cautious = 2,
    Pleased = 3,
    Friendly = 4,
}

/// Attitude for a relationship value: Furious at or below -70, Annoyed above it
/// up to -30, Cautious below 30, Pleased below 70, Friendly from 70.
pub fn attitude(relations: f32) -> Attitude {
    use crate::tribal_rules::{
        ALLY_RELATION_THRESHOLD, ANNOYED_RELATION_LIMIT, FRIENDLY_RELATION_LIMIT, FURIOUS_RELATION_LIMIT,
    };
    let v = if relations.is_finite() { relations } else { 0.0 };
    if v > ANNOYED_RELATION_LIMIT && v < FRIENDLY_RELATION_LIMIT {
        Attitude::Cautious
    } else if (FRIENDLY_RELATION_LIMIT..ALLY_RELATION_THRESHOLD).contains(&v) {
        Attitude::Pleased
    } else if v >= ALLY_RELATION_THRESHOLD {
        Attitude::Friendly
    } else if v > FURIOUS_RELATION_LIMIT {
        Attitude::Annoyed
    } else {
        Attitude::Furious
    }
}

// ======================================================================== plans

/// The plan types the player sees (TribePlanType Raid 1, StealFood 10, Gift
/// 2), in the order the planner tries them (ADAPTED: the behaviour tree's own
/// order is not decoded; the three attitude bands do not overlap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlanKind {
    Raid,
    StealFood,
    Gift,
}

impl PlanKind {
    pub const ALL: [PlanKind; 3] = [PlanKind::Raid, PlanKind::StealFood, PlanKind::Gift];
    pub fn index(self) -> usize {
        match self {
            PlanKind::Raid => 0,
            PlanKind::StealFood => 1,
            PlanKind::Gift => 2,
        }
    }
}

/// One `TribePlans` record (cTribePlanProperties, loader 0xCE02D0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TribePlan {
    pub name: &'static str,
    pub initial_delay: f32,
    pub check_time: f32,
    pub time_between: f32,
    /// Executions allowed (`frequency`; -1 = no limit).
    pub frequency: i32,
    /// `NumRequiredTribeMembers`.
    pub required_members: usize,
    /// `_numberMembersToUse`.
    pub members_to_use: usize,
    /// `RequiredTools`: tool types the tribe must own.
    pub required_tools: &'static [u8],
    /// `_membersToolsProb`: (tool type, share of the workers).
    pub tools: &'static [(u8, f32)],
    /// `min/maxRelationshipToTarget`: the attitude band (-1, -1 = any).
    pub attitude: (i8, i8),
    pub required_food: f32,
    pub desired_food: f32,
    pub exclude_chieftain: bool,
    /// `min/maxTargetPopulation`: the player's population band.
    pub target_population: (usize, usize),
}

const fn raid(
    name: &'static str,
    initial_delay: f32,
    check_time: f32,
    time_between: f32,
    required_members: usize,
    members_to_use: usize,
    required_tools: &'static [u8],
    tools: &'static [(u8, f32)],
) -> TribePlan {
    TribePlan {
        name,
        initial_delay,
        check_time,
        time_between,
        frequency: -1,
        required_members,
        members_to_use,
        required_tools,
        tools,
        attitude: (0, 0),
        required_food: 0.0,
        desired_food: 0.0,
        exclude_chieftain: true,
        target_population: (0, 100),
    }
}

// Tool types: 1 torch, 2 axe, 3 spear, 9 heal (T2B).
pub const BASICRAID1: TribePlan = raid("basicraid1", 0.0, 180.0, 720.0, 2, 2, &[], &[]);
pub const BASICRAID1_EASY: TribePlan = raid("basicraid1_easy", 0.0, 240.0, 720.0, 2, 2, &[], &[]);
pub const BASICRAID1_HARD: TribePlan = raid("basicraid1_hard", 0.0, 120.0, 240.0, 4, 5, &[], &[]);
pub const BASICRAID2: TribePlan = raid("basicraid2", 0.0, 180.0, 720.0, 4, 5, &[], &[(3, 0.9)]);
pub const BASICRAID2_EASY: TribePlan = raid("basicraid2_easy", 0.0, 240.0, 720.0, 2, 3, &[], &[(3, 0.7)]);
pub const BASICRAID2_HARD: TribePlan = raid("basicraid2_hard", 0.0, 120.0, 240.0, 5, 7, &[], &[(3, 1.0)]);
pub const BASICRAID3: TribePlan = raid("basicraid3", 0.0, 180.0, 720.0, 4, 5, &[], &[(9, 0.3), (3, 0.7)]);
pub const BASICRAID3_EASY: TribePlan = raid("basicraid3_easy", 0.0, 240.0, 720.0, 2, 3, &[], &[(9, 0.4), (3, 0.6)]);
pub const BASICRAID3_HARD: TribePlan = raid("basicraid3_hard", 0.0, 120.0, 240.0, 5, 7, &[], &[(9, 0.2), (3, 0.8)]);
pub const BASICRAID4: TribePlan = raid("basicraid4", 0.0, 180.0, 720.0, 4, 5, &[], &[(1, 0.7), (9, 0.3)]);
pub const BASICRAID4_EASY: TribePlan = raid("basicraid4_easy", 0.0, 240.0, 720.0, 2, 3, &[], &[(1, 0.6), (9, 0.4)]);
pub const BASICRAID4_HARD: TribePlan = raid("basicraid4_hard", 0.0, 120.0, 240.0, 5, 7, &[], &[(1, 0.7), (9, 0.3)]);
pub const BASICRAID5: TribePlan = raid("basicraid5", 0.0, 180.0, 720.0, 4, 5, &[1], &[(1, 0.9)]);
pub const BASICRAID5_EASY: TribePlan = raid("basicraid5_easy", 0.0, 240.0, 720.0, 2, 3, &[1], &[(1, 0.7)]);
pub const BASICRAID5_HARD: TribePlan = raid("basicraid5_hard", 0.0, 120.0, 240.0, 5, 7, &[1], &[(1, 1.0)]);
pub const BASICRAID7: TribePlan = raid("basicraid7", 0.0, 180.0, 720.0, 4, 5, &[], &[(9, 0.3), (2, 0.7)]);
pub const BASICRAID7_EASY: TribePlan = raid("basicraid7_easy", 0.0, 240.0, 720.0, 2, 3, &[], &[(9, 0.4), (2, 0.6)]);
/// Installed but listed by no hard archetype (hard 4a lists basicraid4_hard).
#[allow(dead_code)]
pub const BASICRAID7_HARD: TribePlan = raid("basicraid7_hard", 0.0, 120.0, 240.0, 5, 7, &[], &[(9, 0.2), (2, 0.8)]);
pub const BASICRAID8: TribePlan = raid("basicraid8", 0.0, 180.0, 720.0, 4, 5, &[], &[(2, 0.9)]);
pub const BASICRAID8_EASY: TribePlan = raid("basicraid8_easy", 0.0, 240.0, 720.0, 2, 3, &[], &[(2, 0.7)]);
pub const BASICRAID8_HARD: TribePlan = raid("basicraid8_hard", 0.0, 120.0, 240.0, 5, 7, &[], &[(2, 1.0)]);
pub const ADVANCEDRAID1: TribePlan =
    raid("advancedraid1", 120.0, 180.0, 300.0, 5, 7, &[], &[(2, 0.3), (3, 0.3), (1, 0.2), (9, 0.2)]);
pub const ADVANCEDRAID1_EASY: TribePlan =
    raid("advancedraid1_easy", 180.0, 300.0, 300.0, 4, 6, &[], &[(2, 0.2), (3, 0.2), (1, 0.2), (9, 0.3)]);
pub const ADVANCEDRAID1_HARD: TribePlan =
    raid("advancedraid1_hard", 120.0, 120.0, 120.0, 7, 9, &[], &[(2, 0.4), (3, 0.3), (1, 0.1), (9, 0.2)]);

const fn steal(name: &'static str, initial_delay: f32, check_time: f32, time_between: f32) -> TribePlan {
    TribePlan {
        name,
        initial_delay,
        check_time,
        time_between,
        frequency: -1,
        required_members: 1,
        members_to_use: 1,
        required_tools: &[],
        tools: &[],
        attitude: (1, 1),
        required_food: 0.0,
        desired_food: 0.0,
        exclude_chieftain: true,
        target_population: (0, 100),
    }
}

pub const STEALFOOD1: TribePlan = steal("stealfood1", 60.0, 180.0, 240.0);
pub const STEALFOOD1_EASY: TribePlan = steal("stealfood1_easy", 90.0, 240.0, 300.0);
pub const STEALFOOD1_HARD: TribePlan = steal("stealfood1_hard", 60.0, 150.0, 180.0);

const fn gift(name: &'static str, members_to_use: usize) -> TribePlan {
    TribePlan {
        name,
        initial_delay: 0.0,
        check_time: 540.0,
        time_between: 30.0,
        frequency: -1,
        required_members: 1,
        members_to_use,
        required_tools: &[],
        tools: &[],
        attitude: (3, 4),
        required_food: 50.0,
        desired_food: 100.0,
        exclude_chieftain: true,
        target_population: (0, 100),
    }
}

/// `giftplans` of the archetype template (every tribe, every difficulty):
/// largegift first.
pub const LARGEGIFT: TribePlan = gift("largegift", 3);
pub const SMALLGIFT: TribePlan = gift("smallgift", 2);

/// The `raidplans` of an archetype (`TribeArchetypes` and its easy / hard
/// variants; hard 4a lists basicraid4_hard as installed).
pub fn raid_plan(archetype: u8, difficulty: Difficulty) -> Option<&'static TribePlan> {
    use Difficulty::*;
    let plan = match (archetype, difficulty) {
        (1 | 2 | 15, Easy) => &BASICRAID1_EASY,
        (1 | 2 | 15, Medium) => &BASICRAID1,
        (1 | 2 | 15, Hard) => &BASICRAID1_HARD,
        (3, Easy) => &BASICRAID2_EASY,
        (3, Medium) => &BASICRAID2,
        (3, Hard) => &BASICRAID2_HARD,
        (4, Easy) => &BASICRAID3_EASY,
        (4, Medium) => &BASICRAID3,
        (4, Hard) => &BASICRAID3_HARD,
        (7, Easy) => &BASICRAID5_EASY,
        (7, Medium) => &BASICRAID5,
        (7, Hard) => &BASICRAID5_HARD,
        (8, Easy) => &BASICRAID4_EASY,
        (8, Medium) => &BASICRAID4,
        (8 | 11, Hard) => &BASICRAID4_HARD,
        (11, Easy) => &BASICRAID7_EASY,
        (11, Medium) => &BASICRAID7,
        (12, Easy) => &BASICRAID8_EASY,
        (12, Medium) => &BASICRAID8,
        (12, Hard) => &BASICRAID8_HARD,
        (14, Easy) => &ADVANCEDRAID1_EASY,
        (14, Medium) => &ADVANCEDRAID1,
        (14, Hard) => &ADVANCEDRAID1_HARD,
        _ => return None,
    };
    Some(plan)
}

/// `StealFoodPlans` (template: every archetype).
pub fn steal_food_plan(difficulty: Difficulty) -> &'static TribePlan {
    match difficulty {
        Difficulty::Easy => &STEALFOOD1_EASY,
        Difficulty::Medium => &STEALFOOD1,
        Difficulty::Hard => &STEALFOOD1_HARD,
    }
}

/// Every plan of a tribe the player meets, in planner order.
pub fn plan_list(archetype: u8, difficulty: Difficulty) -> Vec<(PlanKind, &'static TribePlan)> {
    let mut out = Vec::new();
    if let Some(p) = raid_plan(archetype, difficulty) {
        out.push((PlanKind::Raid, p));
    }
    out.push((PlanKind::StealFood, steal_food_plan(difficulty)));
    out.push((PlanKind::Gift, &LARGEGIFT));
    out.push((PlanKind::Gift, &SMALLGIFT));
    out
}

/// How many workers carry each tool (0xCE4500: `ceil(n x share)` per pair, in
/// order, never more than the workers left).
pub fn tool_counts(plan: &TribePlan, workers: usize) -> Vec<(u8, usize)> {
    let mut left = workers;
    let mut out = Vec::new();
    for (kind, share) in plan.tools {
        let want = ((workers as f32 * share) - 1e-4).ceil().max(0.0) as usize;
        let n = want.min(left);
        left -= n;
        if n > 0 {
            out.push((*kind, n));
        }
    }
    out
}

/// The `ToolKind` of a retail tool type.
pub fn tool_kind(kind: u8) -> Option<ToolKind> {
    tools::TOOLS.iter().map(|t| t.kind).find(|k| crate::tribal_rules::tool_type(*k) == kind)
}

// ====================================================================== planner

/// What the planner knows when it picks (the context of 0xCE4A70).
#[derive(Debug, Clone, Copy)]
pub struct PlanContext {
    pub attitude: Attitude,
    pub population: usize,
    pub max_population: usize,
    pub food: f32,
    /// Bit per owned tool type.
    pub tools: u32,
    pub target_population: usize,
}

/// One village's planner (saved; `#[serde(default)]` everywhere).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VillagePlanner {
    /// Sub-second time toward the next planner second.
    #[serde(default)]
    pub clock: f32,
    /// `mTimeBetweenPlanExecutionByType` for Raid, StealFood, Gift.
    #[serde(default)]
    pub cooldown: [f32; 3],
    /// Per plan of `plan_list`: the check timer (`mCheckTime`).
    #[serde(default)]
    pub timers: Vec<f32>,
    /// Per plan: executions left (-1 = no limit).
    #[serde(default)]
    pub executions: Vec<i32>,
    /// The plan being executed (runtime: its workers do not survive a load).
    #[serde(skip)]
    pub active: Option<ActivePlan>,
}

/// The executing plan of a village.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivePlan {
    pub kind: PlanKind,
    pub plan: usize,
    /// Workers recruited.
    pub workers: usize,
    /// The start node ran (event, horn).
    pub started: bool,
    /// Seconds since recruitment.
    pub age: f32,
}

impl VillagePlanner {
    /// A planner for these plans (0xCE63E0: timer = InitialDelay + CheckTime).
    pub fn new(plans: &[(PlanKind, &'static TribePlan)]) -> Self {
        Self {
            clock: 0.0,
            cooldown: [0.0; 3],
            timers: plans.iter().map(|(_, p)| p.initial_delay + p.check_time).collect(),
            executions: plans.iter().map(|(_, p)| p.frequency).collect(),
            active: None,
        }
    }

    fn fits(&self, plans: &[(PlanKind, &'static TribePlan)]) -> bool {
        self.timers.len() == plans.len() && self.executions.len() == plans.len()
    }
}

/// Whether a plan's conditions hold (0xCE3080 without the ready test).
pub fn plan_allowed(plan: &TribePlan, ctx: &PlanContext) -> bool {
    let (lo, hi) = plan.attitude;
    let attitude_ok = (lo == -1 && hi == -1) || (lo as i32 <= ctx.attitude as i32 && ctx.attitude as i32 <= hi as i32);
    let members_ok = plan.required_members <= ctx.population || ctx.max_population < plan.required_members;
    let tools_ok = plan.required_tools.iter().all(|t| ctx.tools & (1 << *t) != 0);
    attitude_ok
        && members_ok
        && ctx.food >= plan.required_food
        && tools_ok
        && plan.target_population.0 <= ctx.target_population
        && ctx.target_population <= plan.target_population.1
}

/// One planner second (0xCE4A70 + 0xCE3080). Returns the plan to start, if
/// the village is free and one is ready and allowed.
pub fn planner_second(
    planner: &mut VillagePlanner,
    plans: &[(PlanKind, &'static TribePlan)],
    ctx: &PlanContext,
) -> Option<usize> {
    if !planner.fits(plans) {
        *planner = VillagePlanner::new(plans);
    }
    let busy = planner.active.map(|a| a.plan);
    let mut ready = vec![false; plans.len()];
    for kind in PlanKind::ALL {
        let k = kind.index();
        planner.cooldown[k] -= 1.0;
        if planner.cooldown[k] >= 0.0 {
            continue;
        }
        for (i, (pk, plan)) in plans.iter().enumerate() {
            if *pk != kind || busy == Some(i) {
                continue;
            }
            planner.timers[i] -= 1.0;
            if planner.timers[i] < 0.0 {
                ready[i] = true;
                planner.timers[i] = plan.check_time;
            }
        }
    }
    if planner.active.is_some() {
        return None;
    }
    // ADAPTED: one plan per village at a time, tried in `PlanKind` order. Retail
    // marks plans, not villages, as executing (0xCE4A70: the check timer of every
    // plan that is not executing counts down; 0xCE31A0 sets the cooldown of the
    // finished plan's type) and its behaviour tree (nodes 0x157F6C0.., not
    // decoded) may start a plan of another type while one runs; the three kinds
    // here need disjoint attitudes, so only an attitude change mid-plan shows it.
    for kind in PlanKind::ALL {
        for (i, (pk, plan)) in plans.iter().enumerate() {
            if *pk == kind && ready[i] && planner.executions[i] != 0 && plan_allowed(plan, ctx) {
                if planner.executions[i] > 0 {
                    planner.executions[i] -= 1;
                }
                return Some(i);
            }
        }
    }
    None
}

/// The plan ended (0xCE31A0): its type waits `timeBetweenExecution`.
pub fn finish_plan(planner: &mut VillagePlanner, plans: &[(PlanKind, &'static TribePlan)]) {
    if let Some(active) = planner.active.take() {
        if let Some((kind, plan)) = plans.get(active.plan) {
            planner.cooldown[kind.index()] = plan.time_between;
        }
    }
}

// ================================================================= the workers

/// What a raider is after (0xCE5310).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RaidTarget {
    #[default]
    None,
    /// The player's tool rack.
    Shack(ToolKind),
    /// The player's hut.
    Hut,
    /// One of the player's members.
    Member(Entity),
}

/// Which plan sent a raider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RaidKind {
    /// A raid plan: tool racks, then the hut, then the members.
    Raid,
    /// A steal-food plan: one bundle from the mat.
    #[default]
    StealFood,
}

/// A worker of a gift plan carrying food to the player's mat.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GiftBearer {
    pub phase: GiftPhase,
    pub carry: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftPhase {
    Assemble,
    Approach,
    Return,
    /// Home again: the planner releases it.
    Done,
}

/// A worker's tool for the raid (marker on the mounted model).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaidTool;

/// Within this of their camp fire the workers have assembled (0xCE4030:
/// 0.75 x the tribe radius 30).
pub const ASSEMBLE_RADIUS: f32 = 0.75 * crate::tribal_rules::TRIBE_ZONING_RADIUS;
/// ADAPTED: where a food thief congregates before it steals (0xCE1510 walks
/// toward the target tribe with an untraced distance): just outside the
/// player's village.
pub const CONGREGATE_DISTANCE: f32 = crate::tribal_rules::TRIBE_ZONING_RADIUS + 10.0;
/// How long the congregate clip holds the thief: the step ends when the
/// workers' `raid_congregate_01` (0x05DCC520, ANIM 0x71D0A3D5: 177 frames at 30
/// per second) is done (T9B, 0xCE1690). VERIFIED against the installed ANIM.
pub const CONGREGATE_SECONDS: f32 = 177.0 / 30.0;
/// Reach of a raider striking a tool rack / the hut.
pub const RACK_REACH: f32 = 2.5;
pub const HUT_REACH: f32 = 3.0;
/// The behaviour tick of the raid / steal nodes (0xCE5310 / 0xCE1990: 2 s).
pub const BEHAVIOUR_TICK: f32 = 2.0;
/// The retail event text of both raid starts (182CD6CE!166B09F6 /
/// !B87901DE, 5FCCA927:0D).
pub const INCOMING_RAIDERS: &str = "Incoming raiders!";

/// Where a food thief congregates: on the line from the player's hut toward
/// its own village, `CONGREGATE_DISTANCE` out.
pub fn congregate_spot(home: Vec3) -> Vec3 {
    let hut = tribal::home_position();
    let dir = Vec2::new(home.x - hut.x, home.z - hut.z).normalize_or(Vec2::X);
    tribal::keep_on_land(hut + Vec3::new(dir.x, 0.0, dir.y) * CONGREGATE_DISTANCE)
}

/// The raid's target (0xCE5310): a standing tool rack (not the camp fire;
/// ADAPTED: the one nearest to `from`, the retail preference test 0xC9D2A0 is
/// not traced), else the hut while it stands, else the nearest member.
pub fn raid_target(run: &RunProgress, from: Vec3, members: &[(Entity, Vec3)]) -> RaidTarget {
    let campaign = &run.tribal_campaign;
    let rack = campaign
        .shacks
        .iter()
        .enumerate()
        .filter(|(_, k)| **k != ToolKind::Campfire && !campaign.shack_destroyed(**k))
        .min_by(|a, b| {
            dist_xz(from, tools::shack_position(a.0)).total_cmp(&dist_xz(from, tools::shack_position(b.0)))
        })
        .map(|(_, k)| *k);
    if let Some(kind) = rack {
        return RaidTarget::Shack(kind);
    }
    if !campaign.home_hut_destroyed() {
        return RaidTarget::Hut;
    }
    members
        .iter()
        .min_by(|a, b| dist_xz(from, a.1).total_cmp(&dist_xz(from, b.1)))
        .map_or(RaidTarget::None, |(e, _)| RaidTarget::Member(*e))
}

/// Where a raid target stands.
pub fn target_position(run: &RunProgress, target: RaidTarget, members: &[(Entity, Vec3)]) -> Option<Vec3> {
    match target {
        RaidTarget::None => None,
        RaidTarget::Shack(kind) => run
            .tribal_campaign
            .shacks
            .iter()
            .position(|k| *k == kind)
            .map(tools::shack_position),
        RaidTarget::Hut => Some(tribal::home_position()),
        RaidTarget::Member(e) => members.iter().find(|(m, _)| *m == e).map(|(_, p)| *p),
    }
}

/// One frame of a raider: where it walks (and whether it runs) and what it
/// does on arrival. `members` are the player's living units.
pub struct RaiderStep {
    pub goal: Option<Vec3>,
    pub run: bool,
}

pub fn step_raider(
    run: &mut RunProgress,
    r: &mut RivalMember,
    raider: &mut Raider,
    pos: Vec3,
    dt: f32,
    members: &[(Entity, Vec3)],
) -> RaiderStep {
    let walk = |goal| RaiderStep { goal: Some(goal), run: false };
    let dash = |goal| RaiderStep { goal: Some(goal), run: true };
    match raider.phase {
        RaidPhase::Assemble => {
            if dist_xz(pos, r.home) > ASSEMBLE_RADIUS * 0.5 {
                walk(r.home)
            } else {
                RaiderStep { goal: None, run: false }
            }
        }
        RaidPhase::Congregate => {
            let spot = congregate_spot(r.home);
            if dist_xz(pos, spot) > 1.5 {
                dash(spot)
            } else {
                raider.timer -= dt;
                RaiderStep { goal: None, run: false }
            }
        }
        RaidPhase::Approach => {
            // The food thief heads for the mat (RaidFood 8).
            let mat = tribal::food_pile_position();
            if dist_xz(pos, mat) < 2.0 {
                raider.phase = RaidPhase::Pillage;
                RaiderStep { goal: None, run: false }
            } else {
                dash(mat)
            }
        }
        RaidPhase::Pillage => {
            // T8C: one bundle (carryBundleAmountMax 10) from the stock and one
            // TribeRaid event for the pair (-30, floor -60).
            let take = crate::tribal_rules::raid_take(run.dna_this_stage.max(0.0));
            run.dna_this_stage -= take;
            r.stolen += take;
            if take > 0.0 {
                run.tribal_campaign.post_event(r.village, crate::tribal_rules::RelationEvent::Raid);
                run.tribal_campaign.say(format!("V{} raiders made off with {:.0} food!", r.village + 1, take));
            }
            raider.phase = RaidPhase::Retreat;
            dash(r.home)
        }
        RaidPhase::Raze => {
            let Some(at) = target_position(run, raider.target, members) else {
                return RaiderStep { goal: None, run: false };
            };
            let reach = if raider.target == RaidTarget::Hut { HUT_REACH } else { RACK_REACH };
            if dist_xz(pos, at) > reach {
                return dash(at);
            }
            // RaidHuts 15: the worker's weapon razes (`creatureAbilityRazeDamage`
            // per refresh, bare hands without a tool).
            let damage = crate::tribal_rules::weapon_of(raider.tool).raze_dps() * dt;
            let campaign = &mut run.tribal_campaign;
            match raider.target {
                RaidTarget::Shack(kind) => {
                    if campaign.damage_shack(kind, damage) {
                        let name = tools::info(kind).name;
                        campaign.say(format!("V{} raiders destroyed your {name}!", r.village + 1));
                        run.request_save();
                    }
                }
                RaidTarget::Hut => {
                    let before = campaign.home_hut_destroyed();
                    campaign.damage_home_hut(damage);
                    if !before && campaign.home_hut_destroyed() {
                        campaign.say(format!("V{} raiders destroyed your hut!", r.village + 1));
                        run.request_save();
                    }
                }
                _ => {}
            }
            RaiderStep { goal: None, run: false }
        }
        RaidPhase::Fight => match target_position(run, raider.target, members) {
            // Attack 7: the combat system strikes within reach.
            Some(at) if dist_xz(pos, at) > 1.4 => dash(at),
            _ => RaiderStep { goal: None, run: false },
        },
        RaidPhase::Retreat => {
            if dist_xz(pos, r.home) < 3.0 {
                if r.stolen > 0.0 {
                    // The thief's bundle joins its tribe's stock.
                    let stock = run.tribal_campaign.village_food(r.village);
                    run.tribal_campaign.set_village_food(r.village, stock + r.stolen);
                    r.stolen = 0.0;
                }
                raider.phase = RaidPhase::Done;
                RaiderStep { goal: None, run: false }
            } else {
                dash(r.home)
            }
        }
        RaidPhase::Done => RaiderStep { goal: None, run: false },
    }
}

/// One frame of a gift bearer.
pub fn step_gift_bearer(run: &mut RunProgress, r: &RivalMember, bearer: &mut GiftBearer, pos: Vec3) -> RaiderStep {
    match bearer.phase {
        GiftPhase::Assemble => {
            if dist_xz(pos, r.home) > ASSEMBLE_RADIUS * 0.5 {
                RaiderStep { goal: Some(r.home), run: false }
            } else {
                RaiderStep { goal: None, run: false }
            }
        }
        GiftPhase::Approach => {
            let mat = tribal::food_pile_position();
            if dist_xz(pos, mat) < 2.0 {
                // GiftFood 9. VERIFIED (0xDBC1F1..0xDBC25C): a delivered gift posts
                // `TribeGift` for the RECEIVING tribe toward the giver (scale
                // `-relationship`) only while its attitude is below Cautious; the
                // gift plans run at Pleased..Friendly (`tribe_plans`), so an NPC
                // gift never moves the relationship.
                if bearer.carry > 0.0 {
                    run.award_dna(bearer.carry, &crate::stages::GameStage::Tribal);
                    run.tribal_campaign
                        .say(format!("V{} brings a gift of {:.0} food.", r.village + 1, bearer.carry));
                    bearer.carry = 0.0;
                }
                bearer.phase = GiftPhase::Return;
                RaiderStep { goal: Some(r.home), run: false }
            } else {
                RaiderStep { goal: Some(mat), run: false }
            }
        }
        GiftPhase::Return => {
            if dist_xz(pos, r.home) < 3.0 {
                bearer.phase = GiftPhase::Done;
                RaiderStep { goal: None, run: false }
            } else {
                RaiderStep { goal: Some(r.home), run: false }
            }
        }
        GiftPhase::Done => RaiderStep { goal: None, run: false },
    }
}

// ======================================================================= system

/// Context of village `v` for its planner.
fn context(run: &RunProgress, v: usize, population: usize) -> PlanContext {
    let campaign = &run.tribal_campaign;
    let tribe = campaign.tribe_of(v);
    // ADAPTED: an NPC tribe owns the tools of its `toolprefs` (its racks are
    // the abstract village pieces).
    let tools = tribe.tools.iter().fold(0u32, |m, t| m | (1 << *t));
    PlanContext {
        attitude: campaign.village(v).map_or(Attitude::Cautious, |s| attitude(s.relations)),
        population,
        max_population: tribe.max_population,
        food: campaign.village_food(v),
        tools,
        target_population: run.tribe.len(),
    }
}

/// The NPC planners: once per second every present tribe ticks its plans and
/// may start one (recruiting its workers); every frame the behaviour nodes
/// advance (assemble, start, targets, abort) and finished workers go home.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn npc_planner(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut tick: Local<f32>,
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rivals: Query<
        (
            Entity,
            &Transform,
            &mut RivalMember,
            Option<&mut Raider>,
            Option<&mut GiftBearer>,
            &Health,
        ),
        Without<TribeUnit>,
    >,
    tribe: Query<(Entity, &Transform, &Health), (With<TribeUnit>, Without<RivalMember>)>,
    held: Query<(Entity, &ChildOf), With<RaidTool>>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let dt = time.delta_secs();
    if !(dt > 0.0) {
        return;
    }
    let run = &mut *run;
    let difficulty = Difficulty::from_index(run.cell.difficulty);
    let members: Vec<(Entity, Vec3)> = tribe
        .iter()
        .filter(|(_, _, h)| !h.dead())
        .map(|(e, t, _)| (e, t.translation))
        .collect();
    if run.tribal_campaign.npc_plans.len() != RIVAL_VILLAGES {
        run.tribal_campaign.npc_plans.resize_with(RIVAL_VILLAGES, VillagePlanner::default);
    }
    raid_test_hook(run, difficulty);

    // Release finished workers (home again): back to village life.
    for (e, _, mut r, raider, bearer, _) in &mut rivals {
        let done = raider.as_ref().is_some_and(|x| x.phase == RaidPhase::Done)
            || bearer.as_ref().is_some_and(|b| b.phase == GiftPhase::Done);
        if done {
            commands.entity(e).remove::<(Raider, GiftBearer)>();
            r.mode = RivalMode::Wander;
            r.target = r.home;
            for (tool, parent) in &held {
                if parent.parent() == e {
                    commands.entity(tool).despawn();
                }
            }
        }
    }

    // Per village: who is home, who works.
    let mut idle: [Vec<(Entity, f32)>; RIVAL_VILLAGES] = Default::default();
    let mut workers = [0usize; RIVAL_VILLAGES];
    let mut population = [0usize; RIVAL_VILLAGES];
    for (e, t, r, raider, bearer, h) in rivals.iter() {
        if h.dead() || r.village >= RIVAL_VILLAGES {
            continue;
        }
        population[r.village] += 1;
        let working = raider.is_some_and(|x| x.phase != RaidPhase::Done)
            || bearer.is_some_and(|b| b.phase != GiftPhase::Done);
        if working {
            workers[r.village] += 1;
        } else if raider.is_none() && bearer.is_none() {
            idle[r.village].push((e, dist_xz(t.translation, r.home)));
        }
    }

    *tick += dt;
    let behaviour = *tick >= BEHAVIOUR_TICK;
    if behaviour {
        *tick = 0.0;
    }
    for v in 0..RIVAL_VILLAGES {
        let Some(state) = run.tribal_campaign.village(v) else {
            run.tribal_campaign.npc_plans[v].active = None;
            continue;
        };
        if state.status == RivalStatus::Conquered {
            // A defeated tribe plans nothing (cTribe +0x556, 0xCE4A70).
            run.tribal_campaign.npc_plans[v].active = None;
            continue;
        }
        let archetype = run.tribal_campaign.tribe_of(v).archetype;
        let plans = plan_list(archetype, difficulty);
        // The plan ends when no worker is left (0xCE5310 / 0xCE1990).
        if let Some(mut active) = run.tribal_campaign.npc_plans[v].active {
            active.age += dt;
            run.tribal_campaign.npc_plans[v].active = Some(active);
            if workers[v] == 0 && active.age > 0.5 {
                finish_plan(&mut run.tribal_campaign.npc_plans[v], &plans);
            }
        }
        // Planner seconds.
        run.tribal_campaign.npc_plans[v].clock += dt;
        while run.tribal_campaign.npc_plans[v].clock >= 1.0 {
            run.tribal_campaign.npc_plans[v].clock -= 1.0;
            let ctx = context(run, v, population[v]);
            let Some(index) = planner_second(&mut run.tribal_campaign.npc_plans[v], &plans, &ctx) else {
                continue;
            };
            let (kind, plan) = plans[index];
            // Recruit 0xCE4500: the members nearest their fire.
            idle[v].sort_by(|a, b| a.1.total_cmp(&b.1));
            let count = plan.members_to_use.min(idle[v].len());
            if count == 0 {
                continue;
            }
            let chosen: Vec<Entity> = idle[v].drain(..count).map(|(e, _)| e).collect();
            let mut tool_list: Vec<Option<ToolKind>> = Vec::new();
            for (t, n) in tool_counts(plan, count) {
                let owned = ctx.tools & (1 << t) != 0;
                for _ in 0..n {
                    tool_list.push(if owned { tool_kind(t) } else { None });
                }
            }
            let mut carry_left = 0.0;
            if kind == PlanKind::Gift {
                // ADAPTED: each bearer carries `giftFoodAmount` out of the stock.
                let stock = run.tribal_campaign.village_food(v);
                carry_left = (count as f32 * crate::tribal_rules::GIFT_FOOD_AMOUNT).min(stock);
                run.tribal_campaign.set_village_food(v, stock - carry_left);
            }
            for (i, e) in chosen.iter().enumerate() {
                let tool = tool_list.get(i).copied().flatten();
                match kind {
                    PlanKind::Raid | PlanKind::StealFood => {
                        commands.entity(*e).insert(Raider {
                            hp: crate::tribal_rules::npc_citizen_max_hp(),
                            phase: RaidPhase::Assemble,
                            timer: CONGREGATE_SECONDS,
                            kind: if kind == PlanKind::Raid { RaidKind::Raid } else { RaidKind::StealFood },
                            target: RaidTarget::None,
                            tool,
                        });
                        if let Some(kind) = tool {
                            mount_raid_tool(&mut commands, &mut originals, &mut meshes, &mut images, &mut materials, *e, kind);
                        }
                    }
                    PlanKind::Gift => {
                        let carry = crate::tribal_rules::GIFT_FOOD_AMOUNT.min(carry_left);
                        carry_left -= carry;
                        commands.entity(*e).insert(GiftBearer { phase: GiftPhase::Assemble, carry });
                    }
                }
            }
            for (ent, _, mut r, ..) in &mut rivals {
                if chosen.contains(&ent) {
                    r.mode = RivalMode::Raid;
                }
            }
            run.tribal_campaign.npc_plans[v].active =
                Some(ActivePlan { kind, plan: index, workers: count, started: false, age: 0.0 });
            info!("V{} starts plan {} with {} workers", v + 1, plan.name, count);
        }
    }

    if !behaviour {
        return;
    }
    // Behaviour nodes, every 2 s.
    for v in 0..RIVAL_VILLAGES {
        let Some(mut active) = run.tribal_campaign.npc_plans[v].active else {
            continue;
        };
        let Some(state) = run.tribal_campaign.village(v) else {
            continue;
        };
        let attitude = attitude(state.relations);
        let home = tribal::village_position(v);
        // Abort above Annoyed (0xCE5310 / 0xCE1810 / 0xCE1990).
        let abort = matches!(active.kind, PlanKind::Raid | PlanKind::StealFood) && attitude > Attitude::Annoyed;
        let mut all_assembled = true;
        let mut centre = Vec3::ZERO;
        let mut n = 0.0;
        for (_, t, r, raider, bearer, h) in rivals.iter() {
            if r.village != v || h.dead() {
                continue;
            }
            let assembling = raider.as_ref().is_some_and(|x| x.phase == RaidPhase::Assemble)
                || bearer.as_ref().is_some_and(|b| b.phase == GiftPhase::Assemble);
            if raider.is_some() || bearer.is_some() {
                centre += t.translation;
                n += 1.0;
            }
            if assembling && dist_xz(t.translation, home) > ASSEMBLE_RADIUS {
                all_assembled = false;
            }
        }
        if n > 0.0 {
            centre /= n;
        }
        // The start node runs once every recruited worker is on the map and at
        // the fire.
        let start_now = !active.started && n > 0.0 && all_assembled && !abort;
        if start_now {
            active.started = true;
            run.tribal_campaign.npc_plans[v].active = Some(active);
            match active.kind {
                PlanKind::Raid => {
                    // 0xCE1310: the event, the raid horn and music (game_audio
                    // plays the horn when raiders appear).
                    run.tribal_campaign.say(INCOMING_RAIDERS);
                    info!("V{} raid starts", v + 1);
                }
                PlanKind::StealFood | PlanKind::Gift => {}
            }
        }
        let target = if active.kind == PlanKind::Raid && active.started {
            raid_target(run, centre, &members)
        } else {
            RaidTarget::None
        };
        for (_, _, r, raider, bearer, h) in &mut rivals {
            if r.village != v || h.dead() {
                continue;
            }
            if let Some(mut raider) = raider {
                if raider.phase == RaidPhase::Done {
                    continue;
                }
                if abort {
                    if raider.phase != RaidPhase::Retreat {
                        raider.phase = RaidPhase::Retreat;
                    }
                    continue;
                }
                match (raider.kind, raider.phase) {
                    (RaidKind::Raid, RaidPhase::Assemble) if active.started => {}
                    (RaidKind::Raid, RaidPhase::Assemble) => continue,
                    (RaidKind::StealFood, RaidPhase::Assemble) if active.started => {
                        raider.phase = RaidPhase::Congregate;
                        raider.timer = CONGREGATE_SECONDS;
                        continue;
                    }
                    (RaidKind::StealFood, RaidPhase::Congregate) if raider.timer <= 0.0 => {
                        // 0xCE1810: the thief goes for the mat.
                        raider.phase = RaidPhase::Approach;
                        run.tribal_campaign.say(INCOMING_RAIDERS);
                        continue;
                    }
                    (RaidKind::StealFood, _) => continue,
                    (RaidKind::Raid, RaidPhase::Retreat) => continue,
                    _ => {}
                }
                // The raid's target for every worker (0xCE5310).
                raider.target = target;
                raider.phase = match target {
                    RaidTarget::Shack(_) | RaidTarget::Hut => RaidPhase::Raze,
                    RaidTarget::Member(_) => RaidPhase::Fight,
                    RaidTarget::None => RaidPhase::Retreat,
                };
            } else if let Some(mut bearer) = bearer {
                if bearer.phase == GiftPhase::Assemble && active.started {
                    bearer.phase = GiftPhase::Approach;
                }
            }
        }
    }
}

/// Dev hook for captures: `OPENSPORE_NPC_RAID_TEST=raid` (or `steal`, `gift`)
/// makes the first tribe Furious (Annoyed, Pleased) and lets its plan of that
/// kind come up at the next planner second, once per run of the process.
fn raid_test_hook(run: &mut RunProgress, difficulty: Difficulty) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mode) = std::env::var("OPENSPORE_NPC_RAID_TEST") else {
        DONE.store(true, Ordering::Relaxed);
        return;
    };
    let (kind, relations) = match mode.as_str() {
        "steal" => (PlanKind::StealFood, -50.0),
        "gift" => (PlanKind::Gift, 50.0),
        _ => (PlanKind::Raid, -100.0),
    };
    if run.tribal_campaign.villages.is_empty() {
        return;
    }
    DONE.store(true, Ordering::Relaxed);
    let v = 0;
    run.tribal_campaign.villages[v].appeared = true;
    run.tribal_campaign.villages[v].relations = relations;
    let plans = plan_list(run.tribal_campaign.tribe_of(v).archetype, difficulty);
    let planner = &mut run.tribal_campaign.npc_plans[v];
    *planner = VillagePlanner::new(&plans);
    for (i, (k, _)) in plans.iter().enumerate() {
        if *k == kind {
            planner.timers[i] = 0.0;
            break;
        }
    }
    info!("npc raid test: V1 {mode} (relations {relations})");
}

/// Mount the tool a raid worker grabbed (0xCE4500: GrabTool 3); the model is
/// tagged `RaidTool` so the worker drops it when it is released.
fn mount_raid_tool(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    unit: Entity,
    kind: ToolKind,
) {
    tools::mount_held_tool(commands, originals, meshes, images, materials, unit, Some(kind), false);
    // The pivot child carries `HeldTool(Some(kind))`; tag it next frame.
    commands.queue(move |world: &mut World| {
        let mut q = world.query::<(Entity, &tools::HeldTool, &ChildOf)>();
        let found: Vec<Entity> =
            q.iter(world).filter(|(_, h, c)| c.parent() == unit && h.0 == Some(kind)).map(|(e, _, _)| e).collect();
        for e in found {
            world.entity_mut(e).insert(RaidTool);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(attitude: Attitude) -> PlanContext {
        PlanContext {
            attitude,
            population: 6,
            max_population: 9,
            food: 200.0,
            tools: (1 << 1) | (1 << 2) | (1 << 3) | (1 << 9),
            target_population: 5,
        }
    }

    #[test]
    fn attitude_cuts_follow_the_verified_thresholds() {
        assert_eq!(attitude(-100.0), Attitude::Furious);
        assert_eq!(attitude(-70.0), Attitude::Furious);
        assert_eq!(attitude(-69.0), Attitude::Annoyed);
        assert_eq!(attitude(-30.0), Attitude::Annoyed);
        assert_eq!(attitude(-29.0), Attitude::Cautious);
        assert_eq!(attitude(29.0), Attitude::Cautious);
        assert_eq!(attitude(30.0), Attitude::Pleased);
        assert_eq!(attitude(70.0), Attitude::Friendly);
    }

    #[test]
    fn archetypes_pick_their_installed_raid_plans() {
        use Difficulty::*;
        assert_eq!(raid_plan(1, Medium).unwrap().name, "basicraid1");
        assert_eq!(raid_plan(15, Hard).unwrap().name, "basicraid1_hard");
        assert_eq!(raid_plan(3, Medium).unwrap().name, "basicraid2");
        assert_eq!(raid_plan(4, Easy).unwrap().name, "basicraid3_easy");
        assert_eq!(raid_plan(7, Medium).unwrap().name, "basicraid5");
        assert_eq!(raid_plan(8, Medium).unwrap().name, "basicraid4");
        assert_eq!(raid_plan(11, Hard).unwrap().name, "basicraid4_hard", "hard 4a lists basicraid4_hard");
        assert_eq!(raid_plan(12, Medium).unwrap().name, "basicraid8");
        assert_eq!(raid_plan(14, Medium).unwrap().name, "advancedraid1");
        assert!(raid_plan(0, Medium).is_none());
        let list = plan_list(1, Medium);
        let names: Vec<&str> = list.iter().map(|(_, p)| p.name).collect();
        assert_eq!(names, ["basicraid1", "stealfood1", "largegift", "smallgift"]);
    }

    #[test]
    fn a_furious_tribe_raids_at_its_first_check_and_waits_its_cooldown() {
        let plans = plan_list(1, Difficulty::Medium);
        let mut p = VillagePlanner::new(&plans);
        let furious = ctx(Attitude::Furious);
        // basicraid1: InitialDelay 0 + CheckTime 180: the timer wraps after
        // 181 planner seconds (it must go below zero).
        let mut started = None;
        for s in 1..=200 {
            if let Some(i) = planner_second(&mut p, &plans, &furious) {
                started = Some((s, i));
                break;
            }
        }
        assert_eq!(started, Some((181, 0)));
        p.active = Some(ActivePlan { kind: PlanKind::Raid, plan: 0, workers: 2, started: true, age: 1.0 });
        finish_plan(&mut p, &plans);
        assert_eq!(p.cooldown[0], 720.0, "timeBetweenExecution");
        // While the raid type cools down its timer stands still; the next raid
        // comes after the cooldown and one more check period.
        let mut next = None;
        for s in 1..=2000 {
            if planner_second(&mut p, &plans, &furious).is_some() {
                next = Some(s);
                break;
            }
        }
        let next = next.expect("a second raid");
        assert!(next > 720 && next <= 720 + 182, "second raid after {next} s");
    }

    #[test]
    fn plans_need_their_attitude_members_tools_and_food() {
        let plans = plan_list(7, Difficulty::Medium); // 3a: basicraid5 needs torches
        let raid = plans[0].1;
        assert!(plan_allowed(raid, &ctx(Attitude::Furious)));
        assert!(!plan_allowed(raid, &ctx(Attitude::Annoyed)), "raids only when Furious");
        let mut few = ctx(Attitude::Furious);
        few.population = 3;
        assert!(!plan_allowed(raid, &few), "4 members required");
        few.max_population = 3;
        assert!(plan_allowed(raid, &few), "a tribe that can never have 4 may still go");
        let mut unarmed = ctx(Attitude::Furious);
        unarmed.tools = 1 << 2;
        assert!(!plan_allowed(raid, &unarmed), "RequiredTools torch");
        let steal = plans[1].1;
        assert!(plan_allowed(steal, &ctx(Attitude::Annoyed)));
        assert!(!plan_allowed(steal, &ctx(Attitude::Furious)));
        let gift = plans[2].1;
        assert!(plan_allowed(gift, &ctx(Attitude::Pleased)));
        assert!(plan_allowed(gift, &ctx(Attitude::Friendly)));
        let mut poor = ctx(Attitude::Pleased);
        poor.food = 40.0;
        assert!(!plan_allowed(gift, &poor), "RequiredFood 50");
    }

    #[test]
    fn an_annoyed_tribe_sends_its_thief_after_initial_delay_and_check() {
        let plans = plan_list(2, Difficulty::Medium);
        let mut p = VillagePlanner::new(&plans);
        let annoyed = ctx(Attitude::Annoyed);
        let first = (1..=400).find(|_| planner_second(&mut p, &plans, &annoyed).is_some());
        // stealfood1: 60 + 180, wrapping below zero one second later.
        assert_eq!(first, Some(241));
    }

    #[test]
    fn a_missed_check_waits_a_whole_check_period() {
        let plans = plan_list(1, Difficulty::Medium);
        let mut p = VillagePlanner::new(&plans);
        let calm = ctx(Attitude::Cautious);
        for _ in 0..181 {
            assert!(planner_second(&mut p, &plans, &calm).is_none());
        }
        // The raid's chance passed while the tribe was calm: turning Furious
        // now waits for the next wrap, 181 s after the first.
        let furious = ctx(Attitude::Furious);
        let next = (1..=400).find(|_| planner_second(&mut p, &plans, &furious).is_some());
        assert_eq!(next, Some(181));
    }

    #[test]
    fn workers_take_their_tool_shares() {
        assert_eq!(tool_counts(&BASICRAID2, 5), vec![(3, 5)]);
        assert_eq!(tool_counts(&BASICRAID3, 5), vec![(9, 2), (3, 3)]);
        assert_eq!(tool_counts(&ADVANCEDRAID1, 7), vec![(2, 3), (3, 3), (1, 1)]);
        assert_eq!(tool_counts(&BASICRAID1, 2), vec![]);
        assert_eq!(tool_kind(1), Some(ToolKind::SpikedClubs));
        assert_eq!(tool_kind(9), Some(ToolKind::HealingRods));
    }

    #[test]
    fn raiders_go_for_racks_then_the_hut_then_members() {
        let mut run = RunProgress::default();
        run.tribal_campaign.shacks = vec![ToolKind::Campfire, ToolKind::StoneAxes, ToolKind::WoodenHorns];
        let from = Vec3::new(100.0, 0.0, 0.0);
        let near = match raid_target(&run, from, &[]) {
            RaidTarget::Shack(k) => k,
            other => panic!("{other:?}"),
        };
        assert_ne!(near, ToolKind::Campfire, "the camp fire is never a raid target");
        let health = crate::tribal_rules::tool_health(ToolKind::StoneAxes);
        assert!(run.tribal_campaign.damage_shack(ToolKind::StoneAxes, health));
        assert!(run.tribal_campaign.damage_shack(ToolKind::WoodenHorns, health));
        assert_eq!(raid_target(&run, from, &[]), RaidTarget::Hut);
        run.tribal_campaign.damage_home_hut(10_000.0);
        let mut world = World::new();
        let e = world.spawn_empty().id();
        assert_eq!(raid_target(&run, from, &[(e, Vec3::ZERO)]), RaidTarget::Member(e));
        assert_eq!(raid_target(&run, from, &[]), RaidTarget::None);
    }

    #[test]
    fn a_thief_takes_one_bundle_posts_one_raid_and_brings_it_home() {
        let mut run = RunProgress::default();
        run.tribal_campaign.ensure_ready_seeded(1, Difficulty::Medium);
        run.tribal_campaign.villages[0].appeared = true;
        run.tribal_campaign.villages[0].relations = -40.0;
        run.dna_this_stage = 35.0;
        let home = Vec3::new(200.0, 0.0, 0.0);
        let mut r = RivalMember {
            village: 0,
            home,
            target: home,
            wait: 0.0,
            mode: RivalMode::Raid,
            stolen: 0.0,
            speed: 4.2,
        };
        let mut raider = Raider { phase: RaidPhase::Pillage, ..Raider::default() };
        let stock = run.tribal_campaign.village_food(0);
        step_raider(&mut run, &mut r, &mut raider, tribal::food_pile_position(), 0.1, &[]);
        assert_eq!(run.dna_this_stage, 25.0, "a bundle of carryBundleAmountMax 10");
        assert_eq!(r.stolen, 10.0);
        assert_eq!(run.tribal_campaign.villages[0].relations, -60.0, "TribeRaid -30 with its floor -60");
        assert_eq!(raider.phase, RaidPhase::Retreat);
        step_raider(&mut run, &mut r, &mut raider, home, 0.1, &[]);
        assert_eq!(raider.phase, RaidPhase::Done);
        assert_eq!(run.tribal_campaign.village_food(0), stock + 10.0);
    }
}
