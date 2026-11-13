//! Creature encounters, played the way Spore's Creature stage plays them:
//! * Social: the other creature performs a sequence of social actions, one at
//!   a time (an action bubble above it shows which); answering each with the
//!   same action fills its friendship bar, higher ranks filling more. Wrong
//!   answers lower the bar, and an ignored performance makes it lose
//!   interest. A full bar befriends it; befriending a whole nest allies the
//!   nest (DNA, a part, a place to heal) and allies can join the pack.
//! * Combat: attacking makes the target (and its species) fight back, also
//!   against pack members; kills give DNA and leave meat; killing a whole
//!   nest destroys it, and the last nest's fall makes the species extinct.
//! * Roaming epic and rogue creatures (`epic_creatures`) use the same rules
//!   with their own health, rank gates and rewards.
//!
//! Capability identity and ranks come from the authored parts. Ability damage,
//! refresh, range and combat effects are the retail per-level tables and DNA
//! rewards follow the retail formula (`creature_rules`, notes C1/C2); social
//! timing and the friendship contest are still OpenSpore tuning.
use crate::{
    asset_catalog::Capabilities,
    creature_social::CreatureAwards,
    epic_creatures::{RoamerActor, RoamerStats},
    game::*,
    persistence::WorldId,
    species::{NestStatus, RecordResult, Relationship, RoamerKind},
    stages::GameStage,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CreatureMind {
    pub species: u8,
    pub allied: bool,
    pub defeated: bool,
    /// The friendship bar (0..=100); a full bar befriends the creature.
    pub impression: f32,
    /// The social action the creature is performing right now (0..4).
    pub requested: u8,
    /// Correct answers so far in this performance sequence.
    pub beats: u8,
    /// Seconds left to answer the current performance.
    pub response_time: f32,
    pub attack_timer: f32,
    /// Seconds before a knocked-out creature (beaten by a hunter or a giant,
    /// never by the player) returns to its nest. Old checkpoints load as 0.
    #[serde(default)]
    pub downed: f32,
    /// Seconds a creature that lost interest refuses another performance.
    #[serde(default)]
    pub bored: f32,
    /// Seconds of stun left (a charge hit): it neither moves nor attacks.
    #[serde(default)]
    pub stunned: f32,
    /// Poison from a spit hit: seconds left and HP lost per second.
    #[serde(default)]
    pub poisoned: f32,
    #[serde(default)]
    pub poison_dps: f32,
    /// Knockback still to apply (a charge hit): ground-plane world units,
    /// consumed by `creature_rules::apply_knockback`.
    #[serde(default)]
    pub shove: [f32; 2],
    /// A social step is under way: the creature demonstrated an ability and
    /// waits for the answer. Cleared when the step ends (wrong answer, timeout,
    /// befriended).
    #[serde(default)]
    pub performing: bool,
    /// Social rounds this creature asks for (retail herd archetype
    /// `numSocialProposals`, +1 for an alpha). The friendship bar holds this
    /// many rounds' worth of answers; 0 (older checkpoints, legacy species)
    /// reads as one round.
    #[serde(default)]
    pub rounds: u8,
    /// Attacked by the player's side, or alerted to help a herd mate that was
    /// (creature_support): a best-friends herd does not calm it again.
    #[serde(default)]
    pub provoked: bool,
    /// The social abilities (tray slots) this creature asks for, one per round
    /// (retail 0xDA4360 picks them at random from its species' social
    /// abilities when it proposes and keeps them on the creature, +0x1020,
    /// reusing them while the round count is unchanged).
    #[serde(default)]
    pub requests: Vec<u8>,
    /// Round of the current proposal (0-based).
    #[serde(default)]
    pub round: u8,
    /// The creature's own level of the ability it asks for now.
    #[serde(default)]
    pub requested_level: u8,
    /// Social bar maximum in power units: twice the levels of the requested
    /// abilities (0xDA4360: behaviour +0x1BC). 0 before the first proposal.
    #[serde(default)]
    pub bar_max: f32,
    /// The creature's side of the bar: each demonstration adds its ability's
    /// level (0xDA4BBE -> 0xD9AFF0, row 0); it drains when the player waits.
    #[serde(default)]
    pub npc_fill: f32,
    /// The player's side of the bar: the answers (0xD9B070 -> 0xD9B030, row 1).
    #[serde(default)]
    pub player_fill: f32,
    /// Seconds left before the creature's side starts to drain
    /// (`socialTimeBeforeDrain`), counted once the round is open.
    #[serde(default)]
    pub drain_timer: f32,
    /// The drain timer was started for this round (bar record +0xC).
    #[serde(default)]
    pub drain_armed: bool,
    /// The creature's side is draining (bar record +0xD): it shows impatience.
    #[serde(default)]
    pub draining: bool,
    /// The clip of the creature's last attack (its ability's
    /// `creatureAbilityAnimationIDs`) and a counter bumped on every attack, so
    /// the clip player can start it (`locomotion_preview::npc_request`).
    #[serde(default)]
    pub attack_clip: u32,
    #[serde(default)]
    pub attack_serial: u32,
    /// Group social (notes C10A, 0xDA4360 / 0xDA4950), proposer side: herd
    /// mates that answered its call, in slot order (`creature_group`).
    #[serde(default)]
    pub joiners: Vec<u32>,
    /// Joiners standing in their slot when the gathering ended (behaviour
    /// +0x54, `n`): the bar is `(n + 1)` times as long and every
    /// demonstration adds `(n + 1)` levels to the creature's side.
    #[serde(default)]
    pub group: u8,
    /// Herd mates still to call (behaviour +0x1F4, `SocialGroupSize - 1`)
    /// before the gathering starts; 0 once called.
    #[serde(default)]
    pub group_wanted: u8,
    /// Seconds left of the gathering (`SOCIAL_GROUP_WAIT_SECONDS`): the
    /// creature calls and waits; its proposal starts when it runs out or every
    /// joiner stands in its slot. 0 when not gathering.
    #[serde(default)]
    pub gather: f32,
    /// Joiner side: the proposer this creature answered (its world id).
    #[serde(default)]
    pub joining: Option<u32>,
    /// Joiner side: where its slot is (ground-plane x, z), refreshed every frame.
    #[serde(default)]
    pub slot_at: Option<[f32; 2]>,
    /// Bumped on every demonstration, so the proposer's joiners copy it.
    #[serde(default)]
    pub demo_serial: u32,
    /// A clip another creature asked this one to play (the proposer's
    /// broadcast, event type 3 via 0xD9B190: the joiners play its
    /// demonstration clip) and its serial.
    #[serde(default)]
    pub cue_clip: u32,
    #[serde(default)]
    pub cue_serial: u32,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum EncounterMode {
    #[default]
    Social,
    Combat,
}
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Encounter {
    pub target: Option<u32>,
    pub mode: EncounterMode,
    pub cooldowns: [f32; 4],
    pub message: String,
    /// Seconds the avatar is still stunned by a creature's charge.
    pub stun: f32,
    /// Poison on the avatar (a creature's spit): seconds left, HP per second.
    pub poison_seconds: f32,
    pub poison_dps: f32,
    /// The creature that last hit the avatar, defended against by the pack
    /// members that were within `creatureHelpAllyInRadius` of it
    /// (creature_support, 0xD8F1C0), for `defend_time` seconds.
    pub defend: Option<u32>,
    pub defend_time: f32,
    pub defenders: Vec<u32>,
    /// The player's nest mates (`creature_nestmates`), set every frame before
    /// this system runs: (mate id from `MATE_ID_BASE`, ground position). The
    /// creatures that pick a victim may pick a mate; the hits they land are
    /// collected in `mate_hits` (mate id, damage, attacker's world id) and the
    /// strikes the mates answer with arrive in `mate_strikes` (target id,
    /// damage), applied like a pack member's.
    #[serde(skip)]
    pub mates: Vec<(u32, Vec3)>,
    #[serde(skip)]
    pub mate_hits: Vec<(u32, f32, u32)>,
    #[serde(skip)]
    pub mate_strikes: Vec<(u32, f32)>,
    /// The avatar's charge in flight (`creature_walk` moves it, `drive_target`
    /// lands the hit). Never saved: a checkpoint is not taken mid-rush.
    #[serde(skip)]
    pub rush: Option<Rush>,
    /// The last reason an ability button did nothing (the retail verb tray
    /// check, 0xD1F650 -> button handler 0xD49560) and a counter bumped on every
    /// failure, so the hint layer shows the same hint again for a new failure.
    #[serde(skip)]
    pub error: Option<AbilityError>,
    #[serde(skip)]
    pub error_serial: u32,
    /// The avatar's attack walking to its launch point (retail attack behaviour
    /// 0xD809D0, see [`Launch`]). `creature_controls` walks it, `drive_target`
    /// starts the ability once the target is within reach. Never saved.
    #[serde(skip)]
    pub launch: Option<Launch>,
}

/// An attack ability the avatar started on a target that is still too far away.
/// Retail (VERIFIED by the earlier decode of 0xD809D0): the attack behaviour
/// does not refuse a far target, it walks the creature to the launch point at
/// `(mRushingRange + mRange)` of the target (`WalkTo` 0xC1C720) and then starts
/// the ability (0xC032F0); the verb tray check (0xD1F650) only refuses a target
/// that is too close, has no target, or is recharging.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Launch {
    /// World id of the targeted creature.
    pub target: u32,
    /// The ability button (0-based slot of the attack tray).
    pub slot: usize,
    /// How close to the target's centre the walk ends: the launch distance less
    /// a margin (so the range test passes), plus the target's body radius.
    pub stop: f32,
}

/// ADAPTED: the walk ends this far inside the launch distance, so the next
/// range test (`drive_target`) is already satisfied when the avatar stops.
pub const LAUNCH_MARGIN: f32 = 0.4;

/// Why an ability button did nothing. The numbers are the codes the verb tray
/// check 0xD1F650 returns and the button handler 0xD49560 switches on (VERIFIED:
/// 2 `crg-spitclose`, 3 / 4 `crg-notarget`, 6 `crg-socialalreadyfriends`, 8
/// `crg-abilitynotready`, 10 `crg-superpowerlocked`; each is a
/// `cHintManager::ShowHint` with a 5 s display time (priority 100 as installed,
/// `crg-spitclose` has 1), see
/// `notes\creature-audit-b.md`). The hint layer (`creature_hud::guide`) shows
/// `hint()` when `Encounter::error_serial` changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityError {
    /// 2: the target is nearer than the ability's minimum range (0xD1E4B0:
    /// `cCreatureAbility::GetRangeMin(isAvatar)` above the distance to it).
    TargetTooClose,
    /// 3 (attack abilities, 0xD1E530) and 4 (social, 0xD1E720): nothing is targeted.
    NoTarget,
    /// 6: a social ability used on a creature that is already a friend (0xD1E720).
    SocialAlreadyFriends,
    /// 8: the ability is still recharging (0xD1E8D0).
    NotReady,
    /// Not a verb tray code: the eat behaviour refuses a food the diet does not
    /// allow (a carcass for a creature without the carnivore level, a fruit for
    /// one without the herbivore level; 0xC0B8E0 / 0xC0B900 in the carcass update
    /// 0xD71500 and the fruit one 0xD882C9) by posting the refuse goal
    /// 0x20000000, whose start 0xD70A20 shows `crg-wrongfood` for the avatar
    /// (0xD70A6A; priority 100, 5 s, not an `always` hint).
    WrongFood,
}

impl AbilityError {
    /// Every error the avatar's ability buttons raise.
    pub const ALL: [AbilityError; 5] = [
        AbilityError::TargetTooClose,
        AbilityError::NoTarget,
        AbilityError::SocialAlreadyFriends,
        AbilityError::NotReady,
        AbilityError::WrongFood,
    ];

    /// The error whose message line is `message` (the hint layer shows the retail
    /// hint instead of the message text).
    pub fn from_message(message: &str) -> Option<AbilityError> {
        Self::ALL.into_iter().find(|error| error.text() == message)
    }

    /// The code the verb tray check (0xD1F650) returns.
    pub fn code(self) -> u8 {
        match self {
            AbilityError::TargetTooClose => 2,
            AbilityError::NoTarget => 3,
            AbilityError::SocialAlreadyFriends => 6,
            AbilityError::NotReady => 8,
            // Not a verb tray code (the eat behaviour raises it).
            AbilityError::WrongFood => 0,
        }
    }
    /// The hint the button handler shows (`HintsCRG`).
    pub fn hint(self) -> &'static str {
        match self {
            AbilityError::TargetTooClose => "crg-spitclose",
            AbilityError::NoTarget => "crg-notarget",
            AbilityError::SocialAlreadyFriends => "crg-socialalreadyfriends",
            AbilityError::NotReady => "crg-abilitynotready",
            AbilityError::WrongFood => "crg-wrongfood",
        }
    }
    /// The hint's installed text (`HintsCRG`): the encounter line says the same.
    pub fn text(self) -> &'static str {
        match self {
            AbilityError::TargetTooClose => "You're too close",
            AbilityError::NoTarget => "You don't have a target",
            AbilityError::SocialAlreadyFriends => "Already friends: socialize with someone new",
            AbilityError::NotReady => "Ability is cooling down",
            // ADAPTED: the message line only; the hint bar shows the installed text.
            AbilityError::WrongFood => "That food is not for your diet",
        }
    }
}

impl Encounter {
    /// An ability button failed: remember why and say it on the message line.
    pub fn fail(&mut self, error: AbilityError) {
        self.error = Some(error);
        self.error_serial = self.error_serial.wrapping_add(1);
        self.message = error.text().into();
    }
}

/// The charge rush (retail `cCreatureAbility` +0xF4 `mRushingRange` 20 at +0xFC
/// `mRushingSpeed` 20, +0x100 `mbRushingTargetedCharge`, +0x101
/// `mbRushingDoDamage`; the ability starts at least `mRangeMin` (+0x134, 14 at
/// level 1, else 10) from its target, 0xD1E4B0). The approach window of the
/// attack behaviour (0xD809D0) aims at `(mRushingRange + mRange)`; the charge
/// itself runs along the target at the rushing speed, up to the rushing range,
/// and the hit (damage, stun, knock-back) lands when it reaches the target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rush {
    /// World id of the targeted creature.
    pub target: u32,
    /// Part rank of the charge that started the rush.
    pub rank: u8,
    /// Where the target stands and the gap to its body edge, refreshed every
    /// frame by `drive_target` (a targeted charge follows the target).
    pub goal: Vec3,
    pub gap: f32,
    /// The rush ends this far from the body edge (the ability's reach).
    pub stop: f32,
    pub travelled: f32,
    /// `mRushingRange` and `mRushingSpeed`.
    pub max: f32,
    pub speed: f32,
    /// The avatar caught up with the target: the hit lands next.
    pub reached: bool,
    /// The rushing range ran out before it did.
    pub missed: bool,
}

impl Rush {
    /// A rush from `gap` away from `target` at `goal`, with the retail range /
    /// speed and the hit landing within `stop`.
    pub fn start(target: u32, rank: u8, goal: Vec3, gap: f32, stop: f32) -> Self {
        Rush {
            target,
            rank,
            goal,
            gap,
            stop,
            travelled: 0.0,
            max: crate::creature_rules::CHARGE_RUSH_RANGE,
            speed: crate::creature_rules::CHARGE_RUSH_SPEED,
            reached: false,
            missed: false,
        }
    }
}

/// One frame of the rush: the avatar at `position` runs straight at the target
/// (`rush.goal`) at the rushing speed and stops within `rush.stop` of its body
/// edge. Returns the new position on the ground plane (the height is kept);
/// sets `reached` when it is close enough and `missed` when the rushing range
/// is spent first.
pub fn rush_step(rush: &mut Rush, position: Vec3, dt: f32) -> Vec3 {
    if rush.reached || rush.missed || !(dt.is_finite() && dt > 0.0) {
        return position;
    }
    let to = Vec3::new(rush.goal.x - position.x, 0.0, rush.goal.z - position.z);
    let centre = to.length();
    let to_stop = (rush.gap - rush.stop).max(0.0);
    let advance = (rush.speed * dt).min(to_stop).min((rush.max - rush.travelled).max(0.0));
    let moved = if centre > 1.0e-4 && advance > 0.0 {
        position + to / centre * advance
    } else {
        position
    };
    rush.travelled += advance;
    rush.gap -= advance;
    if rush.gap <= rush.stop + 1.0e-3 {
        rush.reached = true;
    } else if rush.travelled >= rush.max - 1.0e-4 {
        rush.missed = true;
    }
    moved
}
pub const SOCIAL: &[&str] = &["Sing", "Dance", "Charm", "Pose"];
pub const COMBAT: &[&str] = &["Bite", "Charge", "Strike", "Spit"];
/// One social round (retail step 0xDA4CAA, notes C3A): the NPC demonstrates its
/// ability for 5 s, then waits `creatureSocialResponseTimeout` (15 s) for the
/// answer; unanswered, the step fails. This is the whole prompt time: answers
/// count once it has dropped to the 15 s answer window. The player's own 5 s
/// ability refresh (the native social refresh) runs alongside.
pub const SOCIAL_RESPONSE_WINDOW: f32 =
    crate::creature_rules::SOCIAL_DEMONSTRATION_SECONDS + crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
pub const SOCIAL_REFRESH_FALLBACK: f32 = crate::creature_rules::SOCIAL_REFRESH;

/// Canonical original ability-template group (`creatureabilityTemplates~` in
/// SporeModder-FX reg_file.txt). Native `cCreatureAbility` type IDs are read
/// from each installed PROP after normal property inheritance.
pub const ABILITY_TEMPLATE_GROUP: u32 = 0xD2FCB262;

/// These eight files are the installed combat/social ability templates.
/// Their ranks come from equipped creature parts, not separate template files.
const NATIVE_ABILITY_TEMPLATES: &[(&str, u32)] = &[
    ("BiteTemplate", 30),
    ("ChargeTemplate", 31),
    ("StrikeTemplate", 33),
    ("SpitTemplate", 32),
    ("VocalizeTemplate", 37),
    ("DanceTemplate", 34),
    ("CharmTemplate", 36),
    ("PostureTemplate", 35),
];

#[derive(Debug, Clone)]
pub struct AuthoredAbility {
    pub source: crate::dbpf::ResourceKey,
    pub name: String,
    pub ability_type: u32,
    pub damage: Option<f32>,
    pub ranged_damage: Option<f32>,
    /// Native `creatureAbilityRefresh` (01B8346E), populated by these retail
    /// templates. Whether it drives the same cooldown trigger used here is
    /// unverified; the distinct `creatureAbilityRecharge` field is absent.
    pub refresh: Option<f32>,
    /// Preserved separately because the native type exposes both properties.
    pub recharge: Option<f32>,
    pub range: Option<f32>,
    pub duration: Option<f32>,
    pub level_multiplier: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct CreatureAbilityCatalog {
    pub abilities: Vec<AuthoredAbility>,
    pub errors: Vec<String>,
    /// The per-level ability props' clips (`CreatureAbilities!<name><level>`).
    pub clips: Vec<AbilityClip>,
}

/// The clips of one ability level, read from its installed prop (with its
/// template's values inherited): `creatureAbilityAnimationIDs` (0x052047F7,
/// the first entry) and, for charge, `creatureAbilityRushingAnimationID`
/// (0x04A3B2F8, the run-up). VERIFIED: the social step plays
/// `cCreatureAbility::GetAnimationID(ability, 0)` (0x4D3D30, called at
/// 0xDA4BE8) for the requested ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbilityClip {
    pub ability_type: u32,
    pub level: u8,
    pub animation: u32,
    pub rushing: Option<u32>,
}

/// `CreatureAbilities` (group of the per-level ability props).
pub const ABILITY_GROUP_NAME: &str = "CreatureAbilities";
/// The per-level props whose clips the Creature stage plays: tray abilities
/// (level 1..=5) plus call, epic stomp and epic spit (level 0).
const CLIP_ABILITIES: &[(&str, u32, bool)] = &[
    ("bite", 30, true),
    ("charge", 31, true),
    ("strike", 33, true),
    ("spit", 32, true),
    ("vocalize", 37, true),
    ("dance", 34, true),
    ("charm", 36, true),
    ("posture", 35, true),
    ("call1", 16, false),
    ("epicstomp", 41, false),
    ("epicspit", 32, false),
];

impl CreatureAbilityCatalog {
    /// The clip of an ability type at a level (`level` 0 for the unlevelled
    /// epic abilities and call).
    pub fn clip(&self, ability_type: u32, level: u8) -> Option<AbilityClip> {
        self.clips.iter().copied().find(|c| c.ability_type == ability_type && c.level == level)
    }

    /// The clip of a tray slot at `rank` (1..=5).
    pub fn slot_clip(&self, mode: EncounterMode, slot: usize, rank: u8) -> Option<AbilityClip> {
        let ability_type = match mode {
            EncounterMode::Social => *[37, 34, 36, 35].get(slot)?,
            EncounterMode::Combat => *[30, 31, 33, 32].get(slot)?,
        };
        self.clip(ability_type, rank.clamp(1, 5))
    }

    fn load_clips(&mut self, index: &crate::asset_catalog::ResourceIndex) {
        let group = crate::game::spore_hash(ABILITY_GROUP_NAME);
        let raw_u32 = |p: &crate::prop::Property| -> Option<u32> {
            let v = p.values.first()?;
            if !matches!(p.kind, 9 | 10) {
                return None;
            }
            Some(u32::from_be_bytes(v.get(..4)?.try_into().ok()?))
        };
        for &(name, ability_type, levelled) in CLIP_ABILITIES {
            let levels: Vec<(u8, String)> = if levelled {
                (1..=5u8).map(|l| (l, format!("{name}{l}"))).collect()
            } else {
                vec![(0, name.to_string())]
            };
            for (level, prop) in levels {
                let key = crate::dbpf::ResourceKey {
                    group_id: group,
                    instance_id: crate::game::spore_hash(&prop),
                    type_id: crate::asset_catalog::PROP,
                };
                if !index.records.contains_key(&key) {
                    continue;
                }
                let Ok(props) = index.properties(key) else { continue };
                let Some(animation) = props.0.get(&0x0520_47F7).and_then(raw_u32).filter(|a| *a != u32::MAX && *a != 0) else {
                    continue;
                };
                let rushing = props.0.get(&0x04A3_B2F8).and_then(raw_u32).filter(|a| *a != u32::MAX && *a != 0);
                self.clips.push(AbilityClip { ability_type, level, animation, rushing });
            }
        }
    }

    pub fn from_index(index: &crate::asset_catalog::ResourceIndex) -> Self {
        let mut out = Self::default();
        out.load_clips(index);
        for &(name, ability_type) in NATIVE_ABILITY_TEMPLATES {
            let source = crate::dbpf::ResourceKey {
                group_id: ABILITY_TEMPLATE_GROUP,
                instance_id: crate::game::spore_hash(name),
                type_id: crate::asset_catalog::PROP,
            };
            if !index.records.contains_key(&source) {
                out.errors.push(format!(
                    "{:08X}!{:08X}: missing native ability template {name}",
                    source.group_id, source.instance_id
                ));
                continue;
            }
            match index
                .properties(source)
                .and_then(|p| authored_ability(&p, source, name.into(), ability_type))
            {
                Ok(ability) => out.abilities.push(ability),
                Err(error) => out.errors.push(format!(
                    "{:08X}!{:08X} ({name}): {error}",
                    source.group_id, source.instance_id
                )),
            }
        }
        out
    }

    pub fn get(&self, mode: EncounterMode, slot: usize, rank: u8) -> Option<&AuthoredAbility> {
        if !(1..=5).contains(&rank) {
            return None;
        }
        let ability_type = match mode {
            EncounterMode::Social => *[37, 34, 36, 35].get(slot)?,
            EncounterMode::Combat => *[30, 31, 33, 32].get(slot)?,
        };
        self.abilities
            .iter()
            .find(|a| a.ability_type == ability_type)
    }

    pub fn audit(&self) -> serde_json::Value {
        serde_json::json!({
            "abilities": self.abilities.iter().map(|a| serde_json::json!({
                "source": crate::resource_audit::key_json(a.source), "name": a.name,
                "ability_type": a.ability_type, "damage": a.damage,
                "ranged_damage": a.ranged_damage, "range": a.range,
                "refresh": a.refresh, "recharge": a.recharge, "duration": a.duration,
                "level_multiplier": a.level_multiplier,
            })).collect::<Vec<_>>(),
            "errors": self.errors,
            "limitation": "Native template identity, type, range and refresh are read from installed PROP data. The native 5 s refresh value is used as an OpenSpore player lockout; its original trigger semantics are unverified. The 10 s NPC response window, damage scaling, world-unit mapping, hit timing, rank contests and species health remain adaptations. Base damage and level multiplier are absent in these templates."
        })
    }
}

fn authored_ability(
    props: &crate::prop::PropertyList,
    source: crate::dbpf::ResourceKey,
    name: String,
    expected_type: u32,
) -> Result<AuthoredAbility, String> {
    let number = |id| {
        props
            .0
            .get(&id)
            .and_then(|p| p.number())
            .filter(|n| n.is_finite() && *n >= 0.0)
    };
    let actual_type = number(0x01BA8C38).ok_or("ability type missing")?;
    if actual_type != expected_type as f32 {
        return Err(format!(
            "canonical template type {actual_type} != {expected_type}"
        ));
    }
    Ok(AuthoredAbility {
        source,
        name,
        ability_type: expected_type,
        damage: number(0x01B8346A),
        ranged_damage: number(0x0558E32A),
        refresh: number(0x01B8346E),
        recharge: number(0x05417D08),
        range: number(0x01B83470),
        duration: number(0x01C6C623),
        level_multiplier: number(0xF96C61B7),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatAbility {
    pub damage: f32,
    /// Distance at which a hit lands: the ability's range plus the retail hit
    /// buffer (`creatureAttackAbilityRangeHitBuffer`).
    pub range: f32,
    /// Seconds before the ability can be used again (retail refresh).
    pub recharge: f32,
    /// What a hit does besides damage (stun, poison).
    pub effect: crate::creature_rules::CombatEffect,
    pub kind: crate::creature_rules::CombatKind,
    pub level: u8,
}

/// The attack ability for tray slot `choice` at part rank `rank`, straight from
/// the retail per-level tables (`CreatureAbilities!bite1..5` etc., see
/// `creature_rules`): damage, refresh and range are no longer scaled or
/// invented here. The template catalog is kept for the audit only (the per
/// level props override its values). The charge's rush (20 units at speed 20
/// from at least 10-14 units away) is `Rush`; a pack member's charge is still a
/// melee hit at this range (ADAPTED: the NPCs have no approach behaviour).
pub fn combat_ability(_catalog: &CreatureAbilityCatalog, choice: usize, rank: u8) -> CombatAbility {
    use crate::creature_rules as rules;
    let kind = rules::CombatKind::from_choice(choice).unwrap_or(rules::CombatKind::Bite);
    match rules::combat_ability(kind, rank) {
        Some(a) => CombatAbility {
            damage: a.damage,
            range: a.reach(),
            recharge: a.refresh,
            effect: a.effect,
            kind,
            level: a.level,
        },
        None => CombatAbility {
            damage: 0.0,
            range: 0.0,
            recharge: 0.0,
            effect: rules::CombatEffect::None,
            kind,
            level: 0,
        },
    }
}

/// Damage of the player's hit through the retail pipeline (stealth x1.5 for a
/// bite or strike, no randomness).
fn player_hit_damage(ability: &CombatAbility, stealth_strike: bool) -> f32 {
    let Some(stats) = crate::creature_rules::combat_ability(ability.kind, ability.level) else {
        return 0.0;
    };
    crate::creature_rules::attack_damage(
        &stats,
        &crate::creature_rules::AttackContext {
            attacker_is_avatar: true,
            attacker_stealthed: stealth_strike,
            ..Default::default()
        },
    )
}

/// Apply a hit's combat effect to a creature (retail combat effects): a charge
/// stuns it and shoves it away from the attacker, a spit poisons it.
fn apply_hit_effect(
    mind: &mut CreatureMind,
    effect: crate::creature_rules::CombatEffect,
    damage: f32,
    from: Vec3,
    at: Vec3,
) {
    use crate::creature_rules as rules;
    match effect {
        rules::CombatEffect::None => {}
        rules::CombatEffect::Stun { seconds } => {
            mind.stunned = mind.stunned.max(seconds);
            let away = Vec3::new(at.x - from.x, 0.0, at.z - from.z).normalize_or_zero();
            let shove = rules::charge_knockback_impulse(damage) * rules::KNOCKBACK_DISTANCE_PER_IMPULSE;
            mind.shove = [away.x * shove, away.z * shove];
        }
        rules::CombatEffect::Poison {
            seconds,
            damage_per_second,
        } => {
            mind.poisoned = mind.poisoned.max(seconds);
            mind.poison_dps = mind.poison_dps.max(damage_per_second);
        }
    }
}

/// Put a creature's attack effect on the avatar (stun or poison).
pub(crate) fn hurt_avatar_with_effect(
    encounter: &mut Encounter,
    effect: crate::creature_rules::CombatEffect,
) {
    use crate::creature_rules as rules;
    match effect {
        rules::CombatEffect::None => {}
        rules::CombatEffect::Stun { .. } => {
            // Retail stuns the avatar for avatarStunTimeRange x avatarStunTimeMultiplier.
            encounter.stun = encounter.stun.max(rules::avatar_stun_seconds(0.5));
        }
        rules::CombatEffect::Poison {
            seconds,
            damage_per_second,
        } => {
            encounter.poison_seconds = encounter.poison_seconds.max(seconds);
            encounter.poison_dps = encounter.poison_dps.max(damage_per_second);
        }
    }
}
pub fn levels(a: Capabilities, mode: EncounterMode) -> [u8; 4] {
    match mode {
        EncounterMode::Social => [a.sing, a.dance, a.charm, a.pose],
        EncounterMode::Combat => [a.bite, a.charge, a.strike, a.spit],
    }
}

/// Choose an actual mounted pack attack whose native/adapted range reaches
/// this target. Ties are stable by tray order, avoiding random assistant DPS.
pub fn pack_attack(
    catalog: &CreatureAbilityCatalog,
    ranks: [u8; 4],
    distance: f32,
) -> Option<(usize, CombatAbility)> {
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let mut best: Option<(usize, CombatAbility)> = None;
    for (choice, rank) in ranks.into_iter().enumerate() {
        if rank == 0 {
            continue;
        }
        let ability = combat_ability(catalog, choice, rank);
        if distance > ability.range || ability.damage <= 0.0 {
            continue;
        }
        if best.is_none_or(|(_, current)| {
            ability.damage / ability.recharge > current.damage / current.recharge
        }) {
            best = Some((choice, ability));
        }
    }
    best
}

/// DNA for befriending or killing one creature comes from the retail formula
/// (`creature_rules::creature_dna_reward`, see `creature_reward`).
/// Extra DNA when a whole nest of a legacy (non-retail) species is allied or
/// destroyed. A retail herd pays the species-finished bonus of its archetype
/// (0xC2EE80, `creature_ecology::species_finished_dna`, see `nest_bonus`);
/// only species without an archetype record keep this flat amount.
pub const NEST_COMPLETE_DNA: f32 = 30.0;
/// ADAPTED: creatures only start attacks within this distance (retail
/// `creatureBasePerception`), so a spitter with a 31-unit range does not snipe
/// from across the island.
pub const NPC_ATTACK_REACH: f32 = crate::creature_rules::BASE_PERCEPTION;
/// Seconds a creature whose performance went unanswered refuses to watch again
/// (ADAPTED: retail only fails the step).
pub const BORED_SECONDS: f32 = 20.0;
/// Selecting a target (ADAPTED: retail picks targets by mouse), performing to it
/// (the social ability range, 5) and keeping it (`creatureUntargetDistance`, 17.5).
pub const TARGET_RANGE: f32 = 7.0;
pub const PERFORM_RANGE: f32 = crate::creature_rules::SOCIAL_RANGE;
pub const LEAVE_RANGE: f32 = crate::creature_rules::UNTARGET_DISTANCE;
/// A creature without weapons still struggles when attacked.
const STRUGGLE_DAMAGE_SCALE: f32 = 0.5;

type Actors<'w, 's> = Query<
    'w,
    's,
    (
        &'static WorldId,
        &'static Transform,
        &'static mut Npc,
        &'static mut CreatureMind,
        &'static mut Visibility,
    ),
    (Without<Player>, Without<RoamerActor>),
>;
type Roamers<'w, 's> = Query<
    'w,
    's,
    (
        &'static WorldId,
        &'static Transform,
        &'static mut RoamerActor,
        &'static mut Health,
        &'static mut CreatureMind,
        &'static mut Visibility,
    ),
    (Without<Player>, Without<Npc>),
>;

/// The player's merged part ranks (the headless fallback keeps one bite and
/// one song so encounters stay playable without an installation).
pub fn player_abilities(run: &RunProgress, originals: &crate::Originals) -> Capabilities {
    if originals.part_catalog.parts.is_empty() {
        Capabilities {
            bite: 1,
            sing: 1,
            ..default()
        }
    } else {
        originals
            .part_catalog
            .abilities_for_parts_in(crate::asset_catalog::CREATURE_PARTS, &run.parts)
    }
}

pub fn member_abilities(run: &RunProgress, originals: &crate::Originals, id: u32) -> Capabilities {
    if let Some(roamer) = crate::epic_creatures::abilities(run, originals, id) {
        return roamer;
    }
    if originals.part_catalog.parts.is_empty() {
        return Capabilities {
            bite: 1,
            sing: 1,
            ..default()
        };
    }
    run.species_world
        .species_for_member(id)
        .map(|species| crate::creature::species_abilities(species, originals))
        .unwrap_or_default()
}

/// Display name of a nest member's species or of a roamer.
pub fn member_name(run: &RunProgress, id: u32) -> String {
    if let Some(roamer) = run.species_world.roamer(id) {
        return roamer.name.clone();
    }
    let name = run
        .species_world
        .species_for_member(id)
        .map(|s| s.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "creature".into());
    // The retail card prefixes "Alpha ~local_creature_name~" / "Baby ...".
    format!("{}{name}", run.species_world.member_role(id).prefix())
}

/// The multiplier on every hit a nest member or roamer lands (C2D): its herd
/// archetype's `DamageMultiplier` (0.75 starter herds, 2 miniboss, 100 epic),
/// a quarter for a baby. Legacy species and worlds deal unscaled damage.
pub fn attacker_multiplier(run: &RunProgress, id: u32) -> f32 {
    run.species_world
        .archetype_for(id)
        .map(|archetype| {
            crate::creature_ecology::attack_multiplier(archetype, run.species_world.member_role(id))
        })
        .unwrap_or(1.0)
}

/// Combat ranks a creature fights back with: one without weapons still
/// struggles with a weak rank-1 bite. Returns ranks and a damage multiplier.
pub fn fight_back_ranks(ranks: [u8; 4]) -> ([u8; 4], f32) {
    if ranks.iter().all(|rank| *rank == 0) {
        ([1, 0, 0, 0], STRUGGLE_DAMAGE_SCALE)
    } else {
        (ranks, 1.0)
    }
}

pub fn match_response(mind: &mut CreatureMind, choice: usize, rank: u8) -> bool {
    ranked_match_response(mind, choice, rank, false)
}

/// Score one answer to the creature's current performance (retail 0xD9B070,
/// notes C3A): the answer adds the ability's power (its level 1..=5) over the
/// bar maximum to the friendship bar, doubled when a posse member helps and
/// times 0.4 when it is not the requested ability. A wrong answer never
/// subtracts; either answer lets the caller open the next round (a wrong one
/// only costs the round, 0xDA4CAA state 7 -> 4). Answers before the demonstration is over (more than 15 s left), with
/// no such ability, or to a finished creature are ignored. Returns `true` when
/// the bar fills and the creature befriends.
pub fn ranked_match_response(
    mind: &mut CreatureMind,
    choice: usize,
    rank: u8,
    posse_helping: bool,
) -> bool {
    if mind.allied || mind.defeated || mind.response_time <= 0. {
        return false;
    }
    if mind.response_time > crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT || rank == 0 {
        return false;
    }
    let correct = choice == mind.requested as usize;
    if mind.bar_max <= 0.0 {
        // Saved mid-performance before proposals planned their bar: the
        // fallback bar, as many rounds long as the creature asks for.
        mind.bar_max = crate::creature_rules::SOCIAL_BAR_MAX * mind.rounds.max(1) as f32;
        mind.player_fill = mind.impression / 100.0 * mind.bar_max;
        mind.npc_fill = 0.0;
    }
    // 0xD9B070: the answer adds its power to the player's side of the bar; an
    // answer stops the creature's side draining (event type 1 clears the bar
    // record's drain flags).
    mind.player_fill += crate::creature_rules::social_answer_power(rank, correct, posse_helping);
    mind.drain_armed = true;
    mind.draining = false;
    mind.drain_timer = 0.0;
    update_impression(mind);
    // Every answer closes the round (event type 2; `beats` counts the answered
    // rounds): right or wrong, the caller opens the next round (0xDA4CAA:
    // rounds += 1, state 4; a wrong answer first plays 0x068D8F9C, state 7,
    // which returns to state 4 when the clip ends, notes C10A).
    mind.beats = mind.beats.saturating_add(1);
    let _ = correct;
    if mind.npc_fill + mind.player_fill >= mind.bar_max - 1e-3 {
        mind.impression = 100.;
        mind.allied = true;
        mind.performing = false;
        mind.response_time = 0.;
        true
    } else {
        false
    }
}

/// The clip of a tray ability at `rank` from the installed props, else the
/// same ids by name (`creature_clips::ability_action`).
pub fn ability_clip(catalog: &CreatureAbilityCatalog, mode: EncounterMode, slot: usize, rank: u8) -> Option<u32> {
    catalog
        .slot_clip(mode, slot, rank)
        .map(|c| c.animation)
        .or_else(|| crate::creature_clips::ability_action(mode == EncounterMode::Social, slot, rank.max(1)))
}

/// The run-up clip played before an ability's own clip: charge's
/// `creatureAbilityRushingAnimationID` (0x04A3B4FB), none for the others.
pub fn ability_lead_clip(catalog: &CreatureAbilityCatalog, mode: EncounterMode, slot: usize, rank: u8) -> Option<u32> {
    catalog.slot_clip(mode, slot, rank).and_then(|c| c.rushing)
}

/// The run-up clip of an ability clip id (the clip a creature recorded with
/// `note_attack`), if that clip belongs to an ability with one.
pub fn lead_clip_of(catalog: &CreatureAbilityCatalog, animation: u32) -> Option<u32> {
    catalog.clips.iter().find(|c| c.animation == animation).and_then(|c| c.rushing)
}

/// Record a creature's attack for the clip player: its ability's clip and a
/// new attack serial.
pub fn note_attack(mind: &mut CreatureMind, catalog: &CreatureAbilityCatalog, slot: usize, rank: u8) {
    if let Some(clip) = ability_clip(catalog, EncounterMode::Combat, slot, rank) {
        mind.attack_clip = clip;
        mind.attack_serial = mind.attack_serial.wrapping_add(1);
    }
}

/// The friendship shown on the HUD: both sides of the bar over its maximum.
fn update_impression(mind: &mut CreatureMind) {
    if mind.bar_max > 0.0 {
        mind.impression = (100.0 * (mind.npc_fill + mind.player_fill) / mind.bar_max).clamp(0.0, 100.0);
    }
}

/// The abilities a creature asks for in a proposal of `rounds` rounds: one of
/// its social abilities per round, drawn with repetition (0xDA4360 draws with
/// `RandomInt(count)` each round). ADAPTED: a hash of the creature and the
/// round stands in for the game's random generator.
pub fn plan_requests(seed: u64, ranks: [u8; 4], rounds: u8) -> Vec<u8> {
    let available: Vec<u8> = (0..4u8).filter(|i| ranks[*i as usize] > 0).collect();
    if available.is_empty() {
        return Vec::new();
    }
    (0..rounds.max(1) as u64)
        .map(|r| available[(crate::creature_ecology::mix64(seed ^ (r << 32) ^ 0x50C1_A1) % available.len() as u64) as usize])
        .collect()
}

/// The creature proposes (retail SocialPropose 0xDA4360): a fresh bar of
/// `rounds` rounds (the requests are kept while their count is unchanged),
/// then the first demonstration.
pub fn propose(mind: &mut CreatureMind, ranks: [u8; 4]) {
    let rounds = mind.rounds.max(1);
    let keep = mind.requests.len() == rounds as usize
        && mind.requests.iter().all(|r| ranks.get(*r as usize).is_some_and(|rank| *rank > 0));
    if !keep {
        let seed = (mind.species as u64) << 40 ^ ((rounds as u64) << 32) ^ ranks.iter().fold(0u64, |a, r| a << 8 | *r as u64);
        mind.requests = plan_requests(seed, ranks, rounds);
    }
    if mind.requests.is_empty() {
        mind.response_time = 0.;
        mind.performing = false;
        return;
    }
    // A group proposal (0xDA4950 state 3): the bar is `(n + 1)` times as long
    // for the `n` herd mates that joined (`creature_group`).
    mind.bar_max = crate::creature_rules::social_bar_max(mind.requests.iter().map(|r| ranks[*r as usize]))
        * (mind.group as f32 + 1.0);
    mind.npc_fill = 0.0;
    mind.player_fill = 0.0;
    mind.impression = 0.0;
    mind.round = 0;
    mind.beats = 0;
    demonstrate(mind, ranks);
}

/// The creature demonstrates the ability of its current round: its level
/// joins its side of the bar (0xDA4BBE -> 0xD9AFF0: `(n + 1) x mDamage`, n =
/// the herd mates that joined, `group`) and the answer window opens after it.
/// Its joiners play the same clip (`demo_serial`, `creature_group`).
fn demonstrate(mind: &mut CreatureMind, ranks: [u8; 4]) {
    let slot = mind.requests[(mind.round as usize).min(mind.requests.len() - 1)];
    mind.requested = slot;
    mind.requested_level = ranks[slot as usize].max(1);
    mind.npc_fill += (mind.group as f32 + 1.0) * crate::creature_rules::social_power(mind.requested_level);
    mind.demo_serial = mind.demo_serial.wrapping_add(1);
    mind.response_time = SOCIAL_RESPONSE_WINDOW;
    mind.performing = true;
    mind.drain_armed = false;
    mind.draining = false;
    mind.drain_timer = 0.0;
    update_impression(mind);
}

/// Ask the creature's clip player for a one-shot clip (a social reaction):
/// `cue_clip` with a new `cue_serial` (`locomotion_preview::npc_request`).
pub fn play_cue(mind: &mut CreatureMind, clip: u32) {
    mind.cue_clip = clip;
    mind.cue_serial = mind.cue_serial.wrapping_add(1);
}

/// The species' creature-game relationship to the player
/// (`creature_rules::species_relationship`) from the species world: the
/// archetype's `initialAvatarRelationship` and thresholds, its befriended and
/// killed members. Roamers use their archetype's initial value; legacy
/// species without an archetype read as curious.
pub fn avatar_relationship(run: &RunProgress, id: u32, species: u32, roamer: bool) -> u8 {
    let world = &run.species_world;
    if roamer {
        return world
            .archetype_for(id)
            .map_or(crate::creature_rules::relationship::CURIOUS, |a| a.record().initial_relationship);
    }
    species_relationship_of(world, species).unwrap_or(crate::creature_rules::relationship::CURIOUS)
}

/// The relationship of a retail herd's species to the avatar from the species
/// world (`None` for a legacy species without an archetype).
pub fn species_relationship_of(world: &crate::species::SpeciesWorld, species: u32) -> Option<u8> {
    let r = world.archetype_of(species)?.record();
    // The relationship manager's record once the species has had an event
    // (befriend, kill, attack: `species::SpeciesRelation`, 0xBA4A00).
    if let Some(record) = world.relation(species) {
        return Some(record.relationship);
    }
    let counts = world.species_counts(species);
    Some(crate::creature_rules::species_relationship(
        r.initial_relationship,
        counts.allied,
        counts.defeated,
        [r.impress_curious, r.impress_friendly, r.impress_best_friends, r.kills_for_fearful],
    ))
}

/// The next round after a correct answer (0xDA4BBE): the next demonstration,
/// or, with every round used and the bar not full, the end of the proposal
/// (the creature plays 0x53685D8F, state 8). Returns `false` then.
pub fn next_round(mind: &mut CreatureMind, ranks: [u8; 4]) -> bool {
    if mind.requests.is_empty() || mind.bar_max <= 0.0 {
        // A creature saved before proposals were planned: start one.
        propose(mind, ranks);
        return mind.performing;
    }
    mind.round = mind.round.saturating_add(1);
    if mind.round as usize >= mind.requests.len() {
        mind.performing = false;
        mind.response_time = 0.0;
        mind.beats = 0;
        mind.draining = false;
        // 0xDA4950: rounds used without a full bar -> soc_annoyed_reject
        // (broadcast to the group, `creature_group`).
        play_cue(mind, crate::creature_rules::SOCIAL_CLIP_ROUNDS_OVER);
        return false;
    }
    demonstrate(mind, ranks);
    true
}

/// A group proposal starts (0xDA4360 with `SocialGroupSize` > 1): the creature
/// will call up to `wanted` herd mates (`creature_group` picks them from its
/// nearby herd and starts the proposal when the gathering ends). With
/// `wanted` 0 it proposes at once, alone.
pub fn begin_gather(mind: &mut CreatureMind, ranks: [u8; 4], wanted: usize) {
    mind.group = 0;
    mind.joiners.clear();
    if wanted == 0 {
        mind.group_wanted = 0;
        mind.gather = 0.0;
        propose(mind, ranks);
        return;
    }
    mind.group_wanted = wanted.min(u8::MAX as usize) as u8;
    mind.gather = crate::creature_rules::SOCIAL_GROUP_WAIT_SECONDS;
    mind.performing = false;
    mind.response_time = 0.0;
}

/// Herd mates a creature calls when it proposes (`SocialGroupSize - 1` of its
/// herd archetype; legacy species and roamers propose alone).
pub fn group_wanted(run: &RunProgress, id: u32) -> usize {
    run.species_world
        .archetype_for(id)
        .map_or(0, |a| crate::creature_rules::social_group_joiners(a.record().social_group_size))
}

/// The creature performs: a new proposal when it is not performing, else its
/// next round (see `propose`, `next_round`).
pub fn request(mind: &mut CreatureMind, ranks: [u8; 4]) {
    if mind.performing {
        next_round(mind, ranks);
    } else {
        propose(mind, ranks);
    }
}

/// One frame of a creature's social round (0xDA4CAA / 0xD9B9C0): once its
/// demonstration is over and no answer came, it waits `before_drain` seconds,
/// then its side of the bar drains at `drain_rate` bar spans per second.
pub fn social_tick(mind: &mut CreatureMind, dt: f32, before_drain: f32, drain_rate: f32) {
    if !mind.performing || mind.allied || mind.bar_max <= 0.0 {
        mind.draining = false;
        return;
    }
    if mind.response_time > crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT {
        return;
    }
    if !mind.drain_armed {
        mind.drain_armed = true;
        mind.drain_timer = before_drain.max(0.0);
        return;
    }
    if mind.draining {
        mind.npc_fill = crate::creature_rules::social_drain(mind.npc_fill, mind.bar_max, drain_rate, dt);
        update_impression(mind);
    } else if mind.drain_timer > 0.0 {
        mind.drain_timer -= dt;
    } else if mind.player_fill + mind.npc_fill > 0.0 && mind.beats == mind.round {
        // Armed in this round with no answer yet (an answer leaves `beats`
        // ahead of the round until the next demonstration).
        mind.draining = true;
    }
}

/// The drain tuning of a creature: its herd archetype's, else the archetype
/// default (`socialTimeBeforeDrain` 4 s, `socialDrainRate` 0.1).
pub fn drain_tuning(archetype: Option<crate::creature_ecology::Archetype>) -> (f32, f32) {
    let record = archetype
        .map(|a| a.record())
        .unwrap_or(&crate::creature_ecology::DEFAULT_ARCHETYPE);
    (record.social_time_before_drain, record.social_drain_rate)
}

/// The current performance went unanswered: the step fails (retail 0xDA4CAA)
/// and the creature loses interest for a while. The next proposal starts a
/// fresh bar (0xDA4360 resets it). Returns `true` (the creature walks away).
pub fn ignore_performance(mind: &mut CreatureMind) -> bool {
    mind.performing = false;
    mind.response_time = 0.0;
    mind.beats = 0;
    mind.draining = false;
    mind.bored = BORED_SECONDS;
    true
}

/// Native social refresh values gate how often the player can perform. If a
/// catalog is unavailable, retain the same 5 s default (retail refresh 5 s).
/// The NPC prompt lasts longer (see `SOCIAL_RESPONSE_WINDOW`).
pub fn social_refresh(catalog: &CreatureAbilityCatalog, choice: usize, rank: u8) -> f32 {
    catalog
        .get(EncounterMode::Social, choice, rank)
        .and_then(|a| a.recharge.or(a.refresh))
        .filter(|r| r.is_finite() && *r > 0.0)
        .unwrap_or(SOCIAL_REFRESH_FALLBACK)
}

/// Award Creature DNA. A reward earned in the same action that evolved the
/// lineage (a nest bonus after the last kill) never leaks into Tribal.
///
/// Reaching the stage total (brain level 4) no longer ends the stage by
/// itself: retail shows the Advance ("Evolve") button and waits for the
/// player (`creature_flow` plays `CRG2TRG`, the timeline, then Tribe).
fn award(run: &mut RunProgress, _next: &mut NextState<GameStage>, dna: f32, social: bool) {
    let creature = RunProgress::stage_order()
        .iter()
        .position(|stage| *stage == GameStage::Creature)
        .unwrap_or(1);
    if run.stage_order_index > creature {
        return;
    }
    run.award_dna(dna, &GameStage::Creature);
    // The four retail reward callers also add to the player's social / attack
    // balance (0xC75840, flag 1 = social, 0 = attack): the timeline's trait.
    if dna.is_finite() && dna > 0.0 {
        let progression = &mut run.creature_progression;
        if social {
            progression.social_dna += dna;
        } else {
            progression.attack_dna += dna;
        }
    }
    run.request_save();
}

/// Record a member outcome in the species world exactly once. Also reports
/// whether that action crossed the species threshold (allied or extinct).
fn record_outcome(run: &mut RunProgress, id: u32, allied: bool) -> (RecordResult, bool) {
    // Resolve the member's species first: the recorded id and name are all the
    // threshold checks need, so no borrow outlives the record calls.
    let (species, name) = match run.species_world.species_for_member(id) {
        Some(s) => (s.id, s.name.clone()),
        None => return (RecordResult::UnknownMember, false),
    };
    let crossed_before = if allied {
        run.species_world.species_allied(species)
    } else {
        run.species_world.species_extinct(species)
    };
    let result = if allied {
        run.species_world.record_befriend(id)
    } else {
        run.species_world.record_defeat(id)
    };
    if result != RecordResult::Recorded {
        return (result, false);
    }
    let crossed_after = if allied {
        run.species_world.species_allied(species)
    } else {
        run.species_world.species_extinct(species)
    };
    let crossed = !crossed_before && crossed_after;
    if crossed {
        info!(
            "species '{}': {}",
            name,
            if allied {
                "allied to the player"
            } else {
                "extinct"
            }
        );
    }
    (result, crossed)
}

/// Pure: the message shown when a recording crosses a species threshold.
/// Keeps the existing individual award text intact for the non-threshold case.
pub fn species_announcement(
    world: &crate::species::SpeciesWorld,
    id: u32,
    allied: bool,
) -> Option<String> {
    let species = world.species_for_member(id)?;
    if allied {
        if world.species_allied(species.id) {
            let counts = world.species_counts(species.id);
            return Some(format!(
                "The {} are allied: {} of {} befriend",
                species.name,
                counts.allied,
                counts.total()
            ));
        }
    } else if world.species_extinct(species.id) {
        return Some(format!("The {} are extinct", species.name));
    }
    None
}

/// Original part definitions a species carries: the displayed model's
/// native anatomy when it resolved, otherwise the generated build's parts.
pub fn carried_parts(run: &RunProgress, originals: &crate::Originals, species: u32) -> Vec<u32> {
    let Some(species) = run.species_world.species(species) else {
        return Vec::new();
    };
    if let Some(native) = crate::creature::original_build(species, originals) {
        return native
            .parts
            .iter()
            .map(|p| p.definition.instance_id)
            .collect();
    }
    species
        .parts
        .iter()
        .filter_map(|p| originals.part_catalog.by_model(p.instance))
        .map(|d| d.key.instance_id)
        .collect()
}

/// Everything an encounter outcome may change.
struct Ctx<'a> {
    run: &'a mut RunProgress,
    next: &'a mut NextState<GameStage>,
    encounter: &'a mut Encounter,
    awards: &'a mut CreatureAwards,
    originals: &'a crate::Originals,
    player: Capabilities,
    /// Meat to drop at kill sites once the queries are released.
    drops: Vec<(Vec3, f32)>,
    /// This frame's attack was made from stealth (damage x1.5 for bite/strike).
    stealth_strike: bool,
    /// Where the player stands (knockback pushes away from it).
    player_position: Vec3,
    /// Retail herd members hit this frame (id, position): their herd answers
    /// (creature_support::herd_helpers, 0xC075E0).
    alerts: Vec<(u32, Vec3)>,
}

/// One engaged creature, nest member or roamer, as mutable encounter state.
struct TargetView<'a> {
    id: u32,
    name: String,
    /// Distance from the player to the creature's body edge.
    distance: f32,
    position: Vec3,
    hp: &'a mut f32,
    /// `Npc::hostile` for nest members; `RoamerActor::aggressive` for roamers.
    hostile: &'a mut bool,
    mind: &'a mut CreatureMind,
    visibility: &'a mut Visibility,
    social: [u8; 4],
    roamer: Option<RoamerStats>,
}

/// The target's strength rating for the DNA multiplier (retail
/// `mStrengthRating`; ADAPTED, see `creature_rules::npc_strength_rating`).
/// The strength rating behind the DNA multiplier. VERIFIED (Create 0xC09B40):
/// a creature of a herd archetype takes the archetype's `FoodChainLevel`
/// (+0x330) as `mStrengthRating`; others a rating from their parts.
fn target_strength(ctx: &Ctx, t: &TargetView) -> i32 {
    if let Some(archetype) = ctx.run.species_world.archetype_for(t.id) {
        return archetype.record().food_chain as i32;
    }
    match t.roamer {
        Some(stats) => stats.strength,
        None => crate::creature_rules::npc_strength_rating(member_abilities(ctx.run, ctx.originals, t.id)),
    }
}

/// DNA for killing (`befriend` false) or befriending one creature (retail
/// 0xC042A0 / 0xD9BBD0). A retail herd member or roamer pays its archetype's
/// `evoPointsRewarded` / `evoPointsRewardedSocial` (+ the social bonus at a
/// full bar), x0.25 for a baby, x1.25 for an alpha, times the level
/// multiplier. ADAPTED: legacy species use the fallback base 100 and legacy
/// roamers their own base.
fn creature_reward(ctx: &Ctx, t: &TargetView, befriend: bool) -> f32 {
    use crate::creature_rules as rules;
    let strength = target_strength(ctx, t);
    let level = ctx.run.creature_progression.brain_level;
    let difficulty = rules::Difficulty::from_index(ctx.run.cell.difficulty);
    if let Some(archetype) = ctx.run.species_world.archetype_for(t.id) {
        let role = ctx.run.species_world.member_role(t.id);
        return if befriend {
            // 0xD9BBD0: the bonus scales with the player's share of the bar
            // (0xE319F0(1), the answers' side) above one half.
            let score = if t.mind.bar_max > 0.0 { t.mind.player_fill / t.mind.bar_max } else { 1.0 };
            crate::creature_ecology::befriend_dna(archetype, role, level, strength, difficulty, score)
        } else {
            crate::creature_ecology::kill_dna(archetype, role, level, strength, difficulty)
        };
    }
    let base = match t.roamer {
        Some(stats) => stats.dna,
        None => rules::DEFAULT_SPECIES_DNA,
    };
    rules::creature_dna_reward(base, false, false, level, strength, difficulty)
}

/// The species-finished DNA of a retail herd (0xC2EE80: `extinctionReward` or
/// `relationshipBestFriendsReward` times the level multiplier), or the legacy
/// flat nest bonus.
fn nest_bonus(ctx: &Ctx, member: u32, species: u32, allied: bool) -> f32 {
    let Some(archetype) = ctx.run.species_world.archetype_of(species) else {
        return NEST_COMPLETE_DNA;
    };
    // VERIFIED (0xC30320 copies the archetype's FoodChainLevel to the species
    // goal, 0xC2EE80 indexes the multiplier with it).
    let _ = member;
    let strength = archetype.record().food_chain as i32;
    crate::creature_ecology::species_finished_dna(
        archetype,
        allied,
        ctx.run.creature_progression.brain_level,
        strength,
        crate::creature_rules::Difficulty::from_index(ctx.run.cell.difficulty),
    )
}

/// Parts drop from alphas only (`creatureAlphaUnlockChance` 1.0,
/// `creatureNonAlphaUnlockChance` 0): killing or befriending a retail alpha
/// unlocks one of the parts its species carries. Returns the part's name.
fn alpha_part(ctx: &mut Ctx, member: u32) -> Option<String> {
    let world = &ctx.run.species_world;
    let species = world.nest_for_member(member)?.species;
    world.archetype_of(species)?;
    if !world.member_role(member).is_alpha() {
        return None;
    }
    let carried = carried_parts(ctx.run, ctx.originals, species);
    let part = crate::creature_parts::take_species_part(ctx.run, &ctx.originals.part_catalog.parts, carried)?;
    ctx.awards.push(format!("New part unlocked: {part}"));
    Some(part)
}

/// Pay a completed nest's one-time reward: bonus DNA (a retail herd's
/// species-finished reward) and, for a legacy species, one of the parts it
/// carries (retail parts come from alphas, `alpha_part`). Returns the line
/// appended to the encounter message.
fn complete_nest(ctx: &mut Ctx, member: u32) -> Option<String> {
    let index = ctx.run.species_world.nest_index_for_member(member)?;
    let status = ctx.run.species_world.claim_nest(index)?;
    let species = ctx.run.species_world.nests[index].species;
    let retail = ctx.run.species_world.archetype_of(species).is_some();
    let name = if retail {
        species_name(ctx.run, member)
    } else {
        member_name(ctx.run, member)
    };
    let bonus = nest_bonus(ctx, member, species, status == NestStatus::Allied);
    // Best friends pay the social side (0xC2EFC9), extinction the attack side (0xC2F0D2).
    award(ctx.run, ctx.next, bonus, status == NestStatus::Allied);
    if retail {
        // One herd is one species: its species banner (Befriended a species /
        // Extinction) already announced it; the message carries the reward.
        return Some(match status {
            NestStatus::Allied => format!("The {name} are your best friends: +{bonus:.0} DNA, heal at their nest"),
            NestStatus::Destroyed => format!("The {name} are extinct: +{bonus:.0} DNA"),
            NestStatus::Wild => return None,
        });
    }
    let carried = carried_parts(ctx.run, ctx.originals, species);
    let part =
        crate::creature_parts::take_species_part(ctx.run, &ctx.originals.part_catalog.parts, carried);
    let mut line = match status {
        NestStatus::Allied => {
            ctx.awards
                .push(format!("Nest allied: the {name} welcome you"));
            format!("The {name} nest is allied: +{bonus:.0} DNA, heal there")
        }
        NestStatus::Destroyed => {
            ctx.awards.push(format!("Nest destroyed: the {name}"));
            format!("The {name} nest is destroyed: +{bonus:.0} DNA")
        }
        NestStatus::Wild => return None,
    };
    if let Some(part) = part {
        ctx.awards.push(format!("New part unlocked: {part}"));
        line.push_str(&format!(
            " | new part: {part} (mate at your nest to use it)"
        ));
    }
    if status == NestStatus::Allied && ctx.run.species_world.species_fully_allied(species) {
        ctx.awards.push(format!("Every {name} nest is your ally"));
    }
    Some(line)
}

/// A nest member or roamer was killed by the player or the pack.
fn finish_kill(ctx: &mut Ctx, t: &mut TargetView) {
    if t.mind.defeated {
        return;
    }
    t.mind.defeated = true;
    t.mind.response_time = 0.0;
    *t.visibility = Visibility::Hidden;
    ctx.encounter.target = None;
    if t.roamer.is_some() {
        let dna = creature_reward(ctx, t, false) + roamer_finished_bonus(ctx, t, false);
        let recorded = ctx
            .run
            .species_world
            .record_roamer(t.id, Relationship::Defeated)
            == RecordResult::Recorded;
        ctx.encounter.message = if recorded {
            award(ctx.run, ctx.next, dna, false);
            ctx.drops
                .push((t.position, crate::creature_social::MEAT_FOOD * 3.0));
            ctx.awards.push(format!(
                "You slew the {}! +{:.0} DNA",
                t.name.to_lowercase(),
                dna
            ));
            let carried = crate::epic_creatures::carried_parts(ctx.run, ctx.originals, t.id);
            match crate::creature_parts::take_species_part(
                ctx.run,
                &ctx.originals.part_catalog.parts,
                carried,
            ) {
                Some(part) => {
                    ctx.awards.push(format!("New part unlocked: {part}"));
                    format!(
                        "Slew the {}: {:.0} DNA | new part: {part}",
                        t.name.to_lowercase(),
                        dna
                    )
                }
                None => format!("Slew the {}: {:.0} DNA", t.name.to_lowercase(), dna),
            }
        } else {
            "Defeated creature (outcome already recorded)".into()
        };
        ctx.run.request_save();
        return;
    }
    let dna = creature_reward(ctx, t, false);
    let (outcome, extinct) = record_outcome(ctx.run, t.id, false);
    if outcome == RecordResult::Recorded {
        award(ctx.run, ctx.next, dna, false);
        ctx.drops
            .push((t.position, crate::creature_social::MEAT_FOOD));
    }
    let mut message = species_announcement(&ctx.run.species_world, t.id, false).unwrap_or_else(
        || match outcome {
            RecordResult::Recorded => format!("Defeated creature: {dna:.0} DNA"),
            RecordResult::Already => "Defeated creature (outcome already recorded)".into(),
            RecordResult::UnknownMember => "Defeated creature (unknown member; no DNA)".into(),
        },
    );
    if extinct {
        ctx.awards
            .push(format!("Extinction! The {} are gone forever", species_name(ctx.run, t.id)));
    }
    if outcome == RecordResult::Recorded {
        if let Some(part) = alpha_part(ctx, t.id) {
            message.push_str(&format!(" | new part: {part} (mate at your nest to use it)"));
        }
        if let Some(line) = complete_nest(ctx, t.id) {
            message.push_str(" | ");
            message.push_str(&line);
        }
    }
    ctx.encounter.message = message;
    ctx.run.request_save();
}

/// The species name of a nest member (without the alpha / baby prefix).
fn species_name(run: &RunProgress, id: u32) -> String {
    run.species_world
        .species_for_member(id)
        .map(|s| s.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| member_name(run, id))
}

/// A retail roamer's species-finished reward (0xC2EE80): the miniboss is
/// extinct at its first kill and best friends at its first friendship (100
/// each); the epic predator is never extinct and pays its best-friends reward
/// (20) when befriended. Legacy roamers pay nothing extra.
fn roamer_finished_bonus(ctx: &Ctx, t: &TargetView, allied: bool) -> f32 {
    let Some(archetype) = ctx.run.species_world.archetype_for(t.id) else {
        return 0.0;
    };
    if !allied && crate::creature_ecology::kills_to_extinction(archetype).is_none() {
        return 0.0;
    }
    crate::creature_ecology::species_finished_dna(
        archetype,
        allied,
        ctx.run.creature_progression.brain_level,
        target_strength(ctx, t),
        crate::creature_rules::Difficulty::from_index(ctx.run.cell.difficulty),
    )
}

/// A full friendship bar: record the friendship once and pay its rewards.
fn befriend(ctx: &mut Ctx, t: &mut TargetView) {
    // 0xD9BBD0: the reaction clip by the relationship before the event; the
    // proposer's group plays it too (`creature_group`).
    let before = avatar_relationship(ctx.run, t.id, t.mind.species as u32, t.roamer.is_some());
    play_cue(t.mind, crate::creature_rules::befriend_reaction_clip(before));
    *t.hostile = false;
    let dna = creature_reward(ctx, t, true);
    if t.roamer.is_some() {
        let dna = dna + roamer_finished_bonus(ctx, t, true);
        let recorded = ctx
            .run
            .species_world
            .record_roamer(t.id, Relationship::Allied)
            == RecordResult::Recorded;
        if recorded {
            award(ctx.run, ctx.next, dna, true);
            ctx.awards.push(format!(
                "Befriended the {}! +{:.0} DNA",
                t.name.to_lowercase(),
                dna
            ));
        }
        ctx.encounter.message = format!("The {} is your friend now", t.name.to_lowercase());
        ctx.run.request_save();
        return;
    }
    let (outcome, crossed) = record_outcome(ctx.run, t.id, true);
    let recorded = outcome == RecordResult::Recorded;
    if recorded {
        award(ctx.run, ctx.next, dna, true);
    }
    let species = t.mind.species as u32;
    let counts = ctx.run.species_world.species_counts(species);
    let mut message =
        species_announcement(&ctx.run.species_world, t.id, true).unwrap_or_else(|| {
            format!(
                "{} | species alliance {}/{}",
                if recorded {
                    format!("Befriended! {dna:.0} DNA")
                } else {
                    "Already befriended; no new DNA".to_string()
                },
                counts.allied,
                ctx.run.species_world.alliance_required(species)
            )
        });
    if crossed {
        ctx.awards
            .push(format!("Befriended a species: the {}", species_name(ctx.run, t.id)));
    }
    if recorded {
        if let Some(part) = alpha_part(ctx, t.id) {
            message.push_str(&format!(" | new part: {part} (mate at your nest to use it)"));
        }
        if let Some(line) = complete_nest(ctx, t.id) {
            message.push_str(" | ");
            message.push_str(&line);
        }
    }
    ctx.encounter.message = message;
    ctx.run.request_save();
}

/// Targeting a creature (F, a double click, a stance change): start its performance (social) or
/// ready the attack (combat) and explain what happens next.
fn engage(ctx: &mut Ctx, t: &mut TargetView) {
    if ctx.encounter.mode == EncounterMode::Combat {
        ctx.encounter.message = "Choose an ability with 1-4".into();
        return;
    }
    let best_social = levels(ctx.player, EncounterMode::Social)
        .into_iter()
        .max()
        .unwrap_or(0);
    ctx.encounter.message = if t.mind.allied {
        if t.roamer.is_some() {
            format!("The {} is your friend", t.name.to_lowercase())
        } else {
            "Ally: P recruits or dismisses a pack member".into()
        }
    } else if *t.hostile && t.roamer.is_none() {
        "This creature is hostile".into()
    } else if t.mind.bored > 0.0 {
        format!(
            "The {} is not interested right now ({:.0}s)",
            t.name, t.mind.bored
        )
    } else if let Some(stats) = t
        .roamer
        .filter(|stats| best_social < stats.required_social_rank)
    {
        format!(
            "Only rank {} performances impress the {}; fight it with a pack instead",
            stats.required_social_rank,
            t.name.to_lowercase()
        )
    } else {
        if t.mind.gather > 0.0 {
            // Still calling its herd (group social, `creature_group`).
        } else if !t.mind.performing && t.roamer.is_none() && t.social.iter().any(|rank| *rank > 0) {
            // A new proposal: alone, or with herd mates (0xDA4360).
            let wanted = group_wanted(ctx.run, t.id);
            begin_gather(t.mind, t.social, wanted);
        } else if !(t.mind.performing && t.mind.response_time > 0.0) {
            request(t.mind, t.social);
        }
        if t.mind.gather > 0.0 {
            format!("The {} calls its herd to perform with it", t.name)
        } else if t.mind.response_time <= 0.0 {
            "This creature has no social performance parts".into()
        } else {
            format!(
                "The {} performs {}: answer with the same action (1-4)",
                t.name,
                SOCIAL[t.mind.requested.min(3) as usize]
            )
        }
    };
}

/// One frame of the player's encounter with the current target.
fn drive_target(
    ctx: &mut Ctx,
    mut t: TargetView,
    selected: Option<usize>,
    recruit: bool,
    support: &[(u32, [u8; 4])],
    performing: &mut Vec<(u32, f32)>,
) {
    // A charge in flight is the avatar's whole action: its buttons wait.
    if drive_rush(ctx, &mut t) {
        return;
    }
    if ctx.encounter.mode == EncounterMode::Social {
        if t.mind.allied {
            if selected.is_some() {
                // 0xD1E720 returns 6 for a target that is already a friend.
                // ADAPTED: the check is vtable +0xD4 of the target, read as
                // "is already a friend".
                ctx.encounter.error = Some(AbilityError::SocialAlreadyFriends);
                ctx.encounter.error_serial = ctx.encounter.error_serial.wrapping_add(1);
            }
            if !recruit {
                ctx.encounter.message = if t.roamer.is_some() {
                    format!("The {} is your friend", t.name.to_lowercase())
                } else {
                    "Ally: P recruits or dismisses a pack member".into()
                };
            }
            return;
        }
        if (*t.hostile && t.roamer.is_none()) || t.mind.bored > 0.0 {
            return;
        }
        let best_social = levels(ctx.player, EncounterMode::Social)
            .into_iter()
            .max()
            .unwrap_or(0);
        if t.roamer
            .is_some_and(|stats| best_social < stats.required_social_rank)
        {
            return;
        }
        if t.mind.gather > 0.0 {
            // Calling its herd mates: nothing to answer yet.
            if selected.is_some() {
                ctx.encounter.message = format!("The {} is still calling its herd", t.name);
            }
            return;
        }
        if t.mind.performing && t.mind.response_time <= 0. {
            // The performance went unanswered: the step fails.
            ignore_performance(t.mind);
            ctx.encounter.target = None;
            ctx.encounter.message = format!("The {} lost interest and wandered off", t.name);
            ctx.run.request_save();
            return;
        }
        let Some(choice) = selected else {
            return;
        };
        let rank = levels(ctx.player, EncounterMode::Social)[choice];
        if !t.mind.performing {
            ctx.encounter.message = if t.social.iter().any(|rank| *rank > 0) {
                format!(
                    "The {} is not performing: press E to ask for a performance",
                    t.name
                )
            } else {
                "This creature has no social performance parts".into()
            };
        } else if t.mind.response_time > crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT {
            ctx.encounter.message = format!(
                "The {} is still showing {}: wait for it to finish",
                t.name,
                SOCIAL[t.mind.requested.min(3) as usize]
            );
        } else if t.distance > PERFORM_RANGE {
            ctx.encounter.message = "Move closer to perform".into();
        } else if ctx.encounter.cooldowns[choice] > 0. {
            ctx.encounter.fail(AbilityError::NotReady);
        } else if rank == 0 {
            ctx.encounter.message = format!("Mount a part with {}", SOCIAL[choice]);
        } else if let Some(stats) = t.roamer.filter(|stats| rank < stats.required_social_rank) {
            ctx.encounter.message = format!(
                "Your {} (rank {rank}) cannot impress the {}: rank {} needed",
                SOCIAL[choice],
                t.name.to_lowercase(),
                stats.required_social_rank
            );
        } else {
            let requested = t.mind.requested.min(3) as usize;
            let correct = choice == requested;
            // A pack member that can perform this ability helps (retail
            // doubles the answer when a posse member performs too).
            let posse_helping = support.iter().any(|(_, ranks)| ranks[choice] > 0);
            let allied = ranked_match_response(t.mind, choice, rank, posse_helping);
            let recharge = social_refresh(&ctx.originals.creature_abilities, choice, rank);
            ctx.encounter.cooldowns[choice] = recharge;
            performing.extend(
                support
                    .iter()
                    .filter(|(_, ranks)| ranks[choice] > 0)
                    .map(|(id, _)| (*id, recharge)),
            );
            if allied {
                befriend(ctx, &mut t);
            } else if correct {
                if t.roamer.is_some() {
                    // A matched performance calms an angry giant.
                    *t.hostile = false;
                }
                ctx.encounter.message = if next_round(t.mind, t.social) {
                    format!("Matched their {}! Friendship {:.0}%", SOCIAL[requested], t.mind.impression)
                } else {
                    // Every round used and the bar not full (retail state 8).
                    format!(
                        "Matched their {}, but the {} has nothing more to show. Friendship {:.0}% | press E to try again",
                        SOCIAL[requested], t.name, t.mind.impression
                    )
                };
                ctx.run.request_save();
            } else if t.roamer.is_some() {
                // Giants have no patience for a wrong answer.
                *t.hostile = true;
                t.mind.response_time = 0.0;
                ctx.encounter.message =
                    format!("Wrong answer: the {} is enraged!", t.name.to_lowercase());
                ctx.run.request_save();
            } else {
                // A wrong answer costs the round (0xDA4CAA state 7 -> 4): the
                // creature shows its next ability, or ends the proposal when
                // every round is used; the friendship earned so far stays.
                // It reacts with 0x068D8F9C (state 7) before it goes on.
                play_cue(t.mind, crate::creature_rules::SOCIAL_CLIP_WRONG);
                let impression = t.mind.impression;
                ctx.encounter.message = if next_round(t.mind, t.social) {
                    format!(
                        "Wrong answer: they performed {}. Friendship {impression:.0}% | answer the next one",
                        SOCIAL[requested]
                    )
                } else {
                    format!(
                        "Wrong answer: they performed {}. Friendship {impression:.0}% | press E to try again",
                        SOCIAL[requested]
                    )
                };
                ctx.run.request_save();
            }
        }
    } else if let Some(choice) = selected.or_else(|| launch_slot(ctx.encounter, t.id)) {
        let rank = levels(ctx.player, EncounterMode::Combat)[choice];
        let ability = combat_ability(&ctx.originals.creature_abilities, choice, rank);
        let range_min = crate::creature_rules::combat_ability(ability.kind, ability.level)
            .map_or(0.0, |stats| stats.range_min);
        let charge = ability.kind == crate::creature_rules::CombatKind::Charge;
        // The launch distance of the attack behaviour (0xD809D0):
        // `mRushingRange + mRange`, the rushing range being the charge's only.
        let launch_distance =
            ability.range + if charge { crate::creature_rules::CHARGE_RUSH_RANGE } else { 0.0 };
        if rank == 0 {
            ctx.encounter.launch = None;
            ctx.encounter.message = format!("Mount a part with {}", COMBAT[choice]);
        } else if t.distance < range_min {
            // 0xD1E4B0 (checked before the recharge, 0xD1F899): a target nearer
            // than the ability's minimum range (a charge needs its run-up).
            ctx.encounter.launch = None;
            ctx.encounter.fail(AbilityError::TargetTooClose);
        } else if ctx.encounter.cooldowns[choice] > 0. {
            ctx.encounter.launch = None;
            ctx.encounter.fail(AbilityError::NotReady);
        } else if t.distance > launch_distance {
            // Not an error in retail: the attack behaviour walks the avatar to
            // the launch point first (`creature_controls` keeps the goal on
            // the target while `launch` is set).
            let radius = (dist_xz(ctx.player_position, t.position) - t.distance).max(0.0);
            ctx.encounter.launch = Some(Launch {
                target: t.id,
                slot: choice,
                stop: (launch_distance - LAUNCH_MARGIN).max(0.5) + radius,
            });
        } else if charge {
            // Retail: the charge runs at the target (the rush, `Rush`), the
            // refresh starts with the use and the clip player plays the
            // run-up then; the hit lands when the rush arrives.
            ctx.encounter.launch = None;
            ctx.encounter.rush = Some(Rush::start(t.id, rank, t.position, t.distance, ability.range));
            ctx.encounter.cooldowns[choice] = ability.recharge;
            ctx.encounter.message = "Charge!".into();
        } else {
            ctx.encounter.launch = None;
            land_hit(ctx, &mut t, choice, &ability);
            ctx.encounter.cooldowns[choice] = ability.recharge;
        }
    }
}

/// The button of the attack that is still walking to `target`, if any.
fn launch_slot(encounter: &Encounter, target: u32) -> Option<usize> {
    encounter.launch.filter(|launch| launch.target == target).map(|launch| launch.slot)
}

/// A player's attack lands on `t` (a bite, strike or spit at once, a charge when
/// its rush arrives): the target turns hostile and its herd answers, then the
/// retail pipeline: the per-level damage (stealth x1.5 for a bite or strike),
/// then the hit's effect (charge stuns and shoves, spit poisons).
fn land_hit(ctx: &mut Ctx, t: &mut TargetView, choice: usize, ability: &CombatAbility) {
    *t.hostile = true;
    t.mind.allied = false;
    t.mind.provoked = true;
    t.mind.response_time = 0.0;
    if t.roamer.is_none() {
        // Retail herds (0xC075E0): the victim's herd mates within the
        // help radius answer, up to the alert cap; the species is not
        // aggravated as a whole. Legacy species keep that rule.
        if ctx.run.species_world.archetype_for(t.id).is_some() {
            ctx.alerts.push((t.id, t.position));
        } else {
            ctx.run.species_world.record_attack(t.id);
        }
        ctx.run
            .creature_pack
            .members
            .retain(|member| *member != t.id);
    }
    let damage = player_hit_damage(ability, ctx.stealth_strike);
    *t.hp = (*t.hp - damage).max(0.);
    apply_hit_effect(t.mind, ability.effect, damage, ctx.player_position, t.position);
    ctx.encounter.message = format!("{} hit", COMBAT[choice]);
    ctx.run.request_save();
    if *t.hp <= 0. {
        finish_kill(ctx, t);
    }
}

/// The rush of the avatar's charge, once per frame for the targeted creature:
/// keep its goal on the target, drop it when the range ran out, land the hit
/// when it arrived. Returns true while a rush is under way.
fn drive_rush(ctx: &mut Ctx, t: &mut TargetView) -> bool {
    let Some(rush) = ctx.encounter.rush.filter(|rush| rush.target == t.id) else {
        return false;
    };
    if rush.missed {
        ctx.encounter.rush = None;
        ctx.encounter.message = "The charge missed".into();
        return false;
    }
    if rush.reached {
        ctx.encounter.rush = None;
        let ability = combat_ability(&ctx.originals.creature_abilities, 1, rush.rank);
        land_hit(ctx, t, 1, &ability);
        return true;
    }
    // The charge is targeted (`mbRushingTargetedCharge`): it follows the target.
    ctx.encounter.rush = Some(Rush { goal: t.position, gap: t.distance, ..rush });
    true
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    originals: Res<crate::Originals>,
    mut run: ResMut<RunProgress>,
    mut next: ResMut<NextState<GameStage>>,
    mut encounter: ResMut<Encounter>,
    mobility: Option<Res<crate::creature::mobility::CreatureMobility>>,
    controls: Option<Res<crate::creature_controls::CreatureControls>>,
    mut awards: ResMut<CreatureAwards>,
    mut player: Query<(&Transform, &mut Health), With<Player>>,
    mut actors: Actors,
    mut roamers: Roamers,
) {
    let Ok((pt, mut health)) = player.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    // The actor's vertical offset is relative to its own terrain baseline.
    // Include that separation in attack ranges without confusing hillside
    // elevation with a jump. Grounded pack members keep their ordinary range.
    let airborne_height = mobility
        .as_deref()
        .map_or(0.0, |mobility| mobility.height.max(0.0));
    let stealth_strike = mobility.as_deref().is_some_and(|mobility| mobility.stealth_strike);
    let player_gap = |target: Vec3, body_radius: f32| {
        (dist_xz(pt.translation, target) - body_radius)
            .max(0.0)
            .hypot(airborne_height)
    };
    for cd in &mut encounter.cooldowns {
        *cd = (*cd - dt).max(0.);
    }
    // A rush belongs to the target that is still the avatar's, and so does the
    // walk to its launch point (a new target or a stance change ends it).
    if encounter.rush.is_some_and(|rush| encounter.target != Some(rush.target)) {
        encounter.rush = None;
    }
    if encounter.launch.is_some_and(|launch| {
        encounter.target != Some(launch.target) || encounter.mode != EncounterMode::Combat
    }) {
        encounter.launch = None;
    }
    let a = player_abilities(&run, &originals);
    let valid = valid_allies(&run, &actors, &roamers);
    let dna = run.earned_creature_dna();
    if crate::creature_pack::sanitize(&mut run.creature_pack, dna, &valid) {
        run.request_save();
    }
    // Targeting, pack stances, and recruiting come from `creature_controls`: F targets the
    // nearest creature, T cycles pack orders, stance changes and double clicks engage the current
    // target, `KEY_RECRUIT` (P) recruits.
    let (target_request, recruit, pack_stance) = controls.as_deref().map_or(
        (crate::creature_controls::TargetRequest::None, false, false),
        |controls| (controls.target_request, controls.recruit_request, controls.pack_stance_request),
    );
    if pack_stance {
        let new_stance = run.creature_pack.cycle_stance(pt.translation.to_array());
        encounter.message = format!("Pack order: {}", new_stance.label());
        awards.push(format!("Pack Order: {}", new_stance.label()));
        run.request_save();
    }
    if recruit {
        let mut nearby: Vec<(u32, f32)> = actors
            .iter()
            .filter(|(id, t, _, _, _)| {
                valid.contains(&id.0) && dist_xz(pt.translation, t.translation) <= TARGET_RANGE
            })
            .map(|(id, t, _, _, _)| (id.0, dist_xz(pt.translation, t.translation)))
            .collect();
        for (id, t, actor, hp, mind, _) in roamers.iter() {
            if actor.kind == RoamerKind::Rogue
                && valid.contains(&id.0)
                && mind.allied
                && !hp.dead()
                && dist_xz(pt.translation, t.translation) <= TARGET_RANGE
            {
                nearby.push((id.0, dist_xz(pt.translation, t.translation)));
            }
        }
        let candidate = encounter
            .target
            .filter(|id| nearby.iter().any(|(n, _)| n == id))
            .or_else(|| {
                nearby
                    .iter()
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|n| n.0)
            });
        encounter.message = if let Some(id) = candidate {
            match crate::creature_pack::toggle(&mut run.creature_pack, dna, id, &valid) {
                Ok(recruited) => {
                    run.request_save();
                    if recruited {
                        if run.species_world.roamer(id).is_some_and(|r| r.kind == RoamerKind::Rogue) {
                            "Recruited powerful Rogue ally to pack!".into()
                        } else {
                            "Recruited ally to pack".into()
                        }
                    } else {
                        "Dismissed pack member".into()
                    }
                }
                Err(message) => message.into(),
            }
        } else if encounter
            .target
            .and_then(|id| run.species_world.roamer(id))
            .is_some_and(|r| r.kind == RoamerKind::Epic)
        {
            "Epic creatures are untamable giants; they never join a pack".into()
        } else if encounter
            .target
            .and_then(|id| run.species_world.roamer(id))
            .is_some_and(|r| r.kind == RoamerKind::Rogue && r.relationship != Relationship::Allied)
        {
            "Impress the rogue creature with social abilities before recruiting".into()
        } else {
            "Move near a living ally to recruit".into()
        };
    }
    if target_request != crate::creature_controls::TargetRequest::None {
        // The stance (Social or Combat) is the verb tray's (Tab); this only
        // picks the target and engages it as the stance asks.
        // Strangers before allies, then the closest body edge.
        let mut best: Option<(bool, f32, u32)> = None;
        let mut consider = |allied: bool, distance: f32, id: u32| {
            if distance > TARGET_RANGE {
                return;
            }
            if best.is_none_or(|(best_allied, best_distance, _)| {
                (!allied && best_allied) || (allied == best_allied && distance < best_distance)
            }) {
                best = Some((allied, distance, id));
            }
        };
        for (id, t, _, mind, _) in actors.iter() {
            if !mind.defeated && mind.downed <= 0.0 {
                consider(mind.allied, dist_xz(pt.translation, t.translation), id.0);
            }
        }
        for (id, t, actor, hp, mind, _) in roamers.iter() {
            if !mind.defeated && !hp.dead() {
                let radius = crate::epic_creatures::stats(actor.kind).body_radius;
                consider(
                    mind.allied,
                    (dist_xz(pt.translation, t.translation) - radius).max(0.0),
                    id.0,
                );
            }
        }
        if target_request == crate::creature_controls::TargetRequest::Nearest {
            encounter.target = best.map(|(_, _, id)| id);
        }
        match encounter.target {
            Some(target) => {
                let social = levels(
                    member_abilities(&run, &originals, target),
                    EncounterMode::Social,
                );
                let name = member_name(&run, target);
                let mut ctx = Ctx {
                    run: &mut run,
                    next: &mut next,
                    encounter: &mut encounter,
                    awards: &mut awards,
                    originals: &originals,
                    player: a,
                    drops: Vec::new(),
                    stealth_strike: false,
                    player_position: pt.translation,
                    alerts: Vec::new(),
                };
                if let Some((_, t, mut npc, mut mind, mut visibility)) =
                    actors.iter_mut().find(|(id, _, _, _, _)| id.0 == target)
                {
                    let npc = &mut *npc;
                    engage(
                        &mut ctx,
                        &mut TargetView {
                            id: target,
                            name,
                            distance: player_gap(t.translation, 0.0),
                            position: t.translation,
                            hp: &mut npc.hp,
                            hostile: &mut npc.hostile,
                            mind: &mut mind,
                            visibility: &mut visibility,
                            social,
                            roamer: None,
                        },
                    );
                } else if let Some((_, t, mut actor, mut hp, mut mind, mut visibility)) = roamers
                    .iter_mut()
                    .find(|(id, _, _, _, _, _)| id.0 == target)
                {
                    let stats = crate::epic_creatures::stats(actor.kind);
                    engage(
                        &mut ctx,
                        &mut TargetView {
                            id: target,
                            name,
                            distance: player_gap(t.translation, stats.body_radius),
                            position: t.translation,
                            hp: &mut hp.hp,
                            hostile: &mut actor.aggressive,
                            mind: &mut mind,
                            visibility: &mut visibility,
                            social,
                            roamer: Some(stats),
                        },
                    );
                }
            }
            None => encounter.message = "Move closer to a creature".into(),
        }
    }
    let selected = if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        None
    } else {
        [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
        ]
        .iter()
        .position(|&key| keys.just_pressed(key))
    };
    if let Some(choice) = selected {
        // 0xD1E530 returns 3 for an attack ability and 0xD1E720 returns 4 for a
        // social ability with nothing targeted (both `crg-notarget`); an empty
        // slot has no button to press.
        if encounter.target.is_none() && levels(a, encounter.mode)[choice] > 0 {
            encounter.fail(AbilityError::NoTarget);
        }
    }
    // Where the target stands, and how far its body reaches.
    let target_reach = encounter.target.and_then(|target| {
        actors
            .iter()
            .find(|(id, _, _, _, _)| id.0 == target)
            .map(|(_, t, _, _, _)| (t.translation, 0.0))
            .or_else(|| {
                roamers
                    .iter()
                    .find(|(id, _, _, _, _, _)| id.0 == target)
                    .map(|(_, t, actor, _, _, _)| {
                        (
                            t.translation,
                            crate::epic_creatures::stats(actor.kind).body_radius,
                        )
                    })
            })
    });
    let mut social_support: Vec<(u32, [u8; 4])> = actors
        .iter()
        .filter(|(id, t, npc, mind, _)| {
            run.creature_pack.members.contains(&id.0)
                && mind.allied
                && !mind.defeated
                && mind.downed <= 0.0
                && npc.hp > 0.
                && mind.attack_timer <= 0.0
                && target_reach.is_some_and(|(target, radius)| {
                    dist_xz(t.translation, target) <= PERFORM_RANGE + radius
                })
        })
        .map(|(id, _, _, _, _)| {
            (
                id.0,
                levels(
                    member_abilities(&run, &originals, id.0),
                    EncounterMode::Social,
                ),
            )
        })
        .collect();
    for (id, t, actor, hp, mind, _) in roamers.iter() {
        if run.creature_pack.members.contains(&id.0)
            && mind.allied
            && !mind.defeated
            && !hp.dead()
            && actor.attack_timer <= 0.0
            && target_reach.is_some_and(|(target, radius)| {
                dist_xz(t.translation, target) <= PERFORM_RANGE + radius
            })
        {
            if let Some(caps) = crate::epic_creatures::abilities(&run, &originals, id.0) {
                social_support.push((id.0, levels(caps, EncounterMode::Social)));
            }
        }
    }
    // Pack members hostile creatures may turn on.
    let mut pack_positions: Vec<(u32, Vec3)> = actors
        .iter()
        .filter(|(id, _, npc, mind, _)| {
            run.creature_pack.members.contains(&id.0)
                && mind.allied
                && !mind.defeated
                && mind.downed <= 0.0
                && npc.hp > 0.
        })
        .map(|(id, t, _, _, _)| (id.0, t.translation))
        .collect();
    for (id, t, _, hp, mind, _) in roamers.iter() {
        if run.creature_pack.members.contains(&id.0)
            && mind.allied
            && !mind.defeated
            && !hp.dead()
        {
            pack_positions.push((id.0, t.translation));
        }
    }
    let mut performing_supporters = Vec::new();
    let mut pack_hits: Vec<(u32, f32)> = Vec::new();
    let mut ctx = Ctx {
        run: &mut run,
        next: &mut next,
        encounter: &mut encounter,
        awards: &mut awards,
        originals: &originals,
        player: a,
        drops: Vec::new(),
        stealth_strike,
        player_position: pt.translation,
        alerts: Vec::new(),
    };
    for (id, t, mut npc, mut mind, mut visibility) in &mut actors {
        if mind.defeated {
            *visibility = Visibility::Hidden;
            continue;
        }
        if mind.downed > 0.0 {
            if ctx.encounter.target == Some(id.0) {
                ctx.encounter.target = None;
                ctx.encounter.message = "Your target was knocked out and fled".into();
            }
            continue;
        }
        if ctx
            .run
            .species_world
            .species_aggravated(mind.species as u32)
            && !ctx.run.creature_pack.members.contains(&id.0)
        {
            mind.allied = false;
            npc.hostile = true;
        } else if !mind.allied && !mind.provoked && ctx.run.species_world.member_friendly(id.0) {
            // Best friends with a retail herd: the whole herd is friendly.
            mind.allied = true;
            mind.response_time = 0.0;
            mind.performing = false;
            npc.hostile = false;
        } else if !mind.allied && !mind.provoked {
            // Attack on sight follows the species' relationship to the avatar
            // (0xC0B9C0 -> goal 0x40000 in 0xD6EEC0): once befriending or
            // killing has moved it out of hated / hostile, the whole herd stops
            // (no path leads back: kills make a species fearful, never angrier).
            // The spawn (`spawn_species_member`) sets the flag from the same
            // test; a legacy species keeps the flag it was spawned with.
            if npc.hostile
                && species_relationship_of(&ctx.run.species_world, mind.species as u32)
                    .is_some_and(|relationship| !crate::creature_rules::attacks_on_sight(relationship))
            {
                npc.hostile = false;
            }
        }
        mind.attack_timer = (mind.attack_timer - dt).max(0.);
        mind.response_time = (mind.response_time - dt).max(0.);
        let (before_drain, drain_rate) = drain_tuning(ctx.run.species_world.archetype_for(id.0));
        social_tick(&mut mind, dt, before_drain, drain_rate);
        mind.bored = (mind.bored - dt).max(0.);
        mind.stunned = (mind.stunned - dt).max(0.);
        if mind.poisoned > 0.0 {
            // Poison from the player's spit (retail combat effect): HP per
            // second for the effect duration; a poisoned death is the player's kill.
            let (damage, left) = crate::creature_rules::poison_tick(mind.poisoned, mind.poison_dps, dt);
            mind.poisoned = left;
            npc.hp = (npc.hp - damage).max(0.);
            if left <= 0.0 {
                mind.poison_dps = 0.0;
            }
            if npc.hp <= 0. {
                let name = member_name(ctx.run, id.0);
                let npc = &mut *npc;
                finish_kill(
                    &mut ctx,
                    &mut TargetView {
                        id: id.0,
                        name,
                        distance: 0.0,
                        position: t.translation,
                        hp: &mut npc.hp,
                        hostile: &mut npc.hostile,
                        mind: &mut mind,
                        visibility: &mut visibility,
                        social: [0; 4],
                        roamer: None,
                    },
                );
                continue;
            }
        }
        let distance = player_gap(t.translation, 0.0);
        if npc.hostile && !mind.allied && mind.attack_timer <= 0. && mind.stunned <= 0. {
            // Fight back against the nearest attacker in reach: the player
            // or one of the player's pack.
            let (ranks, scale) = fight_back_ranks(levels(
                member_abilities(ctx.run, ctx.originals, id.0),
                EncounterMode::Combat,
            ));
            let mut victims: Vec<(Option<u32>, f32)> = pack_positions
                .iter()
                .filter(|(member, _)| *member != id.0)
                .map(|(member, at)| (Some(*member), dist_xz(*at, t.translation)))
                .collect();
            if !health.dead() {
                victims.push((None, distance));
            }
            // The player's nest mates are creatures of the avatar's own species
            // and are attacked like the pack (creature_nestmates, ADAPTED: the
            // retail victim choice by relationship was not traced).
            victims.extend(
                ctx.encounter
                    .mates
                    .iter()
                    .map(|(mate, at)| (Some(*mate), dist_xz(*at, t.translation))),
            );
            victims.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.is_some().cmp(&b.0.is_some())));
            for (who, gap) in victims {
                if gap > NPC_ATTACK_REACH {
                    continue;
                }
                if let Some((slot, ability)) =
                    pack_attack(&ctx.originals.creature_abilities, ranks, gap)
                {
                    // The attacker plays its ability's own clip (not always a bite).
                    note_attack(&mut mind, &ctx.originals.creature_abilities, slot, ability.level);
                    // Retail: the ability's damage, no randomness and no misses
                    // (npcAttackMissPercentage 0), times the herd archetype's
                    // DamageMultiplier (a baby x0.25); a weaponless creature's
                    // struggle is scaled down.
                    let damage = ability.damage * scale * attacker_multiplier(ctx.run, id.0);
                    // The creatures near the attacker answer the attack on the
                    // avatar or on a mate (0xC075E0 event, 0xD8F1C0 radius): the
                    // pack members and the nest mates within
                    // `creatureHelpAllyInRadius` of it.
                    let defend = |encounter: &mut Encounter| {
                        let pack: Vec<(u32, Vec2)> = pack_positions
                            .iter()
                            .map(|(member, at)| (*member, at.xz()))
                            .collect();
                        encounter.defenders =
                            crate::creature_support::defenders(t.translation.xz(), &pack);
                        encounter.defend = Some(id.0);
                        encounter.defend_time = crate::creature_support::ALERT_MEMORY_SECONDS;
                    };
                    match who {
                        None => {
                            health.hurt(damage);
                            hurt_avatar_with_effect(ctx.encounter, ability.effect);
                            defend(ctx.encounter);
                        }
                        Some(member) if member >= crate::creature_nestmates::MATE_ID_BASE => {
                            ctx.encounter.mate_hits.push((member, damage, id.0));
                            defend(ctx.encounter);
                        }
                        Some(member) => pack_hits.push((member, damage)),
                    }
                    mind.attack_timer = ability.recharge;
                    break;
                }
            }
        }
        if ctx.encounter.target != Some(id.0) {
            continue;
        }
        if distance > LEAVE_RANGE {
            ctx.encounter.target = None;
            ctx.encounter.message = "Target out of range".into();
            continue;
        }
        let social = levels(
            member_abilities(ctx.run, ctx.originals, id.0),
            EncounterMode::Social,
        );
        let name = member_name(ctx.run, id.0);
        let npc = &mut *npc;
        drive_target(
            &mut ctx,
            TargetView {
                id: id.0,
                name,
                distance,
                position: t.translation,
                hp: &mut npc.hp,
                hostile: &mut npc.hostile,
                mind: &mut mind,
                visibility: &mut visibility,
                social,
                roamer: None,
            },
            selected,
            recruit,
            &social_support,
            &mut performing_supporters,
        );
    }
    for (id, t, mut actor, mut hp, mut mind, mut visibility) in &mut roamers {
        if mind.defeated {
            *visibility = Visibility::Hidden;
            continue;
        }
        mind.response_time = (mind.response_time - dt).max(0.);
        let (before_drain, drain_rate) = drain_tuning(Some(actor.kind.archetype()));
        social_tick(&mut mind, dt, before_drain, drain_rate);
        mind.bored = (mind.bored - dt).max(0.);
        mind.stunned = (mind.stunned - dt).max(0.);
        if mind.poisoned > 0.0 {
            let (damage, left) = crate::creature_rules::poison_tick(mind.poisoned, mind.poison_dps, dt);
            mind.poisoned = left;
            hp.hp = (hp.hp - damage).max(0.);
            if left <= 0.0 {
                mind.poison_dps = 0.0;
            }
            if hp.hp <= 0. {
                let stats = crate::epic_creatures::stats(actor.kind);
                let name = member_name(ctx.run, id.0);
                finish_kill(
                    &mut ctx,
                    &mut TargetView {
                        id: id.0,
                        name,
                        distance: 0.0,
                        position: t.translation,
                        hp: &mut hp.hp,
                        hostile: &mut actor.aggressive,
                        mind: &mut mind,
                        visibility: &mut visibility,
                        social: [0; 4],
                        roamer: Some(stats),
                    },
                );
                continue;
            }
        }
        if ctx.encounter.target != Some(id.0) {
            continue;
        }
        let stats = crate::epic_creatures::stats(actor.kind);
        let distance = player_gap(t.translation, stats.body_radius);
        if distance > LEAVE_RANGE {
            ctx.encounter.target = None;
            ctx.encounter.message = "Target out of range".into();
            continue;
        }
        let social = levels(
            member_abilities(ctx.run, ctx.originals, id.0),
            EncounterMode::Social,
        );
        let name = member_name(ctx.run, id.0);
        drive_target(
            &mut ctx,
            TargetView {
                id: id.0,
                name,
                distance,
                position: t.translation,
                hp: &mut hp.hp,
                hostile: &mut actor.aggressive,
                mind: &mut mind,
                visibility: &mut visibility,
                social,
                roamer: Some(stats),
            },
            selected,
            recruit,
            &social_support,
            &mut performing_supporters,
        );
    }
    for (id, _, mut npc, mut mind, mut visibility) in &mut actors {
        if ctx
            .run
            .species_world
            .species_aggravated(mind.species as u32)
            && !mind.defeated
            && !ctx.run.creature_pack.members.contains(&id.0)
        {
            mind.allied = false;
            npc.hostile = true;
        }
        if let Some((_, cooldown)) = performing_supporters
            .iter()
            .find(|(supporter, _)| *supporter == id.0)
        {
            mind.attack_timer = *cooldown;
        }
        for (_, damage) in pack_hits.iter().filter(|(member, _)| *member == id.0) {
            if mind.defeated || npc.hp <= 0.0 {
                break;
            }
            npc.hp = (npc.hp - damage).max(0.0);
            if npc.hp <= 0.0 {
                crate::creature_social::knock_out(&mut npc, &mut mind, &mut visibility);
                ctx.encounter.message =
                    "A pack member was knocked out and fled back to its nest".into();
            }
        }
    }
    for (id, _, _, mut hp, mut mind, mut visibility) in &mut roamers {
        if !ctx.run.creature_pack.members.contains(&id.0) {
            continue;
        }
        for (_, damage) in pack_hits.iter().filter(|(member, _)| *member == id.0) {
            if mind.defeated || hp.dead() {
                break;
            }
            hp.hurt(*damage);
            if hp.dead() {
                mind.defeated = true;
                *visibility = Visibility::Hidden;
                ctx.encounter.message = "A rogue pack member was knocked out in battle".into();
            }
        }
    }
    // Assist only the player's explicit hostile combat target. Each member
    // shares the same attack timer that checkpoints already persist.
    ctx.encounter.defend_time = (ctx.encounter.defend_time - dt).max(0.0);
    if ctx.encounter.defend_time <= 0.0 {
        ctx.encounter.defend = None;
        ctx.encounter.defenders.clear();
    }
    let player_target = ctx
        .encounter
        .target
        .filter(|_| ctx.encounter.mode == EncounterMode::Combat);
    // Without a combat target of the player's, the pack members near the
    // creature that hit the avatar fight it (creature_support).
    let defenders_only = player_target.is_none().then(|| ctx.encounter.defenders.clone());
    if let Some(chosen) = player_target.or(ctx.encounter.defend) {
        let npc_target = actors
            .iter()
            .find(|(id, t, npc, mind, _)| {
                chosen == id.0
                    && npc.hostile
                    && !mind.allied
                    && !mind.defeated
                    && mind.downed <= 0.0
                    && npc.hp > 0.
                    && dist_xz(pt.translation, t.translation) <= LEAVE_RANGE
            })
            .map(|(id, t, _, _, _)| (id.0, t.translation, 0.0));
        let roamer_target = roamers
            .iter()
            .find(|(id, t, actor, hp, mind, _)| {
                chosen == id.0
                    && !mind.allied
                    && !mind.defeated
                    && hp.hp > 0.
                    && dist_xz(pt.translation, t.translation)
                        - crate::epic_creatures::stats(actor.kind).body_radius
                        <= LEAVE_RANGE
            })
            .map(|(id, t, actor, _, _, _)| {
                (
                    id.0,
                    t.translation,
                    crate::epic_creatures::stats(actor.kind).body_radius,
                )
            });
        if let Some((target_id, target_position, radius)) = npc_target.or(roamer_target) {
            let mut damage = 0.;
            for (id, t, npc, mut mind, _) in &mut actors {
                if !ctx.run.creature_pack.members.contains(&id.0)
                    || defenders_only.as_ref().is_some_and(|only| !only.contains(&id.0))
                    || !mind.allied
                    || mind.defeated
                    || mind.downed > 0.0
                    || npc.hp <= 0.
                    || mind.attack_timer > 0.
                {
                    continue;
                }
                let Some((slot, ability)) = pack_attack(
                    &ctx.originals.creature_abilities,
                    levels(
                        member_abilities(ctx.run, ctx.originals, id.0),
                        EncounterMode::Combat,
                    ),
                    (dist_xz(t.translation, target_position) - radius).max(0.0),
                ) else {
                    continue;
                };
                damage += ability.damage * attacker_multiplier(ctx.run, id.0);
                mind.attack_timer = ability.recharge;
                note_attack(&mut mind, &ctx.originals.creature_abilities, slot, ability.level);
            }
            for (id, t, mut actor, hp, mut mind, _) in &mut roamers {
                if !ctx.run.creature_pack.members.contains(&id.0)
                    || defenders_only.as_ref().is_some_and(|only| !only.contains(&id.0))
                    || !mind.allied
                    || mind.defeated
                    || hp.dead()
                    || actor.attack_timer > 0.
                {
                    continue;
                }
                let Some(caps) = crate::epic_creatures::abilities(ctx.run, ctx.originals, id.0) else {
                    continue;
                };
                let Some((slot, ability)) = pack_attack(
                    &ctx.originals.creature_abilities,
                    levels(caps, EncounterMode::Combat),
                    (dist_xz(t.translation, target_position) - radius).max(0.0),
                ) else {
                    continue;
                };
                damage += ability.damage * attacker_multiplier(ctx.run, id.0);
                actor.attack_timer = ability.recharge;
                note_attack(&mut mind, &ctx.originals.creature_abilities, slot, ability.level);
            }
            if damage > 0. {
                let name = member_name(ctx.run, target_id);
                if let Some((_, at, _)) = npc_target {
                    if ctx.run.species_world.archetype_for(target_id).is_some() {
                        ctx.alerts.push((target_id, at));
                    } else {
                        ctx.run.species_world.record_attack(target_id);
                    }
                    if let Some((_, t, mut npc, mut mind, mut visibility)) =
                        actors.iter_mut().find(|(id, _, _, _, _)| id.0 == target_id)
                    {
                        npc.hp = (npc.hp - damage).max(0.);
                        if npc.hp <= 0. {
                            let npc = &mut *npc;
                            finish_kill(
                                &mut ctx,
                                &mut TargetView {
                                    id: target_id,
                                    name,
                                    distance: 0.0,
                                    position: t.translation,
                                    hp: &mut npc.hp,
                                    hostile: &mut npc.hostile,
                                    mind: &mut mind,
                                    visibility: &mut visibility,
                                    social: [0; 4],
                                    roamer: None,
                                },
                            );
                        }
                    }
                } else if let Some((_, t, mut actor, mut hp, mut mind, mut visibility)) = roamers
                    .iter_mut()
                    .find(|(id, _, _, _, _, _)| id.0 == target_id)
                {
                    actor.aggressive = true;
                    hp.hp = (hp.hp - damage).max(0.);
                    if hp.hp <= 0. {
                        let stats = crate::epic_creatures::stats(actor.kind);
                        finish_kill(
                            &mut ctx,
                            &mut TargetView {
                                id: target_id,
                                name,
                                distance: 0.0,
                                position: t.translation,
                                hp: &mut hp.hp,
                                hostile: &mut actor.aggressive,
                                mind: &mut mind,
                                visibility: &mut visibility,
                                social: [0; 4],
                                roamer: Some(stats),
                            },
                        );
                    }
                }
                ctx.run.request_save();
            }
        }
    }
    // The nest mates' strikes (creature_nestmates, computed before this system
    // ran): landed like a pack member's, with the kill, the herd alert and the
    // save request of the pack path.
    let strikes = std::mem::take(&mut ctx.encounter.mate_strikes);
    for (target_id, damage) in strikes {
        let name = member_name(ctx.run, target_id);
        let Some((_, t, mut npc, mut mind, mut visibility)) =
            actors.iter_mut().find(|(id, _, _, _, _)| id.0 == target_id)
        else {
            continue;
        };
        if mind.defeated || npc.hp <= 0.0 {
            continue;
        }
        if ctx.run.species_world.archetype_for(target_id).is_some() {
            ctx.alerts.push((target_id, t.translation));
        } else {
            ctx.run.species_world.record_attack(target_id);
        }
        npc.hp = (npc.hp - damage).max(0.);
        if npc.hp <= 0. {
            let npc = &mut *npc;
            finish_kill(
                &mut ctx,
                &mut TargetView {
                    id: target_id,
                    name,
                    distance: 0.0,
                    position: t.translation,
                    hp: &mut npc.hp,
                    hostile: &mut npc.hostile,
                    mind: &mut mind,
                    visibility: &mut visibility,
                    social: [0; 4],
                    roamer: None,
                },
            );
        }
        ctx.run.request_save();
    }
    let drops = std::mem::take(&mut ctx.drops);
    let alerts = std::mem::take(&mut ctx.alerts);
    drop(ctx);
    alert_herds(&run, &alerts, &mut actors);
    for (position, food) in drops {
        crate::creature_social::spawn_meat(&mut commands, position, food);
    }
    let valid = valid_allies(&run, &actors, &roamers);
    let dna = run.earned_creature_dna();
    if crate::creature_pack::sanitize(&mut run.creature_pack, dna, &valid) {
        run.request_save();
    }
}

/// The herds of the retail members hit this frame answer (0xC075E0): the
/// victim's herd mates within `attacked_SpeciesHelpRadius`, at most
/// `attacked_MaxSpeciesToAlert`, turn on the player.
fn alert_herds(run: &RunProgress, alerts: &[(u32, Vec3)], actors: &mut Actors) {
    for &(victim, at) in alerts {
        let (Some(archetype), Some(nest)) = (
            run.species_world.archetype_for(victim),
            run.species_world.nest_index_for_member(victim),
        ) else {
            continue;
        };
        let candidates: Vec<(u32, Vec2)> = actors
            .iter()
            .filter(|(id, _, npc, mind, _)| {
                id.0 != victim
                    && npc.hp > 0.
                    && !mind.defeated
                    && mind.downed <= 0.0
                    && !run.creature_pack.members.contains(&id.0)
                    && run.species_world.nest_index_for_member(id.0) == Some(nest)
            })
            .map(|(id, t, _, _, _)| (id.0, t.translation.xz()))
            .collect();
        let helpers = crate::creature_support::herd_helpers(archetype, at.xz(), &candidates);
        for (id, _, mut npc, mut mind, _) in actors.iter_mut() {
            if helpers.contains(&id.0) {
                npc.hostile = true;
                mind.allied = false;
                mind.provoked = true;
                mind.response_time = 0.0;
                mind.performing = false;
            }
        }
    }
}

/// Living, recruitable allies (members of an angry species only while they
/// already ride with the pack, plus befriended Rogue creatures).
fn valid_allies(run: &RunProgress, actors: &Actors, roamers: &Roamers) -> Vec<u32> {
    let mut valid: Vec<u32> = actors
        .iter()
        .filter(|(id, _, npc, mind, _)| {
            mind.allied
                && !mind.defeated
                && mind.downed <= 0.0
                && npc.hp > 0.
                && run.species_world.species_for_member(id.0).is_some()
                && (!run.species_world.species_aggravated(mind.species as u32)
                    || run.creature_pack.members.contains(&id.0))
        })
        .map(|(id, _, _, _, _)| id.0)
        .collect();
    for (id, _, actor, hp, mind, _) in roamers.iter() {
        if actor.kind == RoamerKind::Rogue
            && mind.allied
            && !mind.defeated
            && !hp.dead()
        {
            valid.push(id.0);
        }
    }
    valid
}

pub fn hud(
    encounter: &Encounter,
    run: &RunProgress,
    originals: &crate::Originals,
    actors: impl Iterator<Item = (u32, CreatureMind)>,
) -> String {
    let a = player_abilities(run, originals);
    let names = if encounter.mode == EncounterMode::Social {
        SOCIAL
    } else {
        COMBAT
    };
    let ranks = levels(a, encounter.mode);
    let abilities = (0..4)
        .map(|i| {
            format!(
                "{} {} {}{}",
                i + 1,
                names[i],
                ranks[i],
                if encounter.cooldowns[i] > 0. {
                    format!(" ({:.1}s)", encounter.cooldowns[i])
                } else {
                    String::new()
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let target = encounter
        .target
        .and_then(|target| actors.into_iter().find(|(id, _)| *id == target))
        .map(|(id, m)| {
            let name = member_name(run, id);
            if encounter.mode != EncounterMode::Social {
                String::new()
            } else if m.allied {
                format!("{name}: ally |")
            } else if m.bored > 0.0 {
                format!("{name} lost interest ({:.0}s) |", m.bored)
            } else if m.response_time <= 0.0 {
                match run.species_world.roamer(id) {
                    Some(roamer) => format!(
                        "{name} answers only rank {} performances |",
                        crate::epic_creatures::stats(roamer.kind).required_social_rank
                    ),
                    None => {
                        let social = levels(member_abilities(run, originals, id), EncounterMode::Social);
                        if social.iter().any(|rank| *rank > 0) {
                            format!("{name}: friendship {:.0}% | press E to ask for a performance", m.impression)
                        } else {
                            "No social performance parts".to_string()
                        }
                    }
                }
            } else if m.response_time > crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT {
                let action = m.requested.min(3) as usize;
                format!(
                    "{name} shows {} - wait | friendship {:.0}% | {:.1}s |",
                    SOCIAL[action],
                    m.impression,
                    m.response_time - crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT
                )
            } else {
                let action = m.requested.min(3) as usize;
                format!(
                    "{name} performs {} - answer {} | friendship {:.0}% | {:.1}s |",
                    SOCIAL[action],
                    action + 1,
                    m.impression,
                    m.response_time
                )
            }
        })
        .unwrap_or_default();
    format!("{abilities}\n{target} {}", encounter.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A creature whose round is open (the 15 s answer window), asking for action 0.
    fn open_round() -> CreatureMind {
        CreatureMind {
            response_time: crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT,
            performing: true,
            ..default()
        }
    }

    #[test]
    fn social_progress_follows_the_retail_power_formula_and_befriends_once() {
        // A wrong answer with a level-5 ability still adds 0.4 x 5 / 3 of the
        // bar (no penalty); it closes the round like any answer (the caller
        // opens the next one, 0xDA4CAA state 7 -> 4).
        let mut m = open_round();
        assert!(!match_response(&mut m, 1, 5));
        assert!((m.impression - 200. / 3.).abs() < 1e-3, "{}", m.impression);
        assert!(m.performing && m.beats == 1, "a wrong answer only costs the round");
        // No ability, no effect.
        let mut m = open_round();
        assert!(!match_response(&mut m, 0, 0));
        assert_eq!((m.impression, m.beats), (0., 0));
        // Three correct level-1 answers (a third of the bar each) befriend it;
        // each correct answer keeps the step going for the next round.
        assert!(!match_response(&mut m, 0, 1));
        assert!(m.performing && m.beats == 1);
        m.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
        assert!(!match_response(&mut m, 0, 1));
        m.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
        assert!(match_response(&mut m, 0, 1));
        assert!(m.allied && !m.performing);
        assert_eq!(m.impression, 100.);
        // It never befriends twice.
        assert!(!match_response(&mut m, 0, 5));
        // Still demonstrating (more than 15 s left): the answer is ignored.
        let mut demo = CreatureMind {
            response_time: 18.,
            performing: true,
            ..default()
        };
        assert!(!match_response(&mut demo, 0, 5));
        assert_eq!((demo.impression, demo.response_time), (0., 18.));
        // One level-3 answer fills the bar (power 3 over the maximum 3).
        let mut strong = open_round();
        assert!(match_response(&mut strong, 0, 3));
    }

    #[test]
    fn social_requests_only_actions_the_target_can_perform() {
        let mut mind = CreatureMind::default();
        for species in 0..12 {
            mind.species = species;
            mind.performing = false;
            request(&mut mind, [0, 3, 0, 1]);
            assert!(matches!(mind.requested, 1 | 3));
            assert_eq!(mind.response_time, SOCIAL_RESPONSE_WINDOW);
        }
        mind.performing = false;
        request(&mut mind, [0; 4]);
        assert_eq!(mind.response_time, 0.);
        let choice = mind.requested as usize;
        assert!(!match_response(&mut mind, choice, 5));
    }

    #[test]
    fn social_refresh_is_a_five_second_lockout_inside_the_retail_round() {
        let catalog = CreatureAbilityCatalog {
            abilities: vec![AuthoredAbility {
                source: crate::dbpf::ResourceKey {
                    group_id: ABILITY_TEMPLATE_GROUP,
                    instance_id: 1,
                    type_id: crate::asset_catalog::PROP,
                },
                name: "VocalizeTemplate".into(),
                ability_type: 37,
                damage: None,
                ranged_damage: None,
                refresh: Some(5.0),
                recharge: None,
                range: Some(5.0),
                duration: None,
                level_multiplier: None,
            }],
            errors: vec![],
            clips: vec![],
        };
        let mut mind = CreatureMind::default();
        request(&mut mind, [1, 0, 0, 0]);
        let lockout = social_refresh(&catalog, 0, 1);
        assert_eq!(lockout, 5.0);
        // Retail round: a 5 s demonstration, then 15 s to answer.
        assert_eq!(mind.response_time, SOCIAL_RESPONSE_WINDOW);
        assert_eq!(SOCIAL_RESPONSE_WINDOW, 20.0);
        assert!(mind.performing);
        assert_eq!(mind.response_time - lockout, 15.0);
    }

    #[test]
    fn a_helping_posse_member_doubles_an_answer_and_a_wrong_one_never_subtracts() {
        let mut solo = open_round();
        let mut backed = open_round();
        ranked_match_response(&mut solo, 0, 1, false);
        ranked_match_response(&mut backed, 0, 1, true);
        assert!((solo.impression - 100. / 3.).abs() < 1e-3);
        assert!((backed.impression - 200. / 3.).abs() < 1e-3, "x2.0 with a posse member");
        // A wrong level-1 answer adds 0.4 x 1 / 3 of the bar on top of what
        // there was and closes the round (the caller opens the next one).
        let before = backed.impression;
        let mut again = open_round();
        again.impression = before;
        assert!(!ranked_match_response(&mut again, 1, 1, false));
        assert!((again.impression - (before + 40. / 3.)).abs() < 1e-3, "{}", again.impression);
        assert!(again.performing && again.beats == 1);
    }

    #[test]
    fn combat_abilities_use_the_retail_per_level_tables_not_the_templates() {
        use crate::creature_rules::{CombatEffect, CombatKind};
        let source = crate::dbpf::ResourceKey {
            group_id: ABILITY_TEMPLATE_GROUP,
            instance_id: 7,
            type_id: crate::asset_catalog::PROP,
        };
        // A template with unrelated numbers changes nothing.
        let catalog = CreatureAbilityCatalog {
            abilities: vec![AuthoredAbility {
                source,
                name: "SpitTemplate".into(),
                ability_type: 32,
                damage: Some(12.),
                ranged_damage: Some(18.),
                refresh: Some(4.5),
                recharge: Some(9.),
                range: Some(11.),
                duration: Some(2.),
                level_multiplier: Some(1.),
            }],
            errors: vec![],
            clips: vec![],
        };
        // Spit level 3: damage 1.5, refresh 3 s, range 30 + the 1.5 hit buffer,
        // poison for 6 s at 0.417 HP/s.
        assert_eq!(
            combat_ability(&catalog, 3, 3),
            CombatAbility {
                damage: 1.5,
                range: 31.5,
                recharge: 3.0,
                effect: CombatEffect::Poison {
                    seconds: 6.0,
                    damage_per_second: 0.417
                },
                kind: CombatKind::Spit,
                level: 3,
            }
        );
        let bite = combat_ability(&catalog, 0, 5);
        assert_eq!((bite.damage, bite.recharge, bite.range), (3.0, 1.5, 2.5));
        let charge = combat_ability(&catalog, 1, 4);
        assert_eq!((charge.damage, charge.recharge), (2.5, 8.0));
        assert_eq!(charge.effect, CombatEffect::Stun { seconds: 2.25 });
        let strike = combat_ability(&catalog, 2, 3);
        assert_eq!((strike.damage, strike.recharge), (6.0, 4.5));
        let none = combat_ability(&catalog, 0, 0);
        assert_eq!((none.damage, none.range, none.recharge), (0.0, 0.0, 0.0));
        assert!(catalog.get(EncounterMode::Combat, 3, 0).is_none());
        assert!(catalog.get(EncounterMode::Combat, 3, 6).is_none());
        assert!(catalog.get(EncounterMode::Social, 3, 3).is_none());
        assert_eq!(catalog.audit()["abilities"][0]["name"], "SpitTemplate");
    }

    #[test]
    fn assistant_attacks_use_capabilities_range_and_a_stable_damage_rate() {
        let catalog = CreatureAbilityCatalog::default();
        assert!(pack_attack(&catalog, [0; 4], 1.).is_none());
        assert!(pack_attack(&catalog, [1, 0, 0, 0], 3.).is_none());
        assert_eq!(pack_attack(&catalog, [1, 0, 0, 2], 6.).unwrap().0, 3);
        assert_eq!(pack_attack(&catalog, [1, 1, 1, 1], 1.).unwrap().0, 0);
        assert!(pack_attack(&catalog, [1; 4], f32::NAN).is_none());
    }

    #[test]
    fn canonical_ability_type_is_checked_and_nonfinite_values_are_not_used() {
        let mut props = crate::prop::PropertyList::default();
        props.0.insert(
            0x01BA8C38,
            crate::prop::Property {
                kind: 9,
                array: false,
                values: vec![30i32.to_be_bytes().to_vec()],
            },
        );
        props.0.insert(
            0x01B8346A,
            crate::prop::Property {
                kind: 13,
                array: false,
                values: vec![f32::NAN.to_be_bytes().to_vec()],
            },
        );
        let source = crate::dbpf::ResourceKey {
            group_id: ABILITY_TEMPLATE_GROUP,
            instance_id: 1,
            type_id: crate::asset_catalog::PROP,
        };
        assert!(authored_ability(&props, source, "SpitTemplate".into(), 32).is_err());
        let ability = authored_ability(&props, source, "BiteTemplate".into(), 30).unwrap();
        assert!(ability.damage.is_none());
    }

    #[test]
    fn missing_native_templates_are_reported_instead_of_silently_omitted() {
        let catalog =
            CreatureAbilityCatalog::from_index(&crate::asset_catalog::ResourceIndex::default());
        assert!(catalog.abilities.is_empty());
        assert_eq!(catalog.errors.len(), NATIVE_ABILITY_TEMPLATES.len());
        assert!(catalog
            .errors
            .iter()
            .all(|e| e.contains("missing native ability template")));
    }

    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_ability_templates_have_expected_keys_types_and_native_values() {
        let install = crate::spore_install::discover().expect("original Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        assert!(
            index.errors.is_empty(),
            "package index errors: {:?}",
            index.errors
        );

        let catalog = CreatureAbilityCatalog::from_index(&index);
        assert!(
            catalog.errors.is_empty(),
            "template errors: {:?}",
            catalog.errors
        );
        assert_eq!(catalog.abilities.len(), 8, "one entry per native template");

        let expected = [
            (EncounterMode::Combat, 0, "BiteTemplate", 30, 1.5, 1.0),
            (EncounterMode::Combat, 1, "ChargeTemplate", 31, 6.0, 1.0),
            (EncounterMode::Combat, 2, "StrikeTemplate", 33, 4.5, 1.0),
            (EncounterMode::Combat, 3, "SpitTemplate", 32, 3.0, 30.0),
            (EncounterMode::Social, 0, "VocalizeTemplate", 37, 5.0, 5.0),
            (EncounterMode::Social, 1, "DanceTemplate", 34, 5.0, 5.0),
            (EncounterMode::Social, 2, "CharmTemplate", 36, 5.0, 5.0),
            (EncounterMode::Social, 3, "PostureTemplate", 35, 5.0, 5.0),
        ];
        for (mode, slot, name, ability_type, refresh, range) in expected {
            let ability = catalog.get(mode, slot, 1).expect(name);
            assert_eq!(ability.name, name);
            assert_eq!(ability.source.group_id, ABILITY_TEMPLATE_GROUP);
            assert_eq!(ability.source.instance_id, crate::game::spore_hash(name));
            assert_eq!(ability.source.type_id, crate::asset_catalog::PROP);
            assert_eq!(ability.ability_type, ability_type);
            assert_eq!(ability.refresh, Some(refresh));
            assert!(ability.recharge.is_none());
            assert_eq!(ability.range, Some(range));
            assert!(
                ability.damage.is_none(),
                "native template has no base damage field"
            );
            assert!(
                ability.ranged_damage.is_none(),
                "native template has no ranged damage field"
            );
            assert!(
                ability.level_multiplier.is_none(),
                "native template has no level multiplier field"
            );

            if mode == EncounterMode::Combat {
                // The per-level props override the templates (charge refresh is
                // 10 s at level 1, not the template's 6): the effective values
                // are the retail tables.
                let effective = combat_ability(&catalog, slot, 1);
                let retail = crate::creature_rules::combat_ability(
                    crate::creature_rules::CombatKind::from_choice(slot).unwrap(),
                    1,
                )
                .unwrap();
                assert!(effective.damage.is_finite() && effective.damage > 0.0);
                assert_eq!(effective.damage, retail.damage);
                assert_eq!(effective.recharge, retail.refresh);
                assert_eq!(effective.range, retail.reach());
            } else {
                assert_eq!(social_refresh(&catalog, slot, 1), refresh);
                assert!(SOCIAL_RESPONSE_WINDOW > refresh);
            }
        }
        let rank_three_spit = combat_ability(&catalog, 3, 3);
        assert_eq!(rank_three_spit.damage, 1.5);
        assert_eq!(rank_three_spit.range, 31.5);
        assert_eq!(rank_three_spit.recharge, 3.0);
    }

    #[test]
    fn persisted_species_outcome_prevents_repeated_social_dna_after_reload() {
        let mut run = RunProgress::default();
        run.species_world = crate::species::SpeciesWorld::generate(
            7,
            &crate::asset_catalog::PartCatalog::default(),
            24.,
        );
        let id = run.species_world.spawn_list()[0].member;
        let mut next = NextState::<GameStage>::default();
        if record_outcome(&mut run, id, true).0 == RecordResult::Recorded {
            award(&mut run, &mut next, 30., true);
        }
        assert_eq!(run.dna_this_stage, 30.);
        let mut reloaded: RunProgress =
            serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        if record_outcome(&mut reloaded, id, true).0 == RecordResult::Recorded {
            award(&mut reloaded, &mut next, 30., true);
        }
        assert_eq!(reloaded.dna_this_stage, 30.);
        assert_eq!(
            record_outcome(&mut reloaded, 99999, true).0,
            RecordResult::UnknownMember
        );
    }

    #[test]
    fn a_proposal_plans_its_rounds_and_its_bar_from_the_requested_levels() {
        let ranks = [3, 2, 0, 1];
        let mut mind = CreatureMind { species: 2, rounds: 3, ..default() };
        propose(&mut mind, ranks);
        assert_eq!(mind.requests.len(), 3);
        assert!(mind.requests.iter().all(|r| ranks[*r as usize] > 0), "only actions it can perform");
        // 0xDA4360: the bar is twice the requested levels.
        let levels: f32 = mind.requests.iter().map(|r| ranks[*r as usize] as f32).sum();
        assert_eq!(mind.bar_max, 2.0 * levels);
        // The first demonstration fills the creature's side with its level.
        assert_eq!(mind.npc_fill, ranks[mind.requests[0] as usize] as f32);
        assert_eq!((mind.round, mind.requested_level), (0, ranks[mind.requests[0] as usize]));
        // Answering every round at the requested level fills it at the last one.
        for round in 0..3 {
            let (level, asked) = (mind.requested_level, mind.requested as usize);
            mind.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
            let allied = ranked_match_response(&mut mind, asked, level, false);
            assert_eq!(allied, round == 2, "round {round}: {} of {}", mind.npc_fill + mind.player_fill, mind.bar_max);
            if !allied {
                assert!(next_round(&mut mind, ranks));
            }
        }
        assert!(mind.allied && mind.impression == 100.0);
        // Level-1 answers to a level-3 request leave the bar short: after the
        // last round the proposal ends (state 8) without a friend.
        let mut weak = CreatureMind { species: 1, rounds: 1, ..default() };
        propose(&mut weak, [3, 0, 0, 0]);
        weak.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
        assert!(!ranked_match_response(&mut weak, 0, 1, false));
        assert!(weak.performing, "a correct answer keeps the step");
        assert!(!next_round(&mut weak, [3, 0, 0, 0]), "the rounds are used up");
        assert!(!weak.performing && !weak.allied);
        assert!((weak.impression - 100.0 * 4.0 / 6.0).abs() < 1e-3, "{}", weak.impression);
        // The same creature proposes again with the same requests and a fresh bar.
        let requests = weak.requests.clone();
        propose(&mut weak, [3, 0, 0, 0]);
        assert_eq!(weak.requests, requests);
        assert_eq!((weak.player_fill, weak.npc_fill), (0.0, 3.0));
        // A creature with a single action keeps asking for it.
        let mut single = CreatureMind { rounds: 4, ..default() };
        propose(&mut single, [0, 0, 4, 0]);
        assert!(single.requests.iter().all(|r| *r == 2));
    }

    #[test]
    fn an_ignored_round_drains_the_creatures_side_after_its_patience() {
        let ranks = [2, 0, 0, 0];
        let mut mind = CreatureMind { rounds: 1, ..default() };
        propose(&mut mind, ranks);
        assert_eq!((mind.npc_fill, mind.bar_max), (2.0, 4.0));
        // Nothing drains during the demonstration.
        social_tick(&mut mind, 1.0, 0.5, 0.1);
        assert!(!mind.drain_armed);
        // The round opens: patience 0.5 s (medium social), then 0.1 bars/s.
        mind.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
        social_tick(&mut mind, 0.1, 0.5, 0.1);
        assert!(mind.drain_armed && !mind.draining);
        for _ in 0..4 {
            social_tick(&mut mind, 0.1, 0.5, 0.1);
        }
        assert!(!mind.draining, "still patient after 0.4 s");
        for _ in 0..3 {
            social_tick(&mut mind, 0.1, 0.5, 0.1);
        }
        assert!(mind.draining, "patience over");
        let before = mind.npc_fill;
        social_tick(&mut mind, 1.0, 0.5, 0.1);
        assert!((before - mind.npc_fill - 0.4).abs() < 1e-4, "0.1 of a 4-power bar per second");
        for _ in 0..10 {
            social_tick(&mut mind, 1.0, 0.5, 0.1);
        }
        assert_eq!(mind.npc_fill, 0.0, "the creature's side empties");
        // An answer stops the drain and counts in full.
        assert!(!ranked_match_response(&mut mind, 0, 2, false));
        assert!(!mind.draining);
        assert_eq!(mind.player_fill, 2.0);
        social_tick(&mut mind, 5.0, 0.5, 0.1);
        assert_eq!(mind.player_fill, 2.0, "the player's side never drains");
    }

    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_ability_props_carry_the_tray_clips() {
        let install = crate::spore_install::discover().expect("original Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = CreatureAbilityCatalog::from_index(&index);
        for social in [false, true] {
            let mode = if social { EncounterMode::Social } else { EncounterMode::Combat };
            for slot in 0..4 {
                for rank in 1..=5u8 {
                    assert_eq!(
                        catalog.slot_clip(mode, slot, rank).map(|c| c.animation),
                        crate::creature_clips::ability_action(social, slot, rank),
                        "{mode:?} slot {slot} rank {rank}"
                    );
                }
            }
        }
        assert_eq!(catalog.clip(41, 0).map(|c| c.animation), Some(0x04BC_04AB), "epicstomp");
        assert_eq!(catalog.clip(16, 0).map(|c| c.animation), Some(0x0248_1E03), "call");
        assert_eq!(catalog.clip(31, 3).and_then(|c| c.rushing), Some(0x04A3_B4FB), "charge run-up");
    }

    #[test]
    fn babies_ask_for_one_round_fewer_and_alphas_for_one_more() {
        use crate::creature_ecology::{social_rounds, Archetype, MemberRole};
        assert_eq!(social_rounds(Archetype::MediumSocial, MemberRole::Adult), 3);
        assert_eq!(social_rounds(Archetype::MediumSocial, MemberRole::Baby), 2);
        assert_eq!(social_rounds(Archetype::MediumSocial, MemberRole::Alpha), 4);
        assert_eq!(social_rounds(Archetype::Social, MemberRole::Baby), 1, "at least one");
    }

    #[test]
    fn an_unanswered_performance_fails_the_step_without_a_penalty() {
        let mut mind = CreatureMind {
            impression: 15.,
            beats: 1,
            response_time: 0.,
            performing: true,
            ..default()
        };
        // Retail 0xDA4CAA: the step fails; the friendship earned stays.
        assert!(ignore_performance(&mut mind));
        assert_eq!(mind.impression, 15.);
        assert_eq!(mind.bored, BORED_SECONDS);
        assert_eq!(mind.beats, 0);
        assert!(!mind.performing);
        // A creature that is never answered at all walks away too.
        let mut ignored = CreatureMind::default();
        assert!(ignore_performance(&mut ignored));
        // Old checkpoints without the new fields still load.
        let legacy: CreatureMind = serde_json::from_str(
            r#"{"species":1,"allied":false,"defeated":false,"impression":20.0,"requested":2,"beats":1,"response_time":3.0,"attack_timer":0.0}"#,
        )
        .unwrap();
        assert_eq!((legacy.downed, legacy.bored), (0., 0.));
        assert_eq!((legacy.stunned, legacy.poisoned, legacy.shove), (0., 0., [0., 0.]));
        assert!(!legacy.performing);
    }

    #[test]
    fn unarmed_creatures_still_struggle_when_attacked() {
        assert_eq!(
            fight_back_ranks([0; 4]),
            ([1, 0, 0, 0], STRUGGLE_DAMAGE_SCALE)
        );
        assert_eq!(fight_back_ranks([0, 2, 0, 0]), ([0, 2, 0, 0], 1.0));
    }

    fn nest_world() -> (RunProgress, crate::Originals) {
        use crate::asset_catalog::{PartCatalog, PartDefinition, CREATURE_PARTS, PROP, RW4};
        let part = |instance: u32, level: u32, abilities: Capabilities| PartDefinition {
            key: crate::dbpf::ResourceKey {
                type_id: PROP,
                group_id: CREATURE_PARTS,
                instance_id: instance,
            },
            model: crate::dbpf::ResourceKey {
                type_id: RW4,
                group_id: CREATURE_PARTS,
                instance_id: instance + 5000,
            },
            name: format!("Part {instance}"),
            is_null: false,
            price: 10,
            unlock_level: level,
            min_scale: 0.5,
            max_scale: 2.0,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities,
        };
        let catalog = PartCatalog {
            parts: vec![
                part(
                    0x10,
                    3,
                    Capabilities {
                        herbivore: 2,
                        mouth: 1,
                        ..default()
                    },
                ),
                part(
                    0x20,
                    2,
                    Capabilities {
                        eye: 2,
                        ..default()
                    },
                ),
                part(
                    0x30,
                    2,
                    Capabilities {
                        foot: 2,
                        ..default()
                    },
                ),
                part(
                    0x40,
                    3,
                    Capabilities {
                        sing: 2,
                        dance: 1,
                        ..default()
                    },
                ),
            ],
            errors: Vec::new(),
        };
        let mut run = RunProgress::default();
        // A legacy (pre-archetype) world: flat nest bonus and nest parts.
        run.species_world = crate::species::SpeciesWorld::generate_n(
            7,
            &catalog,
            24.,
            crate::species::default_species_count(7),
        );
        let mut originals = crate::Originals::default();
        originals.part_catalog = catalog;
        (run, originals)
    }

    /// The same catalog with a retail world (herd archetypes).
    fn retail_world() -> (RunProgress, crate::Originals) {
        let (mut run, originals) = nest_world();
        run.species_world = crate::species::SpeciesWorld::generate(7, &originals.part_catalog, 24.);
        (run, originals)
    }

    fn reward_ctx<'a>(
        run: &'a mut RunProgress,
        next: &'a mut NextState<GameStage>,
        encounter: &'a mut Encounter,
        awards: &'a mut CreatureAwards,
        originals: &'a crate::Originals,
    ) -> Ctx<'a> {
        Ctx {
            run,
            next,
            encounter,
            awards,
            originals,
            player: Capabilities::default(),
            drops: Vec::new(),
            stealth_strike: false,
            player_position: Vec3::ZERO,
            alerts: Vec::new(),
        }
    }

    #[test]
    fn retail_herds_pay_their_archetype_dna_and_alphas_drop_parts() {
        let (mut run, originals) = retail_world();
        let world = run.species_world.clone();
        let species = world
            .species
            .iter()
            .find(|s| s.archetype == Some(crate::creature_ecology::Archetype::Social))
            .unwrap()
            .id;
        let nest = world.nests.iter().position(|n| n.species == species).unwrap();
        let members: Vec<u32> = world.nests[nest].members.iter().map(|m| m.id).collect();
        assert_eq!(world.member_role(members[0]), crate::creature_ecology::MemberRole::Alpha);
        let (mut next, mut encounter, mut awards) =
            (NextState::<GameStage>::default(), Encounter::default(), CreatureAwards::default());
        let strength =
            crate::creature_rules::npc_strength_rating(member_abilities(&run, &originals, members[1]));
        let mult = crate::creature_rules::dna_points_multiplier(0, strength, crate::creature_rules::Difficulty::Medium);
        assert_eq!(mult, 1.0, "brain level 0 pays in full");
        // Befriend three members: 2 adults, then the alpha (x1.25, a part).
        for (i, id) in [members[1], members[2], members[0]].into_iter().enumerate() {
            let before = run.dna_this_stage;
            let name = member_name(&run, id);
            let mut ctx = reward_ctx(&mut run, &mut next, &mut encounter, &mut awards, &originals);
            let (mut hp, mut hostile) = (10.0, false);
            let mut mind = CreatureMind { species: species as u8, allied: true, ..default() };
            let mut visibility = Visibility::Inherited;
            befriend(
                &mut ctx,
                &mut TargetView {
                    id,
                    name,
                    distance: 1.0,
                    position: Vec3::ZERO,
                    hp: &mut hp,
                    hostile: &mut hostile,
                    mind: &mut mind,
                    visibility: &mut visibility,
                    social: [1, 0, 0, 0],
                    roamer: None,
                },
            );
            let paid = run.dna_this_stage - before;
            match i {
                // evoPointsRewardedSocial 10 + half the social bonus 5.
                0 | 1 => assert_eq!(paid, 12.5),
                // The alpha: 10 x 1.25 + 2.5, then best friends (25).
                _ => assert_eq!(paid, 12.5 + 2.5 + 25.0, "{}", encounter.message),
            }
        }
        assert!(run.species_world.species_allied(species));
        assert_eq!(run.creature_found_parts.len(), 1, "the alpha dropped one part");
        assert!(encounter.message.contains("best friends"), "{}", encounter.message);
        assert!(awards.history.iter().any(|a| a.starts_with("Befriended a species")));
        // The rest of the herd is friendly now.
        assert!(run.species_world.member_friendly(members[3]));

        // Kills: an NPC_Combat herd dies out after three kills (+25).
        let combat = world
            .species
            .iter()
            .find(|s| s.archetype == Some(crate::creature_ecology::Archetype::Combat))
            .unwrap()
            .id;
        let fighters: Vec<u32> = world.nests_of_species(combat)[0].members.iter().map(|m| m.id).collect();
        let before = run.dna_this_stage;
        for id in fighters.iter().skip(1).take(3) {
            let name = member_name(&run, *id);
            let mut ctx = reward_ctx(&mut run, &mut next, &mut encounter, &mut awards, &originals);
            let (mut hp, mut hostile) = (0.0, true);
            let mut mind = CreatureMind { species: combat as u8, ..default() };
            let mut visibility = Visibility::Inherited;
            finish_kill(
                &mut ctx,
                &mut TargetView {
                    id: *id,
                    name,
                    distance: 1.0,
                    position: Vec3::ZERO,
                    hp: &mut hp,
                    hostile: &mut hostile,
                    mind: &mut mind,
                    visibility: &mut visibility,
                    social: [0; 4],
                    roamer: None,
                },
            );
        }
        // The brain level may rise on the way; the first kill pays 7 in full.
        assert!(run.species_world.species_extinct(combat));
        assert!(run.dna_this_stage - before >= 7.0 + 3.0 + 1.0);
        assert!(awards.history.iter().any(|a| a.starts_with("Extinction!")));
        assert_eq!(run.creature_found_parts.len(), 1, "no part from non-alphas");
        // A member of a herd hits with its DamageMultiplier (0.75).
        assert_eq!(attacker_multiplier(&run, fighters[0]), 0.75);
    }

    #[test]
    fn befriending_a_whole_nest_allies_it_once_with_dna_and_a_carried_part() {
        let (mut run, originals) = nest_world();
        let index = 0;
        let members: Vec<u32> = run.species_world.nests[index]
            .members
            .iter()
            .map(|m| m.id)
            .collect();
        assert!(members.len() >= 2);
        let mut next = NextState::<GameStage>::default();
        let mut encounter = Encounter::default();
        let mut awards = CreatureAwards::default();
        for (n, id) in members.iter().enumerate() {
            let species = run.species_world.nests[index].species as u8;
            let mut ctx = Ctx {
                run: &mut run,
                next: &mut next,
                encounter: &mut encounter,
                awards: &mut awards,
                originals: &originals,
                player: Capabilities::default(),
                drops: Vec::new(),
                stealth_strike: false,
                player_position: Vec3::ZERO,
                alerts: Vec::new(),
            };
            let (mut hp, mut hostile) = (50.0, false);
            let mut mind = CreatureMind {
                species,
                allied: true,
                ..default()
            };
            let mut visibility = Visibility::Inherited;
            befriend(
                &mut ctx,
                &mut TargetView {
                    id: *id,
                    name: "Test".into(),
                    distance: 1.0,
                    position: Vec3::ZERO,
                    hp: &mut hp,
                    hostile: &mut hostile,
                    mind: &mut mind,
                    visibility: &mut visibility,
                    social: [1, 0, 0, 0],
                    roamer: None,
                },
            );
            let last = n + 1 == members.len();
            assert_eq!(
                run.species_world.nest_status(index) == crate::species::NestStatus::Allied,
                last
            );
            // 0xD9BBD0: every befriend plays a reaction clip.
            use crate::creature_rules as r;
            assert!(
                [
                    r::SOCIAL_CLIP_FEARFUL_TO_CURIOUS,
                    r::SOCIAL_CLIP_ANGRY_TO_CURIOUS,
                    r::SOCIAL_CLIP_CURIOUS_TO_FRIENDLY,
                    r::SOCIAL_CLIP_FRIENDLY_TO_BEST
                ]
                .contains(&mind.cue_clip),
                "{:08X}",
                mind.cue_clip
            );
            assert_eq!(mind.cue_serial, 1);
        }
        // Retail reward: base 100 x the level multiplier. These members'
        // strength (twice the signature rank 2) keeps the multiplier at 1
        // while the player climbs, so every befriend pays 100.
        let expected = crate::creature_rules::DEFAULT_SPECIES_DNA * members.len() as f32
            + NEST_COMPLETE_DNA;
        assert_eq!(run.dna_this_stage, expected);
        assert!(
            encounter.message.contains("nest is allied"),
            "{}",
            encounter.message
        );
        assert!(awards.history.iter().any(|a| a.starts_with("Nest allied")));
        assert_eq!(
            run.creature_found_parts.len(),
            1,
            "one carried part per nest"
        );
        assert!(run
            .species_world
            .allied_nest_near(run.species_world.nests[index].position, 1.0)
            .is_some());
        // The reward is claimed exactly once, also after a reload.
        let mut reloaded: RunProgress =
            serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        assert!(reloaded.species_world.claim_nest(index).is_none());
    }

    #[test]
    fn a_player_hit_uses_the_retail_damage_stealth_and_effects() {
        use crate::creature_rules::CombatEffect;
        let (mut run, originals) = nest_world();
        let mut next = NextState::<GameStage>::default();
        let mut encounter = Encounter {
            mode: EncounterMode::Combat,
            ..default()
        };
        let mut awards = CreatureAwards::default();
        let mut hit = |choice: usize, level: u8, stealth: bool| -> (f32, CreatureMind, f32) {
            let mut ctx = Ctx {
                run: &mut run,
                next: &mut next,
                encounter: &mut encounter,
                awards: &mut awards,
                originals: &originals,
                player: Capabilities {
                    bite: level,
                    charge: level,
                    strike: level,
                    spit: level,
                    ..default()
                },
                drops: Vec::new(),
                stealth_strike: stealth,
                player_position: Vec3::ZERO,
                alerts: Vec::new(),
            };
            ctx.encounter.cooldowns = [0.; 4];
            let (mut hp, mut hostile) = (100.0, false);
            let mut mind = CreatureMind::default();
            let mut visibility = Visibility::Inherited;
            let mut performing = Vec::new();
            drive_target(
                &mut ctx,
                TargetView {
                    id: 301,
                    name: "Target".into(),
                    distance: 1.0,
                    position: Vec3::new(2.0, 0.0, 0.0),
                    hp: &mut hp,
                    hostile: &mut hostile,
                    mind: &mut mind,
                    visibility: &mut visibility,
                    social: [0; 4],
                    roamer: None,
                },
                Some(choice),
                false,
                &[],
                &mut performing,
            );
            (hp, mind, ctx.encounter.cooldowns[choice])
        };
        // Bite level 3 = 2.0 damage, 1.5 s refresh; stealth x1.5.
        let (hp, _, cooldown) = hit(0, 3, false);
        assert_eq!((hp, cooldown), (98.0, 1.5));
        assert_eq!(hit(0, 3, true).0, 97.0);
        // Strike level 5 = 9.0 damage, 4.5 s refresh; stealth x1.5.
        let (hp, _, cooldown) = hit(2, 5, false);
        assert_eq!((hp, cooldown), (91.0, 4.5));
        assert_eq!(hit(2, 5, true).0, 86.5);
        // Spit level 5 = 2.5 damage and 6 s of poison at 0.5833 HP/s; no stealth bonus.
        let (hp, mind, cooldown) = hit(3, 5, true);
        assert_eq!((hp, cooldown), (97.5, 3.0));
        assert_eq!((mind.poisoned, mind.poison_dps), (6.0, 0.5833));
        // A charge needs its run-up: at 1 unit it is refused (0xD1E4B0, the
        // hint `crg-spitclose`), the rush itself is `a_charge_runs_up_rushes_and_stuns`.
        let (hp, mind, cooldown) = hit(1, 2, false);
        assert_eq!((hp, cooldown, mind.stunned), (100.0, 0.0, 0.0));
        assert!(matches!(
            combat_ability(&CreatureAbilityCatalog::default(), 1, 2).effect,
            CombatEffect::Stun { .. }
        ));
        // No part, no hit.
        let (hp, _, _) = hit(0, 0, false);
        assert_eq!(hp, 100.0);
    }

    /// One `drive_target` call on a creature 301 that stands at `position`,
    /// `distance` from the avatar (the closure of the retail hit test, for the
    /// charge's several frames).
    #[allow(clippy::too_many_arguments)]
    fn drive_charge(
        run: &mut RunProgress,
        originals: &crate::Originals,
        encounter: &mut Encounter,
        level: u8,
        distance: f32,
        position: Vec3,
        selected: Option<usize>,
        creature: &mut (f32, bool, CreatureMind),
    ) {
        let mut next = NextState::<GameStage>::default();
        let mut awards = CreatureAwards::default();
        let mut ctx = Ctx {
            run,
            next: &mut next,
            encounter,
            awards: &mut awards,
            originals,
            player: Capabilities { charge: level, ..default() },
            drops: Vec::new(),
            stealth_strike: false,
            player_position: Vec3::ZERO,
            alerts: Vec::new(),
        };
        let mut visibility = Visibility::Inherited;
        let mut performing = Vec::new();
        drive_target(
            &mut ctx,
            TargetView {
                id: 301,
                name: "Target".into(),
                distance,
                position,
                hp: &mut creature.0,
                hostile: &mut creature.1,
                mind: &mut creature.2,
                visibility: &mut visibility,
                social: [0; 4],
                roamer: None,
            },
            selected,
            false,
            &[],
            &mut performing,
        );
    }

    #[test]
    fn a_charge_runs_up_rushes_and_stuns() {
        use crate::creature_rules::{CHARGE_RUSH_RANGE, CHARGE_RUSH_SPEED};
        let (mut run, originals) = nest_world();
        let mut encounter = Encounter { mode: EncounterMode::Combat, target: Some(301), ..default() };
        let mut creature = (100.0, false, CreatureMind::default());
        let at = Vec3::new(12.0, 0.0, 0.0);
        // Level 1 needs 14 units, level 2 needs 10: 12 units is too close for the first.
        drive_charge(&mut run, &originals, &mut encounter, 1, 12.0, at, Some(1), &mut creature);
        assert_eq!(encounter.error, Some(AbilityError::TargetTooClose));
        assert_eq!(encounter.error_serial, 1);
        assert!(encounter.rush.is_none() && encounter.cooldowns[1] == 0.0);
        assert_eq!(encounter.message, "You're too close");
        // Level 2: the rush starts, the refresh (12 s) runs, nothing is hurt yet.
        drive_charge(&mut run, &originals, &mut encounter, 2, 12.0, at, Some(1), &mut creature);
        let rush = encounter.rush.expect("the charge starts a rush");
        assert_eq!((rush.max, rush.speed, rush.gap), (CHARGE_RUSH_RANGE, CHARGE_RUSH_SPEED, 12.0));
        assert_eq!((creature.0, encounter.cooldowns[1]), (100.0, 12.0));
        // The buttons wait while it runs.
        drive_charge(&mut run, &originals, &mut encounter, 2, 12.0, at, Some(0), &mut creature);
        assert_eq!(creature.0, 100.0);
        // 12 units at speed 20 with the hit landing within 2.5: 0.475 s.
        let mut position = Vec3::ZERO;
        let mut frames = 0;
        while !encounter.rush.unwrap().reached {
            let mut rush = encounter.rush.unwrap();
            position = rush_step(&mut rush, position, 1.0 / 60.0);
            encounter.rush = Some(rush);
            frames += 1;
            assert!(frames < 60, "never arrived");
        }
        assert!((28..=29).contains(&frames), "{frames} frames");
        assert!((position.x - 9.5).abs() < 0.4, "stopped at the reach: {position}");
        // The hit: 1.5 damage, a 1.75 s stun and a shove away from the avatar
        // (impulse clamp(1.5 x 0.01, 0.1, 1) x 3 units); the rush is over.
        drive_charge(&mut run, &originals, &mut encounter, 2, 2.5, at, None, &mut creature);
        assert!(encounter.rush.is_none());
        assert_eq!(creature.0, 98.5);
        assert_eq!(creature.2.stunned, 1.75);
        assert!((creature.2.shove[0] - 0.3).abs() < 1e-6 && creature.2.shove[1].abs() < 1e-6, "{:?}", creature.2.shove);
        assert!(creature.1 && creature.2.provoked, "the target turns hostile");
    }

    #[test]
    fn a_rush_misses_when_the_target_runs_and_never_overshoots() {
        // The target keeps its distance: the rushing range (20) runs out.
        let mut rush = Rush::start(7, 3, Vec3::new(30.0, 0.0, 0.0), 30.0, 2.5);
        let mut position = Vec3::ZERO;
        for _ in 0..200 {
            position = rush_step(&mut rush, position, 0.05);
            // The target backs off as fast as the avatar closes in.
            rush.goal.x += 1.0;
            rush.gap += 1.0;
            if rush.missed || rush.reached {
                break;
            }
        }
        assert!(rush.missed && !rush.reached, "{rush:?}");
        assert!((rush.travelled - 20.0).abs() < 1e-3, "the rush covers its range: {}", rush.travelled);
        // Never past the stop distance: a short gap ends in one step, exactly.
        let mut near = Rush::start(7, 3, Vec3::new(4.0, 0.0, 0.0), 4.0, 2.5);
        let at = rush_step(&mut near, Vec3::ZERO, 1.0);
        assert!(near.reached && (at.x - 1.5).abs() < 1e-5, "{at}");
        // A finished rush stands still.
        assert_eq!(rush_step(&mut near, at, 1.0), at);
    }

    #[test]
    fn a_far_target_makes_the_avatar_walk_to_the_launch_point_before_the_attack() {
        use crate::creature_rules::{CHARGE_RUSH_RANGE, CHARGE_RUSH_SPEED};
        let (mut run, originals) = nest_world();
        let mut encounter = Encounter { mode: EncounterMode::Combat, target: Some(301), ..default() };
        let mut creature = (100.0, false, CreatureMind::default());
        let reach = combat_ability(&originals.creature_abilities, 1, 2).range;
        let launch = CHARGE_RUSH_RANGE + reach;
        // The charge on a target beyond `mRushingRange + mRange` is not refused:
        // the attack walks to the launch point first (0xD809D0), nothing starts.
        let far = Vec3::new(launch + 10.0, 0.0, 0.0);
        drive_charge(&mut run, &originals, &mut encounter, 2, launch + 10.0, far, Some(1), &mut creature);
        assert!(encounter.error.is_none() && encounter.rush.is_none() && encounter.cooldowns[1] == 0.0);
        let walking = encounter.launch.expect("the walk to the launch point");
        assert_eq!((walking.target, walking.slot), (301, 1));
        assert!((walking.stop - (launch - LAUNCH_MARGIN)).abs() < 1e-4, "{walking:?}");
        // Each frame re-tests with no key pressed: still too far, the walk goes on.
        let nearer = Vec3::new(launch + 1.0, 0.0, 0.0);
        drive_charge(&mut run, &originals, &mut encounter, 2, launch + 1.0, nearer, None, &mut creature);
        assert_eq!(encounter.launch, Some(walking));
        assert!(encounter.rush.is_none());
        // Within the launch distance the charge starts by itself and the walk is over.
        let there = Vec3::new(launch - 0.2, 0.0, 0.0);
        drive_charge(&mut run, &originals, &mut encounter, 2, launch - 0.2, there, None, &mut creature);
        assert!(encounter.launch.is_none());
        let rush = encounter.rush.expect("the charge starts at the launch point");
        assert_eq!((rush.speed, encounter.cooldowns[1]), (CHARGE_RUSH_SPEED, 12.0));
        // A walk that ends with the target too close is refused like a press (code 2).
        let mut encounter = Encounter { mode: EncounterMode::Combat, target: Some(301), ..default() };
        drive_charge(&mut run, &originals, &mut encounter, 2, launch + 5.0, far, Some(1), &mut creature);
        assert!(encounter.launch.is_some());
        drive_charge(&mut run, &originals, &mut encounter, 2, 3.0, far, None, &mut creature);
        assert!(encounter.launch.is_none());
        assert_eq!(encounter.error, Some(AbilityError::TargetTooClose));
        // Another target (or a changed stance) ends the walk (the update's rule).
        let walking = Launch { target: 301, slot: 1, stop: 1.0 };
        assert_eq!(launch_slot(&Encounter { launch: Some(walking), ..default() }, 301), Some(1));
        assert_eq!(launch_slot(&Encounter { launch: Some(walking), ..default() }, 302), None);
    }

    #[test]
    fn the_ability_errors_carry_the_retail_codes_and_hints() {
        let all = [
            (AbilityError::TargetTooClose, 2, "crg-spitclose", "You're too close"),
            (AbilityError::NoTarget, 3, "crg-notarget", "You don't have a target"),
            (AbilityError::SocialAlreadyFriends, 6, "crg-socialalreadyfriends", ""),
            (AbilityError::NotReady, 8, "crg-abilitynotready", "Ability is cooling down"),
            (AbilityError::WrongFood, 0, "crg-wrongfood", ""),
        ];
        let mut encounter = Encounter::default();
        for (error, code, hint, text) in all {
            assert_eq!((error.code(), error.hint()), (code, hint));
            if !text.is_empty() {
                assert_eq!(error.text(), text);
            }
            let before = encounter.error_serial;
            encounter.fail(error);
            assert_eq!((encounter.error, encounter.error_serial), (Some(error), before + 1));
        }
    }

    #[test]
    fn the_social_round_demonstrates_then_opens_and_a_wrong_answer_ends_the_step() {
        fn view<'a>(
            mind: &'a mut CreatureMind,
            hp: &'a mut f32,
            hostile: &'a mut bool,
            visibility: &'a mut Visibility,
        ) -> TargetView<'a> {
            TargetView {
                id: 301,
                name: "Test".into(),
                distance: 1.0,
                position: Vec3::ZERO,
                hp,
                hostile,
                mind,
                visibility,
                social: [1, 0, 0, 0],
                roamer: None,
            }
        }
        let (mut run, originals) = nest_world();
        let mut next = NextState::<GameStage>::default();
        let mut encounter = Encounter::default();
        let mut awards = CreatureAwards::default();
        let mut ctx = Ctx {
            run: &mut run,
            next: &mut next,
            encounter: &mut encounter,
            awards: &mut awards,
            originals: &originals,
            player: Capabilities {
                sing: 1,
                dance: 1,
                ..default()
            },
            drops: Vec::new(),
            stealth_strike: false,
            player_position: Vec3::ZERO,
            alerts: Vec::new(),
        };
        let (mut hp, mut hostile) = (10.0, false);
        let mut visibility = Visibility::Inherited;
        let mut mind = CreatureMind {
            impression: 20.,
            ..default()
        };
        let mut performing = Vec::new();
        // E: the creature starts a round; it demonstrates before answers count.
        engage(&mut ctx, &mut view(&mut mind, &mut hp, &mut hostile, &mut visibility));
        assert!(mind.performing);
        assert_eq!(mind.response_time, SOCIAL_RESPONSE_WINDOW);
        // Answering during the demonstration changes nothing.
        drive_target(
            &mut ctx,
            view(&mut mind, &mut hp, &mut hostile, &mut visibility),
            Some(1),
            false,
            &[],
            &mut performing,
        );
        assert!(ctx.encounter.message.contains("still showing"), "{}", ctx.encounter.message);
        // A fresh proposal (0xDA4360): one round, bar 2 x level 1; the
        // demonstration fills the creature's half.
        assert_eq!((mind.bar_max, mind.npc_fill, mind.impression), (2.0, 1.0, 50.0));
        // The round opens (15 s of answer time): a wrong answer (dance for a
        // sing request) adds 0.4 x 1 of the 2-power bar, subtracts nothing and
        // ends the step.
        mind.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
        drive_target(
            &mut ctx,
            view(&mut mind, &mut hp, &mut hostile, &mut visibility),
            Some(1),
            false,
            &[],
            &mut performing,
        );
        assert!((mind.impression - 70.).abs() < 1e-3, "{}", mind.impression);
        assert!(!mind.performing && mind.response_time == 0.);
        assert!(ctx.encounter.message.contains("Wrong answer"), "{}", ctx.encounter.message);
        // With no round open an ability key only asks for E.
        ctx.encounter.cooldowns = [0.; 4];
        drive_target(
            &mut ctx,
            view(&mut mind, &mut hp, &mut hostile, &mut visibility),
            Some(0),
            false,
            &[],
            &mut performing,
        );
        assert!(ctx.encounter.message.contains("press E"), "{}", ctx.encounter.message);
        assert!((mind.impression - 70.).abs() < 1e-3, "the friendship stays");
        // E starts a fresh proposal (a fresh bar); left unanswered for 15 s the
        // step fails and the creature loses interest.
        engage(&mut ctx, &mut view(&mut mind, &mut hp, &mut hostile, &mut visibility));
        assert!(mind.performing);
        mind.response_time = 0.0;
        ctx.encounter.target = Some(301);
        drive_target(
            &mut ctx,
            view(&mut mind, &mut hp, &mut hostile, &mut visibility),
            None,
            false,
            &[],
            &mut performing,
        );
        assert!(!mind.performing && mind.bored == BORED_SECONDS);
        assert_eq!(ctx.encounter.target, None);
        assert!(ctx.encounter.message.contains("lost interest"), "{}", ctx.encounter.message);
        assert!((mind.impression - 50.).abs() < 1e-3, "no penalty for the timeout");
    }

    /// Audit with the installed parts: can the retail DNA formula pay the whole
    /// stage total (1000) in every generated world? Plays every nest member and
    /// roamer once (befriend or kill pay the same), nest bonuses included, in
    /// the best order (highest payout first) and the worst (lowest first).
    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_worlds_can_pay_the_whole_stage_total() {
        use crate::creature_rules as rules;
        let install = crate::spore_install::discover().expect("original Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut originals = crate::Originals::default();
        originals.part_catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        // Retail rewards: each kill pays its archetype's evoPointsRewarded
        // (role multipliers included in the base); a herd dies out after its
        // kill count and pays its extinctionReward once.
        let play = |targets: &[(i32, f32, Option<usize>)], nests: &[(usize, f32, i32)], best_first: bool| {
            let mut remaining = targets.to_vec();
            let mut nest_left: Vec<usize> = nests.iter().map(|n| n.0).collect();
            let mut total = 0.0f32;
            while !remaining.is_empty() && total < rules::STAGE_TOTAL_DNA {
                let level = rules::brain_level_from_total(total);
                let pay = |t: &(i32, f32, Option<usize>)| {
                    rules::creature_dna_reward(t.1, false, false, level, t.0, rules::Difficulty::Medium)
                };
                let pick = remaining
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (i, pay(t)))
                    .max_by(|a, b| {
                        if best_first {
                            a.1.total_cmp(&b.1)
                        } else {
                            b.1.total_cmp(&a.1)
                        }
                    })
                    .unwrap()
                    .0;
                let target = remaining.swap_remove(pick);
                total += pay(&target);
                if let Some(n) = target.2 {
                    nest_left[n] = nest_left[n].saturating_sub(1);
                    if nest_left[n] == 0 {
                        let mult = rules::dna_points_multiplier(level as i32, nests[n].2, rules::Difficulty::Medium);
                        total += (mult * nests[n].1).round().max(1.0);
                        remaining.retain(|t| t.2 != Some(n));
                    }
                }
            }
            total
        };
        let (mut best_short, mut worst_short, mut min_best, mut min_worst) =
            (Vec::new(), Vec::new(), f32::MAX, f32::MAX);
        for seed in 1..=200u64 {
            let mut run = RunProgress::default();
            run.species_world =
                crate::species::SpeciesWorld::generate(seed, &originals.part_catalog, 24.);
            let world = run.species_world.clone();
            let mut targets = Vec::new();
            let mut nests = Vec::new();
            for (n, nest) in world.nests.iter().enumerate() {
                let archetype = world.archetype_of(nest.species).expect("retail herd");
                let record = archetype.record();
                let mut strength = 0;
                for member in &nest.members {
                    strength = rules::npc_strength_rating(member_abilities(&run, &originals, member.id));
                    let role = match member.role {
                        crate::creature_ecology::MemberRole::Alpha => rules::DNA_FOR_ALPHAS,
                        crate::creature_ecology::MemberRole::Baby => rules::DNA_FOR_BABIES,
                        crate::creature_ecology::MemberRole::Adult => 1.0,
                    };
                    targets.push((strength, record.evo_kill * role, Some(n)));
                }
                let kills = crate::creature_ecology::kills_to_extinction(archetype).unwrap_or(u32::MAX) as usize;
                nests.push((kills, record.extinction_reward, strength));
            }
            for roamer in &world.roamers {
                let stats = crate::epic_creatures::stats(roamer.kind);
                let record = roamer.kind.archetype().record();
                // The miniboss: 0 (at least 1) plus its extinctionReward 100.
                let bonus = if roamer.kind == crate::species::RoamerKind::Rogue { record.extinction_reward } else { 0.0 };
                targets.push((stats.strength, record.evo_kill.max(1.0) + bonus, None));
            }
            let best = play(&targets, &nests, true);
            let worst = play(&targets, &nests, false);
            min_best = min_best.min(best);
            min_worst = min_worst.min(worst);
            if best < rules::STAGE_TOTAL_DNA {
                best_short.push(seed);
            }
            if worst < rules::STAGE_TOTAL_DNA {
                worst_short.push(seed);
            }
        }
        println!(
            "stage-total audit over 200 worlds: best order min {min_best:.0} (short seeds {best_short:?}), worst order min {min_worst:.0} (short in {} worlds)",
            worst_short.len()
        );
        assert!(
            best_short.is_empty(),
            "worlds that can never pay 1000 DNA: {best_short:?}"
        );
    }

    #[test]
    fn creature_attacks_poison_and_stun_the_avatar_through_the_encounter() {
        use crate::creature_rules::CombatEffect;
        let mut encounter = Encounter::default();
        hurt_avatar_with_effect(
            &mut encounter,
            CombatEffect::Poison {
                seconds: 6.0,
                damage_per_second: 0.25,
            },
        );
        assert_eq!((encounter.poison_seconds, encounter.poison_dps), (6.0, 0.25));
        hurt_avatar_with_effect(&mut encounter, CombatEffect::Stun { seconds: 2.0 });
        // avatarStunTimeRange (3, 4) x avatarStunTimeMultiplier 0.5.
        assert!((1.5..=2.0).contains(&encounter.stun), "{}", encounter.stun);
        hurt_avatar_with_effect(&mut encounter, CombatEffect::None);
        assert_eq!(encounter.poison_seconds, 6.0);
    }

    #[test]
    fn kills_pay_less_as_the_player_outgrows_the_creature() {
        // Species of strength 4 (signature rank 2): full 100 DNA below brain
        // level 2, then the multiplier table (index 2 * brain - 4).
        let (mut run, originals) = nest_world();
        let pay = |run: &mut RunProgress, member: u32| {
            let mut next = NextState::<GameStage>::default();
            let mut encounter = Encounter::default();
            let mut awards = CreatureAwards::default();
            let mut ctx = Ctx {
                run,
                next: &mut next,
                encounter: &mut encounter,
                awards: &mut awards,
                originals: &originals,
                player: Capabilities::default(),
                drops: Vec::new(),
                stealth_strike: false,
                player_position: Vec3::ZERO,
                alerts: Vec::new(),
            };
            let (mut hp, mut hostile) = (0.0, true);
            let mut mind = CreatureMind::default();
            let mut visibility = Visibility::Inherited;
            let before = ctx.run.dna_this_stage;
            finish_kill(
                &mut ctx,
                &mut TargetView {
                    id: member,
                    name: "Test".into(),
                    distance: 1.0,
                    position: Vec3::ONE,
                    hp: &mut hp,
                    hostile: &mut hostile,
                    mind: &mut mind,
                    visibility: &mut visibility,
                    social: [0; 4],
                    roamer: None,
                },
            );
            ctx.run.dna_this_stage - before
        };
        // Pick members from different nests so no nest bonus is paid.
        let members: Vec<u32> = run
            .species_world
            .nests
            .iter()
            .filter_map(|nest| nest.members.first().map(|m| m.id))
            .collect();
        assert!(members.len() >= 3, "need three separate nests");
        assert_eq!(pay(&mut run, members[0]), 100.0, "brain 0");
        run.creature_progression.observe_dna(300.0);
        assert_eq!(run.creature_progression.brain_level, 2);
        assert!((pay(&mut run, members[1]) - 100.0).abs() < 1e-4, "brain 2, index 0");
        run.creature_progression.observe_dna(600.0);
        assert_eq!(run.creature_progression.brain_level, 3);
        assert!((pay(&mut run, members[2]) - 15.0).abs() < 1e-4, "brain 3, index 2");
        // A defeated creature pays once (outcomes are recorded once).
        assert_eq!(pay(&mut run, members[2]), 0.0);
    }

    #[test]
    fn killing_a_whole_nest_destroys_it_and_the_last_nest_ends_the_species() {
        let (mut run, originals) = nest_world();
        let species = run.species_world.nests[0].species;
        let nests: Vec<usize> = (0..run.species_world.nests.len())
            .filter(|&i| run.species_world.nests[i].species == species)
            .collect();
        let mut next = NextState::<GameStage>::default();
        let mut encounter = Encounter::default();
        let mut awards = CreatureAwards::default();
        let mut drops = 0;
        for &index in &nests {
            let members: Vec<u32> = run.species_world.nests[index]
                .members
                .iter()
                .map(|m| m.id)
                .collect();
            for id in members {
                let mut ctx = Ctx {
                    run: &mut run,
                    next: &mut next,
                    encounter: &mut encounter,
                    awards: &mut awards,
                    originals: &originals,
                    player: Capabilities::default(),
                    drops: Vec::new(),
                    stealth_strike: false,
                    player_position: Vec3::ZERO,
                    alerts: Vec::new(),
                };
                let (mut hp, mut hostile) = (0.0, true);
                let mut mind = CreatureMind::default();
                let mut visibility = Visibility::Inherited;
                finish_kill(
                    &mut ctx,
                    &mut TargetView {
                        id,
                        name: "Test".into(),
                        distance: 1.0,
                        position: Vec3::ONE,
                        hp: &mut hp,
                        hostile: &mut hostile,
                        mind: &mut mind,
                        visibility: &mut visibility,
                        social: [0; 4],
                        roamer: None,
                    },
                );
                drops += ctx.drops.len();
                assert!(mind.defeated);
                assert_eq!(visibility, Visibility::Hidden);
            }
            assert_eq!(
                run.species_world.nest_status(index),
                crate::species::NestStatus::Destroyed
            );
        }
        assert!(drops > 0, "kills leave meat");
        assert!(run.species_world.species_extinct(species));
        assert!(awards.history.iter().any(|a| a.starts_with("Extinction!")));
        assert!(awards
            .history
            .iter()
            .any(|a| a.starts_with("Nest destroyed")));
    }

    #[test]
    fn epic_creatures_need_high_ranks_and_reward_once() {
        let (mut run, originals) = nest_world();
        let roamer = run.species_world.roamers[0].clone();
        assert_eq!(roamer.kind, crate::species::RoamerKind::Epic);
        let stats = crate::epic_creatures::stats(roamer.kind);
        let mut next = NextState::<GameStage>::default();
        let mut encounter = Encounter {
            mode: EncounterMode::Social,
            target: Some(roamer.id),
            ..default()
        };
        let mut awards = CreatureAwards::default();
        let social = levels(
            member_abilities(&run, &originals, roamer.id),
            EncounterMode::Social,
        );
        let (mut hp, mut angry) = (stats.max_hp, true);
        let mut mind = CreatureMind::default();
        let mut visibility = Visibility::Inherited;
        let mut ctx = Ctx {
            run: &mut run,
            next: &mut next,
            encounter: &mut encounter,
            awards: &mut awards,
            originals: &originals,
            player: Capabilities {
                sing: 2,
                ..default()
            },
            drops: Vec::new(),
            stealth_strike: false,
            player_position: Vec3::ZERO,
            alerts: Vec::new(),
        };
        // A rank-2 singer cannot even start the performance.
        engage(
            &mut ctx,
            &mut TargetView {
                id: roamer.id,
                name: roamer.name.clone(),
                distance: 1.0,
                position: Vec3::ZERO,
                hp: &mut hp,
                hostile: &mut angry,
                mind: &mut mind,
                visibility: &mut visibility,
                social,
                roamer: Some(stats),
            },
        );
        assert!(
            ctx.encounter.message.contains("Only rank"),
            "{}",
            ctx.encounter.message
        );
        assert_eq!(mind.response_time, 0.);
        // A rank-5 performer starts it, and enough matches befriend it once.
        ctx.player = Capabilities {
            sing: 5,
            dance: 5,
            charm: 5,
            pose: 5,
            ..default()
        };
        for round in 0..9 {
            if mind.allied {
                break;
            }
            ctx.encounter.cooldowns = [0.; 4];
            let choice = mind.requested as usize;
            if round > 0 {
                // The demonstration is over: the round is open.
                mind.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
            }
            let mut performing = Vec::new();
            let target = TargetView {
                id: roamer.id,
                name: roamer.name.clone(),
                distance: 1.0,
                position: Vec3::ZERO,
                hp: &mut hp,
                hostile: &mut angry,
                mind: &mut mind,
                visibility: &mut visibility,
                social,
                roamer: Some(stats),
            };
            if round == 0 {
                let mut target = target;
                engage(&mut ctx, &mut target);
                continue;
            }
            drive_target(&mut ctx, target, Some(choice), false, &[], &mut performing);
        }
        assert!(mind.allied);
        assert!(!angry, "a befriended giant calms down");
        assert_eq!(ctx.run.dna_this_stage, stats.dna);
        assert_eq!(
            ctx.run
                .species_world
                .roamer(roamer.id)
                .unwrap()
                .relationship,
            crate::species::Relationship::Allied
        );
        assert_eq!(
            ctx.run
                .species_world
                .record_roamer(roamer.id, crate::species::Relationship::Allied),
            RecordResult::Already
        );
    }

    #[test]
    fn befriended_rogue_can_be_recruited_into_pack_and_toggled() {
        let mut run = RunProgress::default();
        run.species_world = crate::species::SpeciesWorld::generate(99, &Default::default(), 22.0);
        let rogue = run
            .species_world
            .roamers
            .iter_mut()
            .find(|r| r.kind == RoamerKind::Rogue)
            .unwrap();
        rogue.relationship = Relationship::Allied;
        let rogue_id = rogue.id;

        let valid = vec![rogue_id];
        let dna = 600.0;
        assert_eq!(crate::creature_pack::toggle(&mut run.creature_pack, dna, rogue_id, &valid), Ok(true));
        assert!(run.creature_pack.members.contains(&rogue_id));
        assert_eq!(run.creature_pack.members.len(), 1);

        assert_eq!(crate::creature_pack::toggle(&mut run.creature_pack, dna, rogue_id, &valid), Ok(false));
        assert!(!run.creature_pack.members.contains(&rogue_id));
    }
}
