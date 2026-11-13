//! The end-of-stage summary: the retail History timeline.
//!
//! Advance (and the History button) hides the HUD, pauses the game and shows
//! the timeline UI (`FUN_00E539A0`, RE 10C). For the Cell stage that is the
//! installed `timeline` layout: the stage tabs and evolution clock on top, the
//! five stage columns, the stage pills and the green check button at the
//! bottom, filled with the Cell stage's history events, the "Which path have
//! you chosen?" trait bar of the diet class and the
//! `Callout_Timeline_TransitionOutOfCell` dialog. The player closes it with the
//! check button or Enter.
//!
//! Each history event is a property file of group `TimelineEvents`
//! (`clg_gamestart`, `clg_discoverpart`, `clg_killedsomething`, `clg_death`,
//! `clg_earnedtrait*`) naming the card layout, icon and title string; the
//! cards are the installed `timelineEvent*` layouts.
//!
//! RE (cell-hud2): `0xE539A0` only hides the Cell HUD, hides the goal cards
//! (`0xE18FF0`) and sends message `0x6256B62` (argument `state == 5`) to the
//! Simulator's shared timeline UI (`Simulator_GetUITimeline`, `0xB3D510`,
//! the same object every stage uses): the `cUITimeline` class
//! (`0xE36000`..`0xE49000`; ctor `0xE40700`, which also owns the stage launch
//! screen).
//!
//! RE (cell-hud3, `0xE449B0` message handler, `0xE44200` show, `0xE36FA0` /
//! `0xE37260` / `0xE372E1` clock, `0xE3A220` stage times, `0xE3B990`
//! sequencer; details in `cell-hud3\CHANGES.md`):
//! * The handler stores arg 2 (`+0x294`), sets the speed state `+0x284` and
//!   calls the show function `0xE44200(true)`: sound `0x6D093BEE`, message
//!   `0x4A07018`, the five stage tab overlays `15503770..74` are hidden and
//!   only the current game mode's one is shown (VERIFIED, implemented in
//!   [`bind_main_stage`]), the five stage columns `74ED2970..74` are hidden
//!   and zeroed (the scroll / zoom code places them afterwards), the five
//!   bottom stage pills `060BE2E0..E4` get the shade `0xFF4B4B4B`
//!   (VERIFIED, implemented), then the extents (`0xE42100`) and the clock are
//!   updated.
//! * The evolution clock counts years from the start of the Cell stage:
//!   the variables `DateCellGameMultiplier` 10000, `DateCreatureGameMultiplier`
//!   1000, `DateTribeGameMultiplier` 100, `DateCivGameMultiplier` 10,
//!   `DateSpaceGameMultiplier` 1 (years per game second), the stage lengths
//!   `DateCreatureGameStartTime` 1000, `...Tribe...` 2000, `...Civ...` 3000,
//!   `...Space...` 4000 (a finished stage counts as 1000 game seconds) and
//!   the quantum 100 000 000 / 1 000 000 / 10 000 / 100 / 100 years per
//!   stage; see [`date_years`] and [`clock_digits`]. The number windows show
//!   billions without leading zeros and the other three groups as three
//!   digits (`%c%c%c`).
//!
//! RE (cell-timeline3, `axis.rs` has the details): the scroll / layout
//! machinery is NOT a continuous time axis with zoom levels. The events are
//! laid out one behind the other (x starts at 5, every card advances x by its
//! width plus 5, `0xE3F970`), the stage's column is as wide as its events, the
//! content scrolls horizontally with a follow smoothing, and the intro
//! sequencer `0xE3B990` waits for the stage's timeline callout to be
//! dismissed, pauses 1.5 s, scrolls 1.5 s, pauses 1.5 s and shows the earned
//! trait. All of that is in [`axis`] and used by [`manage`] / [`bind`] here
//! and by the Creature summary.
//!
//! The window is backed by the blurred world (cell-backdrop): the show function `0xE44200` creates the screen
//! effect `modalUIBackgroundBlur` (`0x4C4BA0A9`, world 0) and starts it; its Cell screen (app flag `0x80`) is
//! the Swarm filter chain decoded in `swarm_fx::filters` (tint the frame toward the effects colour, composite with
//! `CellMinspecCompositeMaterial`, four diagonal blurs at quarter resolution, 40 % desaturation), which runs on
//! the paused world through the GameCamera ([`TimelineRoot::BackdropBlur`]). The result is opaque: the window
//! shows the stage's world, never the sharp one. Only when the filter chains cannot run (`OS_FILTERS=0`, the
//! effect missing) does an opaque flat panel stand in ([`backdrop_node`], [`backdrop_colour`]).
//!
//! RE (cell-timeline4, [`events`] and [`axis`] have the details): the cards are the
//! merged records of the Cell's history (`0xE394A0`: every meal, kill, death and
//! part found with its time and the diet balance of the moment, the editor return,
//! the start), stacked and merged by `0xE3F2A0`, and every card sits at the height
//! `(h / 2) * (1 + 0xE37D20(value))` of its (newest) record's balance in the stage
//! column (`0xE46D80`).
//!
//! RE (cell-timeline5, [`line`] and [`axis`] have the details): the disposition line is a white
//! Catmull-Rom ribbon through the card centres with a disc at every event (`0xE45760` ->
//! `0xE3C150`; `LineColor*` are never read), it wipes in over `LineIntroTimeMS`; the vertical marker
//! window `75581F48` rides it and its height picks the band tint (`0xE42C83`), whose colours are
//! the colour property of the trait templates; the earned-trait card is the stage's last card
//! (`0xE3BFB0`, `0x0EE4273F`), centred in the column with no room behind it (`0xE42100`); cards are
//! loaded one per `EventLoadIntervalMS` inside the screen buffer and glide from their authored
//! position to their target (`0xE423A0`, `0xE46880`); the earned trait's `TraitRollover` callout
//! (`0xE3A6E0`) shows for `CinematicStateShowEarnedTraitTimeMS`.
//!
//! RE (cell-icons, [`bind_trait_callout`] and [`thumbs`] have the details): the callout is a child of
//! the stage tab window with its layout location set to (0, 0) (`0x6771E0`), so it covers the tab; its
//! filler `0x6776C0` writes the name, colour, picture and the four effect pictures (`54D95163` ->
//! `ConsequenceSuperPowers` -> `D4D959E2`). The generation cards show the creature of their editor
//! visit, rendered from a record kept at the visit ([`thumbs`]).
//!
//! ADAPTED (what the decode did not reach, see CHANGES.md): the marker is placed by the centre of
//! its window, the card load range reads the target x, the thumbnails of visits that predate the
//! record are empty and a record without a paint shows the live one ([`thumbs`]), the History
//! backdrop's frame is the paused live world (not a stored copy) and its first filter reads the
//! scratch buffer that starts as the frame (`swarm_fx::filters`), the date read-out of the marker (retail shows none:
//! `DateDisplayOnEventMouseover` is never read) and the mouse hit test of the line are not ported,
//! a save or demo without a recorded history gets a synthetic one ([`events::history`]).

use std::collections::HashMap;

use bevy::prelude::*;

pub(crate) mod axis;
pub mod events;
pub(crate) mod line;
pub mod thumb_paint;
pub mod thumbs;

use super::{dialogs, portrait};
use crate::swarm_fx::SwarmEffect;
use crate::{
    cell_contract::{CellHudState, CellSummary},
    cell_editor_data::{CellEditorData, START_SLOTS},
    dbpf::ResourceKey,
    game::RunProgress,
    spui::{
        model::ImageRef,
        solve::{Rect, RootPolicy},
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const TIMELINE_LAYOUT: &str = "timeline";
pub(crate) const LAYOUT_LARGE: &str = "timelineEventLargeAsset";
pub(crate) const LAYOUT_PART: &str = "timelineEventPartUnlock";
/// Icon card with a count (kills, meals); the layout has no registered name.
pub(crate) const LAYOUT_ICON: &str = "0x378C39D8";
pub(crate) const LAYOUT_DEATH: &str = "timelineEventDeath";
pub(crate) const LAYOUT_TRAIT_BAR: &str = "timelineEventTraitBar";
const TRANSITION_CALLOUT: &str = "Callout_Timeline_TransitionOutOfCell";
/// The `TraitRollover` layout (`0x646739AC`) the trait callout window of `0xE3A6E0` loads.
pub(crate) const LAYOUT_TRAIT_CALLOUT: &str = "0x646739AC";

/// Controls of the trait callout layout (`0xE47930` / `0x6771E0` find `125F2C4F`).
pub(crate) mod rollover {
    pub const ROOT: u32 = 0x125F_2C4F;
    /// "<Trait Name>".
    pub const NAME: u32 = 0x0603_6C70;
    /// The trait's picture in the middle of the rings.
    pub const ICON: u32 = 0x0603_A678;
    /// The ring around the picture and the body panel: both authored with the shade
    /// `FF29FF06` for the game to replace.
    pub const RING: u32 = 0x0667_D3F8;
    pub const BODY: u32 = 0x54F6_3A3E;
    /// The four effect pictures (26 x 26, left to right).
    pub const EFFECTS: [u32; 4] = [0x04C0_1BBE, 0x04C0_1BBF, 0x04C0_1BC0, 0x04C0_1BC1];
}

/// The Cell stage tab's authored rectangle (`06299720` in `timeline`): the callout of the
/// earned trait is attached to the stage tab of the earned stage (`0xE3A6E0`).
pub(crate) const CELL_TAB: Rect = Rect { x1: 125.0, y1: -2.0, x2: 242.0, y2: 148.0 };
/// Locale table of the trait names (`9A5ACF10`) and the ids of herbivore / omnivore /
/// carnivore (`0x42`, `0x44`, `0x40`; the description is the id + 1).
const TRAIT_TABLE: u32 = 0x9A5A_CF10;
const TRAIT_NAME_IDS: [u32; 3] = [0x42, 0x44, 0x40];

/// Controls of the `timeline` layout.
pub(crate) mod win {
    /// The green check button at the bottom right.
    pub const OK: u32 = 0x0446_E560;
    /// The five stage tabs on top (Cell first) and their "selected" overlays.
    pub const TABS: [u32; 5] = [0x0629_9720, 0x0629_9721, 0x0629_9722, 0x0629_9723, 0x0629_9724];
    pub const TAB_SELECTED: [u32; 5] = [0x1550_3770, 0x1550_3771, 0x1550_3772, 0x1550_3773, 0x1550_3774];
    /// Evolution clock numbers: billions, millions, thousands, years.
    pub const CLOCK: [u32; 4] = [0x5549_6B13, 0x5549_6B14, 0x5549_6B15, 0x5549_6B16];
    /// The five stage pills along the bottom (Cell first).
    pub const PILLS: [u32; 5] = [0x060B_E2E0, 0x060B_E2E1, 0x060B_E2E2, 0x060B_E2E3, 0x060B_E2E4];
    /// Stage column backgrounds and foregrounds (Cell first).
    pub const BACK: [u32; 5] = [0x74ED_2970, 0x74ED_2971, 0x74ED_2972, 0x74ED_2973, 0x74ED_2974];
    pub const FRONT: [u32; 5] = [0xB54A_5880, 0xB54A_5881, 0xB54A_5882, 0xB54A_5883, 0xB54A_5884];
    /// The three disposition band windows (top, middle, bottom) `0xE42C83` tints.
    pub const BANDS: [u32; 3] = [0xF537_D890, 0xF537_D891, 0xF537_D892];
    /// The vertical marker sprite (20 x 43) that rides the disposition line (`0xE40C25`).
    pub const MARKER: u32 = 0x7558_1F48;
}

/// Shade the show function `0xE44200` gives the stage pills.
pub(crate) const PILL_SHADE: u32 = 0xFF4B_4B4B;

/// Controls shared by the event card layouts.
pub(crate) mod card {
    pub const ROOT: u32 = 0x94E2_C6D2;
    /// Big image slot (thumbnail, part picture) and small icon slot.
    pub const IMAGE: u32 = 0x060C_6DB0;
    pub const ICON: u32 = 0x060C_6DB8;
    /// Title pill and the count.
    pub const LABEL: u32 = 0x060C_6CA8;
    pub const NUMBER: u32 = 0x34F7_7EEF;
}

/// Trait bar slots (top to bottom: herbivore, omnivore, carnivore = the
/// `065263C8` index of the `clg_earnedtrait*` events 0, 1, 2).
pub(crate) const TRAIT_SLOTS: [u32; 3] = [0x0652_7160, 0x0652_7161, 0x0652_7162];

/// Event property files (`TimelineEvents`) and the property ids read.
pub(crate) const EVENT_GROUP: u32 = 0xE1D7_164F;
const PROP_ICON: u32 = 0x060B_3E8D;
const PROP_TITLE: u32 = 0x060B_3E8E;
/// Consequence trait property files (`ConsequenceTraits`) and their icon.
pub(crate) const TRAIT_GROUP: u32 = 0xF865_C777;
pub(crate) const PROP_TRAIT_ICON: u32 = 0x54D9_5162;
/// Trait property instances: herbivore, omnivore, carnivore.
const TRAITS: [u32; 3] = [0xA8EC_6F99, 0x5ECE_4770, 0xCFB0_1B93];
/// `54D95163`: the trait's effects, an array of keys of `ConsequenceSuperPowers` records (`0x6776C0` reads up to four;
/// every installed trait lists four); `D4D959E2` of such a record is the effect's picture, a key of
/// `ConsequenceGraphics` (the group of the trait pictures too).
const PROP_TRAIT_EFFECTS: u32 = 0x54D9_5163;
const SUPER_POWER_GROUP: u32 = 0x4495_05AF;
const PROP_EFFECT_ICON: u32 = 0xD4D9_59E2;

/// Where the row of cards sits (centre line). Only the Creature summary uses this
/// (ADAPTED there, its event values are not recorded); the Cell summary places
/// every card at its disposition height ([`axis::card_top`]). Where the row starts
/// inside the stage column and the gap behind each card are retail's
/// (`0xE3F970`: 5 and 5).
pub(crate) const CARD_ROW_Y: f32 = 300.0;
pub(crate) const CARD_ROW_X: f32 = axis::EVENT_START_X;
pub(crate) const CARD_GAP: f32 = axis::EVENT_GAP;
/// Top left of the trait bar (it sits over the right panel of the layout).
pub(crate) const TRAIT_BAR_AT: (f32, f32) = (1162.0, 196.0);
/// Vertical centre of the History callout, as a fraction of the screen.
pub(crate) const CALLOUT_CENTRE_Y: f32 = 0.66;
/// Frames after spawning before the layouts are shown (the first frame fixes
/// the state their entrance effects start from).
pub(crate) const SHOW_AFTER_FRAMES: u32 = 2;
/// Layer of the Cell's main layout and of the line over it inside the SPUI surface; the
/// cards are at 105 (the surface camera orders its layouts by `GlobalZIndex`).
pub(crate) const MAIN_Z: i32 = 103;
pub(crate) const LINE_Z: i32 = 104;

/// What one card shows. The ids are unique per card: `stack` is the index of the
/// card's stack in [`TimelineCache::stacks`], the part cards are told apart by
/// their unlock slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardKind {
    /// `clg_gamestart`: the cell emerged (creature thumbnail, "Generation 1").
    GameStart,
    /// `clg_evolveineditor`: the `n`-th editor visit ("Generation n + 2").
    Evolve(usize),
    /// `clg_discoverpart`: a part kind found (unlock slot), one card of a stack.
    Part(usize),
    /// `clg_eatplant` / `clg_eatmeat`: meals eaten (merged, with the count).
    Meals { stack: usize, plant: bool, count: i32 },
    /// `clg_killedsomething`: cells killed (merged, with the count).
    Kills { stack: usize, count: i32 },
    /// `clg_death`: deaths (merged, with the count).
    Deaths { stack: usize, count: i32 },
}

/// The cards of one stack of the history, top to bottom (a part stack has one
/// per part, every other stack one).
pub fn stack_cards(index: usize, stack: &events::Stack) -> Vec<CardKind> {
    match stack.kind {
        events::StackKind::GameStart => vec![CardKind::GameStart],
        events::StackKind::EvolveInEditor(n) => vec![CardKind::Evolve(n)],
        events::StackKind::Meals { plant } => {
            vec![CardKind::Meals { stack: index, plant, count: stack.count as i32 }]
        }
        events::StackKind::Parts => stack.parts.iter().map(|slot| CardKind::Part(*slot as usize)).collect(),
        events::StackKind::Kills => vec![CardKind::Kills { stack: index, count: stack.count as i32 }],
        events::StackKind::Deaths => vec![CardKind::Deaths { stack: index, count: stack.count as i32 }],
    }
}

/// The card layout of a kind.
fn card_layout(kind: &CardKind) -> &'static str {
    match kind {
        CardKind::GameStart | CardKind::Evolve(_) => LAYOUT_LARGE,
        CardKind::Part(_) => LAYOUT_PART,
        CardKind::Meals { .. } | CardKind::Kills { .. } => LAYOUT_ICON,
        CardKind::Deaths { .. } => LAYOUT_DEATH,
    }
}

/// The disposition a summary without a recorded history stands on (ADAPTED: the
/// diet class's side of the line).
fn diet_value_of(diet: &str) -> f32 {
    match diet {
        "clg_plant" => -0.6,
        "clg_meat" => 0.6,
        _ => 0.0,
    }
}

/// Row of cards: left to right, centred on the line `centre_y`.
pub fn place_cards(sizes: &[Vec2], start_x: f32, centre_y: f32, gap: f32) -> Vec<Rect> {
    let mut x = start_x;
    sizes
        .iter()
        .map(|s| {
            let r = Rect::new(x, centre_y - s.y * 0.5, x + s.x, centre_y + s.y * 0.5);
            x += s.x + gap;
            r
        })
        .collect()
}

/// Index of the trait bar slot of a diet class.
pub fn trait_slot(diet: &str) -> usize {
    match diet {
        "clg_plant" => 0,
        "clg_meat" => 2,
        _ => 1,
    }
}

/// `Date*Multiplier` game variables (static initialisers `0x1372930` ..
/// `0x1372A77`, defaults 10000 / 1000 / 100 / 10 / 1): years per second of
/// game time in the Cell, Creature, Tribe, Civ and Space stages.
pub const DATE_MULTIPLIER: [u64; 5] = [10_000, 1_000, 100, 10, 1];
/// `DateCreatureGameStartTime` .. `DateSpaceGameStartTime` (1000, 2000, 3000,
/// 4000) differences: a stage that is behind counts as this many game seconds
/// (`0xE36FA0`, current game mode later than the stage).
pub const DATE_STAGE_SECONDS: [u64; 4] = [1000, 1000, 1000, 1000];
/// The date is cut down to a multiple of this many years (the constants
/// `0x5F5E100`, `0xF4240`, `0x2710`, `0x64`, `0x64` of `0xE36FA0`), per stage
/// of the event.
pub const DATE_QUANTUM: [u64; 5] = [100_000_000, 1_000_000, 10_000, 100, 100];
/// The clock windows show at most 999 999 999 999 years (`0xE37260`).
pub const DATE_MAX: u64 = 999_999_999_999;

/// `0xE36FA0`: the date, in years, of an event of stage `stage` (0 Cell,
/// 1 Creature, 2 Tribe, 3 Civ, 4 Space) that happened `seconds` of game time
/// into its stage. The stages before it count as their fixed length when the
/// timeline is shown from a later stage (`finished_seconds` replaces those
/// lengths when a stage is still running, as the retail code does with the
/// recorded stage start / end times).
pub fn date_years(stage: usize, seconds: u64, finished_seconds: Option<&[u64; 4]>) -> u64 {
    let stage = stage.min(4);
    let mut years = 0u64;
    for earlier in 0..stage.min(4) {
        let length = finished_seconds.map_or(DATE_STAGE_SECONDS[earlier], |f| f[earlier]);
        years += length * DATE_MULTIPLIER[earlier];
    }
    years += seconds * DATE_MULTIPLIER[stage];
    years / DATE_QUANTUM[stage] * DATE_QUANTUM[stage]
}

/// `0xE37260`: the four numbers the clock shows (billions, millions,
/// thousands, years).
pub fn clock_digits(years: u64) -> [u32; 4] {
    let years = years.min(DATE_MAX);
    [
        (years / 1_000_000_000) as u32,
        (years / 1_000_000 % 1000) as u32,
        (years / 1000 % 1000) as u32,
        (years % 1000) as u32,
    ]
}

/// The text of clock window `index` (0 billions .. 3 years): the billions
/// without leading zeros, the other groups as three digits (`0xE372E1`).
pub fn clock_text(index: usize, value: u32) -> String {
    if index == 0 {
        value.to_string()
    } else {
        format!("{value:03}")
    }
}

/// The Cell stage's clock for a stage of `play_seconds` of game time: the
/// date of the timeline's last event (VERIFIED: `0xE42100` ends with
/// `0xE37260(0xE36FA0(last event))`; the mouse-over read-out of the other
/// events, `0xE423A0`, is not ported). A normal run is far below the 100
/// million year quantum, so the clock reads zero.
pub fn clock_numbers(play_seconds: f32) -> [u32; 4] {
    clock_digits(date_years(0, play_seconds.max(0.0) as u64, None))
}

/// Which layout a spawned root is.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub enum TimelineRoot {
    /// The opaque backdrop node ([`backdrop_node`]); no layout.
    Backdrop,
    /// The screen effect `modalUIBackgroundBlur` ([`BACKDROP_EFFECT`]) running on the paused
    /// world; no layout. Used instead of [`TimelineRoot::Backdrop`] when the Swarm filter
    /// chains can run it.
    BackdropBlur,
    Main,
    Callout,
    TraitBar,
    /// The earned trait's `TraitRollover` callout (`0xE3A6E0`).
    TraitCallout,
    Card(CardKind),
}

/// Loaded event definitions, part icons and the spawn clock.
#[derive(Resource, Default)]
pub struct TimelineCache {
    pub(crate) events: HashMap<&'static str, Option<(Option<ResourceKey>, String)>>,
    part_icons: HashMap<usize, Option<ResourceKey>>,
    trait_icons: [Option<ResourceKey>; 3],
    /// The effect pictures of each trait (`PROP_TRAIT_EFFECTS`).
    trait_effects: [Vec<Option<ResourceKey>>; 3],
    traits_loaded: bool,
    /// Frames since the layouts were spawned.
    pub(crate) frames: u32,
    /// The callout was read.
    callout: Option<dialogs::CalloutDef>,
    callout_loaded: bool,
    /// The intro sequencer of the shown summary (`0xE3B990`) and the scroll
    /// position the window shows (`+0x278`, chasing the sequencer's `+0x270`).
    pub(crate) seq: Option<axis::Sequencer>,
    pub(crate) scroll_shown: f32,
    /// Shown colours of the three disposition bands (`0xE42C83`).
    pub(crate) bands: [[f32; 4]; 3],
    /// The cards of the shown history, left to right (`events::stacks`).
    pub(crate) stacks: Vec<events::Stack>,
    /// Size and drawn extent of the card layouts, by layout name.
    pub(crate) metrics: HashMap<&'static str, CardMetrics>,
    /// Capture switches read once from the command line.
    pub(crate) capture: CaptureSwitches,
    /// The earned-trait callout (`0xE3A6E0`) is up (sequencer state 6).
    pub(crate) trait_up: bool,
    /// `+0x280`: milliseconds since the window was shown, up to `LineIntroTimeMS`.
    pub(crate) intro_ms: i32,
    /// The event load clock (`+0xB0`): milliseconds since the last card was loaded / unloaded.
    pub(crate) load_ms: i32,
    /// Load state and glide offset of every card (one per stack, then the trait bar).
    pub(crate) motion: Vec<CardMotion>,
    /// Top of the vertical marker window the last frame (`0xE42E71` reads it for the band).
    pub(crate) marker_top: Option<f32>,
    /// The texture the line is painted into and its node.
    pub(crate) line_image: Option<(Handle<Image>, UVec2)>,
    /// The trait callout's texts (name, description) of the earned trait.
    pub(crate) trait_text: Option<(String, String)>,
    /// The columns of the line texture as last painted (repainted only when they change).
    pub(crate) line_painted: line::Columns,
}

/// One event card of the shown history (`0xE48DC0` loads it, `0xE46880` glides it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CardMotion {
    /// The card's layout is loaded: only loaded cards are shown (`+0x6D`).
    pub(crate) loaded: bool,
    /// Where the card is relative to its target, in the column's pixels (it starts at the
    /// layout's authored position and closes `dt_ms * 0.008` of the gap per frame).
    pub(crate) offset: Vec2,
}

/// `EventLoadScreenBufferPercentage` (`0x16ACF3C`) and `EventLoadIntervalMS` (`0x16ACF4C`).
pub(crate) const EVENT_LOAD_BUFFER: f32 = 0.25;
pub(crate) const EVENT_LOAD_INTERVAL_MS: i32 = 30;

/// `0xE46880`: one frame of a card's glide towards its target.
pub(crate) fn glide(offset: Vec2, dt_ms: i32) -> Vec2 {
    offset * (1.0 - axis::follow_factor(dt_ms))
}

/// `0xE423A0` (the loop at `0xE42475`): a card is loaded while its x is inside the screen plus
/// `EventLoadScreenBufferPercentage` of the width on either side.
pub(crate) fn in_load_range(screen_x: f32, view: f32) -> bool {
    screen_x >= -EVENT_LOAD_BUFFER * view && screen_x < (1.0 + EVENT_LOAD_BUFFER) * view
}

/// One tick of the event loader (`0xE423A0`): once `EventLoadIntervalMS` have passed it loads
/// the first unloaded card in range and unloads the first loaded card out of range, then restarts
/// the clock. `screen_x[i]` is the card's current screen x. Returns (loaded, unloaded).
pub(crate) fn load_tick(
    motion: &mut [CardMotion],
    screen_x: &[f32],
    view: f32,
    load_ms: &mut i32,
    dt_ms: i32,
) -> (Option<usize>, Option<usize>) {
    *load_ms += dt_ms;
    if *load_ms <= EVENT_LOAD_INTERVAL_MS {
        return (None, None);
    }
    let load = (0..motion.len()).find(|&i| !motion[i].loaded && in_load_range(screen_x[i], view));
    let unload = (0..motion.len()).find(|&i| motion[i].loaded && !in_load_range(screen_x[i], view));
    if let Some(i) = load {
        motion[i].loaded = true;
    }
    if let Some(i) = unload {
        motion[i].loaded = false;
    }
    if load.is_some() || unload.is_some() {
        *load_ms = 0;
    }
    (load, unload)
}

pub(crate) fn event_def(
    cache: &mut TimelineCache,
    name: &'static str,
    index: &crate::asset_catalog::ResourceIndex,
    assets: &mut SpuiAssets,
) -> Option<(Option<ResourceKey>, String)> {
    cache
        .events
        .entry(name)
        .or_insert_with(|| {
            let key = ResourceKey {
                group_id: EVENT_GROUP,
                instance_id: crate::world_objects::name_id(name),
                type_id: crate::asset_catalog::PROP,
            };
            let props = index.properties(key).ok()?;
            let icon = props.0.get(&PROP_ICON).and_then(|p| p.key());
            let title = dialogs::string_of(&props, PROP_TITLE, index, assets);
            Some((icon, title))
        })
        .clone()
}

pub(crate) fn image_ref(key: ResourceKey) -> ImageRef {
    ImageRef {
        file: key,
        uv: [0.0, 0.0, 1.0, 1.0],
        dims: None,
    }
}

/// Capture-only switches (`--cell-timeline-dismiss-callout <frames>`: click the
/// callout's check button after that many shown frames, `--cell-timeline-cards
/// <n>`: that many extra part cards so that the content outgrows the screen).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CaptureSwitches {
    read: bool,
    pub(crate) dismiss_callout_after: Option<u32>,
    pub(crate) extra_cards: usize,
    /// `--cell-timeline-history demo` / `demo-wide`: [`events::demo_history`] (once /
    /// four times in a row) instead of the run's history; 0 = off.
    pub(crate) demo_history: usize,
}

impl CaptureSwitches {
    pub(crate) fn read_once(&mut self) {
        if self.read {
            return;
        }
        self.read = true;
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u32>().ok())
        };
        self.dismiss_callout_after = value("--cell-timeline-dismiss-callout");
        self.extra_cards = value("--cell-timeline-cards").unwrap_or(0) as usize;
        self.demo_history = match args
            .iter()
            .position(|a| a == "--cell-timeline-history")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
        {
            Some("demo") => 1,
            Some("demo-wide") => 4,
            _ => 0,
        };
    }
}

/// Everything one frame of a summary's scroll machinery needs to know about
/// the shown stage (used by the Cell and the Creature summary).
pub(crate) struct Frame {
    pub(crate) columns: axis::Columns,
    /// The scroll position the window is drawn at.
    pub(crate) shift: f32,
    /// The callout of state 1 is up.
    pub(crate) callout_up: bool,
    /// The earned trait's callout (`0xE3A6E0`, sequencer state 6) is up.
    pub(crate) trait_up: bool,
}

/// One frame of the sequencer and the scroll: creates the sequencer when the summary appears
/// (scroll at the far right, `0xE42100`), steps it (`0xE3B990`), pulls the target back into
/// range and lets the shown position follow (`0xE423A0`). `stage` is the shown stage, `extent`
/// the final x of its events ([`axis::layout_events`]). The Creature summary keeps the trait
/// bar at the right edge and so the [`axis::RIGHT_MARGIN`] behind its cards.
#[allow(clippy::too_many_arguments)]
pub(crate) fn step_frame(
    cache: &mut TimelineCache,
    stage: usize,
    extent: f32,
    dt_ms: i32,
    show: bool,
    has_callout: bool,
    callout_clicked: bool,
    earned_band: usize,
) -> Frame {
    step_frame_margin(
        cache,
        stage,
        extent,
        dt_ms,
        show,
        has_callout,
        callout_clicked,
        earned_band,
        axis::RIGHT_MARGIN,
    )
}

/// [`step_frame`] with the room kept behind the last card: `0xE42100` adds nothing when the
/// last event carries a trait, as the Cell's earned-trait card does (it is the bar itself).
#[allow(clippy::too_many_arguments)]
pub(crate) fn step_frame_margin(
    cache: &mut TimelineCache,
    stage: usize,
    extent: f32,
    dt_ms: i32,
    show: bool,
    has_callout: bool,
    callout_clicked: bool,
    marker_band: usize,
    margin: f32,
) -> Frame {
    let columns = axis::stage_columns(stage, extent);
    let view = axis::VIEW_WIDTH;
    let max = axis::max_scroll_margin(columns.content_width(), view, margin);
    // `+0x28C`: the right edge of the shown stage's last event (the column's
    // right edge, the final x included).
    let end = columns.right(stage);
    if cache.seq.is_none() {
        cache.scroll_shown = max;
        cache.seq = Some(axis::Sequencer::new(end, max, true));
    }
    let Some(seq) = cache.seq.as_mut() else {
        unreachable!("created above");
    };
    if callout_clicked {
        seq.close_callout();
    }
    if show {
        let last = axis::LastEvent { stage, has_trait: true };
        let out = seq.update(dt_ms, view, Some(last), has_callout);
        if out.show_trait.is_some() {
            cache.trait_up = true;
        }
        if out.hide_trait.is_some() {
            cache.trait_up = false;
        }
        seq.scroll = axis::rubber(seq.scroll, max, dt_ms);
        cache.scroll_shown = axis::follow(cache.scroll_shown, seq.scroll, dt_ms);
        // `0xE42C83`: the band the marker sits in fades in, the others out.
        for (band, shown) in cache.bands.iter_mut().enumerate() {
            *shown = axis::band_tint(*shown, axis::band_target(band, marker_band), dt_ms);
        }
    }
    let callout_up = seq.state == axis::SeqState::Callout && has_callout;
    Frame { columns, shift: cache.scroll_shown.round().clamp(0.0, max), callout_up, trait_up: cache.trait_up }
}

/// What backs the History window when the real effect cannot run, and what backs
/// the Creature summary. Retail's show function `0xE44200` plays the screen
/// effect `modalUIBackgroundBlur` (`0x4C4BA0A9`): ten screens, one per app mode;
/// the Cell's (block app flag `0x80`) is the 7-filter chain the Cell History now
/// runs ([`BACKDROP_EFFECT`]). ADAPTED (the Creature summary, and the Cell one
/// without filter chains): a flat opaque panel of the stage's mean world colour
/// stands in for the blurred world (a strong blur tends to the mean; measured on
/// the Cell and Creature captures, 120 frames in; the Cell chain's own result
/// measures (130, 142, 160) under the columns against this panel's (133, 147, 172)),
/// so the screen is opaque as in retail and the column art (translucent) and the
/// band tints sit on it.
pub(crate) fn backdrop_colour(stage: usize) -> Color {
    let (r, g, b) = match stage {
        0 => (133, 147, 172),
        1 => (100, 106, 62),
        _ => (40, 44, 56),
    };
    Color::srgb_u8(r, g, b)
}

/// The screen effect the History's show function plays (`0xE44200` ->
/// `IEffectsManager::CreateVisualEffect(0x4C4BA0A9, world 0)`, then `Start`).
pub(crate) const BACKDROP_EFFECT: &str = "modalUIBackgroundBlur";

/// Whether the History backdrop can be the real screen effect: filter chains on and the
/// effect installed.
fn backdrop_blur_available(library: Option<&crate::swarm_fx::SwarmLibrary>, effect: u32) -> bool {
    crate::swarm_fx::filters::filters_enabled() && library.is_some_and(|l| l.loaded && l.find(effect).is_some())
}

/// The plain UI node that holds the backdrop colour (below the SPUI composite;
/// the caller adds the layouts' `UiTargetCamera`).
pub(crate) fn backdrop_node(stage: usize) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(backdrop_colour(stage)),
        Pickable::IGNORE,
        GlobalZIndex(BACKDROP_Z),
        crate::stages::StageOwned,
    )
}

/// The SPUI layouts are drawn into one surface that a full-screen node shows at
/// `GlobalZIndex` 100 (`spui::surface::COMPOSITE_Z`, the layout roots' own z only
/// orders them inside it): the backdrop sits just below that node, over the world.
pub(crate) const BACKDROP_Z: i32 = crate::spui::surface::COMPOSITE_Z - 1;

/// Spawn the timeline layouts when the summary appears, despawn them when it
/// is closed.
#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    hud: Res<CellHudState>,
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut data: ResMut<CellEditorData>,
    mut cache: ResMut<TimelineCache>,
    library: Option<Res<crate::swarm_fx::SwarmLibrary>>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &TimelineRoot)>,
    lines: Query<Entity, With<TimelineLine>>,
) {
    let want = *stage.get() == GameStage::Cell && hud.summary.is_some();
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        for e in &lines {
            commands.entity(e).despawn();
        }
        cache.frames = 0;
        cache.seq = None;
        cache.bands = [[0.0; 4]; 3];
        cache.trait_up = false;
        cache.intro_ms = 0;
        cache.load_ms = 0;
        cache.motion.clear();
        cache.marker_top = None;
        cache.line_image = None;
        return;
    }
    if !roots.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    let (Ok(camera), Some(summary)) = (camera.single(), hud.summary.as_ref()) else {
        return;
    };
    let index = &originals.resource_index;
    if !data.loaded {
        *data = CellEditorData::load(index);
    }
    cache.capture.read_once();
    let mut slots: Vec<usize> = run
        .cell
        .unlocked_parts
        .iter()
        .enumerate()
        .filter(|(slot, v)| **v > 0 && !START_SLOTS.contains(slot))
        .map(|(slot, _)| slot)
        .collect();
    // Capture only: `--cell-timeline-cards <n>` adds part cards.
    if cache.capture.extra_cards > 0 {
        let extra = (0..run.cell.unlocked_parts.len().max(32))
            .filter(|slot| !START_SLOTS.contains(slot) && !slots.contains(slot))
            .take(cache.capture.extra_cards)
            .collect::<Vec<_>>();
        slots.extend(extra);
    }
    // The history (0xE394A0 records; ADAPTED: a save without them gets the synthetic
    // one of `events::history`) and its cards (0xE3F970 / 0xE3F2A0).
    let recorded = if cache.capture.demo_history > 0 {
        // Later copies carry neither the start nor part finds (a part is found once).
        (0..cache.capture.demo_history)
            .flat_map(|copy| {
                events::demo_history().into_iter().filter(move |e| {
                    copy == 0 || !matches!(e.kind, events::EventKind::GameStart | events::EventKind::Part(_))
                })
            })
            .collect()
    } else {
        run.cell.timeline.clone()
    };
    let fallback = if recorded.is_empty() {
        diet_value_of(&summary.diet)
    } else {
        run.cell.diet_value()
    };
    let (kills, deaths) = if cache.capture.demo_history > 0 { (0, 0) } else { (summary.kills, summary.deaths) };
    let mut history = events::history(&recorded, &slots, kills, deaths, fallback);
    // The starting parts have no card.
    history.retain(|e| !matches!(e.kind, events::EventKind::Part(slot) if START_SLOTS.contains(&(slot as usize))));
    cache.metrics.clear();
    for name in [LAYOUT_LARGE, LAYOUT_PART, LAYOUT_ICON, LAYOUT_DEATH, LAYOUT_TRAIT_BAR] {
        if let Some(m) = layout_metrics(&mut assets, index, name) {
            cache.metrics.insert(name, m);
        }
    }
    let part_height = cache.metrics.get(LAYOUT_PART).map_or(100.0, |m| m.size.y);
    let stacks = events::stacks(&history, axis::COLUMN_HEIGHT, part_height);
    cache.part_icons.clear();
    for event in &history {
        let events::EventKind::Part(slot) = event.kind else {
            continue;
        };
        let slot = slot as usize;
        let icon = data
            .parts
            .values()
            .filter(|p| p.unlock == Some(slot))
            .min_by_key(|p| p.key.instance_id)
            .map(|p| p.icon);
        cache.part_icons.insert(slot, icon);
    }
    let mut spawn = |commands: &mut Commands, name: &str, kind: TimelineRoot, policy, z| {
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert((kind, crate::stages::StageOwned));
        }
    };
    // The backdrop: retail plays `modalUIBackgroundBlur` over the paused world (`0xE44200`); the flat
    // panel stays as the fallback when the filter chains cannot run it (`OS_FILTERS=0`, no effect).
    let blur = crate::world_objects::name_id(BACKDROP_EFFECT);
    if backdrop_blur_available(library.as_deref(), blur) {
        commands.spawn((SwarmEffect::new(blur), TimelineRoot::BackdropBlur, crate::stages::StageOwned));
    } else {
        commands.spawn((backdrop_node(0), UiTargetCamera(camera), TimelineRoot::Backdrop));
    }
    // The main layout (column art, band tints, the marker) sits under the line, the cards
    // (z 105) over it: the retail paint order of `0xE45760`.
    spawn(&mut commands, TIMELINE_LAYOUT, TimelineRoot::Main, RootPolicy::Fit, MAIN_Z);
    spawn(&mut commands, LAYOUT_TRAIT_BAR, TimelineRoot::TraitBar, RootPolicy::Authored, 105);
    for (index_of_stack, stack) in stacks.iter().enumerate() {
        for kind in stack_cards(index_of_stack, stack) {
            let layout = card_layout(&kind);
            spawn(&mut commands, layout, TimelineRoot::Card(kind), RootPolicy::Authored, 105);
        }
    }
    // One motion record per stack and one for the trait bar, the last card.
    cache.motion = vec![CardMotion::default(); stacks.len() + 1];
    cache.load_ms = EVENT_LOAD_INTERVAL_MS;
    cache.stacks = stacks;
    spawn(&mut commands, LAYOUT_TRAIT_CALLOUT, TimelineRoot::TraitCallout, RootPolicy::Authored, 106);
    spawn(&mut commands, dialogs::CALLOUT_LAYOUT, TimelineRoot::Callout, RootPolicy::Authored, 106);
    cache.frames = 0;
}

/// Fill the stage columns, tabs and clock of the main layout.
fn bind_main(ui: &mut SpuiState, summary: &CellSummary, show: bool, frame: &Frame, bands: &[[f32; 4]; 3]) {
    bind_main_columns(ui, 0, clock_numbers(summary.game_time_seconds), show, frame, bands);
}

/// The main layout for the summary of stage `stage` (0 Cell, 1 Creature, ...)
/// with the stage columns spread over the full width (no events, no scroll).
#[allow(dead_code)]
pub(crate) fn bind_main_stage(ui: &mut SpuiState, stage_index: usize, clock: [u32; 4], show: bool) {
    let frame = Frame {
        columns: axis::stage_columns(stage_index, axis::VIEW_WIDTH),
        shift: 0.0,
        callout_up: false,
        trait_up: false,
    };
    bind_main_columns(ui, stage_index, clock, show, &frame, &[[0.0; 4]; 3]);
}

/// The main layout for the summary of stage `stage`: the stage columns where
/// the retail layout puts them (`0xE3EE30`), shifted by the scroll, that
/// stage's tab is the selected one, the clock shows `clock` and the band
/// windows carry their tint (`0xE42C83`).
pub(crate) fn bind_main_columns(
    ui: &mut SpuiState,
    stage_index: usize,
    clock: [u32; 4],
    show: bool,
    frame: &Frame,
    bands: &[[f32; 4]; 3],
) {
    let Some(root) = ui.layout.roots.first().copied() else {
        return;
    };
    ui.set_visible(root, show);
    // Columns without events have no width and are hidden (`0xE44200`).
    for stage in 0..5 {
        let (left, width) = (frame.columns.left[stage] - frame.shift, frame.columns.width[stage]);
        for control in [win::BACK[stage], win::FRONT[stage]] {
            if let Some(w) = ui.find(control) {
                let a = ui.layout.window(w).area;
                ui.set_visible(w, frame.columns.visible(stage));
                ui.set_area(w, [left, a[1], left + width, a[3]]);
            }
        }
    }
    for (band, control) in win::BANDS.iter().enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_shade(w, axis::pack_argb(bands[band]));
        }
    }
    // The stage's tab is the selected one: its overlay shows and its icon is
    // bright, the others keep the authored dim look.
    for (stage, (tab, selected)) in win::TABS.iter().zip(win::TAB_SELECTED).enumerate() {
        let (Some(tab), Some(overlay)) = (ui.find(*tab), ui.find(selected)) else {
            continue;
        };
        ui.set_visible(overlay, stage == stage_index);
        if stage == stage_index {
            if let Some(icon) = ui.layout.window(tab).children.last().copied() {
                ui.set_shade(icon, 0xFFFF_FFFF);
            }
        }
    }
    for (index, (control, value)) in win::CLOCK.iter().zip(clock).enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_text(w, clock_text(index, value));
        }
    }
    // `0xE44200` dims the five bottom stage pills.
    for pill in win::PILLS {
        if let Some(w) = ui.find(pill) {
            ui.set_shade(w, PILL_SHADE);
        }
    }
}

/// Position the root window of a card layout.
pub(crate) fn place_card_root(ui: &mut SpuiState, at: (f32, f32)) -> Option<Vec2> {
    let root = ui.find(card::ROOT)?;
    let a = ui.layout.window(root).area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    ui.set_area(root, [at.0, at.1, at.0 + w, at.1 + h]);
    Some(Vec2::new(w, h))
}

/// Size of a card layout's root window (authored).
pub(crate) fn card_size(ui: &SpuiState) -> Vec2 {
    ui.find(card::ROOT).map_or(Vec2::new(60.0, 60.0), |r| {
        let a = ui.layout.window(r).area;
        Vec2::new(a[2] - a[0], a[3] - a[1])
    })
}

/// Size and drawn extent of a card layout, solved at the authored root
/// (`0xE3F010` caches both per layout: the size of window `0x94E2C6D2` and the
/// union of the layout's windows, `0x8088A0`, as `+0x78..+0x84`): the root's size
/// and `y1` / `y2` of the union relative to the root's top. `0xE46D80` keeps this
/// extent inside the column. (The raw areas of a layout file are not final: the
/// solver applies the windows' anchors.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardMetrics {
    pub(crate) size: Vec2,
    pub(crate) extent: (f32, f32),
    /// The root window's authored position: where a loaded card starts before it glides to
    /// its target (`0xE48DC0` loads the layout, `0xE46880` moves it).
    pub(crate) origin: Vec2,
}

pub(crate) fn card_metrics(layout: &crate::spui::model::Layout) -> Option<CardMetrics> {
    use crate::spui::solve::{authored_areas, solve};
    let root = layout.find(card::ROOT)?;
    let rects = solve(layout, &authored_areas(layout), [1280.0, 900.0], RootPolicy::Authored);
    let r = *rects.get(root)?;
    let (mut y1, mut y2) = (f32::MAX, f32::MIN);
    for rect in &rects {
        y1 = y1.min(rect.y1);
        y2 = y2.max(rect.y2);
    }
    Some(CardMetrics {
        size: Vec2::new(r.x2 - r.x1, r.y2 - r.y1),
        extent: (y1 - r.y1, y2 - r.y1),
        origin: Vec2::new(r.x1, r.y1),
    })
}

fn layout_metrics(assets: &mut SpuiAssets, index: &crate::asset_catalog::ResourceIndex, name: &str) -> Option<CardMetrics> {
    card_metrics(&*assets.layout(index, name)?)
}

/// The shades `0xE46210` / `0xE47930` give the trait slots: the earned one `0xFFFFFFFF` (the
/// sum `0xFF737373 + 0x8C8C8C`), the others the dim `0xFF737373`.
pub(crate) const TRAIT_SHADE_EARNED: u32 = 0xFFFF_FFFF;
pub(crate) const TRAIT_SHADE_OTHER: u32 = 0xFF73_7373;

/// Fill the trait bar (the stage's last event card, `0x0EE4273F`) with the three trait icons,
/// the earned one bright, and put it at `at` (top left, column pixels already shifted).
fn bind_trait_bar(
    ui: &mut SpuiState,
    summary: &CellSummary,
    icons: &[Option<ResourceKey>; 3],
    show: bool,
    at: (f32, f32),
) {
    let Some(root) = ui.find(card::ROOT) else {
        return;
    };
    ui.set_visible(root, show);
    let a = ui.layout.window(root).area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    ui.set_area(root, [at.0, at.1, at.0 + w, at.1 + h]);
    let earned = trait_slot(&summary.diet);
    for (slot, control) in TRAIT_SLOTS.iter().enumerate() {
        let Some(w) = ui.find(*control) else {
            continue;
        };
        ui.set_image(w, icons[slot].map(image_ref));
        ui.set_shade(w, if slot == earned { TRAIT_SHADE_EARNED } else { TRAIT_SHADE_OTHER });
    }
}

/// The earned trait's callout (`0xE3A6E0` creates a `TraitRollover` window for the stage's
/// trait and shows it for `CinematicStateShowEarnedTraitTimeMS`).
///
/// VERIFIED (march2017): the window is attached to the stage tab window (`0x6771E0`, 0x677244..0x67728D: when the
/// callout root has no parent, `tab->AddWindow(root)` (vtable +0xD8), `BringToFront` (+0xE8) and
/// `root->SetLayoutLocation(0, 0)` (+0x70)), so the callout's top-left corner is the tab's top-left corner and its
/// authored 121 x 147 card covers the 117 x 150 tab. The filler `0x6776C0` (called with the trait key from 0xE3A7F9, no
/// anchor rectangle: its `0x808DD0` placement beside an anchor, left if it fits else right, vertically centred, is
/// not used here) writes: the name (`54D95160`, window `06036C70`), the trait colour (`D4F639CD`, windows `54F63A3E`
/// and `0667D3F8`), the picture (`54D95162`, window `0603A678`), the one of the five badge windows
/// `06133600..04` picked by `54D95164` (all five are empty windows in the installed layout: nothing to draw) and
/// the effect row: `54D95163` lists up to four `ConsequenceSuperPowers` records whose `D4D959E2` picture goes into the
/// windows `04C01BBE..C1` (26 x 26, left to right); with `n` effects only the last `n` windows are shown, and for
/// `n` = 1..3 the chain `D570EDE2 / DE1 / DE0` is moved to keep the group centred (the installed traits all have
/// four effects, which stay where the layout puts them).
fn bind_trait_callout(
    ui: &mut SpuiState,
    on: bool,
    name: &str,
    icon: Option<ResourceKey>,
    colour: [f32; 3],
    effects: &[Option<ResourceKey>],
) {
    let Some(root) = ui.find(rollover::ROOT) else {
        return;
    };
    ui.set_visible(root, on);
    if !on {
        return;
    }
    let a = ui.layout.window(root).area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    ui.set_area(root, [CELL_TAB.x1, CELL_TAB.y1, CELL_TAB.x1 + w, CELL_TAB.y1 + h]);
    if let Some(w) = ui.find(rollover::NAME) {
        ui.set_text(w, name.to_string());
    }
    if let Some(w) = ui.find(rollover::ICON) {
        ui.set_image(w, icon.map(image_ref));
    }
    // The ring and the body are authored in green (`FF29FF06`) and painted in the trait's colour
    // (`D4F639CD` of its template, [`axis::BAND_COLOURS`]).
    for control in [rollover::RING, rollover::BODY] {
        if let Some(w) = ui.find(control) {
            ui.set_shade(w, axis::pack_argb([colour[0], colour[1], colour[2], 1.0]));
        }
    }
    // The effect row (`0x6776C0`: window i is shown when i >= 4 - n).
    let n = effects.len().min(4);
    for i in 0..4usize {
        let Some(w) = ui.find(rollover::EFFECTS[i]) else {
            continue;
        };
        let shown = i >= 4 - n;
        ui.set_visible(w, shown);
        if shown {
            ui.set_image(w, effects.get(i + n - 4).copied().flatten().map(image_ref));
        }
    }
}

/// Fill one event card.
#[allow(clippy::too_many_arguments)]
fn bind_card(
    ui: &mut SpuiState,
    kind: &CardKind,
    rect: Rect,
    show: bool,
    index: &crate::asset_catalog::ResourceIndex,
    assets: &mut SpuiAssets,
    icon: Option<ResourceKey>,
) {
    let Some(root) = ui.find(card::ROOT) else {
        return;
    };
    ui.set_visible(root, show);
    place_card_root(ui, (rect.x1, rect.y1));
    let timeline_text = |assets: &mut SpuiAssets, id: u32| {
        assets
            .localized(index, crate::world_objects::name_id("timeline"), id)
            .unwrap_or_default()
    };
    let set_text = |ui: &mut SpuiState, control: u32, text: String| {
        if let Some(w) = ui.find(control) {
            ui.set_text(w, text);
        }
    };
    match kind {
        CardKind::GameStart => {
            // "Generation ~timelineGenerationCount~".
            let title = timeline_text(assets, 0x060B_4210).replace("~timelineGenerationCount~", "1");
            set_text(ui, card::LABEL, title);
        }
        CardKind::Evolve(n) => {
            // `clg_evolveineditor`: the same card ("Generation ~timelineGenerationCount~").
            let title = timeline_text(assets, 0x060B_4212).replace("~timelineGenerationCount~", &(n + 2).to_string());
            set_text(ui, card::LABEL, title);
        }
        CardKind::Part(_) => {
            if let Some(w) = ui.find(card::IMAGE) {
                ui.set_image(w, icon.map(image_ref));
            }
        }
        CardKind::Meals { count, .. } | CardKind::Kills { count, .. } => {
            if let Some(w) = ui.find(card::ICON) {
                ui.set_image(w, icon.map(image_ref));
            }
            if let Some(w) = ui.find(card::NUMBER) {
                ui.set_visible(w, true);
            }
            set_text(ui, card::NUMBER, count.to_string());
        }
        CardKind::Deaths { count, .. } => set_text(ui, card::NUMBER, count.to_string()),
    }
}

/// The node that shows the line texture inside the SPUI surface.
#[derive(Component)]
pub struct TimelineLine;

/// Bind every timeline layout and turn the check button into the dismiss key.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    mut commands: Commands,
    surface: Option<Res<crate::spui::surface::SpuiSurface>>,
    hud: Res<CellHudState>,
    time: Res<Time>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<TimelineCache>,
    mut roots: Query<(&mut SpuiState, &TimelineRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    lines: Query<Entity, With<TimelineLine>>,
) {
    let Some(summary) = hud.summary.clone() else {
        return;
    };
    let index = &originals.resource_index;
    cache.frames += 1;
    let show = cache.frames > SHOW_AFTER_FRAMES;
    // Icons the cards and the trait bar need.
    if !cache.traits_loaded {
        cache.traits_loaded = true;
        for (i, instance) in TRAITS.iter().enumerate() {
            let key = ResourceKey {
                group_id: TRAIT_GROUP,
                instance_id: *instance,
                type_id: crate::asset_catalog::PROP,
            };
            let props = index.properties(key).ok();
            cache.trait_icons[i] = props.as_ref().and_then(|p| p.0.get(&PROP_TRAIT_ICON).and_then(|v| v.key()));
            cache.trait_effects[i] = props
                .as_ref()
                .map(|p| crate::cell_editor_data::key_list(p, PROP_TRAIT_EFFECTS))
                .unwrap_or_default()
                .into_iter()
                .take(4)
                .map(|effect| {
                    index
                        .properties(crate::cell_editor_data::prop_key(SUPER_POWER_GROUP, effect.instance_id))
                        .ok()
                        .and_then(|p| crate::cell_editor_data::key_of(&p, PROP_EFFECT_ICON))
                })
                .collect();
        }
    }
    if !cache.callout_loaded {
        cache.callout_loaded = true;
        cache.callout = dialogs::read_callout(index, &mut assets, TRANSITION_CALLOUT);
    }
    let kills_icon = event_def(&mut cache, "clg_killedsomething", index, &mut assets).and_then(|(icon, _)| icon);
    let plant_icon = event_def(&mut cache, "clg_eatplant", index, &mut assets).and_then(|(icon, _)| icon);
    let meat_icon = event_def(&mut cache, "clg_eatmeat", index, &mut assets).and_then(|(icon, _)| icon);
    let part_icons = cache.part_icons.clone();
    let trait_icons = cache.trait_icons;
    let trait_effects = cache.trait_effects.clone();
    let callout = cache.callout.clone();
    for icon in trait_icons
        .iter()
        .flatten()
        .chain(trait_effects.iter().flatten().flatten())
        .chain(kills_icon.iter())
        .chain(plant_icon.iter())
        .chain(meat_icon.iter())
        .chain(part_icons.values().flatten())
    {
        let _ = assets.texture(index, &mut images, *icon);
    }
    // Cards in history order (`events::stacks`), laid out one behind the other
    // (`0xE3F970`: x from 5, every card advances by its width + 5) and placed at the
    // height of their disposition value (`0xE46D80`). The earned-trait card, the trait bar,
    // is the last of them (`0xE3BFB0`).
    let metrics = cache.metrics.clone();
    let metric = |k: &CardKind| metrics.get(card_layout(k)).copied();
    let stacks = cache.stacks.clone();
    let bar = metrics.get(LAYOUT_TRAIT_BAR).copied();
    let mut widths: Vec<f32> = stacks
        .iter()
        .enumerate()
        .map(|(i, st)| stack_cards(i, st).first().and_then(|k| metric(k)).map_or(0.0, |m| m.size.x))
        .collect();
    widths.push(bar.map_or(0.0, |m| m.size.x));
    let (xs, extent) = axis::layout_events(&widths);
    // The intro sequencer and the scroll (`0xE3B990`, `0xE423A0`).
    let dt_ms = (time.delta_secs() * 1000.0).round().max(0.0) as i32;
    let clicked_callout = cache.capture.dismiss_callout_after.is_some_and(|n| cache.frames > SHOW_AFTER_FRAMES + n);
    let mut callout_clicked = clicked_callout;
    // `0xE42E71`: the band follows the height of the marker window of the frame before.
    let marker_band = cache
        .marker_top
        .map_or(1, |top| axis::marker_band((top - axis::COLUMN_TOP) / axis::COLUMN_HEIGHT));
    let frame = step_frame_margin(
        &mut cache,
        0,
        extent,
        dt_ms,
        show,
        callout.is_some(),
        callout_clicked,
        marker_band,
        0.0,
    );
    let bands = cache.bands;
    let column_left = frame.columns.left[0] - frame.shift;
    // Target place of every card in the column (the cards of a stack share its glide).
    let h = axis::COLUMN_HEIGHT;
    let mut targets: Vec<Vec<(CardKind, f32, f32, Vec2)>> = Vec::with_capacity(stacks.len() + 1);
    let mut event_points: Vec<line::Point> = Vec::with_capacity(stacks.len() + 1);
    let mut origins: Vec<Vec2> = Vec::with_capacity(stacks.len() + 1);
    for (i, stack) in stacks.iter().enumerate() {
        let kinds = stack_cards(i, stack);
        let Some(CardMetrics { size, extent: extent_y, origin }) = kinds.first().and_then(|k| metric(k)) else {
            targets.push(Vec::new());
            event_points.push([f32::NAN, f32::NAN]);
            origins.push(Vec2::ZERO);
            continue;
        };
        let factor = axis::event_height_factor(stack.value, false);
        let top = axis::card_top(h, factor, size.y);
        let tops = if stack.kind == events::StackKind::Parts {
            axis::stack_tops(top, kinds.len(), size.y, h)
        } else {
            vec![axis::clamp_card_top(top, h, extent_y)]
        };
        // `0xE46D80`: the event point is the centre of the unclamped card.
        event_points.push([xs[i] + size.x * 0.5, axis::COLUMN_TOP + top + size.y * 0.5]);
        origins.push(origin);
        targets.push(kinds.into_iter().zip(tops).map(|(k, t)| (k, xs[i], t, size)).collect());
    }
    // The trait bar: not on the line (`0xE46D80`), so it is centred in the column: `0xE3F970` /
    // `0xE37AF0` hand `(columnH - cardH) / 2` as its y.
    if let Some(bar) = bar {
        targets.push(vec![(CardKind::GameStart, xs[stacks.len()], axis::centred_top(h, bar.size.y), bar.size)]);
        // `0xE37F40`: the trait event takes its own x and the height of the event before it.
        let prev_y = event_points
            .iter()
            .rev()
            .find(|p| p[1].is_finite())
            .map_or(axis::COLUMN_TOP + h * 0.5, |p| p[1]);
        event_points.push([xs[stacks.len()] + bar.size.x * 0.5, prev_y]);
        origins.push(bar.origin);
    } else {
        targets.push(Vec::new());
        event_points.push([f32::NAN, f32::NAN]);
        origins.push(Vec2::ZERO);
    }
    // The event loader and the glide (`0xE423A0`, `0xE46880`).
    if show {
        cache.intro_ms = (cache.intro_ms + dt_ms).clamp(0, line::INTRO_MS);
        if cache.motion.len() != targets.len() {
            cache.motion = vec![CardMotion::default(); targets.len()];
        }
        // ADAPTED: the range test reads the card's target x; retail reads the window's current x
        // (`0xE42525`), which would drop a card that is still gliding in from far away.
        let screen_x: Vec<f32> = targets
            .iter()
            .enumerate()
            .map(|(_, t)| column_left + t.first().map_or(0.0, |(_, x, _, _)| *x))
            .collect();
        let mut load_ms = cache.load_ms;
        let (loaded, _) = load_tick(&mut cache.motion, &screen_x, axis::VIEW_WIDTH, &mut load_ms, dt_ms);
        cache.load_ms = load_ms;
        if let Some(i) = loaded {
            // The layout is loaded at its authored position and glides from there.
            if let Some((_, x, top, _)) = targets[i].first() {
                cache.motion[i].offset = origins[i] - Vec2::new(*x, *top);
            }
        }
        for motion in cache.motion.iter_mut().filter(|m| m.loaded) {
            motion.offset = glide(motion.offset, dt_ms);
        }
    }
    let motion = cache.motion.clone();
    let mut rects: Vec<(CardKind, Rect)> = Vec::new();
    let mut bar_at: Option<(f32, f32)> = None;
    let mut bar_loaded = false;
    for (i, cards) in targets.iter().enumerate() {
        let m = motion.get(i).copied().unwrap_or_default();
        for (kind, x, top, size) in cards {
            let (px, py) = (column_left + x + m.offset.x, axis::COLUMN_TOP + top + m.offset.y);
            if i == stacks.len() {
                bar_at = Some((px, py));
                bar_loaded = m.loaded;
            } else if m.loaded {
                rects.push((kind.clone(), Rect::new(px, py, px + size.x, py + size.y)));
            }
        }
    }
    // The disposition line (`0xE45760` -> `0xE3C150`) through the event points, painted into a
    // texture the surface shows between the main layout and the cards. A card that is not loaded
    // yet is not an event point of the drawn line either (`0xE37E00` skips windows without state).
    let shown_points: Vec<line::Point> = event_points
        .iter()
        .enumerate()
        .filter(|(i, p)| p[0].is_finite() && p[1].is_finite() && motion.get(*i).is_some_and(|m| m.loaded))
        .map(|(_, p)| [column_left + p[0], p[1]])
        .collect();
    let mut line_cols = line::Columns::default();
    let canvas = surface
        .as_ref()
        .map_or(UVec2::new(1280, 900), |s| if s.size.x > 0 { s.size } else { UVec2::new(1280, 900) });
    if show && !shown_points.is_empty() {
        let reveal = line::intro_reveal(cache.intro_ms, canvas.x as f32);
        let samples = line::samples(&shown_points, line::THICKNESS);
        line_cols = line::silhouette(&samples, &shown_points, line::CIRCLE_RADIUS, reveal, canvas.x as usize);
    }
    if let Some(camera) = surface.as_ref().and_then(|s| s.camera) {
        let size = canvas;
        let rebuild = cache.line_image.as_ref().is_none_or(|(_, s)| *s != size) || lines.is_empty();
        if rebuild {
            for e in &lines {
                commands.entity(e).despawn();
            }
            let image = Image::new(
                bevy::render::render_resource::Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
                bevy::render::render_resource::TextureDimension::D2,
                vec![0u8; (size.x * size.y * 4) as usize],
                bevy::render::render_resource::TextureFormat::Rgba8Unorm,
                bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            let handle = images.add(image);
            cache.line_image = Some((handle.clone(), size));
            commands.spawn((
                TimelineLine,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(size.x as f32),
                    height: Val::Px(size.y as f32),
                    ..default()
                },
                ImageNode::new(handle),
                Pickable::IGNORE,
                GlobalZIndex(LINE_Z),
                UiTargetCamera(camera),
                crate::stages::StageOwned,
            ));
        } else if cache.line_painted != line_cols {
            let handle = cache.line_image.as_ref().map(|(h, _)| h.clone());
            if let Some(mut image) = handle.and_then(|h| images.get_mut(&h)) {
                if let Some(data) = image.data.as_mut() {
                    line::paint(&line_cols, size.x as usize, size.y as usize, data);
                }
            }
            cache.line_painted = line_cols.clone();
        }
    }
    let mut new_marker_top: Option<f32> = None;
    let mut dismiss = false;
    for (mut ui, kind) in &mut roots {
        match kind {
            TimelineRoot::Main => {
                bind_main(&mut ui, &summary, show, &frame, &bands);
                new_marker_top = place_marker(&mut ui, &line_cols, show);
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == win::OK {
                        dismiss = true;
                    }
                }
            }
            TimelineRoot::Callout => {
                dialogs::bind_callout(
                    &mut ui,
                    callout.as_ref().filter(|_| show && frame.callout_up),
                    None,
                    CALLOUT_CENTRE_Y,
                );
                for click in ui.take_clicks() {
                    if ui.layout.window(click).control_id == dialogs::callout::OK {
                        callout_clicked = true;
                    }
                }
            }
            TimelineRoot::TraitCallout => {
                let earned = trait_slot(&summary.diet);
                let name = assets.localized(index, TRAIT_TABLE, TRAIT_NAME_IDS[earned]).unwrap_or_default();
                bind_trait_callout(
                    &mut ui,
                    show && frame.trait_up,
                    &name,
                    trait_icons[earned],
                    axis::BAND_COLOURS[earned],
                    &trait_effects[earned],
                );
            }
            TimelineRoot::TraitBar => {
                if let Some(at) = bar_at {
                    let at = if bar_loaded { at } else { (-4000.0, -4000.0) };
                    bind_trait_bar(&mut ui, &summary, &trait_icons, show && bar_loaded, at);
                }
            }
            TimelineRoot::Backdrop | TimelineRoot::BackdropBlur => {}
            TimelineRoot::Card(k) => {
                let Some((_, rect)) = rects.iter().find(|(c, _)| c == k) else {
                    // Not loaded (`0xE48BB0` frees the window): out of sight and out of the way.
                    if let Some(root) = ui.find(card::ROOT) {
                        ui.set_visible(root, false);
                        place_card_root(&mut ui, (-4000.0, -4000.0));
                    }
                    continue;
                };
                let icon = match k {
                    CardKind::Part(slot) => part_icons.get(slot).copied().flatten(),
                    CardKind::Kills { .. } => kills_icon,
                    CardKind::Meals { plant: true, .. } => plant_icon,
                    CardKind::Meals { plant: false, .. } => meat_icon,
                    _ => None,
                };
                bind_card(&mut ui, k, *rect, show, index, &mut assets, icon);
            }
        }
    }
    if new_marker_top.is_some() {
        cache.marker_top = new_marker_top;
    }
    // `0xE36F80`: the dismissed callout moves the sequencer on.
    if callout_clicked && !clicked_callout {
        if let Some(seq) = cache.seq.as_mut() {
            seq.close_callout();
        }
    }
    if dismiss && !keys.pressed(crate::cell_flow::DISMISS_KEY) {
        keys.press(crate::cell_flow::DISMISS_KEY);
        injected.push(crate::cell_flow::DISMISS_KEY);
    }
}

/// `0xE3C28B`..`0xE3C3C6`: put the vertical marker window on the line at its x (centre
/// anchored, ADAPTED: see [`line`]); it keeps its place while the line does not reach it.
/// Returns the top of the marker window, which `0xE42E71` turns into the band.
fn place_marker(ui: &mut SpuiState, cols: &line::Columns, show: bool) -> Option<f32> {
    let marker = ui.find(win::MARKER)?;
    if !ui.ready {
        return None;
    }
    let rect = ui.rect(marker);
    if show {
        if let Some(y) = line::centre_at(cols, (rect.x1 + rect.x2) * 0.5) {
            let parent = ui.layout.window(marker).parent.map_or((0.0, 0.0), |p| {
                let r = ui.rect(p);
                (r.x1, r.y1)
            });
            let top = y - rect.height() * 0.5;
            // The window is hidden in the layout; the line passing it brings it up.
            ui.set_visible(marker, true);
            ui.set_area(
                marker,
                [rect.x1 - parent.0, top - parent.1, rect.x2 - parent.0, top + rect.height() - parent.1],
            );
            return Some(top);
        }
    }
    Some(rect.y1)
}

/// A creature thumbnail over a generation card's image slot (`slot` as in [`thumbs`]: `usize::MAX` is the live
/// portrait of the HUD, shown on the first card until the player has been in the Creator, 0 the neutral cell of the
/// first card after that, `n + 1` the creature of editor visit `n`). Plain nodes above the HUD surface, like the
/// portrait itself.
#[derive(Component)]
pub struct TimelineThumbnail(pub usize);

/// Keep a thumbnail node over the image slot of the first card and of every editor-visit card.
#[allow(clippy::too_many_arguments)]
pub fn thumbnail(
    mut commands: Commands,
    hud: Res<CellHudState>,
    state: Res<portrait::PortraitState>,
    generations: Res<thumbs::GenerationThumbs>,
    roots: Query<(&SpuiState, &TimelineRoot)>,
    mut nodes: Query<(Entity, &TimelineThumbnail, &mut Node, &mut Visibility, &mut ImageNode)>,
    cache: Res<TimelineCache>,
) {
    let on = hud.summary.is_some() && cache.frames > SHOW_AFTER_FRAMES + 20;
    let visits = thumbs::visits(&cache);
    // (slot, the image slot's rectangle, the screen, the picture)
    let mut wanted: Vec<(usize, Rect, [f32; 2], Handle<Image>)> = Vec::new();
    if on {
        for (ui, kind) in &roots {
            if !ui.ready {
                continue;
            }
            let (slot, image) = match kind {
                TimelineRoot::Card(CardKind::GameStart) if visits == 0 => (usize::MAX, state.image()),
                TimelineRoot::Card(CardKind::GameStart) => (0, generations.image(0)),
                TimelineRoot::Card(CardKind::Evolve(n)) => (n + 1, generations.image(n + 1)),
                _ => continue,
            };
            if let (Some(image), Some(w)) = (image, ui.find(card::IMAGE)) {
                wanted.push((slot, ui.rect(w), ui.screen, image));
            }
        }
    }
    for (e, thumbnail, ..) in &nodes {
        if !wanted.iter().any(|w| w.0 == thumbnail.0) {
            commands.entity(e).despawn();
        }
    }
    let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
    for (slot, rect, screen, image) in wanted {
        if let Some((_, _, mut node, mut visibility, mut picture)) = nodes.iter_mut().find(|n| n.1 .0 == slot) {
            node.left = pct(rect.x1, screen[0]);
            node.top = pct(rect.y1, screen[1]);
            node.width = pct(rect.width(), screen[0]);
            node.height = pct(rect.height(), screen[1]);
            if picture.image != image {
                picture.image = image;
            }
            *visibility = Visibility::Inherited;
        } else {
            commands.spawn((
                TimelineThumbnail(slot),
                Node {
                    position_type: PositionType::Absolute,
                    left: pct(rect.x1, screen[0]),
                    top: pct(rect.y1, screen[1]),
                    width: pct(rect.width(), screen[0]),
                    height: pct(rect.height(), screen[1]),
                    ..default()
                },
                ImageNode::new(image),
                Pickable::IGNORE,
                GlobalZIndex(107),
                crate::stages::StageOwned,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(kills: i32, deaths: i32, diet: &str) -> CellSummary {
        CellSummary {
            game_time_seconds: 1500.0,
            kills,
            deaths,
            diet: diet.into(),
            dna: 100,
        }
    }

    #[test]
    fn history_cards_follow_the_run() {
        let cards = |s: &CellSummary, slots: &[usize]| {
            let h = events::history(&[], slots, s.kills, s.deaths, 0.0);
            events::stacks(&h, axis::COLUMN_HEIGHT, 100.0)
                .iter()
                .enumerate()
                .flat_map(|(i, st)| stack_cards(i, st))
                .collect::<Vec<_>>()
        };
        assert_eq!(cards(&summary(0, 0, "clg_mix"), &[]), vec![CardKind::GameStart]);
        let kinds = cards(&summary(12, 2, "clg_meat"), &[7, 2, 7, 5]);
        // The three parts are one stack (newest, slot 7, on top); the kills and the
        // deaths are merged cards with their counts.
        assert_eq!(
            kinds,
            vec![
                CardKind::GameStart,
                CardKind::Part(7),
                CardKind::Part(5),
                CardKind::Part(2),
                CardKind::Kills { stack: 2, count: 12 },
                CardKind::Deaths { stack: 3, count: 2 }
            ]
        );
    }

    #[test]
    fn cards_sit_in_a_row_on_the_centre_line() {
        let rects = place_cards(&[Vec2::new(100.0, 200.0), Vec2::new(50.0, 50.0)], 40.0, 300.0, 30.0);
        assert_eq!(rects[0], Rect::new(40.0, 200.0, 140.0, 400.0));
        assert_eq!(rects[1], Rect::new(170.0, 275.0, 220.0, 325.0));
    }

    #[test]
    fn a_summary_waits_for_the_callout_then_pauses_scrolls_and_tints_the_earned_band() {
        let mut cache = TimelineCache::default();
        // A wide history: the column is 2000 px, so the window starts at the
        // far right and the scripted scroll has somewhere to go.
        let max = axis::max_scroll(2000.0, axis::VIEW_WIDTH);
        assert!(max > 700.0);
        let frame = step_frame(&mut cache, 0, 2000.0, 16, true, true, false, 2);
        assert!(frame.callout_up, "state 1 shows the callout");
        assert_eq!(frame.shift, max, "0xE42100: the window starts at the far right");
        for _ in 0..30 {
            let frame = step_frame(&mut cache, 0, 2000.0, 16, true, true, false, 2);
            assert!(frame.callout_up && frame.shift == max, "nothing moves while the callout waits");
        }
        // The dismissal starts the sequence: the scroll target jumps back one
        // page before the end and the shown position follows it.
        let frame = step_frame(&mut cache, 0, 2000.0, 16, true, true, true, 2);
        assert!(!frame.callout_up);
        let mut moved_back = false;
        for _ in 0..80 {
            let frame = step_frame(&mut cache, 0, 2000.0, 16, true, true, false, 2);
            moved_back |= frame.shift < max - 50.0;
        }
        assert!(moved_back, "the pause is spent at the start of the scroll");
        assert_eq!(cache.seq.as_ref().map(|s| s.state), Some(axis::SeqState::Pause));
        // 1.5 s pause + 1.5 s scroll + 1.5 s pause + 1.5 s trait: done.
        for _ in 0..400 {
            step_frame(&mut cache, 0, 2000.0, 16, true, true, false, 2);
        }
        assert_eq!(cache.seq.as_ref().map(|s| s.state), Some(axis::SeqState::Done));
        let frame = step_frame(&mut cache, 0, 2000.0, 16, true, true, false, 2);
        assert!((frame.shift - max).abs() <= 1.0, "the scroll ends at the end of the history");
        // Only the earned band (bottom, slot 2) is tinted, at TintAlpha.
        assert!((cache.bands[2][3] - axis::TINT_ALPHA).abs() < 0.01, "{:?}", cache.bands);
        assert!(cache.bands[0][3] < 0.01 && cache.bands[1][3] < 0.01);
    }

    #[test]
    fn the_history_window_is_backed_by_an_opaque_panel() {
        for stage in 0..5 {
            assert_eq!(backdrop_colour(stage).to_srgba().alpha, 1.0, "stage {stage}");
        }
        // Just below the SPUI composite and above the world.
        assert_eq!(BACKDROP_Z, crate::spui::surface::COMPOSITE_Z - 1);
        assert!(!axis::SHOW_EMPTY_LATER_COLUMNS, "the retail columns: only stages with events");
    }

    #[test]
    fn diet_picks_the_trait_slot() {
        assert_eq!(trait_slot("clg_plant"), 0);
        assert_eq!(trait_slot("clg_mix"), 1);
        assert_eq!(trait_slot("clg_meat"), 2);
    }

    #[test]
    fn evolution_clock_follows_the_retail_date_formula() {
        // The Cell stage: 10 000 years per second, cut to 100 million.
        assert_eq!(date_years(0, 0, None), 0);
        assert_eq!(date_years(0, 25 * 60, None), 0, "15 million years are below the quantum");
        assert_eq!(date_years(0, 10_000, None), 100_000_000);
        assert_eq!(clock_numbers(0.0), [0, 0, 0, 0]);
        assert_eq!(clock_numbers(25.0 * 60.0), [0, 0, 0, 0]);
        assert_eq!(clock_numbers(10_000.0), [0, 100, 0, 0]);
        // Later stages add the finished stages (1000 s each) and their own time.
        assert_eq!(date_years(1, 0, None), 10_000_000);
        assert_eq!(date_years(1, 1500, None), 11_000_000, "1.5 million cut to 1 million");
        assert_eq!(date_years(2, 3600, None), 10_000_000 + 1_000_000 + 360_000);
        assert_eq!(date_years(3, 0, None), 11_100_000);
        assert_eq!(date_years(4, 7, None), 11_110_000 + 0);
        // A stage still running counts with its recorded length.
        assert_eq!(date_years(1, 0, Some(&[500, 0, 0, 0])), 5_000_000);
    }

    #[test]
    fn clock_windows_group_the_years_in_thousands() {
        assert_eq!(clock_digits(0), [0, 0, 0, 0]);
        assert_eq!(clock_digits(3_950_453_750_100), [999, 999, 999, 999], "clamped to 999 999 999 999");
        assert_eq!(clock_digits(7_453_750_100), [7, 453, 750, 100]);
        assert_eq!(clock_text(0, 7), "7");
        assert_eq!(clock_text(1, 7), "007");
        assert_eq!(clock_text(3, 0), "000");
    }

    #[test]
    fn cards_load_one_at_a_time_inside_the_screen_buffer_and_glide_in() {
        // `0xE423A0`: 30 ms between loads; range = the screen plus a quarter of it on both sides.
        assert!(in_load_range(-320.0, 1280.0) && in_load_range(1599.0, 1280.0));
        assert!(!in_load_range(-321.0, 1280.0) && !in_load_range(1600.0, 1280.0));
        let mut motion = vec![CardMotion::default(); 4];
        let xs = [100.0, 400.0, 900.0, 5000.0];
        let mut load_ms = EVENT_LOAD_INTERVAL_MS;
        // The first tick is already past the interval: card 0 loads.
        assert_eq!(load_tick(&mut motion, &xs, 1280.0, &mut load_ms, 16), (Some(0), None));
        // Too soon for the next one.
        assert_eq!(load_tick(&mut motion, &xs, 1280.0, &mut load_ms, 16), (None, None));
        assert_eq!(load_tick(&mut motion, &xs, 1280.0, &mut load_ms, 16), (Some(1), None));
        for _ in 0..4 {
            load_tick(&mut motion, &xs, 1280.0, &mut load_ms, 16);
        }
        assert_eq!(motion.iter().map(|m| m.loaded).collect::<Vec<_>>(), vec![true, true, true, false], "the far card waits");
        // A loaded card that left the range is unloaded again.
        let moved = [-900.0, 400.0, 900.0, 5000.0];
        load_ms = 100;
        assert_eq!(load_tick(&mut motion, &moved, 1280.0, &mut load_ms, 16), (None, Some(0)));
        // The glide closes dt * 0.008 of the gap per frame (`0xE46880`) and ends on the target.
        let mut offset = Vec2::new(-200.0, 100.0);
        offset = glide(offset, 16);
        assert!((offset.x - (-200.0 * (1.0 - 0.128))).abs() < 1e-3, "{offset:?}");
        for _ in 0..80 {
            offset = glide(offset, 16);
        }
        assert!(offset.length() < 0.01, "{offset:?}");
        assert_eq!(glide(Vec2::new(5.0, 5.0), 200), Vec2::ZERO, "a long frame lands on the target");
    }

    #[test]
    fn the_trait_bar_is_the_last_card_and_the_scroll_keeps_no_room_behind_it() {
        // The bar is centred (`(h - cardH) / 2`), not on the line.
        assert_eq!(axis::centred_top(687.0, 314.0), 186.5);
        // `0xE42100`: no extra width behind a last event with a trait.
        assert_eq!(axis::max_scroll_margin(2135.0, 1280.0, 0.0), 855.0);
        assert_eq!(axis::max_scroll(2135.0, 1280.0), 855.0 + axis::RIGHT_MARGIN, "the Creature summary keeps its margin");
        let mut cache = TimelineCache::default();
        let frame = step_frame_margin(&mut cache, 0, 2135.0, 16, true, true, false, 1, 0.0);
        assert_eq!(frame.shift, 855.0, "the window starts at the far right, flush with the bar");
        // The sequencer's trait state raises the callout flag and its end lowers it.
        cache.seq.as_mut().unwrap().state = axis::SeqState::PauseAfter;
        cache.seq.as_mut().unwrap().timer_ms = 0;
        let frame = step_frame_margin(&mut cache, 0, 2135.0, 16, true, true, false, 1, 0.0);
        assert!(frame.trait_up, "0xE3A6E0 shows the earned trait");
        cache.seq.as_mut().unwrap().timer_ms = 0;
        let frame = step_frame_margin(&mut cache, 0, 2135.0, 16, true, true, false, 1, 0.0);
        assert!(!frame.trait_up, "and hides it again");
    }

    #[test]
    fn the_trait_slots_take_the_retail_shades() {
        assert_eq!(TRAIT_SHADE_EARNED, 0xFF73_7373 + 0x008C_8C8C);
        assert_eq!(TRAIT_SHADE_OTHER, 0xFF73_7373);
    }

    #[test]
    fn installed_history_layouts_and_events_decode() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut assets = SpuiAssets::default();
        let layout = assets.layout(&index, TIMELINE_LAYOUT).expect("timeline layout");
        for control in std::iter::once(win::OK)
            .chain(win::TABS)
            .chain(win::TAB_SELECTED)
            .chain(win::CLOCK)
            .chain(win::BACK)
            .chain(win::FRONT)
            .chain(win::BANDS)
        {
            assert!(layout.find(control).is_some(), "timeline window {control:08X}");
        }
        for name in [LAYOUT_LARGE, LAYOUT_PART, LAYOUT_ICON, LAYOUT_DEATH, LAYOUT_TRAIT_BAR] {
            let card_layout = assets.layout(&index, name).unwrap_or_else(|| panic!("{name}"));
            assert!(card_layout.find(card::ROOT).is_some(), "{name} root");
        }
        let bar = assets.layout(&index, LAYOUT_TRAIT_BAR).unwrap();
        for control in TRAIT_SLOTS {
            assert!(bar.find(control).is_some(), "trait slot {control:08X}");
        }
        // The stage columns the cards live in are 687 px tall (0xE46D80 reads the
        // parent window's height) and every card layout has a drawn extent.
        let solved = crate::spui::solve::solve(
            &layout,
            &crate::spui::solve::authored_areas(&layout),
            [1280.0, 900.0],
            RootPolicy::Fit,
        );
        for stage in 0..5 {
            let back = solved[layout.find(win::BACK[stage]).expect("column")];
            assert_eq!(back.y2 - back.y1, axis::COLUMN_HEIGHT, "column {stage}");
            assert!((back.y1 - axis::COLUMN_TOP).abs() <= 0.5, "column {stage} top {}", back.y1);
        }
        for name in [LAYOUT_LARGE, LAYOUT_PART, LAYOUT_ICON, LAYOUT_DEATH] {
            let m = layout_metrics(&mut assets, &index, name).expect(name);
            eprintln!("card layout {name}: {m:?}");
            assert!(m.size.x > 20.0 && m.size.y > 20.0 && m.size.y < axis::COLUMN_HEIGHT, "{name} {m:?}");
            assert!(m.extent.0 <= 0.5 && m.extent.1 >= m.size.y - 0.5, "{name}: the extent holds the root {m:?}");
        }
        {
            // The trait bar is the last card, centred in the column (`0xE3F970`: `(columnH - cardH) / 2`).
            let m = layout_metrics(&mut assets, &index, LAYOUT_TRAIT_BAR).expect("trait bar metrics");
            eprintln!("card layout {LAYOUT_TRAIT_BAR}: {m:?}");
            assert!(m.size.y < axis::COLUMN_HEIGHT && m.size.x > 50.0, "{m:?}");
            let top = axis::centred_top(axis::COLUMN_HEIGHT, m.size.y);
            // Root top + the drawn extent: the whole bar sits in the column, centred on it.
            let (y1, y2) = (axis::COLUMN_TOP + top + m.extent.0, axis::COLUMN_TOP + top + m.extent.1);
            let column_centre = axis::COLUMN_TOP + axis::COLUMN_HEIGHT * 0.5;
            assert!(y1 >= axis::COLUMN_TOP - 1.0 && y2 <= axis::COLUMN_TOP + axis::COLUMN_HEIGHT + 1.0, "{y1} {y2}");
            assert!(((y1 + y2) * 0.5 - column_centre).abs() < 2.0, "bar centre {} column {column_centre}", (y1 + y2) * 0.5);
            // The earned-trait callout layout (`TraitRollover`) has the windows the callout fills.
            let rollover = assets.layout(&index, LAYOUT_TRAIT_CALLOUT).expect("TraitRollover layout");
            for control in [rollover::ROOT, rollover::NAME, rollover::ICON, rollover::RING, rollover::BODY] {
                assert!(rollover.find(control).is_some(), "rollover window {control:08X}");
            }
            // The band colours are the colour property of the three trait templates (`0xE47930`).
            for (band, template) in [0x9D11_3744u32, 0xEBE9_11E0, 0x1C9B_EF90].into_iter().enumerate() {
                let key = ResourceKey { group_id: TRAIT_GROUP, instance_id: template, type_id: crate::asset_catalog::PROP };
                let props = index.properties(key).expect("trait template");
                let colour = props.0.get(&0xD4F6_39CD).and_then(|p| p.color_rgb()).expect("trait colour");
                for k in 0..3 {
                    assert!((colour[k] - axis::BAND_COLOURS[band][k]).abs() < 0.002, "band {band}: {colour:?}");
                }
            }
            // The three traits resolve their colour through the template (inheritance).
            for (band, instance) in TRAITS.into_iter().enumerate() {
                let key = ResourceKey { group_id: TRAIT_GROUP, instance_id: instance, type_id: crate::asset_catalog::PROP };
                let props = index.properties(key).expect("trait");
                let colour = props.0.get(&0xD4F6_39CD).and_then(|p| p.color_rgb()).expect("inherited trait colour");
                assert!((colour[0] - axis::BAND_COLOURS[band][0]).abs() < 0.002);
            }
            // The trait names (locale `9A5ACF10`).
            for (slot, id) in TRAIT_NAME_IDS.into_iter().enumerate() {
                let name = assets.localized(&index, TRAIT_TABLE, id).expect("trait name");
                assert!(!name.is_empty(), "trait {slot}");
            }
        }
        let mut cache = TimelineCache::default();
        let kills = event_def(&mut cache, "clg_killedsomething", &index, &mut assets).expect("kill event");
        assert!(kills.0.is_some());
        let start = event_def(&mut cache, "clg_gamestart", &index, &mut assets).expect("start event");
        assert!(start.1.starts_with("Generation"), "title {:?}", start.1);
        // The three trait properties carry an icon each.
        for instance in TRAITS {
            let key = ResourceKey {
                group_id: TRAIT_GROUP,
                instance_id: instance,
                type_id: crate::asset_catalog::PROP,
            };
            let props = index.properties(key).expect("trait property");
            assert!(props.0.get(&PROP_TRAIT_ICON).and_then(|p| p.key()).is_some());
        }
        let callout = dialogs::read_callout(&index, &mut assets, TRANSITION_CALLOUT).expect("history callout");
        assert_eq!(callout.title, "History");
        assert!(callout.text.contains("Which path have you chosen?"));
    }
}
