//! The parts of the Creature HUD that belong to other creatures: the pack list
//! (`PosseBar` + one `PosseItemCreature` per member), the name/health card of
//! the creature under the cursor (`Rollover_Creature`) and the friendship meter
//! shown while a creature is being impressed (`SocialBar`).
//!
//! [`publish_world`] fills the matching parts of [`CreatureHudState`] from the
//! Creature components; [`bind_world`] pushes them into the layouts.

use bevy::prelude::*;

use super::{CreatureHudActive, CreatureHudRoot, CreatureHudState};
use crate::{
    creature_actions::{self, CreatureMind, Encounter, EncounterMode},
    epic_creatures::RoamerActor,
    game::{Health, Npc, RunProgress},
    persistence::WorldId,
    spui::{
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiPointer, SpuiState},
    },
    stages::GameStage,
};

pub const ROLLOVER_LAYOUT: &str = "Rollover_Creature";
/// The friendship meter of the Creature stage (VERIFIED: the creature game UI
/// loads `SocialRainbow`; `SocialBar` is the Tribal posse UI's, see
/// `super::rainbow`).
pub const SOCIAL_LAYOUT: &str = super::rainbow::LAYOUT;
pub const POSSE_LAYOUT: &str = "PosseBar";
pub const POSSE_ITEM_LAYOUT: &str = "PosseItemCreature";

/// Control ids of the layouts above (the reference of the layouts; some ids
/// are only checked by the tests).
#[allow(dead_code)]
pub mod id {
    // Rollover_Creature
    pub const ROLL_ROOT: u32 = 0x9331_0E4A;
    pub const ROLL_NAME: u32 = 0x031E_94D0;
    /// White name plate the code tints by the relationship.
    pub const ROLL_PLATE: u32 = 0x9331_03EA;
    /// Bar frame with the health number, and the three bars drawn over it
    /// (health, heal preview, hurt flash).
    pub const ROLL_BAR: u32 = 0x026E_0DF3;
    pub const ROLL_HEALTH: u32 = 0x026E_0DF0;
    pub const ROLL_HEAL: u32 = 0x026E_0DF1;
    pub const ROLL_HURT: u32 = 0x026E_0DF2;
    pub const ROLL_NUMBER: u32 = 0x04D0_39A8;
    /// Eight-frame relationship face.
    pub const ROLL_FACE: u32 = 0x046A_DF86;
    // SocialBar (the Tribal posse UI's meter, 0xCCDEF0 -> 0xE31630; not
    // shown by the Creature HUD any more)
    pub const SOCIAL_ROOT: u32 = 0x9548_1ABB;
    pub const SOCIAL_FILL: u32 = 0xB548_1B0B;
    pub const SOCIAL_FACE_LEFT: u32 = 0xB548_1B68;
    pub const SOCIAL_FACE_RIGHT: u32 = 0xD548_1B5F;
    /// Star burst behind the right face.
    pub const SOCIAL_STAR: u32 = 0xD548_1B72;
    // PosseBar
    pub const POSSE_ROOT: u32 = 0x03D9_8298;
    pub const POSSE_LIST: u32 = 0x03D9_82FF;
    pub const POSSE_TITLE: u32 = 0x064A_90A0;
    pub const POSSE_COUNT: u32 = 0x0649_A840;
    pub const POSSE_TOP: u32 = 0x03DA_E410;
    pub const POSSE_BOTTOM: u32 = 0x03DA_E411;
    /// State icons of the "select all" toggle that the code shows on demand.
    pub const POSSE_HIDDEN: [u32; 2] = [0x064A_386F, 0x03DA_E414];
    // PosseItemCreature
    pub const ITEM_ROOT: u32 = 0x03D9_8299;
    pub const ITEM_BODY: u32 = 0x03D9_8304;
    pub const ITEM_PORTRAIT: u32 = 0x03D9_9151;
    /// Layout windows painted with design-time fill colours (cleared) and
    /// the plus and state icons that sit beside the item (hidden).
    pub const ITEM_FILLED: [u32; 4] = [ITEM_ROOT, ITEM_BODY, 0x03D9_9141, ITEM_PORTRAIT];
    pub const ITEM_HIDDEN: [u32; 2] = [0x03D9_9163, 0x03D9_9185];
    /// Thirty-one frame health meter.
    pub const ITEM_HEALTH: u32 = 0x03D9_9162;
    /// Colour swatches the code reads (never drawn).
    pub const ITEM_SWATCHES: [u32; 3] = [0x03DF_0885, 0x03DF_0886, 0x03DF_0887];
}

/// Retail locale string of the pack bar title (`CreatureGame` "Pack").
pub const PACK_TITLE: &str = "loc:CreatureGame:937c25e9";
/// Pack members shown (the pack never holds more).
pub const MAX_PACK_ITEMS: usize = 3;
/// Left edge and spacing of the pack list (ADAPTED: the retail code places it
/// below the goal cards).
const POSSE_X: f32 = 24.0;
const POSSE_ITEM_STEP: f32 = 76.0;

/// A member of the player's pack as the HUD shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackMember {
    pub id: u32,
    pub name: String,
    /// Health fraction `0.0..=1.0`.
    pub health: f32,
}

/// How a creature feels about the player (frame of the relationship face).
/// All eight faces of the installed art are listed; the game state selects a
/// few of them today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Relation {
    Hostile,
    Wary,
    Alert,
    Curious,
    Neutral,
    Friendly,
    Ally,
    /// The skull: a creature that cannot be impressed any more.
    Skull,
}

impl Relation {
    /// Frame of the eight-frame face icon of `Rollover_Creature`, in the
    /// order of the installed art: angry, sad, surprised (!), confused (?),
    /// blue neutral, green happy, pink in love, skull.
    pub fn frame(self) -> u32 {
        match self {
            Relation::Hostile => 0,
            Relation::Wary => 1,
            Relation::Alert => 2,
            Relation::Curious => 3,
            Relation::Neutral => 4,
            Relation::Friendly => 5,
            Relation::Ally => 6,
            Relation::Skull => 7,
        }
    }

    /// Colour the name plate (a white picture) is tinted with. ADAPTED: the
    /// retail tint is not decoded (the plate window 0x933103EA is only handed
    /// a colour by the card's update 0xB6A820, whose callers pass it); friends
    /// are green, strangers blue, hostile creatures red.
    pub fn plate_color(self) -> u32 {
        match self {
            Relation::Hostile => 0xFFD2_4B3C,
            Relation::Wary | Relation::Alert => 0xFFE0_9640,
            Relation::Curious => 0xFFD8_BE50,
            Relation::Neutral => 0xFF6C_A0D8,
            Relation::Friendly => 0xFF5F_BF6A,
            Relation::Ally => 0xFF48_C48A,
            Relation::Skull => 0xFF88_8890,
        }
    }
}

/// The face of a creature's card from the species relationship `r`
/// (`creature_rules::relationship`, 1 fearful .. 6 best friends) and whether
/// the creature is a pack member. VERIFIED: the card's update (0xB6A9B2) takes
/// `0xB683A0` of the hovered creature: a pack member (general flag 0x100) is 6,
/// the mate (flag 0x10) 7, else the species relationship `0xBA40E0`; and
/// `0xB67630` turns it into the frame: 1 -> 2, 2 -> 1, 3 -> 3, 4 -> 4, 5 -> 0,
/// 6 -> 5, 7 -> 6 (the mate's pink face), anything else hides the face (the
/// skull, frame 7, is the nest card's). The same table drives the friendship
/// meter (0xE31C70, `rainbow::face_frame`).
pub fn relation_for(relationship: u8, pack_member: bool) -> Option<Relation> {
    Some(match if pack_member { 6 } else { relationship } {
        1 => Relation::Alert,
        2 => Relation::Wary,
        3 => Relation::Curious,
        4 => Relation::Neutral,
        5 => Relation::Hostile,
        6 => Relation::Friendly,
        _ => return None,
    })
}

/// The creature card shown near a creature.
#[derive(Clone, Debug, PartialEq)]
pub struct Rollover {
    pub name: String,
    pub health: f32,
    pub hp: i32,
    /// The face (`None` hides it: no relationship, 0xB67630 answers -1).
    pub relation: Option<Relation>,
    /// Anchor in physical screen pixels (the creature's head).
    pub screen: Vec2,
}

/// The friendship meter of the creature being impressed.
#[derive(Clone, Debug, PartialEq)]
pub struct SocialMeter {
    /// Impression `0.0..=1.0` (both sides of the bar).
    pub friendship: f32,
    pub relation: Option<Relation>,
    /// The ability the creature is performing (slot 0..4), if any.
    pub requested: Option<usize>,
    /// Fraction of the answer window left.
    pub time_left: f32,
    /// The two rows of the retail bar as fractions of the whole bar (0xE319F0):
    /// the creature's demonstrations (row 0) and the player's answers (row 1).
    pub creature: f32,
    pub player: f32,
    /// Rounds of the proposal (the meter's ticks).
    pub rounds: u8,
    /// The species' creature-game relationship (`creature_rules::relationship`).
    pub species_relation: u8,
}


/// Health frame (0..=30) of a pack item's meter.
pub fn item_health_frame(health: f32) -> u32 {
    (health.clamp(0.0, 1.0) * 30.0).round() as u32
}

/// Fill the pack, rollover and friendship parts of [`CreatureHudState`].
#[allow(clippy::too_many_arguments)]
pub fn publish_world(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    encounter: Res<Encounter>,
    originals: Res<crate::Originals>,
    pointer: Res<SpuiPointer>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    npcs: Query<(&WorldId, &Transform, &Npc, &CreatureMind, &Visibility), Without<RoamerActor>>,
    roamers: Query<(&WorldId, &Transform, &Health, &CreatureMind, &Visibility, &RoamerActor), Without<Npc>>,
    mut hud: ResMut<CreatureHudState>,
) {
    if *stage.get() != GameStage::Creature {
        return;
    }
    let creature_world = &run.species_world;
    // Pack list and capacity.
    hud.pack_capacity = crate::creature_progression::pack_capacity(run.dna_this_stage);
    hud.pack = run
        .creature_pack
        .members
        .iter()
        .take(MAX_PACK_ITEMS)
        .map(|&member| {
            let health = npcs
                .iter()
                .find(|(id, ..)| id.0 == member)
                .map(|(_, _, npc, ..)| {
                    let max = creature_world
                        .species_for_member(member)
                        .map_or(100.0, |s| crate::creature::displayed_member_stats(s, &originals).1);
                    (npc.hp / max.max(1.0)).clamp(0.0, 1.0)
                })
                .unwrap_or(1.0);
            PackMember { id: member, name: creature_actions::member_name(&run, member), health }
        })
        .collect();
    // Friendship meter of the targeted creature while impressing it.
    hud.social_meter = None;
    if encounter.mode == EncounterMode::Social {
        let found = encounter.target.and_then(|target| {
            npcs.iter().find(|(id, ..)| id.0 == target).map(|(_, _, npc, mind, _)| (mind.clone(), npc.hostile, false)).or_else(
                || {
                    roamers
                        .iter()
                        .find(|(id, ..)| id.0 == target)
                        .map(|(_, _, _, mind, _, roamer)| (mind.clone(), roamer.aggressive, true))
                },
            )
        });
        if let Some((mind, _hostile, roamer)) = found {
            let performing = mind.response_time > 0.0;
            if !mind.allied && (performing || mind.impression > 0.0) {
                let side = |fill: f32| if mind.bar_max > 0.0 { (fill / mind.bar_max).clamp(0.0, 1.0) } else { 0.0 };
                let (creature, player) = if mind.bar_max > 0.0 {
                    (side(mind.npc_fill), side(mind.player_fill))
                } else {
                    // Saved before the bar was planned: all of it is the player's.
                    (0.0, (mind.impression / 100.0).clamp(0.0, 1.0))
                };
                let species_relation = creature_actions::avatar_relationship(
                    &run,
                    encounter.target.unwrap_or(0),
                    mind.species as u32,
                    roamer,
                );
                hud.social_meter = Some(SocialMeter {
                    friendship: (mind.impression / 100.0).clamp(0.0, 1.0),
                    relation: relation_for(species_relation, false),
                    requested: performing.then_some(mind.requested.min(3) as usize),
                    time_left: (mind.response_time / creature_actions::SOCIAL_RESPONSE_WINDOW).clamp(0.0, 1.0),
                    creature,
                    player,
                    rounds: (mind.requests.len() as u8).max(mind.rounds).max(1),
                    species_relation,
                });
            }
        }
    }
    // Rollover: the creature under the pointer, else the targeted one.
    hud.rollover = None;
    let Ok((camera, camera_t)) = cameras.single() else {
        return;
    };
    let scale = camera.target_scaling_factor().unwrap_or(1.0);
    let project = |position: Vec3| camera.world_to_viewport(camera_t, position).ok().map(|p| p * scale);
    struct Candidate {
        id: u32,
        /// An epic creature (its name is shown with the retail prefix).
        epic: bool,
        hp: f32,
        max: f32,
        /// The species relationship (`creature_rules::relationship`).
        relationship: u8,
        pack_member: bool,
        head: Vec2,
        body: Vec2,
        radius: f32,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    for (id, t, npc, mind, visibility) in &npcs {
        if mind.defeated || mind.downed > 0.0 || *visibility == Visibility::Hidden {
            continue;
        }
        let Some(species) = creature_world.species_for_member(id.0) else {
            continue;
        };
        let max = crate::creature::displayed_member_stats(species, &originals).1;
        let lift = 1.15 * t.scale.y + 0.55;
        let (Some(head), Some(body)) =
            (project(t.translation + Vec3::Y * lift), project(t.translation + Vec3::Y * 0.6 * t.scale.y))
        else {
            continue;
        };
        let edge = project(t.translation + Vec3::Y * 0.6 * t.scale.y + Vec3::X * 0.8 * t.scale.x)
            .map_or(0.0, |e| e.distance(body));
        candidates.push(Candidate {
            id: id.0,
            epic: false,
            hp: npc.hp,
            max,
            relationship: creature_actions::avatar_relationship(&run, id.0, mind.species as u32, false),
            pack_member: run.creature_pack.members.contains(&id.0),
            head,
            body,
            radius: edge,
        });
    }
    for (id, t, health, mind, visibility, roamer) in &roamers {
        if mind.defeated || *visibility == Visibility::Hidden || health.dead() {
            continue;
        }
        let lift = 1.15 * t.scale.y + 0.55;
        let (Some(head), Some(body)) =
            (project(t.translation + Vec3::Y * lift), project(t.translation + Vec3::Y * 0.6 * t.scale.y))
        else {
            continue;
        };
        let edge = project(t.translation + Vec3::Y * 0.6 * t.scale.y + Vec3::X * 0.8 * t.scale.x)
            .map_or(0.0, |e| e.distance(body));
        candidates.push(Candidate {
            id: id.0,
            epic: true,
            hp: health.hp,
            max: health.max,
            // As the cursor reads it (`cursor`): an aggressive epic is hostile.
            relationship: if roamer.aggressive {
                crate::creature_rules::relationship::HOSTILE
            } else {
                creature_actions::avatar_relationship(&run, id.0, 0, true)
            },
            pack_member: run.creature_pack.members.contains(&id.0),
            head,
            body,
            radius: edge,
        });
    }
    let hovered = pointer.position.and_then(|cursor| {
        candidates
            .iter()
            .filter(|c| c.body.distance(cursor) <= c.radius.max(HOVER_MIN_RADIUS))
            .min_by(|a, b| a.body.distance(cursor).total_cmp(&b.body.distance(cursor)))
    });
    // The card is the rollover of the hovered object only (0xB6A9B2 reads the
    // rollover target `[+4]`); the targeted creature is marked by the target
    // ring effect, not by a card, so nothing hovered shows no card.
    hud.rollover = hovered.map(|c| {
        let name = creature_actions::member_name(&run, c.id);
        Rollover {
            name: short_name(&if c.epic { format!("Epic {name}") } else { name }),
            health: (c.hp / c.max.max(1.0)).clamp(0.0, 1.0),
            hp: c.hp.max(0.0).ceil() as i32,
            relation: relation_for(c.relationship, c.pack_member),
            screen: c.head,
        }
    });
}

/// Longest name the card's plate holds on one line (the retail names are short;
/// the rewrite's species are named after their mouth part). ADAPTED.
const NAME_CHARS: usize = 18;

/// The name cut to the plate, with an ellipsis when it was longer.
pub fn short_name(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= NAME_CHARS {
        return name.to_string();
    }
    let cut: String = name.chars().take(NAME_CHARS - 3).collect();
    format!("{}...", cut.trim_end())
}

/// How far the card reaches from its anchor (the creature's head): left
/// (name plate), right (face and star burst) and up (the plate's top).
const CARD_LEFT: f32 = 88.0;
const CARD_RIGHT: f32 = 108.0;
const CARD_UP: f32 = 82.0;

/// The card anchor moved so that the card stays on screen.
pub fn clamp_anchor(anchor: Vec2, screen: Vec2) -> Vec2 {
    Vec2::new(
        anchor.x.clamp(CARD_LEFT, (screen.x - CARD_RIGHT).max(CARD_LEFT)),
        anchor.y.clamp(CARD_UP, (screen.y - 8.0).max(CARD_UP)),
    )
}

/// Minimum pick radius around a creature's body, physical pixels.
const HOVER_MIN_RADIUS: f32 = 28.0;

/// Spawn and despawn the roots that depend on counts: mission cards (one
/// `CRGMissionCard` each) and pack items (one `PosseItemCreature` each).
#[allow(clippy::too_many_arguments)]
pub fn manage_dynamic(
    mut commands: Commands,
    active: Res<CreatureHudActive>,
    hud: Res<CreatureHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CreatureHudRoot)>,
) {
    if !active.0 {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let want_cards = hud.goal_cards.len().min(super::guide::MAX_CARDS);
    let want_items = hud.pack.len().min(MAX_PACK_ITEMS);
    for (entity, kind) in &roots {
        let surplus = match kind {
            CreatureHudRoot::Card(n) => *n as usize >= want_cards,
            CreatureHudRoot::PosseItem(n) => *n as usize >= want_items,
            _ => false,
        };
        if surplus {
            commands.entity(entity).despawn();
        }
    }
    let has = |wanted: CreatureHudRoot| roots.iter().any(|(_, kind)| *kind == wanted);
    for n in 0..want_cards {
        let kind = CreatureHudRoot::Card(n as u8);
        if !has(kind) {
            if let Some(layout) = assets.layout(index, super::CARD_LAYOUT) {
                let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(kind);
            }
        }
    }
    for n in 0..want_items {
        let kind = CreatureHudRoot::PosseItem(n as u8);
        if !has(kind) {
            if let Some(layout) = assets.layout(index, POSSE_ITEM_LAYOUT) {
                let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(kind);
            }
        }
    }
}

fn find(ui: &SpuiState, control: u32) -> Option<usize> {
    ui.find(control)
}

/// Move the root window of an authored layout so that `anchor` (a window of
/// the layout) lands with its top-left corner on `(x, y)`. The authored areas
/// of the anchor's ancestors give its offset from the root.
fn place_by(ui: &mut SpuiState, root: usize, anchor: usize, x: f32, y: f32) {
    let mut offset = Vec2::ZERO;
    let mut at = Some(anchor);
    while let Some(w) = at {
        if w == root {
            break;
        }
        let a = ui.layout.window(w).area;
        offset += Vec2::new(a[0], a[1]);
        at = ui.layout.window(w).parent;
    }
    let authored = ui.layout.window(root).area;
    let (w, h) = (authored[2] - authored[0], authored[3] - authored[1]);
    ui.set_area(root, [x - offset.x, y - offset.y, x - offset.x + w, y - offset.y + h]);
}

fn bind_rollover(ui: &mut SpuiState, rollover: Option<&Rollover>) {
    let Some(root) = find(ui, id::ROLL_ROOT) else {
        return;
    };
    ui.set_visible(root, rollover.is_some());
    let Some(r) = rollover else {
        return;
    };
    // The root is the anchor the code moves to the creature's head; the card
    // hangs above it.
    let authored = ui.layout.window(root).area;
    let (w, h) = (authored[2] - authored[0], authored[3] - authored[1]);
    let at = clamp_anchor(r.screen, Vec2::new(ui.screen[0], ui.screen[1]));
    ui.set_area(root, [at.x, at.y, at.x + w, at.y + h]);
    if let Some(w) = find(ui, id::ROLL_NAME) {
        ui.set_text(w, r.name.clone());
    }
    if let Some(w) = find(ui, id::ROLL_PLATE) {
        ui.set_tint(w, r.relation.unwrap_or(Relation::Neutral).plate_color());
    }
    if let Some(w) = find(ui, id::ROLL_NUMBER) {
        ui.set_text(w, r.hp.to_string());
    }
    for control in [id::ROLL_HEAL, id::ROLL_HURT] {
        if let Some(w) = find(ui, control) {
            ui.set_visible(w, false);
        }
    }
    if let Some(w) = find(ui, id::ROLL_HEALTH) {
        let mut area = ui.layout.window(w).area;
        let full = area[2] - area[0];
        area[2] = area[0] + full * r.health.clamp(0.0, 1.0);
        ui.set_area(w, area);
    }
    if let Some(w) = find(ui, id::ROLL_FACE) {
        ui.set_visible(w, r.relation.is_some());
        ui.set_frame(w, r.relation.map(Relation::frame));
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_posse(
    ui: &mut SpuiState,
    hud: &CreatureHudState,
    assets: &mut SpuiAssets,
    index: &crate::asset_catalog::ResourceIndex,
    top: f32,
) {
    let Some(root) = find(ui, id::POSSE_ROOT) else {
        return;
    };
    let show = hud.pack_capacity > 0 || !hud.pack.is_empty();
    ui.set_visible(root, show);
    if !show {
        return;
    }
    if let Some(list) = find(ui, id::POSSE_LIST) {
        place_by(ui, root, list, POSSE_X, top + 34.0);
    }
    if let Some(w) = find(ui, id::POSSE_TITLE) {
        let title = super::guide::resolve(assets, index, PACK_TITLE);
        ui.set_text(w, title);
        ui.set_visible(w, true);
    }
    if let Some(w) = find(ui, id::POSSE_COUNT) {
        ui.set_text(w, format!("{}/{}", hud.pack.len(), hud.pack_capacity));
        ui.set_visible(w, true);
    }
    // The list's edge strips are placeholders for the list length.
    for control in [id::POSSE_TOP, id::POSSE_BOTTOM].iter().chain(&id::POSSE_HIDDEN) {
        if let Some(w) = find(ui, *control) {
            ui.set_visible(w, false);
        }
    }
}

fn bind_item(ui: &mut SpuiState, member: &PackMember, slot: usize, top: f32) {
    let (Some(root), Some(body)) = (find(ui, id::ITEM_ROOT), find(ui, id::ITEM_BODY)) else {
        return;
    };
    place_by(ui, root, body, POSSE_X + 6.0, top + 58.0 + slot as f32 * POSSE_ITEM_STEP);
    ui.set_visible(root, true);
    // The item has no name text: hovering it names the member.
    ui.set_tooltip(body, Some(member.name.clone()));
    for control in id::ITEM_SWATCHES.iter().chain(&id::ITEM_HIDDEN) {
        if let Some(w) = find(ui, *control) {
            ui.set_visible(w, false);
        }
    }
    // The layout paints design-time colour blocks behind the item.
    for control in id::ITEM_FILLED {
        if let Some(w) = find(ui, control) {
            ui.set_tint(w, 0x00FF_FFFF);
        }
    }
    if let Some(w) = find(ui, id::ITEM_HEALTH) {
        ui.set_frame(w, Some(item_health_frame(member.health)));
    }
}

/// Push the pack, rollover and friendship state into the layouts.
pub fn bind_world(
    hud: Res<CreatureHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut roots: Query<(&mut SpuiState, &CreatureHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    // The pack list hangs below the stack of mission cards (their measured
    // bottom edge).
    let cards_bottom = roots
        .iter()
        .filter(|(ui, kind)| matches!(kind, CreatureHudRoot::Card(_)) && ui.ready)
        .map(|(ui, _)| super::guide::card_bottom(ui))
        .fold(0.0_f32, f32::max);
    let top = cards_bottom + 8.0;
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            CreatureHudRoot::Rollover => bind_rollover(&mut ui, hud.rollover.as_ref().filter(|_| !hud.paused)),
            CreatureHudRoot::Social => super::rainbow::bind(&mut ui, hud.social_meter.as_ref().filter(|_| !hud.paused)),
            CreatureHudRoot::Posse => bind_posse(&mut ui, &hud, &mut assets, index, top),
            CreatureHudRoot::PosseItem(n) => {
                if let Some(member) = hud.pack.get(*n as usize) {
                    bind_item(&mut ui, member, *n as usize, top);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_face_follows_the_species_relationship() {
        use crate::creature_rules::relationship as r;
        // 0xB67630: 1 -> frame 2, 2 -> 1, 3 -> 3, 4 -> 4, 5 -> 0, 6 -> 5.
        let frame = |relationship, pack| relation_for(relationship, pack).map(Relation::frame);
        assert_eq!(frame(r::FEARFUL, false), Some(2));
        assert_eq!(frame(r::WARY, false), Some(1));
        assert_eq!(frame(r::CURIOUS, false), Some(3));
        assert_eq!(frame(r::FRIENDLY, false), Some(4));
        assert_eq!(frame(r::HOSTILE, false), Some(0));
        assert_eq!(frame(r::BEST_FRIENDS, false), Some(5));
        // A pack member counts as best friends whatever its species thinks (0xB683A0).
        assert_eq!(frame(r::HOSTILE, true), Some(5));
        // No relationship, no face (the pink face is the mate's, the skull a nest's).
        assert_eq!(frame(0, false), None);
        assert_eq!(frame(7, false), None);
    }

    #[test]
    fn the_eight_faces_are_distinct_frames() {
        let all = [
            Relation::Hostile,
            Relation::Wary,
            Relation::Alert,
            Relation::Curious,
            Relation::Neutral,
            Relation::Friendly,
            Relation::Ally,
            Relation::Skull,
        ];
        let frames: Vec<u32> = all.iter().map(|r| r.frame()).collect();
        assert_eq!(frames, (0..8).collect::<Vec<u32>>());
    }

    #[test]
    fn long_names_are_cut_to_the_plate() {
        assert_eq!(short_name("Noogie"), "Noogie");
        assert_eq!(short_name("  Epic Noogie "), "Epic Noogie");
        let long = short_name("ce_mouth_cute_herbivore (NOT LOCALIZED)");
        assert_eq!(long, "ce_mouth_cute_h...");
        assert!(long.chars().count() <= NAME_CHARS);
        assert_eq!(short_name("exactly eighteen!!"), "exactly eighteen!!");
    }

    #[test]
    fn the_creature_card_stays_on_screen() {
        let screen = Vec2::new(1280.0, 900.0);
        assert_eq!(clamp_anchor(Vec2::new(640.0, 400.0), screen), Vec2::new(640.0, 400.0));
        let corner = clamp_anchor(Vec2::new(-50.0, -10.0), screen);
        assert_eq!((corner.x, corner.y), (CARD_LEFT, CARD_UP));
        let far = clamp_anchor(Vec2::new(5000.0, 5000.0), screen);
        assert_eq!((far.x, far.y), (1280.0 - CARD_RIGHT, 892.0));
        // A screen narrower than the card does not invert the range.
        assert_eq!(clamp_anchor(Vec2::new(10.0, 10.0), Vec2::new(50.0, 50.0)).x, CARD_LEFT);
    }

    #[test]
    fn item_health_uses_the_thirty_one_frames() {
        assert_eq!(item_health_frame(0.0), 0);
        assert_eq!(item_health_frame(1.0), 30);
        assert_eq!(item_health_frame(0.5), 15);
        assert_eq!(item_health_frame(7.0), 30);
    }

    #[test]
    fn installed_world_layouts_have_the_windows_the_binding_uses() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let rollover = assets.layout(&index, ROLLOVER_LAYOUT).expect("Rollover_Creature");
        for control in [
            id::ROLL_ROOT,
            id::ROLL_NAME,
            id::ROLL_PLATE,
            id::ROLL_BAR,
            id::ROLL_HEALTH,
            id::ROLL_HEAL,
            id::ROLL_HURT,
            id::ROLL_NUMBER,
            id::ROLL_FACE,
        ] {
            assert!(rollover.find(control).is_some(), "rollover {control:08X}");
        }
        // The relationship face has eight frames, the friendship faces five.
        let face = rollover.window(rollover.find(id::ROLL_FACE).unwrap());
        assert_eq!(face.anim().map(|a| a.frame_count), Some(8));
        use super::super::rainbow::id as rb;
        let social = assets.layout(&index, SOCIAL_LAYOUT).expect("SocialRainbow");
        for control in [rb::ROOT, rb::FRAME, rb::FACE, rb::STAR, rb::KNOB_LEFT, rb::KNOB_RIGHT, rb::KNOB_FILL_PLAYER, rb::KNOB_FILL_CREATURE]
            .iter()
            .chain(&rb::KNOB_FULL)
            .chain(&rb::TEMPLATES)
        {
            assert!(social.find(*control).is_some(), "social rainbow {control:08X}");
        }
        // The rainbow's face has the eight relationship frames.
        let face = social.window(social.find(rb::FACE).unwrap());
        assert_eq!(face.anim().map(|a| a.frame_count), Some(8));
        let posse = assets.layout(&index, POSSE_LAYOUT).expect("PosseBar");
        for control in
            [id::POSSE_ROOT, id::POSSE_LIST, id::POSSE_TITLE, id::POSSE_COUNT, id::POSSE_TOP, id::POSSE_BOTTOM]
                .iter()
                .chain(&id::POSSE_HIDDEN)
        {
            assert!(posse.find(*control).is_some(), "posse bar {control:08X}");
        }
        let item = assets.layout(&index, POSSE_ITEM_LAYOUT).expect("PosseItemCreature");
        for control in [id::ITEM_ROOT, id::ITEM_BODY, id::ITEM_PORTRAIT, id::ITEM_HEALTH]
            .iter()
            .chain(&id::ITEM_SWATCHES)
            .chain(&id::ITEM_FILLED)
            .chain(&id::ITEM_HIDDEN)
        {
            assert!(item.find(*control).is_some(), "pack item {control:08X}");
        }
        let health = item.window(item.find(id::ITEM_HEALTH).unwrap());
        assert_eq!(health.anim().map(|a| a.frame_count), Some(31));
    }

    #[test]
    fn the_friendship_meter_centres_itself_below_the_top_edge() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        use super::super::rainbow::{self, id as rb};
        let layout = assets.layout(&index, SOCIAL_LAYOUT).unwrap();
        let face = layout.find(rb::FACE).unwrap();
        // The layout's root follows the screen and its group sits at the top
        // middle: any width centres the face.
        for (w, h) in [(1280.0, 900.0), (1920.0, 1080.0)] {
            let areas = crate::spui::solve::authored_areas(&layout);
            let rects = crate::spui::solve::solve(&layout, &areas, [w, h], RootPolicy::Fit);
            let r = rects[face];
            assert!(((r.x1 + r.x2) * 0.5 - w * 0.5).abs() <= 12.0, "{w}: centred {r:?}");
            assert!(r.y1 > 20.0 && r.y1 < 140.0, "{w}: near the top {r:?}");
        }
        let mut ui = SpuiState::new(layout.clone(), RootPolicy::Fit);
        let meter = SocialMeter {
            friendship: 0.5,
            relation: Some(Relation::Neutral),
            requested: None,
            time_left: 1.0,
            creature: 0.25,
            player: 0.0,
            rounds: 3,
            species_relation: 3,
        };
        rainbow::bind(&mut ui, Some(&meter));
        assert!(ui.visible(ui.find(rb::ROOT).unwrap()));
        assert!(!ui.visible(ui.find(rb::KNOB_FILL_PLAYER).unwrap()), "the player's side is empty");
        rainbow::bind(&mut ui, None);
        assert!(!ui.visible(ui.find(rb::ROOT).unwrap()));
    }

    #[test]
    fn placing_by_an_anchor_moves_the_anchor_to_the_target() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let layout = assets.layout(&index, POSSE_ITEM_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Authored);
        let root = ui.find(id::ITEM_ROOT).unwrap();
        let body = ui.find(id::ITEM_BODY).unwrap();
        place_by(&mut ui, root, body, 100.0, 200.0);
        let (r, b) = (ui.area(root), ui.layout.window(body).area);
        // The body's authored offset is added to the root origin.
        assert_eq!((r[0] + b[0], r[1] + b[1]), (100.0, 200.0));
    }
}
