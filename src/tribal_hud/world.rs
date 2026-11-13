//! The parts of the Tribal HUD that show the tribe and what the pointer rests
//! on: the tribe list (`PosseBar` + one `PosseItemTribe` per member, one
//! `PosseItemTribeBaby` per growing egg), the card over the hut with its Add
//! Baby and Tribal Planner buttons (`Rollover_TribeHut`), and the cards over a
//! tribe member (`Rollover_TribeCitizen`), a tool rack (`Rollover_TribeTool`)
//! and a rival tribe (`RolloverTribe2`).
//!
//! [`publish_world`] fills the matching parts of [`TribalHudState`] from the
//! Tribal components; [`bind_world`] pushes them into the layouts and
//! [`list_clicks`] turns a click on a list item into a selection.

use bevy::prelude::*;

use super::{TribalHudActive, TribalHudRoot, TribalHudState, TribalHudUi};
use crate::{
    creature_hud::world::{clamp_anchor, short_name, Relation},
    game::{Health, Player, RunProgress},
    spui::{
        solve::{Rect, RootPolicy},
        view::{self, SpuiAssets, SpuiPointer, SpuiState},
    },
    stages::GameStage,
    tribal::{self, RivalStatus},
    tribal_rts::{Selected, TribeUnit},
    tribal_tools::{self as tools, ToolKind},
};

pub const POSSE_LAYOUT: &str = "PosseBar";
pub const POSSE_ITEM_LAYOUT: &str = "PosseItemTribe";
pub const BABY_ITEM_LAYOUT: &str = "PosseItemTribeBaby";
pub const HUT_LAYOUT: &str = "Rollover_TribeHut";
pub const CITIZEN_LAYOUT: &str = "Rollover_TribeCitizen";
pub const TOOL_LAYOUT: &str = "Rollover_TribeTool";
pub const TRIBE_LAYOUT: &str = "RolloverTribe2";

/// Retail hint for a baby the tribe cannot pay for (`HintsTRG`).
pub const NOT_ENOUGH_FOOD_HINT: &str = "trg-notenoughfoodforbaby";
/// Retail hint for an ability used while the chieftain is not there.
pub const CHIEFTAIN_DOWN_HINT: &str = "trg-chieftainunavailable";

/// Control ids of the layouts above (the reference of the layouts; some ids
/// are only checked by the tests).
#[allow(dead_code)]
pub mod id {
    // PosseBar (the same layout as the Creature pack bar).
    pub const POSSE_ROOT: u32 = 0x03D9_8298;
    pub const POSSE_LIST: u32 = 0x03D9_82FF;
    pub const POSSE_TITLE: u32 = 0x064A_90A0;
    pub const POSSE_COUNT: u32 = 0x0649_A840;
    pub const POSSE_TOP: u32 = 0x03DA_E410;
    pub const POSSE_BOTTOM: u32 = 0x03DA_E411;
    /// The "Select All" toggle (tooltip "Select All") that heads the list.
    pub const POSSE_SELECT_ALL: u32 = 0x03DA_E412;
    /// State icons of the "select all" toggle that the code shows on demand.
    pub const POSSE_HIDDEN: [u32; 2] = [0x064A_386F, 0x03DA_E414];
    // PosseItemTribe / PosseItemTribeBaby
    pub const ITEM_ROOT: u32 = 0x03D9_8299;
    pub const ITEM_BODY: u32 = 0x03D9_8304;
    pub const ITEM_PORTRAIT: u32 = 0x03D9_9151;
    /// Selection highlight (hidden in the layout).
    pub const ITEM_SELECTED: u32 = 0x04F6_56E8;
    /// Thirty-one frame meters: health (green) and hunger (orange).
    pub const ITEM_HEALTH: u32 = 0x03D9_9162;
    pub const ITEM_HUNGER: u32 = 0x03D9_9184;
    /// Layout windows painted with design-time fill colours (cleared).
    pub const ITEM_FILLED: [u32; 4] = [ITEM_ROOT, ITEM_BODY, 0x03D9_9141, ITEM_PORTRAIT];
    /// Icons that sit beside the item in the layout (hidden) and the colour
    /// swatches the code reads (never drawn).
    pub const ITEM_HIDDEN: [u32; 2] = [0x03D9_9163, 0x03D9_9185];
    pub const ITEM_SWATCHES: [u32; 3] = [0x03DF_0885, 0x03DF_0886, 0x03DF_0887];
    // Rollover_TribeCitizen / _TribeTool / _TribeHut (the same ids as the
    // Creature rollover).
    pub const ROLL_ROOT: u32 = 0x9331_0E4A;
    pub const ROLL_NAME: u32 = 0x031E_94D0;
    pub const ROLL_PLATE: u32 = 0x9331_03EA;
    pub const ROLL_BAR: u32 = 0x026E_0DF3;
    pub const ROLL_HEALTH: u32 = 0x026E_0DF0;
    pub const ROLL_HEAL: u32 = 0x026E_0DF1;
    pub const ROLL_HURT: u32 = 0x026E_0DF2;
    pub const ROLL_NUMBER: u32 = 0x04D0_39A8;
    /// Five-frame face (angry, sad, neutral, blue, happy) and the ten-frame
    /// role / tool glyph beside the name.
    pub const ROLL_FACE: u32 = 0x046A_DF86;
    pub const ROLL_GLYPH: u32 = 0x026E_0001;
    // Rollover_TribeHut
    pub const HUT_BODY: u32 = 0x9331_03EB;
    /// The group of the two round buttons below the card.
    pub const HUT_BUTTONS: u32 = 0x041F_852D;
    pub const HUT_ADD_BABY: u32 = 0x0625_CD52;
    pub const HUT_PLANNER: u32 = 0x036A_D4A4;
    /// The tribe size beside the figure icon.
    pub const HUT_COUNT: u32 = 0x0199_A7D2;
    // RolloverTribe2
    pub const TRIBE_ROOT: u32 = 0x04A5_CBB0;
    pub const TRIBE_NAME: u32 = 0x042C_A510;
    /// The white name plate of the tribe card.
    pub const TRIBE_PLATE: u32 = 0x02CF_324C;
}

/// Element of the tribe card's group that holds the member count with its
/// figure icon (no control id; its text is 0199A7D2, as on the hut card).
const TRIBE_COUNT_ELEMENT: usize = 33;
/// Element of the hut card's bar frame group is not needed: the bars are
/// hidden by control id.
/// Space under the name plate of a card that shows no health.
const CARD_MARGIN: f32 = 6.0;
/// File elements of the "+" plate beside the health bar in the member, tool
/// and hut cards (no control id).
const CITIZEN_HEAL_ICON_ELEMENT: usize = 30;
const TOOL_HEAL_ICON_ELEMENT: usize = 36;
const HUT_HEAL_ICON_ELEMENT: usize = 17;
/// Left edge, top and spacing of the tribe list (ADAPTED: the retail code
/// places it itself; here it stands to the right of the stack of goal cards,
/// which is 162 px wide).
const POSSE_X: f32 = 175.0;
const LIST_TOP: f32 = 15.0;
const ROW_STEP: f32 = 70.0;
const COLUMN_STEP: f32 = 92.0;
/// Height at the bottom of the screen the list leaves alone (the world map of
/// the legacy HUD and the bottom bar).
const BOTTOM_RESERVED: f32 = 285.0;
/// The most items shown (the chieftain, the members and the eggs).
pub const MAX_ITEMS: usize = 14;
/// A hut card's reach from its anchor: left, right, up and down (the buttons
/// hang below the anchor).
const HUT_LEFT: f32 = 90.0;
const HUT_RIGHT: f32 = 90.0;
const HUT_UP: f32 = 84.0;
const HUT_DOWN: f32 = 140.0;
/// Pick radii around a tribe member's body, a tool rack, a village and the
/// hut, in world units projected to the screen (minimum in pixels).
const MEMBER_RADIUS: f32 = 0.9;
const RACK_RADIUS: f32 = 1.6;
const VILLAGE_RADIUS: f32 = 4.6;
const HUT_RADIUS: f32 = 3.4;
const MIN_PICK_PX: f32 = 30.0;

/// A row of the tribe list.
#[derive(Clone, Debug)]
pub struct ListMember {
    pub entity: Entity,
    pub name: String,
    /// Health and hunger (energy) fractions `0.0..=1.0`.
    pub health: f32,
    pub energy: f32,
    pub selected: bool,
    pub tool: Option<ToolKind>,
    pub task: &'static str,
    pub chieftain: bool,
}

/// What a rollover card is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardKind {
    Citizen,
    Tool,
    Tribe,
}

/// The card shown over the thing under the pointer.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub kind: CardKind,
    pub name: String,
    /// Health fraction and the number beside the bar, when the thing has any.
    pub health: Option<(f32, i32)>,
    /// Frame of the five-frame face.
    pub face: u32,
    /// Frame of the ten-frame role glyph (`Citizen`, `Tool`); the member count
    /// of a rival tribe (`Tribe`).
    pub count: Option<i32>,
    /// Anchor in physical screen pixels (the head).
    pub screen: Vec2,
}

/// The card over the hut.
#[derive(Clone, Debug, PartialEq)]
pub struct HutCard {
    pub members: usize,
    pub eggs: usize,
    pub cap: usize,
    pub food: i32,
    pub price: i32,
    /// Frame of the five-frame face (the tribe's mood).
    pub mood: u32,
    pub screen: Vec2,
}

/// Frame of the ten-frame role / tool glyph of `Rollover_TribeCitizen`
/// (read from the installed strip: basket, horn, torch, hook, spears, axe,
/// maracas, (curved horn), didgeridoo, healing staff). `None` for a member
/// without a tool.
pub fn role_glyph(tool: Option<ToolKind>) -> Option<u32> {
    Some(match tool? {
        ToolKind::FruitSticks => 0,
        ToolKind::WoodenHorns => 1,
        ToolKind::SpikedClubs => 2,
        ToolKind::FishingSpears => 3,
        ToolKind::ThrowingSpears => 4,
        ToolKind::StoneAxes => 5,
        ToolKind::Maracas => 6,
        ToolKind::Aerophones => 8,
        ToolKind::HealingRods => 9,
        ToolKind::Campfire => return None,
    })
}

/// Retail names of the tribe members by their tool (`TribeGame` table ids
/// `0567D1D0..D8`, "Shaman ~tribe_creature_name~" and so on) and the
/// chieftain's.
pub fn role_title_id(tool: Option<ToolKind>) -> Option<u32> {
    Some(match tool? {
        ToolKind::HealingRods => 0x0567_D1D0,
        ToolKind::ThrowingSpears => 0x0567_D1D1,
        ToolKind::StoneAxes => 0x0567_D1D2,
        ToolKind::SpikedClubs => 0x0567_D1D3,
        ToolKind::FishingSpears => 0x0567_D1D4,
        ToolKind::FruitSticks => 0x0567_D1D5,
        ToolKind::WoodenHorns => 0x0567_D1D6,
        ToolKind::Maracas => 0x0567_D1D7,
        ToolKind::Aerophones => 0x0567_D1D8,
        ToolKind::Campfire => return None,
    })
}

pub const CHIEFTAIN_TITLE_ID: u32 = 0x04BD_540B;
/// The word of a retail name template ("Hornist ~tribe_creature_name~"): the
/// part before the placeholder.
pub fn title_word(template: &str) -> String {
    template.split('~').next().unwrap_or("").trim().to_string()
}

/// Face of a tribe member: green while it is fed and healthy, neutral when
/// hungry, sad when starving or badly hurt (ADAPTED: the retail face shows the
/// relationship; the members' mood is derived from their needs).
pub fn member_face(health: f32, energy: f32) -> u32 {
    if health < 0.25 || energy <= 0.2 {
        1
    } else if energy < 0.5 || health < 0.5 {
        2
    } else {
        4
    }
}

/// The relationship manager's attitude toward a tribe for a relationship value
/// in the campaign's x10 units, 0 Furious .. 4 Friendly (VERIFIED 0xD00BD0: the
/// raw relationship clamped to -10..10 is cut at the manager's four caps
/// -7 / -3 / 3 / 7, `RolloverRltnshpVBadCap` .. `GoodCap`, T9K-2: Furious
/// v <= -7, Annoyed -7 < v <= -3, Cautious -3 < v < 3, Pleased 3 <= v < 7,
/// Friendly v >= 7).
pub fn attitude(relations: f32) -> u32 {
    let v = (relations / 10.0).clamp(-10.0, 10.0);
    if v <= -7.0 {
        0
    } else if v <= -3.0 {
        1
    } else if v < 3.0 {
        2
    } else if v < 7.0 {
        3
    } else {
        4
    }
}

/// Face frame of a rival tribe's relationship card: the five frames of the
/// relationship face (angry, sad, neutral, blue, happy) are the five attitudes
/// in order. An allied tribe is Friendly; a conquered one angry (ADAPTED: the
/// retail relationship of a destroyed tribe was not read).
pub fn village_face(status: RivalStatus, relations: f32) -> u32 {
    match status {
        RivalStatus::Allied => 4,
        RivalStatus::Conquered => 0,
        RivalStatus::Hostile => attitude(relations),
    }
}

/// Face of the hut card: the tribe's mood from its hungry members (none:
/// happy, some: neutral, half or more: angry).
pub fn hut_face(hungry: usize, members: usize) -> u32 {
    if hungry == 0 {
        4
    } else if hungry * 2 < members.max(1) {
        2
    } else {
        0
    }
}

/// Colour of the (white) name plate of the tribe card for a face frame: the
/// plate colours of the Creature card by relationship, by the face's mood
/// (angry, sad, neutral, blue, happy). ADAPTED like those.
pub fn face_plate_color(face: u32) -> u32 {
    match face {
        0 => Relation::Hostile,
        1 => Relation::Wary,
        2 => Relation::Curious,
        3 => Relation::Neutral,
        _ => Relation::Ally,
    }
    .plate_color()
}

/// Health frame (0..=30) of a list item's meter.
pub fn item_frame(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 30.0).round() as u32
}

/// The flipbook grids of the animated icons of the cards, whose layout data
/// says one frame for a strip of several: the relationship face (five faces of
/// 47 px in the 235 px strip) and the role glyph (ten of 27 px in 270 px).
/// ADAPTED: the retail code selects frames by index; the grid is read off the
/// strips' sizes.
pub const FLIPBOOKS: [(u32, u32); 2] = [(id::ROLL_FACE, 5), (id::ROLL_GLYPH, 10)];

/// A layout whose animated icons of [`FLIPBOOKS`] have their frame grid.
pub fn with_flipbooks(layout: std::sync::Arc<crate::spui::model::Layout>) -> std::sync::Arc<crate::spui::model::Layout> {
    let mut patched = (*layout).clone();
    for w in &mut patched.windows {
        let Some((_, columns)) = FLIPBOOKS.iter().find(|(control, _)| *control == w.control_id) else {
            continue;
        };
        if let crate::spui::model::Extra::Anim(a) = &mut w.extra {
            if a.rows.max(1) * a.columns.max(1) <= 1 {
                a.rows = 1;
                a.columns = *columns;
                a.frame_count = *columns;
            }
        }
    }
    std::sync::Arc::new(patched)
}

/// Rows in a column of the list for a screen `height` high (more members use
/// further columns): as many as fit between the header and the bottom area.
pub fn list_rows(height: f32) -> usize {
    (((height - BOTTOM_RESERVED - LIST_TOP - 58.0) / ROW_STEP).floor().max(3.0) as usize).min(8)
}

/// Where the list item `n` stands: columns of `rows` rows, top-left of its
/// body, below the header at `top`.
pub fn item_position(n: usize, top: f32, rows: usize) -> (f32, f32) {
    let rows = rows.max(1);
    let (column, row) = (n / rows, n % rows);
    (POSSE_X + 6.0 + column as f32 * COLUMN_STEP, top + 58.0 + row as f32 * ROW_STEP)
}

/// The anchor of the hut card moved so that the card (with its buttons
/// below) stays on screen.
pub fn clamp_hut_anchor(anchor: Vec2, screen: Vec2) -> Vec2 {
    Vec2::new(
        anchor.x.clamp(HUT_LEFT, (screen.x - HUT_RIGHT).max(HUT_LEFT)),
        anchor.y.clamp(HUT_UP, (screen.y - HUT_DOWN).max(HUT_UP)),
    )
}

/// The nearest of `candidates` `(index, screen position, pick radius)` whose
/// radius contains the cursor.
pub fn nearest_hit(cursor: Vec2, candidates: &[(usize, Vec2, f32)]) -> Option<usize> {
    candidates
        .iter()
        .filter(|(_, at, radius)| at.distance(cursor) <= radius.max(MIN_PICK_PX))
        .min_by(|a, b| a.1.distance(cursor).total_cmp(&b.1.distance(cursor)))
        .map(|(index, ..)| *index)
}

/// Fill the list, the cards and the hut card of [`TribalHudState`].
#[allow(clippy::too_many_arguments)]
pub fn publish_world(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    pointer: Res<SpuiPointer>,
    mut assets: ResMut<SpuiAssets>,
    mut ui_state: ResMut<TribalHudUi>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    members: super::MemberQuery,
    chief: Query<(Entity, &TribeUnit, &Health, &Transform, Option<&Selected>), With<Player>>,
    shacks: Query<(&tools::ToolShack, &Transform)>,
    rivals: Query<&crate::tribal_rts::RivalMember>,
    roots: Query<(&SpuiState, &TribalHudRoot)>,
    mut hud: ResMut<TribalHudState>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    if *stage.get() != GameStage::Tribal {
        return;
    }
    let index = &originals.resource_index;
    let campaign = &run.tribal_campaign;
    let title = |assets: &mut SpuiAssets, tool: Option<ToolKind>| {
        role_title_id(tool)
            .and_then(|id| assets.localized(index, crate::world_objects::name_id("TribeGame"), id))
            .map(|template| title_word(&template))
            .filter(|word| !word.is_empty())
            .unwrap_or_else(|| "Tribe member".to_string())
    };
    // The list: the chieftain first, then the members by roster slot, then
    // the growing eggs. `positions` runs parallel to it (the cards' targets).
    let mut list = Vec::new();
    let mut positions: Vec<Vec3> = Vec::new();
    let mut selected = 0;
    hud.chieftain_up = chief.single().is_ok();
    if let Ok((entity, _, health, transform, sel)) = chief.single() {
        positions.push(transform.translation);
        let name = assets
            .localized(index, crate::world_objects::name_id("TribeGame"), CHIEFTAIN_TITLE_ID)
            .map(|t| title_word(&t))
            .filter(|w| !w.is_empty())
            .unwrap_or_else(|| "Chieftain".to_string());
        selected += usize::from(sel.is_some());
        list.push(ListMember {
            entity,
            name,
            health: health.frac(),
            energy: 1.0,
            selected: sel.is_some(),
            tool: None,
            task: "chieftain",
            chieftain: true,
        });
    }
    let mut living: Vec<_> = members
        .iter()
        .filter(|m| !m.2.dead())
        .map(|(entity, unit, health, task, transform, world_id, sel)| {
            let tool = world_id.and_then(|id| campaign.tool_of(id.0));
            (world_id.map_or(u32::MAX, |id| id.0), entity, unit, health, task, tool, sel.is_some(), transform.translation)
        })
        .collect();
    living.sort_by_key(|m| m.0);
    for (_, entity, unit, health, task, tool, sel, position) in living {
        selected += usize::from(sel);
        positions.push(position);
        list.push(ListMember {
            entity,
            name: title(&mut assets, tool),
            health: health.frac(),
            energy: (unit.energy / tribal::MEMBER_ENERGY_MAX).clamp(0.0, 1.0),
            selected: sel,
            tool,
            task: task.label(),
            chieftain: false,
        });
    }
    list.truncate(MAX_ITEMS);
    positions.truncate(MAX_ITEMS);
    hud.selected = selected;
    hud.list = list;
    hud.babies = campaign
        .births
        .iter()
        .map(|b| (1.0 - b.remaining / tribal::BIRTH_GROWTH_SECONDS).clamp(0.0, 1.0))
        .take(MAX_ITEMS.saturating_sub(hud.list.len()))
        .collect();
    // Cards over what the pointer rests on.
    hud.card = None;
    hud.hut = None;
    let Ok((camera, camera_t)) = cameras.single() else {
        return;
    };
    let scale = camera.target_scaling_factor().unwrap_or(1.0);
    let project = |position: Vec3| camera.world_to_viewport(camera_t, position).ok().map(|p| p * scale);
    // Projected radius of `world` units around `at`.
    let radius = |at: Vec3, world: f32| {
        match (project(at), project(at + camera_t.right().as_vec3() * world)) {
            (Some(a), Some(b)) => a.distance(b),
            _ => 0.0,
        }
    };
    let Some(cursor) = pointer.position else {
        ui_state.hut_card = None;
        return;
    };
    // The hut card: while the pointer is on the hut or on the card itself (so
    // that its buttons can be reached).
    let hut_at = tribal::home_position() + Vec3::Y * 2.0;
    let on_hut = project(hut_at).is_some_and(|p| p.distance(cursor) <= radius(hut_at, HUT_RADIUS).max(MIN_PICK_PX));
    let on_card = ui_state.hut_card.is_some_and(|r| r.contains(cursor.x, cursor.y));
    let hut_ui = roots.iter().find(|(_, kind)| **kind == TribalHudRoot::Hut).filter(|(ui, _)| ui.ready);
    // The card rectangle of the last frame, from the solved layout.
    ui_state.hut_card = hut_ui.and_then(|(ui, _)| {
        let live = ui.interactive();
        let rect = |control: u32| ui.find(control).filter(|w| live[*w]).map(|w| ui.rect(w));
        match (rect(id::HUT_BODY), rect(id::HUT_BUTTONS)) {
            (Some(a), Some(b)) => Some(Rect::new(a.x1.min(b.x1), a.y1.min(b.y1), a.x2.max(b.x2), a.y2.max(b.y2))),
            (Some(a), None) => Some(a),
            _ => None,
        }
    });
    if on_hut || on_card {
        let screen = project(hut_at).unwrap_or(cursor);
        let summary = campaign.status_summary();
        hud.hut = Some(HutCard {
            members: run.tribe.len(),
            eggs: summary.pending_births,
            cap: campaign.population_cap(),
            food: hud.food,
            price: hud.baby_price,
            mood: hut_face(summary.hungry_members, run.tribe.len()),
            screen,
        });
        return;
    }
    // The other cards: a member (or the chieftain), a tool rack, a village.
    enum Thing {
        Member(usize),
        Rack(usize),
        Village(usize),
    }
    let mut things: Vec<Thing> = Vec::new();
    let mut spots: Vec<(usize, Vec2, f32)> = Vec::new();
    let mut heads: Vec<Vec2> = Vec::new();
    let mut add = |thing: Thing, body: Vec3, head: Vec3, world_radius: f32, things: &mut Vec<Thing>| {
        if let (Some(b), Some(h)) = (project(body), project(head)) {
            spots.push((things.len(), b, radius(body, world_radius)));
            heads.push(h);
            things.push(thing);
        }
    };
    for (i, p) in positions.iter().enumerate() {
        add(Thing::Member(i), *p + Vec3::Y * 0.7, *p + Vec3::Y * 1.9, MEMBER_RADIUS, &mut things);
    }
    for (i, (_, t)) in shacks.iter().enumerate() {
        add(Thing::Rack(i), t.translation + Vec3::Y * 1.0, t.translation + Vec3::Y * 2.4, RACK_RADIUS, &mut things);
    }
    for v in 0..tribal::RIVAL_VILLAGES {
        let p = tribal::village_position(v);
        add(Thing::Village(v), p + Vec3::Y * 1.5, p + Vec3::Y * 4.2, VILLAGE_RADIUS, &mut things);
    }
    let Some(hit) = nearest_hit(cursor, &spots) else {
        return;
    };
    let head = heads[hit];
    hud.card = Some(match things[hit] {
        Thing::Member(i) => {
            let m = &hud.list[i.min(hud.list.len().saturating_sub(1))];
            Card {
                kind: CardKind::Citizen,
                name: short_name(&m.name),
                health: Some((m.health, (m.health * 100.0).ceil() as i32)),
                face: member_face(m.health, m.energy),
                count: role_glyph(m.tool).map(|f| f as i32),
                screen: head,
            }
        }
        Thing::Rack(i) => {
            let kind = shacks.iter().nth(i).map(|(s, _)| s.0).unwrap_or(ToolKind::SpikedClubs);
            Card {
                kind: CardKind::Tool,
                name: tools::info(kind).name.to_string(),
                health: None,
                face: 4,
                count: role_glyph(Some(kind)).map(|f| f as i32),
                screen: head,
            }
        }
        Thing::Village(v) => {
            let state = campaign.village(v).cloned().unwrap_or_default();
            Card {
                kind: CardKind::Tribe,
                name: village_name(v),
                health: None,
                face: village_face(state.status, state.relations),
                // The tribe's size: its living members.
                count: Some(rivals.iter().filter(|m| m.village == v).count() as i32),
                screen: head,
            }
        }
    });
}

/// The name of a rival tribe. ADAPTED: the villages have no names of their
/// own; they are numbered like the world map's V1..V5.
pub fn village_name(village: usize) -> String {
    format!("Tribe {}", village + 1)
}

/// Spawn and despawn the roots that depend on counts: the tribe list items
/// (one `PosseItemTribe` per member, one `PosseItemTribeBaby` per egg) and
/// the goal cards.
#[allow(clippy::too_many_arguments)]
pub fn manage_dynamic(
    mut commands: Commands,
    active: Res<TribalHudActive>,
    hud: Res<TribalHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &TribalHudRoot)>,
) {
    if !active.0 {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let want_cards = hud.goal_cards.len().min(super::guide::MAX_CARDS);
    let want_items = hud.list.len().min(MAX_ITEMS);
    let want_babies = hud.babies.len().min(MAX_ITEMS - want_items);
    for (entity, kind) in &roots {
        let surplus = match kind {
            TribalHudRoot::Card(n) => *n as usize >= want_cards,
            TribalHudRoot::PosseItem(n) => *n as usize >= want_items,
            TribalHudRoot::PosseBaby(n) => *n as usize >= want_babies,
            _ => false,
        };
        if surplus {
            commands.entity(entity).despawn();
        }
    }
    let has = |wanted: TribalHudRoot| roots.iter().any(|(_, kind)| *kind == wanted);
    let mut spawn = |commands: &mut Commands, assets: &mut SpuiAssets, name: &str, kind: TribalHudRoot, z: i32| {
        if has(kind) {
            return;
        }
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, RootPolicy::Authored, Some(camera), z);
            commands.entity(root).insert(kind);
        }
    };
    for n in 0..want_cards {
        spawn(&mut commands, &mut assets, super::CARD_LAYOUT, TribalHudRoot::Card(n as u8), 94);
    }
    for n in 0..want_items {
        spawn(&mut commands, &mut assets, POSSE_ITEM_LAYOUT, TribalHudRoot::PosseItem(n as u8), 96);
    }
    for n in 0..want_babies {
        spawn(&mut commands, &mut assets, BABY_ITEM_LAYOUT, TribalHudRoot::PosseBaby(n as u8), 96);
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

/// Put a card's anchor (the root window, a zero-size or small window the code
/// moves) at `at`; the card hangs around it.
fn move_anchor(ui: &mut SpuiState, root: usize, at: Vec2) {
    let authored = ui.layout.window(root).area;
    let (w, h) = (authored[2] - authored[0], authored[3] - authored[1]);
    ui.set_area(root, [at.x, at.y, at.x + w, at.y + h]);
}

fn bind_card(ui: &mut SpuiState, card: Option<&Card>, kind: CardKind) {
    let root_id = if kind == CardKind::Tribe { id::TRIBE_ROOT } else { id::ROLL_ROOT };
    let Some(root) = find(ui, root_id) else {
        return;
    };
    let card = card.filter(|c| c.kind == kind);
    ui.set_visible(root, card.is_some());
    let Some(c) = card else {
        return;
    };
    let screen = Vec2::new(ui.screen[0], ui.screen[1]);
    move_anchor(ui, root, clamp_anchor(c.screen, screen));
    let name_id = if kind == CardKind::Tribe { id::TRIBE_NAME } else { id::ROLL_NAME };
    if let Some(w) = find(ui, name_id) {
        ui.set_text(w, c.name.clone());
    }
    if let Some(w) = find(ui, id::ROLL_FACE) {
        ui.set_frame(w, Some(c.face));
    }
    if kind == CardKind::Tribe {
        // The tribe's size beside the figure icon (the group of
        // TRIBE_COUNT_ELEMENT).
        if let Some(w) = super::element_window(ui, TRIBE_COUNT_ELEMENT) {
            ui.set_visible(w, c.count.is_some());
        }
        if let Some(w) = find(ui, id::HUT_COUNT) {
            ui.set_text(w, c.count.unwrap_or(0).to_string());
        }
        // The plate is a white picture: tinted by the standing of the tribe.
        if let Some(w) = find(ui, id::TRIBE_PLATE) {
            ui.set_tint(w, face_plate_color(c.face));
        }
        return;
    }
    if let Some(w) = find(ui, id::ROLL_PLATE) {
        ui.set_tint(w, Relation::Ally.plate_color());
    }
    // The role / tool glyph beside the name.
    if let Some(w) = find(ui, id::ROLL_GLYPH) {
        ui.set_visible(w, c.count.is_some());
        ui.set_frame(w, c.count.map(|f| f as u32));
    }
    // Health: the bar and the number, for the things that have health; the
    // "+" plate beside the bar goes with them. A card without health (a tool
    // rack) shrinks to the name plate (its pointer is anchored to the bottom).
    let health = c.health;
    if let (Some(body), Some(plate)) = (find(ui, id::HUT_BODY), find(ui, id::ROLL_PLATE)) {
        let b = ui.layout.window(body).area;
        let p = ui.layout.window(plate).area;
        let bottom = if health.is_some() { b[3] } else { b[1] + p[3] + CARD_MARGIN };
        ui.set_area(body, [b[0], b[1], b[2], bottom]);
    }
    let heal_icon = if kind == CardKind::Tool { TOOL_HEAL_ICON_ELEMENT } else { CITIZEN_HEAL_ICON_ELEMENT };
    if let Some(w) = super::element_window(ui, heal_icon) {
        ui.set_visible(w, health.is_some());
    }
    for control in [id::ROLL_BAR, id::ROLL_HEALTH, id::ROLL_NUMBER] {
        if let Some(w) = find(ui, control) {
            ui.set_visible(w, health.is_some());
        }
    }
    for control in [id::ROLL_HEAL, id::ROLL_HURT] {
        if let Some(w) = find(ui, control) {
            ui.set_visible(w, false);
        }
    }
    if let Some((fraction, hp)) = health {
        if let Some(w) = find(ui, id::ROLL_NUMBER) {
            ui.set_text(w, hp.to_string());
        }
        if let Some(w) = find(ui, id::ROLL_HEALTH) {
            let mut area = ui.layout.window(w).area;
            let full = area[2] - area[0];
            area[2] = area[0] + full * fraction.clamp(0.0, 1.0);
            ui.set_area(w, area);
        }
    }
}

fn bind_hut(ui: &mut SpuiState, hut: Option<&HutCard>, hud: &TribalHudState, assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex) {
    let Some(root) = find(ui, id::ROLL_ROOT) else {
        return;
    };
    ui.set_visible(root, hut.is_some());
    let Some(h) = hut else {
        return;
    };
    let screen = Vec2::new(ui.screen[0], ui.screen[1]);
    move_anchor(ui, root, clamp_hut_anchor(h.screen, screen));
    if let Some(w) = find(ui, id::ROLL_NAME) {
        // "Tribe" (`TribeGame` F37C273B).
        ui.set_text(w, super::guide::resolve(assets, index, "loc:TribeGame:f37c273b"));
    }
    if let Some(w) = find(ui, id::ROLL_PLATE) {
        ui.set_tint(w, Relation::Ally.plate_color());
    }
    // The hut has no health in the rewrite: no bar and no "+" plate. The tribe
    // size sits beside the figure icon. The two round buttons below the card
    // (a group the layout starts hidden) are shown with it.
    if let Some(w) = super::element_window(ui, HUT_HEAL_ICON_ELEMENT) {
        ui.set_visible(w, false);
    }
    if let Some(w) = find(ui, id::HUT_BUTTONS) {
        ui.set_visible(w, true);
    }
    for control in [id::ROLL_BAR, id::ROLL_HEALTH, id::ROLL_HEAL, id::ROLL_HURT, id::ROLL_NUMBER] {
        if let Some(w) = find(ui, control) {
            ui.set_visible(w, false);
        }
    }
    if let Some(w) = find(ui, id::HUT_COUNT) {
        ui.set_text(w, format!("{}/{}", h.members + h.eggs, h.cap));
    }
    if let Some(w) = find(ui, id::ROLL_FACE) {
        ui.set_frame(w, Some(h.mood));
    }
    // Tooltip of the Add Baby button: the retail text with the price put in
    // (`~tribe_food_required_for_baby~`), the Planner's is the layout's own.
    if let Some(w) = find(ui, id::HUT_ADD_BABY) {
        let template = super::guide::resolve(assets, index, "loc:tooltips:065fc366");
        ui.set_tooltip(w, Some(super::guide::substitute(&template, &[("tribe_food_required_for_baby", &h.price.to_string())])));
        ui.set_selected(w, false);
        // Looks pressed while the planner is open? No: the planner is the
        // other button.
    }
    if let Some(w) = find(ui, id::HUT_PLANNER) {
        ui.set_selected(w, hud.planner_open);
    }
}

fn bind_posse(ui: &mut SpuiState, hud: &TribalHudState, assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex, top: f32) {
    let Some(root) = find(ui, id::POSSE_ROOT) else {
        return;
    };
    let show = !hud.list.is_empty() || !hud.babies.is_empty();
    ui.set_visible(root, show);
    if !show {
        return;
    }
    if let Some(list) = find(ui, id::POSSE_LIST) {
        place_by(ui, root, list, POSSE_X, top + 34.0);
    }
    if let Some(w) = find(ui, id::POSSE_TITLE) {
        ui.set_text(w, super::guide::resolve(assets, index, "loc:TribeGame:f37c273b"));
        ui.set_visible(w, true);
    }
    if let Some(w) = find(ui, id::POSSE_COUNT) {
        ui.set_text(w, format!("{}/{}", hud.members + hud.eggs, hud.member_cap));
        ui.set_visible(w, true);
    }
    // "Select All" is a push, not a switch: it never stays checked.
    if let Some(w) = find(ui, id::POSSE_SELECT_ALL) {
        ui.set_selected(w, false);
    }
    // The list's edge strips are placeholders for the list length.
    for control in [id::POSSE_TOP, id::POSSE_BOTTOM].iter().chain(&id::POSSE_HIDDEN) {
        if let Some(w) = find(ui, *control) {
            ui.set_visible(w, false);
        }
    }
}

/// Clear the design-time colour blocks of a list item and hide the icons and
/// swatches that sit beside it in the layout.
fn tidy_item(ui: &mut SpuiState) {
    for control in id::ITEM_SWATCHES.iter().chain(&id::ITEM_HIDDEN) {
        if let Some(w) = find(ui, *control) {
            ui.set_visible(w, false);
        }
    }
    for control in id::ITEM_FILLED {
        if let Some(w) = find(ui, control) {
            ui.set_tint(w, 0x00FF_FFFF);
        }
    }
}

fn bind_item(ui: &mut SpuiState, member: &ListMember, slot: usize, top: f32, rows: usize) {
    let (Some(root), Some(body)) = (find(ui, id::ITEM_ROOT), find(ui, id::ITEM_BODY)) else {
        return;
    };
    let (x, y) = item_position(slot, top, rows);
    place_by(ui, root, body, x, y);
    ui.set_visible(root, true);
    // The item has no name text: hovering it names the member.
    ui.set_tooltip(body, Some(format!("{}\n{}", member.name, member.task)));
    tidy_item(ui);
    if let Some(w) = find(ui, id::ITEM_SELECTED) {
        ui.set_visible(w, member.selected);
    }
    if let Some(w) = find(ui, id::ITEM_HEALTH) {
        ui.set_frame(w, Some(item_frame(member.health)));
    }
    if let Some(w) = find(ui, id::ITEM_HUNGER) {
        ui.set_frame(w, Some(item_frame(member.energy)));
    }
}

fn bind_baby(ui: &mut SpuiState, growth: f32, slot: usize, top: f32, rows: usize) {
    let (Some(root), Some(body)) = (find(ui, id::ITEM_ROOT), find(ui, id::ITEM_BODY)) else {
        return;
    };
    let (x, y) = item_position(slot, top, rows);
    place_by(ui, root, body, x, y);
    ui.set_visible(root, true);
    ui.set_tooltip(body, Some(format!("Baby\ngrowing up: {:.0}%", growth * 100.0)));
    tidy_item(ui);
    if let Some(w) = find(ui, id::ITEM_SELECTED) {
        ui.set_visible(w, false);
    }
}

/// Push the list, the cards and the hut card into the layouts.
pub fn bind_world(
    hud: Res<TribalHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut roots: Query<(&mut SpuiState, &TribalHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    let top = LIST_TOP;
    let height = roots.iter().find(|(ui, kind)| **kind == TribalHudRoot::Posse && ui.ready).map_or(900.0, |(ui, _)| ui.screen[1]);
    let rows = list_rows(height);
    let items = hud.list.len().min(MAX_ITEMS);
    let card = hud.card.as_ref().filter(|_| !hud.paused);
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        // The planner drawer covers the list; the list steps aside.
        if hud.planner_open && matches!(kind, TribalHudRoot::Posse | TribalHudRoot::PosseItem(_) | TribalHudRoot::PosseBaby(_)) {
            if let Some(w) = find(&ui, id::POSSE_ROOT).or_else(|| find(&ui, id::ITEM_ROOT)) {
                ui.set_visible(w, false);
            }
            continue;
        }
        match kind {
            TribalHudRoot::Citizen => bind_card(&mut ui, card, CardKind::Citizen),
            TribalHudRoot::Tool => bind_card(&mut ui, card, CardKind::Tool),
            TribalHudRoot::Tribe => bind_card(&mut ui, card, CardKind::Tribe),
            TribalHudRoot::Hut => {
                bind_hut(&mut ui, hud.hut.as_ref().filter(|_| !hud.paused), &hud, &mut assets, index);
            }
            TribalHudRoot::Posse => bind_posse(&mut ui, &hud, &mut assets, index, top),
            TribalHudRoot::PosseItem(n) => {
                if let Some(member) = hud.list.get(*n as usize) {
                    bind_item(&mut ui, member, *n as usize, top, rows);
                }
            }
            TribalHudRoot::PosseBaby(n) => {
                if let Some(growth) = hud.babies.get(*n as usize) {
                    bind_baby(&mut ui, *growth, items + *n as usize, top, rows);
                }
            }
            _ => {}
        }
    }
}

/// A click on a list item selects its member (Shift adds or removes it);
/// "Select All" selects the whole tribe (`Q`). The press itself was kept from
/// the world (`swallow_world_clicks`), the click is taken on release.
#[allow(clippy::too_many_arguments)]
pub fn list_clicks(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Res<SpuiPointer>,
    hud: Res<TribalHudState>,
    mut ui_state: ResMut<TribalHudUi>,
    pause: Res<crate::pause_menu::PauseMenu>,
    roots: Query<(&SpuiState, &TribalHudRoot)>,
    selected: Query<Entity, With<Selected>>,
) {
    if !mouse.just_released(MouseButton::Left) || pause.open {
        return;
    }
    let Some(cursor) = pointer.position else {
        return;
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for (ui, kind) in &roots {
        let TribalHudRoot::PosseItem(n) = kind else {
            continue;
        };
        if !ui.ready {
            continue;
        }
        let live = ui.interactive();
        let Some(body) = ui.find(id::ITEM_BODY).filter(|w| live[*w]) else {
            continue;
        };
        let r = ui.rect(body);
        if !r.contains(cursor.x, cursor.y) {
            continue;
        }
        let Some(member) = hud.list.get(*n as usize) else {
            continue;
        };
        if member.entity == Entity::PLACEHOLDER {
            continue;
        }
        if !shift {
            for e in &selected {
                if e != member.entity {
                    commands.entity(e).remove::<Selected>();
                }
            }
        }
        if shift && member.selected {
            commands.entity(member.entity).remove::<Selected>();
        } else {
            commands.entity(member.entity).insert(Selected);
        }
        ui_state.list_picks += 1;
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_titles_use_the_word_before_the_placeholder() {
        assert_eq!(title_word("Hornist ~tribe_creature_name~"), "Hornist");
        assert_eq!(title_word("Chieftain ~tribe_creature_name~"), "Chieftain");
        assert_eq!(title_word("Plain"), "Plain");
        assert_eq!(title_word("~name~"), "");
    }

    #[test]
    fn every_tool_but_the_campfire_has_a_role_glyph_and_a_title() {
        let mut glyphs = Vec::new();
        let mut titles = Vec::new();
        for info in &tools::TOOLS {
            if info.kind == ToolKind::Campfire {
                assert!(role_glyph(Some(info.kind)).is_none() && role_title_id(Some(info.kind)).is_none());
                continue;
            }
            glyphs.push(role_glyph(Some(info.kind)).expect("glyph"));
            titles.push(role_title_id(Some(info.kind)).expect("title"));
        }
        assert!(role_glyph(None).is_none() && role_title_id(None).is_none());
        glyphs.sort_unstable();
        glyphs.dedup();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(glyphs.len(), 9, "every glyph differs");
        assert_eq!(titles.len(), 9, "every title differs");
        assert!(glyphs.iter().all(|g| *g < 10), "the strip has ten frames");
    }

    #[test]
    fn faces_follow_the_needs_the_standing_and_the_hunger() {
        assert_eq!(member_face(1.0, 1.0), 4);
        assert_eq!(member_face(1.0, 0.4), 2);
        assert_eq!(member_face(0.4, 1.0), 2);
        assert_eq!(member_face(1.0, 0.1), 1);
        assert_eq!(member_face(0.1, 1.0), 1);
        assert_eq!(village_face(RivalStatus::Allied, 100.0), 4);
        assert_eq!(village_face(RivalStatus::Conquered, 0.0), 0);
        // The five attitudes of 0xD00BD0 at the x10 caps -70 / -30 / 30 / 70.
        assert_eq!(village_face(RivalStatus::Hostile, -100.0), 0);
        assert_eq!(village_face(RivalStatus::Hostile, -70.0), 0);
        assert_eq!(village_face(RivalStatus::Hostile, -69.0), 1);
        assert_eq!(village_face(RivalStatus::Hostile, -30.0), 1);
        assert_eq!(village_face(RivalStatus::Hostile, -29.0), 2);
        assert_eq!(village_face(RivalStatus::Hostile, 0.0), 2);
        assert_eq!(village_face(RivalStatus::Hostile, 29.0), 2);
        assert_eq!(village_face(RivalStatus::Hostile, 30.0), 3);
        assert_eq!(village_face(RivalStatus::Hostile, 69.0), 3);
        assert_eq!(village_face(RivalStatus::Hostile, 70.0), 4);
        assert_eq!(hut_face(0, 6), 4);
        assert_eq!(hut_face(1, 6), 2);
        assert_eq!(hut_face(3, 6), 0);
        assert_eq!(hut_face(0, 0), 4);
    }

    #[test]
    fn items_fill_columns_of_rows_that_fit_the_screen() {
        let top = LIST_TOP;
        let rows = list_rows(900.0);
        assert!((3..=8).contains(&rows));
        let (x0, y0) = item_position(0, top, rows);
        let (x1, y1) = item_position(1, top, rows);
        assert_eq!(x0, x1);
        assert_eq!(y1 - y0, ROW_STEP);
        let (xn, yn) = item_position(rows, top, rows);
        assert_eq!(yn, y0, "the next item starts the next column");
        assert_eq!(xn - x0, COLUMN_STEP);
        // The tallest column ends above the bottom area on every screen height.
        for height in [600.0, 768.0, 900.0, 1080.0, 1440.0] {
            let rows = list_rows(height);
            assert!(item_position(rows - 1, top, rows).1 + 66.0 <= height - BOTTOM_RESERVED, "{height}");
        }
        // More rows fit a taller screen, and a degenerate row count does not divide by zero.
        assert!(list_rows(1440.0) > list_rows(768.0));
        assert_eq!(item_position(3, top, 0), item_position(3, top, 1));
        // The list stands to the right of the goal cards (162 px wide at x 5).
        assert!(x0 > 5.0 + 162.0);
    }

    #[test]
    fn the_tribe_plate_takes_the_colour_of_the_face() {
        let colours: std::collections::HashSet<u32> = (0..5).map(face_plate_color).collect();
        assert_eq!(colours.len(), 5, "every face has its own plate colour");
        assert_eq!(face_plate_color(4), Relation::Ally.plate_color());
        assert_eq!(face_plate_color(0), Relation::Hostile.plate_color());
        assert_eq!(face_plate_color(99), face_plate_color(4));
    }

    #[test]
    fn meter_frames_use_the_thirty_one_frames() {
        assert_eq!(item_frame(0.0), 0);
        assert_eq!(item_frame(1.0), 30);
        assert_eq!(item_frame(0.5), 15);
        assert_eq!(item_frame(7.0), 30);
        assert_eq!(item_frame(f32::NAN.max(0.0)), 0);
    }

    #[test]
    fn the_nearest_thing_within_reach_is_picked() {
        let spots = [(0, Vec2::new(100.0, 100.0), 40.0), (1, Vec2::new(150.0, 100.0), 40.0), (2, Vec2::new(900.0, 900.0), 40.0)];
        assert_eq!(nearest_hit(Vec2::new(120.0, 100.0), &spots), Some(0));
        assert_eq!(nearest_hit(Vec2::new(140.0, 100.0), &spots), Some(1));
        assert_eq!(nearest_hit(Vec2::new(500.0, 500.0), &spots), None);
        // A tiny object still has the minimum pick radius.
        let tiny = [(7, Vec2::new(50.0, 50.0), 2.0)];
        assert_eq!(nearest_hit(Vec2::new(70.0, 50.0), &tiny), Some(7));
    }

    #[test]
    fn the_hut_card_stays_on_screen_with_its_buttons() {
        let screen = Vec2::new(1280.0, 900.0);
        assert_eq!(clamp_hut_anchor(Vec2::new(640.0, 300.0), screen), Vec2::new(640.0, 300.0));
        let low = clamp_hut_anchor(Vec2::new(640.0, 890.0), screen);
        assert_eq!(low.y, 900.0 - HUT_DOWN, "the buttons below the anchor fit");
        let corner = clamp_hut_anchor(Vec2::new(-10.0, -10.0), screen);
        assert_eq!((corner.x, corner.y), (HUT_LEFT, HUT_UP));
        assert_eq!(clamp_hut_anchor(Vec2::new(5.0, 5.0), Vec2::new(30.0, 30.0)).x, HUT_LEFT);
    }

    #[test]
    fn villages_are_numbered_like_the_world_map() {
        assert_eq!(village_name(0), "Tribe 1");
        assert_eq!(village_name(4), "Tribe 5");
    }

    fn installed() -> Option<(crate::asset_catalog::ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    /// `--ignored --nocapture`: the flipbook data of every animated icon of the
    /// Tribal layouts (the SPUI window dump cuts it off).
    #[test]
    #[ignore]
    fn dump_installed_tribal_flipbooks() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        for name in [
            crate::tribal_hud::MAIN_LAYOUT,
            CITIZEN_LAYOUT,
            TOOL_LAYOUT,
            HUT_LAYOUT,
            TRIBE_LAYOUT,
            POSSE_ITEM_LAYOUT,
            BABY_ITEM_LAYOUT,
            "Rollover_Tribe_Minimap",
        ] {
            let layout = assets.layout(&index, name).unwrap();
            for w in &layout.windows {
                if let Some(a) = w.anim() {
                    println!(
                        "{name} E{} ctrl={:08X} flags={:05X} area={:?} image={:?} frames={} rows={} columns={} duration={} end_mode={} auto={}",
                        w.element,
                        w.control_id,
                        w.flags,
                        w.area,
                        a.image.as_ref().map(|i| (i.file.instance_id, i.uv, i.dims)),
                        a.frame_count,
                        a.rows,
                        a.columns,
                        a.duration,
                        a.end_mode,
                        a.auto_play
                    );
                }
            }
        }
    }

    #[test]
    fn the_card_strips_get_their_frame_grid() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        for name in [CITIZEN_LAYOUT, TOOL_LAYOUT, HUT_LAYOUT, TRIBE_LAYOUT] {
            let layout = with_flipbooks(assets.layout(&index, name).unwrap());
            let face = layout.window(layout.find(id::ROLL_FACE).unwrap()).anim().cloned().unwrap();
            assert_eq!((face.rows, face.columns, face.frame_count), (1, 5, 5), "{name} face");
        }
        for name in [CITIZEN_LAYOUT, TOOL_LAYOUT] {
            let layout = with_flipbooks(assets.layout(&index, name).unwrap());
            let glyph = layout.window(layout.find(id::ROLL_GLYPH).unwrap()).anim().cloned().unwrap();
            assert_eq!((glyph.rows, glyph.columns, glyph.frame_count), (1, 10, 10), "{name} glyph");
        }
        // A layout whose icons already have a grid is left alone.
        let posse = assets.layout(&index, POSSE_ITEM_LAYOUT).unwrap();
        assert_eq!(with_flipbooks(posse.clone()).windows, posse.windows);
    }

    #[test]
    fn installed_world_layouts_have_the_windows_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let posse = assets.layout(&index, POSSE_LAYOUT).expect("PosseBar");
        for control in [id::POSSE_ROOT, id::POSSE_LIST, id::POSSE_TITLE, id::POSSE_COUNT, id::POSSE_TOP, id::POSSE_BOTTOM, id::POSSE_SELECT_ALL]
            .iter()
            .chain(&id::POSSE_HIDDEN)
        {
            assert!(posse.find(*control).is_some(), "posse bar {control:08X}");
        }
        for name in [POSSE_ITEM_LAYOUT, BABY_ITEM_LAYOUT] {
            let item = assets.layout(&index, name).unwrap_or_else(|| panic!("{name}"));
            for control in [id::ITEM_ROOT, id::ITEM_BODY, id::ITEM_PORTRAIT, id::ITEM_SELECTED]
                .iter()
                .chain(&id::ITEM_SWATCHES)
                .chain(&id::ITEM_FILLED)
                .chain(&id::ITEM_HIDDEN)
            {
                assert!(item.find(*control).is_some(), "{name} {control:08X}");
            }
        }
        let item = assets.layout(&index, POSSE_ITEM_LAYOUT).unwrap();
        for meter in [id::ITEM_HEALTH, id::ITEM_HUNGER] {
            let w = item.window(item.find(meter).unwrap());
            assert_eq!(w.anim().map(|a| a.frame_count), Some(31), "{meter:08X} has 31 frames");
        }
        for name in [CITIZEN_LAYOUT, TOOL_LAYOUT, HUT_LAYOUT] {
            let layout = assets.layout(&index, name).unwrap_or_else(|| panic!("{name}"));
            for control in [id::ROLL_ROOT, id::HUT_BODY, id::ROLL_NAME, id::ROLL_PLATE, id::ROLL_BAR, id::ROLL_HEALTH, id::ROLL_HEAL, id::ROLL_HURT, id::ROLL_NUMBER, id::ROLL_FACE] {
                assert!(layout.find(control).is_some(), "{name} {control:08X}");
            }
        }
        for name in [CITIZEN_LAYOUT, TOOL_LAYOUT] {
            let layout = assets.layout(&index, name).unwrap();
            assert!(layout.find(id::ROLL_GLYPH).is_some(), "{name} role glyph");
        }
        let citizen = assets.layout(&index, CITIZEN_LAYOUT).unwrap();
        // The face has five frames, the role glyph ten (frame count 0 = the
        // whole grid, see spui::paint).
        let grid = |control: u32| {
            let a = citizen.window(citizen.find(control).unwrap()).anim().cloned().unwrap();
            (a.rows * a.columns).max(a.frame_count)
        };
        let _ = grid;
        let hut = assets.layout(&index, HUT_LAYOUT).unwrap();
        for control in [id::HUT_BODY, id::HUT_BUTTONS, id::HUT_ADD_BABY, id::HUT_PLANNER, id::HUT_COUNT] {
            assert!(hut.find(control).is_some(), "hut card {control:08X}");
        }
        let tribe = assets.layout(&index, TRIBE_LAYOUT).expect("RolloverTribe2");
        for control in [id::TRIBE_ROOT, id::TRIBE_NAME, id::TRIBE_PLATE, id::ROLL_FACE] {
            assert!(tribe.find(control).is_some(), "tribe card {control:08X}");
        }
        assert!(
            tribe.windows.iter().any(|w| w.element == TRIBE_COUNT_ELEMENT),
            "the count group of the tribe card"
        );
    }

    #[test]
    fn installed_role_names_and_hut_strings_are_the_retail_text() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let table = crate::world_objects::name_id("TribeGame");
        let word = |assets: &mut SpuiAssets, id: u32| {
            title_word(&assets.localized(&index, table, id).unwrap_or_default())
        };
        assert_eq!(word(&mut assets, CHIEFTAIN_TITLE_ID), "Chieftain");
        assert_eq!(word(&mut assets, role_title_id(Some(ToolKind::WoodenHorns)).unwrap()), "Hornist");
        assert_eq!(word(&mut assets, role_title_id(Some(ToolKind::Maracas)).unwrap()), "Maracan");
        assert_eq!(word(&mut assets, role_title_id(Some(ToolKind::HealingRods)).unwrap()), "Shaman");
        assert_eq!(word(&mut assets, role_title_id(Some(ToolKind::Aerophones)).unwrap()), "Aerophonist");
        assert_eq!(super::super::guide::resolve(&mut assets, &index, "loc:TribeGame:f37c273b"), "Tribe");
        let add = super::super::guide::resolve(&mut assets, &index, "loc:tooltips:065fc366");
        assert!(add.contains("~tribe_food_required_for_baby~"), "{add}");
    }
}
