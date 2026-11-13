//! The retail Creature-stage HUD: spawns the SPUI layouts the original game
//! loads for the Creature stage and binds them to [`CreatureHudState`].
//!
//! * `GlobalUICreature3-7`: bottom bar (DNA readout, the four-segment brain
//!   progress bar with its brain icons, History/Sporepedia/collection/pause
//!   buttons), the social/combat stance buttons with their two four-slot
//!   ability trays, the health and hunger meters and the portrait frame.
//! * `GlobalUIOptions-2`: the swirl orb (options/menu button), shared with the
//!   Cell stage (`cell_hud`).
//! * `CRGMissionCard`: the goal cards at the top left ([`guide`]).
//! * `PosseBar` / `PosseItemCreature`, `Rollover_Creature` and `SocialBar`:
//!   the pack list, the name/health card of the creature under the cursor and
//!   the friendship meter ([`world`]).
//! * the retail callout dialogs and hints (`Callout_CRG_*`, `crg-*`), through
//!   the generic machinery of `cell_hud::dialogs` ([`guide`]).
//!
//! The layouts carry all geometry and art. This module only decides which
//! windows are shown, fills their text and meters, and turns button clicks
//! into the keys the Creature controls already use (stance buttons are `E` /
//! `F`, ability buttons `1`..`4`), so the Creature gameplay modules are not
//! touched: [`publish_state`] fills the [`CreatureHudState`] contract from
//! their resources and components each frame. The only write into Creature
//! state is the retail default stance when the HUD first appears in a new
//! lineage (see `publish_state`).
//!
//! Control ids and roles come from the layouts and their tooltips (see
//! `notes\creature-hud-notes.md`). Where the retail code's behaviour is not
//! documented the binding is marked `ADAPTED`.

use bevy::prelude::*;

pub mod cursor;
pub mod guide;
pub mod portrait;
pub mod rainbow;
pub mod recharge;
pub mod timeline;
pub mod world;

use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::ProgressColors,
    creature_actions::{self, Encounter, EncounterMode},
    game::{Health, Hunger, Player, RunProgress},
    spui::{
        model::{ImageRef, WinId},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const MAIN_LAYOUT: &str = "GlobalUICreature3-7";
pub const CARD_LAYOUT: &str = "CRGMissionCard";
pub const OPTIONS_LAYOUT: &str = crate::cell_hud::OPTIONS_LAYOUT;

/// Control ids (roles from the layouts and their tooltips). The list is the
/// reference of the layout; some ids are only checked by the tests.
#[allow(dead_code)]
pub mod id {
    // GlobalUICreature3-7: bottom bar.
    pub const DNA_TEXT: u32 = 0xABCD_1001;
    pub const BAR: u32 = 0x051B_5478;
    pub const MARKER: u32 = 0x051B_5168;
    /// The four progress segments, one per brain level.
    pub const SEGMENTS: [u32; 4] = [0x051B_54C0, 0x051B_54C1, 0x051B_54C2, 0x051B_54C3];
    /// Two-frame brain icon at the end of each segment (dark, lit).
    pub const BRAIN_ICONS: [u32; 4] = [0x0633_0588, 0x0633_0700, 0x0633_0750, 0x0633_0798];
    /// Advance to Tribe (hidden until the bar is full), its glow and the
    /// History button it replaces.
    pub const ADVANCE: u32 = 0x0645_5B55;
    pub const ADVANCE_GLOW: u32 = 0x0652_A628;
    pub const HISTORY: u32 = 0x0448_27B8;
    pub const PAUSE_TOGGLE: u32 = 0xD305_CA84;
    pub const DNA_PANEL: u32 = 0x051B_6DB0;
    pub const SPOREPEDIA: u32 = 0x0624_4660;
    pub const COLLECTIONS: u32 = 0x0624_4208;
    // Stance buttons (toggles of group 1) and the two ability trays.
    pub const STANCE_SOCIAL: u32 = 0x04C6_1B50;
    pub const STANCE_COMBAT: u32 = 0x04C6_1DD9;
    pub const SOCIAL_TRAY: u32 = 0x053E_FDC8;
    pub const COMBAT_TRAY: u32 = 0x053E_FDC0;
    /// The four ability buttons of each tray, in key order (1..4).
    pub const SOCIAL_VERBS: [u32; 4] = [0x0101_2010, 0x0101_2011, 0x0101_2012, 0x0101_2013];
    pub const COMBAT_VERBS: [u32; 4] = [0x0101_2020, 0x0101_2021, 0x0101_2022, 0x0101_2023];
    /// The row of five round general ability slots under the stance buttons
    /// (`CRG_AbilitiesButton`, retail row ids `0x01012030..34`) and the
    /// group that holds them.
    pub const GENERAL: [u32; 5] = [0x0101_2030, 0x0101_2031, 0x0101_2032, 0x0101_2033, 0x0101_2034];
    pub const GENERAL_GROUP: u32 = 0x04C5_ADE3;
    /// Superpower slot (toggle + locked variant) and the round button above
    /// the stance buttons.
    pub const SUPERPOWER: u32 = 0x0101_2040;
    pub const SUPERPOWER_LOCKED: u32 = 0x0636_EE00;
    pub const ROUND_BUTTON: u32 = 0x05D3_BBE3;
    // Health and hunger meters (C5A of the RE notes).
    pub const HEALTH_PANEL: u32 = 0x053F_45C0;
    /// Warning windows: enabled while health / hunger is below the motive
    /// threshold.
    pub const HEALTH_ROW: u32 = 0x0685_A85E;
    pub const HUNGER_ROW: u32 = 0x0685_A85F;
    pub const HEALTH_METER: u32 = 0x018A_C46E;
    pub const HUNGER_METER: u32 = 0x018A_C464;
    pub const HEALTH_TEXT: u32 = 0x04D0_39A8;
    /// Three-frame diet icon beside the hunger meter.
    pub const DIET_ICON: u32 = 0x0578_F7F8;
    pub const PORTRAIT: u32 = 0x03D9_9151;
    // The planet map panel (the picture is `land_minimap`'s).
    pub const MAP_TOGGLE: u32 = 0x0562_A0F7;
    pub const MAP_HOLDER: u32 = 0x0190_722E;
}

/// Width of a health/hunger meter window at full value (authored area).
const METER_FULL: f32 = 60.0;
/// Health and hunger fraction under which the warning windows are enabled
/// (retail `MotiveLow` 30, read from the global the HUD update compares with).
pub const MOTIVE_LOW: f32 = 0.30;
/// ADAPTED key of the Advance button (the Creature stage has no advance
/// action yet; the Cell stage uses F4 for it).
pub const ADVANCE_KEY: KeyCode = KeyCode::F4;
/// The DNA caption is the number preceded by the DNA symbol glyph of
/// `SporeCurrencyEA` (U+268B, as in the Cell HUD; the layout's own symbol
/// picture beside the caption is hidden).
const DNA_GLYPH: char = '\u{268B}';
/// ADAPTED: em size of the DNA caption (as the Cell HUD: the style says
/// 14 pt but the 13 px high caption window shows ~16 px digits).
const DNA_TEXT_PX: f32 = 16.0;

/// Which layout a spawned root is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CreatureHudRoot {
    Main,
    Orb,
    /// Mission card number `n` (top left, stacked).
    Card(u8),
    /// The pack bar and pack item `n`.
    Posse,
    PosseItem(u8),
    Rollover,
    Social,
    /// Hint bar and callout dialog.
    Hint,
    Callout,
}

/// True while the SPUI Creature HUD is shown; the legacy Creature HUD and
/// the pause panel step aside.
#[derive(Resource, Default)]
pub struct CreatureHudActive(pub bool);

/// Social or combat stance (the green and the red button).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stance {
    #[default]
    Social,
    Combat,
}

/// One ability button of a tray.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VerbSlot {
    /// Rank 0..=5 of the ability (0 = the creature has no such part).
    pub rank: u8,
    /// Seconds left before the ability can be used again.
    pub cooldown: f32,
}

/// The neutral abilities of the row of round slots under the stance buttons
/// (retail category 2: call, jump, glide, sprint, stealth, ...). Only the ones
/// the rewrite has are listed; keys are the existing Creature controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum General {
    /// The mate call (M at the nest).
    Call,
    Jump,
    Glide,
    Sprint,
    Sneak,
}

/// Fill order of the five round slots.
pub const GENERAL_ORDER: [General; 5] = [General::Call, General::Jump, General::Glide, General::Sprint, General::Sneak];

impl General {
    /// The key the existing controls use for the ability.
    pub fn key(self) -> KeyCode {
        match self {
            General::Call => KeyCode::KeyM,
            General::Jump | General::Glide => KeyCode::Space,
            General::Sprint => KeyCode::ShiftLeft,
            General::Sneak => KeyCode::KeyZ,
        }
    }

    /// The ability's name and the key label of its tooltip.
    pub fn name(self) -> &'static str {
        match self {
            General::Call => "Call",
            General::Jump => "Jump",
            General::Glide => "Glide",
            General::Sprint => "Sprint",
            General::Sneak => "Sneak",
        }
    }

    pub fn key_label(self) -> &'static str {
        match self {
            General::Call => "M",
            General::Jump | General::Glide => "Space",
            General::Sprint => "Shift",
            General::Sneak => "Z",
        }
    }

    /// The key stays down while the button does (sprint, glide).
    pub fn holds(self) -> bool {
        matches!(self, General::Sprint | General::Glide)
    }

    /// The white glyph drawn on the round slot (`604B35E7` `mv_*`).
    pub fn icon(self) -> crate::dbpf::ResourceKey {
        let instance_id = match self {
            General::Call => 0x6D16_962B,
            General::Jump => 0x71DF_4AD7,
            General::Glide => 0x3ED4_2F2E,
            General::Sprint => 0x84C3_F9CF,
            General::Sneak => 0x75F4_9945,
        };
        crate::dbpf::ResourceKey {
            type_id: crate::ui_art::UI_PNG_TYPE,
            group_id: crate::ui_art::CREATURE_ACTION_ICON_GROUP,
            instance_id,
        }
    }
}

/// One round slot of the general ability row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneralSlot {
    pub ability: General,
    /// The ability is switched on (sprinting, sneaking, gliding).
    pub active: bool,
}

/// Diet class of the player's creature (`Eat` goal text, hints).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Diet {
    #[default]
    Herbivore,
    Carnivore,
    Omnivore,
}

/// Everything the Creature HUD displays, filled by [`publish_state`] (and by
/// [`world`] / [`guide`] for their parts) so the HUD never reads simulation
/// internals itself.
#[derive(Resource, Clone, Debug, Default)]
pub struct CreatureHudState {
    /// Spendable DNA ("evolution points") beside the progress bar.
    pub dna_points: i32,
    /// DNA earned in this stage and the amount that ends it.
    pub stage_dna: f32,
    pub goal_dna: f32,
    /// Brain level reached (0..=4: the lit brain icons).
    pub brain_level: u8,
    /// Fill of the four progress segments, `0.0..=1.0`.
    pub level_fill: [f32; 4],
    /// The progress bar is full: Advance replaces History.
    pub advance_visible: bool,
    pub health: f32,
    pub max_health: f32,
    pub hunger: f32,
    pub max_hunger: f32,
    pub stance: Stance,
    pub social: [VerbSlot; 4],
    pub combat: [VerbSlot; 4],
    /// The general abilities the creature has, in slot order (the first
    /// ability sits in the centre slot).
    pub general: Vec<GeneralSlot>,
    pub diet: Diet,
    /// The options menu is open (the game is paused).
    pub paused: bool,
    /// Goal cards in display order (retail strings, see [`guide::goal_cards`]).
    pub goal_cards: Vec<CellGoalCard>,
    /// Callouts to show, front first (`Callout_CRG_Opening`, ...).
    pub callouts: Vec<String>,
    /// Active hint: a retail hint id (`crg-lowhealth`) or `msg:<text>`.
    pub hint: Option<String>,
    /// Pack members (name, health fraction) and the capacity of the pack.
    pub pack: Vec<world::PackMember>,
    pub pack_capacity: usize,
    /// Creature under the cursor.
    pub rollover: Option<world::Rollover>,
    /// Friendship meter of the creature being impressed.
    pub social_meter: Option<world::SocialMeter>,
    /// The stage timeline (the summary window at the end of the stage): the
    /// HUD hides behind it.
    pub summary: Option<crate::creature_flow::CreatureSummary>,
    /// Cinematic letterbox bars (a moment's text window is up).
    pub letterbox: bool,
}

impl CreatureHudState {
    /// Fraction of a meter, `0.0..=1.0`.
    pub fn health_fraction(&self) -> f32 {
        fraction(self.health, self.max_health)
    }

    pub fn hunger_fraction(&self) -> f32 {
        fraction(self.hunger, self.max_hunger)
    }

    /// The abilities of the tray of the current stance.
    #[allow(dead_code)]
    pub fn verbs(&self) -> &[VerbSlot; 4] {
        match self.stance {
            Stance::Social => &self.social,
            Stance::Combat => &self.combat,
        }
    }
}

fn fraction(value: f32, max: f32) -> f32 {
    if max > 0.0 && value.is_finite() {
        (value / max).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Stage total DNA at which each brain level is reached (the segment ends of
/// the progress bar). The one place the HUD reads the game's brain rule. In
/// this tree `creature_progression::BRAIN_LEVEL_DNA` holds those cut-offs
/// (100/200/300/400); retail has them cumulative, 100/300/600/1000 of the
/// stage total (RE C1A), which the segment widths of the layout
/// (95:188:284:377 px) confirm. MERGE with `creature-rules`: there
/// `BRAIN_LEVEL_DNA` holds the per-level costs, so this body becomes
/// `crate::creature_progression::BRAIN_LEVEL_TOTALS` (nothing else changes).
pub fn brain_thresholds() -> [f32; 4] {
    crate::creature_progression::BRAIN_LEVEL_TOTALS
}

/// Frame of the three-frame diet icon: both foods (omnivore), meat (carnivore)
/// or fruit (herbivore), in the order of the installed art.
pub fn diet_frame(diet: Diet) -> u32 {
    match diet {
        Diet::Omnivore => 0,
        Diet::Carnivore => 1,
        Diet::Herbivore => 2,
    }
}

/// The stance the HUD starts in: Social when the species has a herbivorous
/// mouth, else Attack (retail HUD init `0xD2C000`, RE C5B).
pub fn default_stance(abilities: &crate::asset_catalog::Capabilities) -> Stance {
    if abilities.herbivore > 0 {
        Stance::Social
    } else {
        Stance::Combat
    }
}

/// Fill of the four progress segments for `dna` earned in the stage. The
/// segments are the four brain levels: segment `i` fills while the DNA runs
/// from the previous brain threshold to the next one
/// (`creature_progression::BRAIN_LEVEL_DNA`, the game's own rank cutoffs).
pub fn level_fills(dna: f32, thresholds: &[f32; 4]) -> [f32; 4] {
    let mut fills = [0.0; 4];
    let mut low = 0.0;
    for (fill, &high) in fills.iter_mut().zip(thresholds) {
        let span = (high - low).max(f32::EPSILON);
        *fill = if dna.is_finite() { ((dna - low) / span).clamp(0.0, 1.0) } else { 0.0 };
        low = high;
    }
    fills
}

/// Where the progress marker sits: the screen x of the first segment that is
/// not full (or the end of the last one), given the segments' screen
/// intervals `(x1, x2)`.
pub fn marker_x(segments: &[(f32, f32)], fills: &[f32; 4]) -> Option<f32> {
    let mut x = segments.first()?.0;
    for (&(x1, x2), &fill) in segments.iter().zip(fills) {
        x = x1 + fill.clamp(0.0, 1.0) * (x2 - x1);
        if fill < 1.0 {
            break;
        }
    }
    Some(x)
}

/// Diet class from the ranks of the mounted mouth parts.
pub fn diet_of(abilities: &crate::asset_catalog::Capabilities) -> Diet {
    match (abilities.herbivore > 0, abilities.carnivore > 0) {
        (true, true) => Diet::Omnivore,
        (false, true) => Diet::Carnivore,
        _ => Diet::Herbivore,
    }
}

/// The ability names of a tray in key order (retail verbs).
pub fn tray_names(stance: Stance) -> &'static [&'static str] {
    match stance {
        Stance::Social => creature_actions::SOCIAL,
        Stance::Combat => creature_actions::COMBAT,
    }
}

/// The installed PNG of an ability icon (`creature_hud` group `604B35E7`).
pub fn verb_icon(stance: Stance, slot: usize) -> Option<crate::dbpf::ResourceKey> {
    let name = tray_names(stance).get(slot)?;
    crate::ui_art::ACTION_ICON_TEXTURES.iter().find(|icon| icon.action.eq_ignore_ascii_case(name)).map(|icon| icon.key)
}

pub struct CreatureHudPlugin;

impl Plugin for CreatureHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(cursor::CreatureCursorPlugin)
            .init_resource::<CreatureHudState>()
            .init_resource::<CreatureHudActive>()
            .init_resource::<guide::GuideState>()
            .init_resource::<guide::GuideCache>()
            .init_resource::<portrait::PortraitState>()
            .init_resource::<timeline::CrgTimelineCache>()
            .add_systems(
                Update,
                (
                    publish_state,
                    world::publish_world,
                    guide::publish_guide,
                    demo_state,
                    manage,
                    world::manage_dynamic,
                    bind,
                    world::bind_world,
                    guide::bind_guide,
                    timeline::manage,
                    timeline::bind,
                    timeline::overlays,
                )
                    .chain()
                    .before(view::sync)
                    .after(view::input),
            )
            // The portrait camera follows the creature; its picture sits over the
            // portrait window once the HUD has been laid out.
            .add_systems(
                Update,
                (portrait::update, portrait::place, timeline::thumbnail).chain().after(view::sync).after(manage),
            )
            // The friendship meter's arc is painted from the solved layout.
            .init_resource::<rainbow::RainbowArc>()
            .add_systems(Update, rainbow::draw.after(view::sync).after(world::bind_world))
            // The verb tray's recharge wipes sit over the buttons' icons.
            .init_resource::<recharge::RechargeOverlays>()
            .add_systems(Update, recharge::draw.after(view::sync).after(bind))
            // Clicks recorded by the SPUI input of the last frame become key
            // presses before the controls (and then the simulation) read the
            // keyboard: ahead of the first system of the controls chain.
            .add_systems(
                Update,
                actions
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Creature)),
            );
    }
}

/// Flags of the capture demo.
struct DemoFlags {
    on: bool,
    paused: bool,
    combat: bool,
}

impl DemoFlags {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        Self {
            on: has("--creature-hud-demo"),
            paused: has("--creature-hud-paused"),
            combat: has("--creature-hud-combat"),
        }
    }
}

/// `--creature-hud-demo` (captures): replace what the game published with a
/// mid-game HUD state so that every part of the layouts can be looked at:
/// partly filled meters and progress bar, abilities with ranks and a cooldown,
/// two pack members, a creature card and the friendship meter.
/// `--creature-hud-paused` also opens the options menu and
/// `--creature-hud-combat` switches to the combat stance.
fn demo_state(stage: Res<State<GameStage>>, mut demo: Local<Option<DemoFlags>>, mut hud: ResMut<CreatureHudState>) {
    let flags = demo.get_or_insert_with(DemoFlags::from_args);
    if !flags.on || *stage.get() != GameStage::Creature {
        return;
    }
    hud.dna_points = 118;
    hud.stage_dna = 140.0;
    hud.goal_dna = brain_thresholds()[3];
    hud.brain_level = 1;
    hud.level_fill = level_fills(140.0, &brain_thresholds());
    hud.diet = Diet::Omnivore;
    hud.general = vec![
        GeneralSlot { ability: General::Call, active: false },
        GeneralSlot { ability: General::Jump, active: false },
        GeneralSlot { ability: General::Glide, active: false },
        GeneralSlot { ability: General::Sprint, active: true },
        GeneralSlot { ability: General::Sneak, active: false },
    ];
    hud.health = 70.0;
    hud.max_health = 100.0;
    hud.hunger = 55.0;
    hud.max_hunger = 100.0;
    hud.stance = if flags.combat { Stance::Combat } else { Stance::Social };
    let slot = |rank, cooldown| VerbSlot { rank, cooldown };
    hud.social = [slot(2, 0.0), slot(1, 2.5), slot(3, 0.0), slot(0, 0.0)];
    hud.combat = [slot(3, 0.0), slot(0, 0.0), slot(1, 1.5), slot(2, 0.0)];
    hud.paused = flags.paused;
    hud.pack = vec![
        world::PackMember { id: 1, name: "Noogie".into(), health: 0.8 },
        world::PackMember { id: 2, name: "Spiker".into(), health: 0.35 },
    ];
    hud.pack_capacity = 3;
    hud.rollover = Some(world::Rollover {
        name: "Grypher".into(),
        health: 0.6,
        hp: 60,
        relation: Some(world::Relation::Friendly),
        screen: Vec2::new(640.0, 520.0),
    });
    hud.social_meter = Some(world::SocialMeter {
        friendship: 0.6,
        relation: Some(world::Relation::Curious),
        requested: Some(1),
        time_left: 0.7,
        creature: 0.35,
        player: 0.25,
        rounds: 3,
        species_relation: crate::creature_rules::relationship::CURIOUS,
    });
}

/// Fill [`CreatureHudState`] from the Creature resources and components.
#[allow(clippy::too_many_arguments)]
fn publish_state(
    stage: Res<State<GameStage>>,
    active: Res<CreatureHudActive>,
    run: Res<RunProgress>,
    mut encounter: ResMut<Encounter>,
    originals: Res<crate::Originals>,
    pause: Res<crate::pause_menu::PauseMenu>,
    mobility: Option<Res<crate::creature::mobility::CreatureMobility>>,
    flow: Res<crate::creature_flow::CreatureFlow>,
    player: Query<(&Health, Option<&Hunger>), With<Player>>,
    mut hud: ResMut<CreatureHudState>,
    mut stance_set: Local<bool>,
) {
    if *stage.get() != GameStage::Creature {
        *stance_set = false;
        return;
    }
    let thresholds = brain_thresholds();
    hud.dna_points = run.editor_dna.max(0.0).floor() as i32;
    hud.stage_dna = run.dna_this_stage;
    hud.goal_dna = thresholds[3];
    hud.brain_level = run.creature_progression.brain_level;
    hud.level_fill = level_fills(run.dna_this_stage, &thresholds);
    // The last brain level ends the stage through the Advance button.
    hud.advance_visible = hud.brain_level >= crate::creature_progression::MAX_BRAIN_LEVEL;
    if let Ok((health, hunger)) = player.single() {
        hud.health = health.hp;
        hud.max_health = health.max;
        let (food, max) = hunger.map_or((1.0, 1.0), |h| (h.food, h.max));
        hud.hunger = food;
        hud.max_hunger = max;
    }
    let abilities = creature_actions::player_abilities(&run, &originals);
    // ADAPTED: the retail HUD init picks the stance from the diet. It is a
    // stance change only (no target, nothing engaged), made once when the HUD
    // appears in a fresh lineage so that a restored stance is kept.
    if active.0 && !*stance_set {
        *stance_set = true;
        let want = match default_stance(&abilities) {
            Stance::Social => EncounterMode::Social,
            Stance::Combat => EncounterMode::Combat,
        };
        if run.dna_this_stage <= 0.0 && encounter.target.is_none() && encounter.mode != want {
            encounter.mode = want;
        }
    }
    hud.stance = match encounter.mode {
        EncounterMode::Social => Stance::Social,
        EncounterMode::Combat => Stance::Combat,
    };
    let slots = |mode: EncounterMode| -> [VerbSlot; 4] {
        let ranks = creature_actions::levels(abilities, mode);
        // The cooldown timers belong to the tray of the current stance.
        let current = mode == encounter.mode;
        std::array::from_fn(|i| VerbSlot {
            rank: ranks[i],
            cooldown: if current { encounter.cooldowns[i] } else { 0.0 },
        })
    };
    hud.social = slots(EncounterMode::Social);
    hud.combat = slots(EncounterMode::Combat);
    // The general abilities the creature has fill the round slots in order.
    let moving = mobility.as_deref();
    hud.general = GENERAL_ORDER
        .iter()
        .filter(|ability| match ability {
            General::Call => true,
            General::Jump => abilities.jump > 0,
            General::Glide => abilities.glide > 0,
            General::Sprint => abilities.sprint > 0,
            General::Sneak => abilities.sneak > 0,
        })
        .map(|&ability| GeneralSlot {
            ability,
            active: moving.is_some_and(|m| match ability {
                General::Glide => m.gliding,
                General::Sprint => m.sprinting,
                General::Sneak => m.sneaking,
                General::Call | General::Jump => false,
            }),
        })
        .collect();
    hud.diet = diet_of(&abilities);
    hud.paused = pause.open;
    hud.summary = flow.summary.clone();
    hud.letterbox = flow.letterbox;
}

/// Spawn or despawn the static HUD layouts depending on the stage.
#[allow(clippy::too_many_arguments)]
fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    options: Res<crate::HudOptions>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut active: ResMut<CreatureHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CreatureHudRoot)>,
) {
    let want = *stage.get() == GameStage::Creature && !editor.open && !options.diagnostics;
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        active.0 = false;
        return;
    }
    if roots.iter().any(|(_, r)| *r == CreatureHudRoot::Main) {
        return;
    }
    if originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let Some(main) = assets.layout(index, MAIN_LAYOUT) else {
        return;
    };
    let spawn = |commands: &mut Commands, assets: &mut SpuiAssets, name: &str, kind, policy, z| {
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        }
    };
    // Rollover first so that the bottom bar stays above it.
    spawn(&mut commands, &mut assets, world::ROLLOVER_LAYOUT, CreatureHudRoot::Rollover, RootPolicy::Authored, 98);
    // The friendship meter is a HUD layout like the main one: its root follows
    // the screen and its group sits at the middle (proportional layout).
    spawn(&mut commands, &mut assets, world::SOCIAL_LAYOUT, CreatureHudRoot::Social, RootPolicy::Fit, 98);
    spawn(&mut commands, &mut assets, world::POSSE_LAYOUT, CreatureHudRoot::Posse, RootPolicy::Authored, 99);
    let root = view::spawn(&mut commands, main, RootPolicy::Fit, Some(camera), 100);
    commands.entity(root).insert(CreatureHudRoot::Main);
    spawn(&mut commands, &mut assets, OPTIONS_LAYOUT, CreatureHudRoot::Orb, RootPolicy::Fit, 101);
    // The hint bar and the callout dialog stay spawned (hidden) so that their
    // pop-in effects run when they are shown.
    spawn(
        &mut commands,
        &mut assets,
        crate::cell_hud::dialogs::HINT_LAYOUT,
        CreatureHudRoot::Hint,
        RootPolicy::Fit,
        102,
    );
    spawn(
        &mut commands,
        &mut assets,
        crate::cell_hud::dialogs::CALLOUT_LAYOUT,
        CreatureHudRoot::Callout,
        RootPolicy::Authored,
        103,
    );
    active.0 = true;
}

fn window(state: &SpuiState, control: u32) -> Option<WinId> {
    state.find(control)
}

fn set_visible(ui: &mut SpuiState, control: u32, on: bool) {
    if let Some(w) = window(ui, control) {
        ui.set_visible(w, on);
    }
}

/// Tooltip of an ability button (the tooltip builder 0xD4A5A0, VERIFIED): in the
/// Creature stage an ability the creature has (level above 0) uses the string
/// 0681D203 `~local_ability_name~ - ~local_level_string~ ~local_ability_level~`
/// ("Bite - Level 2", the word is `CRG_AbilityUI` 04ACE6B0), one without a level
/// the default 0681D202 `~local_ability_name~ [~local_hotkey~]`.
pub fn verb_tooltip(name: &str, slot: usize, rank: u8) -> String {
    if rank > 0 {
        format!("{name} - {LEVEL_WORD} {rank}")
    } else {
        format!("{name} [{}]", slot + 1)
    }
}

/// `CRG_AbilityUI` 04ACE6B0 (checked against the installed table by a test).
const LEVEL_WORD: &str = "Level";

/// Fill one tray: the abilities the creature has show their installed icon
/// (a complete green or red button picture) in place of the button's empty
/// slot art, the other slots stay empty and disabled. An ability that is
/// cooling down keeps its icon undimmed: the retail tray draws a recharge wipe
/// over it (0xD4A560 -> 0xD49F30), see [`recharge`].
fn bind_tray(
    ui: &mut SpuiState,
    controls: &[u32; 4],
    stance: Stance,
    slots: &[VerbSlot; 4],
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    for (slot, &control) in controls.iter().enumerate() {
        let Some(button) = window(ui, control) else {
            continue;
        };
        let VerbSlot { rank, .. } = slots[slot];
        let icon = (rank > 0).then(|| verb_icon(stance, slot)).flatten().and_then(|key| {
            assets.texture(index, images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None })
        });
        ui.set_image(button, icon);
        ui.set_tint(button, 0xFFFF_FFFF);
        ui.set_enabled(button, rank > 0);
        ui.set_tooltip(button, (rank > 0).then(|| verb_tooltip(tray_names(stance)[slot], slot, rank)));
    }
}

/// Size a meter window to `fraction` of its authored width (the children are
/// clipped by it).
fn set_meter(ui: &mut SpuiState, control: u32, fraction: f32) {
    if let Some(w) = window(ui, control) {
        let mut area = ui.layout.window(w).area;
        area[2] = area[0] + METER_FULL * fraction.clamp(0.0, 1.0);
        ui.set_area(w, area);
    }
}

/// Fill the row of round general ability slots: the abilities the creature
/// has show their white glyph in the button's icon slot (over the round blue
/// background), the other slots are hidden, switched on abilities look
/// selected.
fn bind_general(
    ui: &mut SpuiState,
    slots: &[GeneralSlot],
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    for (i, &control) in id::GENERAL.iter().enumerate() {
        let Some(button) = window(ui, control) else {
            continue;
        };
        let slot = slots.get(i);
        ui.set_visible(button, slot.is_some());
        let Some(slot) = slot else {
            continue;
        };
        let key = slot.ability.icon();
        let icon =
            assets.texture(index, images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None });
        ui.set_icon(button, icon);
        ui.set_selected(button, slot.active);
        ui.set_tooltip(button, Some(format!("{} [{}]", slot.ability.name(), slot.ability.key_label())));
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_main(
    ui: &mut SpuiState,
    hud: &CreatureHudState,
    colors: &ProgressColors,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    if let Some(w) = window(ui, id::DNA_TEXT) {
        ui.set_text(w, format!("{DNA_GLYPH}{}", hud.dna_points));
        ui.set_text_size(w, Some(DNA_TEXT_PX));
    }
    if let Some(w) = window(ui, id::PAUSE_TOGGLE) {
        ui.set_selected(w, hud.paused);
    }
    // Advance replaces History once the progress bar is full.
    for control in [id::ADVANCE, id::ADVANCE_GLOW] {
        set_visible(ui, control, hud.advance_visible);
    }
    set_visible(ui, id::HISTORY, !hud.advance_visible);
    // Progress: four segments, one per brain level, plus the marker.
    for (i, control) in id::SEGMENTS.iter().enumerate() {
        let Some(w) = window(ui, *control) else {
            continue;
        };
        let fill = hud.level_fill[i];
        ui.set_shade(w, colors.right);
        ui.set_overlay(w, (fill > 0.0).then_some((fill, colors.left)));
    }
    for (i, control) in id::BRAIN_ICONS.iter().enumerate() {
        if let Some(w) = window(ui, *control) {
            ui.set_frame(w, Some(u32::from(hud.brain_level as usize > i)));
        }
    }
    let spans: Vec<(f32, f32)> =
        id::SEGMENTS.iter().filter_map(|c| window(ui, *c)).map(|w| (ui.rect(w).x1, ui.rect(w).x2)).collect();
    if let (Some(x), Some(bar), Some(marker)) =
        (marker_x(&spans, &hud.level_fill), window(ui, id::BAR), window(ui, id::MARKER))
    {
        let x = x - ui.rect(bar).x1;
        let mut area = ui.layout.window(marker).area;
        area[0] = x;
        area[2] = x;
        ui.set_area(marker, area);
    }
    // Stance buttons and trays.
    let social = hud.stance == Stance::Social;
    if let Some(w) = window(ui, id::STANCE_SOCIAL) {
        ui.set_selected(w, social);
    }
    if let Some(w) = window(ui, id::STANCE_COMBAT) {
        ui.set_selected(w, !social);
    }
    set_visible(ui, id::SOCIAL_TRAY, social);
    set_visible(ui, id::COMBAT_TRAY, !social);
    bind_tray(ui, &id::SOCIAL_VERBS, Stance::Social, &hud.social, assets, images, index);
    bind_tray(ui, &id::COMBAT_VERBS, Stance::Combat, &hud.combat, assets, images, index);
    // Health and hunger meters.
    set_meter(ui, id::HEALTH_METER, hud.health_fraction());
    set_meter(ui, id::HUNGER_METER, hud.hunger_fraction());
    if let Some(w) = window(ui, id::HEALTH_TEXT) {
        ui.set_text(w, (hud.health.max(0.0).ceil() as i32).to_string());
    }
    if let Some(w) = window(ui, id::DIET_ICON) {
        ui.set_frame(w, Some(diet_frame(hud.diet)));
    }
    // The warning windows are enabled while a motive is below the threshold.
    for (control, fraction) in [(id::HEALTH_ROW, hud.health_fraction()), (id::HUNGER_ROW, hud.hunger_fraction())] {
        if let Some(w) = window(ui, control) {
            ui.set_enabled(w, fraction < MOTIVE_LOW);
        }
    }
    bind_general(ui, &hud.general, assets, images, index);
    // Parts of the retail layout that stay hidden: the round button above the
    // stance buttons is named by no code of the image (never shown in retail);
    // the superpower slot (toggle 01012040, locked variant 0636EE00) is handled
    // by the verb tray code (0xD48C1E, 0xD48D63) but ADAPTED: the rewrite has
    // no superpower to show (the unlock state is not decoded).
    for control in [id::SUPERPOWER, id::SUPERPOWER_LOCKED, id::ROUND_BUTTON] {
        set_visible(ui, control, false);
    }
    // The planet map panel (frame, camera buttons, Hide toggle, holder); its
    // picture, name and Hide toggle are `land_minimap`'s.
    set_visible(ui, id::MAP_TOGGLE, true);
}

/// Push [`CreatureHudState`] into the layouts.
#[allow(clippy::too_many_arguments)]
fn bind(
    hud: Res<CreatureHudState>,
    originals: Res<crate::Originals>,
    colors: Res<ProgressColors>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<(&mut SpuiState, &CreatureHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            CreatureHudRoot::Main => bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, index),
            // The orb and its menu are the Cell stage's (the menu is shown
            // while the game is paused through it).
            CreatureHudRoot::Orb => crate::cell_hud::bind_orb(
                &mut ui,
                &crate::cell_contract::CellHudState { paused: hud.paused, ..default() },
            ),
            _ => {}
        }
    }
}

/// Button clicks become the keyboard actions the Creature controls already
/// use. Runs before the simulation (`Controls`) on the clicks the SPUI input
/// recorded in the previous frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn actions(
    hud: Res<CreatureHudState>,
    mut roots: Query<(&mut SpuiState, &CreatureHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    mut front_menu: MessageWriter<crate::front_end::MenuCommand>,
    mut ui_sounds: MessageWriter<crate::creature_flow::UiSound>,
) {
    use crate::cell_hud::id as menu;
    // A key the HUD presses stays down for the frame; `player_hud::release`
    // lets it go at the end of it.
    let mut press = |keys: &mut ButtonInput<KeyCode>, key: KeyCode| {
        if !keys.pressed(key) {
            keys.press(key);
            injected.push(key);
        }
    };
    for (mut ui, kind) in &mut roots {
        if !matches!(kind, CreatureHudRoot::Main | CreatureHudRoot::Orb) {
            continue;
        }
        // Sprint and glide are held: their key stays down while the button
        // is pressed.
        if *kind == CreatureHudRoot::Main {
            for (slot, control) in id::GENERAL.iter().enumerate() {
                let Some(general) = hud.general.get(slot).filter(|g| g.ability.holds()) else {
                    continue;
                };
                let Some(w) = ui.find(*control) else {
                    continue;
                };
                if ui.states[w] & crate::spui::ids::state::CLICKED != 0 {
                    press(&mut keys, general.ability.key());
                }
            }
        }
        for click in ui.take_clicks() {
            let control = ui.layout.window(click).control_id;
            // The options menu and Sporepedia are the front end's.
            if let Some(command) = crate::front_end::menu_command(control) {
                front_menu.write(command);
                continue;
            }
            match control {
                id::STANCE_SOCIAL => press(&mut keys, crate::creature_controls::KEY_STANCE_SOCIAL),
                id::STANCE_COMBAT => press(&mut keys, crate::creature_controls::KEY_STANCE_COMBAT),
                id::ADVANCE => press(&mut keys, ADVANCE_KEY),
                // The pause menu is opened directly: Esc would first untarget a standing target.
                id::PAUSE_TOGGLE | menu::OPTIONS_ORB => pause.open = !pause.open,
                c => {
                    if let Some(slot) = id::SOCIAL_VERBS.iter().position(|v| *v == c) {
                        press(&mut keys, VERB_KEYS[slot]);
                        // Retail 0xD49671: the verb tray click sound.
                        ui_sounds.write(crate::creature_flow::UiSound::VerbClick);
                    } else if let Some(slot) = id::COMBAT_VERBS.iter().position(|v| *v == c) {
                        press(&mut keys, VERB_KEYS[slot]);
                        ui_sounds.write(crate::creature_flow::UiSound::VerbClick);
                    } else if let Some(general) = id::GENERAL
                        .iter()
                        .position(|v| *v == c)
                        .and_then(|slot| hud.general.get(slot))
                        .filter(|g| !g.ability.holds())
                    {
                        press(&mut keys, general.ability.key());
                        ui_sounds.write(crate::creature_flow::UiSound::VerbClick);
                    }
                    // Sporepedia, My Collections and History have no
                    // counterpart in the rewrite yet.
                }
            }
        }
    }
}

/// Ability buttons press the digit keys the tray labels show.
pub const VERB_KEYS: [KeyCode; 4] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLDS: [f32; 4] = [100.0, 200.0, 300.0, 400.0];

    #[test]
    fn segments_fill_one_after_the_other() {
        assert_eq!(level_fills(0.0, &THRESHOLDS), [0.0; 4]);
        assert_eq!(level_fills(50.0, &THRESHOLDS), [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(level_fills(150.0, &THRESHOLDS), [1.0, 0.5, 0.0, 0.0]);
        assert_eq!(level_fills(400.0, &THRESHOLDS), [1.0; 4]);
        assert_eq!(level_fills(9999.0, &THRESHOLDS), [1.0; 4]);
        assert_eq!(level_fills(f32::NAN, &THRESHOLDS), [0.0; 4]);
        // Uneven thresholds: each segment spans its own DNA range.
        let uneven = [100.0, 300.0, 600.0, 1000.0];
        assert_eq!(level_fills(200.0, &uneven), [1.0, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn marker_follows_the_first_unfilled_segment() {
        let spans = [(100.0, 200.0), (210.0, 410.0), (420.0, 720.0), (730.0, 1130.0)];
        assert_eq!(marker_x(&spans, &[0.0; 4]), Some(100.0));
        assert_eq!(marker_x(&spans, &[0.5, 0.0, 0.0, 0.0]), Some(150.0));
        assert_eq!(marker_x(&spans, &[1.0, 0.5, 0.0, 0.0]), Some(310.0));
        assert_eq!(marker_x(&spans, &[1.0; 4]), Some(1130.0));
        assert_eq!(marker_x(&[], &[0.0; 4]), None);
    }

    #[test]
    fn meters_clamp_and_tolerate_bad_values() {
        assert_eq!(fraction(5.0, 10.0), 0.5);
        assert_eq!(fraction(20.0, 10.0), 1.0);
        assert_eq!(fraction(-1.0, 10.0), 0.0);
        // No maximum yet (before the first publish): shown full.
        assert_eq!(fraction(0.0, 0.0), 1.0);
        assert_eq!(fraction(f32::NAN, 10.0), 1.0);
    }

    #[test]
    fn diet_follows_the_mounted_mouths() {
        use crate::asset_catalog::Capabilities;
        let herbivore = Capabilities { herbivore: 1, ..default() };
        let carnivore = Capabilities { carnivore: 2, ..default() };
        let both = Capabilities { herbivore: 1, carnivore: 1, ..default() };
        assert_eq!(diet_of(&herbivore), Diet::Herbivore);
        assert_eq!(diet_of(&carnivore), Diet::Carnivore);
        assert_eq!(diet_of(&both), Diet::Omnivore);
        assert_eq!(diet_of(&Capabilities::default()), Diet::Herbivore);
    }

    #[test]
    fn the_diet_icon_has_a_frame_per_diet() {
        let mut frames = [Diet::Omnivore, Diet::Carnivore, Diet::Herbivore].map(diet_frame);
        frames.sort_unstable();
        assert_eq!(frames, [0, 1, 2]);
    }

    #[test]
    fn the_default_stance_follows_the_mouth() {
        use crate::asset_catalog::Capabilities;
        assert_eq!(default_stance(&Capabilities { herbivore: 1, ..default() }), Stance::Social);
        assert_eq!(default_stance(&Capabilities { herbivore: 1, carnivore: 3, ..default() }), Stance::Social);
        assert_eq!(default_stance(&Capabilities { carnivore: 2, ..default() }), Stance::Combat);
        assert_eq!(default_stance(&Capabilities::default()), Stance::Combat);
    }

    #[test]
    fn general_abilities_use_the_existing_controls() {
        // The mate call, jump/glide on the space bar, sprint on shift, sneak on Z.
        assert_eq!(General::Call.key(), KeyCode::KeyM);
        assert_eq!(General::Jump.key(), General::Glide.key());
        assert_eq!(General::Sprint.key(), KeyCode::ShiftLeft);
        assert_eq!(General::Sneak.key(), KeyCode::KeyZ);
        // Only the abilities that are switched on while the key is down are held.
        let held: Vec<General> = GENERAL_ORDER.iter().copied().filter(|g| g.holds()).collect();
        assert_eq!(held, [General::Glide, General::Sprint]);
        let icons: std::collections::HashSet<_> = GENERAL_ORDER.iter().map(|g| g.icon().instance_id).collect();
        assert_eq!(icons.len(), GENERAL_ORDER.len(), "every ability has its own glyph");
    }

    #[test]
    fn ability_tooltips_name_the_ability_its_key_and_rank() {
        assert_eq!(verb_tooltip("Sing", 0, 2), "Sing - Level 2");
        assert_eq!(verb_tooltip("Spit", 3, 5), "Spit - Level 5");
        assert_eq!(verb_tooltip("Bite", 0, 0), "Bite [1]", "no level: the default string with the hotkey");
        let labels: Vec<_> = GENERAL_ORDER.iter().map(|g| format!("{} [{}]", g.name(), g.key_label())).collect();
        assert_eq!(labels, ["Call [M]", "Jump [Space]", "Glide [Space]", "Sprint [Shift]", "Sneak [Z]"]);
    }

    #[test]
    fn brain_thresholds_are_the_segment_ends_of_the_progress_bar() {
        let t = brain_thresholds();
        assert_eq!(t.len(), id::SEGMENTS.len());
        assert!(t[0] > 0.0 && t.windows(2).all(|w| w[0] < w[1]), "cut-offs increase: {t:?}");
        // The last one is the DNA that ends the stage.
        assert_eq!(t[3], crate::game::tuning_for(&GameStage::Creature).dna_to_advance);
    }

    #[test]
    fn ability_icons_exist_for_all_eight_verbs() {
        for stance in [Stance::Social, Stance::Combat] {
            for slot in 0..4 {
                assert!(verb_icon(stance, slot).is_some(), "{stance:?} slot {slot}");
            }
            assert!(verb_icon(stance, 4).is_none());
        }
        assert_ne!(verb_icon(Stance::Social, 0), verb_icon(Stance::Combat, 0));
    }

    #[test]
    fn the_stance_picks_its_own_tray() {
        let mut hud = CreatureHudState::default();
        hud.social[0].rank = 3;
        hud.combat[1].rank = 2;
        assert_eq!(hud.verbs()[0].rank, 3);
        hud.stance = Stance::Combat;
        assert_eq!(hud.verbs()[1].rank, 2);
    }

    fn creature_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Creature);
        app.update();
        app.update();
        app
    }

    #[test]
    fn the_published_state_follows_the_creature() {
        let mut app = creature_app();
        app.update();
        let hud = app.world().resource::<CreatureHudState>();
        assert!(hud.max_health > 0.0 && hud.health <= hud.max_health, "health {}/{}", hud.health, hud.max_health);
        assert!(hud.max_hunger > 0.0 && hud.hunger <= hud.max_hunger);
        // Without installed parts the creature has the starter abilities: sing and bite.
        assert_eq!(hud.social.map(|s| s.rank), [1, 0, 0, 0]);
        assert_eq!(hud.combat.map(|s| s.rank), [1, 0, 0, 0]);
        assert_eq!(hud.level_fill, [0.0; 4]);
        assert_eq!(hud.brain_level, 0);
        assert!(!hud.advance_visible && !hud.paused);
        assert_eq!(hud.general.first().map(|g| g.ability), Some(General::Call));
        assert!(hud.callouts.is_empty(), "no HUD, no callouts: the simulation is never held up");
    }

    #[test]
    fn dna_moves_the_progress_bar_and_the_pause_menu_is_published() {
        let mut app = creature_app();
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 150.0;
        app.world_mut().resource_mut::<RunProgress>().editor_dna = 42.9;
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = true;
        app.update();
        let hud = app.world().resource::<CreatureHudState>();
        assert_eq!(hud.level_fill, level_fills(150.0, &brain_thresholds()));
        assert_eq!(hud.dna_points, 42, "the number is the spendable DNA, rounded down");
        assert_eq!(hud.stage_dna, 150.0);
        assert!(hud.paused);
    }

    #[test]
    fn the_hud_starts_in_the_stance_of_the_diet_once() {
        let mut app = creature_app();
        assert_eq!(app.world().resource::<Encounter>().mode, EncounterMode::Social);
        // The HUD appears: the starter creature has no herbivorous mouth.
        app.world_mut().insert_resource(CreatureHudActive(true));
        app.update();
        assert_eq!(app.world().resource::<Encounter>().mode, EncounterMode::Combat);
        assert_eq!(app.world().resource::<CreatureHudState>().stance, Stance::Combat);
        // The player's own choice is not undone.
        app.world_mut().resource_mut::<Encounter>().mode = EncounterMode::Social;
        app.update();
        assert_eq!(app.world().resource::<Encounter>().mode, EncounterMode::Social);
    }

    #[test]
    fn a_progressed_lineage_keeps_its_stance() {
        let mut app = creature_app();
        app.world_mut().resource_mut::<RunProgress>().dna_this_stage = 30.0;
        app.world_mut().insert_resource(CreatureHudActive(true));
        app.update();
        assert_eq!(app.world().resource::<Encounter>().mode, EncounterMode::Social);
    }

    /// A root with the installed main layout, to click on.
    fn with_main_layout(app: &mut App) -> Option<Entity> {
        let (index, mut assets) = installed()?;
        let layout = assets.layout(&index, MAIN_LAYOUT)?;
        Some(app.world_mut().spawn((CreatureHudRoot::Main, SpuiState::new(layout, RootPolicy::Fit))).id())
    }

    fn click(app: &mut App, root: Entity, control: u32) {
        let mut ui = app.world_mut().get_mut::<SpuiState>(root).unwrap();
        let w = ui.find(control).unwrap();
        ui.clicks.push(w);
        app.update();
        // Nothing drives the keyboard in a headless app: start the next frame
        // with a clean state, like the input plugin does.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    }

    #[test]
    fn clicking_the_stance_buttons_switches_the_stance_through_the_keys() {
        let mut app = creature_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        click(&mut app, root, id::STANCE_COMBAT);
        assert_eq!(app.world().resource::<Encounter>().mode, EncounterMode::Combat);
        click(&mut app, root, id::STANCE_SOCIAL);
        assert_eq!(app.world().resource::<Encounter>().mode, EncounterMode::Social);
        // The injected key was let go again.
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyF));
    }

    /// Keys the simulation sees as just pressed.
    #[derive(Resource, Default)]
    struct SeenKeys(Vec<KeyCode>);

    fn watch_keys(app: &mut App) {
        app.init_resource::<SeenKeys>().add_systems(
            Update,
            (|keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<SeenKeys>| {
                for key in [KeyCode::Digit1, KeyCode::Digit4, KeyCode::F4] {
                    if keys.just_pressed(key) {
                        seen.0.push(key);
                    }
                }
            })
            .in_set(crate::GameSystems::Simulation),
        );
    }

    #[test]
    fn button_clicks_reach_the_simulation_as_fresh_key_presses() {
        let mut app = creature_app();
        watch_keys(&mut app);
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        click(&mut app, root, id::SOCIAL_VERBS[0]);
        click(&mut app, root, id::COMBAT_VERBS[3]);
        click(&mut app, root, id::ADVANCE);
        let seen = &app.world().resource::<SeenKeys>().0;
        assert_eq!(seen, &[KeyCode::Digit1, KeyCode::Digit4, KeyCode::F4]);
        // The mate call (the first general ability, always available) is read
        // by the controls, which open the Creature creator at the nest.
        assert!(!app.world().resource::<crate::editor::EditorState>().open);
        click(&mut app, root, id::GENERAL[0]);
        assert!(app.world().resource::<crate::editor::EditorState>().open);
    }

    #[test]
    fn the_pause_toggle_is_the_escape_key_the_pause_menu_reads() {
        let mut app = creature_app();
        let Some(root) = with_main_layout(&mut app) else {
            return;
        };
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
    }

    fn installed() -> Option<(ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_main_layout_has_every_control_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).expect("GlobalUICreature3-7");
        let single = [
            id::DNA_TEXT,
            id::BAR,
            id::MARKER,
            id::ADVANCE,
            id::ADVANCE_GLOW,
            id::HISTORY,
            id::PAUSE_TOGGLE,
            id::SPOREPEDIA,
            id::COLLECTIONS,
            id::STANCE_SOCIAL,
            id::STANCE_COMBAT,
            id::SOCIAL_TRAY,
            id::COMBAT_TRAY,
            id::SUPERPOWER,
            id::SUPERPOWER_LOCKED,
            id::GENERAL_GROUP,
            id::ROUND_BUTTON,
            id::HEALTH_PANEL,
            id::HEALTH_ROW,
            id::HUNGER_ROW,
            id::HEALTH_METER,
            id::HUNGER_METER,
            id::HEALTH_TEXT,
            id::DIET_ICON,
            id::PORTRAIT,
            id::MAP_TOGGLE,
            id::MAP_HOLDER,
        ];
        for control in single
            .iter()
            .chain(&id::SEGMENTS)
            .chain(&id::BRAIN_ICONS)
            .chain(&id::SOCIAL_VERBS)
            .chain(&id::COMBAT_VERBS)
            .chain(&id::GENERAL)
        {
            assert!(layout.find(*control).is_some(), "window {control:08X}");
        }
        // The social tray is the one holding the social buttons (the RE notes
        // list the two tray ids the other way round; the layout is the data).
        let holder = |button: u32| {
            let mut at = layout.find(button);
            while let Some(w) = at {
                let control = layout.window(w).control_id;
                if control == id::SOCIAL_TRAY || control == id::COMBAT_TRAY {
                    return control;
                }
                at = layout.window(w).parent;
            }
            0
        };
        assert_eq!(holder(id::SOCIAL_VERBS[0]), id::SOCIAL_TRAY);
        assert_eq!(holder(id::COMBAT_VERBS[0]), id::COMBAT_TRAY);
        // The five general slots sit in one group.
        let group = layout.find(id::GENERAL_GROUP).unwrap();
        for control in id::GENERAL {
            assert_eq!(layout.window(layout.find(control).unwrap()).parent, Some(group), "{control:08X}");
        }
        // The order of the ability buttons is the key order (their captions).
        for verbs in [id::SOCIAL_VERBS, id::COMBAT_VERBS] {
            for (slot, control) in verbs.iter().enumerate() {
                let button = layout.find(*control).unwrap();
                let key = layout.windows.iter().find_map(|w| {
                    (w.parent == Some(button)).then(|| match &w.caption {
                        Some(crate::spui::format::Text::Literal(s)) => s.clone(),
                        _ => String::new(),
                    })
                });
                assert_eq!(key.as_deref(), Some((slot + 1).to_string().as_str()), "{control:08X} key label");
            }
        }
    }

    fn demo_hud() -> CreatureHudState {
        let slot = |rank, cooldown| VerbSlot { rank, cooldown };
        CreatureHudState {
            dna_points: 118,
            brain_level: 1,
            level_fill: level_fills(140.0, &brain_thresholds()),
            health: 70.0,
            max_health: 100.0,
            hunger: 20.0,
            max_hunger: 100.0,
            stance: Stance::Social,
            social: [slot(2, 0.0), slot(1, 2.5), slot(0, 0.0), slot(0, 0.0)],
            combat: [slot(1, 0.0); 4],
            general: vec![
                GeneralSlot { ability: General::Call, active: false },
                GeneralSlot { ability: General::Sprint, active: true },
            ],
            diet: Diet::Carnivore,
            ..default()
        }
    }

    #[test]
    fn the_main_layout_shows_the_published_state() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let mut images = Assets::<Image>::default();
        let mut ui = SpuiState::new(layout, RootPolicy::Fit);
        let mut hud = demo_hud();
        let colors = ProgressColors::default();
        bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, &index);
        let w = |ui: &SpuiState, control: u32| ui.find(control).unwrap();
        // Text, stance and tray.
        assert_eq!(ui.captions[w(&ui, id::DNA_TEXT)].as_deref(), Some("\u{268B}118"));
        assert_eq!(ui.captions[w(&ui, id::HEALTH_TEXT)].as_deref(), Some("70"));
        assert!(ui.visible(w(&ui, id::SOCIAL_TRAY)) && !ui.visible(w(&ui, id::COMBAT_TRAY)));
        assert!(ui.states[w(&ui, id::STANCE_SOCIAL)] & crate::spui::ids::state::SELECTED != 0);
        assert!(ui.states[w(&ui, id::STANCE_COMBAT)] & crate::spui::ids::state::SELECTED == 0);
        // Abilities: icons for ranks above zero, empty disabled slots; a recharging
        // ability keeps its colours (the wipe is `recharge`'s).
        let button = |ui: &SpuiState, slot: usize| w(ui, id::SOCIAL_VERBS[slot]);
        assert!(ui.image_overrides[button(&ui, 0)].is_some() && ui.image_overrides[button(&ui, 2)].is_none());
        assert!(ui.flags[button(&ui, 0)] & crate::spui::ids::flag::ENABLED != 0);
        assert!(ui.flags[button(&ui, 2)] & crate::spui::ids::flag::ENABLED == 0);
        assert_eq!(ui.tints[button(&ui, 0)], 0xFFFF_FFFF);
        assert_eq!(ui.tints[button(&ui, 1)], 0xFFFF_FFFF);
        assert_eq!(ui.tooltip_overrides[button(&ui, 0)].as_deref(), Some("Sing - Level 2"));
        assert_eq!(ui.tooltip_overrides[button(&ui, 2)], None);
        // General abilities: the first two slots are shown, sprint looks selected.
        let general = |ui: &SpuiState, slot: usize| w(ui, id::GENERAL[slot]);
        assert!(ui.visible(general(&ui, 0)) && ui.visible(general(&ui, 1)) && !ui.visible(general(&ui, 2)));
        assert!(ui.icon_overrides[general(&ui, 1)].is_some());
        assert!(ui.states[general(&ui, 1)] & crate::spui::ids::state::SELECTED != 0);
        assert_eq!(ui.tooltip_overrides[general(&ui, 1)].as_deref(), Some("Sprint [Shift]"));
        // Meters: 70% of the authored 60 px, warning windows follow the motive threshold.
        let meter = ui.area(w(&ui, id::HEALTH_METER));
        assert!((meter[2] - meter[0] - 42.0).abs() < 0.01, "{meter:?}");
        assert!(ui.flags[w(&ui, id::HEALTH_ROW)] & crate::spui::ids::flag::ENABLED == 0, "health is fine");
        assert!(ui.flags[w(&ui, id::HUNGER_ROW)] & crate::spui::ids::flag::ENABLED != 0, "hunger is low");
        assert_eq!(ui.frame_overrides[w(&ui, id::DIET_ICON)], Some(diet_frame(Diet::Carnivore)));
        // Progress: the first brain icon is lit, History shows until the last level.
        assert_eq!(ui.frame_overrides[w(&ui, id::BRAIN_ICONS[0])], Some(1));
        assert_eq!(ui.frame_overrides[w(&ui, id::BRAIN_ICONS[1])], Some(0));
        assert!(ui.visible(w(&ui, id::HISTORY)) && !ui.visible(w(&ui, id::ADVANCE)));
        assert_eq!(ui.overlays[w(&ui, id::SEGMENTS[0])].map(|o| o.0), Some(1.0));
        assert!(ui.overlays[w(&ui, id::SEGMENTS[3])].is_none());
        // The combat stance with the last brain level: the other tray, Advance instead of History.
        hud.stance = Stance::Combat;
        hud.advance_visible = true;
        hud.brain_level = 4;
        hud.health = 10.0;
        bind_main(&mut ui, &hud, &colors, &mut assets, &mut images, &index);
        assert!(!ui.visible(w(&ui, id::SOCIAL_TRAY)) && ui.visible(w(&ui, id::COMBAT_TRAY)));
        assert!(ui.visible(w(&ui, id::ADVANCE)) && !ui.visible(w(&ui, id::HISTORY)));
        assert!(ui.flags[w(&ui, id::HEALTH_ROW)] & crate::spui::ids::flag::ENABLED != 0, "health is low");
        assert_eq!(ui.frame_overrides[w(&ui, id::BRAIN_ICONS[3])], Some(1));
    }

    #[test]
    fn installed_stance_and_meter_tooltips_are_the_retail_strings() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let tip = |assets: &mut SpuiAssets, control: u32| {
            let w = layout.find(control).unwrap();
            layout.window(w).procs.iter().find_map(|p| match p {
                crate::spui::model::WinProc::Tooltip { text: Some(text), .. } => Some(assets.text(&index, text)),
                _ => None,
            })
        };
        assert_eq!(tip(&mut assets, id::STANCE_SOCIAL).as_deref(), Some("Social Stance"));
        assert_eq!(tip(&mut assets, id::STANCE_COMBAT).as_deref(), Some("Combat Stance"));
        assert_eq!(tip(&mut assets, 0x0685_A85E).as_deref(), Some("Health"));
        assert_eq!(tip(&mut assets, 0x0685_A85F).as_deref(), Some("Hunger"));
        assert_eq!(tip(&mut assets, id::DNA_PANEL).as_deref(), Some("DNA Points"));
    }

    #[test]
    fn installed_ability_tooltip_strings_are_the_ones_the_hud_formats() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let mut text = |table: &str, id: u32| assets.localized(&index, crate::world_objects::name_id(table), id);
        assert_eq!(text("CRG_AbilityUI", 0x04AC_E6B0).as_deref(), Some(LEVEL_WORD));
        assert_eq!(
            text("tooltips", 0x0681_D203).as_deref(),
            Some("~local_ability_name~ - ~local_level_string~ ~local_ability_level~")
        );
        assert_eq!(text("tooltips", 0x0681_D202).as_deref(), Some("~local_ability_name~ [~local_hotkey~]"));
    }
}
