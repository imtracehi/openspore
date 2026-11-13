//! The stage launch screen (retail loading screen between stages):
//! `GameLaunchScreen3` with the stage's name, its picture, the "Reticulating
//! Splines" status line and the "You May Encounter..." cards.
//!
//! VERIFIED (code, `notes\front-end-notes.md` 8d, `notes\front-end-5-notes.md` 1): the layout
//! belongs to `cUITimeline` (constructor 0xE40700 loads `GameLaunchScreen3`
//! 0xE247F649 because the app property `enableLoadingCards` 0x064A44A8 is
//! true in AppProperties 08EA246B; without it `GameLaunchScreen` 0x9362A3DE);
//! 0xE3E350 (show / hide; callers: the GGE, the Cell mode's OnEnter, the stage
//! transitions...) makes it visible, fills it (0xE3ABA0), starts the cards
//! (0xE3B0A0), pauses the game clock (`cGameTimeManager::Pause(Gameplay)`)
//! and sends 0x0490A868 (hidden: 0x047D2B68, clock resumed).
//! 0xE3ABA0 reads the prop `timeline!GameLaunchScreen` (0x8C509D6C!0x9362A3DE):
//! `gameLaunchScreenImages` (one PNG per stage, drawn as the background of
//! window 0x0660BFB8) and `gameLaunchScreenTexts` (4 per stage: the title in
//! 0x05CBE0B0, the goals in 0x05CBE0B1..B3). In `GameLaunchScreen3` the goals
//! and their panel 0x05D115A8 are authored hidden and no code shows them
//! (no reference to the panel; the goals only get captions), so the retail
//! screen shows the title, the picture and the status line, not the goals.
//! The status text 0x06456C48 keeps its authored caption ("Reticulating
//! Splines"): the code only moves its top to 0.8 of the top bar's height
//! (0xE38400); the `timeline` table's loading lines are not referenced by the
//! code or by any installed prop.
//!
//! The cards (state machine 0xE45460, clock seconds): 0.5 s after the show
//! the "You May Encounter..." text 0x064DB520 slides in from the left onto
//! the bottom bar's top (0xE39080, to x = 10), 0.5 + 0.8 s later it slides
//! out to the left (0xE391B0), 0.5 s later the cards start. 0xE38400 sizes the
//! cards: height = the bottom bar's height, width = the card frame's aspect x
//! height, gap = 2/3 of what is left of screen width / card count; the count
//! is 10 when the screen is wider than 4:3, else 8 (0xE40700). Each card
//! (frame 0x05DB9A70 + i, image 0x064DCE90 + i) shows the card back
//! 02E9A2CA!FADA8C70 and slides in from the right screen edge to its place
//! on the bottom edge (0xE38990), turns edge-on (0xE38B60, a pi/2 turn about
//! the vertical axis), takes the creation's card image and turns back
//! (0xE40420); then the next card. The creations (0xE44D20): the planet's
//! species other than the player's (Creature: 0xE434A0) and, to fill up to
//! 15, Sporepedia creations of the stage's kind (cells; creatures; tribal
//! creatures; vehicles and buildings; UFOs and buildings). When the load ends
//! the stage hides the cards and shows the black cover 0x064D29C0 for 0.5 s
//! (0xE3B1A0, state 12), then hides the screen.
//!
//! The card durations are the constructor's (VERIFIED, 0xE43EA7 / 0xE43EDA):
//! the slide time (+0x410, read by the slide clock test 0xE36CE0 and the move
//! animation of 0xE38990) is `[0x145BE38]` = 0.5 s, the half-turn time
//! (+0x450, read by the turn clock test 0xE36D30 and both turn animations
//! 0xE38B60 / 0xE40420) is `[0x13EB8A0]` = 0.25 s; only the refill loop
//! (states 9 .. 0xB, 0xE38C40) sets both to `[0x1481BF8]` = 0.2 s, and that
//! loop only runs when the creation list ran dry. A card therefore takes
//! 0.5 + 0.25 + 0.25 = 1 s.
//!
//! The creation list (0xE44D20, VERIFIED, fe-rows2): for the Creature stage
//! (and Tribal, case 2) 0xE434A0 walks every creature of the planet
//! (`cGameNounManager` data, the filter 0x1BE418E), takes each one's species
//! profile when it is not the player's own (`cSpeciesManager` 0x4DF490), copies
//! the profile's 12-byte key (+0x504), sorts the keys (0x4F6B90) and drops the
//! repeats (0x4F6850); when fewer than 15 species were found the list is filled
//! with `15 - count` creations the object template DB returns (the stage's
//! asset type 0x9EA3031A / 0x372E2C04 and the theme parameter 0x3CC89B1 =
//! 0x913B23BE for the Creature case); the cards show the first 8 or 10 of the
//! list ([`card_list`]). Cell asks the DB for cells (`0xDFAD9F51`), Civilization
//! and Space for the vehicle / UFO and building types.
//!
//! ADAPTED: OpenSpore loads a stage in a moment, so the screen stays up until
//! the last card has turned (a click or a key closes it sooner; retail ends
//! the screen when the stage's load ends, 0xE3B1A0); the cards move linearly,
//! the turn drawn as the card narrowing to its centre line; the filler cards
//! are random installed Maxis creations of the stage's kind (the DB's query
//! order is not derivable). The planet's own species lead the list when a stage
//! system fills [`LaunchSpecies`] with their card PNG keys; nothing does yet:
//! OpenSpore's Creature / Tribal species are generated parts lists
//! (`species.rs`), not installed creations with a card, and drawing one needs a
//! thumbnail of the generated creature (`thumbs.rs` renders a creature record;
//! the species system would hand the records over). Scripted runs (smoke test,
//! quick capture) skip the screen unless `OS_LAUNCH_SCREEN=<seconds>` is set.

use bevy::prelude::*;

use crate::{
    asset_catalog::ResourceIndex,
    dbpf::ResourceKey,
    spui::{
        model::ImageRef,
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const LAYOUT: &str = "GameLaunchScreen3";
/// Locale table `timeline`.
pub const TABLE: u32 = 0x8C50_9D6C;
/// Seconds the screen stays up when the cards cannot be laid out (ADAPTED:
/// retail waits for the stage's load; there are no cards without the install).
pub const HOLD: f32 = 3.0;

pub mod id {
    pub const TITLE: u32 = 0x05CB_E0B0;
    pub const GOALS: [u32; 3] = [0x05CB_E0B1, 0x05CB_E0B2, 0x05CB_E0B3];
    /// The goals' panel (authored hidden; retail never shows it).
    pub const GOAL_PANEL: u32 = 0x05D1_15A8;
    pub const PICTURE: u32 = 0x0660_BFB8;
    pub const STATUS: u32 = 0x0645_6C48;
    /// "You May Encounter..." (a text window, slid in and out by code).
    pub const ENCOUNTER: u32 = 0x064D_B520;
    /// The top and bottom bars.
    pub const TOP_BAR: u32 = 0x064D_0818;
    pub const BOTTOM_BAR: u32 = 0x064D_0819;
    /// The black cover shown when the load ends (0xE3B1A0).
    pub const COVER: u32 = 0x064D_29C0;
    /// The ten card frames 0x05DB9A70.. (0xE39300) and their images
    /// 0x064DCE90.. (0xE40700).
    pub const CARD_FRAME: u32 = 0x05DB_9A70;
    pub const CARD_IMAGE: u32 = 0x064D_CE90;
}

/// `gameLaunchScreenImages` (VERIFIED, prop 8C509D6C!9362A3DE): Cell .. Space.
pub const IMAGES: [ResourceKey; 5] = [
    ResourceKey { group_id: 0x02E9_A2CA, instance_id: 0xAABD_531E, type_id: 0x2F7D_0004 },
    ResourceKey { group_id: 0x02E9_A2CA, instance_id: 0x2943_DFDC, type_id: 0x2F7D_0004 },
    ResourceKey { group_id: 0x02E9_A2CA, instance_id: 0x2777_B79F, type_id: 0x2F7D_0004 },
    ResourceKey { group_id: 0x02E9_A2CA, instance_id: 0x2D43_E660, type_id: 0x2F7D_0004 },
    ResourceKey { group_id: 0x02E9_A2CA, instance_id: 0x2B6A_F236, type_id: 0x2F7D_0004 },
];

/// The card back each card shows before it turns (VERIFIED, 0xE38990).
pub const CARD_BACK: ResourceKey = ResourceKey { group_id: 0x02E9_A2CA, instance_id: 0xFADA_8C70, type_id: 0x2F7D_0004 };

/// `gameLaunchScreenTexts`: ids `stage * 4 + k` of [`TABLE`] (title, three
/// goals), with the English strings as fallbacks.
pub const TEXTS: [[&str; 4]; 5] = [
    ["Cell Stage", "Eat To Earn DNA & Grow", "Defeat Foes To Unlock Parts", "Add Parts In The Creator & Evolve"],
    ["Creature Stage", "Battle, Impress, Or Scavenge", "Earn DNA & Unlock Parts", "Mate To Evolve"],
    ["Tribal Stage", "Conquer Or Impress 5 Rival Tribes", "Hunt, Fish, Or Gather To Earn Currency", "Purchase Tools To Increase Efficency"],
    [
        "Civilization Stage",
        "Purchase or Construct Vehicles From City Hall",
        "Mine Spice Geysers to Earn Currency",
        "Use Vehicles To Conquer, Convert, Or Buyout Rival Cities",
    ],
    ["Space Stage", "Explore the Heavens To Meet Friend & Foe", "Complete Missions To Earn Currency", "Colonize & Terraform Planets"],
];

/// The loading lines of [`TABLE`] per stage ("Loading Screen - reticulating
/// spines"; the Creature group skips 0x064AA83F). VERIFIED unused by the
/// launch screen code (no code or prop references them); kept for reference.
#[cfg_attr(not(test), allow(dead_code))]
pub fn status_ids(stage: usize) -> Vec<u32> {
    match stage {
        1 => (0x064A_A838..=0x064A_A844).filter(|id| *id != 0x064A_A83F).collect(),
        2 => (0x064A_A845..=0x064A_A84D).collect(),
        3 => (0x064A_A84E..=0x064A_A857).chain([0x064A_A900]).collect(),
        4 => (0x064A_A901..=0x064A_A90C).collect(),
        _ => Vec::new(),
    }
}

/// Stage index of the launch screen (Cell 0 .. Space 4).
pub fn stage_index(stage: &GameStage) -> Option<usize> {
    match stage {
        GameStage::Cell => Some(0),
        GameStage::Creature => Some(1),
        GameStage::Tribal => Some(2),
        GameStage::Civilization => Some(3),
        GameStage::Space => Some(4),
        GameStage::MainMenu => None,
    }
}

/// Encounter strip and card timing (seconds since the show; VERIFIED from
/// the state machine 0xE45460 unless marked).
pub mod timing {
    /// State 1: the text slides in after 0.5 s.
    pub const STRIP_IN_AT: f32 = 0.5;
    /// The text slides (0xE39080 / 0xE391B0 pass 0.5 s).
    pub const SLIDE: f32 = 0.5;
    /// States 2 + 3: 0.5 s, then 0.8 s until it slides out.
    pub const STRIP_OUT_AT: f32 = STRIP_IN_AT + 0.5 + 0.8;
    /// State 4: 0.5 s until the first card.
    pub const CARDS_AT: f32 = STRIP_OUT_AT + 0.5;
    /// A card's slide in (VERIFIED: ctor 0xE43EA7 stores 0.5 into +0x410).
    pub const CARD_SLIDE: f32 = 0.5;
    /// Half a card turn (VERIFIED: ctor 0xE43EDA stores 0.25 into +0x450).
    pub const TURN: f32 = 0.25;
    pub const CARD_STEP: f32 = CARD_SLIDE + 2.0 * TURN;
    /// State 12: the black cover stays 0.5 s before the screen hides.
    pub const COVER: f32 = 0.5;
}

/// Creations the list is filled up to (0xE44D70: `cmp eax, 0xF`).
pub const LIST_LENGTH: usize = 15;

/// The card PNG keys of the planet's species other than the player's (0xE434A0), for the
/// launch screen to show first. A stage system that has the planet's species cards sets it
/// before the screen shows; empty: only the filler.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct LaunchSpecies(pub Vec<ResourceKey>);

/// The cards' creation list (0xE44D20): the species keys sorted and without repeats
/// (0xE434A0: 0x4F6B90, 0x4F6850), then `filler` up to [`LIST_LENGTH`] (the DB's answer).
pub fn card_list(species: &[ResourceKey], filler: impl IntoIterator<Item = ResourceKey>) -> Vec<ResourceKey> {
    let mut list = species.to_vec();
    // ADAPTED: the key order of the retail sort (`ResourceKey::operator <`) is not read:
    // group, instance, type.
    list.sort_by_key(|k| (k.group_id, k.instance_id, k.type_id));
    list.dedup();
    for key in filler {
        if list.len() >= LIST_LENGTH {
            break;
        }
        list.push(key);
    }
    list
}

/// Cards on screen: 10 when wider than 4:3, else 8 (0xE40700).
pub fn card_count(screen: [f32; 2]) -> usize {
    if screen[0] > screen[1] * 4.0 / 3.0 {
        10
    } else {
        8
    }
}

/// Where the cards go (0xE38400 / 0xE38990): the card size, the top of the
/// cards and each card's left edge.
#[derive(Clone, Debug, PartialEq)]
pub struct CardLayout {
    pub w: f32,
    pub h: f32,
    pub y: f32,
    pub xs: Vec<f32>,
}

/// `bar_h` = the bottom bar's height, `aspect` = card frame width / height.
pub fn card_layout(screen: [f32; 2], bar_h: f32, aspect: f32, n: usize) -> CardLayout {
    let mut w = aspect * bar_h;
    let mut h = bar_h;
    let slot = screen[0] / n.max(1) as f32;
    let free = slot - w;
    let gap = if free > 0.0 {
        free * (1.0 / 3.0) * 2.0
    } else {
        w = slot;
        h = slot / aspect;
        free
    };
    let mut x = gap * 0.5;
    let xs = (0..n)
        .map(|_| {
            let here = x;
            x += w + gap;
            here
        })
        .collect();
    CardLayout { w, h, y: screen[1] - h, xs }
}

/// One card at a time: hidden, sliding in (left edge), turning (width
/// fraction, front shown), or resting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardPose {
    pub visible: bool,
    pub x: f32,
    /// Width fraction while the card turns (1 = facing the viewer).
    pub width: f32,
    /// The creation's image is up (after the first half turn).
    pub front: bool,
}

pub fn card_pose(t: f32, i: usize, target_x: f32, screen_w: f32) -> CardPose {
    use timing::*;
    let start = CARDS_AT + i as f32 * CARD_STEP;
    let u = t - start;
    if u < 0.0 {
        return CardPose { visible: false, x: screen_w, width: 1.0, front: false };
    }
    if u < CARD_SLIDE {
        let f = u / CARD_SLIDE;
        return CardPose { visible: true, x: screen_w + (target_x - screen_w) * f, width: 1.0, front: false };
    }
    let turn = u - CARD_SLIDE;
    if turn < TURN {
        let a = std::f32::consts::FRAC_PI_2 * turn / TURN;
        return CardPose { visible: true, x: target_x, width: a.cos(), front: false };
    }
    if turn < 2.0 * TURN {
        let a = std::f32::consts::FRAC_PI_2 * (turn - TURN) / TURN;
        return CardPose { visible: true, x: target_x, width: a.sin(), front: true };
    }
    CardPose { visible: true, x: target_x, width: 1.0, front: true }
}

/// Left edge of the "You May Encounter..." text at time `t`, if shown.
pub fn strip_x(t: f32, text_w: f32, screen_w: f32) -> Option<f32> {
    use timing::*;
    if t < STRIP_IN_AT {
        return None;
    }
    if t < STRIP_IN_AT + SLIDE {
        let f = (t - STRIP_IN_AT) / SLIDE;
        return Some(-text_w + (10.0 + text_w) * f);
    }
    if t < STRIP_OUT_AT {
        return Some(10.0);
    }
    if t < STRIP_OUT_AT + SLIDE {
        let f = (t - STRIP_OUT_AT) / SLIDE;
        return Some(10.0 + (-screen_w - 10.0) * f);
    }
    None
}

/// Seconds the whole sequence takes with `n` cards (the last turn, then the
/// black cover).
pub fn sequence_seconds(n: usize) -> f32 {
    timing::CARDS_AT + n as f32 * timing::CARD_STEP + timing::COVER
}

/// Creation PNG groups the cards draw from per stage (0xE44D20: cells;
/// creatures; tribal creatures; vehicles + buildings; UFOs + buildings).
pub fn card_groups(stage: usize) -> &'static [u32] {
    match stage {
        0 => &[0x4061_6200],
        1 | 2 => &[0x4062_6200],
        3 => &[0x4064_6200, 0x4063_6200],
        _ => &[0x4065_6200, 0x4063_6200],
    }
}

/// Up to `n` installed creation cards of the stage's kind, shuffled by
/// `seed` (ADAPTED: random Maxis creations).
pub fn pick_cards(index: &ResourceIndex, stage: usize, n: usize, seed: u64) -> Vec<ResourceKey> {
    let groups = card_groups(stage);
    let mut per_group: Vec<Vec<ResourceKey>> = groups
        .iter()
        .map(|g| {
            let mut keys: Vec<ResourceKey> =
                index.records.keys().filter(|k| k.type_id == super::maxis::PNG_TYPE && k.group_id == *g).copied().collect();
            keys.sort_by_key(|k| k.instance_id);
            shuffle(&mut keys, seed ^ u64::from(*g));
            keys
        })
        .collect();
    // Alternate the groups (vehicles, buildings, vehicles, ...).
    let mut out = Vec::new();
    while out.len() < n && per_group.iter().any(|g| !g.is_empty()) {
        for g in per_group.iter_mut() {
            if out.len() < n {
                if let Some(k) = g.pop() {
                    out.push(k);
                }
            }
        }
    }
    out
}

fn shuffle(keys: &mut [ResourceKey], seed: u64) {
    let mut s = seed | 1;
    for i in (1..keys.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        keys.swap(i, (s % (i as u64 + 1)) as usize);
    }
}

#[derive(Component)]
pub struct LaunchScreenRoot;

#[derive(Resource, Default)]
pub struct LaunchScreen {
    /// The stage shown and the seconds left.
    pub shown: Option<(usize, f32)>,
    /// The stage the screen last reacted to.
    last: Option<GameStage>,
    filled: bool,
    /// Seconds since the screen showed.
    pub elapsed: f32,
    /// The creations on the cards (PNG keys).
    pub cards: Vec<ResourceKey>,
    /// The SPUI composite was raised over the HUD's own nodes.
    raised: bool,
}

impl LaunchScreen {
    /// The screen is up: the game clock waits (0xE3E350 pauses Gameplay).
    pub fn holds_game(&self) -> bool {
        self.shown.is_some()
    }
}

/// Seconds to hold the screen in this run (0 = off; `None` = the card
/// sequence decides).
fn hold() -> Option<f32> {
    let forced = crate::world_presentation::tune("LAUNCH_SCREEN", 0.0);
    if forced > 0.0 {
        return Some(forced);
    }
    // Unit tests drive the stages themselves (the screen would hold them).
    let scripted = cfg!(test) || std::env::args().any(|a| matches!(a.as_str(), "--quick-capture" | "--smoke-test" | "--render-smoke"));
    if scripted {
        Some(0.0)
    } else {
        None
    }
}

/// `GlobalZIndex` of the SPUI composite while the screen is up: over the
/// stage HUD's own UI nodes (minimap, portraits, timeline art at 101..150)
/// that otherwise draw over the composite (`spui::surface::COMPOSITE_Z`).
const RAISED_COMPOSITE_Z: i32 = 690;

/// Show the screen when a stage starts, count it down, close it on a click
/// or a key; keep the stage HUD under it.
#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    mut screen: ResMut<LaunchScreen>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    time: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    species: Option<Res<LaunchSpecies>>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<Entity, With<LaunchScreenRoot>>,
    mut composite: Query<&mut GlobalZIndex, With<crate::spui::surface::SpuiComposite>>,
) {
    let now = stage.get().clone();
    if screen.last.as_ref() != Some(&now) {
        screen.last = Some(now.clone());
        let seconds = hold().unwrap_or(f32::INFINITY);
        screen.shown = stage_index(&now).filter(|_| seconds > 0.0).map(|s| (s, seconds));
        screen.filled = false;
        screen.elapsed = 0.0;
        screen.cards.clear();
        for e in &roots {
            commands.entity(e).despawn();
        }
    }
    // The HUD stays under the screen.
    // (Every frame while up: the composite may be spawned after the screen.)
    let up = screen.shown.is_some();
    if up || screen.raised {
        let z = if up { RAISED_COMPOSITE_Z } else { crate::spui::surface::COMPOSITE_Z };
        for mut current in &mut composite {
            if current.0 != z {
                current.0 = z;
            }
        }
        screen.raised = up;
    }
    let Some((stage_i, _)) = screen.shown else { return };
    if screen.cards.is_empty() && !originals.resource_index.records.is_empty() {
        let seed = time.elapsed().as_nanos() as u64 ^ 0x9E37_79B9_7F4A_7C15;
        let species = species.as_deref().map_or(&[][..], |s| s.0.as_slice());
        // The planet's species lead; the filler comes from the installed creations (0xE44D20).
        screen.cards = card_list(species, pick_cards(&originals.resource_index, stage_i, LIST_LENGTH, seed));
    }
    let dt = time.delta_secs().min(0.1);
    screen.elapsed += dt;
    // The layout never came up (no install data): fall back to a short hold
    // so the game is not held forever.
    let elapsed = screen.elapsed;
    if let Some((_, left)) = screen.shown.as_mut() {
        if left.is_infinite() && elapsed > 1.0 {
            *left = HOLD;
        }
    }
    let skip = keys.is_some_and(|k| k.get_just_pressed().next().is_some()) || mouse.is_some_and(|m| m.just_pressed(MouseButton::Left));
    let done = screen.shown.as_mut().is_some_and(|(_, left)| {
        *left -= dt;
        *left <= 0.0
    });
    if done || skip {
        screen.shown = None;
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    if roots.is_empty() && !originals.resource_index.records.is_empty() {
        let Ok(camera) = camera.single() else { return };
        if let Some(layout) = assets.layout(&originals.resource_index, LAYOUT) {
            let root = view::spawn(&mut commands, layout, RootPolicy::Fit, Some(camera), 700);
            commands.entity(root).insert(LaunchScreenRoot);
        }
    }
}

/// Fill the layout (title, goals' captions, picture, status line) once, then
/// drive the encounter text and the cards every frame.
pub fn bind(
    mut screen: ResMut<LaunchScreen>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<&mut SpuiState, With<LaunchScreenRoot>>,
) {
    let Some((stage, _)) = screen.shown else { return };
    let index = &originals.resource_index;
    for mut ui in &mut roots {
        if !ui.ready {
            continue;
        }
        let w = ui.screen[0];
        let h = ui.screen[1];
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        if !screen.filled {
            fill(&mut ui, &mut assets, &mut images, index, stage);
            screen.filled = true;
        }
        let t = screen.elapsed;
        let n = card_count([w, h]).min(10);
        // A sequence-length hold (no forced seconds): end after the last card
        // and the black cover.
        let total = sequence_seconds(n);
        if let Some((_, left)) = screen.shown.as_mut() {
            if left.is_infinite() {
                *left = total - t;
            }
        }
        let left = screen.shown.map_or(0.0, |(_, l)| l);
        if let Some(cover) = ui.find(id::COVER) {
            let on = left <= timing::COVER && left.is_finite() && t >= timing::CARDS_AT;
            if ui.visible(cover) != on {
                ui.set_visible(cover, on);
            }
        }
        // "You May Encounter...", on the bottom bar's top edge.
        let bar = ui.find(id::BOTTOM_BAR).map(|b| ui.rect(b));
        if let Some(text) = ui.find(id::ENCOUNTER) {
            let a = ui.area(text);
            let tw = a[2] - a[0];
            let th = a[3] - a[1];
            match (strip_x(t, tw, w), bar) {
                (Some(x), Some(bar)) => {
                    ui.set_visible(text, true);
                    ui.set_area(text, [x, bar.y1, x + tw, bar.y1 + th]);
                }
                _ => ui.set_visible(text, false),
            }
        }
        let Some(bar) = bar else { continue };
        let bar_h = bar.y2 - bar.y1;
        let aspect = ui
            .find(id::CARD_FRAME)
            .map(|f| {
                let a = ui.layout.windows[f].area;
                (a[2] - a[0]) / (a[3] - a[1]).max(1.0)
            })
            .unwrap_or(148.0 / 194.0);
        let cards = card_layout([w, h], bar_h, aspect, n);
        for i in 0..10usize {
            let (Some(frame), image) = (ui.find(id::CARD_FRAME + i as u32), ui.find(id::CARD_IMAGE + i as u32)) else {
                continue;
            };
            let card = screen.cards.get(i).copied();
            if i >= n || card.is_none() {
                ui.set_visible(frame, false);
                continue;
            }
            let pose = card_pose(t, i, cards.xs[i], w);
            ui.set_visible(frame, pose.visible);
            if !pose.visible {
                continue;
            }
            let cw = cards.w * pose.width.max(0.0);
            let x = pose.x + (cards.w - cw) * 0.5;
            ui.set_area(frame, [x, cards.y, x + cw, cards.y + cards.h]);
            if let Some(image) = image {
                ui.set_visible(image, true);
                // The image fills its frame (both anchored to all edges).
                ui.set_area(image, [0.0, 0.0, 0.0, 0.0]);
                let key = if pose.front { card.unwrap_or(CARD_BACK) } else { CARD_BACK };
                let key = if assets.texture(index, &mut images, key).is_some() { key } else { CARD_BACK };
                if assets.texture(index, &mut images, key).is_some() {
                    ui.set_image(image, Some(ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None }));
                }
            }
        }
    }
}

/// Title, goals' captions (hidden, as retail), the stage picture and the
/// status line's place.
fn fill(ui: &mut SpuiState, assets: &mut SpuiAssets, images: &mut Assets<Image>, index: &ResourceIndex, stage: usize) {
    let texts: Vec<String> = (0..4).map(|k| super::loc(assets, index, TABLE, (stage * 4 + k) as u32, TEXTS[stage][k])).collect();
    if let Some(w) = ui.find(id::TITLE) {
        ui.set_text(w, texts[0].clone());
    }
    // 0xE3ABA0 captions the goals; GameLaunchScreen3 keeps them hidden.
    if let Some(w) = ui.find(id::GOAL_PANEL) {
        ui.set_visible(w, false);
    }
    for (k, control) in id::GOALS.iter().enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_text(w, texts[k + 1].clone());
        }
    }
    // 0xE38400: the status line's top at 0.8 of the top bar's height; its
    // caption stays the authored one.
    if let (Some(status), Some(bar)) = (ui.find(id::STATUS), ui.find(id::TOP_BAR)) {
        let r = ui.rect(bar);
        let a = ui.area(status);
        let top = ((r.y2 - r.y1) * 0.8).min(a[3] - 1.0);
        ui.set_area(status, [a[0], top, a[2], a[3]]);
    }
    if let Some(w) = ui.find(id::PICTURE) {
        if assets.texture(index, images, IMAGES[stage]).is_some() {
            ui.set_image(w, Some(ImageRef { file: IMAGES[stage], uv: [0.0, 0.0, 1.0, 1.0], dims: None }));
        }
    }
    // Hidden on show (0xE3E350); the cards drive it from here.
    if let Some(w) = ui.find(id::ENCOUNTER) {
        ui.set_visible(w, false);
    }
    for i in 0..10 {
        if let Some(w) = ui.find(id::CARD_FRAME + i) {
            ui.set_visible(w, false);
        }
    }
    let _ = assets.texture(index, images, CARD_BACK);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_its_texts_picture_and_loading_lines() {
        assert_eq!(stage_index(&GameStage::Creature), Some(1));
        assert_eq!(stage_index(&GameStage::MainMenu), None);
        assert_eq!(status_ids(1).len(), 12);
        assert_eq!(status_ids(2).len(), 9);
        assert_eq!(status_ids(3).len(), 11);
        assert_eq!(status_ids(4).len(), 12);
        assert!(status_ids(0).is_empty(), "the table has no Cell loading lines");
        assert!(!status_ids(1).contains(&0x064A_A83F));
        assert_eq!(IMAGES.len(), TEXTS.len());
    }

    #[test]
    fn cards_follow_the_retail_layout_and_timeline() {
        // 16:10 -> 10 cards; 4:3 -> 8.
        assert_eq!(card_count([1280.0, 800.0]), 10);
        assert_eq!(card_count([1024.0, 768.0]), 8);
        // 1280 x 900: bar 135 high, card 148/194 aspect -> 103 x 135, gap 2/3
        // of the rest of a 128 px slot, half a gap before the first card.
        let l = card_layout([1280.0, 900.0], 135.0, 148.0 / 194.0, 10);
        assert!((l.w - 103.0).abs() < 0.1 && l.h == 135.0 && l.y == 765.0);
        let gap = (128.0 - l.w) * 2.0 / 3.0;
        assert!((l.xs[0] - gap * 0.5).abs() < 1e-3);
        assert!((l.xs[1] - l.xs[0] - (l.w + gap)).abs() < 1e-3);
        assert!(l.xs[9] + l.w < 1280.0);
        // A narrow screen shrinks the cards to the slot.
        let l = card_layout([400.0, 900.0], 135.0, 148.0 / 194.0, 10);
        assert!((l.w - 40.0).abs() < 1e-3 && l.h < 135.0);
        // Text: in at 0.5 s, resting at x = 10, out after 1.8 s, gone at 2.3 s.
        assert_eq!(strip_x(0.4, 300.0, 1280.0), None);
        assert_eq!(strip_x(1.2, 300.0, 1280.0), Some(10.0));
        assert!(strip_x(2.0, 300.0, 1280.0).unwrap() < 10.0);
        assert_eq!(strip_x(2.4, 300.0, 1280.0), None);
        // Card 0 starts at 2.3 s from the right edge, turns, shows its front.
        let p = card_pose(2.2, 0, 8.0, 1280.0);
        assert!(!p.visible);
        let p = card_pose(2.3 + 0.25, 0, 8.0, 1280.0);
        assert!(p.visible && p.x < 1280.0 && p.x > 8.0 && !p.front);
        let p = card_pose(2.3 + 0.5 + 0.1, 0, 8.0, 1280.0);
        assert!(p.width < 1.0 && !p.front);
        let p = card_pose(2.3 + 0.5 + 0.3, 0, 8.0, 1280.0);
        assert!(p.width < 1.0 && p.front);
        assert_eq!(card_pose(2.3 + 1.1, 0, 8.0, 1280.0), CardPose { visible: true, x: 8.0, width: 1.0, front: true });
        // Card 1 waits for card 0's turn (a card takes 0.5 + 2 x 0.25 s).
        assert!(!card_pose(2.3 + 0.95, 1, 8.0, 1280.0).visible);
        assert!(card_pose(2.3 + 1.05, 1, 8.0, 1280.0).visible);
        assert!((sequence_seconds(10) - (2.3 + 10.0 + 0.5)).abs() < 1e-4);
    }

    #[test]
    fn the_planets_species_lead_the_card_list() {
        let key = |i: u32| ResourceKey { group_id: 0x4062_6200, instance_id: i, type_id: 0x2F7D_0004 };
        let filler: Vec<ResourceKey> = (100..130).map(key).collect();
        // No species: the filler alone, 15 long.
        assert_eq!(card_list(&[], filler.clone()), filler[..LIST_LENGTH].to_vec());
        // Species first, sorted, without repeats; then the filler up to 15.
        let list = card_list(&[key(7), key(3), key(7), key(5)], filler.clone());
        assert_eq!(list.len(), LIST_LENGTH);
        assert_eq!(&list[..3], &[key(3), key(5), key(7)]);
        assert_eq!(&list[3..6], &filler[..3]);
        // Fifteen species or more: no filler at all (0xE44D9A: the fill only runs below 15).
        let many: Vec<ResourceKey> = (0..20).map(key).collect();
        let list = card_list(&many, filler);
        assert_eq!(list.len(), 20);
        assert_eq!(&list[..15], &many[..15]);
    }

    #[test]
    fn card_groups_match_each_stage() {
        assert_eq!(card_groups(0), &[0x4061_6200]);
        assert_eq!(card_groups(2), &[0x4062_6200]);
        assert_eq!(card_groups(3), &[0x4064_6200, 0x4063_6200]);
        assert_eq!(card_groups(4), &[0x4065_6200, 0x4063_6200]);
        let mut keys: Vec<ResourceKey> = (0..20).map(|i| ResourceKey { group_id: 1, instance_id: i, type_id: 2 }).collect();
        let before = keys.clone();
        shuffle(&mut keys, 7);
        assert_ne!(keys, before);
        keys.sort_by_key(|k| k.instance_id);
        assert_eq!(keys, before);
    }

    /// The installed prop and layout (needs the game).
    #[test]
    #[ignore = "reads the installed packages"]
    fn installed_launch_screen_layout_and_strings() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut assets = SpuiAssets::default();
        let layout = assets.layout(&index, LAYOUT).expect("GameLaunchScreen3");
        for control in [id::TITLE, id::GOALS[0], id::PICTURE, id::STATUS, id::ENCOUNTER, id::BOTTOM_BAR, id::COVER, id::CARD_FRAME + 9, id::CARD_IMAGE + 9] {
            assert!(layout.windows.iter().any(|w| w.control_id == control), "{control:08X}");
        }
        // The goals' panel is authored hidden (retail never shows it).
        let panel = layout.windows.iter().find(|w| w.control_id == id::GOAL_PANEL).expect("goal panel");
        assert_eq!(panel.flags & 1, 0);
        assert_eq!(assets.localized(&index, TABLE, 0x0000_0004).as_deref(), Some("Creature Stage"));
        for key in IMAGES.iter().chain([&CARD_BACK]) {
            assert!(index.records.contains_key(key), "{key:?}");
        }
        for stage in 0..5 {
            let cards = pick_cards(&index, stage, 10, 1);
            assert_eq!(cards.len(), 10, "stage {stage}");
        }
        let civ = pick_cards(&index, 3, 10, 1);
        assert!(civ.iter().any(|k| k.group_id == 0x4064_6200) && civ.iter().any(|k| k.group_id == 0x4063_6200));
    }
}
