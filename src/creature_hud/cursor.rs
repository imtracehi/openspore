//! The Creature context cursors: what the mouse pointer looks like over the
//! world of the Creature stage (helper `creature-hud-audit`, round 2).
//!
//! Tags: **VERIFIED** (code image, march2017 / SDK addresses, disassembled with
//! `swarm-volume\target\re\dis.py`), **INFERRED** (reading of the neighbouring
//! code), **ADAPTED** (OpenSpore choice).
//!
//! Retail: the Creature display strategy (`cCreatureDisplayStrategy`) owns the
//! cursor. Its start-up (0xD366C0) registers nineteen cursors with
//! `cCursorManager::Load(id, name, true, 0, 0)` (files of group `cursors~`
//! 0x02393C07, resource type `cur`; the Tribal port in `tribal_hud::cursor`
//! loads the same kind of file): `cursor-target` 0x514AB7B0, `cursor-mate`
//! 0x314AB7B1, `cursor-heal` 0xF14AB7B3, `cursor-eat-fruit` 0xB14AB7B6,
//! `cursor-eat-carcass` 0x514AB7B7, `cursor_atk` 0xB14AB7B9, `cursor-evolve`
//! 0x114AB7BA, `cursor-validate` 0x514ABE0B, `cursor_social` 0x71664D85,
//! `cursor_friend` 0x755C4EB5, `cursor_socialNo` 0xF4F6C345,
//! `cursor-crg-addposse` 0x03B360D8, `cursor-crg-pickup` 0x03B360DC,
//! `cursor-crg-putdown` 0x03B360DF, `cur_ClaimNest` 0x7542CDB5,
//! `cursor-crg-give` 0x03B360E2, `cursor-crg-throw` 0x03B360E4,
//! `cursor-target-creature` 0xF36A0C84, `cursor_skeleton2` 0x342D221F.
//!
//! The per-frame update 0xD311B0 asks `0xD30830` (the Creature's
//! `ChooseInputActionID`) for an action id of the hovered object (the object
//! comes from `cGameViewManager` vtbl +0x30, cast to `cGameData`), switches over
//! the id to a cursor id (the table in [`cursor_for`], read entry by entry) and
//! sets it with `cCursorManager::SetActiveCursor` when it differs from the active
//! one. The click side (0xD32FD0) switches over the same id (the ids 6..=0x1C):
//! id 7 sets the creature target (`cCreatureBase` vtbl +0x84), ids 8 / 9 / 0xA /
//! 0xF / 0x10 / 0x11 post an intention word to the avatar (0xC032F0: 0x40000,
//! 8, 0x40000000, 0x10000, 0x10000, 0x10 ...), so the click does what the cursor
//! shows. The same function also moves the target-rollover effect
//! (0x5EBFD37C / 0x8E37991D on the hovered object, re-created 0.1 s after the
//! object or the id changed); that highlight is not drawn here (open gap).
//!
//! `0xD30830` in the order of the code (VERIFIED): the mode word of 0xB3D5D0 is
//! 1 or 2 -> 1; `cSimulatorSystem` 0xB5CB70 is 1 -> 0; the input mode of the
//! `cGameInputManager` is 2 / 3 (the stance ability modes: 3 / 2 and 5 / 4, the
//! ban and spyglass cursors, never active in the Creature stage); the game
//! mode's flag bit 4 (0xD51610 +0x6C), the terrain cursor locked (+0x314 /
//! +0x315) or the avatar dead (+0xB5E) -> 0; else `0xD2FFD0`: the rules by the
//! noun of the object (`GameNounIDs`), mbDead = `+0xB5E`, `mGeneralFlags` =
//! `+0xB58`, the avatar from `cGameNounManager::GetAvatar` (0xB1FD00):
//!
//! ```text
//! kCreatureAnimal 0x18EB45E
//!   the avatar itself: with a carried object a rock / interactive ornament -> 6, else 0
//!   dead (+0xB5E): same species 0, mFoodValue (+0xB84) <= 0 -> 0, else 8 (eat carcass)
//!   alive: flag 0x10 -> 9 (mate); not the avatar's target (+0xE7C) -> 7 (the target ring);
//!          flag 0x100 -> 0xA (social);
//!          social stance (cCreatureGameData::GetAbilityMode == 0): the relationship
//!             (0xBA40E0, creature_rules::relationship) is 5 (hostile) and the creature has
//!             no behaviour entry (2,1,0,0) (0xBCA230) -> 0xE (the refused-social bubble);
//!             `funcD4h` (vtbl +0xD4: best friends (relationship 6), flag 0x80 or the
//!             player's default species) and 0xC0E800 == 0 -> 0x10 (friend), else 0xF
//!          attack stance: same species -> 0, else 0x11 (attack)
//! kFruit 0x2C9CC91    edible flag (+0xA4) and state (+0x16C) not 4 -> 0x17 (eat fruit)
//! kEgg 0x2A034CD      an egg of another species' nest in the attack stance -> 0x15 (attack)
//! kNest 0x52AA6122    a nest of the player's species that is not yet claimed -> 0x14
//!                     (OpenSpore: the destination nest of a pending migration)
//! carried-object ids (AllowCreaturePickup, adventures): 0xB 0xC 0xD 0x13 0x16 0x18 0x19 0x1A
//! ```
//!
//! What this port does with it: [`choose_action`] is the table above for the
//! objects OpenSpore has (creatures, meat piles, fruit bushes, the ground);
//! [`cursor_for`] is the whole of the cursor switch. INFERRED: the meaning of
//! the general flags (0x10 = the mate, 0x100 = a member of the pack, 0x80 =
//! charmed / befriended) from their setters (0xD52A18 creates the mate creature,
//! 0xB2343C / 0xD53773 add pack members, 0xC0C3B0 reads 0x80). ADAPTED: which
//! OpenSpore object stands for which retail noun (a [`Meat`] pile for a dead
//! creature, a [`Bush`] for a fruit, an allied creature for 0x80), the picking
//! (the pointer ray on the creature bodies of `creature_controls`, the ground
//! point on piles, bushes and the claimable nest), the missing nouns (no mate
//! creature objects exist: the cursor 9 only shows over the answering mate), the
//! egg (`creature_eggs::NestEgg`, picked by its ground radius like the piles;
//! the state test of kEgg is `creature_eggs::attackable`, the relationship test
//! the species of the egg's nest) and the behaviour entry (2,1,0,0), whose
//! stand-in is "a social step is under way".

use bevy::{
    asset::RenderAssetUsages,
    ecs::system::SystemParam,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow},
};
use std::collections::HashMap;

use super::CreatureHudActive;
use crate::{
    creature_actions::{CreatureMind, Encounter, EncounterMode},
    creature_controls::{over_hud_margin, pick_creature},
    creature_nestmates::{answering_mate, mate_id, NestMate},
    creature_rules::relationship,
    creature_social::Meat,
    epic_creatures::RoamerActor,
    game::{dist_xz, Bush, Health, Npc, Player, RunProgress},
    persistence::WorldId,
    stages::GameStage,
    tribal_hud::cursor::{cursor_key, decode_cursor, Cursor},
};

/// The `cursors~` file of every cursor the Creature start-up (0xD366C0)
/// registers, in registration order.
pub const REGISTERED: [&str; 19] = [
    "cursor-target",
    "cursor-mate",
    "cursor-heal",
    "cursor-eat-fruit",
    "cursor-eat-carcass",
    "cursor_atk",
    "cursor-evolve",
    "cursor-validate",
    "cursor_social",
    "cursor_friend",
    "cursor_socialNo",
    "cursor-crg-addposse",
    "cursor-crg-pickup",
    "cursor-crg-putdown",
    "cur_ClaimNest",
    "cursor-crg-give",
    "cursor-crg-throw",
    "cursor-target-creature",
    "cursor_skeleton2",
];

/// The results of `0xD30830` (VERIFIED); the names are ours, from what the
/// cursor of the id shows.
#[allow(dead_code)]
pub mod action {
    /// No action: the default arrow (also what the retail code answers while the
    /// game is paused, 0, and its "nothing here" id 0x1C).
    pub const NONE: i32 = 0;
    /// The mode word is 1 or 2 (a cinematic): the default arrow.
    pub const MODE: i32 = 1;
    /// Stance ability modes (the Tribal powers): never active in the Creature stage.
    pub const MODE_ATTACK_TARGET: i32 = 2;
    pub const MODE_ATTACK_VALID: i32 = 3;
    pub const MODE_SOCIAL_TARGET: i32 = 4;
    pub const MODE_SOCIAL_VALID: i32 = 5;
    /// The avatar holding a rock / interactive ornament (no cursor of its own).
    pub const SELF_HOLDING: i32 = 6;
    /// A creature that is not the avatar's target: the targeting ring.
    pub const TARGET: i32 = 7;
    /// A dead creature with food on it.
    pub const EAT_CARCASS: i32 = 8;
    /// The mate.
    pub const MATE: i32 = 9;
    /// A pack member (the targeted one), whatever the stance.
    pub const SOCIAL_PACK: i32 = 0xA;
    /// Carried-object actions (adventures): pick up, give, throw, put down.
    pub const PICKUP: i32 = 0xB;
    pub const GIVE: i32 = 0xC;
    pub const THROW: i32 = 0xD;
    /// Social stance, a hostile creature: the refused-social bubble.
    pub const SOCIAL_NO: i32 = 0xE;
    /// Social stance: the speech bubble.
    pub const SOCIAL: i32 = 0xF;
    /// Social stance, a friend: the smiley.
    pub const FRIEND: i32 = 0x10;
    /// Attack stance: the red crosshair.
    pub const ATTACK: i32 = 0x11;
    pub const ADVENTURE: i32 = 0x12;
    pub const PUTDOWN: i32 = 0x13;
    /// A nest of the player's species that can be claimed.
    pub const CLAIM_NEST: i32 = 0x14;
    /// An egg in the attack stance.
    pub const ATTACK_EGG: i32 = 0x15;
    pub const THROW_FOOD: i32 = 0x16;
    /// A fruit.
    pub const EAT_FRUIT: i32 = 0x17;
    pub const PICKUP_ORNAMENT: i32 = 0x18;
    pub const PICKUP_CREATURE: i32 = 0x19;
    pub const SKELETON: i32 = 0x1A;
    pub const UFO: i32 = 0x1B;
    /// 0xD2FFD0 answered nothing.
    pub const NOTHING: i32 = 0x1C;
}

/// The cursor for an action id: the switch of 0xD311B0 read entry by entry
/// (VERIFIED). The ids without a case (0, 1, 6, 0x1B, 0x1C ...) keep
/// `cursor-default`; 0x12 loads one of two adventure cursors (not installed under
/// a name here: the default arrow).
pub fn cursor_for(action: i32) -> Cursor {
    use action::*;
    match action {
        MODE_ATTACK_TARGET | MODE_SOCIAL_TARGET => Cursor::File("cursor-ban-mode"),
        MODE_ATTACK_VALID => Cursor::File("cursor-ban-content"),
        MODE_SOCIAL_VALID => Cursor::File("cursor_spyglass"),
        TARGET => Cursor::File("cursor-target-creature"),
        EAT_CARCASS => Cursor::File("cursor-eat-carcass"),
        MATE => Cursor::File("cursor-mate"),
        SOCIAL_PACK | SOCIAL => Cursor::File("cursor_social"),
        PICKUP | PICKUP_CREATURE => Cursor::File("cursor-crg-pickup"),
        GIVE => Cursor::File("cursor-crg-give"),
        THROW | THROW_FOOD => Cursor::File("cursor-crg-throw"),
        SOCIAL_NO => Cursor::File("cursor_socialNo"),
        FRIEND => Cursor::File("cursor_friend"),
        ATTACK | ATTACK_EGG => Cursor::File("cursor_atk"),
        PUTDOWN | PICKUP_ORNAMENT => Cursor::File("cursor-crg-putdown"),
        CLAIM_NEST => Cursor::File("cur_ClaimNest"),
        EAT_FRUIT => Cursor::File("cursor-eat-fruit"),
        SKELETON => Cursor::File("cursor_skeleton2"),
        _ => Cursor::Default,
    }
}

/// What the retail rules read of a living creature that is not the avatar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreatureView {
    /// It is the avatar's target (`+0xE7C` of the avatar, 0xC0EFC0).
    pub targeted: bool,
    /// Flag 0x100 of its general flags (INFERRED: a pack member).
    pub pack_member: bool,
    /// Flag 0x10 (INFERRED: the mate).
    pub mate: bool,
    /// The species' relationship to the player (0xBA40E0, `creature_rules::relationship`).
    pub relationship: u8,
    /// `funcD4h` (0xC0C3B0): best friends, charmed (flag 0x80) or the default species.
    pub friend: bool,
    /// A social step is under way (ADAPTED stand-in for the behaviour entry
    /// (2,1,0,0) of 0xBCA230).
    pub performing: bool,
    /// Of the avatar's own species (`+0xB20` equal): never attacked.
    pub same_species: bool,
}

impl Default for CreatureView {
    fn default() -> Self {
        Self {
            targeted: false,
            pack_member: false,
            mate: false,
            relationship: relationship::CURIOUS,
            friend: false,
            performing: false,
            same_species: false,
        }
    }
}

/// What the pointer is over, in the retail nouns of `0xD2FFD0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Open ground, or nothing the rules know.
    Ground,
    /// The avatar itself.
    Avatar,
    /// A living creature.
    Creature(CreatureView),
    /// A dead creature (OpenSpore: a meat pile) with its food value.
    Carcass { food: f32 },
    /// A fruit (OpenSpore: a bush that still has food).
    Fruit { edible: bool },
    /// A nest of the player's species that is not yet claimed (OpenSpore: the
    /// destination nest of a pending migration, `creature_home`).
    Nest,
    /// An egg of another species' nest (`cEgg`, noun kEgg 0x2A034CD): its crack
    /// state (+0x1E8) and whether the species of its nest is a best friend of the
    /// avatar (relationship 6).
    Egg { state: u8, friendly: bool },
}

/// `0xD2FFD0` for the objects OpenSpore has: rules in the order of the code
/// (see the module docs). `social` is `GetAbilityMode() == Social`.
pub fn choose_action(target: Target, social: bool) -> i32 {
    use action::*;
    match target {
        Target::Ground | Target::Avatar => NONE,
        Target::Creature(c) => {
            if c.mate {
                MATE
            } else if !c.targeted {
                TARGET
            } else if c.pack_member {
                SOCIAL_PACK
            } else if social {
                if c.relationship == relationship::HOSTILE && !c.performing {
                    SOCIAL_NO
                } else if c.friend {
                    FRIEND
                } else {
                    SOCIAL
                }
            } else if c.same_species {
                NONE
            } else {
                ATTACK
            }
        }
        Target::Carcass { food } => {
            if food > 0.0 {
                EAT_CARCASS
            } else {
                NONE
            }
        }
        Target::Fruit { edible } => {
            if edible {
                EAT_FRUIT
            } else {
                NONE
            }
        }
        // kNest: a nest of the player's species that is not yet claimed.
        Target::Nest => CLAIM_NEST,
        // kEgg: the attack stance, a state other than 3 / 4 / 5 and a species that
        // is not a best friend (the species of the nest is never the avatar's
        // own here: the avatar's species has no egg objects).
        Target::Egg { state, friendly } => {
            if !social && !friendly && crate::creature_eggs::attackable(state) {
                ATTACK_EGG
            } else {
                NONE
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The world side
// ---------------------------------------------------------------------------

/// A creature body the pointer ray can hit.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub id: u32,
    pub centre: Vec3,
    pub radius: f32,
    pub view: CreatureView,
}

/// A pile of meat or a bush: its centre and how far the pointer reaches it.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub at: Vec3,
    pub reach: f32,
    pub amount: f32,
}

/// A nest egg: its centre on the ground, how far the pointer reaches it and
/// what the kEgg rule reads of it.
#[derive(Clone, Copy, Debug)]
pub struct EggSpot {
    pub entity: Entity,
    pub at: Vec3,
    pub reach: f32,
    pub state: u8,
    pub friendly: bool,
}

/// What the pointer is over and what the retail rules make of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hover {
    pub action: i32,
    pub target: Target,
    /// The creature under the pointer (`WorldId`).
    pub creature: Option<u32>,
    /// The egg under the pointer.
    pub egg: Option<Entity>,
}

/// The `0xD30830` answer for the pointer ray (`origin`, `direction`) and the
/// ground point under it (the one place the cursor reads, so tests and the
/// system agree). Creature bodies win over piles and bushes on the same ground.
pub fn hover_at(
    origin: Vec3,
    direction: Vec3,
    ground: Option<Vec3>,
    bodies: &[Body],
    meat: &[Spot],
    bushes: &[Spot],
    nests: &[Spot],
    eggs: &[EggSpot],
    social: bool,
) -> Hover {
    let hit = |target, creature| Hover { action: choose_action(target, social), target, creature, egg: None };
    let picked = pick_creature(origin, direction, bodies.iter().map(|b| (b.id, b.centre, b.radius)));
    if let Some((id, _)) = picked {
        if let Some(body) = bodies.iter().find(|b| b.id == id) {
            return hit(Target::Creature(body.view), Some(id));
        }
    }
    let nearest = |spots: &[Spot], p: Vec3| {
        spots
            .iter()
            .filter(|s| dist_xz(p, s.at) <= s.reach)
            .min_by(|a, b| dist_xz(p, a.at).total_cmp(&dist_xz(p, b.at)))
            .copied()
    };
    if let Some(p) = ground {
        // The eggs lie inside their nest: the nearest one under the pointer first.
        if let Some(egg) = eggs
            .iter()
            .filter(|e| dist_xz(p, e.at) <= e.reach)
            .min_by(|a, b| dist_xz(p, a.at).total_cmp(&dist_xz(p, b.at)))
        {
            let target = Target::Egg { state: egg.state, friendly: egg.friendly };
            return Hover { egg: Some(egg.entity), ..hit(target, None) };
        }
        if let Some(pile) = nearest(meat, p) {
            return hit(Target::Carcass { food: pile.amount }, None);
        }
        if let Some(bush) = nearest(bushes, p) {
            return hit(Target::Fruit { edible: bush.amount > 0.0 }, None);
        }
        if nearest(nests, p).is_some() {
            return hit(Target::Nest, None);
        }
    }
    hit(Target::Ground, None)
}

/// The action under the pointer (read by captures and tests).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct HoverAction {
    pub action: i32,
    pub creature: Option<u32>,
    /// The egg under the pointer (action 0x15).
    pub egg: Option<Entity>,
    /// Where the food under the pointer lies (actions 8 and 0x17), so a click
    /// can walk there.
    pub food: Option<Vec3>,
}

/// The decoded cursors by file name (`None` = the file could not be read: the
/// default arrow stays).
#[derive(Default)]
pub struct CursorCache {
    images: HashMap<&'static str, Option<(Handle<Image>, (u16, u16))>>,
}

/// Pointer over a pile of meat: how far from its centre it still counts
/// (ADAPTED: the pile's body, a little more than the eating reach of
/// `creature_social::meat`, 1.8).
const MEAT_PICK: f32 = 2.2;
/// Pointer over a bush (ADAPTED: the grazing reach of `creature_graze`).
const BUSH_PICK: f32 = 1.6;
/// Pointer over the nest to claim: the nest's `modelBoundingRadius` (6.5 for
/// the level-0 nest model). ADAPTED: the retail pick shape was not traced.
const NEST_PICK: f32 = 6.5;
/// The hit-test radius of a creature body per unit of scale (the one the click
/// uses, `creature_controls`).
const PICK_RADIUS: f32 = 0.9;

/// The objects of the stage the pointer can be over.
#[derive(SystemParam)]
pub struct Objects<'w, 's> {
    npcs: Query<
        'w,
        's,
        (&'static WorldId, &'static Transform, &'static Npc, &'static CreatureMind),
        (Without<Player>, Without<RoamerActor>),
    >,
    roamers: Query<
        'w,
        's,
        (&'static WorldId, &'static Transform, &'static RoamerActor, &'static Health, &'static CreatureMind),
        (Without<Player>, Without<Npc>),
    >,
    mates: Query<'w, 's, (&'static NestMate, &'static Transform)>,
    eggs: Query<'w, 's, (Entity, &'static Transform, &'static crate::creature_eggs::NestEgg)>,
    meat: Query<'w, 's, (&'static Transform, &'static Meat)>,
    bushes: Query<'w, 's, (&'static Transform, &'static Bush), (Without<Npc>, Without<Player>)>,
}

/// Capture helper: log where the targeted creature, the piles and the bushes
/// are on screen (only while `--spui-pointer` pins the pointer).
fn probe(camera: &Camera, at: &GlobalTransform, pointer: Vec2, ground: Option<Vec3>, bodies: &[Body], meat: &[Spot], bushes: &[Spot]) {
    for b in bodies.iter().filter(|b| b.view.targeted) {
        if let Ok(p) = camera.world_to_viewport(at, b.centre) {
            info!("creature hover: body {} at screen ({:.0}, {:.0}) radius {:.1}", b.id, p.x, p.y, b.radius);
        }
    }
    for (what, spots) in [("meat", meat), ("bush", bushes)] {
        let shown = spots
            .iter()
            .filter_map(|s| camera.world_to_viewport(at, s.at).ok().map(|p| (s, p)))
            .filter(|(_, p)| p.x > 0.0 && p.y > 0.0 && p.x < 1280.0 && p.y < 800.0)
            .take(3);
        for (spot, p) in shown {
            info!("creature hover: {what} {:.1} at screen ({:.0}, {:.0})", spot.amount, p.x, p.y);
        }
    }
    info!("creature hover: pointer ({:.0}, {:.0}), ground {:?}, {} bodies", pointer.x, pointer.y, ground, bodies.len());
}

/// Set the window cursor from the object under the pointer. Runs every frame in
/// the Creature stage; the pointer over the HUD, no pointer or a missing retail
/// file leaves the default arrow.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    spui: Option<Res<crate::spui::view::SpuiPointer>>,
    active: Res<CreatureHudActive>,
    hud: Res<super::CreatureHudState>,
    run: Res<RunProgress>,
    encounter: Res<Encounter>,
    objects: Objects,
    originals: Res<crate::Originals>,
    mut images: ResMut<Assets<Image>>,
    mut hover: ResMut<HoverAction>,
    mut cache: Local<CursorCache>,
    mut shown: Local<Option<Cursor>>,
    mut logged: Local<u32>,
) {
    let mut found = Hover { action: action::NONE, target: Target::Ground, creature: None, egg: None };
    let mut food: Option<Vec3> = None;
    'pick: {
        let window = windows.iter().next().map(|(_, window)| window);
        // Offscreen captures have no window pointer: the SPUI pointer override
        // stands in (`--spui-pointer x,y`).
        let Some(pointer) = window
            .and_then(|window| window.cursor_position())
            .or_else(|| spui.as_deref().and_then(|s| s.position_override.or(s.position)))
        else {
            break 'pick;
        };
        if !active.0 || hud.paused || spui.as_deref().is_some_and(|s| s.over_ui) {
            break 'pick;
        }
        let Ok((camera, camera_transform)) = cameras.single() else { break 'pick };
        let size = window
            .map(|window| Vec2::new(window.width(), window.height()))
            .or_else(|| camera.logical_viewport_size());
        if size.is_some_and(|size| over_hud_margin(pointer, size)) {
            break 'pick;
        }
        let Ok(ray) = camera.viewport_to_world(camera_transform, pointer) else { break 'pick };
        let ground = crate::civ_rts::ground_hit(ray.origin, *ray.direction, surface.as_deref());
        let target = encounter.target;
        let mut bodies: Vec<Body> = Vec::new();
        for (id, t, npc, mind) in &objects.npcs {
            if npc.hp <= 0.0 || mind.defeated || mind.downed > 0.0 {
                continue;
            }
            let species = mind.species as u32;
            let relation = crate::creature_actions::avatar_relationship(&run, id.0, species, false);
            bodies.push(Body {
                id: id.0,
                centre: t.translation,
                radius: PICK_RADIUS * t.scale.x.max(0.8),
                view: CreatureView {
                    targeted: target == Some(id.0),
                    pack_member: run.creature_pack.members.contains(&id.0),
                    mate: false,
                    relationship: relation,
                    friend: mind.allied || relation == relationship::BEST_FRIENDS,
                    performing: mind.performing || mind.response_time > 0.0,
                    same_species: false,
                },
            });
        }
        for (id, t, actor, health, mind) in &objects.roamers {
            if health.hp <= 0.0 || mind.defeated {
                continue;
            }
            let relation = crate::creature_actions::avatar_relationship(&run, id.0, 0, true);
            bodies.push(Body {
                id: id.0,
                centre: t.translation,
                radius: crate::epic_creatures::stats(actor.kind).body_radius,
                view: CreatureView {
                    targeted: target == Some(id.0),
                    pack_member: run.creature_pack.members.contains(&id.0),
                    mate: false,
                    relationship: if actor.aggressive { relationship::HOSTILE } else { relation },
                    friend: mind.allied || relation == relationship::BEST_FRIENDS,
                    performing: mind.performing || mind.response_time > 0.0,
                    same_species: false,
                },
            });
        }
        // The mate that answers a mating call (ADAPTED: the first living adult
        // nest mate, `answering_mate`) is the one creature with flag 0x10: its
        // cursor is the mate cursor, targeted or not (0xD2FFD0 tests it first).
        if run.creature_lifecycle.mating_call_active {
            if let Some(index) = answering_mate(objects.mates.iter().map(|(mate, _)| mate)) {
                if let Some((_, t)) = objects.mates.iter().find(|(mate, _)| mate.index == index) {
                    bodies.push(Body {
                        id: mate_id(index),
                        centre: t.translation,
                        radius: PICK_RADIUS * t.scale.x.max(0.8),
                        view: CreatureView {
                            mate: true,
                            relationship: relationship::BEST_FRIENDS,
                            friend: true,
                            same_species: true,
                            ..default()
                        },
                    });
                }
            }
        }
        let piles: Vec<Spot> =
            objects.meat.iter().map(|(t, m)| Spot { at: t.translation, reach: MEAT_PICK, amount: m.food }).collect();
        let patches: Vec<Spot> = objects
            .bushes
            .iter()
            .map(|(t, b)| Spot { at: t.translation, reach: BUSH_PICK, amount: b.food_left })
            .collect();
        // The nest the species is moving to is the one nest of the player's
        // species that is not yet claimed (0xD41510 claims it from 50 units).
        let claimable: Vec<Spot> = run
            .creature_progression
            .pending_migration
            .map(|m| Spot { at: Vec3::from_array(m.destination), reach: NEST_PICK, amount: 1.0 })
            .into_iter()
            .collect();
        if spui.as_deref().is_some_and(|s| s.position_override.is_some()) && encounter.target.is_some() && {
            *logged += 1;
            *logged % 30 == 0
        } {
            // Captures pin the pointer (`--spui-pointer`): where the targeted
            // creature, the piles and the bushes are on screen, to aim it.
            probe(camera, camera_transform, pointer, ground, &bodies, &piles, &patches);
        }
        // The nest eggs (`creature_eggs`): a best friend's eggs are left alone.
        let egg_spots: Vec<EggSpot> = objects
            .eggs
            .iter()
            .map(|(entity, t, egg)| {
                let species = run.species_world.nests.get(egg.nest).map_or(0, |n| n.species as u32);
                EggSpot {
                    entity,
                    at: t.translation,
                    reach: crate::creature_eggs::PICK_RADIUS,
                    state: egg.state,
                    friendly: crate::creature_actions::avatar_relationship(&run, egg.member, species, false)
                        == relationship::BEST_FRIENDS,
                }
            })
            .collect();
        found = hover_at(
            ray.origin,
            *ray.direction,
            ground,
            &bodies,
            &piles,
            &patches,
            &claimable,
            &egg_spots,
            encounter.mode == EncounterMode::Social,
        );
        // The food under the pointer, for the click (0xD33150 / 0xD335AA walk to it).
        if let (Some(p), true) = (ground, matches!(found.action, action::EAT_CARCASS | action::EAT_FRUIT)) {
            let spots = if found.action == action::EAT_CARCASS { &piles } else { &patches };
            food = spots
                .iter()
                .filter(|s| dist_xz(p, s.at) <= s.reach)
                .min_by(|a, b| dist_xz(p, a.at).total_cmp(&dist_xz(p, b.at)))
                .map(|s| s.at);
        }
    }
    if hover.action != found.action || hover.creature != found.creature || hover.egg != found.egg || hover.food != food {
        if spui.as_deref().is_some_and(|s| s.position_override.is_some()) {
            // Captures pin the pointer (`--spui-pointer`): say what it found.
            info!("creature hover: action {:#x}, {:?}", found.action, found.target);
        }
        hover.action = found.action;
        hover.creature = found.creature;
        hover.egg = found.egg;
        hover.food = food;
    }
    let want = cursor_for(found.action);
    if *shown == Some(want) {
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
    if matches!(want, Cursor::File(_)) {
        debug!("creature cursor: action {:#x} -> {:?}", found.action, want);
    }
    *shown = Some(want);
}

/// Back to the default arrow on leaving the Creature stage.
pub fn reset(mut commands: Commands, windows: Query<Entity, With<PrimaryWindow>>, mut hover: ResMut<HoverAction>) {
    *hover = HoverAction::default();
    for window in &windows {
        commands.entity(window).insert(CursorIcon::default());
    }
}

pub struct CreatureCursorPlugin;

impl Plugin for CreatureCursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverAction>()
            .add_systems(OnExit(GameStage::Creature), reset)
            .add_systems(
                Update,
                update
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(resource_exists::<RunProgress>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action::*;

    fn view() -> CreatureView {
        CreatureView { targeted: true, ..CreatureView::default() }
    }

    #[test]
    fn the_cursor_table_follows_the_switch_at_0xd311b0() {
        for id in [NONE, MODE, SELF_HOLDING, ADVENTURE, UFO, NOTHING] {
            assert_eq!(cursor_for(id), Cursor::Default, "{id:#x}");
        }
        assert_eq!(cursor_for(2), Cursor::File("cursor-ban-mode"));
        assert_eq!(cursor_for(4), Cursor::File("cursor-ban-mode"));
        assert_eq!(cursor_for(3), Cursor::File("cursor-ban-content"));
        assert_eq!(cursor_for(5), Cursor::File("cursor_spyglass"));
        assert_eq!(cursor_for(TARGET), Cursor::File("cursor-target-creature"));
        assert_eq!(cursor_for(EAT_CARCASS), Cursor::File("cursor-eat-carcass"));
        assert_eq!(cursor_for(MATE), Cursor::File("cursor-mate"));
        for id in [SOCIAL_PACK, SOCIAL] {
            assert_eq!(cursor_for(id), Cursor::File("cursor_social"));
        }
        for id in [PICKUP, PICKUP_CREATURE] {
            assert_eq!(cursor_for(id), Cursor::File("cursor-crg-pickup"));
        }
        assert_eq!(cursor_for(GIVE), Cursor::File("cursor-crg-give"));
        for id in [THROW, THROW_FOOD] {
            assert_eq!(cursor_for(id), Cursor::File("cursor-crg-throw"));
        }
        assert_eq!(cursor_for(SOCIAL_NO), Cursor::File("cursor_socialNo"));
        assert_eq!(cursor_for(FRIEND), Cursor::File("cursor_friend"));
        for id in [ATTACK, ATTACK_EGG] {
            assert_eq!(cursor_for(id), Cursor::File("cursor_atk"));
        }
        for id in [PUTDOWN, PICKUP_ORNAMENT] {
            assert_eq!(cursor_for(id), Cursor::File("cursor-crg-putdown"));
        }
        assert_eq!(cursor_for(CLAIM_NEST), Cursor::File("cur_ClaimNest"));
        assert_eq!(cursor_for(EAT_FRUIT), Cursor::File("cursor-eat-fruit"));
        assert_eq!(cursor_for(SKELETON), Cursor::File("cursor_skeleton2"));
    }

    #[test]
    fn a_creature_first_shows_the_targeting_ring_then_the_stance_cursor() {
        let stranger = CreatureView { targeted: false, ..view() };
        assert_eq!(choose_action(Target::Creature(stranger), true), TARGET);
        assert_eq!(choose_action(Target::Creature(stranger), false), TARGET);
        // The targeted one: the social bubble or the attack crosshair.
        assert_eq!(choose_action(Target::Creature(view()), true), SOCIAL);
        assert_eq!(choose_action(Target::Creature(view()), false), ATTACK);
        // A friend smiles at the social cursor; a hostile one refuses it unless it is performing.
        let friend = CreatureView { friend: true, ..view() };
        assert_eq!(choose_action(Target::Creature(friend), true), FRIEND);
        assert_eq!(choose_action(Target::Creature(friend), false), ATTACK);
        let hostile = CreatureView { relationship: relationship::HOSTILE, ..view() };
        assert_eq!(choose_action(Target::Creature(hostile), true), SOCIAL_NO);
        let answering = CreatureView { performing: true, ..hostile };
        assert_eq!(choose_action(Target::Creature(answering), true), SOCIAL);
        // The social cursor of a pack member shows in both stances; the mate wins over all.
        let pack = CreatureView { pack_member: true, ..view() };
        assert_eq!(choose_action(Target::Creature(pack), true), SOCIAL_PACK);
        assert_eq!(choose_action(Target::Creature(pack), false), SOCIAL_PACK);
        let mate = CreatureView { mate: true, targeted: false, ..view() };
        assert_eq!(choose_action(Target::Creature(mate), false), MATE);
        // The player's own species is never attacked.
        let own = CreatureView { same_species: true, ..view() };
        assert_eq!(choose_action(Target::Creature(own), false), NONE);
    }

    #[test]
    fn food_and_ground_follow_the_noun_rules() {
        assert_eq!(choose_action(Target::Ground, true), NONE);
        assert_eq!(choose_action(Target::Avatar, false), NONE);
        assert_eq!(choose_action(Target::Carcass { food: 12.0 }, true), EAT_CARCASS);
        assert_eq!(choose_action(Target::Carcass { food: 0.0 }, true), NONE);
        assert_eq!(choose_action(Target::Fruit { edible: true }, false), EAT_FRUIT);
        assert_eq!(choose_action(Target::Fruit { edible: false }, false), NONE);
        assert_eq!(choose_action(Target::Nest, true), CLAIM_NEST);
        assert_eq!(cursor_for(CLAIM_NEST), Cursor::File("cur_ClaimNest"));
    }

    #[test]
    fn the_pointer_ray_finds_creatures_meat_and_bushes() {
        let creature = Body { id: 7, centre: Vec3::new(0.0, 1.0, -10.0), radius: 1.0, view: view() };
        let piles = [Spot { at: Vec3::new(5.0, 0.0, -10.0), reach: 1.5, amount: 20.0 }];
        let patches = [Spot { at: Vec3::new(-5.0, 0.0, -10.0), reach: 1.6, amount: 60.0 }];
        let nests = [Spot { at: Vec3::new(30.0, 0.0, -10.0), reach: 6.5, amount: 1.0 }];
        let ahead = |at: Vec3| Vec3::new(at.x, 0.0, at.z);
        let ray = |at: Vec3| (Vec3::new(0.0, 1.0, 0.0), (at - Vec3::new(0.0, 1.0, 0.0)).normalize());
        let look = |at: Vec3, social: bool| {
            let (o, d) = ray(at);
            hover_at(o, d, Some(ahead(at)), &[creature], &piles, &patches, &nests, &[], social)
        };
        let h = look(Vec3::new(0.0, 1.0, -10.0), true);
        assert_eq!((h.action, h.creature), (SOCIAL, Some(7)));
        assert_eq!(look(Vec3::new(0.0, 1.0, -10.0), false).action, ATTACK);
        assert_eq!(look(Vec3::new(5.0, 0.0, -10.0), true).action, EAT_CARCASS);
        assert_eq!(look(Vec3::new(-5.0, 0.0, -10.0), true).action, EAT_FRUIT);
        assert_eq!(look(Vec3::new(15.0, 0.0, -10.0), true).action, NONE);
        // The claimable nest (kNest): the claim cursor in either stance, within its radius.
        assert_eq!(look(Vec3::new(30.0, 0.0, -10.0), true).action, CLAIM_NEST);
        assert_eq!(look(Vec3::new(34.0, 0.0, -10.0), false).action, CLAIM_NEST);
        assert_eq!(look(Vec3::new(38.0, 0.0, -10.0), false).action, NONE);
        // Ground under a creature is the creature's.
        let on_pile = Body { centre: Vec3::new(5.0, 1.0, -10.0), ..creature };
        let (o, d) = ray(Vec3::new(5.0, 1.0, -10.0));
        let h = hover_at(o, d, Some(Vec3::new(5.0, 0.0, -10.0)), &[on_pile], &piles, &patches, &nests, &[], true);
        assert_eq!(h.creature, Some(7));
    }

    #[test]
    fn a_whole_egg_shows_the_attack_cursor_in_the_attack_stance_only() {
        // kEgg (0xD2FFD0): attack stance, state not 3 / 4 / 5, not a best friend's.
        let egg = |state, friendly| Target::Egg { state, friendly };
        for state in [0, 1, 2, 6] {
            assert_eq!(choose_action(egg(state, false), false), ATTACK_EGG, "state {state}");
            assert_eq!(choose_action(egg(state, false), true), NONE, "social stance");
        }
        for state in [3, 4, 5] {
            assert_eq!(choose_action(egg(state, false), false), NONE, "state {state}");
        }
        assert_eq!(choose_action(egg(0, true), false), NONE, "a best friend's egg");
        assert_eq!(cursor_for(ATTACK_EGG), Cursor::File("cursor_atk"));
        // The pointer picks the egg on the ground inside its reach, before the nest.
        let spot = EggSpot { entity: Entity::PLACEHOLDER, at: Vec3::new(2.0, 0.0, -10.0), reach: 1.1, state: 6, friendly: false };
        let nests = [Spot { at: Vec3::new(2.0, 0.0, -10.0), reach: 6.5, amount: 1.0 }];
        let ray = |at: Vec3| (Vec3::new(0.0, 1.0, 0.0), (at - Vec3::new(0.0, 1.0, 0.0)).normalize());
        let look = |at: Vec3, social: bool| {
            let (o, d) = ray(at);
            hover_at(o, d, Some(at), &[], &[], &[], &nests, &[spot], social)
        };
        let h = look(Vec3::new(2.5, 0.0, -10.0), false);
        assert_eq!((h.action, h.egg), (ATTACK_EGG, Some(Entity::PLACEHOLDER)));
        assert_eq!(look(Vec3::new(5.0, 0.0, -10.0), false).egg, None, "off the egg, still the nest");
    }

    #[test]
    fn the_answering_mate_shows_the_mate_cursor_before_anything_else() {
        // 0xD2FFD0 tests flag 0x10 first: the mate cursor even for a mate that is
        // the avatar's target, in either stance, and over the same-species rule.
        let mate = CreatureView { mate: true, friend: true, same_species: true, targeted: true, ..CreatureView::default() };
        let body = Body { id: crate::creature_nestmates::mate_id(0), centre: Vec3::new(0.0, 1.0, -10.0), radius: 1.0, view: mate };
        let (o, d) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0));
        for social in [true, false] {
            let h = hover_at(o, d, None, &[body], &[], &[], &[], &[], social);
            assert_eq!(h.action, action::MATE);
            assert_eq!(h.creature, Some(body.id));
        }
        assert!(matches!(cursor_for(action::MATE), Cursor::File("cursor-mate")));
    }

    fn installed() -> Option<crate::asset_catalog::ResourceIndex> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then_some(index)
    }

    #[test]
    fn every_cursor_the_creature_start_up_registers_is_an_installed_file() {
        let Some(index) = installed() else { return };
        for name in REGISTERED {
            let bytes = index.read(cursor_key(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            let cur = decode_cursor(&bytes).unwrap_or_else(|| panic!("{name} does not decode"));
            assert_eq!((cur.width, cur.height), (32, 32), "{name}");
            assert!(cur.rgba.chunks(4).any(|p| p[3] > 0), "{name} is blank");
            assert!(cur.hotspot.0 < 32 && cur.hotspot.1 < 32, "{name}");
        }
        // Every cursor the switch picks is one of the registered ones (or the Tribal ban / spyglass pair).
        for id in 0..=0x1C {
            if let Cursor::File(name) = cursor_for(id) {
                let extra = ["cursor-ban-mode", "cursor-ban-content", "cursor_spyglass"];
                assert!(REGISTERED.contains(&name) || extra.contains(&name), "{name}");
            }
        }
    }
}
