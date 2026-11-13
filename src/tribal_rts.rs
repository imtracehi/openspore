//! Tribal RTS layer: selectable tribe units, mouse and keyboard commands, the
//! gather -> carry -> food mat loop, fishing, hunting, wild creatures, rival
//! villagers and their raids, and the on-screen tribal panel.
//!
//! Spore's Tribal stage is played by selecting tribe members and clicking on
//! things. This module mirrors that:
//!
//! * **Select** - left-click a member (Shift adds/removes), drag a box, or
//!   press `Q` for the whole tribe. Selected members stand on a solid ring.
//! * **Command** - with a selection, left-click (or right-click at any time)
//!   on a fruit bush (gather), meat (collect), the shore (fish), a creature
//!   or rival (attack), a hostile village (attack, or perform in Friendly
//!   stance), a tool shack (pick up its tool), the
//!   food mat / hut (go home) or the ground (move). Right-drag still orbits
//!   the camera.
//! * **Keys** - `Z` gather, `X` fish, `H` hunt, `V` follow the chieftain,
//!   `T` next tool in the palette, `B` build that shack or hand its tool to
//!   the selection. `C` changes stance, `K` tames and `J` gathers eggs.
//!
//! Every unit runs its task on its own: gatherers strip a bush into a
//! bundle (original `carryBundleAmountMax`, 10 food), walk it to the food
//! mat, and go back until the bush is bare, then pick the next bush.
//!
//! Original models are used where the installation provides them (creature
//! species for rivals and wildlife, meat and fruit props, ripples are
//! primitives); everything has a primitive fallback for headless tests.

use bevy::prelude::*;
use bevy::{ui::Pressed, ui_widgets::Button};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::game::*;
use crate::persistence::WorldId;
use crate::stages::{GameStage, StageOwned};
use crate::tribal::{self, RaidPhase, Raider, RivalStatus, TribeRole};
use crate::tribal_tools::{self as tools, ToolKind};

#[path = "tribal_livestock.rs"]
pub mod livestock;

/// ADAPTED: food per second a member takes from a carcass (the retail clip
/// `hunt_meat_hack` is not read; fruit and fish follow their clips, see
/// `tribal_rules::gather_cycle_seconds` and `FishCycle`).
pub const CARCASS_RATE: f32 = 6.0;
/// How close a unit must be to a bush / meat to work it.
pub const WORK_REACH: f32 = 1.7;
/// How close a carrier must be to the food mat to drop off.
pub const DELIVER_REACH: f32 = 2.4;
/// How far idle members look for raiders and predators to fight.
pub const GUARD_RADIUS: f32 = 9.0;
/// Hungry members can still gather/fish when the mat is empty, but work slower.
pub const EMERGENCY_WORK_MULTIPLIER: f32 = 0.25;
/// Members return to the food mat below this reserve and stay until fed/rested.
pub const ENERGY_RETURN_THRESHOLD: f32 = 20.0;
/// Recovery ends with a useful reserve so members do not immediately turn back.
pub const ENERGY_RESUME_THRESHOLD: f32 = 75.0;
/// Food consumed in one meal at the village food mat: one food.
pub const MEAL_FOOD: f32 = 1.0;
/// Hunger restored by one meal (`eatHungerBoostPerFood`); a meal also heals
/// `eatHealthBoostPerFood` (25) health points.
pub const MEAL_ENERGY: f32 = crate::tribal_rules::EAT_HUNGER_BOOST_PER_FOOD;
/// Minimum rest interval between eating from the communal food mat.
pub const RECOVERY_MEAL_SECONDS: f32 = 3.0;
/// Food on a fresh carcass of a grazer (a predator's is worth 20).
pub const MEAT_FOOD: f32 = 15.0;
/// Seconds before an untouched carcass rots away.
pub const MEAT_ROT_SECONDS: f32 = 90.0;
/// Wild creatures the island keeps alive: the herds of the Tribal level
/// generation list (two medium and two large prey herds, one large predator
/// herd; the two epic predators come on top), see `tribal_rules::TRIBAL_HERDS`.
pub const WILDLIFE: usize = crate::tribal_rules::herd_creatures();
/// ADAPTED: members at the walls of a besieged village close in on its
/// defenders that are fighting the tribe within this distance (so the whole
/// war party answers a counter-attack instead of a few members at a time).
pub const SIEGE_HELP_RADIUS: f32 = 5.0;
/// The epic predator's aggro radius and leash: its `herdtypes~` record
/// (TRG_NPC_Epic_Predator) carries `baseSightRadius` 10 and `TerritoryRadius`
/// 50 (VERIFIED by dumping the installed record; the herd init at 0xC6E445
/// copies the territory into cHerd +0x108). ADAPTED: the sight radius is the
/// distance at which it goes for a unit, the territory the distance from its
/// lair at which it turns back (the cHerd readers of +0x108 were not found).
pub const EPIC_AGGRO_RADIUS: f32 = crate::tribal_rules::TRG_EPIC_PREDATOR.sight_radius;
// Lead (Landing 214): the records' TerritoryRadius (epic 50, predators 200) is kept in `tribal_rules`, but which code reads
// cHerd +0x108 was not found, so using it as the chase leash (5x / 25x the old reach) is not applied; the old ADAPTED leashes stay.
pub const EPIC_LEASH: f32 = 10.0;
/// Predators hunt inside their territory: the `TerritoryRadius` of the
/// TRG_NPC_Large_Predator records (200); beyond this distance from home they
/// give up the chase (same ADAPTED reading of the radius as the epic's leash).
pub const PREDATOR_TERRITORY: f32 = 8.0;
/// ADAPTED placement rule (not a record value): a predator's nest is placed at
/// least this far from the bushes the tribe forages from.
pub const PREDATOR_FORAGE_CLEARANCE: f32 = 10.0;
/// Food one raider carries off.
pub const RAID_STEAL: f32 = crate::tribal_rules::RAID_LOAD;
/// Seconds of pixels a click may drag before it becomes a box / an orbit.
const CLICK_SLOP: f32 = 6.0;

// ---------------------------------------------------------------------------
// Components and resources
// ---------------------------------------------------------------------------

/// Marker on selected tribe units.
#[derive(Component, Debug, Clone, Copy)]
pub struct Selected;

/// What a unit is carrying back to the food mat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FoodKind {
    #[default]
    Fruit,
    Fish,
    Meat,
}

impl FoodKind {
    /// The kind of load in the retail carry tables.
    pub fn haul(self) -> crate::tribal_rules::Haul {
        match self {
            FoodKind::Fruit => crate::tribal_rules::Haul::Fruit,
            FoodKind::Fish => crate::tribal_rules::Haul::Fish,
            FoodKind::Meat => crate::tribal_rules::Haul::Meat,
        }
    }

    /// Food in a full bundle of this kind (`carryBundleAmountMax_*`: five
    /// items; a fish is worth 1.5).
    pub fn capacity(self) -> f32 {
        crate::tribal_rules::bundle_food(self.haul())
    }

    /// [`Self::capacity`] at the tribe's gather level (the outfit's
    /// accessories, `outfitter::bundle_food`).
    pub fn capacity_at(self, gather: u8) -> f32 {
        crate::outfitter::bundle_food(self.haul(), gather)
    }
}

/// Every controllable tribal unit: the chieftain and each member.
#[derive(Component, Debug, Clone)]
pub struct TribeUnit {
    pub chieftain: bool,
    pub speed: f32,
    /// Food in the unit's arms, delivered at the food mat.
    pub carrying: f32,
    pub carry_kind: FoodKind,
    /// Progress timer of the current action: the seconds into the current
    /// gather clip cycle / fishing cycle (0xD75F70 / 0xD73FD0).
    pub work: f32,
    /// The rolled parts of the current fishing cycle (not saved).
    pub fish: crate::tribal_rules::FishCycle,
    /// Fish credited so far; seeds the next cycle's rolls (not saved).
    pub catches: u32,
    /// Formation offset handed out with the last group command.
    pub offset: Vec2,
    /// Presentation phase (bob while working / dancing).
    pub anim: f32,
    /// True on frames the unit is actively working or fighting.
    pub busy: bool,
    /// True while a besieger is busy with a defender instead of the huts.
    pub fighting: bool,
    /// Hunger (0..=100), persisted by stable roster slot for tribe members.
    pub energy: f32,
    /// Seconds hunger has been held at the maximum (`hungerDecayDelay`, 120 s,
    /// before it starts to fall); a stopwatch that is not saved.
    pub hunger_hold: f32,
    /// Seconds until the weapon's special attack is ready again (a stopwatch
    /// that is not saved).
    pub special_timer: f32,
    /// The order interrupted by hunger; revalidated when resumed.
    pub resume_task: Option<UnitTask>,
    /// Time spent without food at zero energy.
    pub starvation_time: f32,
    /// Separate clock so a paused fishing/work order resumes its progress.
    pub recovery_work: f32,
    /// Whether the no-food warning has already been shown for this recovery.
    pub warned_no_food: bool,
    /// Self defence (`tribal_rules::RETALIATE_LEASH`): the order a hurt member
    /// dropped to fight back, and where it was hit; resumed when the attacker
    /// falls or gets away. Not saved.
    pub retaliate: Option<(UnitTask, Vec3)>,
}

impl TribeUnit {
    pub fn new(chieftain: bool, speed: f32) -> Self {
        Self {
            chieftain,
            speed,
            carrying: 0.0,
            carry_kind: FoodKind::Fruit,
            work: 0.0,
            fish: crate::tribal_rules::FishCycle::default(),
            catches: 0,
            offset: Vec2::ZERO,
            anim: 0.0,
            busy: false,
            fighting: false,
            energy: tribal::MEMBER_ENERGY_MAX,
            hunger_hold: 0.0,
            special_timer: 0.0,
            resume_task: None,
            starvation_time: 0.0,
            recovery_work: 0.0,
            warned_no_food: false,
            retaliate: None,
        }
    }

    /// Fight back against `attacker` (hit at `at`): a member at work drops its
    /// order and remembers it. Members already fighting, walking on an explicit
    /// move order or performing keep their order. Returns true when it turned.
    pub fn retaliate_against(&mut self, task: &mut UnitTask, attacker: Entity, at: Vec3) -> bool {
        match *task {
            UnitTask::Hunt(_) | UnitTask::Siege(_) | UnitTask::MoveTo(_) | UnitTask::Perform(_) => {
                false
            }
            current => {
                if self.retaliate.is_none() {
                    self.retaliate = Some((current, at));
                }
                *task = UnitTask::Hunt(attacker);
                true
            }
        }
    }

    /// Stop fighting back: the dropped order (or `fallback`).
    pub fn end_retaliation(&mut self, fallback: UnitTask) -> UnitTask {
        self.retaliate.take().map(|(task, _)| task).unwrap_or(fallback)
    }

    pub fn interrupt_for_food(&mut self, task: &mut UnitTask, emergency_food_work: bool) -> bool {
        if self.energy > ENERGY_RETURN_THRESHOLD
            || emergency_food_work
            || matches!(*task, UnitTask::Recover)
        {
            return false;
        }
        self.resume_task = Some(*task);
        self.recovery_work = 0.0;
        self.starvation_time = 0.0;
        self.warned_no_food = false;
        *task = UnitTask::Recover;
        true
    }

    pub fn resume_after_meal(&mut self, task: &mut UnitTask, position: Vec3) -> bool {
        if self.energy < ENERGY_RESUME_THRESHOLD || !matches!(*task, UnitTask::Recover) {
            return false;
        }
        *task = self.resume_task.take().unwrap_or(UnitTask::Idle(position));
        self.warned_no_food = false;
        self.recovery_work = 0.0;
        true
    }
}

/// Charge communal stock only when the entire meal is available.
pub fn eat_from_stockpile(food: &mut f32, energy: &mut f32) -> bool {
    if !food.is_finite()
        || !energy.is_finite()
        || *food < MEAL_FOOD
        || *energy >= tribal::MEMBER_ENERGY_MAX
    {
        return false;
    }
    *food -= MEAL_FOOD;
    *energy = (*energy + MEAL_ENERGY).min(tribal::MEMBER_ENERGY_MAX);
    true
}

/// What to do after a delivery or a tool pick-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum After {
    Idle,
    Gather(Entity),
    Fish(Vec3),
}

/// A unit's current order. Members keep working their order until it runs
/// out (bare bushes are replaced by the next nearest bush).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum UnitTask {
    /// Stand near a spot (and guard it against raiders).
    Idle(Vec3),
    /// Stay close to the chieftain.
    Follow,
    MoveTo(Vec3),
    /// Strip a bush or carcass into a carried bundle.
    Gather(Entity),
    /// Carry the bundle to the food mat, then carry on.
    Deliver(After),
    /// Fish at a shore spot (needs fishing spears).
    Fish(Vec3),
    /// Attack a creature, rival or raider.
    Hunt(Entity),
    /// Attack a rival village's huts.
    Siege(usize),
    /// Steal a bundle from a rival tribe's food mat and carry it home
    /// (`kCitizenActionRaidFood` 8).
    Raid(usize),
    /// Dance and play for a rival village.
    Perform(usize),
    /// Walk to a tool shack and take its tool.
    FetchTool(ToolKind, After),
    /// Return to the village food mat, eat from actual stock, then resume.
    Recover,
    /// Feed a living wild grazer, then lead it to the animal pen.
    Domesticate(Entity),
    /// Carry a gift bundle from the tribe's food mat to a rival village's food
    /// mat (`kCitizenActionGiftFood` 9, 0xCD0DC9): the food leaves the stock
    /// when the bundle is picked up, the gift (`TribeGift`) happens when it is
    /// put down on the other tribe's mat.
    Gift(usize),
}

impl UnitTask {
    pub fn label(&self) -> &'static str {
        match self {
            UnitTask::Idle(_) => "idle",
            UnitTask::Follow => "following",
            UnitTask::MoveTo(_) => "moving",
            UnitTask::Gather(_) => "gathering",
            UnitTask::Deliver(_) => "carrying food",
            UnitTask::Fish(_) => "fishing",
            UnitTask::Hunt(_) => "fighting",
            UnitTask::Siege(_) => "raiding",
            UnitTask::Raid(_) => "stealing food",
            UnitTask::Perform(_) => "performing",
            UnitTask::FetchTool(..) => "fetching a tool",
            UnitTask::Recover => "eating / resting",
            UnitTask::Domesticate(_) => "domesticating",
            UnitTask::Gift(_) => "carrying a gift",
        }
    }
}

/// A carcass left by a hunt; collected like a bush.
#[derive(Component, Debug, Clone)]
pub struct MeatPile {
    pub food_left: f32,
    pub rot: f32,
}

/// A wild creature: grazers flee, predators attack.
#[derive(Component, Debug, Clone)]
pub struct WildAnimal {
    pub hostile: bool,
    pub speed: f32,
    pub home: Vec3,
    pub target: Vec3,
    pub wait: f32,
    pub flee: f32,
    pub chase: Option<Entity>,
}

/// A wild creature of one of the generation list's herds (T4).
#[derive(Component, Debug, Clone, Copy)]
pub struct HerdMember {
    /// Index into `tribal_rules::TRIBAL_HERDS`.
    pub herd: usize,
    /// Food its carcass gives (`foodValue`).
    pub food: f32,
    /// Moving and fighting: within the casting thresholds of the tribe's units
    /// (`CastingManager_DistanceThresholds`); a frozen herd waits.
    pub active: bool,
    /// Guard or lounger (T9E, `tribal_rules::herd_layout`).
    pub role: crate::tribal_rules::HerdRole,
    /// A guard's seconds left at its post.
    pub post_timer: f32,
}

/// The layout key of herd `herd` (repeatable per herd).
pub fn herd_key(herd: usize) -> u64 {
    0x4E52_D000 ^ (herd as u64).wrapping_mul(0x2545_F491)
}

/// The role of creature `member` of herd `herd` (`tribal_rules::herd_layout`;
/// members beyond the herd size lounge in the last group).
pub fn herd_role(herd: usize, member: usize) -> crate::tribal_rules::HerdRole {
    let archetype = crate::tribal_rules::herd_archetype(herd);
    let layout = crate::tribal_rules::herd_layout(&archetype.groups, archetype.creatures, herd_key(herd));
    layout
        .get(member)
        .or(layout.last())
        .copied()
        .unwrap_or(crate::tribal_rules::HerdRole::Lounge { group: 0 })
}

/// ADAPTED: the guards' ring starts at a per-herd angle.
fn guard_phase(herd: usize) -> f32 {
    (herd_key(herd) % 628) as f32 / 100.0
}

/// One of the two epic predators (`TRG_NPC_Epic_Predator`).
#[derive(Component, Debug, Clone, Copy)]
pub struct EpicPredator;

/// What a rival villager is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RivalMode {
    Wander,
    Defend(Entity),
    Raid,
}

/// A member of a rival village (also used for raiders, which additionally
/// carry `tribal::Raider`).
#[derive(Component, Debug, Clone)]
pub struct RivalMember {
    pub village: usize,
    pub home: Vec3,
    pub target: Vec3,
    pub wait: f32,
    pub mode: RivalMode,
    pub stolen: f32,
    pub speed: f32,
}

/// A shore spot where fish jump.
#[derive(Component, Debug, Clone, Copy)]
pub struct FishingSpot;

/// Home props whose original model follows the campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomePropKind {
    Hut,
    Totem,
    EggPen,
    /// The food mat (`trg_totem_foodmat<level>`, matching the totem, 0xE8F530).
    FoodMat,
    FoodPile,
}

#[derive(Component, Debug, Clone)]
pub struct HomeProp {
    pub kind: HomePropKind,
    pub shown: String,
}

/// Fixed root that parents overlay rings (so terrain grounding never
/// re-bases them).
#[derive(Component)]
pub struct OverlayRoot;

/// Identity / selection ring under an actor.
#[derive(Component)]
pub struct UnitRing(pub Entity);

/// Child showing the bundle a unit carries (its platter PROP name).
#[derive(Component, Debug, Clone)]
pub struct CarriedFood(pub FoodKind, pub String);

/// Short-lived marker where an order was given.
#[derive(Component)]
pub struct CommandPing(pub f32);

/// Ripple animation at a fishing spot.
#[derive(Component)]
pub struct Ripple(pub f32);

/// Runtime state of the RTS layer (reset on every stage entry).
#[derive(Resource, Debug, Clone)]
pub struct TribalRts {
    /// Terrain seed (for the coastline).
    pub seed: u64,
    pub left_press: Option<Vec2>,
    pub right_press: Option<Vec2>,
    pub right_travel: f32,
    pub drag: Option<(Vec2, Vec2)>,
    /// Tool highlighted in the palette (`T`, `B`).
    pub palette: ToolKind,
    pub wildlife_timer: f32,
    /// Respawn stopwatch per herd of `tribal_rules::TRIBAL_HERDS`.
    pub herd_timers: [f32; crate::tribal_rules::TRIBAL_HERDS.len()],
    /// Where each herd lives (see [`herd_homes`]).
    pub herd_homes: Vec<Vec3>,
    /// The forest herds (`forestSpeciesRules`, T9F; see [`forest_herds`]).
    pub forest_herds: Vec<ForestHerd>,
    pub villager_timers: [f32; tribal::RIVAL_VILLAGES],
    pub raid_turn: usize,
    pub rng: u64,
}

impl TribalRts {
    /// Where herd `index` nests (a generation-list herd or a forest herd).
    pub fn herd_home(&self, index: usize) -> Option<Vec3> {
        if index >= crate::tribal_rules::FOREST_HERD_BASE {
            self.forest_herds.iter().find(|f| f.herd == index).map(|f| f.home)
        } else {
            self.herd_homes.get(index).copied()
        }
    }

    pub fn new(seed: u64) -> Self {
        let mut rts = Self::bare(seed);
        rts.forest_herds = forest_herds(seed, &rts.herd_homes);
        rts
    }

    fn bare(seed: u64) -> Self {
        Self {
            seed,
            left_press: None,
            right_press: None,
            right_travel: 0.0,
            drag: None,
            palette: ToolKind::SpikedClubs,
            wildlife_timer: 0.0,
            herd_timers: [0.0; crate::tribal_rules::TRIBAL_HERDS.len()],
            herd_homes: herd_homes(seed),
            forest_herds: Vec::new(),
            villager_timers: [0.0; tribal::RIVAL_VILLAGES],
            raid_turn: 0,
            rng: seed ^ 0x7121_BA1,
        }
    }
}

impl Default for TribalRts {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Lazily created ring / ping assets.
#[derive(Resource)]
pub struct RtsAssets {
    ring: Handle<Mesh>,
    selected: Handle<StandardMaterial>,
    member: Handle<StandardMaterial>,
    hostile: Handle<StandardMaterial>,
    allied: Handle<StandardMaterial>,
    wild: Handle<StandardMaterial>,
    ping: Handle<StandardMaterial>,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Pure: formation offset for the `i`-th unit of a group order (a sunflower
/// spiral, so groups spread around a target instead of stacking).
pub fn formation_offset(i: usize) -> Vec2 {
    if i == 0 {
        return Vec2::ZERO;
    }
    let angle = i as f32 * 2.399_963;
    let radius = 0.9 + 0.55 * (i as f32).sqrt();
    Vec2::new(angle.cos(), angle.sin()) * radius
}

/// Pure: screen-space rectangle test for box selection (corners in any order).
pub fn in_rect(point: Vec2, a: Vec2, b: Vec2) -> bool {
    let min = a.min(b);
    let max = a.max(b);
    point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
}

/// Step `pos` toward `target` in the stage chart; returns the new position
/// and the remaining chart distance. On the planet (`tribal_planet`) the
/// step covers `speed * dt` of surface (the chart is compressed toward the
/// limb) and stays on land: a step onto the sea slides along the coast or
/// stops (ADAPTED: herd animals and pets do not swim, their swim clips are
/// not wired; tribe members and raiders use [`step_toward_swim`]).
pub fn step_toward(pos: Vec3, target: Vec3, speed: f32, dt: f32) -> (Vec3, f32) {
    step_toward_as(pos, target, speed, dt, false)
}

/// [`step_toward`] for tribe members and raiders (tribal-swim, T9J): retail
/// `WalkTo` 0xC1C720 sets a straight locomotion goal (no path finding) and a
/// creature that enters water starts swimming (0xC04E30 -> 0xC14F70), so the
/// walk is the straight line over land and water alike. ADAPTED: cliffs are
/// still walked around (`tribal_path`, the swimmers' grid), and the swim speed
/// is the walking speed (retail's locomotion strategy is not traced).
pub fn step_toward_swim(pos: Vec3, target: Vec3, speed: f32, dt: f32) -> (Vec3, f32) {
    step_toward_as(pos, target, speed, dt, true)
}

fn step_toward_as(pos: Vec3, target: Vec3, speed: f32, dt: f32, swim: bool) -> (Vec3, f32) {
    let delta = Vec2::new(target.x - pos.x, target.z - pos.z);
    let d = delta.length();
    if d < 0.05 {
        return (pos, d);
    }
    let layout = crate::tribal_planet::current();
    // Path finding (tribal_path): head for the next visible corner of the
    // route around water and cliffs (swimmers: around cliffs only); straight
    // on when the way is clear.
    let aim = if swim { layout.swim_waypoint(pos, target) } else { layout.waypoint(pos, target) };
    let to_aim = Vec2::new(aim.x - pos.x, aim.z - pos.z);
    let (dir, reach) = if to_aim.length() > 0.05 {
        (to_aim / to_aim.length(), to_aim.length())
    } else {
        (delta / d, d)
    };
    let step = (speed * dt / layout.stretch(pos.xz(), dir)).min(reach);
    let to = Vec3::new(pos.x + dir.x * step, pos.y, pos.z + dir.y * step);
    let next = if swim { layout.swim_step(pos, to) } else { layout.walk(pos, to) };
    (next, Vec2::new(target.x - next.x, target.z - next.z).length())
}

/// Yaw that faces travel direction `dir` (matches `move_player_relative`).
fn facing(dir: Vec2) -> Option<Quat> {
    (dir.length_squared() > 1e-6).then(|| Quat::from_rotation_y((-dir.x).atan2(-dir.y)))
}

/// What a click on the world resolves to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    Gather(Entity),
    Domesticate(Entity),
    Fish(Vec3),
    Hunt(Entity),
    Siege(usize),
    /// Steal food from a rival tribe's food mat.
    Raid(usize),
    Perform(usize),
    /// Carry a gift bundle to the village's food mat.
    Gift(usize),
    Fetch(ToolKind),
    Home,
    Move(Vec3),
}

/// Everything a click can land on, gathered once per click.
#[derive(Default, Clone)]
pub struct ClickWorld {
    pub bushes: Vec<(Entity, Vec3, f32)>,
    pub meat: Vec<(Entity, Vec3)>,
    pub foes: Vec<(Entity, Vec3)>,
    pub grazers: Vec<(Entity, Vec3)>,
    pub shacks: Vec<(ToolKind, Vec3)>,
    pub hostile_villages: Vec<(usize, Vec3)>,
    /// Food mats of the hostile villages (the raid targets).
    pub food_mats: Vec<(usize, Vec3)>,
    pub spots: Vec<Vec3>,
    /// tribal_controls: egg piles (also in `bushes`) and rival members with
    /// their village, so a right click can be named in the original's actions.
    pub eggs: Vec<Entity>,
    pub rivals: Vec<(Entity, usize)>,
    /// tribal_click (`tribal_hud::click`): what the retail input rules read
    /// besides the objects above. Members of allied tribes (not foes) and the
    /// huts and food mats of the allied villages (`village`, hut, food mat)
    /// are hover targets of the retail rules (attitude Friendly).
    pub allied_members: Vec<(Entity, Vec3, usize)>,
    pub allied_villages: Vec<(usize, Vec3, Vec3)>,
    /// The player's own living members (entity, position, health fraction).
    pub members: Vec<(Entity, Vec3, f32)>,
    /// Rival members that cannot take a gift now (ADAPTED stand-in of the
    /// retail member-state test 0xDA8180: fighting members refuse).
    pub busy: Vec<Entity>,
    /// The player's species diet; `None` = no diet gate (synthetic worlds).
    pub diet: Option<crate::creature_hud::Diet>,
}

/// Pure: friendly commands tame grazers and perform for villages; aggressive
/// commands hunt and besiege. Actual performances still require instruments.
pub fn resolve_click(p: Vec3, world: &ClickWorld, musical: bool) -> Order {
    let near = |q: Vec3, r: f32| dist_xz(p, q) <= r;
    if let Some((e, _)) = world
        .foes
        .iter()
        .filter(|(_, q)| near(*q, 1.8))
        .min_by(|a, b| dist_xz(p, a.1).total_cmp(&dist_xz(p, b.1)))
    {
        if musical && world.grazers.iter().any(|(grazer, _)| grazer == e) {
            return Order::Domesticate(*e);
        }
        return Order::Hunt(*e);
    }
    if let Some((e, _)) = world.meat.iter().find(|(_, q)| near(*q, 1.6)) {
        return Order::Gather(*e);
    }
    if let Some((e, _, _)) = world
        .bushes
        .iter()
        .filter(|(_, q, _)| near(*q, 1.8))
        .min_by(|a, b| dist_xz(p, a.1).total_cmp(&dist_xz(p, b.1)))
    {
        return Order::Gather(*e);
    }
    // A hostile tribe's food mat: raid it (perform in the social stance).
    if let Some((v, _)) = world.food_mats.iter().find(|(_, q)| near(*q, 2.0)) {
        return if musical {
            Order::Perform(*v)
        } else {
            Order::Raid(*v)
        };
    }
    if let Some((v, _)) = world.hostile_villages.iter().find(|(_, q)| near(*q, 5.0)) {
        return if musical {
            Order::Perform(*v)
        } else {
            Order::Siege(*v)
        };
    }
    if let Some((k, _)) = world.shacks.iter().find(|(_, q)| near(*q, 2.0)) {
        return Order::Fetch(*k);
    }
    if near(tribal::food_pile_position(), 2.2) || near(tribal::home_position(), 2.6) {
        return Order::Home;
    }
    if let Some(spot) = world
        .spots
        .iter()
        .filter(|q| near(**q, 4.0))
        .min_by(|a, b| dist_xz(p, **a).total_cmp(&dist_xz(p, **b)))
    {
        return Order::Fish(*spot);
    }
    Order::Move(p)
}

// ---------------------------------------------------------------------------
// Scene construction (called from `tribal::setup_tribal`)
// ---------------------------------------------------------------------------

/// Creature-stage species ids are small; tribal populations use their own
/// ranges so rival tribes and wildlife look like distinct original species.
fn rival_species(village: usize) -> u32 {
    300 + village as u32 * 11
}
fn wild_species(hostile: bool, i: usize) -> u32 {
    if hostile && i >= 1000 {
        // The epic predators.
        560 + (i % 2) as u32 * 7
    } else if hostile {
        520 + (i % 2) as u32 * 5
    } else {
        470 + (i % 2) as u32 * 3
    }
}

/// Spawn a creature body: the original species visual when installed,
/// otherwise a tinted capsule. Grounding keeps it on the terrain.
#[allow(clippy::too_many_arguments)]
fn spawn_creature(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    species: u32,
    pos: Vec3,
    scale: f32,
    tint: Color,
    name: String,
) -> Entity {
    let key = crate::baked_model::species_key(originals, species);
    let native = key.is_some_and(|key| {
        crate::baked_model::ensure(originals, key, meshes, images, materials).is_ok()
    });
    let entity = commands
        .spawn((
            StageOwned,
            Name::new(name),
            Visibility::Inherited,
            Transform::from_xyz(pos.x, -0.6 + 0.9 * scale, pos.z).with_scale(Vec3::splat(scale)),
        ))
        .id();
    if native
        && crate::baked_model::spawn_species_visual(commands, originals, meshes, entity, species)
    {
        return entity;
    }
    commands.entity(entity).insert((
        Mesh3d(meshes.add(Capsule3d::new(0.45, 0.9))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: tint,
            perceptual_roughness: 0.8,
            ..default()
        })),
    ));
    entity
}

/// The body of a villager of `village` at `at` (the village's species, no AI:
/// the vignette fillers, `tribal_vignettes::spawn_filler`).
pub fn spawn_villager_body(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    village: usize,
    at: Vec3,
) -> Entity {
    spawn_creature(
        commands,
        originals,
        meshes,
        images,
        materials,
        rival_species(village),
        at,
        0.55,
        tribal::village_tint(RivalStatus::Hostile, village),
        format!("V{} villager", village + 1),
    )
}

/// Spawn one rival villager of `village` around `home`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_villager(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    village: usize,
    index: usize,
    status: RivalStatus,
    raider: bool,
) -> Entity {
    let home = tribal::village_position(village);
    let angle = index as f32 * 1.7 + village as f32;
    let pos = home + Vec3::new(angle.cos() * 4.5, 0.0, angle.sin() * 4.5);
    let e = spawn_creature(
        commands,
        originals,
        meshes,
        images,
        materials,
        rival_species(village),
        pos,
        0.55,
        tribal::village_tint(status, village),
        format!("V{} villager", village + 1),
    );
    commands.entity(e).insert((
        RivalMember {
            village,
            home,
            target: pos,
            wait: 1.0 + index as f32,
            mode: if raider {
                RivalMode::Raid
            } else {
                RivalMode::Wander
            },
            stolen: 0.0,
            speed: 4.2,
        },
        // A citizen of its tribe: 100 + its species' health parts (tribal_rules).
        Health::new(crate::tribal_rules::npc_citizen_max_hp()),
    ));
    if raider {
        commands.entity(e).insert(Raider { phase: RaidPhase::Approach, ..Raider::default() });
    }
    e
}

/// Spawn creature `member` of herd `herd` (an index into
/// `tribal_rules::TRIBAL_HERDS`) at `pos`: the archetype's hit points, size and
/// carcass food. Predators stalk the tribe; epics only near their lair.
#[allow(clippy::too_many_arguments)]
pub fn spawn_herd_creature(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    herd: usize,
    member: usize,
    pos: Vec3,
) -> Entity {
    let archetype = crate::tribal_rules::herd_archetype(herd);
    let hostile = archetype.predator;
    // Species: grazers and predators alternate two installed species each, the
    // epics have their own.
    let index = if archetype.epic { 1000 + herd } else { herd * 7 + member };
    let species = wild_species(hostile, index);
    let e = spawn_creature(
        commands,
        originals,
        meshes,
        images,
        materials,
        species,
        pos,
        crate::tribal_rules::herd_body_scale(archetype),
        if archetype.epic {
            Color::srgb(0.35, 0.08, 0.06)
        } else if hostile {
            Color::srgb(0.55, 0.15, 0.12)
        } else {
            Color::srgb(0.70, 0.62, 0.40)
        },
        format!("{} {}", archetype.name, member + 1),
    );
    commands.entity(e).insert((
        WildAnimal {
            hostile,
            // ADAPTED speeds (OpenSpore's grazer / predator pace; epics lumber).
            speed: if archetype.epic {
                3.4
            } else if hostile {
                4.6
            } else {
                3.6
            },
            home: pos,
            target: pos,
            wait: member as f32 * 0.7,
            flee: 0.0,
            chase: None,
        },
        Health::new(archetype.health),
        livestock::WildlifeSpecies(species),
        HerdMember {
            herd,
            food: archetype.food,
            active: false,
            role: herd_role(herd, member),
            post_timer: archetype.groups.guard_duration.0 + member as f32 * 3.0,
        },
    ));
    if archetype.epic {
        commands.entity(e).insert(EpicPredator);
    }
    e
}

/// ADAPTED spacing of the herd homes (retail units; the generator's own
/// exclusive areas for herds are not traced): prey graze at least
/// `PREY_CLEARANCE` from the hut and the villages, predators and epics keep
/// `PREDATOR_CLEARANCE` (twice the zoning radius) from them, herds stand
/// `HERD_SPACING` apart.
const PREY_CLEARANCE: f32 = 40.0;
const PREDATOR_CLEARANCE: f32 = 60.0;
const HERD_SPACING: f32 = 40.0;

/// Pure: where each herd of `tribal_rules::TRIBAL_HERDS` lives: its
/// `OriginDistance` ring from the stage origin (a chord on the planet), on
/// land, clear of the villages, the home hut and the other herds; the epics
/// also keep away from the fruit bushes.
pub fn herd_homes(seed: u64) -> Vec<Vec3> {
    let layout = crate::tribal_planet::current();
    let land = &layout.land;
    let bushes = bush_positions(seed, 14);
    // The bushes the tribe forages from (`gatherFindTreeRadius` around the mat).
    let mat = tribal::food_pile_position();
    let forage: Vec<Vec3> = bushes
        .iter()
        .copied()
        .filter(|b| dist_xz(*b, mat) <= crate::tribal_rules::forage_radius())
        .collect();
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x4E4D_5EED);
    let mut homes: Vec<Vec3> = Vec::new();
    for herd in crate::tribal_rules::TRIBAL_HERDS.iter() {
        // Prey graze in the middle of their `OriginDistance` ring; predators and
        // epics keep to its far end (ADAPTED: that keeps their territory out of
        // the home camp, where herd casting leaves them asleep until the tribe
        // comes near).
        let (want, spread) = if herd.predator {
            (herd.origin_distance.1 * crate::tribal_rules::LAYOUT_SCALE, 0.8..1.0)
        } else {
            (crate::tribal_rules::herd_distance(herd), 0.85..1.15)
        };
        let mut best: Option<(f32, Vec3)> = None;
        for _ in 0..96 {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let radius = want * rng.random_range(spread.clone());
            let at = land.toward(layout.origin, radius, angle);
            if !land.is_land(at) {
                continue;
            }
            let p = Vec3::new(at.x, 0.0, at.y);
            let villages = (0..tribal::RIVAL_VILLAGES)
                .map(|v| dist_xz(p, tribal::village_position(v)))
                .fold(f32::MAX, f32::min);
            let herds = homes.iter().map(|h| dist_xz(p, *h)).fold(f32::MAX, f32::min);
            let fruit = bushes.iter().map(|b| dist_xz(p, *b)).fold(f32::MAX, f32::min);
            let home = dist_xz(p, tribal::home_position());
            let foraged = forage.iter().map(|b| dist_xz(p, *b)).fold(f32::MAX, f32::min);
            // Prey may graze near a village; predator territories stay clear of
            // the villages' defence rings and of the bushes the tribe forages
            // from, the epics farther still.
            let clear = if herd.epic {
                villages > PREDATOR_CLEARANCE
                    && fruit > 6.0
                    && home > PREDATOR_CLEARANCE + 20.0
                    && foraged > 9.0
            } else if herd.predator {
                villages > PREDATOR_CLEARANCE
                    && home > PREDATOR_CLEARANCE
                    && foraged > PREDATOR_FORAGE_CLEARANCE
            } else {
                villages > PREY_CLEARANCE && home > PREY_CLEARANCE
            } && herds > HERD_SPACING;
            if clear {
                best = Some((f32::MAX, p));
                break;
            }
            // Keep the roomiest spot in case nothing is fully clear.
            let score = villages.min(herds).min(home)
                + if herd.predator { 2.0 * foraged.min(12.0) } else { 0.0 };
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, p));
            }
        }
        homes.push(best.map(|(_, p)| p).unwrap_or_else(|| {
            let at = layout.land.toward(layout.origin, want, homes.len() as f32 * 0.9);
            Vec3::new(at.x, 0.0, at.y)
        }));
    }
    homes
}

/// A forest herd's nest (T9F) and its casting state: its creatures exist
/// only while a unit of the tribe is near (`CastingManager_DistanceThresholds`
/// 100 / 150, `MaxAnimalPoolSize` 100: retail casts herds from their nests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForestHerd {
    /// Herd index (`tribal_rules::FOREST_HERD_BASE` + 2 x instance + rule).
    pub herd: usize,
    pub home: Vec3,
    pub forest: Vec2,
    pub spawned: bool,
    /// Respawn stopwatch while cast.
    pub timer: f32,
}

/// The forests near the hut (T9F): the flora groves (INFERRED: the forest
/// list of 0xBA08C0 holds the grove messages 0x030FC4EC; same density and
/// positions as `planet_flora::plan`), on land above the beach, within
/// `maxForestDistanceForNounGeneration` (chord from the hut) and on the
/// hut's continent (reachable on the path grid). Chart points, nearest first.
pub fn forest_centres() -> Vec<Vec2> {
    use crate::planet_flora as flora;
    let layout = crate::tribal_planet::current();
    let (Some(sp), Some(nav)) = (layout.planet.as_deref(), layout.nav.as_deref()) else {
        return Vec::new();
    };
    let n = flora::CELLS_PER_EDGE;
    let half_box = flora::GROVE_SOURCE * 2.0 / n as f32;
    let mut out: Vec<(f32, Vec2)> = Vec::new();
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let (uc, vc) = ((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
                for gi in 0..flora::GROVES_PER_CELL {
                    let (hx, hy) = flora::halton((flora::GROVE_START + gi as u32) % flora::GROVE_CLUSTERS + 1);
                    let u = uc + (hx * 2.0 - 1.0) * half_box * 0.5;
                    let v = vc + (hy * 2.0 - 1.0) * half_box * 0.5;
                    let dir = crate::planet_query::face_point(face, u, v).normalize();
                    if !flora::flora_map(&sp.planet, dir) {
                        continue;
                    }
                    let world = sp.frame.to_world(dir * sp.planet.height(dir));
                    if world.y < sp.frame.centre.y {
                        continue;
                    }
                    let chart = Vec2::new(world.x, world.z);
                    let d = layout.land.chord(Vec2::ZERO, chart);
                    if d <= crate::tribal_rules::MAX_FOREST_DISTANCE_FOR_NOUN_GENERATION && nav.path(Vec2::ZERO, chart).is_some() {
                        out.push((d, chart));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// Pure (given the layout): the forest herds (VERIFIED rule 0xB9C980 /
/// 0xB9AB60, ADAPTED placement search): at every forest each
/// `FOREST_SPECIES_RULES` species rolls its chance; a nest goes `OriginDistance`
/// from the forest on land, keeping its exclusive radius from every other herd
/// and the camp clearances of [`herd_homes`] from the hut and the villages.
pub fn forest_herds(seed: u64, others: &[Vec3]) -> Vec<ForestHerd> {
    let layout = crate::tribal_planet::current();
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xF0E5_7EED);
    let mut homes: Vec<Vec3> = others.to_vec();
    let mut out = Vec::new();
    for forest in forest_centres() {
        for (k, rule) in crate::tribal_rules::FOREST_SPECIES_RULES.iter().enumerate() {
            if rng.random_range(0.0..1.0f32) >= rule.chance {
                continue;
            }
            let (lo, hi) = rule.archetype.origin_distance;
            let start = rng.random_range(0.0..std::f32::consts::TAU);
            let clearance = if rule.archetype.predator { PREDATOR_CLEARANCE } else { PREY_CLEARANCE };
            let found = (0..12).find_map(|i| {
                let angle = start + i as f32 * std::f32::consts::TAU / 12.0;
                let d = lo + (hi - lo) * ((i * 7 % 12) as f32 / 11.0);
                let p = layout.land.toward(forest, d, angle);
                let p3 = Vec3::new(p.x, 0.0, p.y);
                let clear = layout.land.is_land(p)
                    && dist_xz(p3, tribal::home_position()) > clearance
                    && layout.villages.iter().all(|v| dist_xz(p3, *v) > clearance)
                    && homes.iter().all(|h| dist_xz(p3, *h) > rule.exclusive_radius);
                clear.then_some(p3)
            });
            if let Some(home) = found {
                homes.push(home);
                out.push(ForestHerd {
                    herd: crate::tribal_rules::FOREST_HERD_BASE
                        + 2 * out.len()
                        + k,
                    home: tribal::keep_on_land(home),
                    forest,
                    spawned: false,
                    timer: 0.0,
                });
            }
        }
    }
    out
}

/// Pure: where creature `member` of herd `herd` stands around the herd's
/// home (its nest): a guard at its post, a lounger beside its group's spot
/// (T9E).
pub fn herd_spot(_seed: u64, home: Vec3, herd: usize, member: usize) -> Vec3 {
    let archetype = crate::tribal_rules::herd_archetype(herd);
    let spot = match herd_role(herd, member) {
        crate::tribal_rules::HerdRole::Guard { post } => {
            crate::tribal_rules::guard_post(&archetype.groups, home, post, guard_phase(herd), 0.5).0
        }
        crate::tribal_rules::HerdRole::Lounge { group } => {
            // The group's spot (0xD8D790: a random point on the ring around the
            // nest, drawn once per group), then this member's own place beside it.
            let layout = crate::tribal_rules::herd_layout(&archetype.groups, archetype.creatures, herd_key(herd));
            let members = crate::tribal_rules::lounge_group_size(&layout, group);
            let key = herd_key(herd) ^ u64::from(group).wrapping_mul(0x9E37_79B9);
            let group_spot = crate::tribal_rules::lounge_spot(
                home,
                archetype,
                members,
                unit_roll(key, 11),
                unit_roll(key, 12),
            );
            let angle = member as f32 * 2.399_963; // golden angle
            let radius = 0.8 + (member % 3) as f32 * 0.6;
            group_spot + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
        }
    };
    tribal::keep_on_land(spot)
}

/// Spawn the world life around the villages: overlay root, fishing spots,
/// wildlife, rival villagers and the tribal panel.
#[allow(clippy::too_many_arguments)]
pub fn spawn_village_life(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    run: &RunProgress,
    icons: &tools::ToolIcons,
) {
    let seed = run.species_world.seed;
    commands.insert_resource(TribalRts::new(seed));
    commands.spawn((
        StageOwned,
        OverlayRoot,
        Name::new("tribal overlay"),
        Transform::IDENTITY,
        Visibility::Inherited,
    ));
    // Fishing spots: ripples on the water's edge.
    let ripple = meshes.add(Torus::new(0.8, 0.95));
    let water = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.92, 1.0, 0.55),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    for (i, spot) in tribal::fishing_spots().into_iter().enumerate() {
        let out = Vec2::new(spot.x, spot.z).normalize_or(Vec2::X) * 1.6;
        commands.spawn((
            StageOwned,
            FishingSpot,
            Ripple(i as f32 * 0.37),
            Name::new(format!("fishing spot {}", i + 1)),
            Mesh3d(ripple.clone()),
            MeshMaterial3d(water.clone()),
            bevy::light::NotShadowCaster,
            Transform::from_xyz(spot.x + out.x, -0.6 + 0.04, spot.z + out.y)
                .with_scale(Vec3::new(1.0, 0.05, 1.0)),
        ));
    }
    // Wildlife: the herds of the Tribal generation list around their homes
    // (prey, a predator pack and two epic predators).
    let homes = herd_homes(seed);
    for (herd, archetype) in crate::tribal_rules::TRIBAL_HERDS.iter().enumerate() {
        for member in 0..archetype.creatures {
            let p = herd_spot(seed, homes[herd], herd, member);
            spawn_herd_creature(
                commands, originals, meshes, images, materials, herd, member, p,
            );
        }
    }
    livestock::restore(
        commands,
        originals,
        meshes,
        images,
        materials,
        &run.tribal_campaign,
    );
    // Rival villagers, who also defend their huts. Tribes that have not come
    // into view get theirs when they do (`tribal::tribal_tribes_appear`).
    for v in 0..tribal::RIVAL_VILLAGES {
        match run.tribal_campaign.village(v).map(|s| s.status) {
            None | Some(RivalStatus::Conquered) => continue,
            Some(_) => spawn_village_members(
                commands,
                originals,
                meshes,
                images,
                materials,
                &run.tribal_campaign,
                v,
            ),
        }
    }
    spawn_panel(commands, icons);
}

/// Spawn the members of village `v`: its tribe's `initialpopulation` (3 / 6 / 9
/// by tier).
#[allow(clippy::too_many_arguments)]
pub fn spawn_village_members(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    campaign: &tribal::TribalCampaign,
    v: usize,
) {
    let status = campaign.village(v).map(|s| s.status).unwrap_or_default();
    for i in 0..campaign.tribe_of(v).initial_population {
        spawn_villager(
            commands, originals, meshes, images, materials, v, i, status, false,
        );
    }
}

/// Spawn a fruit bush (original `Fruit_Normal_01` when installed).
#[allow(clippy::too_many_arguments)]
pub fn spawn_bush(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    id: u32,
    pos: Vec3,
) {
    if let Some(e) = crate::world_objects::spawn_at(
        commands,
        originals,
        crate::dbpf::ResourceKey {
            group_id: 0,
            instance_id: 0x154F_BFB5,
            type_id: crate::asset_catalog::PROP,
        },
        "Fruit_Normal_01",
        meshes,
        images,
        materials,
        Vec3::new(pos.x, -0.6, pos.z),
        2.2,
    ) {
        commands.entity(e).insert((
            WorldId(id),
            Bush {
                food_left: tribal::BUSH_FOOD,
            },
        ));
        return;
    }
    commands.spawn((
        StageOwned,
        WorldId(id),
        Bush {
            food_left: tribal::BUSH_FOOD,
        },
        Name::new("fruit bush"),
        Mesh3d(meshes.add(Sphere::new(1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.15, 0.55, 0.15),
            ..default()
        })),
        Transform::from_xyz(pos.x, 0., pos.z),
    ));
}

/// ADAPTED: wild bushes grow this far from the hut (inside
/// `gatherFindTreeRadius`, 70, so the gatherers find them; terrainDistribute
/// flora and fruit groves are not placed from the planet script yet).
const WILD_BUSH_RADIUS: (f32, f32) = (12.0, 60.0);

/// Pure: deterministic bush layout - groves near home plus wild bushes
/// spread around it on land, kept clear of the villages and the shack ring.
pub fn bush_positions(seed: u64, count: usize) -> Vec<Vec3> {
    let layout = crate::tribal_planet::current();
    let mut rng = ChaCha8Rng::seed_from_u64(0x721B_A1 ^ seed.rotate_left(13));
    let mut out: Vec<Vec3> = Vec::new();
    let groves = [Vec3::new(-11.0, 0.0, 9.0), Vec3::new(12.0, 0.0, 10.0)];
    let mut guard = 0;
    while out.len() < count && guard < 2000 {
        guard += 1;
        let p = if out.len() < 6 {
            let g = groves[out.len() % 2];
            g + Vec3::new(
                rng.random_range(-4.0..4.0),
                0.0,
                rng.random_range(-4.0..4.0),
            )
        } else {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let radius = rng.random_range(WILD_BUSH_RADIUS.0..WILD_BUSH_RADIUS.1);
            Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
        };
        let clear = layout.land.is_land(p.xz())
            && dist_xz(p, tribal::home_position()) > 10.5
            && (0..tribal::RIVAL_VILLAGES).all(|v| dist_xz(p, tribal::village_position(v)) > 7.5)
            && out.iter().all(|q| dist_xz(p, *q) > 2.6);
        if clear {
            out.push(p);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Input: selection and commands
// ---------------------------------------------------------------------------

/// Give `order` to `units` (formation offsets per unit). Returns how many
/// accepted it.
fn give_order(
    order: Order,
    units: &mut Query<
        (
            Entity,
            &Transform,
            &mut TribeUnit,
            &mut UnitTask,
            Option<&Selected>,
            Option<&WorldId>,
        ),
        Without<RivalMember>,
    >,
    chosen: &[Entity],
    campaign: &mut tribal::TribalCampaign,
) -> usize {
    let mut accepted = 0;
    let mut refused: Option<String> = None;
    for (i, entity) in chosen.iter().enumerate() {
        let Ok((_, transform, mut unit, mut task, _, id)) = units.get_mut(*entity) else {
            continue;
        };
        let tool = id.and_then(|id| campaign.tool_of(id.0));
        unit.offset = formation_offset(i);
        unit.work = 0.0;
        unit.resume_task = None;
        let next = match order {
            Order::Gather(e) => UnitTask::Gather(e),
            Order::Domesticate(e) => UnitTask::Domesticate(e),
            Order::Hunt(e) => UnitTask::Hunt(e),
            Order::Siege(v) => UnitTask::Siege(v),
            Order::Raid(v) => {
                if unit.carrying > 0.0 {
                    refused = Some("Drop off the food you carry first.".into());
                    continue;
                }
                UnitTask::Raid(v)
            }
            Order::Perform(v) => UnitTask::Perform(v),
            Order::Gift(v) => {
                if unit.carrying > 0.0 {
                    refused = Some("Drop off the food you carry first.".into());
                    continue;
                }
                UnitTask::Gift(v)
            }
            Order::Move(p) => UnitTask::MoveTo(p),
            Order::Home => {
                if unit.carrying > 0.0 {
                    UnitTask::Deliver(After::Idle)
                } else {
                    UnitTask::Idle(tribal::home_position() + Vec3::new(0.0, 0.0, 3.0))
                }
            }
            Order::Fish(spot) => {
                if tools::can_fish(tool) {
                    UnitTask::Fish(spot)
                } else if unit.chieftain {
                    refused = Some("The chieftain leads; members do the fishing.".into());
                    continue;
                } else if campaign.has_shack(ToolKind::FishingSpears) {
                    UnitTask::FetchTool(ToolKind::FishingSpears, After::Fish(spot))
                } else {
                    refused = Some("Build the Fishing Spears shack first (T/B).".into());
                    continue;
                }
            }
            Order::Fetch(kind) => {
                if unit.chieftain {
                    refused = Some("The chieftain keeps his staff.".into());
                    continue;
                }
                if kind == ToolKind::Campfire {
                    UnitTask::Idle(tools::shack_position(
                        campaign.shacks.iter().position(|k| *k == kind).unwrap_or(0),
                    ))
                } else {
                    UnitTask::FetchTool(kind, After::Idle)
                }
            }
        };
        let _ = transform;
        *task = next;
        accepted += 1;
    }
    if accepted == 0 {
        if let Some(reason) = refused {
            campaign.say(reason);
        }
    }
    accepted
}

/// Mouse picking, selection and the keyboard command keys.
#[allow(clippy::too_many_arguments)]
pub fn tribal_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut rts: ResMut<TribalRts>,
    mut run: ResMut<RunProgress>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    hover: Option<Res<bevy::picking::hover::HoverMap>>,
    ui_buttons: Query<(), With<Button>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
    mut units: Query<
        (
            Entity,
            &Transform,
            &mut TribeUnit,
            &mut UnitTask,
            Option<&Selected>,
            Option<&WorldId>,
        ),
        Without<RivalMember>,
    >,
    objects: Query<
        (
            Entity,
            &Transform,
            Option<&Bush>,
            Option<&MeatPile>,
            Option<&WildAnimal>,
            Option<&RivalMember>,
            Option<&tools::ToolShack>,
            Option<&FishingSpot>,
            Option<&livestock::EggPile>,
        ),
        Without<TribeUnit>,
    >,
    mut controls: ResMut<crate::tribal_controls::TribalControls>,
    // tribal_click: the HUD's diet and the members' health (the retail input
    // rules read them); grouped to stay within the system parameter limit.
    (spui, stage_layout, hud, healths): (
        Option<Res<crate::spui::view::SpuiPointer>>,
        Option<Res<crate::tribal_planet::StageLayout>>,
        Option<Res<crate::tribal_hud::TribalHudState>>,
        Query<&Health, With<TribeUnit>>,
    ),
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let run = &mut *run;
    // tribal_controls: Ctrl is the original's multi-select modifier (Ctrl+click
    // adds or removes a member); Shift stays as an extra.
    let shift = keys.pressed(KeyCode::ShiftLeft)
        || keys.pressed(KeyCode::ShiftRight)
        || keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight);
    let selected: Vec<Entity> = units
        .iter()
        .filter(|(_, _, _, _, s, _)| s.is_some())
        .map(|(e, ..)| e)
        .collect();
    // Commands act on the selection, or on every member when none is chosen.
    let members: Vec<Entity> = units
        .iter()
        .filter(|(_, _, u, ..)| !u.chieftain)
        .map(|(e, ..)| e)
        .collect();
    let acting: Vec<Entity> = if selected.is_empty() {
        members.clone()
    } else {
        selected.clone()
    };

    // --- keyboard ---------------------------------------------------------
    // ADAPTED: C (stance), T and B (tools) and Z X H K J V (member orders) are
    // extras: the original's trigger files bind none of them (it commands with
    // the right click and the HUD).
    if keys.just_pressed(KeyCode::KeyC) {
        run.tribal_campaign.friendly_stance = !run.tribal_campaign.friendly_stance;
        run.tribal_campaign
            .say(if run.tribal_campaign.friendly_stance {
                "Friendly stance: right-click a wild grazer to tame, or a village to perform."
            } else {
                "Aggressive stance: right-click creatures to hunt, or villages to attack."
            });
        run.request_save();
    }
    // tribal_controls: select the tribe (the header bar of the tribal list, or
    // Ctrl+A); Q is a camera strafe key in the original.
    if keys.just_pressed(crate::tribal_controls::KEY_SELECT_TRIBE) {
        for (e, ..) in units.iter() {
            commands.entity(e).insert(Selected);
        }
        run.tribal_campaign.say("The whole tribe is selected.");
    }
    if keys.just_pressed(KeyCode::KeyT) {
        rts.palette = tools::next_tool(rts.palette);
    }
    if keys.just_pressed(KeyCode::KeyB) {
        build_or_equip(rts.palette, &acting, &mut units, run);
    }
    let allied: Vec<usize> = (0..tribal::RIVAL_VILLAGES)
        .filter(|v| {
            run.tribal_campaign
                .village(*v)
                .is_some_and(|s| s.status == RivalStatus::Allied)
        })
        .collect();
    let world = || {
        let mut w = ClickWorld::default();
        for (e, t, bush, meat, animal, rival, shack, spot, egg) in objects.iter() {
            let p = t.translation;
            if let Some(b) = bush {
                w.bushes.push((e, p, b.food_left));
            }
            if meat.is_some() {
                w.meat.push((e, p));
            }
            if animal.is_some() || rival.is_some_and(|r| !allied.contains(&r.village)) {
                w.foes.push((e, p));
            }
            if let Some(r) = rival {
                w.rivals.push((e, r.village));
                if allied.contains(&r.village) {
                    w.allied_members.push((e, p, r.village));
                } else if r.mode != RivalMode::Wander {
                    w.busy.push(e);
                }
            }
            if egg.is_some() {
                w.eggs.push(e);
            }
            if animal.is_some_and(|animal| !animal.hostile) {
                w.grazers.push((e, p));
            }
            if let Some(s) = shack {
                w.shacks.push((s.0, p));
            }
            if spot.is_some() {
                w.spots.push(p);
            }
        }
        w
    };
    if keys.just_pressed(KeyCode::KeyZ) {
        // Each gatherer heads for the nearest bush with food.
        let w = world();
        let mut n = 0;
        for (i, e) in acting.iter().enumerate() {
            let Ok((_, t, mut unit, mut task, _, _)) = units.get_mut(*e) else {
                continue;
            };
            let best =
                w.bushes.iter().filter(|(_, _, f)| *f > 1.0).min_by(|a, b| {
                    dist_xz(t.translation, a.1).total_cmp(&dist_xz(t.translation, b.1))
                });
            if let Some((bush, _, _)) = best {
                unit.offset = formation_offset(i);
                unit.resume_task = None;
                *task = UnitTask::Gather(*bush);
                n += 1;
            }
        }
        run.tribal_campaign.say(if n > 0 {
            format!("{n} off to gather fruit.")
        } else {
            "No bush has fruit right now.".into()
        });
    }
    if keys.just_pressed(KeyCode::KeyK) {
        let centre = acting
            .iter()
            .filter_map(|e| units.get(*e).ok().map(|u| u.1.translation))
            .fold(Vec3::ZERO, |sum, pos| sum + pos)
            / acting.len().max(1) as f32;
        let animal = objects
            .iter()
            .filter(|o| o.4.is_some_and(|a| !a.hostile))
            .min_by(|a, b| {
                dist_xz(centre, a.1.translation)
                    .total_cmp(&dist_xz(centre, b.1.translation))
                    .then(a.0.to_bits().cmp(&b.0.to_bits()))
            })
            .map(|o| o.0);
        if let Some(animal) = animal {
            let n = give_order(
                Order::Domesticate(animal),
                &mut units,
                &acting,
                &mut run.tribal_campaign,
            );
            if n > 0 {
                run.tribal_campaign.say("Feed a wild grazer: 5 to 15 food a feeding by its herd (K). Eggs are gathered at the pen with J.");
            }
        } else {
            run.tribal_campaign
                .say("No living wild grazer is available to domesticate.");
        }
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        if let Some(pool) = objects.iter().find(|o| o.8.is_some()).map(|o| o.0) {
            give_order(
                Order::Gather(pool),
                &mut units,
                &acting,
                &mut run.tribal_campaign,
            );
            run.tribal_campaign
                .say("Gather the eggs and carry them to the food mat.");
        } else {
            run.tribal_campaign
                .say("Domesticate a wild grazer first (K); its eggs collect at the pen.");
        }
    }
    if keys.just_pressed(KeyCode::KeyX) {
        let w = world();
        let mut chosen = Vec::new();
        for e in &acting {
            if let Ok((_, t, ..)) = units.get(*e) {
                chosen.push((*e, t.translation));
            }
        }
        for (e, pos) in chosen {
            let spot = w
                .spots
                .iter()
                .min_by(|a, b| dist_xz(pos, **a).total_cmp(&dist_xz(pos, **b)))
                .copied();
            if let Some(spot) = spot {
                give_order(
                    Order::Fish(spot),
                    &mut units,
                    &[e],
                    &mut run.tribal_campaign,
                );
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyH) {
        let w = world();
        let centre = acting
            .iter()
            .filter_map(|e| units.get(*e).ok().map(|u| u.1.translation))
            .fold(Vec3::ZERO, |a, b| a + b)
            / acting.len().max(1) as f32;
        let prey = objects
            .iter()
            .filter(|o| o.4.is_some())
            .min_by(|a, b| {
                dist_xz(centre, a.1.translation).total_cmp(&dist_xz(centre, b.1.translation))
            })
            .map(|o| o.0);
        let _ = w;
        match prey {
            Some(prey) => {
                give_order(
                    Order::Hunt(prey),
                    &mut units,
                    &acting,
                    &mut run.tribal_campaign,
                );
                run.tribal_campaign.say("The hunters set out.");
            }
            None => run.tribal_campaign.say("No creature to hunt nearby."),
        }
    }
    if keys.just_pressed(KeyCode::KeyV) {
        let followers: Vec<Entity> = acting
            .iter()
            .copied()
            .filter(|e| units.get(*e).is_ok_and(|u| !u.2.chieftain))
            .collect();
        for (i, e) in followers.iter().enumerate() {
            if let Ok((_, _, mut unit, mut task, ..)) = units.get_mut(*e) {
                unit.offset = formation_offset(i + 1);
                unit.resume_task = None;
                *task = UnitTask::Follow;
            }
        }
        let threat_active = objects
            .iter()
            .any(|(_, _, _, _, _, rival, _, _, _)| rival.is_some());
        if threat_active {
            run.tribal_campaign
                .say("Rally Horn! The tribe rallies home to defend against threats!");
        } else {
            run.tribal_campaign.say("The tribe follows the chieftain.");
        }
    }

    // --- mouse -------------------------------------------------------------
    let Some(window) = windows.iter().next() else {
        return;
    };
    let cursor = window.cursor_position();
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    // A press on a HUD or palette button is the button's, not the world's
    // (tribal_controls: the retail SPUI HUD reports it through `SpuiPointer`).
    let over_ui = spui.as_deref().is_some_and(|pointer| pointer.over_ui)
        || hover.as_deref().is_some_and(|map| {
        map.get(&bevy::picking::pointer::PointerId::Mouse)
            .is_some_and(|hits| {
                hits.keys().any(|e| {
                    let mut at = *e;
                    for _ in 0..4 {
                        if ui_buttons.contains(at) {
                            return true;
                        }
                        match parents.get(at) {
                            Ok(p) => at = p.parent(),
                            Err(_) => break,
                        }
                    }
                    false
                })
            })
    });
    if mouse.just_pressed(MouseButton::Left) && !over_ui {
        rts.left_press = cursor;
    }
    if mouse.just_pressed(MouseButton::Right) && !over_ui {
        rts.right_press = cursor;
        rts.right_travel = 0.0;
    }
    if let (Some(start), Some(now)) = (rts.left_press, cursor) {
        if mouse.pressed(MouseButton::Left) && start.distance(now) > CLICK_SLOP {
            rts.drag = Some((start, now));
        }
    }
    if let (Some(start), Some(now)) = (rts.right_press, cursor) {
        rts.right_travel = rts.right_travel.max(start.distance(now));
    }
    let ground = |screen: Vec2| -> Option<Vec3> {
        let ray = camera.viewport_to_world(camera_transform, screen).ok()?;
        ground_hit(ray, surface.as_deref())
    };
    let screen_of = |p: Vec3| {
        camera
            .world_to_viewport(camera_transform, p + Vec3::Y * 0.6)
            .ok()
    };

    // Left release: finish a box, select a unit, or command the selection.
    if mouse.just_released(MouseButton::Left) {
        let start = rts.left_press.take();
        if let Some((a, b)) = rts.drag.take() {
            if !shift {
                for e in &selected {
                    commands.entity(*e).remove::<Selected>();
                }
            }
            let mut n = 0;
            for (e, t, ..) in units.iter() {
                if screen_of(t.translation).is_some_and(|s| in_rect(s, a, b)) {
                    commands.entity(e).insert(Selected);
                    n += 1;
                }
            }
            if n > 0 {
                run.tribal_campaign.say(format!("{n} selected."));
            }
        } else if let (Some(_), Some(at)) = (start, cursor) {
            let picked = units
                .iter()
                .filter_map(|(e, t, ..)| {
                    screen_of(t.translation).map(|s| (e, s.distance(at), t.translation))
                })
                .filter(|(_, d, _)| *d < 32.0)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((e, _, _)) = picked {
                let was = selected.contains(&e);
                if shift {
                    if was {
                        commands.entity(e).remove::<Selected>();
                    } else {
                        commands.entity(e).insert(Selected);
                    }
                } else {
                    for other in &selected {
                        if *other != e {
                            commands.entity(*other).remove::<Selected>();
                        }
                    }
                    commands.entity(e).insert(Selected);
                }
            } else if let Some(p) = ground(at) {
                // tribal_controls: the left button only selects (the manual:
                // "the left mouse button is used for selecting units");
                // commands are the right click's.
                // Nothing selected: a click on the hut or egg pen recruits.
                if dist_xz(p, tribal::egg_pen_position()) < 2.0 {
                    run.tribal_campaign
                        .say("Press R near home to pay for a growing egg.");
                }
            }
        }
    }
    // Right click (not a pan or an orbit drag): command the selection.
    if mouse.just_released(MouseButton::Right) {
        let start = rts.right_press.take();
        // The camera's drag scroll takes over past `DRAG_SLOP` (tribal_controls).
        if start.is_some() && rts.right_travel <= crate::tribal_controls::DRAG_SLOP {
            if let Some(p) = cursor.and_then(|c| ground(c)) {
                // tribal_click: the diet and the living members, which the
                // retail input rules (`tribal_hud::click`) read.
                let mut w = world();
                w.diet = hud.as_deref().map(|h| h.diet);
                w.members = units
                    .iter()
                    .filter_map(|u| {
                        let frac = healths.get(u.0).map_or(1.0, |h| h.frac());
                        (frac > 0.0).then_some((u.0, u.1.translation, frac))
                    })
                    .collect();
                command_at(
                    p,
                    &selected,
                    &w,
                    &mut units,
                    run,
                    &mut commands,
                    &mut controls,
                );
            }
        }
    }
}

/// Resolve a right click into an order for `chosen`: the retail right click
/// (tribal_click, `tribal_hud::click`). The object under the pointer gives the
/// `TribeInputAction` id the cursor shows (`ChooseInputActionID`, the diet gate
/// included) and the id gives what the click does (`DoActionGeneric`): nothing,
/// a hint, or one `CitizenAction` for the selected members.
pub(crate) fn command_at(
    p: Vec3,
    chosen: &[Entity],
    world: &ClickWorld,
    units: &mut Query<
        (
            Entity,
            &Transform,
            &mut TribeUnit,
            &mut UnitTask,
            Option<&Selected>,
            Option<&WorldId>,
        ),
        Without<RivalMember>,
    >,
    run: &mut RunProgress,
    commands: &mut Commands,
    controls: &mut crate::tribal_controls::TribalControls,
) {
    use crate::tribal_controls::CitizenAction as A;
    use crate::tribal_hud::{click, cursor};
    // The input acts on the selection.
    if chosen.is_empty() {
        return;
    }
    let mut w = world.clone();
    cursor::prepare(&mut w, run);
    let diet = w.diet.unwrap_or(crate::creature_hud::Diet::Omnivore);
    let chieftain = chosen
        .iter()
        .any(|e| units.get(*e).is_ok_and(|u| u.2.chieftain));
    let healers: Vec<Entity> = chosen
        .iter()
        .copied()
        .filter(|e| {
            units.get(*e).is_ok_and(|u| {
                u.5.and_then(|id| run.tribal_campaign.tool_of(id.0)) == Some(ToolKind::HealingRods)
            })
        })
        .collect();
    let hover = cursor::hover_at(p, &w, run, diet, &cursor::Selection { chieftain, healers: healers.clone() });
    let state = click::ClickState {
        food: run.dna_this_stage,
        gift_cost: tribal::GIFT_FOOD,
        feed_cost: livestock::TAME_FOOD,
    };
    let command = match click::resolve(&hover, &state) {
        click::Outcome::Nothing => return,
        click::Outcome::Hint(hint) => {
            run.tribal_campaign.say(hint.message());
            return;
        }
        click::Outcome::Command(command) => command,
    };
    // The members carry the food over (GiftFood, 0xCD0DC9 / 0xDBB7D0): one of
    // them (ADAPTED: one bundle of `giftFoodAmount` per command; the retail
    // code gives every selected member the action) takes a bundle from the
    // food mat and puts it down on the other tribe's mat, where the gift
    // happens (`UnitTask::Gift`); the others walk over with it.
    let gift_village = match (command.action, hover.village) {
        (A::GiftFood | A::GiftMember, Some(village)) => {
            let status = run.tribal_campaign.village(village).map(|v| v.status);
            if status != Some(RivalStatus::Hostile) {
                run.tribal_campaign.say("They need no gifts.");
                return;
            }
            Some(village)
        }
        (A::GiftFood | A::GiftMember, None) => return,
        _ => None,
    };
    // The heal action is the healers' alone (0xCD1250).
    let mut recipients: Vec<Entity> = if command.action == A::Heal {
        healers
    } else {
        chosen.to_vec()
    };
    let mut n = 0;
    if let Some(village) = gift_village {
        let carrier = recipients
            .iter()
            .copied()
            .filter(|e| units.get(*e).is_ok_and(|u| u.2.carrying <= 0.0))
            .min_by_key(|e| units.get(*e).is_ok_and(|u| u.2.chieftain));
        if let Some(carrier) = carrier {
            n += give_order(Order::Gift(village), units, &[carrier], &mut run.tribal_campaign);
            recipients.retain(|e| *e != carrier);
        }
    }
    let order = command.order;
    n += give_order(order, units, &recipients, &mut run.tribal_campaign);
    if n > 0 {
        controls.issue(command, n, p);
        let line = match (command.action, order) {
            (A::GiftFood | A::GiftMember | A::Heal, _) => "",
            (_, Order::Gather(_)) => "Gather!",
            (_, Order::Domesticate(_)) => "Feed and domesticate the creature!",
            (_, Order::Fish(_)) => "Go fishing!",
            (_, Order::Hunt(_)) => "Attack!",
            (_, Order::Siege(_)) => "Raid their village!",
            (_, Order::Raid(_)) => "Steal their food!",
            (_, Order::Perform(_)) => "Perform for them (E starts the song)!",
            (_, Order::Gift(_)) => "",
            (_, Order::Fetch(_)) => "Fetch the tool!",
            (_, Order::Home) => "Back home!",
            (_, Order::Move(_)) => "",
        };
        if !line.is_empty() {
            run.tribal_campaign.say(line);
        }
        commands.spawn((
            StageOwned,
            CommandPing(0.0),
            Transform::from_translation(p + Vec3::Y * 0.1),
            Visibility::Inherited,
        ));
    }
}

/// `B` / palette click: build the shack, or hand its tool to the members.
fn build_or_equip(
    kind: ToolKind,
    acting: &[Entity],
    units: &mut Query<
        (
            Entity,
            &Transform,
            &mut TribeUnit,
            &mut UnitTask,
            Option<&Selected>,
            Option<&WorldId>,
        ),
        Without<RivalMember>,
    >,
    run: &mut RunProgress,
) {
    if !run.tribal_campaign.has_shack(kind) {
        let food = run.dna_this_stage;
        match run.tribal_campaign.build_shack(kind, food) {
            Ok(cost) => {
                run.dna_this_stage = (run.dna_this_stage - cost).max(0.0);
                run.request_save();
            }
            Err(reason) => run.tribal_campaign.say(reason),
        }
        return;
    }
    if kind == ToolKind::Campfire {
        run.tribal_campaign
            .say("The campfire warms everyone resting at home.");
        return;
    }
    let members: Vec<Entity> = acting
        .iter()
        .copied()
        .filter(|e| units.get(*e).is_ok_and(|u| !u.2.chieftain))
        .collect();
    let n = give_order(
        Order::Fetch(kind),
        units,
        &members,
        &mut run.tribal_campaign,
    );
    if n > 0 {
        run.tribal_campaign.say(format!(
            "{n} fetch {} from the shack.",
            tools::info(kind).name
        ));
    }
}

/// Intersect a camera ray with the terrain (or the base plane). On the
/// planet: the height field or the sea surface, whichever is nearer, at any
/// camera distance.
pub(crate) fn ground_hit(ray: Ray3d, surface: Option<&crate::terrain::LandSurface>) -> Option<Vec3> {
    if let Some(planet) = surface.and_then(|s| s.planet()) {
        return crate::tribal_planet::ray_hit(planet, ray.origin, *ray.direction);
    }
    let height = |p: Vec3| surface.map_or(-0.6, |s| s.height(p.x, p.z));
    let dir = *ray.direction;
    if dir.y >= -1e-4 {
        return None;
    }
    let mut prev = 0.0;
    let mut t = 0.5;
    while t < 400.0 {
        let p = ray.origin + dir * t;
        if p.y <= height(p) {
            // Refine between prev and t.
            let (mut lo, mut hi) = (prev, t);
            for _ in 0..12 {
                let mid = (lo + hi) * 0.5;
                let q = ray.origin + dir * mid;
                if q.y <= height(q) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some(ray.origin + dir * hi);
        }
        prev = t;
        t += 0.5;
    }
    None
}

// ---------------------------------------------------------------------------
// Unit behaviour
// ---------------------------------------------------------------------------

/// Run every unit's order: walk, gather, carry, fish, fetch tools.
#[allow(clippy::too_many_arguments)]
pub fn tribal_units_think(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    _rts: Res<TribalRts>,
    _keys: Res<ButtonInput<KeyCode>>,
    mut units: Query<
        (
            Entity,
            &mut Transform,
            &mut TribeUnit,
            &mut UnitTask,
            Option<&WorldId>,
            &mut Health,
        ),
        Without<RivalMember>,
    >,
    mut bushes: Query<(Entity, &Transform, &mut Bush), Without<TribeUnit>>,
    egg_sources: Query<(), With<livestock::EggPile>>,
    mut meat: Query<(Entity, &Transform, &mut MeatPile), (Without<TribeUnit>, Without<Bush>)>,
    targets: Query<
        (Entity, &Transform),
        (
            Or<(With<WildAnimal>, With<RivalMember>)>,
            Without<TribeUnit>,
            Without<Bush>,
            Without<MeatPile>,
        ),
    >,
    shacks: Query<
        (&tools::ToolShack, &Transform),
        (
            Without<TribeUnit>,
            Without<Bush>,
            Without<MeatPile>,
            Without<WildAnimal>,
            Without<RivalMember>,
        ),
    >,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let dt = time.delta_secs();
    let run = &mut *run;
    let difficulty = crate::creature_rules::Difficulty::from_index(run.cell.difficulty);
    let roles: Vec<u32> = run.tribe.iter().map(|m| m.role).collect();
    let role_mult = gather_multiplier(&roles);
    // The outfit's gather level enlarges every bundle (outfitter).
    let gather = crate::outfitter::tribe_levels(run).gather;
    // tribal_controls: W A S D and the arrows pan the camera (the Tribal stage
    // has no avatar walking); the chieftain is commanded like a member.
    let manual = false;
    let chief_pos = units
        .iter()
        .find(|u| u.2.chieftain)
        .map(|u| u.1.translation)
        .unwrap_or(Vec3::ZERO);
    let pile = tribal::food_pile_position();
    let mut positions: Vec<(Entity, Vec3)> = Vec::new();
    // Whether each member sent to a gathering song stands at its spot (the
    // performance state 1 waits for every participant, 0xDA3283).
    let mut performers: Vec<bool> = Vec::new();
    for (entity, mut transform, mut unit, mut task, id, mut health) in &mut units {
        unit.busy = false;
        if unit.chieftain && manual {
            // WASD drives the chieftain directly; drop his order.
            if !matches!(*task, UnitTask::Idle(_)) {
                *task = UnitTask::Idle(transform.translation);
            }
            positions.push((entity, transform.translation));
            continue;
        }
        let tool = id.and_then(|id| run.tribal_campaign.tool_of(id.0));
        let pos = transform.translation;
        let offset3 = Vec3::new(unit.offset.x, 0.0, unit.offset.y);
        if let Some(id) = id.filter(|_| !unit.chieftain) {
            // If a meal is unavailable, let an interrupted food order resume so
            // it can produce the first bundle/catch. Work remains finite and
            // slow at low energy; the normal Recover path resumes as soon as
            // the mat has a full meal. Non-food orders still return to the mat.
            if run.dna_this_stage < MEAL_FOOD && matches!(*task, UnitTask::Recover) {
                if let Some(resume @ (UnitTask::Gather(_) | UnitTask::Fish(_))) = unit.resume_task {
                    *task = resume;
                    unit.resume_task = None;
                    run.tribal_campaign.say(format!(
                        "Food mat is empty; member {} is gathering slowly for a meal.",
                        id.0 - tribal::MATE_ID_BASE + 1
                    ));
                }
            }
            let emergency_food_work = run.dna_this_stage < MEAL_FOOD
                && matches!(
                    *task,
                    UnitTask::Gather(_)
                        | UnitTask::Fish(_)
                        | UnitTask::Deliver(After::Gather(_) | After::Fish(_))
                );
            if unit.interrupt_for_food(&mut *task, emergency_food_work) {
                run.tribal_campaign.say(format!(
                    "Member {} is hungry and returning to the food mat.",
                    id.0 - tribal::MATE_ID_BASE + 1
                ));
            }
        }
        // Where to walk this frame (None = stay).
        let mut goal: Option<Vec3> = None;
        match *task {
            UnitTask::Idle(anchor) => {
                if unit.chieftain {
                    // The player can be restored or repositioned independently
                    // of this task. Idle means hold the current pose, not walk
                    // back to the stale spawn anchor; explicit orders below
                    // still move the chief normally.
                    *task = UnitTask::Idle(pos);
                } else if dist_xz(pos, anchor + offset3) > 1.2 {
                    goal = Some(anchor + offset3);
                }
            }
            UnitTask::Follow => {
                let spot = chief_pos + offset3 * 1.3 + Vec3::new(0.0, 0.0, 1.5);
                if dist_xz(pos, spot) > 1.6 {
                    goal = Some(spot);
                }
            }
            UnitTask::MoveTo(p) => {
                let spot = p + offset3;
                if dist_xz(pos, spot) < 0.35 {
                    *task = UnitTask::Idle(p);
                } else {
                    goal = Some(spot);
                }
            }
            UnitTask::Gather(source) => {
                // A bush or a carcass; a bare source hands over to the next.
                let found = bushes
                    .get(source)
                    .map(|(_, t, b)| (t.translation, b.food_left, FoodKind::Fruit))
                    .ok()
                    .or_else(|| {
                        meat.get(source)
                            .ok()
                            .map(|(_, t, m)| (t.translation, m.food_left, FoodKind::Meat))
                    });
                match found {
                    Some((at, left, kind))
                        if left > 0.5 && unit.carrying < kind.capacity_at(gather) - 1e-3 =>
                    {
                        let side = Vec2::new(pos.x - at.x, pos.z - at.z)
                            .normalize_or(unit.offset.normalize_or(Vec2::X));
                        let stand = at + Vec3::new(side.x, 0.0, side.y) * 1.25;
                        if dist_xz(pos, at) > WORK_REACH {
                            goal = Some(stand);
                        } else {
                            unit.busy = true;
                            let cap = kind.capacity_at(gather);
                            face(&mut transform, at - pos);
                            if kind == FoodKind::Fruit {
                                // One clip cycle (0xD75F70): the smack with the
                                // gather tool (`gathr_treesmacker`) or the pick
                                // by hand (`gathr_picktree`); the fruit is
                                // credited when the clip ends. The roles' and
                                // hunger's speed factors scale the cycle clock
                                // (ADAPTED, as before).
                                let sticks = tool == Some(ToolKind::FruitSticks);
                                let cycle = crate::tribal_rules::gather_cycle_seconds(sticks, unit.chieftain);
                                unit.work += dt * emergency_work_multiplier(unit.energy) * role_mult;
                                if unit.work >= cycle {
                                    unit.work -= cycle;
                                    let room = cap - unit.carrying;
                                    let take = crate::tribal_rules::gather_cycle_food(sticks, room).min(left);
                                    unit.carrying += take;
                                    unit.carry_kind = kind;
                                    if let Ok((_, _, mut b)) = bushes.get_mut(source) {
                                        b.food_left -= take;
                                    }
                                }
                            } else {
                                let rate = CARCASS_RATE * emergency_work_multiplier(unit.energy) * role_mult;
                                // One bundle at a time: five pieces of meat.
                                let take = (rate * dt).min(left).min(cap - unit.carrying);
                                unit.carrying += take;
                                unit.carry_kind = kind;
                                if let Ok((_, _, mut m)) = meat.get_mut(source) {
                                    m.food_left -= take;
                                }
                            }
                        }
                    }
                    Some(_) if unit.carrying > 0.0 => {
                        *task = UnitTask::Deliver(After::Gather(source));
                    }
                    Some((at, _, _)) if egg_sources.contains(source) => {
                        // An egg assignment is recurring work at the pen;
                        // waiting for production must not redirect to a forest.
                        if dist_xz(pos, at) > WORK_REACH {
                            goal = Some(at);
                        }
                    }
                    _ => {
                        // Bare or gone: the nearest bush with fruit inside the
                        // tribe's foraging range (`gatherFindTreeRadius`) is next.
                        let next = bushes
                            .iter()
                            .filter(|(e, t, b)| {
                                *e != source
                                    && b.food_left > crate::tribal_rules::BARE_BUSH
                                    && dist_xz(t.translation, pile)
                                        <= crate::tribal_rules::forage_radius()
                            })
                            .min_by(|a, b| {
                                dist_xz(pos, a.1.translation)
                                    .total_cmp(&dist_xz(pos, b.1.translation))
                            })
                            .map(|(e, _, _)| e);
                        *task = if unit.carrying > 0.0 {
                            UnitTask::Deliver(next.map_or(After::Idle, After::Gather))
                        } else {
                            next.map_or(UnitTask::Idle(pos), UnitTask::Gather)
                        };
                    }
                }
            }
            UnitTask::Deliver(after) => {
                if dist_xz(pos, pile) > DELIVER_REACH {
                    goal = Some(pile + offset3 * 0.6);
                } else {
                    if unit.carrying > 0.0 {
                        run.award_dna(unit.carrying, &GameStage::Tribal);
                    }
                    unit.carrying = 0.0;
                    *task = match after {
                        After::Idle => UnitTask::Idle(pos),
                        After::Gather(e) => UnitTask::Gather(e),
                        After::Fish(spot) => UnitTask::Fish(spot),
                    };
                }
            }
            UnitTask::Fish(spot) => {
                let stand = tribal::keep_on_land(spot + offset3 * 0.5);
                if !tools::can_fish(tool) {
                    *task = UnitTask::Idle(pos);
                } else if dist_xz(pos, stand) > 0.6 {
                    goal = Some(stand);
                } else {
                    unit.busy = true;
                    if unit.work <= 0.0 {
                        // A new catch cycle: the random loop length and the
                        // `fish_succ_N` pick (0xD73FD0 states 8, 9 and 0xB).
                        let seed = entity.to_bits() ^ (u64::from(unit.catches) << 32);
                        unit.fish = crate::tribal_rules::FishCycle::roll(unit_roll(seed, 1), unit_roll(seed, 2));
                    }
                    let before = unit.work;
                    unit.work += dt * emergency_work_multiplier(unit.energy);
                    let out = Vec3::new(spot.x, 0.0, spot.z).normalize_or(Vec3::X);
                    face(&mut transform, out);
                    let credit_at = unit.fish.credit_at();
                    if before < credit_at && unit.work >= credit_at {
                        // The dump puts one fish worth 1.5 food on the bundle.
                        unit.carry_kind = FoodKind::Fish;
                        unit.carrying = (unit.carrying + crate::tribal_rules::FISHING_FOOD_PER_FISH)
                            .min(FoodKind::Fish.capacity_at(gather));
                        unit.catches = unit.catches.wrapping_add(1);
                    }
                    if unit.work >= unit.fish.end() {
                        unit.work = 0.0;
                        if unit.carrying + 0.01 >= FoodKind::Fish.capacity_at(gather) {
                            *task = UnitTask::Deliver(After::Fish(spot));
                        }
                    }
                }
            }
            UnitTask::Domesticate(target) => match targets.get(target) {
                Ok((_, pose)) => {
                    if dist_xz(pos, pose.translation) > WORK_REACH {
                        goal = Some(pose.translation);
                    } else {
                        unit.busy = true;
                        face(&mut transform, pose.translation - pos);
                    }
                }
                Err(_) => *task = UnitTask::Follow,
            },
            UnitTask::Hunt(target) => match targets.get(target) {
                Ok((_, t))
                    if unit.retaliate.is_some_and(|(_, hit_at)| {
                        dist_xz(t.translation, hit_at) > crate::tribal_rules::RETALIATE_LEASH
                    }) =>
                {
                    // The attacker got away: back to work.
                    *task = unit.end_retaliation(UnitTask::Idle(pos));
                }
                Ok((_, t)) => {
                    let range = tools::attack_range(tool) * 0.85;
                    if dist_xz(pos, t.translation) > range {
                        goal = Some(t.translation);
                    } else {
                        unit.busy = true;
                        face(&mut transform, t.translation - pos);
                    }
                }
                Err(_) => *task = unit.end_retaliation(UnitTask::Idle(pos)),
            },
            UnitTask::Siege(v) => {
                let resolved = run
                    .tribal_campaign
                    .village(v)
                    .is_none_or(|s| s.status.is_resolved());
                if resolved {
                    *task = UnitTask::Idle(pos);
                } else {
                    let centre = tribal::village_position(v);
                    let dir = Vec2::new(pos.x - centre.x, pos.z - centre.z)
                        .normalize_or(Vec2::new(-centre.x, -centre.z).normalize_or(Vec2::X));
                    let stand = centre + Vec3::new(dir.x, 0.0, dir.y) * 3.2 + offset3 * 0.4;
                    if dist_xz(pos, centre) > 4.6 {
                        goal = Some(stand);
                    } else {
                        unit.busy = true;
                        face(&mut transform, centre - pos);
                    }
                }
            }
            UnitTask::Raid(v) => {
                let hostile = run
                    .tribal_campaign
                    .village(v)
                    .is_some_and(|s| s.status == RivalStatus::Hostile);
                let mat = tribal::village_food_mat(v);
                if !hostile {
                    *task = UnitTask::Idle(pos);
                } else if dist_xz(pos, mat) > WORK_REACH + 0.6 {
                    goal = Some(mat + offset3 * 0.3);
                } else {
                    // Fill the arms from their stock, then carry it home.
                    let take = run.tribal_campaign.raid_food(v);
                    if take > 0.0 {
                        unit.carrying = take;
                        unit.carry_kind = FoodKind::Fruit;
                        run.tribal_campaign
                            .say(format!("Your raiders steal {take:.0} food from V{}!", v + 1));
                        *task = UnitTask::Deliver(After::Idle);
                    } else {
                        run.tribal_campaign
                            .say(format!("V{} has no food left to steal.", v + 1));
                        *task = UnitTask::Idle(pos);
                    }
                }
            }
            UnitTask::Gift(v) => {
                let open = run
                    .tribal_campaign
                    .village(v)
                    .is_some_and(|s| s.status == RivalStatus::Hostile);
                if !open {
                    // The tribe needs no gift any more: the bundle goes home.
                    *task = if unit.carrying > 0.0 {
                        UnitTask::Deliver(After::Idle)
                    } else {
                        UnitTask::Idle(pos)
                    };
                } else if unit.carrying <= 0.0 {
                    // Pick the bundle up at the food mat; the stock pays for it.
                    if dist_xz(pos, pile) > DELIVER_REACH {
                        goal = Some(pile + offset3 * 0.6);
                    } else if run.dna_this_stage >= tribal::GIFT_FOOD {
                        run.dna_this_stage -= tribal::GIFT_FOOD;
                        unit.carrying = tribal::GIFT_FOOD;
                        unit.carry_kind = FoodKind::Fruit;
                        run.request_save();
                    } else {
                        run.tribal_campaign.say(format!(
                            "Not enough food for a gift: {:.0} needed.",
                            tribal::GIFT_FOOD
                        ));
                        *task = UnitTask::Idle(pos);
                    }
                } else {
                    // Walk the bundle to the other tribe's mat and put it down.
                    let mat = tribal::village_food_mat(v);
                    if dist_xz(pos, mat) > WORK_REACH + 0.6 {
                        goal = Some(mat + offset3 * 0.3);
                    } else {
                        unit.carrying = 0.0;
                        let before = run.tribal_campaign.village(v).map(|s| s.relations).unwrap_or_default();
                        run.tribal_campaign.deliver_gift(v);
                        let after = run.tribal_campaign.village(v).map(|s| s.relations).unwrap_or_default();
                        run.tribal_campaign.say(if after > before {
                            format!("V{} accepts the gift and calms down.", v + 1)
                        } else {
                            format!("V{} takes the food.", v + 1)
                        });
                        run.request_save();
                        *task = UnitTask::Idle(pos);
                    }
                }
            }
            UnitTask::Perform(v) => {
                let centre = tribal::village_position(v);
                let song = run.tribal_campaign.performance.filter(|p| p.village == v);
                let ending = run.tribal_campaign.social_end.filter(|e| e.village == v);
                // The spot the song's layout gave this member (0xDA22C0,
                // `tribal_rules::social_stands`); a member sent to perform without a
                // song stands 5 units in front of the village (ADAPTED).
                let laid_out = run.tribal_campaign.song_stand(entity.to_bits());
                if laid_out.is_some() && song.is_none() && ending.is_none() {
                    // The song and its ending are over (the stop 0xD9FE60 ran): the
                    // member's action ends and it idles where it stands.
                    run.tribal_campaign.forget_stand(entity.to_bits());
                    *task = UnitTask::Idle(pos);
                } else {
                    let stand = match laid_out {
                        Some(spot) => tribal::keep_on_land(spot),
                        None => {
                            let toward_home = Vec2::new(-centre.x, -centre.z).normalize_or(Vec2::X);
                            let side = Vec2::new(-toward_home.y, toward_home.x);
                            centre
                                + Vec3::new(toward_home.x, 0.0, toward_home.y) * 5.0
                                + Vec3::new(side.x, 0.0, side.y) * unit.offset.x
                        }
                    };
                    let arrived = dist_xz(pos, stand) <= crate::tribal_rules::SOCIAL_ARRIVE_DISTANCE;
                    if !arrived {
                        goal = Some(stand);
                    } else {
                        // At its spot the member plays for the song (member stages
                        // 3..8) and stays through the ending clips.
                        unit.busy = song.is_some() || ending.is_some();
                        face(&mut transform, centre - pos);
                    }
                    if song.is_some_and(|p| p.gathering) {
                        performers.push(arrived);
                    }
                }
            }
            UnitTask::FetchTool(kind, after) => {
                let at = shacks
                    .iter()
                    .find(|(s, _)| s.0 == kind)
                    .map(|(_, t)| t.translation);
                match (at, id) {
                    (Some(at), Some(id)) => {
                        if dist_xz(pos, at) > 2.6 {
                            goal = Some(at + offset3 * 0.4);
                        } else {
                            match run.tribal_campaign.equip(id.0, kind) {
                                Ok(()) => {
                                    run.request_save();
                                    *task = match after {
                                        After::Idle => UnitTask::Idle(pos),
                                        After::Gather(e) => UnitTask::Gather(e),
                                        After::Fish(spot) => UnitTask::Fish(spot),
                                    };
                                }
                                Err(reason) => {
                                    run.tribal_campaign.say(reason);
                                    *task = UnitTask::Idle(pos);
                                }
                            }
                        }
                    }
                    _ => *task = UnitTask::Idle(pos),
                }
            }
            UnitTask::Recover => {
                if dist_xz(pos, pile) > DELIVER_REACH {
                    goal = Some(pile + offset3 * 0.6);
                } else {
                    if unit.carrying > 0.0 {
                        run.award_dna(unit.carrying, &GameStage::Tribal);
                        unit.carrying = 0.0;
                    }
                    unit.recovery_work += dt;
                    if unit.energy < tribal::MEMBER_ENERGY_MAX
                        && unit.recovery_work >= RECOVERY_MEAL_SECONDS
                    {
                        unit.recovery_work = 0.0;
                        if eat_from_stockpile(&mut run.dna_this_stage, &mut unit.energy) {
                            unit.starvation_time = 0.0;
                            unit.warned_no_food = false;
                            // One food also heals `eatHealthBoostPerFood` (25).
                            health.heal(crate::tribal_rules::EAT_HEALTH_BOOST_PER_FOOD);
                            if let Some(id) = id {
                                if run
                                    .tribal_campaign
                                    .set_energy(id.0 - tribal::MATE_ID_BASE, unit.energy)
                                {
                                    run.request_save();
                                }
                            }
                            run.request_save();
                        } else if !unit.warned_no_food {
                            run.tribal_campaign.say(format!(
                                "Member {} has no food to eat; gather supplies at the mat.",
                                id.map(|id| id.0 - tribal::MATE_ID_BASE + 1).unwrap_or(0)
                            ));
                            unit.warned_no_food = true;
                        }
                    }
                    if unit.energy <= 0.01 && run.dna_this_stage < MEAL_FOOD {
                        // Starving hurts at `creatureStarvationPain` (0.5 a second).
                        health.hp = (health.hp
                            - crate::tribal_rules::starvation_damage(unit.energy, dt))
                        .max(0.0);
                    }
                    if unit.resume_after_meal(&mut *task, pos) {
                        if let Some(id) = id {
                            run.tribal_campaign.say(format!(
                                "Member {} is fed and resumes {}.",
                                id.0 - tribal::MATE_ID_BASE + 1,
                                task.label()
                            ));
                        }
                    }
                }
            }
        }
        // Hunger is held at the maximum for `hungerDecayDelay` (120 s) and then
        // falls by `TribeHungerDrain` (0.333 a second), whatever the member does.
        if !unit.chieftain {
            let (energy, hold) =
                crate::tribal_rules::hunger_step(unit.energy, unit.hunger_hold, dt, difficulty);
            unit.energy = energy;
            unit.hunger_hold = hold;
        }
        if let Some(id) = id.filter(|_| !unit.chieftain) {
            if run
                .tribal_campaign
                .set_energy(id.0 - tribal::MATE_ID_BASE, unit.energy)
            {
                run.request_save();
            }
        }
        if let Some(goal) = goal {
            let goal = tribal::keep_on_land(goal);
            let (next, _) = step_toward_swim(pos, goal, unit.speed, dt);
            face(&mut transform, next - pos);
            transform.translation.x = next.x;
            transform.translation.z = next.z;
        }
        unit.anim += dt * if unit.busy { 9.0 } else { 0.0 };
        positions.push((entity, transform.translation));
    }
    // Performance state 1 -> 5 (0xDA3283, 0xDA3306, 0xDA3461): when every member
    // sent to the song stands at its spot the music starts and the game begins.
    // (The chieftain walks by the player's hand and does not hold the song.)
    if run.tribal_campaign.performance.is_some_and(|p| p.gathering && p.gather_time > 0.0)
        && performers.iter().all(|arrived| *arrived)
    {
        run.tribal_campaign.troupe_ready();
    }
    // Gentle separation so a group never collapses into one spot.
    for (entity, mut transform, unit, ..) in &mut units {
        if unit.chieftain && manual {
            continue;
        }
        let me = transform.translation;
        let mut push = Vec2::ZERO;
        for (other, p) in &positions {
            if *other == entity {
                continue;
            }
            let d = Vec2::new(me.x - p.x, me.z - p.z);
            let len = d.length();
            if len < 0.9 && len > 1e-4 {
                push += d / len * (0.9 - len);
            }
        }
        if push != Vec2::ZERO {
            // Members swim (tribal-swim): the push is not stopped at the shore.
            let p = crate::tribal_planet::current()
                .swim_step(me, me + Vec3::new(push.x, 0.0, push.y) * (4.0 * dt).min(0.5));
            transform.translation.x = p.x;
            transform.translation.z = p.z;
        }
    }
}

/// A repeatable roll in 0..1 for a unit and a purpose (splitmix64).
fn unit_roll(seed: u64, salt: u64) -> f32 {
    let mut x = seed.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn emergency_work_multiplier(energy: f32) -> f32 {
    if energy <= ENERGY_RETURN_THRESHOLD {
        EMERGENCY_WORK_MULTIPLIER
    } else {
        1.0
    }
}

fn face(transform: &mut Transform, dir: Vec3) {
    if let Some(q) = facing(Vec2::new(dir.x, dir.z)) {
        transform.rotation = q;
    }
}

// ---------------------------------------------------------------------------
// Wildlife
// ---------------------------------------------------------------------------

/// Grazers wander and flee, predators stalk the tribe, epics guard their lair;
/// herds far from every unit of the tribe freeze (herd casting), traps lure
/// wild animals, and every herd refills at its archetype's respawn rate.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn tribal_wildlife(
    time: Res<Time>,
    run: Res<RunProgress>,
    mut rts: ResMut<TribalRts>,
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut animals: Query<
        (
            Entity,
            &mut Transform,
            &mut WildAnimal,
            &Health,
            Option<&mut HerdMember>,
            Has<EpicPredator>,
        ),
        Without<TribeUnit>,
    >,
    tribe: Query<
        (Entity, &Transform, &Health),
        (
            Or<(With<TribeUnit>, With<livestock::DomesticAnimal>)>,
            Without<WildAnimal>,
        ),
    >,
    mut meat: Query<(Entity, &mut MeatPile)>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let dt = time.delta_secs();
    let seed = rts.seed;
    let mut rng = ChaCha8Rng::seed_from_u64(rts.rng);
    rts.rng = rng.random();
    let members: Vec<(Entity, Vec3)> = tribe
        .iter()
        .filter(|(_, _, h)| !h.dead())
        .map(|(e, t, _)| (e, t.translation))
        .collect();
    let traps: Vec<Vec3> = run
        .tribal_campaign
        .active_traps
        .iter()
        .map(|t| Vec3::from_array(t.position))
        .collect();
    let lure = crate::tribal_rules::trap_lure_radius();
    for (_, mut t, mut a, health, herd, epic) in &mut animals {
        let pos = t.translation;
        // Herd casting: a herd comes alive when a unit of the tribe is near
        // and freezes again once every unit is far away.
        let mut wander = 1.0..6.0;
        if let Some(mut herd) = herd {
            let nearest = members
                .iter()
                .map(|(_, p)| dist_xz(*p, pos))
                .fold(f32::MAX, f32::min);
            herd.active = crate::tribal_rules::herd_active(herd.active, nearest);
            if !herd.active {
                a.chase = None;
                continue;
            }
            // Guards hold their post for `guardPositionDurationRange`, then
            // take the next (ADAPTED order: post by post); loungers keep to
            // their group's spot (T9E).
            let groups = crate::tribal_rules::herd_archetype(herd.herd).groups;
            if let crate::tribal_rules::HerdRole::Guard { post } = herd.role {
                wander = 0.5..1.5;
                herd.post_timer -= dt;
                if herd.post_timer <= 0.0 {
                    if let Some(nest) = rts.herd_home(herd.herd) {
                        let next = (post + 1) % groups.guard_locations.max(1);
                        let roll = rng.random_range(0.0..1.0);
                        let (at, hold) =
                            crate::tribal_rules::guard_post(&groups, nest, next, guard_phase(herd.herd), roll);
                        herd.role = crate::tribal_rules::HerdRole::Guard { post: next };
                        herd.post_timer = hold;
                        a.home = tribal::keep_on_land(at);
                    }
                }
            } else {
                wander = 0.6..2.5;
            }
        }
        let hurt = health.hp < health.max;
        let mut goal = None;
        let mut speed = a.speed * 0.5;
        if epic {
            // The epic guards its lair: it goes for units close to it and is
            // leashed back home beyond `EPIC_LEASH`.
            let at_home = dist_xz(pos, a.home) <= EPIC_LEASH;
            let prey = members
                .iter()
                .filter(|(_, p)| at_home && dist_xz(*p, pos) < EPIC_AGGRO_RADIUS)
                .min_by(|x, y| dist_xz(pos, x.1).total_cmp(&dist_xz(pos, y.1)));
            a.chase = prey.map(|(e, _)| *e);
            if let Some((_, p)) = prey {
                if dist_xz(*p, pos) > 2.0 {
                    goal = Some(*p);
                    speed = a.speed;
                }
            } else if !at_home {
                goal = Some(a.home);
                speed = a.speed;
            }
        } else if a.hostile {
            // Stalk the nearest tribe unit near its range that is inside its
            // territory; never chase beyond it.
            let in_territory = dist_xz(pos, a.home) <= PREDATOR_TERRITORY;
            let prey = members
                .iter()
                .filter(|(_, p)| {
                    in_territory
                        && dist_xz(*p, a.home) <= PREDATOR_TERRITORY
                        && dist_xz(*p, pos) < if hurt { 14.0 } else { 7.5 }
                })
                .min_by(|x, y| dist_xz(pos, x.1).total_cmp(&dist_xz(pos, y.1)));
            a.chase = prey.map(|(e, _)| *e);
            if let Some((_, p)) = prey {
                if dist_xz(*p, pos) > 1.5 {
                    goal = Some(*p);
                    speed = a.speed;
                }
            } else if !in_territory {
                goal = Some(a.home);
                speed = a.speed;
            }
        } else if hurt && a.flee < 3.0 {
            // A wounded grazer bolts away from the hunters for a while.
            a.flee += dt;
            if let Some((_, p)) = members
                .iter()
                .min_by(|x, y| dist_xz(pos, x.1).total_cmp(&dist_xz(pos, y.1)))
            {
                let away = Vec2::new(pos.x - p.x, pos.z - p.z).normalize_or(Vec2::X);
                goal = Some(pos + Vec3::new(away.x, 0.0, away.y) * 4.0);
                speed = a.speed * 1.2;
            }
        }
        // A trap (the Traps superpower) lures the wild animals around it.
        if goal.is_none() && a.chase.is_none() && !epic {
            if let Some(trap) = traps
                .iter()
                .filter(|p| dist_xz(**p, pos) <= lure)
                .min_by(|x, y| dist_xz(pos, **x).total_cmp(&dist_xz(pos, **y)))
            {
                goal = Some(*trap);
            }
        }
        if goal.is_none() && a.chase.is_none() {
            a.wait -= dt;
            if a.wait <= 0.0 && dist_xz(pos, a.target) < 0.5 {
                let angle = rng.random_range(0.0..std::f32::consts::TAU);
                let r = rng.random_range(wander.clone());
                a.target = a.home + Vec3::new(angle.cos() * r, 0.0, angle.sin() * r);
                a.wait = rng.random_range(2.0..6.0);
            }
            if dist_xz(pos, a.target) >= 0.5 {
                goal = Some(a.target);
            }
        }
        // An epic lingers at its lair (ADAPTED: wanders half as far).
        if epic && a.chase.is_none() && dist_xz(a.target, a.home) > 3.0 {
            a.target = a.home + (a.target - a.home) * 0.5;
        }
        if let Some(goal) = goal {
            let goal = tribal::keep_on_land(goal);
            let (next, _) = step_toward(pos, goal, speed, dt);
            face(&mut t, next - pos);
            t.translation.x = next.x;
            t.translation.z = next.z;
        }
    }
    // Rot carcasses nobody collects.
    for (e, mut m) in &mut meat {
        m.rot += dt;
        if m.rot > MEAT_ROT_SECONDS || m.food_left <= 0.5 {
            commands.entity(e).despawn();
        }
    }
    // Restock: every herd short of creatures gets one back after its
    // archetype's `RespawnRate` (prey and predators 120 s, epics 1200 s).
    let herds = &crate::tribal_rules::TRIBAL_HERDS;
    let mut alive = [0usize; crate::tribal_rules::TRIBAL_HERDS.len()];
    for (_, _, _, health, herd, _) in animals.iter() {
        if let Some(herd) = herd.filter(|_| !health.dead()) {
            if let Some(n) = alive.get_mut(herd.herd) {
                *n += 1;
            }
        }
    }
    if rts.herd_homes.len() < herds.len() {
        rts.herd_homes = herd_homes(seed);
    }
    for (index, archetype) in herds.iter().enumerate() {
        let missing = archetype.creatures.saturating_sub(alive[index]);
        let (timer, back) = crate::tribal_rules::herd_respawn_step(
            rts.herd_timers[index],
            dt,
            missing,
            archetype.respawn,
        );
        rts.herd_timers[index] = timer;
        if back {
            let member = alive[index];
            let p = herd_spot(seed, rts.herd_homes[index], index, member);
            spawn_herd_creature(
                &mut commands,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                index,
                member,
                p,
            );
        }
    }
    // Forest herds are cast from their nests (T9F): their creatures come when a
    // unit of the tribe is within the activation distance, leave beyond the
    // deactivation distance, and a cast herd refills at its `RespawnRate`.
    let (on, off) = crate::tribal_rules::herd_activation_distances();
    let cast: Vec<(Entity, usize, bool)> = animals
        .iter()
        .filter_map(|(e, _, _, h, herd, _)| herd.map(|m| (e, m.herd, h.dead())))
        .collect();
    for i in 0..rts.forest_herds.len() {
        let fh = rts.forest_herds[i];
        let archetype = crate::tribal_rules::herd_archetype(fh.herd);
        let nearest = members.iter().map(|(_, p)| dist_xz(*p, fh.home)).fold(f32::MAX, f32::min);
        let mine: Vec<(Entity, bool)> =
            cast.iter().filter(|(_, h, _)| *h == fh.herd).map(|(e, _, dead)| (*e, *dead)).collect();
        if !fh.spawned && nearest < on {
            for member in 0..archetype.creatures {
                let p = herd_spot(seed, fh.home, fh.herd, member);
                spawn_herd_creature(
                    &mut commands, &mut originals, &mut meshes, &mut images, &mut materials, fh.herd, member, p,
                );
            }
            rts.forest_herds[i].spawned = true;
            rts.forest_herds[i].timer = 0.0;
        } else if fh.spawned && nearest > off {
            for (e, _) in &mine {
                commands.entity(*e).despawn();
            }
            rts.forest_herds[i].spawned = false;
        } else if fh.spawned {
            let alive = mine.iter().filter(|(_, dead)| !dead).count();
            let missing = archetype.creatures.saturating_sub(alive);
            let (timer, back) =
                crate::tribal_rules::herd_respawn_step(fh.timer, dt, missing, archetype.respawn);
            rts.forest_herds[i].timer = timer;
            if back {
                let p = herd_spot(seed, fh.home, fh.herd, alive);
                spawn_herd_creature(
                    &mut commands, &mut originals, &mut meshes, &mut images, &mut materials, fh.herd, alive, p,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rival villages: villagers, defence, raids, ally gifts
// ---------------------------------------------------------------------------

/// Villagers wander at home, defend against attackers, and raid the food
/// mat; allies send food gifts.
#[allow(clippy::too_many_arguments)]
pub fn tribal_rivals(
    time: Res<Time>,
    mut rts: ResMut<TribalRts>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rivals: Query<
        (
            Entity,
            &mut Transform,
            &mut RivalMember,
            Option<&mut Raider>,
            Option<&mut crate::tribal_plans::GiftBearer>,
        ),
        Without<TribeUnit>,
    >,
    tribe: Query<(Entity, &Transform, &UnitTask, &Health), (With<TribeUnit>, Without<RivalMember>)>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let dt = time.delta_secs();
    let run = &mut *run;
    let mut rng = ChaCha8Rng::seed_from_u64(rts.rng ^ 0x5EED);
    let members: Vec<(Entity, Vec3, UnitTask)> = tribe
        .iter()
        .filter(|(_, _, _, h)| !h.dead())
        .map(|(e, t, task, _)| (e, t.translation, *task))
        .collect();
    let member_spots: Vec<(Entity, Vec3)> = members.iter().map(|(e, p, _)| (*e, *p)).collect();
    let raid_alerts: Vec<bool> = (0..tribal::RIVAL_VILLAGES)
        .map(|v| run.tribal_campaign.raid_alerted(v))
        .collect();
    let mut alive = [0usize; tribal::RIVAL_VILLAGES];
    for (entity, mut t, mut r, raider, bearer) in &mut rivals {
        let status = run
            .tribal_campaign
            .village(r.village)
            .map(|v| v.status)
            .unwrap_or_default();
        if status == RivalStatus::Conquered {
            // The survivors scatter when their village falls.
            commands.entity(entity).despawn();
            continue;
        }
        let pos = t.translation;
        let mut goal = None;
        let mut speed = r.speed * 0.5;
        if let Some(mut raider) = raider {
            alive[r.village] += 1;
            // A worker of its tribe's raid / steal-food plan (`tribal_plans`).
            if status == RivalStatus::Allied && raider.phase != RaidPhase::Retreat && raider.phase != RaidPhase::Done {
                raider.phase = RaidPhase::Retreat;
            }
            let step = crate::tribal_plans::step_raider(run, &mut r, &mut raider, pos, dt, &member_spots);
            goal = step.goal;
            if step.run {
                speed = r.speed;
            }
        } else if let Some(mut bearer) = bearer {
            alive[r.village] += 1;
            let step = crate::tribal_plans::step_gift_bearer(run, &r, &mut bearer, pos);
            goal = step.goal;
        } else {
            alive[r.village] += 1;
            // Defend: anyone attacking the village or its people is fought.
            let threat = (status == RivalStatus::Hostile)
                .then(|| {
                    members
                        .iter()
                        .filter(|(_, p, task)| {
                            dist_xz(*p, r.home) < 11.0
                                && (matches!(task, UnitTask::Siege(v) | UnitTask::Raid(v) if *v == r.village)
                                    || raid_alerts[r.village])
                                || matches!(task, UnitTask::Hunt(e) if *e == entity)
                                || (dist_xz(*p, pos) < 3.0
                                    && matches!(task, UnitTask::Hunt(_) | UnitTask::Siege(_)))
                        })
                        .min_by(|a, b| dist_xz(pos, a.1).total_cmp(&dist_xz(pos, b.1)))
                        .map(|(e, p, _)| (*e, *p))
                })
                .flatten();
            r.mode = match threat {
                Some((e, _)) => RivalMode::Defend(e),
                None => RivalMode::Wander,
            };
            if let Some((_, p)) = threat {
                if dist_xz(pos, p) > 1.4 {
                    goal = Some(p);
                    speed = r.speed;
                }
            } else {
                r.wait -= dt;
                if r.wait <= 0.0 && dist_xz(pos, r.target) < 0.5 {
                    let angle = rng.random_range(0.0..std::f32::consts::TAU);
                    let rad = rng.random_range(2.5..6.5);
                    r.target = r.home + Vec3::new(angle.cos() * rad, 0.0, angle.sin() * rad);
                    r.wait = rng.random_range(1.5..5.0);
                }
                if dist_xz(pos, r.target) >= 0.5 {
                    goal = Some(r.target);
                }
            }
        }
        if let Some(goal) = goal {
            let goal = tribal::keep_on_land(goal);
            let (next, _) = step_toward_swim(pos, goal, speed, dt);
            face(&mut t, next - pos);
            t.translation.x = next.x;
            t.translation.z = next.z;
        }
    }
    rts.rng = rng.random();
    // Replace fallen villagers of hostile and allied villages.
    for v in 0..tribal::RIVAL_VILLAGES {
        // A tribe that has not come into view has no members to replace.
        let Some(status) = run.tribal_campaign.village(v).map(|s| s.status) else {
            rts.villager_timers[v] = 0.0;
            continue;
        };
        // The tribe breeds back to its starting strength (3 / 6 / 9 by tier):
        // mating plus growing up takes 65 s.
        if status == RivalStatus::Conquered
            || alive[v] >= run.tribal_campaign.tribe_of(v).initial_population
        {
            rts.villager_timers[v] = 0.0;
            continue;
        }
        rts.villager_timers[v] += dt;
        if rts.villager_timers[v] >= crate::tribal_rules::villager_replace_seconds() {
            rts.villager_timers[v] = 0.0;
            spawn_villager(
                &mut commands,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                v,
                alive[v],
                status,
                false,
            );
        }
    }
    // Raids, food thieves and gifts (also the allies' gifts) are the NPC
    // plans (`tribal_plans`, T9).
}

// ---------------------------------------------------------------------------
// Combat, healing, death
// ---------------------------------------------------------------------------

/// Resolve every fight of the frame, then healing, deaths and carcasses.
#[allow(clippy::too_many_arguments)]
pub fn tribal_combat(
    time: Res<Time>,
    mut run: ResMut<RunProgress>,
    mut commands: Commands,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut tribe: Query<
        (
            Entity,
            &mut Transform,
            &mut TribeUnit,
            &mut UnitTask,
            &mut Health,
            Option<&WorldId>,
            Option<&TribeRole>,
        ),
        (Without<WildAnimal>, Without<RivalMember>),
    >,
    mut animals: Query<
        (Entity, &Transform, &WildAnimal, &mut Health),
        (Without<TribeUnit>, Without<RivalMember>),
    >,
    mut rivals: Query<
        (
            Entity,
            &Transform,
            &RivalMember,
            &mut Health,
            Option<&Raider>,
        ),
        (Without<TribeUnit>, Without<WildAnimal>),
    >,
    mut pets: Query<
        (
            Entity,
            &Transform,
            &mut Health,
            &livestock::PetGuard,
            &livestock::DomesticAnimal,
        ),
        (
            With<livestock::DomesticAnimal>,
            Without<TribeUnit>,
            Without<WildAnimal>,
            Without<RivalMember>,
        ),
    >,
    herds: Query<&HerdMember>,
    epics: Query<(), With<EpicPredator>>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let dt = time.delta_secs();
    let run = &mut *run;
    let roles: Vec<u32> = run.tribe.iter().map(|m| m.role).collect();
    // Snapshot the tribe.
    let units: Vec<(Entity, Vec3, UnitTask, Option<ToolKind>, bool, bool)> = tribe
        .iter()
        .map(|(e, t, u, task, h, id, _)| {
            (
                e,
                t.translation,
                *task,
                id.and_then(|id| run.tribal_campaign.tool_of(id.0)),
                u.chieftain,
                h.dead(),
            )
        })
        .collect();

    // Traps (the Traps superpower) spring on the first raider or wild animal
    // that comes within `PoisonRange` (scaled). A wild animal is ensnared and
    // left as a carcass for the hunters; raiders and the epics, too big to
    // hold, take ADAPTED 0.81 damage instead.
    if !run.tribal_campaign.active_traps.is_empty() {
        let spring = crate::tribal_rules::trap_spring_radius();
        let mut sprung: Option<(usize, Entity, bool)> = None;
        'traps: for (idx, trap) in run.tribal_campaign.active_traps.iter().enumerate() {
            let trap_pos = Vec3::from_array(trap.position);
            for (e, t, _, h, raider) in rivals.iter() {
                if raider.is_some() && !h.dead() && dist_xz(t.translation, trap_pos) <= spring {
                    sprung = Some((idx, e, true));
                    break 'traps;
                }
            }
            for (e, t, _, h) in animals.iter() {
                if !h.dead() && dist_xz(t.translation, trap_pos) <= spring {
                    sprung = Some((idx, e, false));
                    break 'traps;
                }
            }
        }
        if let Some((idx, target, is_raider)) = sprung {
            run.tribal_campaign.active_traps.remove(idx);
            if is_raider {
                if let Ok((_, _, _, mut h, _)) = rivals.get_mut(target) {
                    h.hurt(crate::tribal_rules::TRAP_DAMAGE);
                }
                run.tribal_campaign.say("Snap! A trap caught a rival raider!");
            } else {
                let epic = epics.contains(target);
                if let Ok((_, _, _, mut h)) = animals.get_mut(target) {
                    let damage = if epic {
                        crate::tribal_rules::TRAP_DAMAGE
                    } else {
                        h.max
                    };
                    h.hurt(damage);
                }
                run.tribal_campaign.say(if epic {
                    "Snap! The trap wounds the epic beast!"
                } else {
                    "Snap! A trap caught wild game - collect the meat!"
                });
            }
            run.request_save();
        }
    }

    // Idle units guard: raiders and predators close by are fought.
    let threats: Vec<(Entity, Vec3)> = rivals
        .iter()
        .filter(|r| r.4.is_some() && !r.3.dead())
        .map(|r| (r.0, r.1.translation))
        .chain(
            animals
                .iter()
                .filter(|a| a.2.hostile && a.2.chase.is_some() && !a.3.dead())
                .map(|a| (a.0, a.1.translation)),
        )
        .collect();
    for (e, pos, task, _, chieftain, dead) in &units {
        if *dead || *chieftain || !matches!(task, UnitTask::Idle(_) | UnitTask::Follow) {
            continue;
        }
        if let Some((foe, _)) = threats
            .iter()
            .filter(|(_, p)| dist_xz(*p, *pos) < GUARD_RADIUS)
            .min_by(|a, b| dist_xz(*pos, a.1).total_cmp(&dist_xz(*pos, b.1)))
        {
            if let Ok((_, _, _, mut t, ..)) = tribe.get_mut(*e) {
                *t = UnitTask::Hunt(*foe);
            }
        }
    }
    // Besiegers first deal with the defenders standing in their way.
    let defenders: Vec<(Entity, Vec3, usize, bool)> = rivals
        .iter()
        .filter(|r| r.4.is_none() && !r.3.dead())
        .map(|r| {
            (
                r.0,
                r.1.translation,
                r.2.village,
                matches!(r.2.mode, RivalMode::Defend(_)),
            )
        })
        .collect();
    let mut strikes: Vec<(Entity, Vec3, Entity, Option<ToolKind>, bool)> = Vec::new();
    for (e, pos, task, tool, chieftain, dead) in &units {
        let fighting = match (*dead, task) {
            (false, UnitTask::Siege(v)) => defenders
                .iter()
                .filter(|d| d.2 == *v && dist_xz(d.1, *pos) <= tools::attack_range(*tool) + 0.3)
                .min_by(|a, b| dist_xz(*pos, a.1).total_cmp(&dist_xz(*pos, b.1)))
                .map(|d| d.0),
            (false, UnitTask::Hunt(target)) => Some(*target),
            _ => None,
        };
        // Nobody in reach, but a defender is fighting the tribe close by: step
        // towards it (the next frames bring it into reach).
        if fighting.is_none() && !*dead {
            if let UnitTask::Siege(v) = task {
                let help = defenders
                    .iter()
                    .filter(|d| d.2 == *v && d.3 && dist_xz(d.1, *pos) <= SIEGE_HELP_RADIUS)
                    .min_by(|a, b| dist_xz(*pos, a.1).total_cmp(&dist_xz(*pos, b.1)))
                    .map(|d| d.1);
                if let (Some(at), Ok((_, mut t, unit, ..))) = (help, tribe.get_mut(*e)) {
                    let (next, _) = step_toward_swim(t.translation, at, unit.speed, dt);
                    t.translation.x = next.x;
                    t.translation.z = next.z;
                }
            }
        }
        if let Ok((_, _, mut unit, ..)) = tribe.get_mut(*e) {
            unit.fighting = matches!(task, UnitTask::Siege(_)) && fighting.is_some();
        }
        if let Some(target) = fighting {
            strikes.push((*e, *pos, target, *tool, *chieftain));
        }
    }
    // Tribe strikes: each unit hits with the damage and refresh of its weapon
    // (the chieftain with his staff), the spear throwing beyond melee reach; every
    // `refresh` seconds of fighting the weapon's special hits an area.
    let mut specials: Vec<(Vec3, f32, f32)> = Vec::new();
    for (e, pos, target, tool, chieftain) in &strikes {
        let target = *target;
        let range = tools::attack_range(*tool) + 0.3;
        let weapon = if *chieftain {
            crate::tribal_rules::CHIEFTAIN_STAFF
        } else {
            crate::tribal_rules::weapon_of(*tool)
        };
        let scale = if *chieftain {
            crate::tribal_rules::CHIEFTAIN_DAMAGE_SCALE
        } else {
            1.0
        };
        let mut struck: Option<Vec3> = None;
        let mut struck_village: Option<usize> = None;
        if let Ok((_, t, _, mut h)) = animals.get_mut(target) {
            let d = dist_xz(*pos, t.translation);
            if d <= range {
                h.hurt(weapon.dps_at(d, tools::MELEE_REACH) * scale * dt);
                struck = Some(t.translation);
            }
        } else if let Ok((_, t, rival, mut h, _)) = rivals.get_mut(target) {
            let d = dist_xz(*pos, t.translation);
            if d <= range {
                h.hurt(weapon.dps_at(d, tools::MELEE_REACH) * scale * dt);
                struck = Some(t.translation);
                struck_village = Some(rival.village);
            }
        }
        // Hurting a tribe's member is a `TribeAttack` event (-10, floor -60) per
        // blow (0xC26C85): this frame's share of one blow of the weapon.
        if let Some(village) = struck_village {
            run.tribal_campaign.post_hostile_strikes(
                village,
                crate::tribal_rules::RelationEvent::Attack,
                dt / weapon.refresh.max(0.1),
            );
        }
        if let (Some(at), Some(special)) = (struck, weapon.special) {
            if let Ok((_, _, mut unit, ..)) = tribe.get_mut(*e) {
                let (timer, fire) =
                    crate::tribal_rules::special_step(unit.special_timer, dt, special.refresh);
                unit.special_timer = timer;
                if fire {
                    specials.push((
                        at,
                        special.cone_radius.max(0.5),
                        (special.damage + special.ranged_damage) * scale,
                    ));
                }
            }
        }
    }
    for (at, radius, damage) in specials {
        for (_, t, _, mut h) in &mut animals {
            if !h.dead() && dist_xz(at, t.translation) <= radius {
                h.hurt(damage);
            }
        }
        for (_, t, rival, mut h, _) in &mut rivals {
            if !h.dead() && dist_xz(at, t.translation) <= radius {
                h.hurt(damage);
                run.tribal_campaign.post_hostile_strikes(
                    rival.village,
                    crate::tribal_rules::RelationEvent::Attack,
                    1.0,
                );
            }
        }
    }
    // Pets defend locally using the same living foe health as tribe strikes.
    for (_, pose, health, guard, _) in &pets {
        if health.dead() {
            continue;
        }
        let Some(target) = guard.target else {
            continue;
        };
        if let Ok((_, t, _, mut h)) = animals.get_mut(target) {
            if dist_xz(pose.translation, t.translation) <= livestock::PET_REACH {
                h.hurt(livestock::PET_DPS * dt);
            }
        } else if let Ok((_, t, rival, mut h, _)) = rivals.get_mut(target) {
            if run
                .tribal_campaign
                .village(rival.village)
                .is_some_and(|v| v.status == RivalStatus::Hostile)
                && dist_xz(pose.translation, t.translation) <= livestock::PET_REACH
            {
                h.hurt(livestock::PET_DPS * dt);
            }
        }
    }
    let combatants: Vec<_> = units
        .iter()
        .filter(|u| !u.5)
        .map(|u| (u.0, u.1))
        .chain(
            pets.iter()
                .filter(|p| !p.2.dead())
                .map(|p| (p.0, p.1.translation)),
        )
        .collect();
    // Predators, defenders and raiders strike back.
    let mut hits: Vec<(Entity, f32, Entity)> = Vec::new();
    // Beastmaster: animals within the power's reach (`creatureAbilityRange`
    // 50, scaled) of where it was used.
    let charm_reach =
        crate::tribal_rules::superpower_reach(&crate::tribal_rules::SUPERPOWER_BEASTMASTER);
    let is_charmed = |pos: Vec3| {
        run.tribal_campaign
            .charmed_creatures
            .iter()
            .any(|c| dist_xz(pos, Vec3::from_array(c.position)) <= charm_reach)
    };
    for (e, t, a, h) in animals.iter() {
        if !a.hostile || h.dead() {
            continue;
        }
        if is_charmed(t.translation) {
            // Charmed beasts strike at raiders!
            for (re, rt, _, rh, raider) in rivals.iter() {
                if raider.is_some() && !rh.dead() && dist_xz(rt.translation, t.translation) < 2.5 {
                    hits.push((re, 12.0 * dt, e));
                    break;
                }
            }
            continue;
        }
        if let Some(prey) = a.chase {
            // A herd predator bites with its archetype's damage (a level-3 bite
            // times `DamageMultiplier`, x15 for an epic); creatures outside the
            // herds keep OpenSpore's 7 a second.
            let reach = if epics.contains(e) { 3.0 } else { 2.0 };
            if combatants
                .iter()
                .any(|u| u.0 == prey && dist_xz(u.1, t.translation) < reach)
            {
                let dps = herds
                    .get(e)
                    .map(|h| crate::tribal_rules::predator_dps(crate::tribal_rules::herd_archetype(h.herd)))
                    .unwrap_or(7.0);
                hits.push((prey, dps * dt, e));
            }
        }
    }
    for (attacker, t, r, h, raider) in rivals.iter() {
        if h.dead()
            || run
                .tribal_campaign
                .village(r.village)
                .is_none_or(|v| v.status != RivalStatus::Hostile)
        {
            continue;
        }
        let target = match r.mode {
            RivalMode::Defend(e) => Some(e),
            _ if raider.is_some() => combatants
                .iter()
                .filter(|u| dist_xz(u.1, t.translation) < 2.0)
                .min_by(|a, b| dist_xz(a.1, t.translation).total_cmp(&dist_xz(b.1, t.translation)))
                .map(|u| u.0),
            _ => None,
        };
        if let Some(target) = target {
            if combatants
                .iter()
                .any(|u| u.0 == target && dist_xz(u.1, t.translation) < 2.0)
            {
                // A defender hits with the average weapon of its tribe's roles.
                let defend =
                    tribal::retaliation_dps(run.tribal_campaign.tribe_of(r.village), &roles);
                hits.push((target, defend * dt, attacker));
            }
        }
    }
    for (target, damage, attacker) in hits {
        // The chieftain's toughness is his `chieftainHealthScale` (1.5x a
        // member's hit points); blows land at full strength.
        if let Ok((_, t, mut unit, mut task, mut h, _, _)) = tribe.get_mut(target) {
            h.hurt(damage);
            // Self defence: a hurt member at work fights back (INFERRED rule,
            // tribal_rules::RETALIATE_LEASH); the chieftain too.
            if !h.dead() {
                let at = t.translation;
                unit.retaliate_against(&mut task, attacker, at);
            }
        } else if let Ok((_, _, mut h, _, _)) = pets.get_mut(target) {
            h.hurt(damage);
        }
    }
    // Keep the campaign and checkpoint on the same post-combat health frame.
    for (_, _, health, _, animal) in &pets {
        if let Some(record) = run
            .tribal_campaign
            .livestock
            .iter_mut()
            .find(|record| record.slot == animal.0)
        {
            record.health = health.hp;
        }
        if health.dead() {
            run.request_save();
        }
    }
    // Healing rods (a shaman restores `healPercentPerSecondWithTool` = 5% of a
    // member's health per second within 5 units; healers do not stack) and the
    // campfire.
    let campfire = run.tribal_campaign.has_shack(ToolKind::Campfire);
    let healers: Vec<(Vec3, f32)> = units
        .iter()
        .filter(|u| !u.5)
        .map(|u| (u.1, tools::heal_rate(u.3)))
        .filter(|(_, r)| *r > 0.0)
        .collect();
    for (_, t, _, _, mut h, _, _) in &mut tribe {
        if h.dead() {
            continue;
        }
        let percent = healers
            .iter()
            .filter(|(p, _)| crate::tribal_rules::in_healing_reach(dist_xz(*p, t.translation)))
            .map(|(_, r)| *r)
            .fold(0.0, f32::max);
        let mut regen: f32 = h.max * percent / 100.0;
        if campfire && dist_xz(t.translation, tribal::home_position()) <= tools::CAMPFIRE_RADIUS {
            regen += tools::CAMPFIRE_REGEN;
        }
        if regen > 0.0 {
            h.heal(regen * dt);
        }
    }
    // Deaths: carcasses for the hunters, the chieftain is carried home.
    for (e, t, animal, h) in animals.iter() {
        if !h.dead() {
            continue;
        }
        let at = t.translation;
        commands.entity(e).despawn();
        let carcass = spawn_meat(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            at,
            // The herd archetype's `foodValue` (prey 10 / 15, predators 20,
            // epics 300); creatures outside the herds keep the old mapping.
            herds
                .get(e)
                .map(|h| h.food)
                .unwrap_or_else(|_| crate::tribal_rules::carcass_food(animal.hostile)),
        );
        for (_, _, mut unit, mut task, ..) in &mut tribe {
            if *task == UnitTask::Hunt(e) {
                *task = unit.end_retaliation(UnitTask::Gather(carcass));
            }
        }
        run.tribal_campaign
            .say("The hunt is won - carry the meat home!");
    }
    for (e, _, r, h, raider) in rivals.iter() {
        if !h.dead() {
            continue;
        }
        commands.entity(e).despawn();
        // Killing a tribe's member is a `TribeKill` event (-60, floor -100).
        run.tribal_campaign
            .post_event(r.village, crate::tribal_rules::RelationEvent::Kill);
        if raider.is_some() && r.stolen > 0.5 {
            // Stolen food falls where the raider fell... back onto the mat.
            run.dna_this_stage += r.stolen;
            run.tribal_campaign
                .say(format!("A raider falls - {:.0} food recovered!", r.stolen));
        } else if raider.is_some() {
            run.tribal_campaign.say("A raider is driven off!");
        }
        for (_, _, mut unit, mut task, ..) in &mut tribe {
            if *task == UnitTask::Hunt(e) {
                *task = unit.end_retaliation(UnitTask::Idle(
                    tribal::home_position() + Vec3::new(0.0, 0.0, 3.0),
                ));
            }
        }
    }
    for (_, mut t, mut unit, mut task, h, _, _) in &mut tribe {
        if unit.chieftain && h.dead() && run.tribal_campaign.chieftain_respawn <= 0.0 {
            // The chieftain falls and waits, hidden at the hut, for
            // `chieftainRespawnTime` (90 s); `tribal_chieftain_respawn` brings
            // him back with full health.
            t.translation = tribal::home_position() + Vec3::new(2.0, t.translation.y, 6.0);
            unit.carrying = 0.0;
            *task = UnitTask::Idle(t.translation);
            run.tribal_campaign.chieftain_respawn = crate::tribal_rules::CHIEFTAIN_RESPAWN_TIME;
            run.tribal_campaign.say(format!(
                "The chieftain has fallen - he returns in {:.0} seconds.",
                crate::tribal_rules::CHIEFTAIN_RESPAWN_TIME
            ));
            run.request_save();
        }
    }
}

/// Spawn a carcass worth `food` (original meat prop when installed).
fn spawn_meat(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    food: f32,
) -> Entity {
    let pile = MeatPile {
        food_left: food,
        rot: 0.0,
    };
    if let Some(e) = crate::world_objects::spawn(
        commands,
        originals,
        crate::world_objects::FOOD_GROUP,
        "foodpile_meat_01",
        meshes,
        images,
        materials,
        Vec3::new(at.x, -0.6, at.z),
        1.4,
    ) {
        commands.entity(e).insert(pile);
        return e;
    }
    commands
        .spawn((
            StageOwned,
            pile,
            Name::new("meat"),
            Mesh3d(meshes.add(Cuboid::new(0.8, 0.4, 0.6))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.2, 0.2),
                ..default()
            })),
            Transform::from_xyz(at.x, -0.4, at.z),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Presentation: rings, carried food, home props, pings, ripples, panel
// ---------------------------------------------------------------------------

/// The asset stores spawning original models needs, bundled into one param.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ModelAssets<'w> {
    pub originals: ResMut<'w, crate::Originals>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
}

/// Keep rings, carried bundles, home props and the panel in step.
#[allow(clippy::too_many_arguments)]
pub fn tribal_presentation(
    time: Res<Time>,
    run: Res<RunProgress>,
    rts: Option<Res<TribalRts>>,
    assets: Option<Res<RtsAssets>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut commands: Commands,
    mut models: ModelAssets,
    overlay: Query<(Entity, &Transform), With<OverlayRoot>>,
    actors: Query<
        (
            Entity,
            &Transform,
            Option<&TribeUnit>,
            Option<&Selected>,
            Option<&RivalMember>,
            Option<&WildAnimal>,
            Option<&Health>,
        ),
        (
            Or<(With<TribeUnit>, With<RivalMember>, With<WildAnimal>)>,
            Without<UnitRing>,
            Without<OverlayRoot>,
            Without<CommandPing>,
            Without<Ripple>,
        ),
    >,
    mut rings: Query<
        (
            Entity,
            &UnitRing,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (Without<OverlayRoot>, Without<CommandPing>, Without<Ripple>),
    >,
    home_props: Query<(Entity, &HomeProp)>,
    mut pings: Query<
        (Entity, &mut CommandPing, &mut Transform, Option<&Mesh3d>),
        (Without<UnitRing>, Without<Ripple>, Without<OverlayRoot>),
    >,
    mut ripples: Query<
        (&mut Ripple, &mut Transform),
        (
            Without<UnitRing>,
            Without<CommandPing>,
            Without<OverlayRoot>,
        ),
    >,
    mut panel: PanelQueries,
) {
    let dt = time.delta_secs();
    let Some(rts) = rts else {
        return;
    };
    let ModelAssets {
        originals,
        meshes,
        images,
        materials,
    } = &mut models;
    let assets = match assets {
        Some(a) => a,
        None => {
            let ring = meshes.add(Torus::new(0.92, 1.0));
            let make = |color: Color| StandardMaterial {
                base_color: color,
                emissive: color.to_linear() * 0.4,
                unlit: true,
                cull_mode: None,
                ..default()
            };
            commands.insert_resource(RtsAssets {
                ring,
                // The manual: "A selected tribe member has a yellow circle around them."
                selected: materials.add(make(Color::srgb(1.0, 0.92, 0.12))),
                member: materials.add(make(Color::srgba(0.30, 0.65, 0.30, 0.9))),
                hostile: materials.add(make(Color::srgb(1.0, 0.30, 0.12))),
                allied: materials.add(make(Color::srgb(0.25, 0.65, 1.0))),
                wild: materials.add(make(Color::srgb(0.9, 0.5, 0.15))),
                ping: materials.add(make(Color::srgb(1.0, 1.0, 0.6))),
            });
            return;
        }
    };
    let Ok((overlay, overlay_t)) = overlay.single() else {
        return;
    };
    let base = overlay_t.translation;
    let height = |p: Vec3| surface.as_deref().map_or(-0.6, |s| s.height(p.x, p.z));
    // --- rings ---------------------------------------------------------------
    let mut wanted: Vec<(Entity, Vec3, f32, Handle<StandardMaterial>)> = Vec::new();
    for (e, t, unit, selected, rival, animal, health) in actors.iter() {
        if health.is_some_and(|h| h.dead()) {
            continue;
        }
        let p = t.translation;
        let (radius, material) = if let Some(_unit) = unit {
            (
                0.85 * t.scale.x.max(0.6),
                if selected.is_some() {
                    assets.selected.clone()
                } else {
                    assets.member.clone()
                },
            )
        } else if let Some(r) = rival {
            let status = run
                .tribal_campaign
                .village(r.village)
                .map(|v| v.status)
                .unwrap_or_default();
            (
                0.7,
                if status == RivalStatus::Allied {
                    assets.allied.clone()
                } else {
                    assets.hostile.clone()
                },
            )
        } else if let Some(a) = animal {
            if !a.hostile && health.is_none_or(|h| h.hp >= h.max) {
                continue;
            }
            (
                0.9,
                if a.hostile {
                    assets.hostile.clone()
                } else {
                    assets.wild.clone()
                },
            )
        } else {
            continue;
        };
        wanted.push((e, p, radius, material));
    }
    let mut seen = Vec::new();
    for (ring, owner, mut t, mut mat) in &mut rings {
        match wanted.iter().find(|w| w.0 == owner.0) {
            Some((_, p, radius, material)) => {
                seen.push(owner.0);
                t.translation = Vec3::new(p.x, height(*p) + 0.06, p.z) - base;
                t.scale = Vec3::new(*radius, 0.06, *radius);
                if mat.0 != *material {
                    mat.0 = material.clone();
                }
            }
            None => commands.entity(ring).despawn(),
        }
    }
    for (e, p, radius, material) in &wanted {
        if seen.contains(e) {
            continue;
        }
        commands.spawn((
            StageOwned,
            UnitRing(*e),
            Mesh3d(assets.ring.clone()),
            MeshMaterial3d(material.clone()),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            Transform::from_translation(Vec3::new(p.x, height(*p) + 0.06, p.z) - base)
                .with_scale(Vec3::new(*radius, 0.06, *radius)),
            ChildOf(overlay),
        ));
    }
    // (The carried bundles are their platters: `tribal_village::carried_platters`.)
    // --- home props follow the campaign ------------------------------------
    let resolved = run.tribal_campaign.resolved_count();
    let food = run.dna_this_stage;
    let members = run.tribe.len();
    // The hut's style (the species' diet) and damage look (`tribal_village`).
    let look = crate::tribal_village::HomeLook::of(&run, originals);
    for (e, prop) in &home_props {
        let want = home_prop_name(
            prop.kind,
            resolved,
            food,
            members,
            run.tribal_campaign.population_cap(),
            &look,
        );
        if want != prop.shown {
            commands.entity(e).despawn();
            spawn_home_prop(
                &mut commands,
                originals,
                meshes,
                images,
                materials,
                prop.kind,
                resolved,
                food,
                members,
                run.tribal_campaign.population_cap(),
                &look,
            );
        }
    }
    // --- pings and ripples ---------------------------------------------------
    for (e, mut ping, mut t, mesh) in &mut pings {
        ping.0 += dt;
        if ping.0 > 0.6 {
            commands.entity(e).despawn();
            continue;
        }
        if mesh.is_none() {
            let p = t.translation;
            t.translation.y = height(p) + 0.08;
            commands.entity(e).insert((
                Mesh3d(assets.ring.clone()),
                MeshMaterial3d(assets.ping.clone()),
                bevy::light::NotShadowCaster,
            ));
        }
        let s = 0.5 + ping.0 * 1.8;
        t.scale = Vec3::new(s, 0.05, s);
    }
    for (mut r, mut t) in &mut ripples {
        r.0 = (r.0 + dt * 0.5) % 1.0;
        let s = 0.4 + r.0 * 1.4;
        t.scale = Vec3::new(s, 0.05, s);
    }
    // --- panel -----------------------------------------------------------
    update_panel(&run, &rts, &actors, &mut panel);
}

/// Pure: which original model a home prop shows for the campaign state. The
/// hut is `MasterHutList`'s list for the player's diet at the tribe level in
/// its damage look, the totem and the food mat follow the level (VERIFIED
/// 0xC99DC0 / 0xC999D0 / 0xE8F530 / 0xE93F30, `tribal_village`).
pub fn home_prop_name(
    kind: HomePropKind,
    resolved: usize,
    food: f32,
    members: usize,
    cap: usize,
    look: &crate::tribal_village::HomeLook,
) -> String {
    let level = crate::tribal_rules::tribe_level(resolved);
    match kind {
        HomePropKind::Hut => crate::tribal_village::home_hut_name(look, resolved),
        HomePropKind::Totem => crate::tribal_village::totem_name(level).into(),
        HomePropKind::FoodMat => crate::tribal_village::food_mat(Some(level)).1.into(),
        HomePropKind::EggPen => {
            if members >= cap {
                "tt_eggpen_none".into()
            } else if food >= tribal::RECRUIT_FOOD {
                "tt_eggpen_full".into()
            } else {
                "tt_eggpen_some".into()
            }
        }
        HomePropKind::FoodPile => {
            if food < 5.0 {
                "empty".into()
            } else {
                format!("foodpile_fruit_0{}", (1 + (food / 60.0) as usize).min(6))
            }
        }
    }
}

/// Spawn (or respawn) one home prop with the model the campaign calls for.
#[allow(clippy::too_many_arguments)]
pub fn spawn_home_prop(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    kind: HomePropKind,
    resolved: usize,
    food: f32,
    members: usize,
    cap: usize,
    look: &crate::tribal_village::HomeLook,
) {
    let name = home_prop_name(kind, resolved, food, members, cap, look);
    let (pos, width, group) = match kind {
        HomePropKind::Hut => (
            tribal::home_position(),
            // The ruin keeps the standing hut's footprint.
            4.2 + tribal::hut_level(resolved) as f32 * 0.6,
            crate::world_objects::HUT_GROUP,
        ),
        HomePropKind::Totem => (tribal::totem_position(), 1.6, tools::TRIBAL_ART_GROUP),
        HomePropKind::EggPen => (tribal::egg_pen_position(), 2.6, tools::TRIBAL_ART_GROUP),
        HomePropKind::FoodMat => (tribal::food_pile_position(), 3.2, tools::TRIBAL_ART_GROUP),
        HomePropKind::FoodPile => (
            tribal::food_pile_position(),
            1.2 + (food / 60.0).min(5.0) * 0.25,
            crate::world_objects::FOOD_GROUP,
        ),
    };
    let marker = HomeProp {
        kind,
        shown: name.clone(),
    };
    if name == "empty" {
        commands.spawn((
            StageOwned,
            marker,
            Transform::from_xyz(pos.x, -0.6, pos.z),
            Visibility::Hidden,
        ));
        return;
    }
    if let Some(e) = tools::spawn_prop(
        commands,
        originals,
        group,
        &name,
        meshes,
        images,
        materials,
        // The food mat lies just under the pile (as the old `tt_food_mat`).
        Vec3::new(pos.x, if kind == HomePropKind::FoodMat { -0.62 } else { -0.6 }, pos.z),
        width,
    ) {
        commands.entity(e).insert(marker);
        if kind == HomePropKind::Hut {
            commands.entity(e).insert(Name::new("home hut"));
        }
        return;
    }
    let (mesh, color, y): (Mesh, Color, f32) = match kind {
        HomePropKind::Hut => (
            Cone::new(2.4, 3.0).into(),
            Color::srgb(0.55, 0.38, 0.2),
            0.4,
        ),
        HomePropKind::Totem => (
            Cylinder::new(0.35, 3.2).into(),
            Color::srgb(0.6, 0.35, 0.18),
            1.0,
        ),
        HomePropKind::EggPen => (
            Cylinder::new(1.1, 0.4).into(),
            Color::srgb(0.75, 0.68, 0.45),
            -0.4,
        ),
        HomePropKind::FoodMat => (
            Cylinder::new(1.5, 0.08).into(),
            Color::srgb(0.62, 0.48, 0.28),
            -0.56,
        ),
        HomePropKind::FoodPile => (
            Sphere::new(0.4 + (food / 120.0).min(1.2)).into(),
            Color::srgb(0.85, 0.45, 0.2),
            -0.4,
        ),
    };
    commands.spawn((
        StageOwned,
        marker,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(pos.x, y, pos.z),
    ));
}

// ---------------------------------------------------------------------------
// The tribal panel (tool palette, selection, performance call)
// ---------------------------------------------------------------------------

#[derive(Component)]
pub struct TribalPanel;
#[derive(Component)]
pub struct PaletteButton(pub ToolKind);
#[derive(Component)]
pub struct PaletteIcon(pub ToolKind);
#[derive(Component)]
pub struct PaletteLabel;
#[derive(Component)]
pub struct SelectionLabel;
#[derive(Component)]
pub struct SongPanel;
#[derive(Component)]
pub struct SongLabel;
#[derive(Component)]
pub struct SongNote(pub u32);
#[derive(Component)]
pub struct DragBox;

/// Queries the panel update needs, bundled so the system stays readable.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PanelQueries<'w, 's> {
    buttons: Query<
        'w,
        's,
        (
            &'static PaletteButton,
            &'static mut BorderColor,
            &'static mut BackgroundColor,
        ),
        (Without<SongNote>, Without<DragBox>, Without<SongPanel>),
    >,
    texts: Query<
        'w,
        's,
        (
            &'static mut Text,
            Option<&'static PaletteLabel>,
            Option<&'static SelectionLabel>,
            Option<&'static SongLabel>,
        ),
    >,
    song: Query<'w, 's, &'static mut Node, (With<SongPanel>, Without<DragBox>, Without<SongNote>)>,
    notes: Query<
        'w,
        's,
        (
            &'static SongNote,
            &'static mut Node,
            &'static mut BackgroundColor,
        ),
        (Without<SongPanel>, Without<DragBox>, Without<PaletteButton>),
    >,
    drag: Query<'w, 's, &'static mut Node, (With<DragBox>, Without<SongPanel>, Without<SongNote>)>,
    /// The retail HUD is up: the song's own picture is its bar (`tribal_hud::social`)
    /// and the villagers' thought bubbles, so the legacy song panel stays away.
    hud: Option<Res<'w, crate::tribal_hud::TribalHudActive>>,
}

fn label(value: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.93, 0.85)),
    )
}

/// Build the panel: palette (bottom centre), selection line, song call (top
/// centre) and the drag box.
fn spawn_panel(commands: &mut Commands, icons: &tools::ToolIcons) {
    commands
        .spawn((
            StageOwned,
            TribalPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                bottom: Val::Px(185.0),
                margin: UiRect::left(Val::Px(-300.0)),
                width: Val::Px(600.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(3.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((label("", 12.0), SelectionLabel));
            p.spawn((label("", 12.0), PaletteLabel));
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|row| {
                for tool in &tools::TOOLS {
                    row.spawn((
                        Button::default(),
                        PaletteButton(tool.kind),
                        Node {
                            width: Val::Px(54.0),
                            height: Val::Px(54.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(8.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.10, 0.08, 0.05, 0.7)),
                        BorderColor::all(Color::srgba(0.6, 0.5, 0.3, 0.6)),
                    ))
                    .with_children(|b| {
                        match icons.get(tool.kind) {
                            Some(image) => {
                                b.spawn((
                                    PaletteIcon(tool.kind),
                                    ImageNode::new(image),
                                    Node {
                                        width: Val::Px(34.0),
                                        height: Val::Px(34.0),
                                        ..default()
                                    },
                                ));
                            }
                            None => {
                                let short = tool.name.split(' ').next().unwrap_or("");
                                b.spawn((PaletteIcon(tool.kind), label(short, 10.0)));
                            }
                        }
                        b.spawn(label(format!("{:.0}", tool.price), 9.0));
                    });
                }
            });
        });
    commands
        .spawn((
            StageOwned,
            SongPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Px(70.0),
                margin: UiRect::left(Val::Px(-230.0)),
                width: Val::Px(460.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(12.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.12, 0.07, 0.02, 0.82)),
            BorderColor::all(Color::srgba(0.95, 0.75, 0.3, 0.8)),
        ))
        .with_children(|p| {
            p.spawn((label("", 15.0), SongLabel));
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|row| {
                for i in 0..6 {
                    row.spawn((
                        SongNote(i),
                        Node {
                            width: Val::Px(62.0),
                            height: Val::Px(30.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.3, 0.3, 0.3, 0.6)),
                    ));
                }
            });
        });
    commands.spawn((
        StageOwned,
        DragBox,
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(Val::Px(1.5)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.4, 1.0, 0.4, 0.12)),
        BorderColor::all(Color::srgba(0.5, 1.0, 0.5, 0.9)),
    ));
}

#[allow(clippy::type_complexity)]
fn update_panel(
    run: &RunProgress,
    rts: &TribalRts,
    actors: &Query<
        (
            Entity,
            &Transform,
            Option<&TribeUnit>,
            Option<&Selected>,
            Option<&RivalMember>,
            Option<&WildAnimal>,
            Option<&Health>,
        ),
        (
            Or<(With<TribeUnit>, With<RivalMember>, With<WildAnimal>)>,
            Without<UnitRing>,
            Without<OverlayRoot>,
            Without<CommandPing>,
            Without<Ripple>,
        ),
    >,
    panel: &mut PanelQueries,
) {
    let campaign = &run.tribal_campaign;
    let resolved = campaign.resolved_count();
    // Palette states.
    for (button, mut border, mut background) in &mut panel.buttons {
        let built = campaign.has_shack(button.0);
        let unlocked = tools::is_unlocked(button.0, resolved);
        let highlighted = rts.palette == button.0;
        let edge = if highlighted {
            Color::srgb(1.0, 0.85, 0.2)
        } else if built {
            Color::srgba(0.4, 0.95, 0.4, 0.8)
        } else {
            Color::srgba(0.6, 0.5, 0.3, 0.6)
        };
        *border = BorderColor::all(edge);
        background.0 = if !unlocked {
            Color::srgba(0.05, 0.05, 0.05, 0.75)
        } else if built {
            Color::srgba(0.10, 0.22, 0.08, 0.78)
        } else {
            Color::srgba(0.16, 0.11, 0.05, 0.72)
        };
    }
    let info = tools::info(rts.palette);
    let palette_line = if campaign.has_shack(rts.palette) {
        if rts.palette == ToolKind::Campfire {
            format!("{} built - {}", info.name, info.effect)
        } else {
            format!(
                "{} - {} | B / click: hand to the selection",
                info.name, info.effect
            )
        }
    } else if tools::is_unlocked(rts.palette, resolved) {
        format!(
            "{} ({:.0} food) - {} | B / click: build the shack",
            info.name, info.price, info.effect
        )
    } else {
        format!(
            "{} - unlocks after settling {} village{}",
            info.name,
            info.tier,
            if info.tier == 1 { "" } else { "s" }
        )
    };
    // Selection summary.
    let mut chosen = 0;
    let mut chief = false;
    let mut carrying = 0.0;
    let mut energy_total = 0.0;
    let mut energy_count = 0usize;
    let mut hungry_count = 0usize;
    let mut all_energy_total = 0.0;
    let mut all_energy_count = 0usize;
    let mut all_hungry_count = 0usize;
    for (_, _, unit, selected, ..) in actors.iter() {
        let Some(unit) = unit else {
            continue;
        };
        carrying += unit.carrying;
        if !unit.chieftain {
            all_energy_total += unit.energy;
            all_energy_count += 1;
            all_hungry_count += usize::from(unit.energy <= ENERGY_RETURN_THRESHOLD);
            if selected.is_some() {
                energy_total += unit.energy;
                energy_count += 1;
                hungry_count += usize::from(unit.energy <= ENERGY_RETURN_THRESHOLD);
            }
        }
        if selected.is_some() {
            chosen += 1;
            chief |= unit.chieftain;
        }
    }
    let members = run.tribe.len();
    if energy_count == 0 {
        energy_total = all_energy_total;
        energy_count = all_energy_count;
        hungry_count = all_hungry_count;
    }
    let energy_summary = if energy_count == 0 {
        "n/a".to_string()
    } else {
        format!(
            "{:.0}%{}",
            energy_total / energy_count as f32,
            if hungry_count > 0 {
                format!(" ({} returning to eat)", hungry_count)
            } else {
                String::new()
            }
        )
    };
    let selection_line = format!(
        "Tribe {members}/{} | selected {}{} | carrying {:.0} | member energy {} | Ctrl+A all  Z gather  X fish  H hunt  V follow  T/B tools",
        campaign.population_cap(),
        chosen,
        if chief { " (incl. chief)" } else { "" },
        carrying,
        energy_summary
    );
    for (mut text, palette, selection, song) in &mut panel.texts {
        if palette.is_some() {
            if text.0 != palette_line {
                text.0 = palette_line.clone();
            }
        } else if selection.is_some() {
            if text.0 != selection_line {
                text.0 = selection_line.clone();
            }
        } else if song.is_some() {
            let line = match campaign.performance {
                Some(p) if p.gathering => format!("V{} song: the troupe is taking its places...", p.village + 1),
                Some(p) if p.bubble => format!(
                    "V{} wants {} x{} - press [{}]   score {:.0}/{:.0}   (beat in {:.1}s)",
                    p.village + 1,
                    tribal::ability_name(p.prompt),
                    p.required,
                    p.prompt,
                    p.score,
                    p.threshold,
                    p.time_left.max(0.0)
                ),
                Some(p) => format!(
                    "V{} song: score {:.0}/{:.0}   (beat in {:.1}s)",
                    p.village + 1,
                    p.score,
                    p.threshold,
                    p.time_left.max(0.0)
                ),
                None => String::new(),
            };
            if text.0 != line {
                text.0 = line;
            }
        }
    }
    // Song call.
    let performing = campaign.performance;
    let retail_hud = panel.hud.as_ref().is_some_and(|h| h.0);
    for mut node in &mut panel.song {
        let want = if performing.is_some() && !retail_hud {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    if let Some(p) = performing {
        for (note, mut node, mut bg) in &mut panel.notes {
            let visible = note.0 < p.notes;
            node.display = if visible {
                Display::Flex
            } else {
                Display::None
            };
            bg.0 = if note.0 < p.note {
                Color::srgba(0.85, 0.75, 0.35, 0.85)
            } else if note.0 == p.note {
                let pulse = (p.time_left * 6.0).sin() * 0.15 + 0.85;
                Color::srgba(1.0 * pulse, 0.55 * pulse, 0.15, 0.95)
            } else {
                Color::srgba(0.25, 0.22, 0.18, 0.7)
            };
        }
    }
    // Drag box.
    for mut node in &mut panel.drag {
        match rts.drag {
            Some((a, b)) => {
                let min = a.min(b);
                let size = (a - b).abs();
                node.display = Display::Flex;
                node.left = Val::Px(min.x);
                node.top = Val::Px(min.y);
                node.width = Val::Px(size.x);
                node.height = Val::Px(size.y);
            }
            None => {
                if node.display != Display::None {
                    node.display = Display::None;
                }
            }
        }
    }
}

/// Palette clicks: build the shack or hand its tool out.
#[allow(clippy::type_complexity)]
pub fn tribal_palette_clicks(
    mut rts: ResMut<TribalRts>,
    mut run: ResMut<RunProgress>,
    clicked: Query<&PaletteButton, Added<Pressed>>,
    mut units: Query<
        (
            Entity,
            &Transform,
            &mut TribeUnit,
            &mut UnitTask,
            Option<&Selected>,
            Option<&WorldId>,
        ),
        Without<RivalMember>,
    >,
) {
    for button in &clicked {
        rts.palette = button.0;
        let selected: Vec<Entity> = units
            .iter()
            .filter(|u| u.4.is_some())
            .map(|u| u.0)
            .collect();
        let acting = if selected.is_empty() {
            units
                .iter()
                .filter(|u| !u.2.chieftain)
                .map(|u| u.0)
                .collect()
        } else {
            selected
        };
        build_or_equip(button.0, &acting, &mut units, &mut run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meals_charge_only_available_stock_and_never_overfill_energy() {
        let mut food = MEAL_FOOD - 0.01;
        let mut energy = 0.0;
        assert!(!eat_from_stockpile(&mut food, &mut energy));
        assert_eq!(food, MEAL_FOOD - 0.01, "failed meals spend nothing");
        assert_eq!(energy, 0.0, "failed meals do not mint energy");

        // One food is one meal: it refills hunger (`eatHungerBoostPerFood` 100).
        assert_eq!(MEAL_FOOD, 1.0);
        food = 7.0;
        energy = 20.0;
        assert!(eat_from_stockpile(&mut food, &mut energy));
        assert_eq!(food, 6.0);
        assert_eq!(energy, tribal::MEMBER_ENERGY_MAX);
        assert!(!eat_from_stockpile(&mut food, &mut energy));
        assert_eq!(food, 6.0, "a full unit cannot consume another meal");
    }

    #[test]
    fn hungry_member_pauses_and_resumes_its_explicit_order() {
        let mut unit = TribeUnit::new(false, 5.0);
        let target = World::new().spawn_empty().id();
        let mut task = UnitTask::Gather(target);
        unit.energy = ENERGY_RETURN_THRESHOLD;
        unit.work = 1.25;
        assert!(unit.interrupt_for_food(&mut task, false));
        assert_eq!(task, UnitTask::Recover);
        assert_eq!(unit.resume_task, Some(UnitTask::Gather(target)));
        assert!(
            !unit.interrupt_for_food(&mut task, false),
            "recovery is not nested"
        );

        unit.energy = ENERGY_RESUME_THRESHOLD - 0.1;
        assert!(!unit.resume_after_meal(&mut task, Vec3::ZERO));
        unit.energy = ENERGY_RESUME_THRESHOLD;
        assert!(unit.resume_after_meal(&mut task, Vec3::ZERO));
        assert_eq!(task, UnitTask::Gather(target));
        assert_eq!(unit.resume_task, None);
        assert_eq!(
            unit.work, 1.25,
            "interrupted work progress survives a meal break"
        );
    }

    #[test]
    fn hungry_member_can_gather_from_empty_mat_then_eat_and_resume() {
        use std::time::Duration;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        let mut run = RunProgress::default();
        run.dna_this_stage = 0.0;
        app.insert_resource(run);
        app.insert_resource(TribalRts::default());
        app.add_systems(Update, tribal_units_think);

        let food_mat = tribal::food_pile_position();
        let bush = app
            .world_mut()
            .spawn((
                Transform::from_translation(food_mat),
                Bush { food_left: 80.0 },
            ))
            .id();
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(food_mat),
                TribeUnit::new(false, 5.0),
                UnitTask::Gather(bush),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        {
            let mut unit = app.world_mut().get_mut::<TribeUnit>(member).unwrap();
            unit.energy = 0.0;
        }

        // Stop on the first actual resume transition. Continuing after that
        // would let the unit finish its second bundle and legitimately idle on
        // the now-empty bush, obscuring whether the meal break worked.
        let mut resumed_snapshot = None;
        for _ in 0..200 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(250));
            app.update();
            let unit = app.world().get::<TribeUnit>(member).unwrap();
            let task = *app.world().get::<UnitTask>(member).unwrap();
            let active_food_order = matches!(
                task,
                UnitTask::Gather(source) | UnitTask::Deliver(After::Gather(source)) if source == bush
            );
            let stock = app.world().resource::<RunProgress>().dna_this_stage;
            if unit.energy >= ENERGY_RESUME_THRESHOLD && stock < 10.0 && active_food_order {
                resumed_snapshot = Some((unit.energy, stock, task));
                break;
            }
        }
        let (energy, stock, task) = resumed_snapshot.expect(
            "after eating gathered food, the exhausted member must resume the same bush order",
        );
        assert!(
            energy >= ENERGY_RESUME_THRESHOLD,
            "recovery threshold: {energy}"
        );
        assert!(
            (3.0..5.0).contains(&stock),
            "bundle delivered and meals consumed: {stock}"
        );
        assert!(
            matches!(task, UnitTask::Gather(source) | UnitTask::Deliver(After::Gather(source)) if source == bush)
        );
        assert!(app.world().get::<Bush>(bush).unwrap().food_left < 80.0);
    }

    /// A bare world running only the unit-behaviour system, with `stock` food
    /// on the mat.
    fn unit_app(stock: f32) -> App {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        let mut run = RunProgress::default();
        run.dna_this_stage = stock;
        app.insert_resource(run);
        app.insert_resource(TribalRts::default());
        app.add_systems(Update, tribal_units_think);
        app
    }

    fn advance(app: &mut App, seconds: f32, step: f32) {
        use std::time::Duration;
        for _ in 0..(seconds / step).round() as usize {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_secs_f32(step));
            app.update();
        }
    }

    #[test]
    fn member_hunger_is_held_for_two_minutes_then_drains_by_a_third_a_second() {
        let mut app = unit_app(0.0);
        let at = Vec3::new(5.0, 0.0, 5.0);
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                TribeUnit::new(false, 5.0),
                UnitTask::Idle(at),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        advance(&mut app, 119.0, 1.0);
        let energy = app.world().get::<TribeUnit>(member).unwrap().energy;
        assert!(energy > 99.99, "hunger is held at the maximum for 120 s: {energy}");
        advance(&mut app, 31.0, 1.0);
        let energy = app.world().get::<TribeUnit>(member).unwrap().energy;
        // 150 s in: about 30 s of draining at 0.333 a second.
        assert!(
            (energy - (100.0 - 31.0 * 0.333)).abs() < 1.0,
            "drained to {energy}"
        );
    }

    #[test]
    fn eating_a_meal_refills_hunger_and_heals_twenty_five() {
        let mut app = unit_app(5.0);
        let mat = tribal::food_pile_position();
        let mut unit = TribeUnit::new(false, 5.0);
        unit.energy = 10.0;
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(mat),
                unit,
                UnitTask::Recover,
                WorldId(tribal::MATE_ID_BASE),
                Health { hp: 50.0, max: 100.0 },
            ))
            .id();
        advance(&mut app, 4.0, 0.25);
        let unit = app.world().get::<TribeUnit>(member).unwrap();
        assert!(unit.energy > 99.0, "one food refills hunger: {}", unit.energy);
        let health = app.world().get::<Health>(member).unwrap();
        assert!((health.hp - 75.0).abs() < 1e-3, "and heals 25: {}", health.hp);
        let stock = app.world().resource::<RunProgress>().dna_this_stage;
        assert_eq!(stock, 4.0, "a meal costs one food");
    }

    #[test]
    fn bundles_hold_five_items_and_a_fish_is_worth_one_and_a_half() {
        assert_eq!(FoodKind::Fruit.capacity(), 5.0);
        assert_eq!(FoodKind::Meat.capacity(), 5.0);
        assert_eq!(FoodKind::Fish.capacity(), 7.5);
        assert!(FoodKind::Fish.capacity() <= tribal::CARRY_MAX);
        // A gatherer fills one bundle of fruit and then walks it to the mat.
        let mut app = unit_app(0.0);
        let at = tribal::food_pile_position() + Vec3::new(3.0, 0.0, 0.0);
        let bush = app
            .world_mut()
            .spawn((Transform::from_translation(at), Bush { food_left: 10.0 }))
            .id();
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                TribeUnit::new(false, 5.0),
                UnitTask::Gather(bush),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        // Watch the first trip: the arms fill to one bundle, never more, and
        // that bundle is what reaches the mat.
        let mut most: f32 = 0.0;
        for _ in 0..140 {
            advance(&mut app, 0.1, 0.1);
            most = most.max(app.world().get::<TribeUnit>(member).unwrap().carrying);
            if app.world().resource::<RunProgress>().dna_this_stage > 0.0 {
                break;
            }
        }
        assert!((most - 5.0).abs() < 0.01, "one bundle is five fruit: {most}");
        let stock = app.world().resource::<RunProgress>().dna_this_stage;
        assert!((stock - 5.0).abs() < 0.01, "the bundle reaches the mat: {stock}");
    }

    #[test]
    fn a_gathering_song_begins_when_every_member_stands_at_its_spot() {
        // The performance states 0..4 (0xDA2F30): the song waits for every member
        // to stand at the spot the layout gave it, then the music starts; once the
        // song and its ending are over the members idle where they stand.
        let mut app = unit_app(0.0);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.ensure_ready();
            run.tribal_campaign.villages[0].relations = 0.0;
            run.tribal_campaign.start_performance_gathering(0, crate::tribal::Troupe::FULL).unwrap();
        }
        let centre = tribal::village_position(0);
        let spots = [centre + Vec3::new(-10.0, 0.0, 0.0), centre + Vec3::new(0.0, 0.0, -14.0)];
        let mut members = Vec::new();
        for (i, spot) in spots.iter().enumerate() {
            let member = app
                .world_mut()
                .spawn((
                    Transform::from_translation(*spot + Vec3::new(-30.0, 0.0, 6.0 * i as f32)),
                    TribeUnit::new(false, 5.0),
                    UnitTask::Perform(0),
                    WorldId(tribal::MATE_ID_BASE + i as u32),
                    Health::new(100.0),
                ))
                .id();
            app.world_mut()
                .resource_mut::<RunProgress>()
                .tribal_campaign
                .song_stands
                .push((member.to_bits(), *spot));
            members.push(member);
        }
        let step = |app: &mut App, seconds: f32| {
            for _ in 0..(seconds / 0.1).round() as usize {
                advance(app, 0.1, 0.1);
                app.world_mut().resource_mut::<RunProgress>().tribal_campaign.tick(0.1);
            }
        };
        step(&mut app, 2.0);
        let p = app.world().resource::<RunProgress>().tribal_campaign.performance.unwrap();
        assert!(p.gathering && p.note == 0, "the members are still walking: {p:?}");
        assert!(!app.world().get::<TribeUnit>(members[0]).unwrap().busy);
        // They walk about 30 units at 5 a second.
        step(&mut app, 8.0);
        let p = app.world().resource::<RunProgress>().tribal_campaign.performance.unwrap();
        assert!(!p.gathering, "everybody is in place, the music starts");
        for (member, spot) in members.iter().zip(spots) {
            let at = app.world().get::<Transform>(*member).unwrap().translation;
            assert!(dist_xz(at, spot) < 0.7, "{at:?} vs {spot:?}");
            assert!(app.world().get::<TribeUnit>(*member).unwrap().busy);
        }
        // The song and its ending over: the members idle where they stand.
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.performance = None;
            run.tribal_campaign.social_end = None;
        }
        step(&mut app, 0.2);
        for member in members {
            assert!(matches!(*app.world().get::<UnitTask>(member).unwrap(), UnitTask::Idle(_)));
        }
        assert!(app.world().resource::<RunProgress>().tribal_campaign.song_stands.is_empty());
    }

    #[test]
    fn a_gatherer_by_hand_brings_one_fruit_per_pick_clip() {
        // 0xD75F70 states 3..6: `gathr_picktree` (49 frames = 1.633 s) per
        // fruit worth `gatherFoodPerFruit` 1.0, credited when the clip ends.
        let mut app = unit_app(0.0);
        // No roster, so no role bonus on the clock.
        app.world_mut().resource_mut::<RunProgress>().tribe.clear();
        let at = tribal::food_pile_position() + Vec3::new(3.0, 0.0, 0.0);
        let bush = app
            .world_mut()
            .spawn((Transform::from_translation(at), Bush { food_left: 10.0 }))
            .id();
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                TribeUnit::new(false, 5.0),
                UnitTask::Gather(bush),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        advance(&mut app, 1.5, 0.05);
        assert_eq!(app.world().get::<TribeUnit>(member).unwrap().carrying, 0.0, "the first clip is still playing");
        advance(&mut app, 0.25, 0.05);
        assert_eq!(app.world().get::<TribeUnit>(member).unwrap().carrying, 1.0, "one pick, one fruit");
        advance(&mut app, 1.7, 0.05);
        assert_eq!(app.world().get::<TribeUnit>(member).unwrap().carrying, 2.0);
        assert!((app.world().get::<Bush>(bush).unwrap().food_left - 8.0).abs() < 1e-4);
    }

    #[test]
    fn a_fisher_lands_one_fish_per_cycle_of_the_installed_clips() {
        // 0xD73FD0: start, a loop of 0.5..1.0 s, the 4.03 s pull, `fish_succ_N`,
        // then `fish_dump` puts the fish (1.5 food) on the bundle.
        let mut app = unit_app(0.0);
        app.world_mut().resource_mut::<RunProgress>().tribe.clear();
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .tools
            .insert(0, ToolKind::FishingSpears);
        let spot = tribal::food_pile_position() + Vec3::new(-3.0, 0.0, 0.0);
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(spot),
                TribeUnit::new(false, 5.0),
                UnitTask::Fish(spot),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        let mut credited = None;
        let mut t = 0.0;
        while t < 20.0 {
            advance(&mut app, 0.05, 0.05);
            t += 0.05;
            if app.world().get::<TribeUnit>(member).unwrap().carrying > 0.0 {
                credited = Some(t);
                break;
            }
        }
        let credited = credited.expect("a catch within one cycle");
        let unit = app.world().get::<TribeUnit>(member).unwrap();
        assert_eq!(unit.carrying, crate::tribal_rules::FISHING_FOOD_PER_FISH);
        assert!((credited - unit.fish.credit_at()).abs() < 0.12, "{credited} vs {}", unit.fish.credit_at());
        // The next catch comes one whole cycle later (the dump clip finishes first).
        let end = unit.fish.end();
        advance(&mut app, end - credited + 0.2, 0.05);
        assert!(app.world().get::<TribeUnit>(member).unwrap().work < 1.0, "a new cycle started");
    }

    #[test]
    fn formation_spreads_a_group() {
        let offsets: Vec<Vec2> = (0..12).map(formation_offset).collect();
        for (i, a) in offsets.iter().enumerate() {
            for b in &offsets[i + 1..] {
                assert!(a.distance(*b) > 0.5, "{a:?} and {b:?} overlap");
            }
        }
    }

    #[test]
    fn box_selection_accepts_corners_in_any_order() {
        assert!(in_rect(Vec2::new(5.0, 5.0), Vec2::ZERO, Vec2::splat(10.0)));
        assert!(in_rect(Vec2::new(5.0, 5.0), Vec2::splat(10.0), Vec2::ZERO));
        assert!(!in_rect(
            Vec2::new(15.0, 5.0),
            Vec2::ZERO,
            Vec2::splat(10.0)
        ));
    }

    #[test]
    fn clicks_resolve_to_spore_style_orders() {
        let mut ids = World::new();
        let bush = ids.spawn_empty().id();
        let foe = ids.spawn_empty().id();
        let world = ClickWorld {
            bushes: vec![(bush, Vec3::new(10.0, 0.0, 0.0), 50.0)],
            meat: vec![],
            foes: vec![(foe, Vec3::new(-10.0, 0.0, 0.0))],
            grazers: vec![(foe, Vec3::new(-10.0, 0.0, 0.0))],
            shacks: vec![(ToolKind::WoodenHorns, tools::shack_position(0))],
            hostile_villages: vec![(2, tribal::village_position(2))],
            spots: vec![Vec3::new(0.0, 0.0, 30.0)],
            ..ClickWorld::default()
        };
        assert_eq!(
            resolve_click(Vec3::new(10.5, 0.0, 0.3), &world, false),
            Order::Gather(bush)
        );
        assert_eq!(
            resolve_click(Vec3::new(-10.0, 0.0, 0.5), &world, false),
            Order::Hunt(foe)
        );
        assert_eq!(
            resolve_click(Vec3::new(-10.0, 0.0, 0.5), &world, true),
            Order::Domesticate(foe)
        );
        let village = tribal::village_position(2);
        assert_eq!(resolve_click(village, &world, false), Order::Siege(2));
        assert_eq!(resolve_click(village, &world, true), Order::Perform(2));
        assert_eq!(
            resolve_click(tools::shack_position(0), &world, false),
            Order::Fetch(ToolKind::WoodenHorns)
        );
        assert_eq!(
            resolve_click(tribal::food_pile_position(), &world, false),
            Order::Home
        );
        assert_eq!(
            resolve_click(Vec3::new(0.0, 0.0, 29.0), &world, false),
            Order::Fish(Vec3::new(0.0, 0.0, 30.0))
        );
        let ground = Vec3::new(5.0, 0.0, -12.0);
        assert_eq!(resolve_click(ground, &world, false), Order::Move(ground));
    }

    #[test]
    fn bushes_are_spread_clear_of_villages_and_home() {
        let bushes = bush_positions(1234, 14);
        assert_eq!(bushes.len(), 14);
        for b in &bushes {
            assert!(dist_xz(*b, tribal::home_position()) > 10.0);
            for v in 0..tribal::RIVAL_VILLAGES {
                assert!(dist_xz(*b, tribal::village_position(v)) > 7.0);
            }
        }
    }

    #[test]
    fn herds_live_on_their_rings_clear_of_villages_and_the_epics_of_fruit() {
        for seed in [0u64, 7, 1234, 0xDEAD_BEEF] {
            let homes = herd_homes(seed);
            let bushes = bush_positions(seed, 14);
            assert_eq!(homes.len(), crate::tribal_rules::TRIBAL_HERDS.len());
            for (i, (home, herd)) in homes
                .iter()
                .zip(crate::tribal_rules::TRIBAL_HERDS.iter())
                .enumerate()
            {
                assert!(dist_xz(*home, tribal::home_position()) > 7.0, "seed {seed} herd {i}");
                for v in 0..tribal::RIVAL_VILLAGES {
                    assert!(
                        dist_xz(*home, tribal::village_position(v)) > 9.0,
                        "seed {seed} herd {i} sits on V{}",
                        v + 1
                    );
                }
                if herd.epic {
                    assert!(
                        bushes.iter().all(|b| dist_xz(*home, *b) > 6.0),
                        "seed {seed}: epic {i} guards a fruit bush"
                    );
                }
                if herd.predator {
                    assert!(
                        dist_xz(*home, tribal::home_position()) > 18.0,
                        "seed {seed}: predators {i} next to the camp"
                    );
                    for v in 0..tribal::RIVAL_VILLAGES {
                        assert!(
                            dist_xz(*home, tribal::village_position(v)) > 12.0,
                            "seed {seed}: predators {i} prowl at V{}",
                            v + 1
                        );
                    }
                }
                for other in homes.iter().take(i) {
                    assert!(dist_xz(*home, *other) > 6.0, "seed {seed} herds overlap");
                }
            }
            // Members stand around their herd's home: guards on the ring of
            // posts (10..15), loungers by their group's spot inside it.
            for member in 0..crate::tribal_rules::TRIBAL_HERDS[0].creatures {
                let spot = herd_spot(seed, homes[0], 0, member);
                assert!(dist_xz(spot, homes[0]) < 16.5, "seed {seed} member {member}");
            }
        }
    }

    #[test]
    fn home_props_follow_the_campaign() {
        use crate::tribal_village::{HomeLook, HutDiet};
        let look = HomeLook::UNDAMAGED;
        // The player's hut is its diet's style at the tribe level (carnivore:
        // timber), the totem and the food mat follow the level.
        assert_eq!(home_prop_name(HomePropKind::Hut, 0, 0.0, 3, 6, &look), "he_timber_1");
        assert_eq!(home_prop_name(HomePropKind::Hut, 5, 0.0, 3, 6, &look), "he_timber_3");
        let herbivore = HomeLook { diet: HutDiet::Herbivore, health: 0.2 };
        assert_eq!(home_prop_name(HomePropKind::Hut, 1, 0.0, 3, 6, &herbivore), "he_yurt_2_dmghi");
        assert_eq!(home_prop_name(HomePropKind::Totem, 3, 0.0, 3, 6, &look), "trg_totem_pole3");
        assert_eq!(home_prop_name(HomePropKind::FoodMat, 1, 0.0, 3, 6, &look), "trg_totem_foodmat2");
        assert_eq!(home_prop_name(HomePropKind::FoodPile, 0, 0.0, 3, 6, &look), "empty");
        assert_eq!(home_prop_name(HomePropKind::FoodPile, 0, 400.0, 3, 6, &look), "foodpile_fruit_06");
        assert_eq!(home_prop_name(HomePropKind::EggPen, 0, 100.0, 6, 6, &look), "tt_eggpen_none");
        assert_eq!(home_prop_name(HomePropKind::EggPen, 0, 100.0, 3, 6, &look), "tt_eggpen_full");
    }

    #[test]
    fn clicks_on_a_rival_food_mat_raid_it() {
        let mat = tribal::village_food_mat(2);
        let world = ClickWorld {
            hostile_villages: vec![(2, tribal::village_position(2))],
            food_mats: vec![(2, mat)],
            ..ClickWorld::default()
        };
        assert_eq!(resolve_click(mat, &world, false), Order::Raid(2));
        assert_eq!(resolve_click(mat, &world, true), Order::Perform(2));
        // The rest of the village is still a siege.
        let village = tribal::village_position(2);
        assert_eq!(resolve_click(village, &world, false), Order::Siege(2));
    }

    #[test]
    fn raiders_steal_a_bundle_each_with_one_raid_event_and_carry_it_home() {
        let mut app = unit_app(20.0);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.ensure_ready();
            run.tribal_campaign.villages[1].relations = 0.0;
        }
        let mat = tribal::village_food_mat(1);
        let mut raiders = Vec::new();
        for i in 0..2 {
            let at = mat + Vec3::new(i as f32 * 0.5, 0.0, 0.0);
            raiders.push(
                app.world_mut()
                    .spawn((
                        Transform::from_translation(at),
                        TribeUnit::new(false, 6.0),
                        UnitTask::Raid(1),
                        WorldId(tribal::MATE_ID_BASE + i),
                        Health::new(100.0),
                    ))
                    .id(),
            );
        }
        let initial = app.world().resource::<RunProgress>().tribal_campaign.tribe_of(1).initial_food;
        advance(&mut app, 0.2, 0.1);
        for raider in &raiders {
            let unit = app.world().get::<TribeUnit>(*raider).unwrap();
            assert_eq!(unit.carrying, crate::tribal_rules::RAID_LOAD);
            assert_eq!(
                *app.world().get::<UnitTask>(*raider).unwrap(),
                UnitTask::Deliver(After::Idle)
            );
        }
        let campaign = &app.world().resource::<RunProgress>().tribal_campaign;
        assert_eq!(campaign.village_food(1), initial - 2.0 * crate::tribal_rules::RAID_LOAD);
        // One raid, one TribeRaid event (-30), and the tribe is alerted.
        assert_eq!(campaign.village(1).unwrap().relations, -30.0);
        assert!(campaign.raid_alerted(1));
        // They walk home and put the loot on the mat.
        advance(&mut app, 90.0, 0.1);
        let stock = app.world().resource::<RunProgress>().dna_this_stage;
        assert!((stock - 40.0).abs() < 0.01, "stock {stock}");
        for raider in &raiders {
            assert_eq!(app.world().get::<TribeUnit>(*raider).unwrap().carrying, 0.0);
        }
        // The raided stock grows back with FoodRate.
        let food = app.world().resource::<RunProgress>().tribal_campaign.village_food(1);
        assert!(food <= initial - 2.0 * crate::tribal_rules::RAID_LOAD + 0.01);
    }

    #[test]
    fn a_member_carries_the_gift_from_the_food_mat_to_the_other_tribes_mat() {
        let mut app = unit_app(25.0);
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribal_campaign.ensure_ready();
            run.tribal_campaign.villages[1].relations = -50.0;
        }
        let carrier = app
            .world_mut()
            .spawn((
                Transform::from_translation(tribal::food_pile_position()),
                TribeUnit::new(false, 6.0),
                UnitTask::Gift(1),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        // At the mat the bundle is picked up and paid from the stock; nothing
        // has happened to the other tribe yet.
        advance(&mut app, 0.2, 0.1);
        assert_eq!(app.world().get::<TribeUnit>(carrier).unwrap().carrying, tribal::GIFT_FOOD);
        assert_eq!(*app.world().get::<UnitTask>(carrier).unwrap(), UnitTask::Gift(1));
        let run = app.world().resource::<RunProgress>();
        assert!((run.dna_this_stage - 15.0).abs() < 0.01, "stock {}", run.dna_this_stage);
        assert_eq!(run.tribal_campaign.village(1).unwrap().relations, -50.0);
        assert_eq!(run.tribal_campaign.village(1).unwrap().gift_timer, 0.0);
        // It walks the bundle over; putting it down is the gift: an Annoyed tribe
        // goes to Cautious (0) and the fading timer starts.
        advance(&mut app, 150.0, 0.1);
        let unit = app.world().get::<TribeUnit>(carrier).unwrap();
        assert_eq!(unit.carrying, 0.0);
        assert!(matches!(*app.world().get::<UnitTask>(carrier).unwrap(), UnitTask::Idle(_)));
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.tribal_campaign.village(1).unwrap().relations, 0.0);
        assert!(run.tribal_campaign.village(1).unwrap().gift_timer > 0.0);
        assert!((run.dna_this_stage - 15.0).abs() < 0.01, "the food stays paid");
    }

    #[test]
    fn a_gift_cannot_be_picked_up_without_the_food() {
        let mut app = unit_app(4.0);
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.ensure_ready();
        let carrier = app
            .world_mut()
            .spawn((
                Transform::from_translation(tribal::food_pile_position()),
                TribeUnit::new(false, 6.0),
                UnitTask::Gift(1),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        advance(&mut app, 0.5, 0.1);
        assert_eq!(app.world().get::<TribeUnit>(carrier).unwrap().carrying, 0.0);
        assert!(matches!(*app.world().get::<UnitTask>(carrier).unwrap(), UnitTask::Idle(_)));
        let run = app.world().resource::<RunProgress>();
        assert!((run.dna_this_stage - 4.0).abs() < 0.01);
    }

    #[test]
    fn a_hurt_member_fights_back_then_resumes_its_work() {
        let mut ids = World::new();
        let bush = ids.spawn_empty().id();
        let foe = ids.spawn_empty().id();
        let at = Vec3::new(3.0, 0.0, 4.0);
        let mut unit = TribeUnit::new(false, 5.0);
        let mut task = UnitTask::Gather(bush);
        assert!(unit.retaliate_against(&mut task, foe, at));
        assert_eq!(task, UnitTask::Hunt(foe));
        // A second blow keeps the first remembered order.
        assert!(!unit.retaliate_against(&mut task, foe, at));
        assert_eq!(unit.end_retaliation(UnitTask::Idle(at)), UnitTask::Gather(bush));
        assert_eq!(unit.end_retaliation(UnitTask::Idle(at)), UnitTask::Idle(at));
        // Explicit fights, moves and performances are not interrupted.
        for mut busy in [UnitTask::Siege(1), UnitTask::MoveTo(at), UnitTask::Perform(2)] {
            let before = busy;
            assert!(!unit.retaliate_against(&mut busy, foe, at));
            assert_eq!(busy, before);
        }

        // In the world: the attacker runs past the leash, the member goes back
        // to gathering.
        let mut app = unit_app(0.0);
        let bush = app
            .world_mut()
            .spawn((Transform::from_translation(at + Vec3::X * 2.0), Bush { food_left: 50.0 }))
            .id();
        let foe = app
            .world_mut()
            .spawn((
                Transform::from_translation(at + Vec3::X * 30.0),
                WildAnimal {
                    hostile: true,
                    speed: 5.0,
                    home: at,
                    target: at,
                    wait: 0.0,
                    flee: 0.0,
                    chase: None,
                },
            ))
            .id();
        let mut unit = TribeUnit::new(false, 5.0);
        unit.retaliate = Some((UnitTask::Gather(bush), at));
        let member = app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                unit,
                UnitTask::Hunt(foe),
                WorldId(tribal::MATE_ID_BASE),
                Health::new(100.0),
            ))
            .id();
        advance(&mut app, 0.1, 0.1);
        assert!(matches!(
            *app.world().get::<UnitTask>(member).unwrap(),
            UnitTask::Gather(b) | UnitTask::Deliver(After::Gather(b)) if b == bush
        ));
        assert!(app.world().get::<TribeUnit>(member).unwrap().retaliate.is_none());
    }
}
