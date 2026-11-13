//! The universe simulation of the Space stage: NPC empires that grow and talk
//! to the player, raids and attack waves on the player's colonies.
//!
//! Sources (`notes\space-comm-notes.md` U1-U4): UniverseSimulatorTuning
//! (timers, awareness), spacenpcai (growth tick and action rules), the NPC
//! action executor 0xFE9410 (VERIFIED: "nice" sends `offer_gift` below the
//! alliance threshold and `offer_alliance` at it, `welcome_to_neighborhood`
//! to an empire that has not met the player; "nasty" demands tribute above the
//! war threshold, declares war at or below it, `warn_against_trespass` before
//! contact), the incoming conversations (`incoming_messages`, `event_raid*`).
//! What the code does not give is marked ADAPTED.

use std::collections::{BTreeMap, BTreeSet};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    game::RunProgress,
    space::{CityBuilding, DistressKind, SpaceCampaign, TributeDemand},
    space_comm::{dialog, EVENT_GROB_TEASER_RAID_FILE, EVENT_GROB_WAR_RAID_FILE, EVENT_RAID_ALLY_FILE, EVENT_RAID_FILE, EVENT_RAID_WAR_FILE, INCOMING_FILE},
    space_galaxy::{EmpireId, Galaxy},
    space_rules::{self as rules, RelationshipEvent},
    space_worldgen::StarKind,
    stages::GameStage,
};

/// spacenpcai `minTimeEGS` (5) in seconds: the NPC update 0xFEA3A0 compares
/// it x 60000 ms (minutes, VERIFIED) with the time spent outside the galaxy
/// view when the player enters it. (`tickFrequencyEGS` and
/// `maxProbabilityEGS` are in the data but no code reads them, VERIFIED: the
/// property ids 0x0429D67B / 0x0429D6AE occur nowhere in the code.)
pub const NPC_TURN_MIN_TIME: f32 = 5.0 * 60.0;
/// spacenpcai `maxTimeEGS` (10) in seconds: a turn at the latest this long
/// after the previous one (0xFEA3A0).
pub const NPC_TURN_MAX_TIME: f32 = 10.0 * 60.0;
/// 0xFEA3A0: the empires owning an Empire-tech star (not an outpost) within 30
/// units of the active star take the turn (FindStars, techLevels 0x20).
pub const NPC_TURN_RADIUS: f32 = 30.0;
/// 0xFE8AE0: an empire's candidate stars lie within 4 units of its stars.
pub const NPC_CANDIDATE_RADIUS: f32 = 4.0;
/// 0xFE8E00: the Empire-tech neighbours of a candidate within 2 units.
pub const NPC_NEIGHBOUR_RADIUS: f32 = 2.0;
/// spacenpcai `empireRadiusMean` (galaxy units).
pub const EMPIRE_RADIUS: f32 = 20.0;
/// UniverseSimulatorTuning `recentDeathConversationDuration` (s).
pub const RECENT_DEATH_CONVERSATION: f32 = 120.0;
/// ADAPTED: how long an unanswered transmission waits (tribute: its limit).
pub const MESSAGE_LIFETIME: f32 = 120.0;
/// ADAPTED: transmissions waiting at once.
pub const MAX_INBOX: usize = 3;

/// A transmission waiting for the player (the comm button glows).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Incoming {
    /// The sender (None: the player's own colonists).
    pub empire: Option<EmpireId>,
    pub file: u32,
    pub dialog: u32,
    pub world: u32,
    /// `~cost~` of the message (tribute).
    pub amount: f32,
    pub time_left: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackKind {
    /// An empire at war with the player (`universeSimulatorWarAttack*`).
    War,
    /// The Grox (`universeSimulatorGrobWarAttack*`).
    Grox,
    /// Grox tease attack before a war (`universeSimulatorGrobTeaseAttack*`).
    GroxTease,
    /// Raiders on an ally's world (`universeSimulatorPirateRaidAlly*`).
    AllyRaid,
}

impl AttackKind {
    /// The retail raid mission row of the attack (`raidwar`, `grobwarraid`,
    /// `grobteaserraid`, `raidally`).
    pub fn row(self) -> &'static rules::RaidRow {
        match self {
            AttackKind::War => &rules::RAID_ROW_WAR,
            AttackKind::Grox => &rules::RAID_ROW_GROX_WAR,
            AttackKind::GroxTease => &rules::RAID_ROW_GROX_TEASER,
            AttackKind::AllyRaid => &rules::RAID_ROW_ALLY,
        }
    }
}

/// A raid on a world in progress: the raid mission (`RaidEvent`, Update
/// 0xC5E410; the rules are [`rules::RaidRow`]).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attack {
    pub kind: AttackKind,
    pub empire: Option<EmpireId>,
    /// The ally being raided (AllyRaid; the `UnderAttack` flag).
    pub ally: Option<EmpireId>,
    pub world: u32,
    /// Seconds to the failure deadline (the row's wait + duration).
    pub time_left: f32,
    /// The fighters of the row (3-5, Grox war 3-10).
    pub fighters: u32,
    pub kills_at_start: u32,
    /// The fighters have warped in (the player reached the system).
    #[serde(default)]
    pub spawned: bool,
    /// The mission's elapsed time (cMission +0x7C), seconds.
    #[serde(default)]
    pub elapsed: f32,
    /// The raiders have arrived (`mWaitingForRaid` +0x215 cleared).
    #[serde(default)]
    pub arrived: bool,
    /// `mTimeOfArrivalMS` +0x220: the elapsed time when the player first was at
    /// the raided world after the arrival.
    #[serde(default)]
    pub entered_at: Option<f32>,
}

impl Attack {
    pub fn new(kind: AttackKind, empire: Option<EmpireId>, ally: Option<EmpireId>, world: u32, fighters: u32, kills: u32) -> Self {
        Attack {
            kind,
            empire,
            ally,
            world,
            time_left: kind.row().deadline(),
            fighters,
            kills_at_start: kills,
            spawned: false,
            elapsed: 0.0,
            arrived: false,
            entered_at: None,
        }
    }
}

/// The saved state of the simulation (`SpaceCampaign::universe`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UniverseState {
    /// Empires the player has spoken with (first contact done, flag 0x5F1F0CA).
    pub contacted: BTreeSet<EmpireId>,
    pub inbox: Vec<Incoming>,
    pub attacks: Vec<Attack>,
    /// Stars empires colonised during play (star index -> empire).
    pub grown: BTreeMap<u32, EmpireId>,
    /// Grown stars that are still outposts (star flag 0x100: set by the
    /// outpost action, cleared by the upgrade action of 0xFE9410).
    pub outposts: BTreeSet<u32>,
    /// Wars between two NPC empires (the attack action on another empire's
    /// star, DeclareWar in 0xFE9410), as (lower id, higher id).
    pub npc_wars: BTreeSet<(EmpireId, EmpireId)>,
    /// Relationships between NPC empires (cRelationshipManager keeps one per
    /// directed pair: the first empire's view of the second, 0xD05A60), keyed
    /// `from << 16 | to` (`npc_key`).
    pub npc_relations: BTreeMap<u32, rules::Relationship>,
    /// Alliances between NPC empires (DeclareAlliance in 0xFE9410), as
    /// (lower id, higher id).
    pub npc_alliances: BTreeSet<(EmpireId, EmpireId)>,
    /// The relationship manager's decay period clock for the NPC pairs
    /// (0xD06070 every `RELATIONSHIP_DECAY_PERIOD`).
    pub npc_decay_clock: f32,
    /// The NPC update 0xFEA3A0: time since the last turn (+0x40), time in
    /// the current context group (+0x38: the galaxy view or not), whether
    /// that group is the galaxy view (+0x5C), and the empires still to act
    /// this turn (+0x60, one per frame, highest id first).
    pub turn_clock: f32,
    pub context_clock: f32,
    pub context_galaxy: bool,
    pub turn_queue: Vec<EmpireId>,
    /// The galaxy view is open (set by the Bevy system before each tick).
    #[serde(skip)]
    pub galaxy_view: bool,
    /// Old saves (the earlier 6 s growth step); unused.
    pub egs_clock: f32,
    pub next_war_attack: f32,
    pub next_grox_attack: f32,
    pub next_grox_tease: f32,
    pub next_ally_raid: f32,
    /// Universe time of the last NPC transmission.
    pub last_message: f32,
    pub seed: u32,
    /// Buildings lost to raids (statistics).
    pub buildings_lost: u32,
    pub raids_repelled: u32,
    /// The pirate distress being watched (its world) and the defence count then.
    pub raid_watch: Option<u32>,
    pub defended_seen: u32,
    /// The pirate raid's fighters have warped in.
    pub raid_spawned: bool,
    /// Special greetings: when the UFO was last destroyed, a colony's disaster
    /// (world, helped), colonies already greeted, the Grox news told.
    pub last_death: Option<f32>,
    pub ships_lost_seen: u32,
    pub disaster_greeting: Option<(u32, bool)>,
    pub bio_watch: Option<u32>,
    pub cleansed_seen: u32,
    pub greeted_colonies: BTreeSet<u32>,
    pub grox_news_told: bool,
    /// The artifact routine of the universe simulator and the tools lying on
    /// worlds (`space_tool_artifacts`).
    pub tool_artifacts: crate::space_tool_artifacts::ToolArtifacts,
    /// The relationship value of each empire at the previous stance update
    /// (0x1013370 keeps it per empire, 0xC8C6E0).
    pub stance_seen: BTreeMap<EmpireId, f32>,
}

impl UniverseState {
    fn roll(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    fn send(&mut self, msg: Incoming) -> bool {
        if self.inbox.len() >= MAX_INBOX || self.inbox.iter().any(|m| m.empire == msg.empire && m.dialog == msg.dialog) {
            return false;
        }
        self.inbox.push(msg);
        true
    }

    /// The empire that owns `star` through growth.
    pub fn grown_owner(&self, star: u32) -> Option<EmpireId> {
        self.grown.get(&star).copied()
    }
}

/// The NPC action rules of the installed spacenpcai (PatchData
/// 02AE0C7E!249E8357, VERIFIED data): `NPCActionRulesMilitary` (0x056E61E1),
/// `NPCActionRulesColonise` (0x056E61E2), `NPCActionRulesEconomic`
/// (0x056E61E3), parsed by 0xFEA9A0 in this order.
pub const NPC_ACTION_RULES: [&[&str]; 3] = [
    &[
        "10 outpost -color Blue -nearMe 1 -nearEnemy 1 -nearNeutral -3",
        "9 upgrade -color Blue",
        "8 outpost -color Yellow -nearMe -1 -nearEnemy -3 -nearNeutral -1",
        "1 nasty -nearMe 2 -ally -8 -player 3 -enemy -99 -empireStars 0.5",
        "2 upgrade -color Yellow",
        "1 outpost -color Red -nearMe -1",
        "1 outpost -color Red",
        "0 nice -nearMe 2 -enemy 3 -ally -99",
        "0 attack -color Blue -nearMe 1 -player 1 -enemy 9 -ally -99",
        "0 attack -color Yellow -nearMe 1 -player 1 -enemy 8 -ally -99",
        "0 attack -color Red -nearMe 1 -player 1 -enemy 7 -ally -99",
    ],
    &[
        "10 outpost -color Blue -nearMe 1 -nearEnemy -5",
        "9 upgrade -color Blue",
        "8 upgrade -color Yellow",
        "7 outpost -color Yellow -nearMe -1 -nearEnemy -1 -nearNeutral -1",
        "5 nasty -nearMe 2 -ally -3 -player 1 -enemy -99 -empireColonies -1",
        "2 outpost -color Red",
        "1 upgrade -color Red",
        "0 nice -nearMe 2 -enemy 3 -player 2 -ally -99 -empireStars 0.5",
        "0 attack -color Blue -nearMe 1 -player 1 -enemy 9 -ally -99",
        "0 attack -color Yellow -nearMe 1 -player 1 -enemy 8 -ally -99",
        "0 attack -color Red -nearMe 1 -player 1 -enemy 7 -ally -99",
    ],
    &[
        "8 outpost -color Red -nearMe -1 -nearEnemy -1 -nearAlly -1 -nearNeutral 1",
        "5 nice -enemy 1 -nearMe 2 -ally -99",
        "4 outpost -color Yellow -nearMe -1 -nearEnemy -1 -nearNeutral -1",
        "3 outpost -color Blue -nearMe -1 -nearEnemy -1 -nearNeutral -1",
        "2 upgrade -color Red -nearMe 1",
        "1 upgrade -color Yellow -nearMe 1",
        "0 upgrade -color Blue -nearMe 1",
    ],
];

/// The NPC actions of 0xFE9410 (names hashed by 0xFE8930).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcAction {
    Outpost = 0,
    Upgrade = 1,
    Attack = 2,
    Nice = 3,
    Nasty = 4,
}

impl NpcAction {
    fn parse(name: &str) -> Self {
        // 0xFE8930: unknown names (and "outpost") are 0.
        match name.to_ascii_lowercase().as_str() {
            "upgrade" => NpcAction::Upgrade,
            "attack" => NpcAction::Attack,
            "nice" => NpcAction::Nice,
            "nasty" => NpcAction::Nasty,
            _ => NpcAction::Outpost,
        }
    }
}

/// One stored rule (0xFE8990): base, nearMe, nearEnemy, nearNeutral,
/// nearAlly, ally, enemy, player, relationship, empireStars,
/// empireColonies, the order 0xFE8E00 / 0xFE8B40 weigh the features in.
pub type NpcRule = [f32; 11];

/// A parsed rule set: 5 actions x 3 star colours (red 0, yellow 1, blue 2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NpcRules(pub [[NpcRule; 3]; 5]);

impl NpcRules {
    /// 0xFEA9A0 / 0xFE8A20: each line is "<base> <action> [-option value]..";
    /// a line without `-color` is stored for all three colours, a later line
    /// for the same action and colour replaces the earlier one. Options the
    /// line does not give are 0 (INFERRED: the ArgScript defaults).
    pub fn parse(lines: &[&str]) -> Self {
        let mut rules = NpcRules::default();
        for line in lines {
            let tok: Vec<&str> = line.split_whitespace().collect();
            let (Some(base), Some(action)) = (tok.first().and_then(|t| t.parse::<f32>().ok()), tok.get(1)) else {
                continue;
            };
            let mut rule: NpcRule = [0.0; 11];
            rule[0] = base;
            let mut colour: Option<usize> = None;
            let mut i = 2;
            while i + 1 < tok.len() {
                let value = tok[i + 1];
                let slot = match tok[i].to_ascii_lowercase().as_str() {
                    "-color" => {
                        // 0xFE8A20: "red" 0, "blue" 2, anything else 1.
                        colour = Some(match value.to_ascii_lowercase().as_str() {
                            "red" => 0,
                            "blue" => 2,
                            _ => 1,
                        });
                        None
                    }
                    "-nearme" => Some(1),
                    "-nearenemy" => Some(2),
                    "-nearneutral" => Some(3),
                    "-nearally" => Some(4),
                    "-ally" => Some(5),
                    "-enemy" => Some(6),
                    "-player" => Some(7),
                    "-relationship" => Some(8),
                    "-empirestars" => Some(9),
                    "-empirecolonies" => Some(10),
                    _ => None,
                };
                if let (Some(s), Ok(v)) = (slot, value.parse::<f32>()) {
                    rule[s] = v;
                }
                i += 2;
            }
            let a = NpcAction::parse(action) as usize;
            match colour {
                Some(c) => rules.0[a][c] = rule,
                None => rules.0[a] = [rule; 3],
            }
        }
        rules
    }

    /// The score of `action` at a star of `colour` for the features.
    pub fn score(&self, action: NpcAction, colour: usize, f: &NpcFeatures) -> f32 {
        let r = &self.0[action as usize][colour];
        r[0] + r[1] * f.near_me
            + r[2] * f.near_enemy
            + r[3] * f.near_neutral
            + r[4] * f.near_ally
            + r[5] * f.ally
            + r[6] * f.enemy
            + r[7] * f.player
            + r[8] * f.relationship
            + r[9] * f.empire_stars
            + r[10] * f.empire_colonies
    }
}

/// The rule set of an empire: the category table 0x15B46D0 indexed by the
/// archetype (warrior, zealot, Grox 0 -> Military; scientist, shaman, bard,
/// ecologist 1 -> Colonise; trader, diplomat 2 -> Economic; VERIFIED).
pub fn npc_rules(empire: EmpireId) -> &'static NpcRules {
    static RULES: std::sync::OnceLock<[NpcRules; 3]> = std::sync::OnceLock::new();
    let sets = RULES.get_or_init(|| NPC_ACTION_RULES.map(NpcRules::parse));
    &sets[npc_category(empire) as usize]
}

/// 0x15B46D0 for an empire id (ids carry the archetype in their last digit,
/// `EmpireArchetype::id_residue`; 0 is the Grox). ADAPTED: residue 8 (no
/// retail NPC archetype, OpenSpore's Knight) reads the table's 0.
pub fn npc_category(empire: EmpireId) -> u8 {
    if empire == 0 {
        return 0;
    }
    crate::space_worldgen::EmpireArchetype::from_residue(empire as u32 % 10).map_or(0, |a| a.category())
}

/// The features 0xFE8E00 weighs for a candidate star.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NpcFeatures {
    /// Empire-tech stars within 2 units (the star itself included): 0.5 each,
    /// at most 1, split by owner (the empire, at war with it, allied, other).
    pub near_me: f32,
    pub near_enemy: f32,
    pub near_neutral: f32,
    pub near_ally: f32,
    /// The star's owner (another empire): allied 1, at war 1, the
    /// relationship value (-10..10), the player 1.
    pub ally: f32,
    pub enemy: f32,
    pub relationship: f32,
    pub player: f32,
    /// The empire's stars, and those that are not outposts.
    pub empire_stars: f32,
    pub empire_colonies: f32,
}

/// The owner of a star for the NPC AI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarOwner {
    Nobody,
    Player,
    Empire(EmpireId),
}

/// The red / yellow / blue colour index of a star kind (0xC6FFA0: G, GG,
/// GM 1; O, OO, OM, OG 2; anything else 0).
pub fn colour_index(kind: StarKind) -> usize {
    match kind {
        StarKind::Yellow | StarKind::BinaryGG | StarKind::BinaryGM => 1,
        StarKind::Blue | StarKind::BinaryOO | StarKind::BinaryOM | StarKind::BinaryOG => 2,
        _ => 0,
    }
}

/// The ownership view of the galaxy the NPC AI reads.
struct Owners<'a> {
    campaign: &'a SpaceCampaign,
    galaxy: &'a Galaxy,
    player: BTreeSet<u32>,
}

impl<'a> Owners<'a> {
    fn new(campaign: &'a SpaceCampaign, galaxy: &'a Galaxy) -> Self {
        let mut player: BTreeSet<u32> = campaign
            .colonies
            .iter()
            .chain(campaign.conquered_worlds.iter())
            .map(|w| crate::space_galaxy::system_of_world(*w))
            .collect();
        player.insert(crate::space_galaxy::HOME_STAR);
        Owners { campaign, galaxy, player }
    }

    fn owner(&self, star: u32) -> StarOwner {
        if self.player.contains(&star) {
            return StarOwner::Player;
        }
        if let Some(e) = self.campaign.universe.grown_owner(star) {
            return StarOwner::Empire(e);
        }
        match self.galaxy.star(star).and_then(|s| s.empire) {
            Some(e) => StarOwner::Empire(e),
            None => StarOwner::Nobody,
        }
    }

    /// Empire tech level (5): owned stars (the player's colonised stars,
    /// grown outposts and colonies, the generated empires).
    fn empire_tech(&self, star: u32) -> bool {
        self.owner(star) != StarOwner::Nobody
            || self.galaxy.star(star).is_some_and(|s| s.tech == crate::space_worldgen::TechLevel::Empire)
    }

    /// Creature tech level (1) and a planet to settle (0xBBAA30: every
    /// system's first planet is rocky, so a star with planets qualifies).
    fn settleable(&self, star: u32) -> bool {
        self.owner(star) == StarOwner::Nobody
            && self
                .galaxy
                .star(star)
                .is_some_and(|s| s.tech == crate::space_worldgen::TechLevel::Creature && s.kind.has_planets())
    }

    fn at_war(&self, a: EmpireId, b: StarOwner) -> bool {
        match b {
            StarOwner::Player => self.campaign.wars.contains(&a),
            StarOwner::Empire(e) => self.campaign.universe.npc_wars.contains(&(a.min(e), a.max(e))),
            StarOwner::Nobody => false,
        }
    }

    /// The player's allies, or an alliance between two NPC empires (made by
    /// the nice action of 0xFE9410, `npc_nice_nasty`).
    fn allied(&self, a: EmpireId, b: StarOwner) -> bool {
        match b {
            StarOwner::Player => self.campaign.allies.contains(&a),
            StarOwner::Empire(e) => self.campaign.universe.npc_alliances.contains(&(a.min(e), a.max(e))),
            StarOwner::Nobody => false,
        }
    }

    /// The stars of `empire` (0xC30A30) seen within `radius` of `around`.
    fn stars_of(&self, empire: EmpireId, around: u32, radius: f32) -> Vec<u32> {
        let mut all: Vec<u32> = std::iter::once(around)
            .chain(self.galaxy.stars_within(around, radius).into_iter().map(|(i, _)| i))
            .filter(|s| self.owner(*s) == StarOwner::Empire(empire))
            .collect();
        all.sort_unstable();
        all
    }

    /// 0xFE8E00 features of star `star` for `empire`.
    fn features(&self, empire: EmpireId, star: u32, stars: f32, colonies: f32) -> NpcFeatures {
        let mut f = NpcFeatures { empire_stars: stars, empire_colonies: colonies, ..Default::default() };
        let near = std::iter::once(star).chain(self.galaxy.stars_within(star, NPC_NEIGHBOUR_RADIUS).into_iter().map(|(i, _)| i));
        for s in near.filter(|s| self.empire_tech(*s)) {
            let o = self.owner(s);
            let slot = if o == StarOwner::Empire(empire) {
                &mut f.near_me
            } else if self.at_war(empire, o) {
                &mut f.near_enemy
            } else if self.allied(empire, o) {
                &mut f.near_ally
            } else {
                &mut f.near_neutral
            };
            *slot += 0.5;
        }
        for v in [&mut f.near_me, &mut f.near_enemy, &mut f.near_neutral, &mut f.near_ally] {
            *v = v.min(1.0);
        }
        let owner = self.owner(star);
        if owner != StarOwner::Nobody && owner != StarOwner::Empire(empire) {
            f.ally = if self.allied(empire, owner) { 1.0 } else { 0.0 };
            f.enemy = if self.at_war(empire, owner) { 1.0 } else { 0.0 };
            // 0xD00950: the empire's relationship with the owner.
            f.relationship = match owner {
                StarOwner::Empire(e) => npc_relationship(self.campaign, empire, e),
                _ => self.campaign.relationship(empire),
            };
        }
        f.player = if owner == StarOwner::Player { 1.0 } else { 0.0 };
        f
    }
}

/// 0xFE9410's choice for `empire`: the candidate star and action with the
/// best score above 1 (None: the empire does nothing this turn).
pub fn choose_action(campaign: &SpaceCampaign, galaxy: &Galaxy, empire: EmpireId) -> Option<(u32, NpcAction, f32)> {
    let owners = Owners::new(campaign, galaxy);
    let table = npc_rules(empire);
    // The empire's stars near the ship (ADAPTED: retail walks its whole star
    // list; the counts below use the full size).
    let mine = owners.stars_of(empire, campaign.current_system, NPC_TURN_RADIUS + EMPIRE_RADIUS + NPC_CANDIDATE_RADIUS);
    let outposts = campaign.universe.outposts.iter().filter(|s| campaign.universe.grown_owner(**s) == Some(empire)).count();
    let stars = (galaxy.empire_stars(empire) as usize + campaign.universe.grown.values().filter(|e| **e == empire).count()) as f32;
    let colonies = stars - outposts as f32;
    // 0x5F1F0CA (first contact done) for actions toward the player's stars.
    let contact = !campaign.universe.contacted.is_empty();
    let awareness = rules::awareness_level(campaign.awareness.get(&empire).copied().unwrap_or(0.0));
    let mut seen = BTreeSet::new();
    let mut best: Option<(u32, NpcAction, f32)> = None;
    for &own in &mine {
        // 0xFE8AE0: FindStars within 4 units of each star (itself included).
        let near = std::iter::once(own).chain(galaxy.stars_within(own, NPC_CANDIDATE_RADIUS).into_iter().map(|(i, _)| i));
        for star in near {
            if !seen.insert(star) {
                continue;
            }
            let Some(s) = galaxy.star(star) else { continue };
            if !s.kind.has_planets() {
                continue;
            }
            let colour = colour_index(s.kind);
            let owner = owners.owner(star);
            let f = owners.features(empire, star, stars, colonies);
            let mut pick: Option<(NpcAction, f32)> = None;
            let consider = |a: NpcAction, v: f32, pick: &mut Option<(NpcAction, f32)>| {
                if v > pick.map_or(1.0, |p| p.1) {
                    *pick = Some((a, v));
                }
            };
            if owners.settleable(star) {
                consider(NpcAction::Outpost, table.score(NpcAction::Outpost, colour, &f), &mut pick);
            } else if owner == StarOwner::Empire(empire) {
                if campaign.universe.outposts.contains(&star) {
                    consider(NpcAction::Upgrade, table.score(NpcAction::Upgrade, colour, &f), &mut pick);
                }
            } else if owner != StarOwner::Nobody {
                // The player's stars only after a first contact and while the
                // empire is very / extremely aware of the player (0x1010650
                // level 2..3).
                if owner == StarOwner::Player && !(contact && (2..=3).contains(&awareness)) {
                    continue;
                }
                for a in [NpcAction::Attack, NpcAction::Nice, NpcAction::Nasty] {
                    consider(a, table.score(a, colour, &f), &mut pick);
                }
            }
            if let Some((a, v)) = pick {
                if v > best.map_or(1.0, |b| b.2) {
                    best = Some((star, a, v));
                }
            }
        }
    }
    best
}

/// Run 0xFE9410 for `empire`: choose and carry out its action. Returns an
/// alert for the player, if any.
pub fn npc_act(campaign: &mut SpaceCampaign, galaxy: &Galaxy, empire: EmpireId) -> Option<String> {
    let (star, action, _) = choose_action(campaign, galaxy, empire)?;
    let owner = Owners::new(campaign, galaxy).owner(star);
    // Outposts and upgrades wait while the player is in that system (outside
    // the galaxy view).
    let here = star == campaign.current_system && !campaign.universe.galaxy_view;
    match action {
        NpcAction::Outpost if !here && owner == StarOwner::Nobody => {
            campaign.universe.grown.insert(star, empire);
            campaign.universe.outposts.insert(star);
            if let Some(size) = campaign.empire_sizes.get_mut(empire as usize) {
                *size += 1;
            }
            debug!("empire {empire} placed an outpost at {} ({star})", galaxy.star_name(star));
            None
        }
        NpcAction::Upgrade if !here => {
            campaign.universe.outposts.remove(&star);
            upgrade_colony(campaign, star);
            None
        }
        NpcAction::Attack => match owner {
            // `raidwarplayer` 0x77184BAE on the player's planet and war.
            StarOwner::Player => {
                if !campaign.wars.contains(&empire) {
                    campaign.wars.push(empire);
                    campaign.allies.retain(|a| *a != empire);
                }
                let world = campaign
                    .colonies
                    .iter()
                    .copied()
                    .find(|w| crate::space_galaxy::system_of_world(*w) == star)
                    .unwrap_or(crate::space_galaxy::world_id(star, 0));
                start_attack_on(campaign, AttackKind::War, Some(empire), world)
            }
            // `raidwar` 0xBBAE6B99 and war between the two empires.
            StarOwner::Empire(other) => {
                npc_attack(campaign, empire, other);
                None
            }
            StarOwner::Nobody => None,
        },
        NpcAction::Nice | NpcAction::Nasty if owner == StarOwner::Player => {
            let msg = if action == NpcAction::Nice { nice(campaign, empire) } else { nasty(campaign, empire) };
            let m = msg?;
            if campaign.universe.send(m) {
                campaign.universe.last_message = campaign.universe_clock;
                return Some(format!("Incoming transmission from the {} empire", SpaceCampaign::empire_name(empire)));
            }
            None
        }
        // Between NPC empires: BeNice / BeNasty both ways, then an alliance /
        // war past the thresholds.
        NpcAction::Nice | NpcAction::Nasty => {
            if let StarOwner::Empire(other) = owner {
                npc_nice_nasty(campaign, empire, other, action == NpcAction::Nice);
            }
            None
        }
        _ => None,
    }
}

/// The key of the directed pair (`from`'s view of `to`).
pub fn npc_key(from: EmpireId, to: EmpireId) -> u32 {
    (from as u32) << 16 | to as u32
}

/// 0xD00950 / 0xD05A60 between two NPC empires: the sum of the pair's event
/// contributions with `from`'s personality rows, capped to +-10. (The Grox
/// term of 0xD05A60, SpaceRelationship +0x60, is not added: INFERRED 0, the
/// installed Grox base relationship.)
pub fn npc_relationship(campaign: &SpaceCampaign, from: EmpireId, to: EmpireId) -> f32 {
    campaign.universe.npc_relations.get(&npc_key(from, to)).map_or(0.0, |r| {
        r.value(SpaceCampaign::personality(from), campaign.difficulty())
    })
}

/// ApplyRelationship(from, to, event, scale) between NPC empires (0xD05DD0).
pub fn apply_npc_relationship(campaign: &mut SpaceCampaign, from: EmpireId, to: EmpireId, event: RelationshipEvent, scale: f32) {
    let personality = SpaceCampaign::personality(from);
    let difficulty = campaign.difficulty();
    campaign.universe.npc_relations.entry(npc_key(from, to)).or_default().apply(event, scale, personality, difficulty);
}

fn npc_pair(a: EmpireId, b: EmpireId) -> (EmpireId, EmpireId) {
    (a.min(b), a.max(b))
}

/// cRelationshipManager::DeclareWar 0xD065E0 between two NPC empires
/// (VERIFIED): nothing when they are at war already; else the war flag on
/// both directed pairs, the alliance flag cleared (a war ends an alliance)
/// and RelationshipEventSpaceStartedWar 0x05F8A1AD x 1 on both views.
pub fn npc_declare_war(campaign: &mut SpaceCampaign, empire: EmpireId, other: EmpireId) {
    let pair = npc_pair(empire, other);
    if campaign.universe.npc_wars.contains(&pair) {
        return;
    }
    campaign.universe.npc_wars.insert(pair);
    campaign.universe.npc_alliances.remove(&pair);
    apply_npc_relationship(campaign, other, empire, RelationshipEvent::StartedWar, 1.0);
    apply_npc_relationship(campaign, empire, other, RelationshipEvent::StartedWar, 1.0);
}

/// 0xFE9410 action 2 on another NPC empire's star: `raidwar` 0xBBAE6B99 (no
/// player-facing effect here), DeclareWar, then
/// RelationshipEventSpaceDestroyBuilding 0x0526E53C x 20 on the owner's view
/// of the attacker. The event is applied only while neither empire is in
/// the cached event data's set (cSimulatorSpaceGame +0x30, 0xFE6730, +0xE8):
/// 0xFE69B0 fills it with every non-player empire flagged
/// kEmpireFlagFromSaveGame 0x40 (VERIFIED); OpenSpore has no such empires,
/// so the set is empty.
pub fn npc_attack(campaign: &mut SpaceCampaign, empire: EmpireId, other: EmpireId) {
    npc_declare_war(campaign, empire, other);
    apply_npc_relationship(campaign, other, empire, RelationshipEvent::DestroyBuilding, 20.0);
}

/// 0xFE9410 actions 3 (nice) / 4 (nasty) between NPC empires: BeNice
/// 0x0577909A / BeNasty 0x0577909B x 1 both ways (same set condition as
/// `npc_attack`), then, when not already allied (nice) / at war (nasty), the
/// empire's relationship with the owner decides: an alliance when the
/// alliance chance 0x1040830 (1 at `createAllianceBase` 7 or more, else 0)
/// is above 0.5, a war at or below the war threshold (0xCBCC70: manager
/// +0x38; warThreshold, warPassiveThreshold and warEndThreshold are all -7).
///
/// NPC wars end per the code (VERIFIED): DeclarePeace 0xD03920 is called by
/// DeclareAlliance 0xD06960 (first thing it does), by the player-NPC stance
/// update 0x1013370 and by the player's comm choices 0x102CF20 / 0x102DF30;
/// between two NPCs the only way out of a war is this alliance. The rules
/// favour it: every `nice` rule weighs `-enemy` positive (3 / 3 / 1), so an
/// empire at war keeps choosing nice toward its enemy until the BeNice
/// events (with the 60 s decay of the grudges) reach `createAllianceBase`.
pub fn npc_nice_nasty(campaign: &mut SpaceCampaign, empire: EmpireId, other: EmpireId, nice: bool) {
    let event = if nice { RelationshipEvent::BeNice } else { RelationshipEvent::BeNasty };
    apply_npc_relationship(campaign, other, empire, event, 1.0);
    apply_npc_relationship(campaign, empire, other, event, 1.0);
    let pair = npc_pair(empire, other);
    let value = npc_relationship(campaign, empire, other);
    if nice {
        let chance = if value >= rules::CREATE_ALLIANCE_THRESHOLD { 1.0 } else { 0.0 };
        let u = &mut campaign.universe;
        if !u.npc_alliances.contains(&pair) && chance > 0.5 {
            // DeclareAlliance: DeclarePeace (the war flag cleared, the
            // events kept), then the alliance flag on both pairs.
            u.npc_wars.remove(&pair);
            u.npc_alliances.insert(pair);
        }
    } else if !campaign.universe.npc_wars.contains(&pair) && rules::declares_war(value) {
        npc_declare_war(campaign, empire, other);
    }
}

/// The upgrade action of 0xFE9410 on an outpost: its colony planet (the
/// first settleable world, as the outpost action colonised it) becomes T2
/// with temperature 0.65 and atmosphere 0.5 (water clamped to 0.5..0.65,
/// not modelled). The world's ecology entry carries the new climate (no
/// food web: the player's food-web badge does not count it).
pub fn upgrade_colony(campaign: &mut SpaceCampaign, star: u32) {
    let world = crate::space_galaxy::world_id(star, 0);
    let i = campaign.ensure_ecology(world);
    campaign.ecology[i].temperature = UPGRADE_TEMPERATURE;
    campaign.ecology[i].atmosphere = UPGRADE_ATMOSPHERE;
}

/// 0xFE9410 upgrade: the colony planet's temperature / atmosphere scores.
pub const UPGRADE_TEMPERATURE: f32 = 0.65;
pub const UPGRADE_ATMOSPHERE: f32 = 0.5;

/// The empires that take a turn (0xFEA3A0): owners of an Empire-tech star
/// that is not an outpost within 30 units of the active star, not the
/// player; highest id last (the queue is popped from the back).
pub fn turn_empires(campaign: &SpaceCampaign, galaxy: &Galaxy) -> Vec<EmpireId> {
    let owners = Owners::new(campaign, galaxy);
    let here = campaign.current_system;
    let mut set = BTreeSet::new();
    for star in std::iter::once(here).chain(galaxy.stars_within(here, NPC_TURN_RADIUS).into_iter().map(|(i, _)| i)) {
        if let StarOwner::Empire(e) = owners.owner(star) {
            if !campaign.universe.outposts.contains(&star) {
                set.insert(e);
            }
        }
    }
    set.into_iter().collect()
}

/// An NPC's "nice" action toward the player (0xFE9410 type 3).
pub fn nice(campaign: &mut SpaceCampaign, empire: EmpireId) -> Option<Incoming> {
    if campaign.allies.contains(&empire) {
        return None;
    }
    let world = home_world_of(campaign, empire);
    let met = campaign.relationships.get(&empire).is_some_and(|r| r.met);
    let dialog = if !met {
        dialog::WELCOME
    } else if rules::offers_alliance(campaign.relationship(empire)) {
        dialog::OFFER_ALLIANCE
    } else {
        dialog::OFFER_GIFT
    };
    Some(Incoming { empire: Some(empire), file: INCOMING_FILE, dialog, world, amount: rules::NPC_GIFT_AMOUNT, time_left: MESSAGE_LIFETIME })
}

/// An NPC's "nasty" action toward the player (0xFE9410 type 4).
pub fn nasty(campaign: &mut SpaceCampaign, empire: EmpireId) -> Option<Incoming> {
    if campaign.wars.contains(&empire) {
        return None;
    }
    let world = home_world_of(campaign, empire);
    let met = campaign.relationships.get(&empire).is_some_and(|r| r.met);
    if !met {
        return Some(Incoming { empire: Some(empire), file: INCOMING_FILE, dialog: dialog::WARN_TRESPASS, world, amount: 0.0, time_left: MESSAGE_LIFETIME });
    }
    if rules::declares_war(campaign.relationship(empire)) {
        campaign.wars.push(empire);
        campaign.allies.retain(|a| *a != empire);
        return Some(Incoming { empire: Some(empire), file: INCOMING_FILE, dialog: dialog::DECLARE_WAR, world, amount: 0.0, time_left: MESSAGE_LIFETIME });
    }
    if campaign.tribute_demand.is_some() {
        return None;
    }
    let amount = crate::space_comm::tribute_amount(usize::from(campaign.captain_rank()), campaign.difficulty());
    campaign.tribute_demand = Some(TributeDemand { empire, amount, time_left: rules::NPC_TRIBUTE_TIME_LIMIT });
    Some(Incoming {
        empire: Some(empire),
        file: INCOMING_FILE,
        dialog: dialog::DEMAND_TRIBUTE,
        world,
        amount,
        time_left: rules::NPC_TRIBUTE_TIME_LIMIT,
    })
}

fn home_world_of(campaign: &SpaceCampaign, empire: EmpireId) -> u32 {
    crate::space_galaxy::registered_galaxy()
        .and_then(|g| g.empire_by_id(empire).map(|r| crate::space_galaxy::world_id(r.home, 0)))
        .unwrap_or_else(|| crate::space_galaxy::world_id(campaign.current_system, 0))
}

/// Raid a world: up to `RAID_MAX_BUILDINGS_LOST` buildings, each turret saves
/// one; production buildings go first. Returns the buildings destroyed.
pub fn destroy_buildings(campaign: &mut SpaceCampaign, world: u32) -> u32 {
    destroy_buildings_by(campaign, world, &rules::RAID_ROW_WAR)
}

/// The same for a raid row (`missionRaidBuildingsDestroyed` minus
/// `missionRaidBuildingsSavedPerTurret` per turret). ADAPTED: retail's damage
/// tick 0xC5CA10 destroys the buildings one by one while the raiders live; the
/// port removes the whole loss when the raid fails.
pub fn destroy_buildings_by(campaign: &mut SpaceCampaign, world: u32, row: &rules::RaidRow) -> u32 {
    let turrets = campaign.total_defense_turrets(world) as u32;
    let mut to_lose = row.buildings_lost(turrets);
    let mut lost = 0;
    if let Some(layouts) = campaign.city_layouts.get_mut(&world) {
        for kind in [CityBuilding::Factory, CityBuilding::House, CityBuilding::Entertainment] {
            for layout in layouts.iter_mut() {
                for slot in layout.slots.iter_mut() {
                    if to_lose > 0 && *slot == Some(kind) {
                        *slot = None;
                        to_lose -= 1;
                        lost += 1;
                    }
                }
            }
        }
    }
    campaign.universe.buildings_lost += lost;
    lost
}

fn start_attack(campaign: &mut SpaceCampaign, kind: AttackKind, empire: Option<EmpireId>) -> Option<String> {
    let colonies: Vec<u32> = campaign.colonies.iter().copied().filter(|w| *w != 0).collect();
    if colonies.is_empty() {
        return None;
    }
    let world = colonies[(campaign.universe.roll() * colonies.len() as f32) as usize % colonies.len()];
    start_attack_on(campaign, kind, empire, world)
}

/// A raid of `kind` on the player's `world` (the colonists call for help).
fn start_attack_on(campaign: &mut SpaceCampaign, kind: AttackKind, empire: Option<EmpireId>, world: u32) -> Option<String> {
    let u = &mut campaign.universe;
    let row = kind.row();
    let fighters = row.fighters(u.roll());
    let kills = campaign.kills;
    let file = match kind {
        AttackKind::War => EVENT_RAID_WAR_FILE,
        AttackKind::Grox => EVENT_GROB_WAR_RAID_FILE,
        AttackKind::GroxTease => EVENT_GROB_TEASER_RAID_FILE,
        AttackKind::AllyRaid => EVENT_RAID_ALLY_FILE,
    };
    campaign.universe.attacks.push(Attack::new(kind, empire, None, world, fighters, kills));
    campaign.universe.send(Incoming { empire: None, file, dialog: dialog::MISSION_ASSIGN, world, amount: 0.0, time_left: row.deadline() });
    let who = empire.map_or("Raiders".to_string(), |e| format!("The {} empire", SpaceCampaign::empire_name(e)));
    Some(format!("ALERT: {who} attacks your colony on world {world}! Defend it within {:.0} s", row.deadline()))
}

/// Advance the universe by `dt` seconds of Space play. Returns the alerts.
pub fn tick(campaign: &mut SpaceCampaign, dt: f32, galaxy: Option<&Galaxy>) -> Vec<String> {
    let mut alerts = Vec::new();
    let now = campaign.universe_clock;
    if campaign.universe.seed == 0 {
        campaign.universe.seed = 0x5EED_0001 ^ campaign.current_system;
    }
    // Awareness decay: `grobAwarenessDecayRate` 0.001 / s, empires 0.
    if let Some(a) = campaign.awareness.get_mut(&0) {
        *a = (*a - rules::GROX_AWARENESS_DECAY * dt).max(0.0);
    }
    // The UFO destroyed (the colonists' `myspecies_greeting_after_death`).
    if campaign.ships_lost > campaign.universe.ships_lost_seen {
        campaign.universe.last_death = Some(now);
    }
    campaign.universe.ships_lost_seen = campaign.ships_lost;
    // A bio-disaster ends: helped when it was cleansed.
    match campaign.active_distress.filter(|d| d.kind == DistressKind::BiohazardOutbreak) {
        Some(d) if campaign.universe.bio_watch != Some(d.world_id) => {
            campaign.universe.bio_watch = Some(d.world_id);
            campaign.universe.cleansed_seen = campaign.biohazards_cleansed;
        }
        None => {
            if let Some(world) = campaign.universe.bio_watch.take() {
                let helped = campaign.biohazards_cleansed > campaign.universe.cleansed_seen;
                campaign.universe.disaster_greeting = Some((world, helped));
            }
        }
        _ => {}
    }
    // The relationship manager's decay (0xD06070) of the NPC pairs, every
    // `RELATIONSHIP_DECAY_PERIOD` like the player's.
    campaign.universe.npc_decay_clock += dt;
    while campaign.universe.npc_decay_clock >= rules::RELATIONSHIP_DECAY_PERIOD {
        campaign.universe.npc_decay_clock -= rules::RELATIONSHIP_DECAY_PERIOD;
        let difficulty = campaign.difficulty();
        campaign.universe.npc_relations.retain(|key, rel| {
            rel.decay_tick(SpaceCampaign::personality((key >> 16) as EmpireId), difficulty);
            !rel.events.is_empty()
        });
    }
    // Transmissions expire.
    for m in &mut campaign.universe.inbox {
        m.time_left -= dt;
    }
    campaign.universe.inbox.retain(|m| m.time_left > 0.0);

    // The NPC turn (0xFEA3A0): the time in the current context group (the
    // galaxy view or not) and since the last turn; a turn when the player
    // enters the galaxy view after `minTimeEGS` minutes outside it, or
    // `maxTimeEGS` minutes after the last turn. Each empire of the turn then
    // acts once (0xFE9410), one per frame.
    let u = &mut campaign.universe;
    let mut switched = 0.0;
    if u.context_galaxy == u.galaxy_view {
        u.context_clock += dt;
    } else {
        switched = u.context_clock;
        u.context_clock = 0.0;
        u.context_galaxy = u.galaxy_view;
    }
    u.turn_clock += dt;
    if let Some(g) = galaxy {
        if let Some(e) = campaign.universe.turn_queue.pop() {
            alerts.extend(npc_act(campaign, g, e));
        } else if (campaign.universe.galaxy_view && switched >= NPC_TURN_MIN_TIME) || campaign.universe.turn_clock >= NPC_TURN_MAX_TIME {
            campaign.universe.turn_clock = 0.0;
            campaign.universe.context_clock = 0.0;
            campaign.universe.turn_queue = turn_empires(campaign, g);
            debug!("universe: NPC turn for {} empires", campaign.universe.turn_queue.len());
        }
    }

    // The player-stance update (0x1013650 -> 0x1013370, VERIFIED): every
    // empire with a relationship to the player, against the value of the
    // previous update (`SpaceCampaign::stance_update`). ADAPTED: the Grox
    // keep their event-driven stance; retail walks at most 100 empires a
    // frame.
    let empires: Vec<EmpireId> = campaign.relationships.keys().copied().filter(|e| *e != 0).collect();
    for e in empires {
        let last = campaign.universe.stance_seen.get(&e).copied();
        if let Some(msg) = campaign.stance_update(e, last) {
            alerts.push(msg);
        }
        let value = campaign.relationship(e);
        campaign.universe.stance_seen.insert(e, value);
    }

    // The artifact routine (0x10105D0): tools appear on worlds near the
    // active star (`space_tool_artifacts`).
    if let Some(g) = galaxy {
        crate::space_tool_artifacts::tick(campaign, dt, g);
    }

    // War attack waves (first after 300 s of war, then every 900 s).
    if campaign.wars.iter().any(|e| *e != 0) {
        let u = &mut campaign.universe;
        if u.next_war_attack <= 0.0 {
            u.next_war_attack = now + rules::WAR_ATTACK_MIN_TIME;
        } else if now >= u.next_war_attack {
            u.next_war_attack = now + rules::WAR_ATTACK_FREQUENCY;
            let e = campaign.wars.iter().copied().find(|e| *e != 0);
            alerts.extend(start_attack(campaign, AttackKind::War, e));
        }
    } else {
        campaign.universe.next_war_attack = 0.0;
    }
    // Grox waves: at war and aware (every 120 s extremely aware, 240 s
    // otherwise, first after 240 s); tease attacks when aware but at peace.
    let grox_level = rules::awareness_level(campaign.awareness.get(&0).copied().unwrap_or(0.0));
    if grox_level >= 1 && !campaign.allies.contains(&0) {
        let at_war = campaign.wars.contains(&0);
        let u = &mut campaign.universe;
        if at_war {
            let every = if grox_level >= 3 { rules::GROX_WAR_ATTACK_FREQ_EXTREMELY_AWARE } else { rules::GROX_WAR_ATTACK_FREQ_AWARE };
            if u.next_grox_attack <= 0.0 {
                u.next_grox_attack = now + rules::GROX_WAR_ATTACK_MIN_TIME;
            } else if now >= u.next_grox_attack {
                u.next_grox_attack = now + every;
                alerts.extend(start_attack(campaign, AttackKind::Grox, Some(0)));
            }
        } else if u.next_grox_tease <= 0.0 {
            u.next_grox_tease = now + rules::GROX_TEASE_ATTACK_MIN_TIME;
        } else if now >= u.next_grox_tease {
            u.next_grox_tease = now + rules::GROX_TEASE_ATTACK_FREQUENCY;
            alerts.extend(start_attack(campaign, AttackKind::GroxTease, Some(0)));
        }
    }
    // Raids on allies (first after 300 s, every 900 s): the ally calls for help.
    if let Some(&ally) = campaign.allies.iter().find(|a| **a != 0) {
        let u = &mut campaign.universe;
        if u.next_ally_raid <= 0.0 {
            u.next_ally_raid = now + rules::PIRATE_RAID_ALLY_MIN_TIME;
        } else if now >= u.next_ally_raid {
            u.next_ally_raid = now + rules::PIRATE_RAID_ALLY_FREQUENCY;
            let world = home_world_of(campaign, ally);
            let kills = campaign.kills;
            let row = AttackKind::AllyRaid.row();
            let fighters = row.fighters(campaign.universe.roll());
            campaign.universe.attacks.push(Attack::new(AttackKind::AllyRaid, None, Some(ally), world, fighters, kills));
            campaign.universe.send(Incoming { empire: Some(ally), file: EVENT_RAID_ALLY_FILE, dialog: dialog::MISSION_ASSIGN, world, amount: 0.0, time_left: row.deadline() });
            alerts.push(format!("Your ally the {} empire is under attack!", SpaceCampaign::empire_name(ally)));
        }
    }
    // Pirate raids of the rules (`active_distress`): the colonists call, and a
    // raid that runs out destroys buildings.
    match campaign.active_distress.filter(|d| d.kind == DistressKind::PirateRaid) {
        Some(d) if campaign.universe.raid_watch != Some(d.world_id) => {
            campaign.universe.raid_watch = Some(d.world_id);
            campaign.universe.defended_seen = campaign.pirate_raids_defended;
            campaign.universe.send(Incoming { empire: None, file: EVENT_RAID_FILE, dialog: dialog::MISSION_ASSIGN, world: d.world_id, amount: 0.0, time_left: d.time_remaining });
        }
        None => {
            campaign.universe.raid_spawned = false;
            if let Some(world) = campaign.universe.raid_watch.take() {
                if campaign.pirate_raids_defended <= campaign.universe.defended_seen {
                    let lost = destroy_buildings(campaign, world);
                    alerts.push(format!("Pirates destroyed {lost} buildings on world {world}"));
                }
            }
        }
        _ => {}
    }
    // Attacks in progress: the raid mission's Update (0xC5E410, VERIFIED, see
    // `rules::RaidRow`). The raiders arrive when the wait is over or the player
    // is at the world; the mission is completed when no raider is left (ADAPTED:
    // the player destroying the row's fighters in the system stands for it) or, for
    // UFOs that leave on arrival, 25 s after the player entered; it fails when
    // raiders are still there `wait + duration` seconds after it started.
    let mut finished = Vec::new();
    for (i, a) in campaign.universe.attacks.iter_mut().enumerate() {
        let row = a.kind.row();
        a.elapsed += dt;
        a.time_left = (row.deadline() - a.elapsed).max(0.0);
        let here = crate::space_galaxy::system_of_world(a.world) == campaign.current_system;
        if !a.arrived {
            if a.elapsed > row.wait || here {
                a.arrived = true;
            } else {
                continue;
            }
        }
        if here && a.entered_at.is_none() {
            a.entered_at = Some(a.elapsed);
        }
        let left = row.leave_on_arrival && a.entered_at.is_some_and(|t| a.elapsed - t > rules::RAID_LEAVE_AFTER);
        let cleared = here && campaign.kills >= a.kills_at_start + a.fighters;
        if left || cleared {
            finished.push((i, Some(left)));
        } else if a.elapsed >= row.deadline() {
            finished.push((i, None));
        }
    }
    for (i, completed) in finished.into_iter().rev() {
        let a = campaign.universe.attacks.remove(i);
        let here = crate::space_galaxy::system_of_world(a.world) == campaign.current_system;
        match completed {
            // 0xC5BB60 (the raid mission's OnMissionCompleted, VERIFIED): the
            // planet's owner gets MissionComplete of the mission's 0x039787B3
            // size (`raidally` 3) and, being allied, the FriendsDefended badge
            // gains one; none of it for UFOs that leave on arrival (+0x21C).
            Some(left) => {
                campaign.universe.raids_repelled += 1;
                if !left {
                    if a.kind == AttackKind::AllyRaid {
                        if let Some(ally) = a.ally {
                            campaign.apply_relationship(ally, RelationshipEvent::MissionComplete, rules::RAID_ALLY_COMPLETE_RELATION);
                            campaign.ally_raids_defended += 1;
                        }
                    } else {
                        campaign.pirate_raids_defended += 1;
                    }
                }
                alerts.push(if left {
                    format!("The raiders left world {}", a.world)
                } else {
                    format!("The attack on world {} was repelled", a.world)
                });
            }
            // The raid failed (0xC5BC50: an allied owner hears MissionFailed of
            // |missionRaidAllyFailRelationshipDelta| unless the player was at the
            // planet); a player's colony loses the row's buildings.
            None => {
                if a.kind == AttackKind::AllyRaid {
                    if let (Some(ally), false) = (a.ally, here) {
                        let delta = a.kind.row().ally_fail_delta.abs();
                        campaign.apply_relationship(ally, RelationshipEvent::MissionFailed, delta);
                    }
                } else {
                    let lost = destroy_buildings_by(campaign, a.world, a.kind.row());
                    alerts.push(format!("The attack on world {} destroyed {lost} buildings", a.world));
                }
            }
        }
    }
    alerts
}

// ---------------------------------------------------------------------------
// Bevy.
// ---------------------------------------------------------------------------

pub struct SpaceUniversePlugin;

impl Plugin for SpaceUniversePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (run_universe, adopt_grown_system, spawn_raiders, galaxy_overlay)
                .chain()
                .after(crate::GameSystems::Controls)
                .run_if(in_state(GameStage::Space)),
        )
        .init_resource::<crate::space_hud::comm::CommPortrait>()
        .add_systems(
            Update,
            (crate::space_hud::comm::update_portrait, crate::space_hud::comm::place_portrait)
                .chain()
                .after(crate::spui::view::sync),
        )
        .add_systems(
            Update,
            crate::space_hud::comm::publish_event_cards
                .after(crate::space_hud::guide::publish_guide)
                .before(crate::space_hud::guide::bind_guide),
        );
    }
}

/// Run the simulation while the Space game runs (not paused, comm closed:
/// the comm screen pauses the game, `cGameTimeManager::Pause(CommScreen)`).
fn run_universe(
    time: Res<Time>,
    galaxy: Option<Res<Galaxy>>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    session: Option<Res<crate::space_comm::CommSession>>,
    view: Option<Res<crate::space_view::SpaceView>>,
    mut run: ResMut<RunProgress>,
    mut advanced: Local<bool>,
) {
    let galaxy_view = view.is_some_and(|v| v.galaxy);
    if run.space_campaign.universe.galaxy_view != galaxy_view {
        run.space_campaign.universe.galaxy_view = galaxy_view;
    }
    // `--space-universe-advance N`: simulate N seconds at once (captures).
    if !*advanced && run.space_campaign.economy_version != 0 {
        *advanced = true;
        let args: Vec<String> = std::env::args().collect();
        if let Some(n) = args.iter().position(|a| a == "--space-universe-advance").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u32>().ok()) {
            for _ in 0..n {
                run.space_campaign.universe_clock += 1.0;
                tick(&mut run.space_campaign, 1.0, galaxy.as_deref());
            }
            let u = &run.space_campaign.universe;
            info!("universe: advanced {n} s: {} stars grown, {} transmissions, {} attacks", u.grown.len(), u.inbox.len(), u.attacks.len());
        }
        // `--space-universe-raid`: an empire at war raids the world the ship
        // starts at (captures of the fighters and the mission card).
        if args.iter().any(|a| a == "--space-universe-raid") {
            let c = &mut run.space_campaign;
            let world = crate::space_galaxy::world_id(c.current_system, 0);
            let kills = c.kills;
            c.universe.attacks.push(Attack::new(AttackKind::War, Some(3), None, world, 4, kills));
        }
    }
    if pause.is_some_and(|p| p.open) || session.is_some_and(|s| s.open) {
        return;
    }
    let dt = time.delta_secs().min(0.25);
    if dt <= 0.0 {
        return;
    }
    let alerts = tick(&mut run.space_campaign, dt, galaxy.as_deref());
    if let Some(last) = alerts.last() {
        run.space_campaign.message = last.clone();
        run.request_save();
    }
}

/// Raid fighters warp in around the attacked world once the player is in its
/// system: `fighters` (3-5) per attack, Grox interceptors for Grox raids, and
/// 3-5 pirates for the rules' pirate raid (S2H). They fight with the
/// hostile-ship AI of `space::space_pirates`; the raid ends when the player
/// destroys as many ships there or when its time runs out.
#[allow(clippy::too_many_arguments)]
fn spawn_raiders(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    system: Option<Res<crate::space::CurrentSystem>>,
    view: Option<Res<crate::space_view::SpaceView>>,
    worlds: Query<(&crate::persistence::WorldId, &Transform)>,
    mut run: ResMut<RunProgress>,
) {
    use rand::SeedableRng;
    if view.as_ref().is_some_and(|v| v.galaxy) || system.is_none() {
        return;
    }
    let extent = system.as_ref().map_or(60.0, |s| s.extent());
    let current = run.space_campaign.current_system;
    let kills = run.space_campaign.kills;
    let at = |world: u32| worlds.iter().find(|(id, _)| id.0 == world).map(|(_, t)| t.translation);
    let mut seed = ((current as u64) << 8) ^ run.space_campaign.universe_clock as u64;
    let mut spawn = |commands: &mut Commands, around: Vec3, count: u32, grox: bool| {
        seed = seed.wrapping_add(0x9E37_79B9);
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        for _ in 0..count {
            crate::space::spawn_enemy_ship(commands, &mut meshes, &mut materials, &mut rng, around, extent, grox);
        }
    };
    for a in run.space_campaign.universe.attacks.iter_mut() {
        if a.spawned || crate::space_galaxy::system_of_world(a.world) != current {
            continue;
        }
        let Some(around) = at(a.world) else {
            continue;
        };
        let grox = matches!(a.kind, AttackKind::Grox | AttackKind::GroxTease);
        spawn(&mut commands, around, a.fighters, grox);
        a.spawned = true;
        a.kills_at_start = kills;
    }
    // The rare ambush (0x10096B0, `space_rares`): pirates at the world.
    if let Some(world) = run.space_campaign.rares.ambush {
        if crate::space_galaxy::system_of_world(world) == current {
            if let Some(around) = at(world) {
                let count = 3 + run.space_campaign.universe.seed % 3;
                spawn(&mut commands, around, count, false);
            }
        }
        run.space_campaign.rares.ambush = None;
    }
    if let Some(d) = run.space_campaign.active_distress.filter(|d| d.kind == DistressKind::PirateRaid) {
        if !run.space_campaign.universe.raid_spawned && d.system_id == current {
            if let Some(around) = at(d.world_id) {
                let count = 3 + run.space_campaign.universe.seed % 3;
                spawn(&mut commands, around, count, false);
                run.space_campaign.universe.raid_spawned = true;
            }
        }
    }
    // The creatures of an accepted eradicate mission (0xC4F350 places
    // `missionEradicateNumCreatures` of them while the player is on the planet;
    // `missionTimerStartsOnFirstEntry`: the time limit starts now). ADAPTED: hostile
    // ships stand in for the diseased animals (the port has no creatures on a planet).
    let mut placed = false;
    if let Some(m) = run.space_campaign.active_alien_mission.as_mut() {
        if m.kind == crate::space::AlienMissionKind::Eradicate && !m.extra.spawned && m.progress < m.target_count {
            if let Some(world) = m.target_world.filter(|w| crate::space_galaxy::system_of_world(*w) == current) {
                if let Some(around) = at(world) {
                    spawn(&mut commands, around, m.target_count, false);
                    m.extra.spawned = true;
                    m.time_left = m.extra.duration;
                    placed = true;
                }
            }
        }
    }
    if placed {
        run.space_campaign.message = "The diseased animals are here: destroy them before the time runs out".into();
    }
}

/// A system an empire colonised during play belongs to it on arrival.
fn adopt_grown_system(mut run: ResMut<RunProgress>) {
    let c = &run.space_campaign;
    if c.current_system_empire.is_none() {
        if let Some(e) = c.universe.grown_owner(c.current_system) {
            run.space_campaign.current_system_empire = Some(e);
        }
    }
}

#[derive(Component)]
pub struct GrownTerritory;

/// Grown empire stars (the empire's colour) and attacked systems (red) on the
/// galaxy map.
#[allow(clippy::too_many_arguments)]
fn galaxy_overlay(
    mut commands: Commands,
    run: Res<RunProgress>,
    galaxy: Option<Res<Galaxy>>,
    map: Option<Res<crate::space_galaxy::GalaxyMapMaterials>>,
    roots: Query<Entity, With<crate::space_galaxy::GalaxyMapRoot>>,
    existing: Query<Entity, With<GrownTerritory>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<BTreeMap<u32, Handle<StandardMaterial>>>,
    mut last: Local<Option<(usize, Vec<u32>)>>,
) {
    let (Some(galaxy), Some(map), Ok(root)) = (galaxy, map, roots.single()) else {
        return;
    };
    let u = &run.space_campaign.universe;
    let attacked: Vec<u32> = u.attacks.iter().map(|a| crate::space_galaxy::system_of_world(a.world)).collect();
    let key = (u.grown.len(), attacked.clone());
    if last.as_ref() == Some(&key) && !existing.is_empty() == (!u.grown.is_empty() || !attacked.is_empty()) {
        return;
    }
    *last = Some(key);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let texture = materials.get(&map.territory).and_then(|m| m.base_color_texture.clone());
    let mut material = |colour_key: u32, rgb: [f32; 3], alpha: f32| {
        cache
            .entry(colour_key)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgba(rgb[0], rgb[1], rgb[2], alpha),
                    base_color_texture: texture.clone(),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })
            })
            .clone()
    };
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    for (star, empire) in &u.grown {
        let Some(s) = galaxy.star(*star) else {
            continue;
        };
        let m = material(*empire as u32, crate::space_galaxy::empire_color(*empire), 0.35);
        let child = commands
            .spawn((
                GrownTerritory,
                NotShadowCaster,
                Mesh3d(map.disc_mesh.clone()),
                MeshMaterial3d(m),
                Transform::from_xyz(s.position.x, 0.6, s.position.y).with_rotation(flat).with_scale(Vec3::splat(14.0)),
            ))
            .id();
        commands.entity(root).add_child(child);
    }
    for system in attacked {
        let Some(s) = galaxy.star(system) else {
            continue;
        };
        let m = material(u32::MAX, [1.0, 0.15, 0.1], 0.6);
        let child = commands
            .spawn((
                GrownTerritory,
                NotShadowCaster,
                Mesh3d(map.disc_mesh.clone()),
                MeshMaterial3d(m),
                Transform::from_xyz(s.position.x, 1.0, s.position.y).with_rotation(flat).with_scale(Vec3::splat(30.0)),
            ))
            .id();
        commands.entity(root).add_child(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::SpaceArchetype;

    fn campaign_with_colony() -> SpaceCampaign {
        let mut c = SpaceCampaign::default();
        let world = crate::space_galaxy::world_id(5, 1);
        c.colonies = vec![0, world];
        let mut layout = crate::space::CityLayout::new();
        layout.slots = vec![
            Some(CityBuilding::Factory),
            Some(CityBuilding::House),
            Some(CityBuilding::Factory),
            Some(CityBuilding::Turret),
            Some(CityBuilding::Entertainment),
            Some(CityBuilding::Turret),
        ];
        c.city_layouts.insert(world, vec![layout.clone(), layout]);
        c
    }

    fn empire_of(arch: SpaceArchetype) -> EmpireId {
        (1..80).find(|e| crate::space::empire_archetype(*e) == arch).expect("empire")
    }

    #[test]
    fn raids_destroy_up_to_twelve_buildings_less_one_per_turret() {
        let mut c = campaign_with_colony();
        let world = c.colonies[1];
        // 8 production buildings, 4 turrets: 12 - 4 = 8 may be lost, all 8 are.
        assert_eq!(destroy_buildings(&mut c, world), 8);
        assert_eq!(c.total_defense_turrets(world), 4, "turrets are not lost");
        assert_eq!(c.universe.buildings_lost, 8);
        assert_eq!(destroy_buildings(&mut c, world), 0);
    }

    #[test]
    fn nice_and_nasty_follow_the_npc_action_executor() {
        let mut c = SpaceCampaign::default();
        let e = empire_of(SpaceArchetype::Warrior);
        // Before contact: welcome / trespass warning.
        assert_eq!(nice(&mut c, e).unwrap().dialog, dialog::WELCOME);
        assert_eq!(nasty(&mut c, e).unwrap().dialog, dialog::WARN_TRESPASS);
        c.meet_empire(e);
        // Below the alliance threshold: a gift; at it: an alliance offer.
        let rel = c.relationship(e);
        let d = nice(&mut c, e).unwrap().dialog;
        assert_eq!(d, if rules::offers_alliance(rel) { dialog::OFFER_ALLIANCE } else { dialog::OFFER_GIFT });
        // Above the war threshold: a tribute demand of the retail size.
        if !rules::declares_war(c.relationship(e)) {
            let m = nasty(&mut c, e).unwrap();
            assert_eq!(m.dialog, dialog::DEMAND_TRIBUTE);
            let demand = c.tribute_demand.expect("demand");
            assert_eq!(demand.time_left, rules::NPC_TRIBUTE_TIME_LIMIT);
            assert_eq!(m.amount, demand.amount);
            assert!(nasty(&mut c, e).is_none(), "one open demand at a time");
        }
        // At the war threshold: war.
        c.tribute_demand = None;
        for _ in 0..30 {
            c.apply_relationship(e, RelationshipEvent::BeNasty, 1.0);
        }
        c.wars.retain(|w| *w != e);
        assert!(rules::declares_war(c.relationship(e)));
        assert_eq!(nasty(&mut c, e).unwrap().dialog, dialog::DECLARE_WAR);
        assert!(c.wars.contains(&e));
        assert!(nasty(&mut c, e).is_none(), "nothing more at war");
    }

    #[test]
    fn war_and_grox_waves_follow_the_simulator_timers() {
        let mut c = campaign_with_colony();
        let e = empire_of(SpaceArchetype::Zealot);
        c.meet_empire(e);
        c.wars.push(e);
        // A grudge that keeps the war (the stance update 0x1013370 ends a war
        // above -7), renewed as it decays.
        c.apply_relationship(e, RelationshipEvent::StartedWar, 1.0);
        c.apply_relationship(e, RelationshipEvent::BeNasty, 2.0);
        let mut alerts = Vec::new();
        // Step one second at a time for 1300 s (no galaxy: no growth).
        for t in 0..1300 {
            c.universe_clock += 1.0;
            if t % 60 == 0 {
                c.apply_relationship(e, RelationshipEvent::BeNasty, 1.0);
            }
            alerts.extend(tick(&mut c, 1.0, None));
        }
        assert!(c.wars.contains(&e), "{alerts:?}");
        let war_attacks = alerts.iter().filter(|a| a.contains("attacks your colony")).count();
        // First at 300 s, then 1200 s: two waves (each lasts 300 s).
        assert_eq!(war_attacks, 2, "{alerts:?}");
        assert!(alerts.iter().any(|a| a.contains("destroyed")), "an undefended attack costs buildings");
        // Grox: extremely aware and at war -> every 120 s after 240 s.
        let mut g = campaign_with_colony();
        g.awareness.insert(0, 20.0);
        g.wars.push(0);
        let mut n = 0;
        for _ in 0..600 {
            g.universe_clock += 1.0;
            n += tick(&mut g, 1.0, None).iter().filter(|a| a.contains("Grox")).count();
        }
        // 240 s after the first aware step (t = 1), then every 120 s: 241, 361, 481.
        assert_eq!(n, 3);
        assert!(g.awareness[&0] < 20.0, "Grox awareness decays");
    }

    #[test]
    fn a_defended_attack_saves_the_buildings() {
        let mut c = campaign_with_colony();
        let world = c.colonies[1];
        c.current_system = crate::space_galaxy::system_of_world(world);
        assert!(start_attack(&mut c, AttackKind::War, Some(3)).is_some());
        let fighters = c.universe.attacks[0].fighters;
        assert!((3..=5).contains(&fighters));
        assert_eq!(c.universe.inbox.len(), 1, "the colonists call for help");
        c.kills += fighters;
        let alerts = tick(&mut c, 1.0, None);
        assert!(alerts.iter().any(|a| a.contains("repelled")));
        assert!(c.universe.attacks.is_empty());
        assert_eq!(c.universe.buildings_lost, 0);
    }

    #[test]
    fn raid_rows_carry_the_retail_data() {
        // The raid mission files (`raidwar`, `grobwarraid`, `grobteaserraid`, `raidally`):
        // missionRaidAllyWaitDuration (default 10) + missionRaidDuration.
        assert_eq!(AttackKind::War.row().deadline(), 360.0);
        assert_eq!(AttackKind::Grox.row().deadline(), 610.0);
        assert_eq!(AttackKind::GroxTease.row().deadline(), 910.0);
        assert_eq!(AttackKind::AllyRaid.row().deadline(), 360.0);
        assert!(AttackKind::GroxTease.row().leave_on_arrival);
        assert!(!AttackKind::War.row().leave_on_arrival);
        let r = AttackKind::Grox.row();
        assert_eq!((r.fighters(0.0), r.fighters(0.999)), (3, 10), "the Grox bring 3 to 10 fighters");
        let r = AttackKind::War.row();
        assert_eq!((r.fighters(0.0), r.fighters(0.5), r.fighters(0.999)), (3, 4, 5));
        // 12 buildings, a turret saves one; the tease destroys none.
        assert_eq!(r.buildings_lost(0), 12);
        assert_eq!(r.buildings_lost(4), 8);
        assert_eq!(r.buildings_lost(20), 0);
        assert_eq!(AttackKind::GroxTease.row().buildings_lost(0), 0);
        assert_eq!(AttackKind::AllyRaid.row().ally_fail_delta, -2.0);
    }

    #[test]
    fn a_raid_arrives_after_its_wait_and_fails_when_raiders_remain_at_the_deadline() {
        let mut c = campaign_with_colony();
        let world = c.colonies[1];
        // The player is away: the raiders arrive when the wait is over (0xC5E4C4).
        c.current_system = crate::space_galaxy::system_of_world(world) + 1;
        c.universe.attacks.push(Attack::new(AttackKind::War, Some(3), None, world, 4, c.kills));
        for _ in 0..59 {
            tick(&mut c, 1.0, None);
        }
        assert!(!c.universe.attacks[0].arrived, "still waiting at 59 s");
        tick(&mut c, 2.0, None);
        assert!(c.universe.attacks[0].arrived, "arrived after the 60 s wait");
        assert!(c.universe.attacks[0].entered_at.is_none(), "the player was not there");
        // Raiders are still alive until wait + duration = 360 s: the mission fails then.
        for _ in 0..297 {
            tick(&mut c, 1.0, None);
        }
        assert_eq!(c.universe.attacks.len(), 1, "still active at 359 s");
        assert_eq!(c.universe.buildings_lost, 0);
        let alerts = tick(&mut c, 2.0, None);
        assert!(c.universe.attacks.is_empty());
        // The colony has four turrets: 12 - 4 buildings are lost (0xC5E7E2 deadline).
        assert_eq!(c.universe.buildings_lost, 8);
        assert!(alerts.iter().any(|a| a.contains("destroyed 8 buildings")), "{alerts:?}");
    }

    #[test]
    fn the_player_at_the_world_brings_the_raiders_at_once_and_clearing_them_completes_the_raid() {
        let mut c = campaign_with_colony();
        let world = c.colonies[1];
        c.current_system = crate::space_galaxy::system_of_world(world);
        c.universe.attacks.push(Attack::new(AttackKind::Grox, Some(0), None, world, 6, c.kills));
        tick(&mut c, 1.0, None);
        assert!(c.universe.attacks[0].arrived, "the player is on the planet: no wait");
        assert_eq!(c.universe.attacks[0].entered_at, Some(1.0), "mTimeOfArrival is set once");
        tick(&mut c, 5.0, None);
        assert_eq!(c.universe.attacks[0].entered_at, Some(1.0));
        c.kills += 5;
        tick(&mut c, 1.0, None);
        assert_eq!(c.universe.attacks.len(), 1, "five of six fighters: not cleared");
        c.kills += 1;
        let alerts = tick(&mut c, 1.0, None);
        assert!(alerts.iter().any(|a| a.contains("repelled")), "{alerts:?}");
        assert!(c.universe.attacks.is_empty());
        assert_eq!((c.pirate_raids_defended, c.universe.buildings_lost), (1, 0));
    }

    #[test]
    fn the_grox_teaser_raid_leaves_25_seconds_after_the_player_entered() {
        let mut c = campaign_with_colony();
        let world = c.colonies[1];
        c.current_system = crate::space_galaxy::system_of_world(world);
        c.universe.attacks.push(Attack::new(AttackKind::GroxTease, Some(0), None, world, 4, c.kills));
        // 0xC5E64C: elapsed - timeOfArrival must exceed 25 s (0x61A8): the entry is
        // noticed on the first step (t = 1 s), so the UFOs go on the step of t = 26.5 s.
        for _ in 0..26 {
            tick(&mut c, 1.0, None);
        }
        assert_eq!(c.universe.attacks.len(), 1, "25 s after the entry the UFOs are still there");
        let alerts = tick(&mut c, 0.5, None);
        assert!(c.universe.attacks.is_empty());
        assert!(alerts.iter().any(|a| a.contains("left")), "{alerts:?}");
        // 0xC5BB66: a raid whose UFOs leave counts for nothing, and it destroyed nothing.
        assert_eq!((c.pirate_raids_defended, c.universe.buildings_lost), (0, 0));
        // Away from the world the UFOs never leave: it fails at its deadline but loses nothing.
        let mut d = campaign_with_colony();
        d.current_system = crate::space_galaxy::system_of_world(world) + 1;
        d.universe.attacks.push(Attack::new(AttackKind::GroxTease, Some(0), None, world, 4, d.kills));
        for _ in 0..915 {
            tick(&mut d, 1.0, None);
        }
        assert!(d.universe.attacks.is_empty());
        assert_eq!(d.universe.buildings_lost, 0, "missionRaidBuildingsDestroyed is 0 for the tease");
    }

    #[test]
    fn an_ally_raid_that_fails_with_the_player_away_costs_the_allys_trust() {
        let mut c = SpaceCampaign::default();
        let ally = empire_of(SpaceArchetype::Diplomat);
        c.meet_empire(ally);
        c.allies.push(ally);
        let world = crate::space_galaxy::world_id(5, 0);
        c.current_system = 9;
        c.universe.attacks.push(Attack::new(AttackKind::AllyRaid, None, Some(ally), world, 3, c.kills));
        let before = c.relationship(ally);
        for _ in 0..361 {
            tick(&mut c, 1.0, None);
        }
        assert!(c.universe.attacks.is_empty());
        // 0xC5BC50: MissionFailed of |missionRaidAllyFailRelationshipDelta| (2.0).
        assert!(c.relationships[&ally].events.contains_key(&RelationshipEvent::MissionFailed));
        assert!(c.relationship(ally) <= before);
        assert_eq!(c.ally_raids_defended, 0);
    }

    #[test]
    fn npc_rules_parse_like_the_retail_rule_loader() {
        let military = NpcRules::parse(NPC_ACTION_RULES[0]);
        // "10 outpost -color Blue -nearMe 1 -nearEnemy 1 -nearNeutral -3".
        assert_eq!(military.0[NpcAction::Outpost as usize][2][..4], [10.0, 1.0, 1.0, -3.0]);
        // The later "1 outpost -color Red" replaces "1 outpost -color Red -nearMe -1".
        assert_eq!(military.0[NpcAction::Outpost as usize][0], [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        // A line without a colour serves all three: nasty -ally -8 -player 3
        // -enemy -99 -empireStars 0.5 (slots 5, 7, 6, 9).
        for c in 0..3 {
            let r = military.0[NpcAction::Nasty as usize][c];
            assert_eq!((r[0], r[1], r[5], r[6], r[7], r[9]), (1.0, 2.0, -8.0, -99.0, 3.0, 0.5));
        }
        // Economic empires have no attack / nasty rules (score 0, never > 1).
        let economic = NpcRules::parse(NPC_ACTION_RULES[2]);
        assert_eq!(economic.0[NpcAction::Nasty as usize][1], [0.0; 11]);
        let f = NpcFeatures { near_me: 0.5, player: 1.0, empire_stars: 6.0, ..Default::default() };
        assert_eq!(military.score(NpcAction::Nasty, 1, &f), 1.0 + 1.0 + 3.0 + 3.0);
        // The category table 0x15B46D0: Grox / warrior / zealot military,
        // trader / diplomat economic, the rest colonise.
        assert_eq!(npc_category(0), 0);
        assert_eq!(npc_category(12), 0, "warrior");
        assert_eq!(npc_category(15), 2, "trader");
        assert_eq!(npc_category(14), 2, "diplomat");
        assert_eq!(npc_category(16), 1, "bard");
        assert_eq!(colour_index(StarKind::BinaryGM), 1);
        assert_eq!(colour_index(StarKind::BinaryOM), 2);
        assert_eq!(colour_index(StarKind::BinaryMM), 0);
    }

    #[test]
    fn npc_turns_follow_the_egs_timers_and_place_outposts() {
        let galaxy = Galaxy::generate(None);
        let e = galaxy.empires.iter().find(|r| r.stars >= 4 && npc_category(r.id) == 0).expect("a military empire").id;
        let mut c = SpaceCampaign::default();
        c.current_system = galaxy.empire_by_id(e).unwrap().home;
        c.current_system_empire = Some(e);
        let mut d = c.clone();
        // No turn before `maxTimeEGS` (10 minutes) outside the galaxy view.
        for _ in 0..599 {
            c.universe_clock += 1.0;
            tick(&mut c, 1.0, Some(&galaxy));
        }
        assert!(c.universe.turn_queue.is_empty() && c.universe.grown.is_empty());
        c.universe_clock += 1.0;
        tick(&mut c, 1.0, Some(&galaxy));
        let queued = c.universe.turn_queue.clone();
        assert!(queued.contains(&e), "the empire of the system takes the turn: {queued:?}");
        assert!(queued.windows(2).all(|w| w[0] < w[1]), "a sorted set, popped from the back");
        // One empire acts per frame.
        for _ in 0..queued.len() {
            c.universe_clock += 1.0;
            tick(&mut c, 1.0, Some(&galaxy));
        }
        assert!(c.universe.turn_queue.is_empty());
        assert!(!c.universe.grown.is_empty(), "empires placed outposts");
        assert!(c.universe.grown.keys().all(|s| c.universe.outposts.contains(s)));
        // Entering the galaxy view after 5 minutes outside it starts a turn.
        for _ in 0..300 {
            d.universe_clock += 1.0;
            tick(&mut d, 1.0, Some(&galaxy));
        }
        assert!(d.universe.turn_queue.is_empty());
        d.universe.galaxy_view = true;
        tick(&mut d, 0.1, Some(&galaxy));
        assert!(!d.universe.turn_queue.is_empty(), "galaxy entry after 300 s");
        // The choice is deterministic and an outpost is later upgraded.
        let first = choose_action(&c, &galaxy, e);
        assert_eq!(first, choose_action(&c.clone(), &galaxy, e));
        if let Some((star, NpcAction::Upgrade, _)) = first {
            c.universe.galaxy_view = true;
            npc_act(&mut c, &galaxy, e);
            assert!(!c.universe.outposts.contains(&star));
        }
    }

    #[test]
    fn npc_attacks_on_the_player_need_contact_and_awareness() {
        let galaxy = Galaxy::generate(None);
        let e = galaxy.empires.iter().find(|r| npc_category(r.id) == 0).expect("military").id;
        let home = galaxy.empire_by_id(e).unwrap().home;
        // A player colony right next to the empire's home.
        let next = galaxy.stars_within(home, 3.0).into_iter().map(|(i, _)| i).find(|i| galaxy.star(*i).is_some_and(|s| s.empire.is_none() && s.kind.has_planets()));
        let Some(next) = next else { return };
        let mut c = SpaceCampaign::default();
        c.current_system = home;
        c.colonies = vec![0, crate::space_galaxy::world_id(next, 0)];
        let player_target = |c: &SpaceCampaign| choose_action(c, &galaxy, e).is_some_and(|(s, _, _)| s == next);
        assert!(!player_target(&c), "no first contact: the player's star is no candidate");
        c.universe.contacted.insert(e);
        c.awareness.insert(e, 1.0);
        assert!(!player_target(&c), "only aware: still no candidate");
        c.awareness.insert(e, 5.0);
        c.meet_empire(e);
        if let Some((star, action, _)) = choose_action(&c, &galaxy, e) {
            if star == next {
                assert_ne!(action, NpcAction::Outpost);
            }
        }
    }

    #[test]
    fn the_universe_state_survives_a_save() {
        let mut c = campaign_with_colony();
        c.universe.grown.insert(42, 7);
        c.universe.contacted.insert(7);
        c.universe.inbox.push(Incoming { empire: Some(7), file: INCOMING_FILE, dialog: dialog::OFFER_GIFT, world: 3, amount: 1.0, time_left: 5.0 });
        npc_nice_nasty(&mut c, 7, 9, true);
        c.universe.npc_alliances.insert((7, 9));
        c.rares.found.insert("rare-jewel1".into());
        c.rares.detector.insert(1234, true);
        let json = serde_json::to_string(&c).unwrap();
        let back: SpaceCampaign = serde_json::from_str(&json).unwrap();
        assert_eq!(back.universe, c.universe);
        // An old save without the field loads with an empty universe.
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut().unwrap().remove("universe");
        let old: SpaceCampaign = serde_json::from_value(v).unwrap();
        assert_eq!(old.universe, UniverseState::default());
        assert_eq!(back.rares, c.rares);
    }

    #[test]
    fn npc_empires_keep_relationships_alliances_and_wars() {
        let mut c = SpaceCampaign::default();
        let (a, b) = (empire_of(SpaceArchetype::Diplomat), empire_of(SpaceArchetype::Trader));
        assert_eq!(npc_relationship(&c, a, b), 0.0);
        // BeNice +2 both ways per nice action (directed pairs).
        npc_nice_nasty(&mut c, a, b, true);
        assert!(npc_relationship(&c, a, b) > 0.0 && npc_relationship(&c, b, a) > 0.0);
        assert!(c.universe.npc_alliances.is_empty());
        // Repeated before the decay wears it off: past createAllianceBase 7.
        for _ in 0..4 {
            npc_nice_nasty(&mut c, a, b, true);
        }
        assert!(npc_relationship(&c, a, b) >= rules::CREATE_ALLIANCE_THRESHOLD);
        assert!(c.universe.npc_alliances.contains(&(a.min(b), a.max(b))));
        // An attack: DestroyBuilding x 20 on the owner's view, war, no alliance.
        let before = npc_relationship(&c, b, a);
        npc_attack(&mut c, a, b);
        assert!(npc_relationship(&c, b, a) <= before - 9.9);
        assert!(c.universe.npc_wars.contains(&(a.min(b), a.max(b))));
        assert!(c.universe.npc_alliances.is_empty());
        // The 60 s decay wears the events off.
        for _ in 0..200 {
            tick(&mut c, 60.0, None);
        }
        assert!(c.universe.npc_relations.is_empty());
        // Nasty: BeNasty -4 both ways; war only at -7 or below.
        let mut d = SpaceCampaign::default();
        npc_nice_nasty(&mut d, a, b, false);
        assert!(npc_relationship(&d, a, b) < 0.0 && d.universe.npc_wars.is_empty());
        npc_nice_nasty(&mut d, a, b, false);
        assert!(d.universe.npc_wars.contains(&(a.min(b), a.max(b))));
    }

    #[test]
    fn npc_wars_start_with_started_war_and_end_in_an_alliance() {
        let mut c = SpaceCampaign::default();
        let (a, b) = (empire_of(SpaceArchetype::Warrior), empire_of(SpaceArchetype::Diplomat));
        let pair = (a.min(b), a.max(b));
        // DeclareWar 0xD065E0: StartedWar x 1 on both views.
        npc_declare_war(&mut c, a, b);
        assert!(c.universe.npc_wars.contains(&pair));
        let (ab, ba) = (npc_relationship(&c, a, b), npc_relationship(&c, b, a));
        assert!(ab < 0.0 && ba < 0.0);
        // At war already: no second StartedWar.
        npc_declare_war(&mut c, a, b);
        assert_eq!((npc_relationship(&c, a, b), npc_relationship(&c, b, a)), (ab, ba));
        // Nice actions toward the enemy (the rules' `-enemy` weight) until
        // createAllianceBase: DeclareAlliance -> DeclarePeace.
        let mut n = 0;
        while c.universe.npc_wars.contains(&pair) && n < 40 {
            npc_nice_nasty(&mut c, a, b, true);
            tick(&mut c, 60.0, None);
            n += 1;
        }
        assert!(!c.universe.npc_wars.contains(&pair), "still at war after {n} nice turns");
        assert!(c.universe.npc_alliances.contains(&pair));
        // The nice rules weigh `-enemy` positive in every rule set.
        for set in NPC_ACTION_RULES {
            let rules = NpcRules::parse(set);
            for colour in 0..3 {
                // Slot 6: `-enemy`.
                assert!(rules.0[NpcAction::Nice as usize][colour][6] > 0.0, "{set:?}");
            }
        }
    }

    #[test]
    fn the_stance_update_declares_and_ends_wars_with_the_player() {
        let mut c = SpaceCampaign::default();
        let e = 3;
        c.apply_relationship(e, RelationshipEvent::StartedWar, 1.0);
        c.apply_relationship(e, RelationshipEvent::BeNasty, 1.0);
        c.wars.retain(|w| *w != e);
        assert!(c.relationship(e) < rules::WAR_THRESHOLD);
        // 0x1013370: below warThreshold and not at war -> war.
        let alerts = tick(&mut c, 0.1, None);
        assert!(c.wars.contains(&e), "{alerts:?}");
        // A bought peace resets the relationship: the update keeps it.
        c.relationships.remove(&e);
        c.meet_empire(e);
        tick(&mut c, 0.1, None);
        assert!(!c.wars.contains(&e));
        // An ally below endAlliance loses the alliance.
        c.allies.push(e);
        c.apply_relationship(e, RelationshipEvent::BeNasty, 1.0);
        tick(&mut c, 0.1, None);
        assert!(!c.allies.contains(&e));
    }

    #[test]
    fn the_upgrade_action_makes_the_colony_world_t2() {
        let mut c = SpaceCampaign::default();
        let star = 4242;
        upgrade_colony(&mut c, star);
        let e = c.ecology_for(crate::space_galaxy::world_id(star, 0));
        assert_eq!((e.temperature, e.atmosphere), (UPGRADE_TEMPERATURE, UPGRADE_ATMOSPHERE));
        assert_eq!(e.t_score(), 2);
        assert_eq!(e.colony_count, 0);
        // No food web: the player's food-web badge does not count it.
        assert_eq!(c.badge_counter(rules::BadgeCounter::FoodWebsComplete), 0);
    }
}
