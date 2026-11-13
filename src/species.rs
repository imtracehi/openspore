//! Creature-stage species ecology: distinct original-data NPC species, their
//! nests, and persistent members with encounter outcomes.
//!
//! The original Creature stage populates the world with *species*, not copies
//! of the player. This module builds that ecology from the installed part
//! catalog: every species gets its own body profile, its own part build and
//! its own nests, so the player meets genuinely different creatures, and the
//! world survives save/load without resetting anything.
//!
//! Slot conventions follow the rest of the tree (see `body.rs`, `game.rs`):
//! parts mount on `BodyProfile::slot(bone)`; bone 0 is the mouth anchor, 1/2
//! the mirrored eye slots, 3/4 the mirrored foot slots, 5/6 dorsal details and
//! 7 the rear detail. Mirrored copies share a nonzero `pair_id` and set
//! `mirrored`, exactly like the editor's `game::part_add_mirrored`.
//!
//! Species builds never charge the player's editor DNA — they are picked
//! straight from the catalog, bounded by an unlock tier, and are a pure
//! function of the world seed. The same pick discipline is used for the
//! player's first parts in `editor.rs` (non-null creature parts, mouth chosen
//! by authored diet ranks), so species and player read from the same data.
//!
//! This module is deliberately simulation-only: Bevy is used only for the
//! `Resource` derive and every decision is a hash of the seed, so the whole
//! module is testable headlessly with no RNG state and no Bevy world.
//! `creature.rs`/`runtime.rs` map the data onto entities; see
//! `OPENCODE_PROGRESS.md` for the exact wiring.
//!
//! Retail population (creature-eco, notes C4): a fresh world is generated from
//! the original's `CreatureGameLevelGenerationList` (`creature_ecology`): one
//! species per placed herd archetype (about 22-26 herds of 4-10 creatures,
//! placed by distance band from the home nest), members with alpha / baby /
//! guard roles, and the archetype's own relationship thresholds (best friends
//! after 3, extinct after 3-7 kills). Species without an archetype (worlds
//! saved before this change, and `generate_n`) keep the earlier rules.

// The integration surface (queries and counts the stage systems will call) is
// intentionally ahead of the wiring, which Codex owns in `creature.rs`; the
// dead-code warnings clear as soon as the first call site lands. All logic
// here is covered by the tests below, so nothing real is unused.
#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::asset_catalog::{Capabilities, PartCatalog, PartDefinition, CREATURE_PARTS};
use crate::body::BodyProfile;
use crate::creature_ecology::{self as ecology, Archetype, MemberRole};
use crate::game::PartInstance;

/// Persistent member ids start here and share the `persistence::WorldId`
/// band the creature stage has always used for its members (300..306 for the
/// original six), so a member id doubles as a world id and stays stable
/// across save/load. Buses (100..), tribe (200..) and space items (1..n)
/// still keep their own bands.
pub const MEMBER_ID_BASE: u32 = 300;
/// Species a fresh world asks for: three or four, chosen by the seed.
pub const MIN_DEFAULT_SPECIES: usize = 3;
pub const MAX_DEFAULT_SPECIES: usize = 4;
/// Species generated for `generate` when the caller does not ask for a
/// specific count (the brief requires at least three distinct species).
pub const DEFAULT_SPECIES: usize = MAX_DEFAULT_SPECIES;
pub const MAX_SPECIES: usize = 8;
pub const MIN_SPECIES: usize = 1;
pub const MIN_MEMBERS_PER_NEST: usize = 2;
pub const MAX_MEMBERS_PER_NEST: usize = 4;
pub const MAX_TOTAL_MEMBERS: usize = 24;
pub const MAX_PARTS_PER_SPECIES: usize = 12;
/// How deep an unlock level a species may draw from (bounded rank picks: the
/// original tiers a fresh world is expected to show).
pub const MAX_UNLOCK_TIER: u32 = 3;
/// A species counts as allied to the player once this many of its members
/// are befriended.
// Three distinct encounters make alliance an ecology objective. This count
// is currently OpenSpore tuning; no native threshold comparison has passed.
pub const DEFAULT_ALLIANCE_THRESHOLD: u32 = 3;
/// Extinction threshold value meaning "only when every member is defeated".
pub const EXTINCT_WHEN_ALL: u32 = 0;
/// Radius nests are scattered within for a new world.
pub const DEFAULT_NEST_RADIUS: f32 = 24.0;
/// Per-species body scale multiplier, applied by the spawner like
/// `game::CreatureStats::size`.
pub const MIN_SPECIES_SIZE: f32 = 0.75;
pub const MAX_SPECIES_SIZE: f32 = 1.25;

/// What a species eats. Derived from the authored ranks of its own mouth, so
/// a herbivore mouth always yields a herbivore species and a carnivore mouth
/// a carnivore species (the original couples mouths to diets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Diet {
    #[default]
    Herbivore,
    Omnivore,
    Carnivore,
}

impl Diet {
    pub fn label(self) -> &'static str {
        match self {
            Diet::Herbivore => "Herbivore",
            Diet::Omnivore => "Omnivore",
            Diet::Carnivore => "Carnivore",
        }
    }

    /// Pure: diet from a mouth's authored herbivore/carnivore ranks. Both
    /// ranks nonzero means the original mouth feeds either way.
    pub fn from_mouth(herbivore: u8, carnivore: u8) -> Self {
        match (herbivore > 0, carnivore > 0) {
            (true, true) => Diet::Omnivore,
            (true, false) => Diet::Herbivore,
            (false, true) => Diet::Carnivore,
            (false, false) => Diet::Omnivore,
        }
    }
}

/// Outcome of an encounter with one persistent member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Relationship {
    /// Untouched (still wild).
    #[default]
    Neutral,
    /// Befriended: it joins the pack and counts toward alliance.
    Allied,
    /// Defeated: dead. Absorbing — a dead member never changes state again.
    Defeated,
}

impl Relationship {
    pub fn is_alive(self) -> bool {
        self != Relationship::Defeated
    }
}

/// What happened to a species, as the relationship manager's event handler
/// 0xBA4A00 sees it (its jump table 0xBA4D94: 0 a member killed, 3 a member
/// befriended, 1 a member attacked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationEvent {
    Kill,
    Befriend,
    Attack,
}

/// One species' relationship to the avatar (`tRelationshipData`, the record
/// 0xBA3B40 returns): `relationship` +0 (`creature_rules::relationship`),
/// `times_friendly` +4 (how often it became friendly), `impressions` +8 (members
/// befriended), `kills` +0xC (members killed since the last reset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeciesRelation {
    pub species: u32,
    pub relationship: u8,
    pub times_friendly: u32,
    pub impressions: u32,
    pub kills: u32,
}

impl SpeciesRelation {
    /// The event handler 0xBA4A00 on this record. `thresholds` are the herd's
    /// `NumToImpressForCurious / Friendly / BestFriends` and
    /// `numToKillForFearfulRelationship`. VERIFIED: a befriend (0xBA4B10) clears
    /// the kill count (0xBA4480) and counts an impression (0xBA43E0: best friends
    /// / friendly / curious at their thresholds, else unchanged); a kill (0xBA4BB5)
    /// counts one and from `numToKillForFearful` makes the species fearful, hostile
    /// when it already was or became friendly more than once (0xBA43C0 > 1); an
    /// attack (0xBA4B89) makes it hated, hostile when friendly more than once, and
    /// leaves a hostile one hostile. A change of relationship to curious clears the
    /// kill count and to friendly counts a friendly time (0xBA4D03).
    pub fn apply(&mut self, event: RelationEvent, thresholds: [u32; 4]) {
        use crate::creature_rules::relationship::*;
        let [curious, friendly, best, fearful] = thresholds;
        let before = self.relationship;
        let mut after = before;
        match event {
            RelationEvent::Befriend => {
                self.kills = 0;
                self.impressions += 1;
                after = if self.impressions >= best.max(1) {
                    BEST_FRIENDS
                } else if self.impressions >= friendly.max(1) {
                    FRIENDLY
                } else if self.impressions >= curious {
                    CURIOUS
                } else {
                    before
                };
            }
            RelationEvent::Kill => {
                self.kills += 1;
                if fearful > 0 && self.kills >= fearful {
                    after = if self.times_friendly > 1 || before == HOSTILE { HOSTILE } else { FEARFUL };
                }
            }
            RelationEvent::Attack => {
                after = if self.times_friendly > 1 {
                    HOSTILE
                } else if before == HOSTILE {
                    HOSTILE
                } else {
                    WARY
                };
            }
        }
        if after != before {
            if after == CURIOUS {
                self.kills = 0;
            } else if after == FRIENDLY {
                self.times_friendly += 1;
            }
            self.relationship = after;
        }
    }
}

/// Counts of each relationship, for world- or species-level summaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelationshipCounts {
    pub neutral: u32,
    pub allied: u32,
    pub defeated: u32,
}

impl RelationshipCounts {
    pub fn total(&self) -> u32 {
        self.neutral + self.allied + self.defeated
    }
    /// Alive (not defeated) members.
    pub fn alive(&self) -> u32 {
        self.neutral + self.allied
    }
    pub fn count(&self, relationship: Relationship) -> u32 {
        match relationship {
            Relationship::Neutral => self.neutral,
            Relationship::Allied => self.allied,
            Relationship::Defeated => self.defeated,
        }
    }
    pub fn add(&mut self, relationship: Relationship) {
        match relationship {
            Relationship::Neutral => self.neutral += 1,
            Relationship::Allied => self.allied += 1,
            Relationship::Defeated => self.defeated += 1,
        }
    }
    pub fn of(members: impl Iterator<Item = Relationship>) -> Self {
        let mut out = Self::default();
        for m in members {
            out.add(m);
        }
        out
    }
}

/// Result of recording an encounter against a persistent member id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordResult {
    /// First time this member reached this outcome; the world changed.
    Recorded,
    /// The member already had this outcome (or is dead): ignored.
    Already,
    /// No member with that id exists in this world.
    UnknownMember,
}

/// One persistent individual of a species. `id` is the stable identity used
/// for recording outcomes and doubles as a `persistence::WorldId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub id: u32,
    pub relationship: Relationship,
    /// Alpha, baby or adult (retail herd roles; older saves load as adults).
    #[serde(default)]
    pub role: MemberRole,
    /// Stays near the nest (`percentGuards`).
    #[serde(default)]
    pub guard: bool,
    /// Seconds lived as a baby while the herd was active; it grows up after
    /// `creatureNPCMinBabyTime` (creature_support). Older saves load as 0.
    #[serde(default)]
    pub baby_age: f32,
}

impl Member {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            relationship: Relationship::Neutral,
            role: MemberRole::Adult,
            guard: false,
            baby_age: 0.0,
        }
    }
}

/// A species' home: position plus the members living there. `members` is the
/// live population, so it shrinks as members are defeated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nest {
    /// Owning species id.
    pub species: u32,
    pub position: [f32; 3],
    pub members: Vec<Member>,
    /// The nest's one-time completion reward (DNA and a carried part) has
    /// been paid. Saves without the field start unclaimed.
    #[serde(default)]
    pub rewarded: bool,
    /// The herd's full size (`NumCreatures`); a lost member is replaced after
    /// the archetype's `RespawnRate` until the species is extinct. 0 (older
    /// saves, legacy worlds): never replaced.
    #[serde(default)]
    pub herd_size: u32,
    /// Seconds the nest has been short of members.
    #[serde(default)]
    pub respawn_timer: f32,
}

/// What the player has made of a whole nest. Spore completes a nest when
/// every member is befriended (it becomes a friendly nest) or every member
/// is killed (the nest is destroyed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestStatus {
    /// At least one member is still wild (or the species was attacked).
    Wild,
    /// Every member is befriended and the species is not angry.
    Allied,
    /// Every member is dead.
    Destroyed,
}

/// Persistent id band for roaming epic and rogue creatures. It sits above
/// the species member band (300..) and the creature tests' member window.
pub const ROAMER_ID_BASE: u32 = 1300;
/// Giant aggressive creatures per fresh world. ADAPTED (OpenSpore 0.80
/// counts): the generation list's epic / miniboss placement could not be
/// traced (see `creature_ecology::EXTRA_SETS`), so every world keeps them.
pub const EPIC_COUNT: usize = 2;
/// Solitary aggressive creatures that roam without a nest (retail
/// `npc_miniboss`, shown as "Rogue").
pub const ROGUE_COUNT: usize = 1;
/// Actor scales from the archetypes' `OverrideSize` (epic 20, miniboss 5.7;
/// `creature_ecology::species_scale`).
pub const EPIC_SCALE: f32 = ecology::NPC_EPIC_PREDATOR.size * ecology::SIZE_TO_SCALE;
pub const ROGUE_SCALE: f32 = 5.7 * ecology::SIZE_TO_SCALE;
/// Candidate lairs, all on the far side of the island from the default home
/// nest (0, 8), so the hatching ground stays a safe start.
const LAIR_CANDIDATES: [[f32; 2]; 6] = [
    [-19.0, -19.0],
    [19.0, -19.0],
    [0.0, -20.0],
    [-20.0, -5.0],
    [20.0, -5.0],
    [-10.0, -20.0],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RoamerKind {
    /// A giant: very dangerous, befriended only by rank 4+ performances.
    #[default]
    Epic,
    /// A solitary aggressive loner, tougher than a nest member.
    Rogue,
}

impl RoamerKind {
    pub fn label(self) -> &'static str {
        match self {
            RoamerKind::Epic => "Epic",
            RoamerKind::Rogue => "Rogue",
        }
    }

    /// The retail herd archetype behind the roamer.
    pub fn archetype(self) -> Archetype {
        match self {
            RoamerKind::Epic => Archetype::EpicPredator,
            RoamerKind::Rogue => Archetype::MiniBoss,
        }
    }
}

/// One persistent roaming creature: no nest, its own lair and territory.
/// Its live position and health are checkpointed like any `WorldId` actor;
/// the relationship here makes befriending/killing it exactly-once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roamer {
    pub id: u32,
    pub kind: RoamerKind,
    /// Territory centre (y is unused; ground height comes from the terrain).
    pub lair: [f32; 3],
    /// Index into the installed creature-model pool (`baked_model::species_key`).
    pub visual: u32,
    pub scale: f32,
    pub name: String,
    pub relationship: Relationship,
    /// Seconds since a never-extinct roamer (the epic predator) was slain; it
    /// returns after its archetype's `RespawnRate` (600 s).
    #[serde(default)]
    pub respawn_timer: f32,
}

impl Nest {
    pub fn population(&self) -> u32 {
        self.members.len() as u32
    }
    /// Alive (not defeated) members.
    pub fn surviving(&self) -> u32 {
        self.members
            .iter()
            .filter(|m| m.relationship.is_alive())
            .count() as u32
    }
    pub fn member(&self, id: u32) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }
}

/// Where a spawner should put one member of the ecology: its persistent id,
/// its species, and a world position (nest centre plus a small ring offset
/// so a nest reads as a group).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberSpawn {
    pub member: u32,
    pub species: u32,
    pub position: [f32; 3],
}

/// One distinct species: independent body, independent part build, and the
/// metadata a spawner needs. Builds are a pure function of the world seed and
/// the catalog, so reloading restores them byte for byte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Species {
    /// Stable species id (index into `SpeciesWorld::species`), stored on
    /// spawned members as `creature_actions::CreatureMind::species`.
    pub id: u32,
    /// Localized mouth part name when the catalog provides one.
    pub name: String,
    pub diet: Diet,
    /// Unlock depth this species drew from (1..=MAX_UNLOCK_TIER).
    pub tier: u32,
    /// Body scale multiplier, bounded to
    /// `MIN_SPECIES_SIZE..=MAX_SPECIES_SIZE`.
    pub size: f32,
    /// Carnivores hunt the player; others can be befriended.
    pub hostile: bool,
    /// Spawner tint (linear rgb), derived from the diet plus seed jitter.
    pub tint: [f32; 3],
    /// Independent body shape (validated: finite, in-range, non-degenerate).
    pub body: BodyProfile,
    /// Independent part build: same record type the player saves.
    pub parts: Vec<PartInstance>,
    /// The retail herd archetype this species fills (`None`: a legacy species
    /// that keeps the earlier OpenSpore rules).
    #[serde(default)]
    pub archetype: Option<Archetype>,
    /// Which of the archetype's daily schedules the herd follows.
    #[serde(default)]
    pub schedule: u32,
    /// NPC evolution steps taken (`dnaPointsEvolutionThresholds`, 0..=2).
    #[serde(default)]
    pub evolution: u8,
}

/// ADAPTED: an evolved species shows another installed creature of the pool
/// (retail swaps in an evolved species profile, `mEvolvedSpeciesProfileKeys`).
pub const EVOLVED_VISUAL_STRIDE: u32 = 211;

impl Species {
    /// Merged authored capabilities of this species' own parts — the same
    /// call the player path uses, so ranks agree with a mount of those parts.
    pub fn abilities(&self, catalog: &PartCatalog) -> Capabilities {
        catalog.abilities_in(CREATURE_PARTS, self.parts.iter().map(|p| p.instance))
    }

    /// Index into the installed creature-model pool (`baked_model::species_key`):
    /// the species id, moved along the pool once per evolution step.
    pub fn visual(&self) -> u32 {
        self.id + EVOLVED_VISUAL_STRIDE * self.evolution as u32
    }
}

/// The whole creature-stage ecology: seed, species, nests and thresholds.
/// Serialized into the lineage save (see `OPENCODE_PROGRESS.md`); `Default`
/// is an empty world, so old saves without the field load unchanged.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SpeciesWorld {
    /// Seed the world was generated from (also shown by the HUD).
    #[serde(default)]
    pub seed: u64,
    /// Radius nests were scattered within when the world was generated.
    #[serde(default)]
    pub radius: f32,
    /// True once generated; guards against regenerating over a saved world.
    #[serde(default)]
    pub generated: bool,
    /// Allies needed for a species to count as allied to the player.
    #[serde(default = "default_alliance_threshold")]
    pub alliance_threshold: u32,
    /// Defeated members needed to count the species as extinct, or
    /// `EXTINCT_WHEN_ALL` for "every member".
    #[serde(default)]
    pub extinction_threshold: u32,
    /// An actual attack angers the species, including an existing alliance.
    /// Preserve member outcomes to prevent befriending/attacking reward loops.
    #[serde(default)]
    pub aggravated_species: Vec<u32>,
    /// The relationship manager's record of every species the player has had an
    /// event with (`cSpeciesRelationshipManager`, `tRelationshipData`): see
    /// [`SpeciesRelation`]. A species without one reads from the outcome counts.
    #[serde(default)]
    pub relations: Vec<SpeciesRelation>,
    #[serde(default)]
    pub species: Vec<Species>,
    #[serde(default)]
    pub nests: Vec<Nest>,
    /// Epic and rogue creatures. Older saves gain them once on stage entry
    /// (`ensure_roamers`), without touching any recorded member outcome.
    #[serde(default)]
    pub roamers: Vec<Roamer>,
    #[serde(default)]
    pub roamers_generated: bool,
    /// The Creature stage's time of day in hours (`creatureStartTimeOfDay` 7
    /// for a fresh world), advanced by `creature_ecology::advance_clock`.
    #[serde(default = "default_time_of_day")]
    pub time_of_day: f32,
    /// Nights that have begun (seeds each night's predator ambush).
    #[serde(default)]
    pub nights: u32,
    /// Where the generation list's Carcass lies (a meal for a carnivore
    /// start), and whether it was placed this visit.
    #[serde(default)]
    pub carcass: Option<[f32; 3]>,
    /// True once the nests stand on the original's planet at the retail
    /// distances (planet-re 4E step 4). Older worlds were laid out on the
    /// flat island at a tenth of the scale; `relayout_on` moves them once.
    #[serde(default)]
    pub planet_layout: bool,
    /// The Creature stage's planet frame (planet-local to world rotation)
    /// after the floating origin turned the planet under the avatar
    /// (`creature_planet::reanchor`); `None`: the stage origin's frame.
    #[serde(default)]
    pub planet_frame: Option<[f32; 4]>,
}

fn default_alliance_threshold() -> u32 {
    DEFAULT_ALLIANCE_THRESHOLD
}

fn default_time_of_day() -> f32 {
    ecology::START_TIME_OF_DAY
}

impl SpeciesWorld {
    // ---------------------------------------------------------------- world

    /// Generate a fresh world from the retail generation list
    /// (`generate_retail`). `radius` is kept for the callers' signature; the
    /// retail bands decide where herds live. Pure: the same seed always
    /// produces the same world.
    pub fn generate(seed: u64, catalog: &PartCatalog, radius: f32) -> Self {
        let _ = radius;
        Self::generate_retail(seed, catalog)
    }

    /// The retail population (notes C4A/C4B): every herd the generation list
    /// places becomes one species with one nest of `NumCreatures` members
    /// (alphas, babies and guards by the archetype's shares), at the
    /// archetype's distance band from the home nest; the Carcass becomes a
    /// meal near home; the night-predator herd is added for the night; the
    /// roamers (epic predator, miniboss) follow.
    pub fn generate_retail(seed: u64, catalog: &PartCatalog) -> Self {
        Self::generate_retail_on(seed, catalog, &crate::creature_planet::Land::flat())
    }

    /// `generate_retail` on a planet: the nests at their bands on its land.
    /// Nest positions are world points (chart x, z and the surface height).
    pub fn generate_retail_on(seed: u64, catalog: &PartCatalog, land: &crate::creature_planet::Land) -> Self {
        let pool = PartPool::from_catalog(catalog);
        let phase = seed % 0x1_0000;
        let mut world = SpeciesWorld {
            seed,
            radius: ecology::casting_thresholds().1,
            generated: true,
            alliance_threshold: DEFAULT_ALLIANCE_THRESHOLD,
            extinction_threshold: EXTINCT_WHEN_ALL,
            time_of_day: ecology::START_TIME_OF_DAY,
            planet_layout: land.on_planet(),
            ..Default::default()
        };
        let mut plan = ecology::plan_population(seed);
        // The night-predator herd is created at night near the avatar (0xD4F290);
        // it is generated with the world so its build and outcomes persist.
        plan.push(ecology::HerdPlan {
            archetype: Archetype::NightPredator,
            slot: ecology::SPECIES_FILES.len() + ecology::EXTRA_SETS.len(),
        });
        let placed = ecology::place_population(seed, &plan, ecology::HOME, land);
        // The forest sets (notes C9D, VERIFIED 0xB9C980): every forest within
        // 200 m of the start gets the sets of orestSpeciesRules, each herd at
        // its band from the forest; the epic predator and the miniboss come
        // from here. Without a planet (no forests) the roamers fall back to
        // ensure_roamers_on (ADAPTED counts).
        let forests: Vec<[f32; 2]> = land
            .forests(Vec2::from(ecology::HOME), ecology::FOREST_RADIUS * ecology::CREATURE_WORLD_SCALE)
            .into_iter()
            .map(|p| p.to_array())
            .collect();
        let mut claimed = placed.clone();
        let forest_herds = ecology::place_forest_population(seed, &forests, &mut claimed, land);
        if !forests.is_empty() {
            bevy::log::info!(
                "species world {seed:#X}: {} forests within {} m -> {} forest herds ({:?})",
                forests.len(),
                ecology::FOREST_RADIUS,
                forest_herds.len(),
                forest_herds.iter().map(|h| h.archetype).collect::<Vec<_>>()
            );
        }
        let mut plan = plan;
        let mut placed = placed;
        let mut forest_roamers = Vec::new();
        for herd in &forest_herds {
            if herd.archetype.is_herd() {
                plan.push(ecology::HerdPlan { archetype: herd.archetype, slot: herd.slot });
                placed.push(ecology::Placement { archetype: herd.archetype, position: herd.position });
            } else {
                forest_roamers.push(*herd);
            }
        }
        let mut next_member: u32 = MEMBER_ID_BASE;
        for (herd, at) in plan.iter().zip(placed.iter()) {
            let archetype = herd.archetype;
            let point = land.world_point(Vec2::from(at.position)).to_array();
            if archetype == Archetype::Carcass {
                world.carcass = Some(point);
                continue;
            }
            let index = world.species.len() as u32;
            let Some(mut species) = pool.build_species_for(seed, phase, index, archetype) else {
                continue;
            };
            species.schedule = (mix64(seed ^ 0x5C4E ^ ((herd.slot as u64) << 8)) % 3) as u32;
            let key = seed ^ 0x4E57_0000 ^ ((herd.slot as u64) << 12);
            let count = ecology::herd_size(archetype, key);
            let members = ecology::member_roles(archetype, count, key)
                .into_iter()
                .map(|(role, guard)| {
                    let mut m = Member::new(next_member);
                    m.role = role;
                    m.guard = guard;
                    next_member += 1;
                    m
                })
                .collect();
            world.nests.push(Nest {
                species: species.id,
                position: point,
                members,
                rewarded: false,
                herd_size: count,
                respawn_timer: 0.0,
            });
            world.species.push(species);
        }
        if !forests.is_empty() {
            world.roamers_generated = true;
            for (index, herd) in forest_roamers.iter().enumerate() {
                let kind = if herd.archetype == Archetype::EpicPredator { RoamerKind::Epic } else { RoamerKind::Rogue };
                let visual = 40 + (mix64(phase ^ 0xE91C_0000 ^ index as u64) % 64) as u32;
                world.roamers.push(Roamer {
                    id: ROAMER_ID_BASE + index as u32,
                    kind,
                    lair: land.world_point(Vec2::from(herd.position)).to_array(),
                    visual,
                    scale: ecology::species_scale(herd.archetype),
                    name: format!("{} creature", kind.label()),
                    relationship: Relationship::Neutral,
                    respawn_timer: 0.0,
                });
            }
        }
        world.ensure_roamers_on(land);
        world
    }

    /// Move an island-scale retail world (saved before the planet) onto the
    /// planet once: every nest, lair and the carcass is placed again at its
    /// archetype's retail band, in the world's own order, as a fresh world of
    /// the same seed would be. Members, relationships and outcomes are kept.
    /// Returns whether anything moved.
    pub fn relayout_on(&mut self, land: &crate::creature_planet::Land) -> bool {
        if self.planet_layout || !land.on_planet() || !self.generated {
            return false;
        }
        self.planet_layout = true;
        if !self.is_retail() {
            return false;
        }
        let plan: Vec<ecology::HerdPlan> = self
            .nests
            .iter()
            .enumerate()
            .filter_map(|(slot, n)| {
                Some(ecology::HerdPlan {
                    archetype: self.species(n.species)?.archetype?,
                    slot,
                })
            })
            .collect();
        let placed = ecology::place_population(self.seed, &plan, ecology::HOME, land);
        for (herd, at) in plan.iter().zip(placed.iter()) {
            self.nests[herd.slot].position = land.world_point(Vec2::from(at.position)).to_array();
        }
        if self.carcass.is_some() {
            let at = ecology::place_nest(
                self.seed,
                self.seed ^ 0xCA2C,
                Archetype::Carcass,
                0,
                ecology::HOME,
                &placed,
                land,
            );
            self.carcass = Some(land.world_point(Vec2::from(at)).to_array());
        }
        let mut lairs: Vec<ecology::Placement> = placed;
        for (index, roamer) in self.roamers.iter_mut().enumerate() {
            let at = Self::roamer_lair(self.seed, index, roamer.kind, &lairs, land);
            lairs.push(ecology::Placement {
                archetype: roamer.kind.archetype(),
                position: at,
            });
            roamer.lair = land.world_point(Vec2::from(at)).to_array();
        }
        true
    }

    /// Generate a legacy (pre-archetype) world with `count` species (clamped
    /// to `MIN_SPECIES..=MAX_SPECIES`). Every species is guaranteed at least
    /// one populated nest: the member budget is shared so early species cannot
    /// starve later ones.
    pub fn generate_n(seed: u64, catalog: &PartCatalog, radius: f32, count: usize) -> Self {
        let count = count.clamp(MIN_SPECIES, MAX_SPECIES);
        let radius = if radius.is_finite() && radius > 0.5 {
            radius.min(200.0)
        } else {
            DEFAULT_NEST_RADIUS
        };
        let pool = PartPool::from_catalog(catalog);
        let phase = seed % 0x1_0000;
        let mut world = SpeciesWorld {
            seed,
            radius,
            generated: true,
            alliance_threshold: DEFAULT_ALLIANCE_THRESHOLD,
            extinction_threshold: EXTINCT_WHEN_ALL,
            time_of_day: ecology::START_TIME_OF_DAY,
            ..Default::default()
        };
        // Single running counter: member ids never move once assigned, so
        // exactly-once outcomes survive reloads.
        let mut next_member: u32 = MEMBER_ID_BASE;
        let mut budget: usize = MAX_TOTAL_MEMBERS;
        for i in 0..count as u32 {
            let Some(species) = pool.build_species(seed, phase, i) else {
                continue;
            };
            // Reserve an even share for every species that still needs one, so
            // the last species is populated too.
            let remaining = (count - i as usize) as usize;
            let share = (budget / remaining).max(MIN_MEMBERS_PER_NEST);
            // One to three homes per species, fanned around a per-species
            // centre so nests of different species stay apart.
            let homes = 1 + (mix64(phase ^ ((i as u64) + 1) * 104_729) % 3) as usize;
            let max_homes = (share / MIN_MEMBERS_PER_NEST).min(homes).max(1);
            let centre_angle = i as f32 / count as f32 * std::f32::consts::TAU;
            let cx = centre_angle.cos() * radius * 0.45;
            let cz = centre_angle.sin() * radius * 0.45;
            let mut taken = 0usize;
            for h in 0..max_homes {
                // Leave the share's remainder for the homes still to build.
                let left_in_share = share - taken;
                let homes_left = max_homes - h;
                if homes_left * MIN_MEMBERS_PER_NEST > left_in_share {
                    break;
                }
                let want = nest_member_count(seed, i, h as u32).min(left_in_share);
                let nest_index = world.nests.len();
                let (ox, oz) = nest_offset(nest_index, max_homes);
                world.nests.push(Nest {
                    species: species.id,
                    position: [cx + ox, 0.4, cz + oz],
                    members: (0..want)
                        .map(|_| {
                            let m = Member::new(next_member);
                            next_member += 1;
                            m
                        })
                        .collect(),
                    rewarded: false,
                    herd_size: 0,
                    respawn_timer: 0.0,
                });
                taken += want;
            }
            budget = budget.saturating_sub(taken);
            world.species.push(species);
        }
        world.ensure_roamers();
        world
    }

    /// Add the world's epic and rogue creatures once. Lairs are chosen from
    /// fixed far-side candidates, farthest from every nest first, so giants
    /// do not camp on a nest. Pure: the same seed and nests give the same
    /// roamers, and an existing roster (and its outcomes) is never rebuilt.
    pub fn ensure_roamers(&mut self) {
        self.ensure_roamers_on(&crate::creature_planet::Land::flat());
    }

    /// `ensure_roamers` with the retail lairs on the planet's land.
    pub fn ensure_roamers_on(&mut self, land: &crate::creature_planet::Land) {
        if self.roamers_generated {
            return;
        }
        self.roamers_generated = true;
        let phase = self.seed % 0x1_0000;
        if self.is_retail() {
            self.place_retail_roamers(phase, land);
            return;
        }
        let mut lairs: Vec<(f32, u64, [f32; 2])> = LAIR_CANDIDATES
            .iter()
            .enumerate()
            .map(|(i, at)| {
                let clearance = self
                    .nests
                    .iter()
                    .map(|n| dist_xz(n.position, [at[0], 0.0, at[1]]))
                    .fold(f32::INFINITY, f32::min);
                (clearance.min(60.0), mix64(phase ^ 0xE91C ^ i as u64), *at)
            })
            .collect();
        // Farthest from nests first; the seed breaks ties between equally
        // clear lairs so different worlds put their giants in different places.
        lairs.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let kinds = std::iter::repeat_n(RoamerKind::Epic, EPIC_COUNT)
            .chain(std::iter::repeat_n(RoamerKind::Rogue, ROGUE_COUNT));
        for (index, (kind, (_, _, at))) in kinds.zip(lairs).enumerate() {
            let scale = match kind {
                RoamerKind::Epic => EPIC_SCALE,
                RoamerKind::Rogue => ROGUE_SCALE,
            };
            // Visual pool slots far above the species ids, so a giant never
            // shares its model with a nesting species of the same world.
            let visual = 40 + (mix64(phase ^ 0xE91C_0000 ^ index as u64) % 64) as u32;
            self.roamers.push(Roamer {
                id: ROAMER_ID_BASE + index as u32,
                kind,
                lair: [at[0], 0.4, at[1]],
                visual,
                scale,
                name: format!("{} creature", kind.label()),
                relationship: Relationship::Neutral,
                respawn_timer: 0.0,
            });
        }
    }

    /// A world generated from the retail generation list.
    pub fn is_retail(&self) -> bool {
        self.species.iter().any(|s| s.archetype.is_some())
    }

    /// Lair of roamer `index`: its archetype's band from the home nest, no
    /// closer than the casting activation distance (see
    /// `place_retail_roamers`), on land outside the exclusive areas of `placed`.
    fn roamer_lair(
        seed: u64,
        index: usize,
        kind: RoamerKind,
        placed: &[ecology::Placement],
        land: &crate::creature_planet::Land,
    ) -> [f32; 2] {
        let archetype = kind.archetype();
        let (lo, hi) = ecology::band(archetype, if kind == RoamerKind::Epic { 1 } else { 0 });
        let near = ecology::casting_thresholds().0;
        ecology::place_in_band(
            seed,
            seed ^ 0x40A3_0000 ^ ((index as u64) << 16),
            archetype,
            (lo.max(near), hi.max(near + 1.0)),
            ecology::HOME,
            placed,
            land,
        )
    }

    /// Retail lairs: each roamer at its archetype's band from the home nest,
    /// outside the herds' exclusive areas. ADAPTED (their placement is not
    /// traced, `creature_ecology::EXTRA_SETS`): the epic predator uses its
    /// optional band (120-300 m) instead of 50-80 m, and no roamer lair is
    /// closer to the hatching nest than the casting activation distance
    /// (100 m), so a 250-HP x2 / 1000-HP x100 creature does not start on it.
    fn place_retail_roamers(&mut self, phase: u64, land: &crate::creature_planet::Land) {
        let mut placed: Vec<ecology::Placement> = self
            .nests
            .iter()
            .filter_map(|n| {
                Some(ecology::Placement {
                    archetype: self.species(n.species)?.archetype?,
                    position: [n.position[0], n.position[2]],
                })
            })
            .collect();
        let kinds = std::iter::repeat_n(RoamerKind::Epic, EPIC_COUNT)
            .chain(std::iter::repeat_n(RoamerKind::Rogue, ROGUE_COUNT));
        for (index, kind) in kinds.enumerate() {
            let archetype = kind.archetype();
            let at = Self::roamer_lair(self.seed, index, kind, &placed, land);
            placed.push(ecology::Placement {
                archetype,
                position: at,
            });
            let visual = 40 + (mix64(phase ^ 0xE91C_0000 ^ index as u64) % 64) as u32;
            self.roamers.push(Roamer {
                id: ROAMER_ID_BASE + index as u32,
                kind,
                lair: land.world_point(Vec2::from(at)).to_array(),
                visual,
                scale: ecology::species_scale(archetype),
                name: format!("{} creature", kind.label()),
                relationship: Relationship::Neutral,
                respawn_timer: 0.0,
            });
        }
    }

    pub fn roamer(&self, id: u32) -> Option<&Roamer> {
        self.roamers.iter().find(|r| r.id == id)
    }

    /// Record a roamer outcome exactly once; death is absorbing.
    pub fn record_roamer(&mut self, id: u32, relationship: Relationship) -> RecordResult {
        let Some(roamer) = self.roamers.iter_mut().find(|r| r.id == id) else {
            return RecordResult::UnknownMember;
        };
        if roamer.relationship == relationship || roamer.relationship == Relationship::Defeated {
            return RecordResult::Already;
        }
        roamer.relationship = relationship;
        roamer.respawn_timer = 0.0;
        RecordResult::Recorded
    }

    /// Tick slain roamers that are never extinct (the epic predator,
    /// `NumToKillForExtinction` -1): after the archetype's `RespawnRate` the
    /// roamer is back, wild. Returns the ids that came back.
    pub fn tick_roamer_respawns(&mut self, dt: f32) -> Vec<u32> {
        let mut back = Vec::new();
        if !dt.is_finite() || dt <= 0.0 {
            return back;
        }
        for roamer in &mut self.roamers {
            let archetype = roamer.kind.archetype();
            if roamer.relationship != Relationship::Defeated
                || ecology::kills_to_extinction(archetype).is_some()
            {
                continue;
            }
            roamer.respawn_timer += dt;
            if roamer.respawn_timer >= archetype.record().respawn_rate {
                roamer.respawn_timer = 0.0;
                roamer.relationship = Relationship::Neutral;
                back.push(roamer.id);
            }
        }
        back
    }

    // --------------------------------------------------------------- nests

    pub fn nest_index_for_member(&self, id: u32) -> Option<usize> {
        self.nests.iter().position(|n| n.member(id).is_some())
    }

    /// Whole-nest outcome. An attacked (angry) species cannot hold an allied
    /// nest, so mixing kills and friendships never completes one twice.
    pub fn nest_status(&self, index: usize) -> NestStatus {
        let Some(nest) = self.nests.get(index) else {
            return NestStatus::Wild;
        };
        if nest.members.is_empty() {
            return NestStatus::Wild;
        }
        // Retail herds finish with their species: extinct after the
        // archetype's kills, allied at best friends (C4B).
        if self.archetype_of(nest.species).is_some() {
            return if self.species_extinct(nest.species) {
                NestStatus::Destroyed
            } else if self.species_allied(nest.species) {
                NestStatus::Allied
            } else {
                NestStatus::Wild
            };
        }
        if nest
            .members
            .iter()
            .all(|m| m.relationship == Relationship::Defeated)
        {
            NestStatus::Destroyed
        } else if !self.species_aggravated(nest.species)
            && nest
                .members
                .iter()
                .all(|m| m.relationship == Relationship::Allied)
        {
            NestStatus::Allied
        } else {
            NestStatus::Wild
        }
    }

    /// Claim a completed nest's one-time reward. Returns the completion the
    /// first time a nest is allied or destroyed, and `None` afterwards.
    pub fn claim_nest(&mut self, index: usize) -> Option<NestStatus> {
        let status = self.nest_status(index);
        let nest = self.nests.get_mut(index)?;
        if status == NestStatus::Wild || nest.rewarded {
            return None;
        }
        nest.rewarded = true;
        Some(status)
    }

    /// The allied nest whose centre is within `radius` of `position`, if
    /// any: a friendly nest is a place to heal, as in Spore.
    pub fn allied_nest_near(&self, position: [f32; 3], radius: f32) -> Option<usize> {
        (0..self.nests.len()).find(|&i| {
            self.nest_status(i) == NestStatus::Allied
                && dist_xz(self.nests[i].position, position) <= radius
        })
    }

    /// Every nest of the species is befriended.
    pub fn species_fully_allied(&self, species: u32) -> bool {
        let nests: Vec<usize> = (0..self.nests.len())
            .filter(|&i| self.nests[i].species == species)
            .collect();
        !nests.is_empty()
            && nests
                .iter()
                .all(|&i| self.nest_status(i) == NestStatus::Allied)
    }

    /// Generate only when the world is empty (entering a stage with a save
    /// that may already hold a world must not wipe it).
    pub fn ensure_generated(&mut self, seed: u64, catalog: &PartCatalog, radius: f32) {
        if !self.generated || self.species.is_empty() {
            *self = Self::generate(seed, catalog, radius);
        }
    }

    /// `ensure_generated` on the stage planet; an island-scale world saved
    /// earlier is moved onto the planet once (`relayout_on`).
    pub fn ensure_generated_on(&mut self, seed: u64, catalog: &PartCatalog, land: &crate::creature_planet::Land) {
        if !self.generated || self.species.is_empty() {
            *self = Self::generate_retail_on(seed, catalog, land);
        } else {
            self.relayout_on(land);
        }
    }

    /// Regenerate a fresh world (new run / restart), replacing any state.
    pub fn new_world(&mut self, seed: u64, catalog: &PartCatalog, radius: f32) {
        *self = Self::generate(seed, catalog, radius);
    }

    // ------------------------------------------------------------- lookups

    pub fn species(&self, id: u32) -> Option<&Species> {
        self.species.iter().find(|s| s.id == id)
    }

    pub fn nest_for_species(&self, species: u32) -> Option<&Nest> {
        self.nests.iter().find(|n| n.species == species)
    }

    /// Every nest of a species.
    pub fn nests_of_species(&self, species: u32) -> Vec<&Nest> {
        self.nests.iter().filter(|n| n.species == species).collect()
    }

    /// Nest whose position is closest to `position` (xz distance, matching the
    /// game's `game::dist_xz`).
    pub fn nearest_nest(&self, position: [f32; 3]) -> Option<&Nest> {
        self.nests
            .iter()
            .min_by(|a, b| dist_xz(a.position, position).total_cmp(&dist_xz(b.position, position)))
    }

    pub fn member(&self, id: u32) -> Option<&Member> {
        self.nests.iter().find_map(|n| n.member(id))
    }

    pub fn member_mut(&mut self, id: u32) -> Option<&mut Member> {
        self.nests
            .iter_mut()
            .find_map(|n| n.members.iter_mut().find(|m| m.id == id))
    }

    /// Nest that holds a member id (used to resolve an entity back to its
    /// species after a reload).
    pub fn nest_for_member(&self, id: u32) -> Option<&Nest> {
        self.nests.iter().find(|n| n.member(id).is_some())
    }

    pub fn species_for_member(&self, id: u32) -> Option<&Species> {
        let nest = self.nest_for_member(id)?;
        self.species(nest.species)
    }

    pub fn relationship(&self, id: u32) -> Option<Relationship> {
        self.member(id).map(|m| m.relationship)
    }

    /// The retail archetype of a species, if it has one.
    pub fn archetype_of(&self, species: u32) -> Option<Archetype> {
        self.species(species)?.archetype
    }

    /// A member's ring offset inside its nest: the legacy ring, or for a retail
    /// herd a ring sized for its creatures (`creature_ecology::spawn_ring_radius`).
    fn nest_ring_offset(&self, nest: &Nest, index: usize) -> (f32, f32) {
        let count = nest.members.len();
        match self.species(nest.species).filter(|s| s.archetype.is_some()) {
            Some(species) => {
                if count <= 1 {
                    return (0.0, 0.0);
                }
                let ring = ecology::spawn_ring_radius(species.size, count);
                let angle = index as f32 / count as f32 * std::f32::consts::TAU;
                (angle.cos() * ring, angle.sin() * ring)
            }
            None => member_offset(index, count),
        }
    }

    /// World position of a member (nest centre plus its ring offset), or
    /// `None` for an unknown id.
    pub fn member_position(&self, id: u32) -> Option<[f32; 3]> {
        for nest in &self.nests {
            if let Some(index) = nest.members.iter().position(|m| m.id == id) {
                let (ox, oz) = self.nest_ring_offset(nest, index);
                return Some([
                    nest.position[0] + ox,
                    nest.position[1],
                    nest.position[2] + oz,
                ]);
            }
        }
        None
    }

    /// Every member with the species it belongs to and where to spawn it.
    /// Deterministic order (nests, then members) for reproducible spawning.
    pub fn spawn_list(&self) -> Vec<MemberSpawn> {
        let mut out = Vec::new();
        for nest in &self.nests {
            for (index, m) in nest.members.iter().enumerate() {
                let (ox, oz) = self.nest_ring_offset(nest, index);
                out.push(MemberSpawn {
                    member: m.id,
                    species: nest.species,
                    position: [
                        nest.position[0] + ox,
                        nest.position[1],
                        nest.position[2] + oz,
                    ],
                });
            }
        }
        out
    }

    pub fn total_members(&self) -> usize {
        self.nests.iter().map(|n| n.members.len()).sum()
    }

    // ------------------------------------------------------------ outcomes

    /// Befriend a persistent member. Returns `Recorded` the first time and
    /// `Already` afterwards, so DNA is awarded exactly once per member and
    /// survives reloads. Dead members never become allies.
    pub fn record_befriend(&mut self, id: u32) -> RecordResult {
        self.record(id, Relationship::Allied)
    }

    /// Defeat a persistent member. Returns `Recorded` the first time and
    /// `Already` afterwards. Death is absorbing and beats an alliance: a
    /// member that was befriended and then killed ends up `Defeated`.
    pub fn record_defeat(&mut self, id: u32) -> RecordResult {
        self.record(id, Relationship::Defeated)
    }

    pub fn record_attack(&mut self, id: u32) -> RecordResult {
        let Some(member) = self.member(id) else {
            return RecordResult::UnknownMember;
        };
        if !member.relationship.is_alive() {
            return RecordResult::Already;
        }
        let species = self.nest_for_member(id).unwrap().species;
        if self.aggravated_species.contains(&species) {
            return RecordResult::Already;
        }
        self.ensure_relation(species);
        self.aggravated_species.push(species);
        // The attack event (0xBA4B89): hated, or hostile after two friendly times.
        self.relation_event(species, RelationEvent::Attack);
        RecordResult::Recorded
    }

    pub fn species_aggravated(&self, species: u32) -> bool {
        self.aggravated_species.contains(&species)
    }

    /// Friends that ally the species: the archetype's
    /// `NumToImpressForBestFriends` (3; miniboss 1) for a retail herd, the
    /// saved threshold otherwise; never more than the species has members.
    pub fn alliance_required(&self, species: u32) -> u32 {
        let total = self.species_counts(species).total();
        match self.archetype_of(species) {
            Some(archetype) => ecology::best_friends_needed(archetype, total),
            None => self.alliance_threshold.max(1).min(total.max(1)),
        }
    }

    /// The species is friendly (`NumToImpressForFriendly`, 2 for every herd).
    pub fn species_friendly(&self, species: u32) -> bool {
        let needed = self
            .archetype_of(species)
            .map(ecology::friendly_needed)
            .unwrap_or_else(|| self.alliance_required(species));
        !self.species_aggravated(species) && self.species_counts(species).allied >= needed
    }

    /// A member that treats the player as a friend: befriended itself, or a
    /// living member of a retail species the player is best friends with (the
    /// whole herd turns friendly and can join the pack).
    pub fn member_friendly(&self, id: u32) -> bool {
        let Some(member) = self.member(id) else {
            return false;
        };
        match member.relationship {
            Relationship::Allied => true,
            Relationship::Defeated => false,
            Relationship::Neutral => self
                .nest_for_member(id)
                .is_some_and(|n| self.archetype_of(n.species).is_some() && self.species_allied(n.species)),
        }
    }

    /// The role of a member (adults for unknown ids and legacy members).
    pub fn member_role(&self, id: u32) -> MemberRole {
        self.member(id).map(|m| m.role).unwrap_or_default()
    }

    /// The archetype a creature id belongs to: a herd member's species
    /// archetype, or a roamer's.
    pub fn archetype_for(&self, id: u32) -> Option<Archetype> {
        if let Some(roamer) = self.roamer(id) {
            return self.is_retail().then_some(roamer.kind.archetype());
        }
        self.archetype_of(self.nest_for_member(id)?.species)
    }

    /// Replace lost herd members (archetype `RespawnRate`): a nest of a living,
    /// not-extinct species that is short of its herd size counts up, and when
    /// the rate is reached a new adult member hatches with the next free id of
    /// the member band. Returns the new ids. Night predators (rate 100000) and
    /// legacy nests (herd size 0) are never refilled.
    pub fn tick_respawn(&mut self, dt: f32) -> Vec<u32> {
        self.tick_respawn_eggs(dt, |_| 0, 1)
    }

    /// `tick_respawn` with the retail wave rule of the herd update (0xC6DE5C):
    /// when the timer has run past `RespawnRate` the herd asks for
    /// `min(shortfall, wave) - eggs` new members at once, `eggs` being the
    /// members of nest `index` that are still incubating in an egg (they count
    /// as members of the nest here, as retail's egg count is subtracted from
    /// the shortfall). `wave` is `maxNumEggs` (2) for the egg-laying herds.
    pub fn tick_respawn_eggs(&mut self, dt: f32, eggs: impl Fn(usize) -> usize, wave: usize) -> Vec<u32> {
        let mut born = Vec::new();
        if !dt.is_finite() || dt <= 0.0 {
            return born;
        }
        let mut next = self
            .nests
            .iter()
            .flat_map(|n| n.members.iter().map(|m| m.id))
            .max()
            .map_or(MEMBER_ID_BASE, |id| id + 1);
        for index in 0..self.nests.len() {
            let species = self.nests[index].species;
            let Some(archetype) = self.archetype_of(species) else {
                continue;
            };
            let incubating = eggs(index);
            let short = {
                let nest = &self.nests[index];
                nest.herd_size > 0 && nest.surviving() < nest.herd_size
            };
            if !short || self.species_extinct(species) {
                self.nests[index].respawn_timer = 0.0;
                continue;
            }
            let rate = archetype.record().respawn_rate;
            let nest = &mut self.nests[index];
            nest.respawn_timer += dt;
            if nest.respawn_timer >= rate && next < ROAMER_ID_BASE {
                // Living members only count against the target (the incubating
                // ones are already in `surviving`).
                let living = (nest.surviving() as usize).saturating_sub(incubating);
                let count =
                    crate::creature_eggs::eggs_wanted(nest.herd_size as usize, living, incubating, wave.max(1));
                if count == 0 {
                    continue;
                }
                nest.respawn_timer = 0.0;
                for _ in 0..count {
                    if next >= ROAMER_ID_BASE {
                        break;
                    }
                    nest.members.push(Member::new(next));
                    born.push(next);
                    next += 1;
                }
            }
        }
        born
    }

    fn record(&mut self, id: u32, relationship: Relationship) -> RecordResult {
        // The species' relationship record starts from the counts before the member changes
        // (a save from before the record continues from them).
        let species = self.nest_for_member(id).map(|n| n.species);
        if let Some(species) = species {
            self.ensure_relation(species);
        }
        let Some(member) = self.member_mut(id) else {
            return RecordResult::UnknownMember;
        };
        if member.relationship == relationship {
            return RecordResult::Already;
        }
        // A defeated member is gone for good; anything else can transition.
        if member.relationship == Relationship::Defeated {
            return RecordResult::Already;
        }
        member.relationship = relationship;
        if let Some(species) = species {
            match relationship {
                Relationship::Allied => self.relation_event(species, RelationEvent::Befriend),
                Relationship::Defeated => self.relation_event(species, RelationEvent::Kill),
                Relationship::Neutral => {}
            }
        }
        RecordResult::Recorded
    }

    /// The relationship manager's record of `species` (retail herds only).
    pub fn relation(&self, species: u32) -> Option<&SpeciesRelation> {
        self.relations.iter().find(|r| r.species == species)
    }

    /// Create the record of a retail herd's species from the counts and its
    /// initial relationship, when it has none (a legacy species has none).
    fn ensure_relation(&mut self, species: u32) {
        if self.relation(species).is_some() {
            return;
        }
        let Some(archetype) = self.archetype_of(species) else {
            return;
        };
        let r = archetype.record();
        let counts = self.species_counts(species);
        let relationship = crate::creature_rules::species_relationship(
            r.initial_relationship,
            counts.allied,
            counts.defeated,
            [r.impress_curious, r.impress_friendly, r.impress_best_friends, r.kills_for_fearful],
        );
        let aggravated = self.aggravated_species.contains(&species);
        self.relations.push(SpeciesRelation {
            species,
            relationship: if aggravated && relationship != crate::creature_rules::relationship::HOSTILE {
                crate::creature_rules::relationship::WARY
            } else {
                relationship
            },
            times_friendly: u32::from(relationship == crate::creature_rules::relationship::FRIENDLY
                || relationship == crate::creature_rules::relationship::BEST_FRIENDS),
            impressions: counts.allied,
            kills: counts.defeated,
        });
    }

    /// Run an event on the species' record (the thresholds are the herd's).
    fn relation_event(&mut self, species: u32, event: RelationEvent) {
        let Some(archetype) = self.archetype_of(species) else {
            return;
        };
        let r = archetype.record();
        let thresholds = [r.impress_curious, r.impress_friendly, r.impress_best_friends, r.kills_for_fearful];
        if let Some(relation) = self.relations.iter_mut().find(|rel| rel.species == species) {
            relation.apply(event, thresholds);
        }
    }

    // ------------------------------------------------------------- counts

    pub fn outcome_counts(&self) -> RelationshipCounts {
        RelationshipCounts::of(
            self.nests
                .iter()
                .flat_map(|n| n.members.iter().map(|m| m.relationship)),
        )
    }

    pub fn species_counts(&self, species: u32) -> RelationshipCounts {
        RelationshipCounts::of(
            self.nests_of_species(species)
                .into_iter()
                .flat_map(|n| n.members.iter().map(|m| m.relationship)),
        )
    }

    /// True once the species has enough befriended members to count as
    /// allied to the player.
    pub fn species_allied(&self, species: u32) -> bool {
        !self.species_aggravated(species)
            && self.species_counts(species).allied >= self.alliance_required(species)
    }

    /// True once the species' members are gone: for a retail herd after the
    /// archetype's `NumToKillForExtinction` kills (never for -1); otherwise
    /// every member is defeated (`EXTINCT_WHEN_ALL`) or `extinction_threshold`
    /// of them are.
    pub fn species_extinct(&self, species: u32) -> bool {
        let counts = self.species_counts(species);
        if counts.total() == 0 {
            return false;
        }
        if let Some(archetype) = self.archetype_of(species) {
            return match ecology::kills_to_extinction(archetype) {
                Some(kills) => counts.defeated >= kills || counts.alive() == 0,
                None => false,
            };
        }
        if self.extinction_threshold == EXTINCT_WHEN_ALL {
            counts.alive() == 0
        } else {
            counts.defeated >= self.extinction_threshold
        }
    }

    /// Alive (not defeated) members of a species.
    pub fn surviving_members(&self, species: u32) -> u32 {
        self.species_counts(species).alive()
    }
}

/// Species count a fresh world asks for: three or four, decided by the seed.
pub fn default_species_count(seed: u64) -> usize {
    let span = (MAX_DEFAULT_SPECIES - MIN_DEFAULT_SPECIES + 1) as u64;
    MIN_DEFAULT_SPECIES + (mix64(seed ^ 0x5150) % span) as usize
}

/// Deterministic seed for a run's ecology, derived from the player's own
/// lineage build: each creature gets its own world, and the same save always
/// regenerates the same one.
pub fn lineage_seed(parts: &[PartInstance], body: &BodyProfile) -> u64 {
    let mut z = 0x5DEE_0000u64;
    for (i, p) in parts.iter().enumerate() {
        z ^= (p.instance as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) << (i % 24);
        z ^= (p.bone as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    }
    let body = body.validated();
    for (i, point) in body.spine.iter().enumerate() {
        z ^= (point.radius.to_bits() as u64)
            .wrapping_mul(0x1656_67B1_9E37_79F9)
            .rotate_left(i as u32 % 63);
        for axis in 0..3 {
            z ^= (point.position[axis].to_bits() as u64).wrapping_add(i as u64 + axis as u64);
        }
    }
    mix64(z)
}

/// Flat xz distance, matching `game::dist_xz`.
fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dz = a[2] - b[2];
    (dx * dx + dz * dz).sqrt()
}

/// splitmix64 finalizer: a stable, dependency-free hash so every decision
/// is a pure function of the world seed (no RNG state anywhere).
fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Bounded value in `lo..=hi` from a hash (deterministic, finite).
fn spread(key: u64, lo: f32, hi: f32) -> f32 {
    lo + (mix64(key) % 1000) as f32 / 1000.0 * (hi - lo)
}

/// Deterministic index into a bucket.
fn pick_index(key: u64, len: usize) -> usize {
    (mix64(key) % len as u64) as usize
}

/// Highest authored rank wins, ties broken by the seed index. Keeps a
/// species' signature ability the strongest it can field at its tier.
fn pick_by_rank<'a>(
    list: &[&'a PartCandidate],
    rank: impl Fn(&PartCandidate) -> u8,
    key: u64,
) -> &'a PartCandidate {
    let top = list.iter().map(|c| rank(c)).max().unwrap_or(0);
    let finalists: Vec<&&PartCandidate> = list.iter().filter(|c| rank(c) == top).collect();
    finalists[pick_index(key, finalists.len())]
}

/// Members per nest: `MIN_MEMBERS_PER_NEST..=MAX_MEMBERS_PER_NEST`.
fn nest_member_count(seed: u64, species: u32, home: u32) -> usize {
    let span = (MAX_MEMBERS_PER_NEST - MIN_MEMBERS_PER_NEST + 1) as u64;
    MIN_MEMBERS_PER_NEST
        + (mix64(seed ^ 0x51CE ^ (((species as u64) + 1) << 8 | home as u64)) % span) as usize
}

/// Fan a species' homes out around its centre so nests do not overlap.
fn nest_offset(nest_index: usize, homes: usize) -> (f32, f32) {
    if homes <= 1 {
        return (0.0, 0.0);
    }
    let angle = nest_index as f32 / homes as f32 * std::f32::consts::TAU;
    let radius = 2.0 + 1.5 * (nest_index % 3) as f32;
    (angle.cos() * radius, angle.sin() * radius)
}

/// Ring offset for a member inside its nest (zero for a single member).
fn member_offset(index: usize, count: usize) -> (f32, f32) {
    if count <= 1 {
        return (0.0, 0.0);
    }
    let angle = index as f32 / count as f32 * std::f32::consts::TAU;
    (angle.cos() * 1.1, angle.sin() * 1.1)
}

// ---------------------------------------------------------------- picking

/// A catalog part rewritten for species assembly: the fields generation
/// needs, flattened out of the full definition.
#[derive(Debug, Clone)]
struct PartCandidate {
    instance: u32,
    name: String,
    unlock_level: u32,
    herbivore: u8,
    carnivore: u8,
    social: u8,
    combat: u8,
    min_scale: f32,
    max_scale: f32,
    default_scale: f32,
}

/// Capability buckets over the non-null creature parts of a catalog. Each
/// bucket stays price-ascending so fallbacks land on the cheapest authored
/// part.
#[derive(Debug, Clone, Default)]
struct PartPool {
    mouths: Vec<PartCandidate>,
    eyes: Vec<PartCandidate>,
    feet: Vec<PartCandidate>,
    socials: Vec<PartCandidate>,
    combats: Vec<PartCandidate>,
}

impl PartPool {
    fn from_catalog(catalog: &PartCatalog) -> Self {
        let mut pool = Self::default();
        let mut ordered: Vec<&PartDefinition> =
            catalog.parts.iter().filter(|p| !p.is_null).collect();
        // Deterministic: group, then category, then price, then id.
        ordered.sort_by_key(|p| (p.key.group_id, p.category(), p.price, p.key.instance_id));
        for p in ordered {
            if p.key.group_id != CREATURE_PARTS {
                continue;
            }
            let a = p.abilities;
            if a.mouth > 0 {
                pool.mouths.push(PartCandidate::from(p));
            } else if a.eye > 0 {
                pool.eyes.push(PartCandidate::from(p));
            } else if a.foot > 0 {
                pool.feet.push(PartCandidate::from(p));
            } else if a.bite.max(a.charge).max(a.strike).max(a.spit) > 0 {
                pool.combats.push(PartCandidate::from(p));
            } else if a.sing.max(a.dance).max(a.charm).max(a.pose) > 0 {
                pool.socials.push(PartCandidate::from(p));
            }
        }
        pool
    }

    /// Digests a part's authored ranks into a slot-ready candidate.
    fn build_species(&self, seed: u64, phase: u64, index: u32) -> Option<Species> {
        let budget =
            (mix64(phase ^ ((index as u64) + 1) * 7919) % MAX_UNLOCK_TIER as u64 + 1) as u32;
        let size = spread(
            seed ^ 0x5125 ^ ((index as u64) << 32),
            MIN_SPECIES_SIZE,
            MAX_SPECIES_SIZE,
        );
        self.build_species_with(seed, index, target_diet(index), budget, size)
    }

    /// A species filling a retail herd archetype: the archetype's diet (social
    /// herds herbivores, combat herds carnivores), an unlock tier for its size
    /// class, its `OverrideSize` as the actor scale, and its hostility.
    fn build_species_for(
        &self,
        seed: u64,
        phase: u64,
        index: u32,
        archetype: Archetype,
    ) -> Option<Species> {
        let _ = phase;
        let record = archetype.record();
        let mut species = self.build_species_with(
            seed,
            index,
            record.diet,
            archetype.unlock_tier().min(MAX_UNLOCK_TIER),
            ecology::species_scale(archetype),
        )?;
        species.archetype = Some(archetype);
        species.hostile = archetype.hostile();
        species.tint = match record.diet {
            Diet::Carnivore => [0.75, 0.25, 0.25],
            Diet::Omnivore => [0.6, 0.6, 0.7],
            Diet::Herbivore => [0.45, 0.7, 0.35],
        };
        Some(species)
    }

    fn build_species_with(
        &self,
        seed: u64,
        index: u32,
        target: Diet,
        budget: u32,
        size: f32,
    ) -> Option<Species> {
        // Mouth first, aiming at this slot's planned diet. The mouth's own
        // authored ranks decide the diet a species actually gets, which is
        // what keeps herbivore mouths herbivores and carnivore mouths
        // carnivores; `DIET_PLAN` only steers *which* mouths are asked for.
        let mouth = self
            .eligible(&self.mouths, budget)
            .map(|list| choose_mouth(&list, target, seed ^ 0x00D1 ^ ((index as u64) << 3)));
        let (mouth, diet) = match mouth {
            Some(m) => (Some(m), Diet::from_mouth(m.herbivore, m.carnivore)),
            // No catalog (or no mouths at all): fall back to the planned
            // diet so the world still shows the full range of behaviour.
            None => (None, target),
        };
        let mut parts: Vec<PartInstance> = Vec::new();
        if let Some(m) = mouth {
            let key = seed ^ 0x0101 ^ ((index as u64) << 3);
            parts.push(mounted(m, 0, key));
        }
        // Mirrored eye and foot pairs (shared pair id, one side mirrored).
        if parts.len() + 2 <= MAX_PARTS_PER_SPECIES {
            if let Some(list) = self.eligible(&self.eyes, budget) {
                let c = list[pick_index(seed ^ 0x0202 ^ ((index as u64) << 3), list.len())];
                push_mirrored_pair(&mut parts, c, 1, 2, 1, seed ^ 0x7777, index);
            }
        }
        if parts.len() + 2 <= MAX_PARTS_PER_SPECIES {
            if let Some(list) = self.eligible(&self.feet, budget) {
                let c = list[pick_index(seed ^ 0x0303 ^ ((index as u64) << 3), list.len())];
                push_mirrored_pair(&mut parts, c, 3, 4, 2, seed ^ 0x8888, index);
            }
        }
        // One extra ability part, chosen by diet the way the original does:
        // carnivores bite, everyone else performs. Keeps builds distinct.
        let extra_key = seed ^ 0x0404 ^ ((index as u64) << 3);
        let preferred = match diet {
            Diet::Carnivore => &self.combats,
            _ => &self.socials,
        };
        let spare = match diet {
            Diet::Carnivore => &self.socials,
            _ => &self.combats,
        };
        let extra = self
            .eligible(preferred, budget)
            .or_else(|| self.eligible(spare, budget));
        if let Some(list) = extra {
            if parts.len() < MAX_PARTS_PER_SPECIES {
                // Highest authored rank wins (ties broken by the seed), so a
                // species' signature ability is its strongest one.
                let rank = if diet == Diet::Carnivore {
                    |c: &PartCandidate| c.combat
                } else {
                    |c: &PartCandidate| c.social
                };
                let c = pick_by_rank(&list, rank, extra_key);
                let mut p = mounted(c, 7, extra_key);
                p.yaw = spread(extra_key ^ 0x99, -0.4, 0.4);
                parts.push(p);
            }
        }
        let (tint, hostile) = match diet {
            Diet::Herbivore => ([0.35 + size * 0.1, 0.7, 0.35], false),
            Diet::Omnivore => ([0.6, 0.6, 0.7], false),
            Diet::Carnivore => ([0.75, 0.25, 0.25], true),
        };
        let name = mouth
            .map(|m| m.name.clone())
            .unwrap_or_else(|| format!("Species {}", index + 1));
        Some(Species {
            id: index,
            name,
            diet,
            tier: budget,
            size,
            hostile,
            tint,
            body: species_body(seed, index),
            parts,
            archetype: None,
            schedule: 0,
            evolution: 0,
        })
    }

    /// Candidates a tier may use, cheapest-first; the cheapest authored part
    /// of the bucket is the fallback when nothing is unlocked yet.
    fn eligible<'a>(
        &'a self,
        bucket: &'a [PartCandidate],
        tier: u32,
    ) -> Option<Vec<&'a PartCandidate>> {
        if bucket.is_empty() {
            return None;
        }
        let allowed: Vec<&PartCandidate> =
            bucket.iter().filter(|c| c.unlock_level <= tier).collect();
        if allowed.is_empty() {
            Some(vec![&bucket[0]])
        } else {
            Some(allowed)
        }
    }
}

impl From<&PartDefinition> for PartCandidate {
    fn from(p: &PartDefinition) -> Self {
        let a = p.abilities;
        Self {
            instance: p.model.instance_id,
            name: p.name.clone(),
            unlock_level: p.unlock_level,
            herbivore: a.herbivore,
            carnivore: a.carnivore,
            social: a.sing.max(a.dance).max(a.charm).max(a.pose),
            combat: a.bite.max(a.charge).max(a.strike).max(a.spit),
            min_scale: p.min_scale,
            max_scale: p.max_scale,
            default_scale: p.default_scale,
        }
    }
}

/// Deliberate diet plan for the species a world generates, in order. This is
/// an OpenSpore approximation of the original's ecology, not a verified rule:
/// slot 0/1 stay approachable so there is always someone to befriend, slot 2
/// brings a hunter, and later slots round the mix out.
pub const DIET_PLAN: [Diet; 4] = [
    Diet::Herbivore,
    Diet::Herbivore,
    Diet::Carnivore,
    Diet::Omnivore,
];

/// Diet fallback when no mouth is available, cycling the same mix.
const DIET_ROTATION: [Diet; 3] = [Diet::Herbivore, Diet::Carnivore, Diet::Omnivore];

/// Diet species `index` is steered toward before its mouth is chosen.
fn target_diet(index: u32) -> Diet {
    DIET_PLAN[(index as usize) % DIET_PLAN.len()]
}

/// Pick a mouth for a slot's planned diet: a herbivore slot takes a
/// herbivore-only mouth, a carnivore slot a carnivore-only one, and an
/// omnivore slot one that feeds both ways. When the tier reaches nothing that
/// exact, approachable slots still take a non-hostile mouth if one exists, so
/// the first species is always someone the player can befriend.
fn choose_mouth<'a>(list: &[&'a PartCandidate], target: Diet, key: u64) -> &'a PartCandidate {
    let compatible = |c: &PartCandidate| match target {
        Diet::Herbivore => c.herbivore > 0 && c.carnivore == 0,
        Diet::Carnivore => c.carnivore > 0 && c.herbivore == 0,
        Diet::Omnivore => c.herbivore > 0 && c.carnivore > 0,
    };
    let approaching = |c: &PartCandidate| match target {
        Diet::Carnivore => c.carnivore > 0,
        _ => c.herbivore > 0,
    };
    let exact: Vec<&PartCandidate> = list.iter().copied().filter(|c| compatible(c)).collect();
    let pool: Vec<&PartCandidate> = if exact.is_empty() {
        let approachable: Vec<&PartCandidate> =
            list.iter().copied().filter(|c| approaching(c)).collect();
        if approachable.is_empty() {
            list.iter().copied().collect()
        } else {
            approachable
        }
    } else {
        exact
    };
    pool[pick_index(key, pool.len())]
}

/// A freshly mounted part record with bounded scale and yaw.
fn mounted(c: &PartCandidate, bone: u32, key: u64) -> PartInstance {
    let mut p = PartInstance::new(c.instance, bone);
    p.scale = scale_bounded(c, key);
    p.yaw = 0.0;
    p
}

/// Bounded scale: the part's authored default, jittered and clamped into the
/// part's own min/max so no build can be degenerate or absurd.
fn scale_bounded(c: &PartCandidate, key: u64) -> f32 {
    let jitter = spread(key ^ 0x55AA, 0.85, 1.15);
    let raw = c.default_scale * jitter;
    if !raw.is_finite() {
        return 1.0;
    }
    let lo = c.min_scale.max(0.01);
    let hi = c.max_scale.max(lo);
    raw.clamp(lo, hi)
}

/// Push a left/right mirrored pair: same instance, shared pair id, the right
/// side mirrored exactly like `game::part_add_mirrored` does.
fn push_mirrored_pair(
    parts: &mut Vec<PartInstance>,
    c: &PartCandidate,
    left: u32,
    right: u32,
    pair_id: u32,
    seed: u64,
    index: u32,
) {
    if parts.len() + 2 > MAX_PARTS_PER_SPECIES {
        return;
    }
    let key = seed ^ 0x6666 ^ ((index as u64) << 3);
    let mut l = mounted(c, left, key);
    l.pair_id = pair_id;
    let mut r = mounted(c, right, key);
    r.pair_id = pair_id;
    r.mirrored = true;
    parts.push(l);
    parts.push(r);
}

/// Deterministic per-species body: the five-to-seven point spine layout the
/// editor uses, with seed-derived radii/offsets so distinct species differ.
/// Everything is finite by construction; `validated()` should pass it through
/// unchanged (asserted in tests).
fn species_body(seed: u64, index: u32) -> BodyProfile {
    let key = seed ^ 0xB0DD ^ ((index as u64) << 20);
    let points = 5 + (mix64(key) % 3) as usize; // 5..=7
    let bulk = spread(key ^ 0x1111, 0.85, 1.15);
    let mut spine = Vec::with_capacity(points);
    for i in 0..points {
        let t = i as f32 / (points - 1) as f32;
        // z from +0.85 (head) down to -1.0 (tail), like the default body.
        let z = 0.85 - 1.85 * t;
        let y = 0.05 + 0.6 * t + spread(key ^ i as u64, -0.05, 0.05);
        // Narrow head and tail, fat body: peak radius at the middle.
        let base = 0.25 + 0.47 * (1.0 - (2.0 * t - 1.0).abs()).powf(0.8);
        let radius = (base * bulk).clamp(0.08, 2.0);
        spine.push(crate::body::SpinePoint {
            position: [0.0, y, z],
            radius,
        });
    }
    let body = BodyProfile { spine };
    let validated = body.validated();
    // `validated` can only fall back when the construction is degenerate;
    // the layout above keeps consecutive points well apart, so this holds.
    if validated == body {
        body
    } else {
        validated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic catalog part with the authored ranks set explicitly.
    fn candidate(
        instance: u32,
        price: u32,
        unlock_level: u32,
        herbivore: u8,
        carnivore: u8,
        mouth: u8,
        eye: u8,
        foot: u8,
        social: u8,
        combat: u8,
    ) -> PartDefinition {
        PartDefinition {
            key: crate::dbpf::ResourceKey {
                type_id: crate::asset_catalog::PROP,
                group_id: CREATURE_PARTS,
                instance_id: instance,
            },
            model: crate::dbpf::ResourceKey {
                type_id: crate::asset_catalog::RW4,
                group_id: CREATURE_PARTS,
                instance_id: instance + 5000,
            },
            name: format!("part{instance}"),
            is_null: false,
            price,
            unlock_level,
            min_scale: 0.5,
            max_scale: 2.0,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities: Capabilities {
                herbivore,
                carnivore,
                mouth,
                eye,
                foot,
                sing: social,
                dance: social,
                charm: social,
                pose: social,
                bite: combat,
                charge: combat,
                strike: combat,
                spit: combat,
                ..Capabilities::default()
            },
        }
    }

    /// A small catalog: herbivore and carnivore mouths, eyes, feet, social and
    /// combat parts at authored prices, ranks and unlock levels.
    fn catalog() -> PartCatalog {
        PartCatalog {
            parts: vec![
                candidate(0x10, 40, 2, 3, 0, 2, 0, 0, 0, 0), // herbivore mouth
                candidate(0x11, 60, 3, 0, 2, 2, 0, 0, 0, 0), // carnivore mouth
                candidate(0x12, 20, 1, 2, 2, 1, 0, 0, 0, 0), // omnivore mouth
                candidate(0x20, 25, 1, 0, 0, 0, 2, 0, 0, 0), // eye
                candidate(0x21, 30, 2, 0, 0, 0, 3, 0, 0, 0), // eye
                candidate(0x30, 15, 1, 0, 0, 0, 0, 3, 0, 0), // foot
                candidate(0x31, 18, 1, 0, 0, 0, 0, 4, 0, 0), // foot
                candidate(0x40, 22, 1, 0, 0, 0, 0, 0, 4, 0), // social
                candidate(0x50, 28, 1, 0, 0, 0, 0, 0, 0, 4), // combat
            ],
            errors: Vec::new(),
        }
    }

    fn empty_catalog() -> PartCatalog {
        PartCatalog {
            parts: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn world_is_reproducible_and_distinct_per_seed() {
        let a = SpeciesWorld::generate(7, &catalog(), 24.0);
        let b = SpeciesWorld::generate(7, &catalog(), 24.0);
        assert_eq!(a, b);
        let c = SpeciesWorld::generate(8, &catalog(), 24.0);
        assert_ne!(a, c, "different seeds must produce different worlds");
    }

    /// A legacy (pre-archetype) world with the old default species count.
    fn legacy(seed: u64, radius: f32) -> SpeciesWorld {
        SpeciesWorld::generate_n(seed, &catalog(), radius, default_species_count(seed))
    }

    #[test]
    fn generates_at_least_three_distinct_species() {
        for seed in 0..12u64 {
            let world = legacy(seed, 24.0);
            assert!(world.species.len() >= 3, "seed {seed}");
            for s in &world.species {
                assert!(!s.parts.is_empty(), "species {} has parts", s.id);
            }
            // Distinct bodies and builds, not copies of one template.
            for i in 0..world.species.len() {
                for j in (i + 1)..world.species.len() {
                    let (a, b) = (&world.species[i], &world.species[j]);
                    assert_ne!(a.body, b.body);
                    assert_ne!(a.parts, b.parts);
                }
            }
        }
    }

    #[test]
    fn every_species_owns_a_populated_nest_for_any_count_or_seed() {
        // Early species used to be able to spend the whole member budget and
        // leave the last species with nothing to spawn.
        for seed in 0..40u64 {
            for count in [MIN_DEFAULT_SPECIES, MAX_DEFAULT_SPECIES, MAX_SPECIES] {
                let world = SpeciesWorld::generate_n(seed, &catalog(), 24.0, count);
                assert_eq!(world.species.len(), count, "seed {seed} count {count}");
                assert!(world.total_members() <= MAX_TOTAL_MEMBERS);
                for species in &world.species {
                    let nests = world.nests_of_species(species.id);
                    assert!(
                        !nests.is_empty(),
                        "seed {seed} species {} has no nest",
                        species.id
                    );
                    assert!(
                        nests
                            .iter()
                            .any(|n| n.population() >= MIN_MEMBERS_PER_NEST as u32),
                        "seed {seed} species {} has no populated nest",
                        species.id
                    );
                    assert!(world.species_counts(species.id).alive() > 0);
                }
            }
        }
    }

    #[test]
    fn member_ids_start_at_the_persistent_base_and_stay_unique() {
        for seed in 0..12u64 {
            let world = SpeciesWorld::generate(seed, &catalog(), 24.0);
            let ids: Vec<u32> = world.spawn_list().iter().map(|s| s.member).collect();
            assert_eq!(
                ids[0], MEMBER_ID_BASE,
                "headless social play uses the first id"
            );
            // The original six members keep their ids when the world is that
            // small, so existing saves and tests stay meaningful.
            let small = SpeciesWorld::generate_n(seed, &catalog(), 24.0, 1);
            let small_ids: Vec<u32> = small.spawn_list().iter().map(|s| s.member).collect();
            assert!(small_ids
                .iter()
                .enumerate()
                .all(|(i, id)| *id == MEMBER_ID_BASE + i as u32));
            assert_eq!(ids.len(), world.total_members());
            let unique: std::collections::HashSet<u32> = ids.iter().copied().collect();
            assert_eq!(unique.len(), ids.len(), "seed {seed}");
        }
    }

    #[test]
    fn diet_plan_keeps_the_first_species_approachable() {
        // Social play needs someone friendly early; a hunter follows.
        let mut hunters = 0;
        for seed in 0..20u64 {
            let world = SpeciesWorld::generate(seed, &catalog(), 24.0);
            assert!(!world.species[0].hostile, "seed {seed}");
            assert!(world.species.len() >= 2);
            assert!(!world.species[1].hostile, "seed {seed}");
            hunters += world.species.iter().filter(|s| s.hostile).count();
        }
        assert!(hunters > 0, "the diet plan must still produce hunters");
    }

    #[test]
    fn mouths_are_selected_for_the_planned_diet() {
        let catalog = catalog();
        for seed in 0..20u64 {
            let world = SpeciesWorld::generate(seed, &catalog, 24.0);
            for species in &world.species {
                let Some(mouth) = species.parts.first() else {
                    continue;
                };
                let part = catalog.by_model(mouth.instance).expect("catalog mouth");
                let (herb, carn) = (part.abilities.herbivore, part.abilities.carnivore);
                match species.diet {
                    Diet::Herbivore => assert!(herb > 0 && carn == 0, "seed {seed}"),
                    Diet::Carnivore => assert!(carn > 0 && herb == 0, "seed {seed}"),
                    Diet::Omnivore => assert!(herb > 0 && carn > 0, "seed {seed}"),
                }
                // A mouth only ever says one of these; never a wild diet.
                assert!(species.diet != Diet::Carnivore || species.hostile);
                assert!(species.diet != Diet::Herbivore || !species.hostile);
            }
        }
    }

    #[test]
    fn diet_follows_the_authored_mouth_ranks() {
        let catalog = catalog();
        let world = SpeciesWorld::generate(3, &catalog, 20.0);
        for s in &world.species {
            let Some(mouth) = s.parts.first() else {
                // Fallback builds still carry a usable diet.
                assert!(matches!(
                    s.diet,
                    Diet::Herbivore | Diet::Carnivore | Diet::Omnivore
                ));
                continue;
            };
            assert_eq!(mouth.bone, 0, "mouth mounts on the head slot");
            // The diet is exactly the mouth's authored ranks, so a herbivore
            // mouth never yields a carnivore species (and vice versa).
            let part = catalog.by_model(mouth.instance).expect("catalog mouth");
            assert_eq!(
                s.diet,
                Diet::from_mouth(part.abilities.herbivore, part.abilities.carnivore)
            );
        }
    }

    #[test]
    fn eyes_and_feet_are_mirrored_pairs() {
        let world = SpeciesWorld::generate(11, &catalog(), 24.0);
        for s in &world.species {
            let left_eye = s.parts.iter().find(|p| p.bone == 1);
            let right_eye = s.parts.iter().find(|p| p.bone == 2);
            let left_foot = s.parts.iter().find(|p| p.bone == 3);
            let right_foot = s.parts.iter().find(|p| p.bone == 4);
            let pairs = [(left_eye, right_eye), (left_foot, right_foot)];
            for (l, r) in pairs {
                let (Some(l), Some(r)) = (l, r) else {
                    continue;
                };
                assert_eq!(l.instance, r.instance, "paired slots share the mesh");
                assert_eq!(l.pair_id, r.pair_id);
                assert!(l.pair_id > 0);
                assert!(!l.mirrored);
                assert!(r.mirrored, "the right side is mirrored");
            }
        }
    }

    #[test]
    fn builds_are_finite_and_validated() {
        let world = SpeciesWorld::generate(5, &catalog(), 24.0);
        for s in world.species.iter().chain(legacy(5, 24.0).species.iter()) {
            match s.archetype {
                // Retail herds are drawn at their archetype's size.
                Some(archetype) => assert_eq!(s.size, ecology::species_scale(archetype)),
                None => assert!(
                    s.size.is_finite() && s.size >= MIN_SPECIES_SIZE && s.size <= MAX_SPECIES_SIZE
                ),
            }
            assert_eq!(s.body, s.body.validated());
            assert!(s.parts.len() <= MAX_PARTS_PER_SPECIES);
            assert!(s.tier >= 1 && s.tier <= MAX_UNLOCK_TIER);
            for p in &s.parts {
                assert!(p.scale.is_finite() && p.scale > 0.0);
                assert!(p.yaw.is_finite());
                assert!(p.offset.iter().all(|v| v.is_finite()));
            }
            assert!(s.tint.iter().all(|v| *v >= 0.0 && *v <= 1.0));
        }
    }

    #[test]
    fn fallback_builds_when_the_catalog_is_absent() {
        let world = SpeciesWorld::generate(11, &empty_catalog(), 24.0);
        assert!(world.generated);
        assert!(world.species.len() >= 3);
        let diets: Vec<_> = world.species.iter().map(|s| s.diet).collect();
        assert!(diets.contains(&Diet::Herbivore));
        assert!(diets.contains(&Diet::Carnivore));
        for s in &world.species {
            // No parts: the spawner uses the procedural body alone.
            assert!(s.parts.is_empty());
            assert_eq!(s.body, s.body.validated());
        }
        assert_eq!(world.total_members(), world.spawn_list().len());
    }

    #[test]
    fn nests_have_positions_and_members() {
        let world = legacy(11, 24.0);
        assert!(world.total_members() <= MAX_TOTAL_MEMBERS);
        assert!(world.total_members() >= 3 * MIN_MEMBERS_PER_NEST);
        let mut ids = std::collections::HashSet::new();
        for nest in &world.nests {
            assert!(nest.population() >= MIN_MEMBERS_PER_NEST as u32);
            assert!(nest.position.iter().all(|v| v.is_finite()));
            assert!(world.species(nest.species).is_some());
            for m in &nest.members {
                assert!(ids.insert(m.id), "member ids are unique");
            }
        }
    }

    #[test]
    fn nearest_nest_finds_the_closest_one() {
        let world = SpeciesWorld::generate(11, &catalog(), 24.0);
        let first = &world.nests[0];
        let p = [first.position[0], 0.0, first.position[2]];
        let nearest = world.nearest_nest(p).expect("a nest exists");
        assert_eq!(nearest.position, first.position);
        let far = world.nearest_nest([0.0, 0.0, 0.0]);
        assert!(far.is_some());
    }

    #[test]
    fn outcomes_record_exactly_once() {
        let mut world = SpeciesWorld::generate(11, &catalog(), 24.0);
        let first = world.spawn_list()[0].member;
        assert_eq!(world.relationship(first), Some(Relationship::Neutral));
        assert_eq!(world.record_befriend(first), RecordResult::Recorded);
        assert_eq!(world.record_befriend(first), RecordResult::Already);
        assert_eq!(world.relationship(first), Some(Relationship::Allied));
        assert_eq!(world.record_defeat(first), RecordResult::Recorded);
        assert_eq!(world.record_defeat(first), RecordResult::Already);
        assert_eq!(world.relationship(first), Some(Relationship::Defeated));
        assert_eq!(world.record_befriend(first), RecordResult::Already);
        assert!(world
            .record_befriend(0xDEAD)
            .eq(&RecordResult::UnknownMember));
        assert!(world.record_defeat(0xDEAD).eq(&RecordResult::UnknownMember));
    }

    #[test]
    fn alliance_and_extinction_thresholds() {
        let mut world = legacy(11, 24.0);
        let species = world.species[0].id;
        let members: Vec<u32> = world.nests_of_species(species)[0]
            .members
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(world.surviving_members(species), members.len() as u32);
        // Existing saved thresholds remain honored, while fresh worlds need
        // three distinct allies, bounded by the entire species population.
        world.alliance_threshold = 1;
        assert_eq!(world.record_befriend(members[0]), RecordResult::Recorded);
        assert!(world.species_allied(species));
        // Extinction only when every member is defeated.
        assert!(!world.species_extinct(species));
        for id in &members {
            world.record_defeat(*id);
        }
        assert_eq!(world.surviving_members(species), 0);
        assert!(world.species_extinct(species));
        // A fractional threshold flags extinction earlier.
        world.extinction_threshold = 2;
        let other = world.species[1].id;
        let others: Vec<u32> = world.nests_of_species(other)[0]
            .members
            .iter()
            .map(|m| m.id)
            .collect();
        assert!(!world.species_extinct(other));
        world.record_defeat(others[0]);
        world.record_defeat(others[1]);
        assert!(world.species_extinct(other));
        assert_eq!(world.surviving_members(other), others.len() as u32 - 2);
    }

    #[test]
    fn fresh_alliance_requires_distinct_members_and_attack_keeps_reward_history() {
        let mut world = SpeciesWorld::generate(11, &catalog(), 24.0);
        let species = world.species[0].id;
        let members: Vec<u32> = world
            .spawn_list()
            .iter()
            .filter(|m| m.species == species)
            .map(|m| m.member)
            .collect();
        let needed = world.alliance_required(species);
        assert_eq!(needed, 3.min(members.len() as u32));
        for (i, member) in members.iter().take(needed as usize).enumerate() {
            assert_eq!(world.record_befriend(*member), RecordResult::Recorded);
            assert_eq!(world.record_befriend(*member), RecordResult::Already);
            assert_eq!(world.species_allied(species), i + 1 == needed as usize);
        }
        assert_eq!(world.record_attack(members[0]), RecordResult::Recorded);
        assert_eq!(world.record_attack(members[0]), RecordResult::Already);
        assert!(!world.species_allied(species));
        assert!(world.species_aggravated(species));
        assert_eq!(world.record_befriend(members[0]), RecordResult::Already);
        let back: SpeciesWorld =
            serde_json::from_str(&serde_json::to_string(&world).unwrap()).unwrap();
        assert!(back.species_aggravated(species));
        assert_eq!(back.relationship(members[0]), Some(Relationship::Allied));
    }

    #[test]
    fn outcomes_and_builds_survive_serialization() {
        let mut world = SpeciesWorld::generate(11, &catalog(), 24.0);
        let first = world.spawn_list()[0].member;
        world.record_befriend(first);
        world.record_defeat(world.spawn_list()[1].member);
        let json = serde_json::to_string(&world).expect("serializes");
        let back: SpeciesWorld = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(world, back);
        // Old saves without the field still load (the type defaults).
        let empty: SpeciesWorld = serde_json::from_str("{}").expect("default");
        assert!(!empty.generated);
        assert!(empty.species.is_empty());
        assert_eq!(empty.alliance_threshold, DEFAULT_ALLIANCE_THRESHOLD);
    }

    #[test]
    fn world_state_is_stable_across_regeneration_calls() {
        let mut world = SpeciesWorld::default();
        assert!(!world.generated);
        world.ensure_generated(21, &catalog(), 24.0);
        assert!(world.generated);
        let once = world.clone();
        // Ensuring again must not wipe a generated world.
        world.ensure_generated(22, &catalog(), 24.0);
        assert_eq!(world, once);
        world.new_world(23, &catalog(), 24.0);
        assert_ne!(world, once);
    }

    #[test]
    fn members_can_be_resolved_back_to_their_species() {
        let world = SpeciesWorld::generate(11, &catalog(), 24.0);
        for spawn in world.spawn_list() {
            assert_eq!(
                world.nest_for_member(spawn.member).unwrap().species,
                spawn.species
            );
            assert_eq!(
                world.species_for_member(spawn.member).unwrap().id,
                spawn.species
            );
            let position = world.member_position(spawn.member).expect("position");
            assert!(position.iter().all(|v| v.is_finite()));
            assert_eq!(
                distance(position, spawn.position),
                0.0,
                "position matches the spawn list"
            );
        }
        assert!(world.species_for_member(0xDEAD).is_none());
        assert!(world.member_position(0xDEAD).is_none());
    }

    fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn diet_labels_and_counts_are_consistent() {
        assert_eq!(Diet::Herbivore.label(), "Herbivore");
        assert_eq!(Diet::Omnivore.label(), "Omnivore");
        assert_eq!(Diet::Carnivore.label(), "Carnivore");
        assert_eq!(Diet::from_mouth(2, 0), Diet::Herbivore);
        assert_eq!(Diet::from_mouth(0, 2), Diet::Carnivore);
        assert_eq!(Diet::from_mouth(1, 1), Diet::Omnivore);
        assert_eq!(Diet::from_mouth(0, 0), Diet::Omnivore);
        assert!(Relationship::Defeated.is_alive() == false);
        assert!(Relationship::Allied.is_alive());
        let mut c = RelationshipCounts::default();
        c.add(Relationship::Neutral);
        c.add(Relationship::Allied);
        c.add(Relationship::Defeated);
        assert_eq!(c.total(), 3);
        assert_eq!(c.alive(), 2);
        assert_eq!(c.count(Relationship::Allied), 1);
    }

    #[test]
    fn requested_species_counts_stay_in_range_and_keep_hunters() {
        for seed in [11u64, 29, 77] {
            for count in [MIN_DEFAULT_SPECIES, MAX_DEFAULT_SPECIES, MAX_SPECIES] {
                let world = SpeciesWorld::generate_n(seed, &catalog(), 24.0, count);
                assert_eq!(world.species.len(), count);
                assert!(
                    !world.species[0].hostile,
                    "the starter species stays friendly"
                );
                // Only a minority hunts, so the world is mostly approachable.
                assert!(world.species.iter().filter(|s| s.hostile).count() <= count / 2);
            }
            // Out-of-range requests clamp instead of panicking.
            let world = SpeciesWorld::generate_n(seed, &catalog(), 24.0, 99);
            assert_eq!(world.species.len(), MAX_SPECIES);
        }
    }

    #[test]
    fn lineage_seed_depends_on_the_build_but_is_reproducible() {
        let mut run = crate::game::RunProgress::default();
        let mut world = crate::species::SpeciesWorld::default();
        let plain = lineage_seed(&run.parts, &run.body);
        assert_eq!(plain, lineage_seed(&run.parts, &run.body));
        run.parts.push(crate::game::PartInstance::new(7, 1));
        assert_ne!(plain, lineage_seed(&run.parts, &run.body));
        // A body change alone also reshapes the ecology.
        run.body.spine[2].radius = 1.4;
        assert_ne!(lineage_seed(&run.parts, &run.body), {
            let mut clone = run.body.clone();
            clone.spine[2].radius = 0.7;
            lineage_seed(&run.parts, &clone)
        });
        assert_eq!(
            lineage_seed(&run.parts, &run.body),
            lineage_seed(&run.parts, &run.body)
        );
        // The seed drive a reproducible world, not a panic.
        world.ensure_generated(plain, &catalog(), DEFAULT_NEST_RADIUS);
        assert!(world.generated && !world.species.is_empty());
        let again = SpeciesWorld::generate(plain, &catalog(), DEFAULT_NEST_RADIUS);
        assert_eq!(world, again);
    }

    // ------------------------------------------------------------ retail ecology

    fn herd_of(world: &SpeciesWorld, archetype: Archetype) -> (u32, Vec<u32>) {
        let species = world
            .species
            .iter()
            .find(|s| s.archetype == Some(archetype))
            .expect("herd")
            .id;
        let members = world.nests_of_species(species)[0]
            .members
            .iter()
            .map(|m| m.id)
            .collect();
        (species, members)
    }

    #[test]
    fn a_fresh_world_is_the_retail_generation_list() {
        for seed in [3u64, 11, 77, 4242] {
            let world = SpeciesWorld::generate(seed, &catalog(), DEFAULT_NEST_RADIUS);
            assert!(world.is_retail());
            let plan = ecology::plan_population(seed);
            let herds = plan.iter().filter(|h| h.archetype.is_herd()).count();
            // One species (and one nest) per herd, plus the night predators.
            assert_eq!(world.species.len(), herds + 1, "seed {seed}");
            assert_eq!(world.nests.len(), world.species.len());
            assert!(world.carcass.is_some(), "the Carcass lies near home");
            let count = |a: Archetype| world.species.iter().filter(|s| s.archetype == Some(a)).count();
            assert_eq!(count(Archetype::Social), 5);
            assert_eq!(count(Archetype::Social2), 2);
            assert_eq!(count(Archetype::Combat), 3);
            assert_eq!(count(Archetype::SmallSocial2), 4);
            assert_eq!(count(Archetype::SmallCombat2), 3);
            assert_eq!(count(Archetype::MediumSocial2), 2);
            assert_eq!(count(Archetype::MediumCombat2), 1);
            assert_eq!(count(Archetype::LargeSocial2), 1);
            assert_eq!(count(Archetype::NightPredator), 1);
            assert!(world.total_members() >= 115, "seed {seed}: {}", world.total_members());
            for nest in &world.nests {
                let archetype = world.archetype_of(nest.species).unwrap();
                assert_eq!(nest.members.len() as u32, nest.herd_size);
                assert_eq!(nest.herd_size, archetype.record().num_creatures.0 as u32);
                let alphas = nest.members.iter().filter(|m| m.role == MemberRole::Alpha).count();
                assert_eq!(alphas, ecology::alpha_count(archetype, nest.herd_size) as usize);
                let species = world.species(nest.species).unwrap();
                assert_eq!(species.hostile, archetype.hostile());
                // Retail bands: no nest beyond the farthest band (350) plus
                // the widened fallback.
                let d = dist_xz(nest.position, [ecology::HOME[0], 0.0, ecology::HOME[1]]);
                assert!(d <= 350.0 * 1.5 + 1.0, "{d}");
            }
            // The starter herds are approachable herbivores near home.
            assert_eq!(world.species[0].archetype, Some(Archetype::Social));
            assert!(!world.species[0].hostile);
            assert_eq!(world.spawn_list()[0].member, MEMBER_ID_BASE);
            // Roamers sit at their archetype bands with the retail sizes.
            assert_eq!(world.roamers.len(), EPIC_COUNT + ROGUE_COUNT);
            for roamer in &world.roamers {
                let d = dist_xz(roamer.lair, [ecology::HOME[0], 0.0, ecology::HOME[1]]);
                assert!(d >= ecology::HOME_CLEARANCE);
                assert_eq!(roamer.scale, ecology::species_scale(roamer.kind.archetype()));
                assert!(d >= ecology::casting_thresholds().0 - 1e-3, "lair {d} too close to home");
            }
            let back: SpeciesWorld = serde_json::from_str(&serde_json::to_string(&world).unwrap()).unwrap();
            assert_eq!(back, world);
        }
    }

    #[test]
    fn retail_herds_ally_at_best_friends_and_die_out_at_their_kill_count() {
        let mut world = SpeciesWorld::generate(11, &catalog(), DEFAULT_NEST_RADIUS);
        let (species, members) = herd_of(&world, Archetype::Social);
        assert_eq!(members.len(), 4);
        assert_eq!(world.alliance_required(species), 3, "NumToImpressForBestFriends");
        world.record_befriend(members[0]);
        assert!(!world.species_friendly(species));
        world.record_befriend(members[1]);
        assert!(world.species_friendly(species), "friendly at 2");
        assert!(!world.species_allied(species));
        assert!(!world.member_friendly(members[3]));
        world.record_befriend(members[2]);
        assert!(world.species_allied(species), "best friends at 3");
        // The whole herd turns friendly and its nest is an allied nest.
        assert!(world.member_friendly(members[3]));
        let nest = world.nest_index_for_member(members[0]).unwrap();
        assert_eq!(world.nest_status(nest), NestStatus::Allied);
        assert_eq!(world.claim_nest(nest), Some(NestStatus::Allied));
        assert_eq!(world.claim_nest(nest), None);

        // NPC_Combat: extinct after 3 of its 4 members are killed.
        let (combat, fighters) = herd_of(&world, Archetype::Combat);
        world.record_defeat(fighters[0]);
        world.record_defeat(fighters[1]);
        assert!(!world.species_extinct(combat));
        world.record_defeat(fighters[2]);
        assert!(world.species_extinct(combat), "NumToKillForExtinction 3");
        let nest = world.nest_index_for_member(fighters[0]).unwrap();
        assert_eq!(world.nest_status(nest), NestStatus::Destroyed);
        // A large herd needs seven kills.
        let (large, herd) = herd_of(&world, Archetype::LargeSocial2);
        for id in herd.iter().take(6) {
            world.record_defeat(*id);
        }
        assert!(!world.species_extinct(large));
        world.record_defeat(herd[6]);
        assert!(world.species_extinct(large));
        // Legacy species keep the saved thresholds.
        let mut old = legacy(11, 24.0);
        let s = old.species[0].id;
        assert_eq!(old.alliance_required(s), 3.min(old.species_counts(s).total()));
        old.alliance_threshold = 1;
        assert_eq!(old.alliance_required(s), 1);
    }

    #[test]
    fn lost_herd_members_hatch_again_until_the_species_is_extinct() {
        let mut world = SpeciesWorld::generate(11, &catalog(), DEFAULT_NEST_RADIUS);
        let (species, members) = herd_of(&world, Archetype::Social);
        world.record_defeat(members[0]);
        let rate = Archetype::Social.record().respawn_rate;
        assert!(world.tick_respawn(rate - 1.0).is_empty());
        let born = world.tick_respawn(1.0);
        assert_eq!(born.len(), 1, "one replacement after RespawnRate");
        let new = born[0];
        assert!(new > members[3] && new < ROAMER_ID_BASE);
        assert_eq!(world.nest_for_member(new).unwrap().species, species);
        assert_eq!(world.relationship(new), Some(Relationship::Neutral));
        assert!(world.tick_respawn(rate * 2.0).is_empty(), "the herd is full again");
        // Kills still count toward extinction across replacements.
        world.record_defeat(members[1]);
        world.record_defeat(new);
        assert!(world.species_extinct(species));
        assert!(world.tick_respawn(rate * 5.0).is_empty(), "an extinct species never refills");
        // Night predators (RespawnRate 100000) never come back within a visit.
        let (_, night) = herd_of(&world, Archetype::NightPredator);
        world.record_defeat(night[0]);
        assert!(world.tick_respawn(1000.0).iter().all(|id| !world
            .nest_for_member(*id)
            .is_some_and(|n| world.archetype_of(n.species) == Some(Archetype::NightPredator))));
    }

    #[test]
    fn a_herd_lays_up_to_two_eggs_per_wave_less_the_ones_it_has() {
        // 0xC6DE5C: `min(shortfall, 2) - eggs` replacements once `RespawnRate` has run.
        let mut world = SpeciesWorld::generate(11, &catalog(), DEFAULT_NEST_RADIUS);
        // Any herd that can lose three members and live on (more kills to extinction).
        let species = world
            .species
            .iter()
            .filter_map(|s| s.archetype.map(|a| (s.id, a)))
            .find(|(id, a)| {
                let nest = world.nests_of_species(*id)[0];
                nest.herd_size > 3 && a.record().kills_to_extinction > 3 && a.record().respawn_rate < 1000.0
            })
            .expect("a herd that survives three losses");
        let (species, archetype) = species;
        let members: Vec<u32> = world.nests_of_species(species)[0].members.iter().map(|m| m.id).collect();
        let index = world.nests.iter().position(|n| n.species == species).unwrap();
        for id in &members[..3] {
            world.record_defeat(*id);
        }
        let rate = archetype.record().respawn_rate;
        assert!(world.tick_respawn_eggs(rate - 1.0, |_| 0, 2).is_empty());
        let born = world.tick_respawn_eggs(1.0, |_| 0, 2);
        assert_eq!(born.len(), 2, "two eggs at once for a shortfall of three");
        // Both are incubating: the third missing member waits for an egg slot.
        assert!(world.tick_respawn_eggs(rate * 2.0, |i| if i == index { 2 } else { 0 }, 2).is_empty());
        // One hatched (no longer counted): the next wave lays one.
        let again = world.tick_respawn_eggs(rate * 2.0, |i| if i == index { 1 } else { 0 }, 2);
        assert_eq!(again.len(), 1);
        // The plain `tick_respawn` still replaces one member per rate.
        let mut plain = SpeciesWorld::generate(11, &catalog(), DEFAULT_NEST_RADIUS);
        for id in &members[..3] {
            plain.record_defeat(*id);
        }
        assert_eq!(plain.tick_respawn(rate).len(), 1);
    }

    #[test]
    fn the_epic_predator_returns_and_the_miniboss_stays_dead() {
        let mut world = SpeciesWorld::generate(11, &catalog(), DEFAULT_NEST_RADIUS);
        let epic = world.roamers.iter().find(|r| r.kind == RoamerKind::Epic).unwrap().id;
        let rogue = world.roamers.iter().find(|r| r.kind == RoamerKind::Rogue).unwrap().id;
        world.record_roamer(epic, Relationship::Defeated);
        world.record_roamer(rogue, Relationship::Defeated);
        assert!(world.tick_roamer_respawns(599.0).is_empty());
        assert_eq!(world.tick_roamer_respawns(1.0), vec![epic], "RespawnRate 600, never extinct");
        assert_eq!(world.roamer(epic).unwrap().relationship, Relationship::Neutral);
        assert!(world.tick_roamer_respawns(10_000.0).is_empty());
        assert_eq!(world.roamer(rogue).unwrap().relationship, Relationship::Defeated);
        assert_eq!(world.archetype_for(epic), Some(Archetype::EpicPredator));
        assert_eq!(world.archetype_for(rogue), Some(Archetype::MiniBoss));
        assert_eq!(legacy(11, 24.0).archetype_for(ROAMER_ID_BASE), None);
    }

    #[test]
    fn the_relationship_record_follows_the_event_history() {
        use crate::creature_rules::relationship::*;
        // Thresholds of a social herd: curious 1, friendly 2, best friends 3, fearful at 3 kills.
        let t = [1, 2, 3, 3];
        let record = |relationship| SpeciesRelation { species: 0, relationship, times_friendly: 0, impressions: 0, kills: 0 };
        // Kills then a befriend: the befriend clears the kill count (0xBA4480), so two more
        // kills do not make the species fearful; counts alone would have said so.
        let mut r = record(CURIOUS);
        r.apply(RelationEvent::Kill, t);
        r.apply(RelationEvent::Kill, t);
        r.apply(RelationEvent::Befriend, t);
        assert_eq!((r.relationship, r.kills, r.impressions), (CURIOUS, 0, 1));
        r.apply(RelationEvent::Kill, t);
        r.apply(RelationEvent::Kill, t);
        assert_eq!(r.relationship, CURIOUS);
        r.apply(RelationEvent::Kill, t);
        assert_eq!(r.relationship, FEARFUL, "three kills since the last befriend");
        // Friendly twice, then angered: hostile instead of hated (0xBA43C0 > 1).
        let mut r = record(CURIOUS);
        r.apply(RelationEvent::Befriend, t);
        r.apply(RelationEvent::Befriend, t);
        assert_eq!((r.relationship, r.times_friendly), (FRIENDLY, 1));
        r.apply(RelationEvent::Kill, t);
        r.apply(RelationEvent::Kill, t);
        r.apply(RelationEvent::Kill, t);
        assert_eq!(r.relationship, FEARFUL);
        r.apply(RelationEvent::Befriend, t);
        r.apply(RelationEvent::Befriend, t);
        assert_eq!(r.relationship, BEST_FRIENDS, "three impressions");
        // An attack makes a normal species hated and a hostile one stays hostile.
        let mut r = record(CURIOUS);
        r.apply(RelationEvent::Attack, t);
        assert_eq!(r.relationship, WARY);
        let mut r = record(HOSTILE);
        r.apply(RelationEvent::Attack, t);
        assert_eq!(r.relationship, HOSTILE);
    }

    #[test]
    fn a_world_keeps_a_relationship_record_from_the_first_event() {
        use crate::creature_rules::relationship::*;
        let mut world = SpeciesWorld::generate(11, &catalog(), DEFAULT_NEST_RADIUS);
        let species = world.species.iter().find(|s| s.archetype.is_some_and(|a| a.record().initial_relationship == CURIOUS)).unwrap().id;
        let member = world.nests.iter().find(|n| n.species == species).unwrap().members[0].id;
        assert!(world.relation(species).is_none());
        assert_eq!(crate::creature_actions::species_relationship_of(&world, species), Some(CURIOUS));
        assert_eq!(world.record_attack(member), RecordResult::Recorded);
        assert_eq!(world.relation(species).unwrap().relationship, WARY, "an attacked species is hated");
        assert_eq!(crate::creature_actions::species_relationship_of(&world, species), Some(WARY));
        assert!(crate::creature_rules::attacks_on_sight(WARY));
        // The record survives a save.
        let back: SpeciesWorld = serde_json::from_str(&serde_json::to_string(&world).unwrap()).unwrap();
        assert_eq!(back.relation(species), world.relation(species));
        // A save without records reads from the counts as before.
        let mut old = world.clone();
        old.relations.clear();
        assert_eq!(crate::creature_actions::species_relationship_of(&old, species), Some(CURIOUS));
    }

    #[test]
    fn saves_from_before_the_retail_ecology_still_load() {
        let old = r#"{"seed":5,"radius":24.0,"generated":true,"species":[],
            "nests":[{"species":0,"position":[1.0,0.4,2.0],"members":[{"id":300,"relationship":"Allied"}]}],
            "roamers":[{"id":1300,"kind":"Epic","lair":[0.0,0.4,-19.0],"visual":40,"scale":3.5,"name":"Epic creature","relationship":"Neutral"}]}"#;
        let world: SpeciesWorld = serde_json::from_str(old).unwrap();
        assert_eq!(world.nests[0].members[0].role, MemberRole::Adult);
        assert_eq!(world.nests[0].herd_size, 0);
        assert_eq!(world.time_of_day, ecology::START_TIME_OF_DAY);
        assert_eq!(world.roamers[0].respawn_timer, 0.0);
        assert!(!world.is_retail());
    }
}
