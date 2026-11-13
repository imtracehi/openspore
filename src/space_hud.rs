//! The retail Space-stage HUD: spawns the SPUI layouts the original game loads
//! for the Space stage and binds them to [`SpaceHudState`].
//!
//! * `GlobalUISpace2-1`: the shared progress bar at the bottom (rank progress
//!   over ten segments with ten rank pips, Sporebucks), the galactic core meter,
//!   the health and energy bars, the tool charging bar, the tool palette (eight
//!   tabs, paged panels), the starmap filter toggles and hold-type camera
//!   buttons, the planet panel (terraform score bullseye, T1-T3, food web slots,
//!   colony slots, climate icons), the species/communication panel and the
//!   Capture System / Purchase System panel ([`world`], [`palette`]).
//! * `GlobalUIOptions-2`: the swirl orb (options/menu button), shared with the
//!   Cell and Creature stages.
//! * `SPGMissionCard`: the mission cards at the top left ([`guide`]).
//! * `SpaceMainTool` and friends: one tool button per palette slot, spawned as
//!   separate roots and laid over the palette frames ([`palette`]).
//! * `CommScreen-3`: the communication screen of an alien empire ([`comm`]).
//! * the retail callout dialogs and hints (`Callout_SPG_*`, `HintsSPG`),
//!   through the generic machinery of `cell_hud::dialogs` ([`guide`]).
//!
//! The layouts carry all geometry and art. This module only decides which
//! windows are shown, fills their text and meters, and turns button clicks into
//! the keys the Space controls already use, so the Space gameplay modules are
//! not touched: [`publish_state`] fills the [`SpaceHudState`] contract from the
//! Space resources and components each frame. The only writes into Space state
//! are the player's own choices (the selected planetary tool, the starmap
//! filter), each in one function ([`world::set_filter`], [`palette::select`]).
//!
//! Control ids and roles come from the layouts and their tooltips and from
//! `notes\space-re-notes.md` S5 (see `notes\space-hud-notes.md`). Where the
//! retail code's behaviour is not documented the binding is marked `ADAPTED`.
//! `space-rules` and `space-ctrl` change the Space rules and controls in
//! parallel: everything this module reads from `SpaceCampaign` goes through
//! the few `snapshot_*` functions below (marked TODO where a retail model is
//! expected to replace today's stand-in).

use bevy::prelude::*;

pub mod collections;
pub mod comm;
pub mod guide;
pub mod minimap;
pub mod palette;
pub mod reward;
pub mod tutorial;
pub mod world;

use crate::{
    asset_catalog::ResourceIndex,
    cell_contract::CellGoalCard,
    cell_hud::ProgressColors,
    game::{Health, Planet, Player, RunProgress},
    persistence::WorldId,
    space::SpaceCampaign,
    space_galaxy::Galaxy,
    space_view::{GalaxyFilter, SpaceView, SpaceZoom},
    spui::{
        model::WinId,
        solve::{Rect, RootPolicy},
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const MAIN_LAYOUT: &str = "GlobalUISpace2-1";
pub const CARD_LAYOUT: &str = "SPGMissionCard";
pub const OPTIONS_LAYOUT: &str = crate::cell_hud::OPTIONS_LAYOUT;

/// Control ids of `GlobalUISpace2-1` (roles from the layout and its tooltips
/// and `space-re-notes.md` S5A; some ids are only checked by the tests).
#[allow(dead_code)]
pub mod id {
    // Bottom bar: shared progress bar (rank progress + Sporebucks).
    pub const BAR: u32 = 0x051B_5478;
    /// The ten progress segments, one per rank (`051B54C0..C9`).
    pub const SEGMENTS: [u32; 10] = [
        0x051B_54C0,
        0x051B_54C1,
        0x051B_54C2,
        0x051B_54C3,
        0x051B_54C4,
        0x051B_54C5,
        0x051B_54C6,
        0x051B_54C7,
        0x051B_54C8,
        0x051B_54C9,
    ];
    /// The ten rank pips (animated icons: dark, lit).
    pub const PIPS: [u32; 10] = [
        0x0616_1080,
        0x0616_1081,
        0x0616_1082,
        0x0616_1083,
        0x0616_1084,
        0x0616_1085,
        0x0616_1086,
        0x0616_1087,
        0x0616_1088,
        0x0616_1089,
    ];
    /// Sporebucks caption and its panel (tooltip "Sporebucks").
    pub const MONEY: u32 = 0xABCD_1001;
    pub const MONEY_PANEL: u32 = 0x051B_6DB0;
    pub const HISTORY: u32 = 0x0448_27B8;
    pub const PAUSE_TOGGLE: u32 = 0xD305_CA84;
    pub const SPOREPEDIA: u32 = 0x0269_9440;
    pub const COLLECTIONS: u32 = 0x0624_4208;
    // Galactic core meter: marker, home mark and fill.
    pub const CORE_METER: u32 = 0x0616_EBC0;
    pub const CORE_MARKER: u32 = 0x0617_4050;
    pub const CORE_HOME: u32 = 0x0616_EBB8;
    pub const CORE_FILL: u32 = 0x0616_EBC8;
    // Health and energy (right).
    pub const VITALS: u32 = 0x065E_40F0;
    pub const HEALTH_ROW: u32 = 0x0639_6B68;
    pub const HEALTH_TEXT: u32 = 0x04D0_39A8;
    pub const HEALTH_BAR: u32 = 0x01C4_04AC;
    pub const ENERGY_ROW: u32 = 0x0639_6B78;
    pub const ENERGY_BAR: u32 = 0x0548_1520;
    /// Tool charging bar (`Charging...`) and its fill.
    pub const CHARGING: u32 = 0x0341_BA47;
    pub const CHARGING_FILL: u32 = 0x0341_BA3B;
    pub const UPGRADE: u32 = 0x05E3_BFBD;
    pub const PICKUP_SCOUT: u32 = 0x02C3_8AB9;
    pub const POSSE: u32 = 0x064E_7C20;
    // Species / communication panel.
    pub const SHIP_PANEL: u32 = 0x05D6_46C8;
    pub const SPECIES_NAME: u32 = 0x044D_94D1;
    pub const HAPPINESS: u32 = 0x03FE_A475;
    pub const PORTRAIT: u32 = 0x036C_0957;
    pub const COMM_FRAME: u32 = 0x05D6_AA28;
    pub const COMM_GLOW: u32 = 0x061B_0530;
    pub const COMM_BUTTON: u32 = 0x017F_29BF;
    // Starmap filters and the camera cluster.
    pub const FILTERS: u32 = 0x02E1_AD08;
    pub const FILTER_MY_EMPIRE: u32 = 0x02E1_B6F8;
    pub const FILTER_ALLIES: u32 = 0x02E1_CBD7;
    pub const FILTER_EMPIRES: u32 = 0x02E1_B708;
    pub const FILTER_MISSIONS: u32 = 0x02E1_B720;
    pub const FILTER_TRAIL: u32 = 0x02E1_B72C;
    pub const FILTER_VISITED: u32 = 0x02E1_B73C;
    pub const FILTER_BUTTONS: [u32; 6] =
        [FILTER_MY_EMPIRE, FILTER_ALLIES, FILTER_EMPIRES, FILTER_MISSIONS, FILTER_TRAIL, FILTER_VISITED];
    /// Camera cluster of the galaxy / system context (zoom in, zoom out and
    /// two rotate buttons; the same ids exist in the planet panel with the
    /// prefix `04CAB5..`).
    pub const GALAXY_CAMERA: u32 = 0x053F_4AE0;
    pub const GALAXY_ZOOM_IN: u32 = 0x05CA_B570;
    pub const GALAXY_ZOOM_OUT: u32 = 0x05CA_B571;
    pub const GALAXY_ROTATE_LEFT: u32 = 0x05CA_B56F;
    pub const GALAXY_ROTATE_RIGHT: u32 = 0x05CA_B56E;
    pub const PLANET_ZOOM_IN: u32 = 0x04CA_B570;
    pub const PLANET_ZOOM_OUT: u32 = 0x04CA_B571;
    pub const PLANET_ROTATE_LEFT: u32 = 0x04CA_B56F;
    pub const PLANET_ROTATE_RIGHT: u32 = 0x04CA_B56E;
    // Capture / Purchase System panel.
    pub const CAPTURE_PANEL: u32 = 0x0656_80F8;
    pub const CAPTURE_BUTTON: u32 = 0x0656_80E8;
    pub const PURCHASE_BUTTON: u32 = 0x065C_E2DA;
    pub const CAPTURE_TEXT: [u32; 2] = [0x0656_7050, 0x0656_7051];
    pub const CAPTURE_LIST: u32 = 0x065B_9B00;
    pub const CAPTURE_PLANETS: [u32; 5] = [0x0656_7060, 0x0657_A850, 0x0657_A848, 0x0657_A838, 0x0657_A840];
    /// Windows of the mission log and the tool reward dialog (hidden here).
    pub const MISSION_LOG: u32 = 0x06B8_21A8;
    pub const REWARD: u32 = 0x0664_C270;
}

/// Which of the Space layouts a spawned root is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpaceHudRoot {
    Main,
    Orb,
    /// Mission card number `n` (top left, stacked).
    Card(u8),
    /// The palette slot `n` (a `SpaceMainTool` button laid over a slot frame).
    Tool(u8),
    /// The communication screen (`CommScreen-3`).
    Comm,
    /// Hint bar and callout dialog.
    Hint,
    Callout,
    /// My Collections (0x40464200!0xD9B09D9D) and its badge card `n`.
    Collections,
    Badge(u8),
    /// Row `n` of the collections category column
    /// (`AssetBrowserFeedListCategory`).
    Category(u8),
}

/// True while the SPUI Space HUD is shown; the legacy Space HUD, the legacy
/// terraforming console and the pause panel step aside.
#[derive(Resource, Default)]
pub struct SpaceHudActive(pub bool);

/// The view the HUD follows (the three Space cameras).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HudZoom {
    /// The UFO low over one planet: planet panel and terraforming.
    Planet,
    #[default]
    System,
    Galaxy,
}

impl From<SpaceZoom> for HudZoom {
    fn from(zoom: SpaceZoom) -> Self {
        match zoom {
            SpaceZoom::Planet => HudZoom::Planet,
            SpaceZoom::System => HudZoom::System,
            SpaceZoom::Galaxy => HudZoom::Galaxy,
        }
    }
}

/// Distance to the galactic core and to the player's home star, in the
/// galactic plane.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CoreMeter {
    /// Parsecs to the galactic core (the meter's own unit, see [`core_marker`]).
    pub distance: f32,
    /// Parsecs from the home star to the core.
    pub home: f32,
    /// Angle around the core, degrees (the marker tooltip).
    pub angle: f32,
}

/// The planet the UFO hovers over, for the terraform panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanetPanel {
    pub world: u32,
    pub name: String,
    /// Temperature and atmosphere scores, `0.0..=1.0` (bullseye x and 1 - y).
    pub temperature: f32,
    pub atmosphere: f32,
    /// The marble's velocity `(temperature, atmosphere)` (`+0x1FD8` / `+0x1FD4` of
    /// the planet object): the bullseye's arrow follows it (0x1066F30).
    pub velocity: (f32, f32),
    /// Terraform level reached, `0..=3`.
    pub t_score: u8,
    /// Food web: plants, herbivores and carnivores in the planet's web.
    pub flora: u8,
    pub herbivores: u8,
    pub carnivores: u8,
    /// Colonies on the planet and the planet's colony limit.
    pub colonies: u8,
    pub colony_limit: u8,
    /// An alien empire owns the planet (name of the empire, relation label).
    pub empire: Option<(String, String)>,
}

/// The Capture System / Purchase System panel of an empire's system.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapturePanel {
    pub empire: String,
    /// The colonised planets of the system, `true` once one has surrendered.
    pub planets: Vec<bool>,
    pub can_capture: bool,
    pub can_purchase: bool,
}

/// Everything the Space HUD displays, filled by [`publish_state`] (and by
/// [`guide`] / [`comm`] for their parts) so that the HUD never reads
/// simulation internals itself.
#[derive(Resource, Clone, Debug, Default)]
pub struct SpaceHudState {
    pub zoom: HudZoom,
    /// The options menu is open (the game is paused).
    pub paused: bool,
    /// Sporebucks. TODO(space-rules): the wallet; today the spice stock.
    pub money: i64,
    /// Badge points and the ranks they earned (`0..=10`) with the fraction of
    /// the way to the next one.
    pub badge_points: u32,
    pub rank: u8,
    pub rank_fraction: f32,
    pub health: f32,
    pub max_health: f32,
    pub energy: f32,
    pub max_energy: f32,
    /// A tool is recharging: how far it has come, `0.0..=1.0`.
    pub charging: Option<f32>,
    pub core: Option<CoreMeter>,
    pub tools: Vec<palette::HudTool>,
    pub filter: GalaxyFilter,
    pub planet: Option<PlanetPanel>,
    pub capture: Option<CapturePanel>,
    /// Species/communication panel: the empire the UFO is near, if any.
    pub comm: Option<comm::CommTarget>,
    /// The retail communication screen is open.
    pub comm_open: bool,
    /// Goal cards in display order.
    pub goal_cards: Vec<CellGoalCard>,
    /// The tool reward window: the ids of the tools unlocked.
    pub reward: Option<Vec<String>>,
    /// Callouts to show, front first (`Callout_SPG_Opening`, ...).
    pub callouts: Vec<String>,
    /// Active hint: a retail hint id (`spg-fly`) or `msg:<text>`.
    pub hint: Option<String>,
    /// The last feedback line of the campaign and the galaxy map's caption of
    /// the selected star.
    pub message: String,
    pub galaxy_caption: Option<String>,
}

impl SpaceHudState {
    pub fn health_fraction(&self) -> f32 {
        fraction(self.health, self.max_health)
    }

    pub fn energy_fraction(&self) -> f32 {
        fraction(self.energy, self.max_energy)
    }
}

fn fraction(value: f32, max: f32) -> f32 {
    if max > 0.0 && value.is_finite() {
        (value / max).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// What the user chose in the HUD itself (palette tab and page, hidden panels):
/// state of the interface, not of the game.
#[derive(Resource, Clone, Debug)]
pub struct SpaceHudUi {
    pub tab: palette::Tab,
    /// The palette is folded away (the Hide button).
    pub palette_hidden: bool,
    /// Page of the palette panel of each tab.
    pub page: [usize; 8],
    /// The planet panel shows the minimap instead of the terraform panel.
    pub minimap: bool,
    /// The planet panel is folded away (its Hide button).
    pub planet_hidden: bool,
    /// Tool index (into [`SpaceHudState::tools`]) of every palette slot root,
    /// refreshed by the binding.
    pub slot_tools: Vec<Option<usize>>,
}

impl Default for SpaceHudUi {
    fn default() -> Self {
        Self {
            tab: palette::Tab::Main,
            palette_hidden: false,
            page: [0; 8],
            minimap: false,
            planet_hidden: false,
            slot_tools: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure rules (unit tested).
// ---------------------------------------------------------------------------

/// Fill of the progress bar: the ten equal segments fill one after the other
/// (`rank + fraction` of the ten).
pub fn bar_fill(rank: u8, fraction: f32) -> f32 {
    ((rank as f32 + fraction.clamp(0.0, 1.0)) / 10.0).clamp(0.0, 1.0)
}

/// Sporebucks as the HUD writes them: `$` and the amount with thousands
/// separators (the authored caption is `$10,000,000`).
pub fn format_money(money: i64) -> String {
    let digits = money.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{}${grouped}", if money < 0 { "-" } else { "" })
}

/// The rim of the galaxy in the core meter's unit (RE S5B: the meter uses
/// 10,000 parsecs as the rim distance).
pub const CORE_RIM_PC: f32 = 10_000.0;

/// Position of the core meter's marker along the meter, `0.0..=1.0`, from the
/// player's distance `d` to the core, the home star's distance `dh` and the
/// position `m` of the home mark (RE S5B 0x106A2A0: `(1 - d/dh)(1 - m) + m`
/// inside the home distance, `(1 - (d - dh)/(10000 - dh)) m` outside).
pub fn core_marker(d: f32, dh: f32, m: f32) -> f32 {
    let dh = dh.max(1e-3);
    let p = if d <= dh { (1.0 - d / dh) * (1.0 - m) + m } else { (1.0 - (d - dh) / (CORE_RIM_PC - dh).max(1e-3)) * m };
    p.clamp(0.0, 1.0)
}

/// Fill of the Charging bar: the frame width times `clamp(p, 0.15, 1.0)`
/// (RE S5B 0x1067AB0).
pub fn charging_fill(progress: f32) -> f32 {
    progress.clamp(0.15, 1.0)
}

/// Where the terraform bullseye marker sits, `(x, y)` in `0.0..=1.0` of the
/// bullseye window: x is the temperature score, y is one minus the atmosphere
/// score (RE S5B 0x1066F30).
pub fn bullseye_position(temperature: f32, atmosphere: f32) -> (f32, f32) {
    (temperature.clamp(0.0, 1.0), 1.0 - atmosphere.clamp(0.0, 1.0))
}

/// Game units of the galactic plane to the core meter's parsecs: the galaxy
/// radius (700 units, the star grid half-width) is the rim.
pub fn to_core_parsecs(distance: f32) -> f32 {
    distance / crate::space_galaxy::GALAXY_RADIUS * CORE_RIM_PC
}

// ---------------------------------------------------------------------------
// Snapshots of the Space state. Everything the HUD reads from `SpaceCampaign`
// goes through these, so the parallel `space-rules` change (wallet, retail
// ranks and missions) only has to touch them.
// ---------------------------------------------------------------------------

/// Sporebucks, rank and health/energy of the UFO. Reads `RunProgress::spice`
/// (the Sporebucks wallet), `SpaceCampaign::{total_badge_points, captain_rank,
/// rank_progress, energy, max_reactor_energy, archetype.cooldown_secs,
/// superweapon_cooldown, max_hull, message}` and the player's `Health`.
fn snapshot_ship(run: &RunProgress, health: Option<&Health>, hud: &mut SpaceHudState) {
    let campaign = &run.space_campaign;
    // The wallet: Sporebucks (start 100,000), separate from the spice cargo.
    hud.money = run.spice.round() as i64;
    hud.badge_points = campaign.total_badge_points();
    // Promotions earned (the lit pips) and the way to the next one (the bar).
    hud.rank = campaign.captain_rank();
    hud.rank_fraction = campaign.rank_progress();
    // Health: 1000 x level x difficulty (the Health component keeps it).
    match health {
        Some(health) => {
            hud.health = health.hp;
            hud.max_health = health.max;
        }
        None => hud.max_health = campaign.max_hull(),
    }
    hud.energy = campaign.energy;
    hud.max_energy = campaign.max_reactor_energy();
    // The archetype superpower recharges for `cooldown_secs`: the charging bar.
    // TODO(space-rules): the other tools' recharge (shield, cloak) when they exist.
    let total = campaign.archetype.cooldown_secs();
    hud.charging =
        (campaign.superweapon_cooldown > 0.0 && total > 0.0).then(|| 1.0 - campaign.superweapon_cooldown / total);
    hud.message = campaign.message.clone();
}

/// The planet panel of world `id`.
fn snapshot_planet(campaign: &SpaceCampaign, id: u32, name: &str) -> PlanetPanel {
    let ecology = campaign.ecology_for(id);
    let empire = campaign.world_empire(id).map(|e| {
        let empire = crate::space_galaxy::EMPIRES.get(e as usize).map_or("Alien", |e| e.0);
        (empire.to_string(), campaign.relation_label(e).to_string())
    });
    PlanetPanel {
        world: id,
        name: name.to_string(),
        temperature: ecology.temperature,
        atmosphere: ecology.atmosphere,
        velocity: campaign.marbles.get(&id).map_or((0.0, 0.0), |m| (m.v_temperature, m.v_atmosphere)),
        t_score: ecology.t_score(),
        flora: ecology.flora,
        herbivores: ecology.herbivores,
        carnivores: ecology.carnivores,
        colonies: ecology.colony_count,
        colony_limit: ecology.colony_capacity(),
        empire,
    }
}

/// The Capture / Purchase panel of the current system: its empire's
/// colonies, which of them have surrendered (defences down) and whether a
/// trade route to the system has matured (RE S5B 0x1069AC0, adapted to the
/// campaign's empire colonies).
fn snapshot_capture(campaign: &SpaceCampaign, worlds: &[(u32, bool)]) -> Option<CapturePanel> {
    let empire = campaign.current_system_empire?;
    let planets: Vec<(u32, bool)> = worlds
        .iter()
        .copied()
        .filter(|(id, _)| campaign.world_empire(*id) == Some(empire) && !campaign.colonies.contains(id))
        .map(|(id, _)| (id, campaign.empire_defense_hp(id) <= 0.0))
        .take(5)
        .collect();
    if planets.is_empty() {
        return None;
    }
    let matured = planets.iter().any(|(id, _)| campaign.trade_routes.get(id).is_some_and(|p| *p >= 1.0));
    Some(CapturePanel {
        empire: crate::space_galaxy::EMPIRES.get(empire as usize).map_or("Alien", |e| e.0).to_string(),
        planets: planets.iter().map(|(_, surrendered)| *surrendered).collect(),
        can_capture: planets.iter().all(|(_, surrendered)| *surrendered),
        can_purchase: matured,
    })
}

/// Distance to the core and home distance from the galaxy.
fn snapshot_core(galaxy: &Galaxy, campaign: &SpaceCampaign) -> Option<CoreMeter> {
    let star = galaxy.star(campaign.current_system)?;
    let home = galaxy.star(crate::space_galaxy::HOME_STAR)?;
    Some(CoreMeter {
        distance: to_core_parsecs(star.position.length()),
        home: to_core_parsecs(home.position.length()),
        angle: star.position.y.atan2(star.position.x).to_degrees().rem_euclid(360.0),
    })
}

pub struct SpaceHudPlugin;

impl Plugin for SpaceHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpaceHudState>()
            .init_resource::<SpaceHudActive>()
            .init_resource::<SpaceHudUi>()
            .init_resource::<guide::GuideState>()
            .init_resource::<guide::GuideCache>()
            .init_resource::<palette::ToolInfoCache>()
            .init_resource::<comm::CommState>()
            .init_resource::<crate::space_comm::CommSession>()
            .init_resource::<crate::space_comm::CnvLibrary>()
            .init_resource::<reward::RewardState>()
            .init_resource::<collections::CollectionsState>()
            .add_systems(
                Update,
                (
                    publish_state,
                    guide::publish_guide,
                    comm::publish_comm,
                    comm::drive_session,
                    reward::publish_reward,
                    demo_state,
                    manage,
                    bind,
                    palette::bind_items,
                    guide::bind_guide,
                    comm::bind_comm,
                    reward::bind_reward,
                    collections::bind,
                )
                    .chain()
                    .before(view::sync)
                    .after(view::input),
            )
            // Clicks recorded by the SPUI input of the last frame become key
            // presses before the controls (and then the simulation) read the
            // keyboard: ahead of the first system of the controls chain.
            .add_systems(
                Update,
                actions
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Space)),
            )
            // The minimap picture (a bevy UI node over the minimap holder) follows
            // the laid out layout.
            .add_systems(Update, (minimap::manage_minimap, minimap::update_minimap).chain().after(view::sync))
            // A click on the planet minimap sends the UFO there (0xE0B970); read before the
            // controls see the press.
            .add_systems(
                Update,
                minimap::minimap_pointer
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Space)),
            )
            // The comm screen's code-made rows (responses, inventories).
            .add_systems(Update, (comm::manage_rows, comm::update_rows).chain().after(view::sync))
            // The legacy terraforming console and map are bevy-UI panels the
            // Space gameplay module keeps up to date; they step aside while the
            // retail HUD shows the same information.
            .add_systems(
                Update,
                hide_legacy.in_set(crate::GameSystems::Presentation).run_if(in_state(GameStage::Space)),
            );
    }
}

/// Hide the legacy Space console and the legacy view caption while the retail
/// HUD is up (the planet panel and the hint bar show the same information).
fn hide_legacy(
    active: Res<SpaceHudActive>,
    mut legacy: Query<
        &mut Visibility,
        Or<(With<crate::space::SpaceConsoleRoot>, With<crate::space_galaxy::SpaceCaption>)>,
    >,
) {
    if !active.0 {
        return;
    }
    for mut v in &mut legacy {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
}

// ---------------------------------------------------------------------------
// Capture demo.
// ---------------------------------------------------------------------------

/// Flags of the capture demo.
struct DemoFlags {
    on: bool,
    paused: bool,
    tab: Option<u32>,
    comm: bool,
    capture: bool,
    minimap: bool,
}

impl DemoFlags {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
        Self {
            on: has("--space-hud-demo"),
            paused: has("--space-hud-paused"),
            tab: value("--space-hud-tab").and_then(|v| v.parse().ok()),
            comm: has("--space-hud-comm"),
            capture: has("--space-hud-capture"),
            minimap: has("--space-hud-minimap"),
        }
    }
}

/// `--space-hud-demo` (captures): replace what the game published with a
/// mid-game HUD state so that every part of the layout can be looked at:
/// rank progress and money, a damaged hull, a charging tool, the planet panel
/// of an empire world, the capture panel. `--space-hud-paused` also opens the
/// options menu, `--space-hud-tab N` picks the palette tab (0..=7),
/// `--space-hud-comm` opens the communication screen, `--space-hud-capture`
/// shows the Capture System panel and `--space-hud-minimap` the minimap of the
/// planet panel (`--space-hud-reward` shows the tool reward window).
fn demo_state(
    stage: Res<State<GameStage>>,
    mut demo: Local<Option<DemoFlags>>,
    mut hud: ResMut<SpaceHudState>,
    mut ui: ResMut<SpaceHudUi>,
    mut run: ResMut<RunProgress>,
) {
    let flags = demo.get_or_insert_with(DemoFlags::from_args);
    if !flags.on || *stage.get() != GameStage::Space {
        return;
    }
    // The capture owns the table tools the palette shows generically (shield,
    // cloak, potions, the antimatter weapons) so the combat panel can be looked at.
    if !run.space_campaign.has_tool("shield") {
        for id in ["shield", "cloak", "rallycall"] {
            run.space_campaign.grant_tool(id);
        }
        for id in ["repairpotion", "repairmegapotion", "energypotion", "energymegapotion", "antimatterbomb", "antimattermissile", "aoe_repair"] {
            run.space_campaign.add_charges(id, 3);
        }
    }
    hud.money = 120_450;
    hud.badge_points = 58;
    hud.rank = crate::space_rules::rank_from_points(hud.badge_points);
    hud.rank_fraction = crate::space_rules::rank_progress(hud.badge_points);
    hud.max_health = 1000.0;
    hud.health = 640.0;
    hud.max_energy = 100.0;
    hud.energy = 72.0;
    hud.charging = Some(0.4);
    hud.core = Some(CoreMeter { distance: 6200.0, home: 7400.0, angle: 112.0 });
    hud.paused = flags.paused;
    if hud.planet.is_none() && hud.zoom == HudZoom::Planet {
        hud.planet = Some(PlanetPanel {
            world: 3,
            name: "Nyxor III".into(),
            temperature: 0.46,
            atmosphere: 0.58,
            // A moving marble, so the bullseye arrow shows (`--space-hud-demo`).
            velocity: (0.08, 0.05),
            t_score: 2,
            flora: 5,
            herbivores: 3,
            carnivores: 1,
            colonies: 1,
            colony_limit: 2,
            empire: None,
        });
    }
    // A moving marble on whatever planet panel is up, so the bullseye arrow shows.
    if let Some(panel) = hud.planet.as_mut().filter(|p| p.velocity == (0.0, 0.0)) {
        panel.velocity = (0.08, 0.05);
    }
    if flags.capture {
        hud.capture = Some(CapturePanel {
            empire: "Zelem".into(),
            planets: vec![true, false, true],
            can_capture: false,
            can_purchase: true,
        });
    }
    if (flags.comm || flags.capture) && hud.comm.is_none() && hud.zoom == HudZoom::Planet {
        hud.comm = Some(comm::CommTarget {
            empire: 3,
            name: "Avorni".into(),
            relation: "Neutral".into(),
            greeting: "Greetings, traveller. We have watched your colonies with interest.".into(),
            incoming: flags.capture,
            contract: Some("Pirate Hunt (1/3): destroy the raiders in this system".into()),
            owner_cost_index: 4,
            ..Default::default()
        });
    }
    if flags.comm {
        hud.comm_open = true;
    }
    if flags.minimap {
        ui.minimap = true;
    }
    if let Some(tab) = flags.tab {
        if let Some(tab) = palette::Tab::ALL.get(tab as usize) {
            ui.tab = *tab;
        }
    }
}

// ---------------------------------------------------------------------------
// Publishing the game state.
// ---------------------------------------------------------------------------

/// Fill [`SpaceHudState`] from the Space resources and components.
#[allow(clippy::too_many_arguments)]
fn publish_state(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    view: Option<Res<SpaceView>>,
    galaxy: Option<Res<Galaxy>>,
    pause: Res<crate::pause_menu::PauseMenu>,
    player: Query<(&Health, &Transform), With<Player>>,
    planets: Query<(&WorldId, &Transform, &Planet)>,
    mut hud: ResMut<SpaceHudState>,
) {
    if *stage.get() != GameStage::Space {
        return;
    }
    let zoom = view.as_deref().map_or(HudZoom::System, |v| v.zoom().into());
    hud.zoom = zoom;
    hud.filter = view.as_deref().map_or(GalaxyFilter::All, |v| v.filter);
    hud.paused = pause.open;
    let (health, position) = match player.single() {
        Ok((h, t)) => (Some(h), Some(t.translation)),
        Err(_) => (None, None),
    };
    snapshot_ship(&run, health, &mut hud);
    hud.core = galaxy.as_deref().and_then(|g| snapshot_core(g, &run.space_campaign));
    hud.tools = palette::build_tools(&run.space_campaign, run.spice);
    // The world the UFO is over: the nearest planet.
    let nearest = position.and_then(|p| {
        planets.iter().min_by(|a, b| {
            p.distance(a.1.translation).total_cmp(&p.distance(b.1.translation)).then(a.0 .0.cmp(&b.0 .0))
        })
    });
    hud.planet = nearest.map(|(id, _, planet)| snapshot_planet(&run.space_campaign, id.0, &planet.name));
    let worlds: Vec<(u32, bool)> = planets.iter().map(|(id, _, p)| (id.0, p.colonized)).collect();
    hud.capture = snapshot_capture(&run.space_campaign, &worlds);
    hud.galaxy_caption = match (galaxy.as_deref(), view.as_deref()) {
        (Some(galaxy), Some(view)) if view.galaxy => {
            Some(crate::space_galaxy::galaxy_caption(galaxy, &run.space_campaign, view))
        }
        _ => None,
    };
}

// ---------------------------------------------------------------------------
// Spawning.
// ---------------------------------------------------------------------------

/// Spawn or despawn the layouts depending on the stage.
#[allow(clippy::too_many_arguments)]
fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    options: Res<crate::HudOptions>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut active: ResMut<SpaceHudActive>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &SpaceHudRoot)>,
) {
    let want = *stage.get() == GameStage::Space && !editor.open && !options.diagnostics;
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        active.0 = false;
        return;
    }
    if roots.iter().any(|(_, r)| *r == SpaceHudRoot::Main) {
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
    // Mission cards first so that the bottom bar stays above them.
    for n in 0..guide::MAX_CARDS {
        spawn(&mut commands, &mut assets, CARD_LAYOUT, SpaceHudRoot::Card(n as u8), RootPolicy::Authored, 99);
    }
    let root = view::spawn(&mut commands, main, RootPolicy::Fit, Some(camera), 100);
    commands.entity(root).insert(SpaceHudRoot::Main);
    // One tool button per palette slot, over the main layout.
    for n in 0..palette::SLOTS {
        spawn(&mut commands, &mut assets, palette::TOOL_LAYOUT, SpaceHudRoot::Tool(n as u8), RootPolicy::Authored, 101);
    }
    spawn(&mut commands, &mut assets, OPTIONS_LAYOUT, SpaceHudRoot::Orb, RootPolicy::Fit, 102);
    spawn(&mut commands, &mut assets, comm::COMM_LAYOUT, SpaceHudRoot::Comm, RootPolicy::Fit, 103);
    spawn(&mut commands, &mut assets, collections::LAYOUT, SpaceHudRoot::Collections, RootPolicy::Fit, 106);
    for n in 0..collections::PAGE {
        spawn(&mut commands, &mut assets, collections::ITEM_LAYOUT, SpaceHudRoot::Badge(n as u8), RootPolicy::Authored, 107);
    }
    for n in 0..collections::ROWS {
        spawn(&mut commands, &mut assets, collections::CATEGORY_LAYOUT, SpaceHudRoot::Category(n as u8), RootPolicy::Authored, 107);
    }
    // The hint bar and the callout dialog stay spawned (hidden) so that their
    // pop-in effects run when they are shown.
    spawn(&mut commands, &mut assets, crate::cell_hud::dialogs::HINT_LAYOUT, SpaceHudRoot::Hint, RootPolicy::Fit, 104);
    spawn(
        &mut commands,
        &mut assets,
        crate::cell_hud::dialogs::CALLOUT_LAYOUT,
        SpaceHudRoot::Callout,
        RootPolicy::Authored,
        // Above My Collections (106): its pages show callouts (0x1064645).
        108,
    );
    active.0 = true;
}

// ---------------------------------------------------------------------------
// Binding the main layout.
// ---------------------------------------------------------------------------

pub(crate) fn window(state: &SpuiState, control: u32) -> Option<WinId> {
    state.find(control)
}

pub(crate) fn set_visible(ui: &mut SpuiState, control: u32, on: bool) {
    if let Some(w) = window(ui, control) {
        ui.set_visible(w, on);
    }
}

/// Size a meter window to `fraction` of its authored width (its children are
/// clipped by it).
pub(crate) fn set_meter(ui: &mut SpuiState, control: u32, fraction: f32) {
    if let Some(w) = window(ui, control) {
        let authored = ui.layout.window(w).area;
        let mut area = authored;
        area[2] = authored[0] + (authored[2] - authored[0]) * fraction.clamp(0.0, 1.0);
        ui.set_area(w, area);
    }
}

/// Move a window so that its centre lies at screen x `x` (a marker): the
/// window keeps its authored size and its y. Works on any layout style: the
/// correction is taken against where the window was solved last frame.
pub(crate) fn centre_at_x(ui: &mut SpuiState, w: WinId, x: f32) {
    let current = ui.rect(w).center().0;
    let mut area = ui.area(w);
    let delta = x - current;
    if delta.abs() < 0.01 {
        return;
    }
    area[0] += delta;
    area[2] += delta;
    ui.set_area(w, area);
}

/// The ancestor chain of a window contains a window with this control id.
pub(crate) fn within(ui: &SpuiState, mut w: WinId, ancestor: u32) -> bool {
    while let Some(parent) = ui.layout.window(w).parent {
        if ui.layout.window(parent).control_id == ancestor {
            return true;
        }
        w = parent;
    }
    false
}

/// ADAPTED: em size of the Sporebucks caption (the style says 14 pt).
const MONEY_TEXT_PX: f32 = 16.0;

fn bind_main(
    ui: &mut SpuiState,
    hud: &SpaceHudState,
    hud_ui: &SpaceHudUi,
    colors: &ProgressColors,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    // Sporebucks.
    if let Some(w) = window(ui, id::MONEY) {
        ui.set_text(w, format_money(hud.money));
        ui.set_text_size(w, Some(MONEY_TEXT_PX));
    }
    if let Some(w) = window(ui, id::PAUSE_TOGGLE) {
        ui.set_selected(w, hud.paused);
    }
    // Rank progress: the bar's container paints the ten segments (their own
    // windows are mostly the dots and ticks over its art), so one fill over
    // the container, clipped to the progress, recolours the art and the
    // segments' children alike. The unfilled part keeps the authored colour.
    if let Some(bar) = bar_container(ui) {
        let fill = bar_fill(hud.rank, hud.rank_fraction);
        ui.set_overlay_front(bar, true);
        ui.set_overlay(bar, (fill > 0.0).then_some((fill, colors.left)));
    }
    for (i, control) in id::PIPS.iter().enumerate() {
        if let Some(w) = window(ui, *control) {
            ui.set_frame(w, Some(u32::from(hud.rank as usize > i)));
        }
    }
    bind_core(ui, hud);
    // Health and energy.
    set_meter(ui, id::HEALTH_BAR, hud.health_fraction());
    set_meter(ui, id::ENERGY_BAR, hud.energy_fraction());
    if let Some(w) = window(ui, id::HEALTH_TEXT) {
        ui.set_text(w, (hud.health.max(0.0).ceil() as i32).to_string());
    }
    // The vitals rows are enabled while health / energy is at or under a fifth
    // of its maximum (0x10660AE: the row's flag 2 is the result of 0xC37420 /
    // 0xC37460, `value <= 0.2 x max`).
    for (control, value, max) in
        [(id::HEALTH_ROW, hud.health, hud.max_health), (id::ENERGY_ROW, hud.energy, hud.max_energy)]
    {
        if let Some(w) = window(ui, control) {
            ui.set_enabled(w, value <= max * LOW_MOTIVE);
        }
    }
    // Charging bar: shown while a tool recharges.
    set_visible(ui, id::CHARGING, hud.charging.is_some());
    if let Some(p) = hud.charging {
        set_meter(ui, id::CHARGING_FILL, charging_fill(p));
    }
    palette::bind_panels(ui, hud, hud_ui, assets, images, index);
    world::bind_world(ui, hud, hud_ui);
    world::bind_arrow(ui, hud, assets, images, index);
    // ADAPTED: parts of the retail layout whose behaviour is not documented or
    // that have no counterpart in the rewrite stay hidden: the pickup button of
    // the hologram scout, the posse list, the mission log container and the
    // upgrade button (the tool reward dialog is bound by `reward`).
    for control in [id::PICKUP_SCOUT, id::POSSE, id::MISSION_LOG, id::UPGRADE] {
        set_visible(ui, control, false);
    }
}

/// The container of the ten progress segments. The first segment's control id
/// is shared by this container (it spans the bar and holds the other nine)
/// and the segment inside it.
fn bar_container(ui: &SpuiState) -> Option<WinId> {
    ui.find_all(id::SEGMENTS[0])
        .into_iter()
        .find(|w| ui.layout.window(*w).children.iter().any(|c| ui.layout.window(*c).control_id == id::SEGMENTS[1]))
}

/// Health and energy fraction at or under which the vitals rows are enabled:
/// 0.2 (the constant at 0x146BB00, VERIFIED with the callers 0xC37420 /
/// 0xC37460 / 0xC38BFE).
pub const LOW_MOTIVE: f32 = 0.20;

/// The galactic core meter: marker position from the distance to the core.
fn bind_core(ui: &mut SpuiState, hud: &SpaceHudState) {
    let (Some(meter), Some(marker), Some(home), Some(fill)) =
        (window(ui, id::CORE_METER), window(ui, id::CORE_MARKER), window(ui, id::CORE_HOME), window(ui, id::CORE_FILL))
    else {
        return;
    };
    let Some(core) = hud.core else {
        ui.set_visible(meter, false);
        return;
    };
    ui.set_visible(meter, true);
    let bar = ui.rect(meter);
    if bar.width() < 1.0 {
        return;
    }
    // The home mark sits where the home star's distance puts the marker.
    let m = ((ui.rect(home).center().0 - bar.x1) / bar.width()).clamp(0.0, 1.0);
    let p = core_marker(core.distance, core.home, m);
    let x = bar.x1 + p * bar.width();
    centre_at_x(ui, marker, x);
    // The fill runs from the left end of the meter to the marker.
    let mut area = ui.layout.window(fill).area;
    let width = (x - ui.rect(fill).x1).max(0.0);
    area[2] = area[0] + width;
    ui.set_area(fill, area);
    ui.set_tooltip(marker, Some(format!("Angle: {:.0}\u{b0}, Distance: {:.0} (pc)", core.angle, core.distance)));
}

/// Push [`SpaceHudState`] into the layouts.
#[allow(clippy::too_many_arguments)]
fn bind(
    hud: Res<SpaceHudState>,
    hud_ui: Res<SpaceHudUi>,
    originals: Res<crate::Originals>,
    colors: Res<ProgressColors>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
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
            SpaceHudRoot::Main => bind_main(&mut ui, &hud, &hud_ui, &colors, &mut assets, &mut images, index),
            // The orb and its menu are the Cell stage's (the menu is shown
            // while the game is paused through it).
            SpaceHudRoot::Orb => crate::cell_hud::bind_orb(
                &mut ui,
                &crate::cell_contract::CellHudState { paused: hud.paused, ..default() },
            ),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Clicks.
// ---------------------------------------------------------------------------

/// Press a key for the frame; `player_hud::release` lets it go at the end of it.
pub(crate) fn press(keys: &mut ButtonInput<KeyCode>, injected: &mut crate::player_hud::InjectedKeys, key: KeyCode) {
    if !keys.pressed(key) {
        keys.press(key);
        injected.push(key);
    }
}

/// Zoom notches per second of a held zoom button (ADAPTED: the retail camera
/// buttons feed the UFO camera controller while held).
pub const HOLD_ZOOM_RATE: f32 = 2.0;

/// Button clicks become the keyboard actions the Space controls already use.
/// Runs before the simulation (`Controls`) on the clicks the SPUI input
/// recorded in the previous frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn actions(
    time: Res<Time>,
    hud: Res<SpaceHudState>,
    mut hud_ui: ResMut<SpaceHudUi>,
    mut roots: Query<(&mut SpuiState, &SpaceHudRoot)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<crate::player_hud::InjectedKeys>,
    mut wheel: MessageWriter<bevy::input::mouse::MouseWheel>,
    mut view: Option<ResMut<SpaceView>>,
    _ship: Query<&mut Health, With<Player>>,
    mut run: ResMut<RunProgress>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    mut comm_state: ResMut<comm::CommState>,
    mut rewards: ResMut<reward::RewardState>,
    mut front_menu: MessageWriter<crate::front_end::MenuCommand>,
    mut collections_state: ResMut<collections::CollectionsState>,
) {
    use crate::cell_hud::id as menu;
    let dt = time.delta_secs();
    for (mut ui, kind) in &mut roots {
        match kind {
            SpaceHudRoot::Main => {
                // The camera buttons are hold buttons: they act every frame
                // while pressed.
                for (control, notches, key) in [
                    (id::GALAXY_ZOOM_IN, 1.0, None),
                    (id::PLANET_ZOOM_IN, 1.0, None),
                    (id::GALAXY_ZOOM_OUT, -1.0, None),
                    (id::PLANET_ZOOM_OUT, -1.0, None),
                    (id::GALAXY_ROTATE_LEFT, 0.0, Some(KeyCode::Comma)),
                    (id::PLANET_ROTATE_LEFT, 0.0, Some(KeyCode::Comma)),
                    (id::GALAXY_ROTATE_RIGHT, 0.0, Some(KeyCode::Period)),
                    (id::PLANET_ROTATE_RIGHT, 0.0, Some(KeyCode::Period)),
                ] {
                    let Some(w) = ui.find(control) else {
                        continue;
                    };
                    if ui.states[w] & crate::spui::ids::state::CLICKED == 0 || !ui.visible(w) {
                        continue;
                    }
                    if notches != 0.0 {
                        wheel.write(bevy::input::mouse::MouseWheel {
                            unit: bevy::input::mouse::MouseScrollUnit::Line,
                            x: 0.0,
                            y: notches * HOLD_ZOOM_RATE * dt,
                            window: Entity::PLACEHOLDER,
                            phase: bevy::input::touch::TouchPhase::Moved,
                        });
                    }
                    if let Some(key) = key {
                        press(&mut keys, &mut injected, key);
                    }
                }
                for click in ui.take_clicks() {
                    let control = ui.layout.window(click).control_id;
                    // My Collections is the Space collections window (0xE190A0).
                    if control == id::COLLECTIONS {
                        collections_state.open = !collections_state.open;
                        continue;
                    }
                    // Sporepedia and the options menu are the front end's.
                    if let Some(command) = crate::front_end::menu_command(control) {
                        front_menu.write(command);
                        continue;
                    }
                    match control {
                        id::PAUSE_TOGGLE => pause.open = !pause.open,
                        id::COMM_BUTTON => comm_state.open = !comm_state.open,
                        reward::rid::CLOSE => rewards.dismiss(),
                        id::CAPTURE_BUTTON | id::PURCHASE_BUTTON => {
                            // The capture and the buyout of a system are the
                            // colonise action on its worlds.
                            press(&mut keys, &mut injected, KeyCode::KeyC);
                        }
                        c if id::FILTER_BUTTONS.contains(&c) => {
                            if let Some(view) = view.as_deref_mut() {
                                world::set_filter(view, c);
                            }
                        }
                        id::HISTORY => {
                            // No counterpart in the rewrite yet.
                        }
                        _ => {
                            palette::click(&mut hud_ui, control);
                            world::click(&mut hud_ui, control);
                        }
                    }
                }
            }
            SpaceHudRoot::Tool(slot) => {
                let Some(tool) =
                    hud_ui.slot_tools.get(*slot as usize).copied().flatten().and_then(|i| hud.tools.get(i))
                else {
                    ui.take_clicks();
                    continue;
                };
                // Held tools (the laser) fire every frame while the button is down.
                if tool.def.hold {
                    if let (Some(w), Some(key)) = (ui.layout.roots.first().copied(), tool.def.key) {
                        if ui.states[w] & crate::spui::ids::state::CLICKED != 0 {
                            press(&mut keys, &mut injected, key);
                        }
                    }
                }
                for _ in ui.take_clicks() {
                    palette::select(tool, &mut run, &mut keys, &mut injected);
                }
            }
            SpaceHudRoot::Collections => {
                for click in ui.take_clicks() {
                    let control = ui.layout.window(click).control_id;
                    collections_state.click(control);
                }
            }
            SpaceHudRoot::Category(row) => {
                if !ui.take_clicks().is_empty() {
                    collections_state.select_row(*row as usize);
                }
            }
            SpaceHudRoot::Comm => {
                // The comm session (`comm::drive_session`) runs the clicks.
                for click in ui.take_clicks() {
                    let control = ui.layout.window(click).control_id;
                    comm_state.clicks.push(control);
                }
            }
            SpaceHudRoot::Orb => {
                for click in ui.take_clicks() {
                    let control = ui.layout.window(click).control_id;
                    if let Some(command) = crate::front_end::menu_command(control) {
                        front_menu.write(command);
                    } else if control == menu::OPTIONS_ORB {
                        pause.open = !pause.open;
                    }
                }
            }
            // Cards, dialogs and the communication screen take their own clicks.
            _ => {}
        }
    }
}

/// Rectangle of a window of the main layout (callout targets), if it is shown.
pub(crate) fn main_rect(roots: &Query<(&mut SpuiState, &SpaceHudRoot)>, control: u32) -> Option<Rect> {
    roots.iter().find_map(|(ui, kind)| {
        if !ui.ready || !matches!(kind, SpaceHudRoot::Main | SpaceHudRoot::Card(0)) {
            return None;
        }
        // Where a control id is shared by a container and the window inside it
        // (the progress bar's first segment), the inner window is the target.
        let all = ui.find_all(control);
        let inner = all
            .iter()
            .copied()
            .find(|w| ui.layout.window(*w).parent.is_some_and(|p| ui.layout.window(p).control_id == control));
        inner.or_else(|| all.first().copied()).filter(|w| ui.visible(*w)).map(|w| ui.rect(w))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ten_segments_fill_one_after_the_other() {
        assert_eq!(bar_fill(0, 0.0), 0.0);
        assert_eq!(bar_fill(0, 0.5), 0.05);
        assert!((bar_fill(3, 0.5) - 0.35).abs() < 1e-6);
        assert_eq!(bar_fill(10, 1.0), 1.0);
        assert_eq!(bar_fill(9, 5.0), 1.0, "the fraction is clamped");
    }

    #[test]
    fn money_has_thousands_separators() {
        assert_eq!(format_money(0), "$0");
        assert_eq!(format_money(999), "$999");
        assert_eq!(format_money(1_000), "$1,000");
        assert_eq!(format_money(100_000), "$100,000");
        assert_eq!(format_money(10_000_000), "$10,000,000");
        assert_eq!(format_money(-12_345), "-$12,345");
    }

    #[test]
    fn the_core_marker_follows_the_retail_formula() {
        let m = 0.1;
        // At the home star's distance the marker is on the home mark.
        assert!((core_marker(7400.0, 7400.0, m) - m).abs() < 1e-6);
        // At the core it is at the far end; at the rim at the start.
        assert_eq!(core_marker(0.0, 7400.0, m), 1.0);
        assert_eq!(core_marker(CORE_RIM_PC, 7400.0, m), 0.0);
        // Inside the home distance it moves linearly from the mark to the end.
        let half = core_marker(3700.0, 7400.0, m);
        assert!((half - (0.5 * 0.9 + 0.1)).abs() < 1e-6);
        // Outside it moves from the mark towards the start.
        let out = core_marker(8700.0, 7400.0, m);
        assert!(out < m && out > 0.0);
        // Nonsense input stays on the meter.
        assert_eq!(core_marker(-5.0, 100.0, 0.1), 1.0);
        assert_eq!(core_marker(1e9, 100.0, 0.1), 0.0);
    }

    #[test]
    fn the_charging_bar_never_shrinks_below_fifteen_percent() {
        assert_eq!(charging_fill(0.0), 0.15);
        assert_eq!(charging_fill(0.4), 0.4);
        assert_eq!(charging_fill(3.0), 1.0);
    }

    #[test]
    fn the_bullseye_marker_uses_temperature_and_inverted_atmosphere() {
        assert_eq!(bullseye_position(0.5, 0.5), (0.5, 0.5));
        assert_eq!(bullseye_position(0.0, 1.0), (0.0, 0.0));
        assert_eq!(bullseye_position(1.0, 0.0), (1.0, 1.0));
        assert_eq!(bullseye_position(2.0, -1.0), (1.0, 1.0));
    }

    #[test]
    fn the_galaxy_radius_maps_to_the_core_meters_rim() {
        assert_eq!(to_core_parsecs(crate::space_galaxy::GALAXY_RADIUS), CORE_RIM_PC);
        assert_eq!(to_core_parsecs(0.0), 0.0);
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
    fn the_hud_state_defaults_to_the_solar_system() {
        let hud = SpaceHudState::default();
        assert_eq!(hud.zoom, HudZoom::System);
        assert!(hud.callouts.is_empty() && hud.hint.is_none() && hud.tools.is_empty());
        assert_eq!(hud.health_fraction(), 1.0, "no maximum yet: full");
    }

    fn demo_hud() -> SpaceHudState {
        let mut campaign = SpaceCampaign::default();
        campaign.archetype = crate::space::SpaceArchetype::Trader;
        SpaceHudState {
            zoom: HudZoom::Planet,
            money: 120_450,
            badge_points: 58,
            rank: 4,
            rank_fraction: 0.32,
            health: 640.0,
            max_health: 1000.0,
            energy: 20.0,
            max_energy: 100.0,
            charging: Some(0.4),
            core: Some(CoreMeter { distance: 6200.0, home: 7400.0, angle: 112.0 }),
            tools: palette::build_tools(&campaign, 1e6),
            planet: Some(PlanetPanel {
                world: 3,
                name: "Nyxor III".into(),
                temperature: 0.46,
                atmosphere: 0.58,
                velocity: (0.0, 0.0),
                t_score: 2,
                flora: 5,
                herbivores: 3,
                carnivores: 1,
                colonies: 1,
                colony_limit: 2,
                empire: None,
            }),
            comm: Some(comm::CommTarget {
                empire: 2,
                name: "Zelem".into(),
                relation: "Neutral".into(),
                greeting: "Hello".into(),
                incoming: true,
                contract: None,
                owner_cost_index: 4,
                ..Default::default()
            }),
            capture: Some(CapturePanel {
                empire: "Zelem".into(),
                planets: vec![true, false],
                can_capture: false,
                can_purchase: true,
            }),
            ..default()
        }
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
        let layout = assets.layout(&index, MAIN_LAYOUT).expect("GlobalUISpace2-1");
        let single = [
            id::BAR,
            id::MONEY,
            id::MONEY_PANEL,
            id::HISTORY,
            id::PAUSE_TOGGLE,
            id::SPOREPEDIA,
            id::COLLECTIONS,
            id::CORE_METER,
            id::CORE_MARKER,
            id::CORE_HOME,
            id::CORE_FILL,
            id::VITALS,
            id::HEALTH_ROW,
            id::HEALTH_TEXT,
            id::HEALTH_BAR,
            id::ENERGY_ROW,
            id::ENERGY_BAR,
            id::CHARGING,
            id::CHARGING_FILL,
            id::UPGRADE,
            id::PICKUP_SCOUT,
            id::POSSE,
            id::SHIP_PANEL,
            id::SPECIES_NAME,
            id::HAPPINESS,
            id::PORTRAIT,
            id::COMM_FRAME,
            id::COMM_GLOW,
            id::COMM_BUTTON,
            id::FILTERS,
            id::GALAXY_CAMERA,
            id::GALAXY_ZOOM_IN,
            id::GALAXY_ZOOM_OUT,
            id::GALAXY_ROTATE_LEFT,
            id::GALAXY_ROTATE_RIGHT,
            id::PLANET_ZOOM_IN,
            id::PLANET_ZOOM_OUT,
            id::PLANET_ROTATE_LEFT,
            id::PLANET_ROTATE_RIGHT,
            id::CAPTURE_PANEL,
            id::CAPTURE_BUTTON,
            id::PURCHASE_BUTTON,
            id::CAPTURE_LIST,
            id::MISSION_LOG,
            id::REWARD,
            palette::pid::PALETTE,
            palette::pid::HIDE,
            palette::pid::JETTISON,
            world::pid::PANEL,
            world::pid::NAME,
            world::pid::HIDE,
            world::pid::MODE_TOGGLE,
            world::pid::MINIMAP_HOLDER,
            world::pid::TERRAFORM,
            world::pid::COLONY_COUNT,
            world::pid::BULLSEYE,
            world::pid::MARKER,
        ];
        for control in single
            .iter()
            .chain(&id::SEGMENTS)
            .chain(&id::PIPS)
            .chain(&id::FILTER_BUTTONS)
            .chain(&id::CAPTURE_TEXT)
            .chain(&id::CAPTURE_PLANETS)
            .chain(&world::pid::T_BUTTONS)
            .chain(&world::pid::ROW_LOCKS)
            .chain(&world::pid::ROW_HOVER)
            .chain(&world::pid::COLONY_SLOTS)
            .chain(world::pid::ROW_SLOTS.iter().flatten())
        {
            assert!(layout.find(*control).is_some(), "window {control:08X}");
        }
        for tab in palette::Tab::ALL {
            assert!(layout.find(tab.tab_control()).is_some(), "tab {tab:?}");
            assert!(layout.find(tab.panel_control()).is_some(), "panel {tab:?}");
            for row in tab.rows().into_iter().filter(|r| *r != 0) {
                assert!(layout.find(row).is_some(), "row {row:08X} of {tab:?}");
                // Each row holds a slot frame and the two page arrows.
                let container = layout.find(row).unwrap();
                let kids: Vec<u32> =
                    layout.window(container).children.iter().map(|c| layout.window(*c).control_id).collect();
                assert!(kids.contains(&palette::pid::SLOT_FRAME), "{row:08X} children {kids:08X?}");
                assert!(kids.contains(&palette::pid::PAGE_PREV) && kids.contains(&palette::pid::PAGE_NEXT));
            }
        }
        // The first segment's id is shared by the container of the ten and the segment inside it.
        assert_eq!(layout.find_all(id::SEGMENTS[0]).len(), 2);
        for control in &id::SEGMENTS[1..] {
            assert_eq!(layout.find_all(*control).len(), 1, "{control:08X}");
        }
        // The bar picture id is shared by the health and the energy bar.
        assert_eq!(layout.find_all(0x0665_FE90).len(), 2);
    }

    #[test]
    fn installed_satellite_layouts_have_the_windows_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let tool = assets.layout(&index, palette::TOOL_LAYOUT).expect("SpaceMainTool");
        for control in [palette::pid::TOOL_COUNT, palette::pid::TOOL_KEY] {
            assert!(tool.find(control).is_some(), "tool window {control:08X}");
        }
        assert_eq!(tool.roots.len(), 1, "one root button per tool");
        let cards = assets.layout(&index, CARD_LAYOUT).expect("SPGMissionCard");
        for control in [
            guide::card::ROOT,
            guide::card::KIND,
            guide::card::COUNT,
            guide::card::COUNT_BACKDROP,
            guide::card::HEADLINE,
            guide::card::DESCRIPTION,
            guide::card::PANEL,
            guide::card::GROUP,
        ] {
            assert!(cards.find(control).is_some(), "card window {control:08X}");
        }
        let screen = assets.layout(&index, comm::COMM_LAYOUT).expect("CommScreen-3");
        for control in [
            comm::cid::SCREEN,
            comm::cid::BOTTOM,
            comm::cid::SPEAKER,
            comm::cid::GOODBYE,
            comm::cid::ACCEPT,
            comm::cid::DECLINE,
            comm::cid::TABS,
            comm::cid::TAB_TRADE,
            comm::cid::TAB_REPAIR,
            comm::cid::TAB_RECHARGE,
            comm::cid::TAB_MISSIONS,
            comm::cid::TAB_DIPLOMACY,
            comm::cid::TRADING,
            comm::cid::NATION_NAME,
            comm::cid::GREETING,
            comm::cid::PORTRAIT_FILL,
        ] {
            assert!(screen.find(control).is_some(), "comm window {control:08X}");
        }
        assert!(assets.layout(&index, crate::cell_hud::dialogs::HINT_LAYOUT).is_some());
        assert!(assets.layout(&index, crate::cell_hud::dialogs::CALLOUT_LAYOUT).is_some());
    }

    #[test]
    fn installed_tools_have_names_icons_and_descriptions() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let mut ids: Vec<&str> = palette::TOOLS.iter().map(|t| t.id).collect();
        for archetype in crate::space::SpaceArchetype::ALL {
            if let Some(def) = palette::superpower_tool(archetype) {
                ids.push(def.id);
            }
        }
        for name in ids {
            let info = palette::read_tool(&index, &mut assets, name).unwrap_or_else(|| panic!("tool {name}"));
            assert!(!info.name.is_empty(), "{name} has a name");
            assert!(info.icon.is_some(), "{name} has an icon");
        }
        // The tab and row of every palette tool is the retail panel of its property file.
        let mut defs: Vec<palette::ToolDef> = palette::TOOLS.to_vec();
        defs.extend(crate::space::SpaceArchetype::ALL.iter().filter_map(|a| palette::superpower_tool(*a)));
        for def in defs {
            let info = palette::read_tool(&index, &mut assets, def.id).unwrap();
            let placement = info.panel.and_then(palette::panel_of);
            assert_eq!(placement, Some((def.tab, def.row)), "{} sits in {:?}", def.id, placement);
        }
        let laser = palette::read_tool(&index, &mut assets, "laserlevel1").unwrap();
        assert_eq!(laser.name, "Mini Laser");
        assert!(laser.description.starts_with("Use on ground targets"));
    }

    #[test]
    fn installed_space_callouts_and_hints_decode_and_point_at_existing_windows() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let main = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let cards = assets.layout(&index, CARD_LAYOUT).unwrap();
        for name in guide::ALL_CALLOUTS {
            let def =
                crate::cell_hud::dialogs::read_callout(&index, &mut assets, name).unwrap_or_else(|| panic!("{name}"));
            assert!(!def.title.is_empty() || !def.text.is_empty(), "{name} has text");
            for target in &def.targets {
                let target = guide::callout_target(*target);
                assert!(main.find(target).is_some() || cards.find(target).is_some(), "{name} points at {target:08X}");
            }
        }
        for hint in ["spg-fly", "spg-zoomsolar", "spg-zoomgalaxy", "spg-opencommfirsttime"] {
            let def =
                crate::cell_hud::dialogs::read_hint(&index, &mut assets, hint).unwrap_or_else(|| panic!("{hint}"));
            assert!(!def.before.is_empty() || !def.after.is_empty(), "{hint} has text");
        }
        let fly = crate::cell_hud::dialogs::read_hint(&index, &mut assets, "spg-fly").unwrap();
        assert!(fly.before.contains("fly your spaceship"));
    }

    #[test]
    fn the_main_layout_shows_the_published_state() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let layout = assets.layout(&index, MAIN_LAYOUT).unwrap();
        let mut images = Assets::<Image>::default();
        let mut ui = SpuiState::new(layout, RootPolicy::Fit);
        let hud = demo_hud();
        let hud_ui = SpaceHudUi::default();
        let colors = ProgressColors::default();
        bind_main(&mut ui, &hud, &hud_ui, &colors, &mut assets, &mut images, &index);
        let w = |ui: &SpuiState, control: u32| ui.find(control).unwrap();
        // Sporebucks, health caption and meters.
        assert_eq!(ui.captions[w(&ui, id::MONEY)].as_deref(), Some("$120,450"));
        assert_eq!(ui.captions[w(&ui, id::HEALTH_TEXT)].as_deref(), Some("640"));
        let bar = ui.area(w(&ui, id::HEALTH_BAR));
        let authored = ui.layout.window(w(&ui, id::HEALTH_BAR)).area;
        assert!(((bar[2] - bar[0]) - (authored[2] - authored[0]) * 0.64).abs() < 0.01, "{bar:?}");
        let energy = ui.area(w(&ui, id::ENERGY_BAR));
        assert!(((energy[2] - energy[0]) - (authored[2] - authored[0]) * 0.2).abs() < 0.5, "{energy:?}");
        assert!(ui.flags[w(&ui, id::HEALTH_ROW)] & crate::spui::ids::flag::ENABLED == 0, "health is fine");
        assert!(ui.flags[w(&ui, id::ENERGY_ROW)] & crate::spui::ids::flag::ENABLED != 0, "energy is low");
        // Rank: four pips lit, the segments fill one after the other.
        for (i, pip) in id::PIPS.iter().enumerate() {
            assert_eq!(ui.frame_overrides[w(&ui, *pip)], Some(u32::from(i < 4)), "pip {i}");
        }
        let bar = bar_container(&ui).expect("the container of the ten segments");
        let fill = ui.overlays[bar].map(|o| o.0).expect("the progress fill");
        assert!((fill - 0.432).abs() < 1e-4, "four ranks and a third of the fifth: {fill}");
        assert!(ui.overlays[w(&ui, id::SEGMENTS[9])].is_none(), "the fill is the container's");
        assert_eq!(ui.layout.window(bar).children.len() >= 10, true, "it holds the ten segments");
        // Charging bar shown with at least the minimum fill.
        assert!(ui.visible(w(&ui, id::CHARGING)));
        // Palette: the Main tab's panel shows, the others do not.
        for tab in palette::Tab::ALL {
            assert_eq!(ui.visible(w(&ui, tab.panel_control())), tab == palette::Tab::Main, "{tab:?}");
            let selected = ui.states[w(&ui, tab.tab_control())] & crate::spui::ids::state::SELECTED != 0;
            assert_eq!(selected, tab == palette::Tab::Main, "{tab:?}");
        }
        // Planet context: the planet panel, species panel and capture panel show.
        assert!(
            ui.visible(w(&ui, world::pid::PANEL))
                && ui.visible(w(&ui, id::SHIP_PANEL))
                && ui.visible(w(&ui, id::CAPTURE_PANEL))
        );
        assert!(!ui.visible(w(&ui, id::FILTERS)), "no starmap cluster in the planet view");
        assert_eq!(ui.captions[w(&ui, world::pid::NAME)].as_deref(), Some("Nyxor III"));
        assert_eq!(ui.captions[w(&ui, world::pid::COLONY_COUNT)].as_deref(), Some("1/2"));
        assert_eq!(ui.captions[w(&ui, id::SPECIES_NAME)].as_deref(), Some("Zelem"));
        assert!(ui.visible(w(&ui, id::COMM_GLOW)), "an incoming message lights the glow");
        // T1 and T2 lit, T3 not; the T3 row is locked, the others are not.
        let lit = |ui: &SpuiState, c: u32| ui.states[w(ui, c)] & crate::spui::ids::state::SELECTED != 0;
        assert!(
            lit(&ui, world::pid::T_BUTTONS[0])
                && lit(&ui, world::pid::T_BUTTONS[1])
                && !lit(&ui, world::pid::T_BUTTONS[2])
        );
        assert!(!ui.visible(w(&ui, world::pid::ROW_LOCKS[0])) && !ui.visible(w(&ui, world::pid::ROW_LOCKS[1])));
        assert!(ui.visible(w(&ui, world::pid::ROW_LOCKS[2])));
        // Food web: the first row of a five-plant web is full of plants.
        let filled = |ui: &SpuiState, c: u32| ui.image_overrides[w(ui, c)].is_some();
        assert!(world::pid::ROW_SLOTS[0][..3].iter().all(|c| filled(&ui, *c)));
        assert!(
            filled(&ui, world::pid::ROW_SLOTS[1][0])
                && filled(&ui, world::pid::ROW_SLOTS[1][1])
                && !filled(&ui, world::pid::ROW_SLOTS[1][2])
        );
        assert!(filled(&ui, world::pid::COLONY_SLOTS[0]) && !filled(&ui, world::pid::COLONY_SLOTS[1]));
        assert!(
            ui.flags[w(&ui, world::pid::COLONY_SLOTS[2])] & crate::spui::ids::flag::ENABLED == 0,
            "beyond the colony limit"
        );
        // Capture: purchase shows (a trade route matured), capture does not.
        assert!(ui.visible(w(&ui, id::PURCHASE_BUTTON)) && !ui.visible(w(&ui, id::CAPTURE_BUTTON)));
        assert!(ui.visible(w(&ui, id::CAPTURE_PLANETS[1])) && !ui.visible(w(&ui, id::CAPTURE_PLANETS[2])));
        assert_eq!(ui.frame_overrides[w(&ui, id::CAPTURE_PLANETS[0])], Some(1));
        // The starmap view: the cluster with the six toggles replaces the planet panel.
        let mut galaxy = demo_hud();
        galaxy.zoom = HudZoom::Galaxy;
        galaxy.filter = GalaxyFilter::Visited;
        bind_main(&mut ui, &galaxy, &hud_ui, &colors, &mut assets, &mut images, &index);
        assert!(
            ui.visible(w(&ui, id::FILTERS))
                && !ui.visible(w(&ui, world::pid::PANEL))
                && !ui.visible(w(&ui, id::SHIP_PANEL))
        );
        assert!(!ui.visible(w(&ui, id::CAPTURE_PANEL)), "no capture panel in the galaxy view");
        for control in id::FILTER_BUTTONS {
            assert!(ui.visible(w(&ui, control)));
            assert_eq!(
                ui.states[w(&ui, control)] & crate::spui::ids::state::SELECTED != 0,
                control == id::FILTER_VISITED
            );
        }
        // The system view has neither the planet panel nor the starmap cluster.
        let mut system = demo_hud();
        system.zoom = HudZoom::System;
        bind_main(&mut ui, &system, &hud_ui, &colors, &mut assets, &mut images, &index);
        assert!(!ui.visible(w(&ui, id::FILTERS)) && !ui.visible(w(&ui, world::pid::PANEL)));
        // A folded palette hides every panel.
        let folded = SpaceHudUi { palette_hidden: true, ..default() };
        bind_main(&mut ui, &hud, &folded, &colors, &mut assets, &mut images, &index);
        assert!(palette::Tab::ALL.iter().all(|t| !ui.visible(w(&ui, t.panel_control()))));
    }

    fn space_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Space);
        app.update();
        app.update();
        app
    }

    #[test]
    fn the_published_state_follows_the_ship_and_the_campaign() {
        let mut app = space_app();
        app.update();
        let hud = app.world().resource::<SpaceHudState>();
        assert!(hud.max_health > 0.0 && hud.health <= hud.max_health, "health {}/{}", hud.health, hud.max_health);
        assert!(hud.max_energy > 0.0 && hud.energy <= hud.max_energy);
        assert_eq!(hud.rank, 0);
        assert_eq!(hud.zoom, HudZoom::System);
        assert!(hud.core.is_some_and(|c| c.home > 0.0 && c.distance > 0.0), "the galaxy gives the core meter");
        assert!(hud.tools.iter().any(|t| t.def.id == "scan"));
        assert!(hud.planet.is_some(), "the ship is over some world");
        assert_eq!(hud.goal_cards.len(), 0, "the HUD is not active in a headless app: the guide waits");
        // The camera: the galaxy view changes the context; a pause is published.
        app.world_mut().resource_mut::<SpaceView>().galaxy = true;
        app.world_mut().resource_mut::<crate::pause_menu::PauseMenu>().open = true;
        app.update();
        let hud = app.world().resource::<SpaceHudState>();
        assert_eq!(hud.zoom, HudZoom::Galaxy);
        assert!(hud.paused);
        assert!(hud.galaxy_caption.is_some(), "the galaxy map names the selected star");
    }

    #[test]
    fn badge_points_become_ranks_in_the_published_state() {
        let mut run = RunProgress::default();
        let mut hud = SpaceHudState::default();
        snapshot_ship(&run, None, &mut hud);
        assert_eq!((hud.rank, hud.rank_fraction), (0, run.space_campaign.rank_progress()));
        run.space_campaign.visited = (0..70).collect();
        run.space_campaign.colonies = (1..=25).collect();
        snapshot_ship(&run, None, &mut hud);
        assert_eq!(hud.badge_points, run.space_campaign.total_badge_points());
        assert!(hud.badge_points >= 10);
        assert_eq!(hud.rank, run.space_campaign.captain_rank());
        assert_eq!(hud.rank, crate::space_rules::rank_from_points(hud.badge_points));
        assert_eq!(hud.rank_fraction, crate::space_rules::rank_progress(hud.badge_points));
        assert!(hud.rank >= 1);
        // The wallet is the Sporebucks of the run; the hull maximum falls back to the rules.
        run.spice = 123_456.4;
        snapshot_ship(&run, None, &mut hud);
        assert_eq!(hud.money, 123_456);
        assert_eq!(hud.max_health, run.space_campaign.max_hull());
        // The archetype superpower recharging is the charging bar.
        run.space_campaign.archetype = crate::space::SpaceArchetype::Bard;
        run.space_campaign.superweapon_cooldown = run.space_campaign.archetype.cooldown_secs() * 0.25;
        snapshot_ship(&run, Some(&Health::new(100.0)), &mut hud);
        assert!((hud.charging.unwrap() - 0.75).abs() < 1e-5);
        assert_eq!((hud.health, hud.max_health), (100.0, 100.0));
        // No superpower (Wanderer): no charging bar.
        run.space_campaign.archetype = crate::space::SpaceArchetype::Wanderer;
        snapshot_ship(&run, None, &mut hud);
        assert!(hud.charging.is_none());
    }

    /// A root with an installed layout, to click on.
    fn with_layout(app: &mut App, name: &str, kind: SpaceHudRoot, policy: RootPolicy) -> Option<Entity> {
        let (index, mut assets) = installed()?;
        let layout = assets.layout(&index, name)?;
        Some(app.world_mut().spawn((kind, SpuiState::new(layout, policy))).id())
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

    /// Keys the simulation sees as just pressed.
    #[derive(Resource, Default)]
    struct SeenKeys(Vec<KeyCode>);

    fn watch_keys(app: &mut App) {
        app.init_resource::<SeenKeys>().add_systems(
            Update,
            (|keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<SeenKeys>| {
                for key in [KeyCode::KeyC, KeyCode::KeyV, KeyCode::KeyU, KeyCode::KeyK, KeyCode::KeyB] {
                    if keys.just_pressed(key) {
                        seen.0.push(key);
                    }
                }
            })
            .in_set(crate::GameSystems::Simulation),
        );
    }

    #[test]
    fn clicks_change_the_filter_the_pause_state_and_the_palette() {
        let mut app = space_app();
        let Some(root) = with_layout(&mut app, MAIN_LAYOUT, SpaceHudRoot::Main, RootPolicy::Fit) else {
            return;
        };
        click(&mut app, root, id::FILTER_VISITED);
        assert_eq!(app.world().resource::<SpaceView>().filter, GalaxyFilter::Visited);
        click(&mut app, root, id::FILTER_VISITED);
        assert_eq!(app.world().resource::<SpaceView>().filter, GalaxyFilter::All);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(app.world().resource::<crate::pause_menu::PauseMenu>().open);
        click(&mut app, root, id::PAUSE_TOGGLE);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        // Tabs, the Hide button and the planet panel's own toggles.
        click(&mut app, root, palette::Tab::Weapons.tab_control());
        assert_eq!(app.world().resource::<SpaceHudUi>().tab, palette::Tab::Weapons);
        click(&mut app, root, palette::pid::HIDE);
        assert!(app.world().resource::<SpaceHudUi>().palette_hidden);
        click(&mut app, root, world::pid::MODE_TOGGLE);
        assert!(app.world().resource::<SpaceHudUi>().minimap);
        click(&mut app, root, world::pid::HIDE);
        assert!(app.world().resource::<SpaceHudUi>().planet_hidden);
        // The communication button opens the screen only where there is somebody to talk to.
        click(&mut app, root, id::COMM_BUTTON);
        assert!(!app.world().resource::<comm::CommState>().open, "nobody answers over open space");
    }

    #[test]
    fn capture_buttons_and_tool_buttons_reach_the_simulation_as_key_presses() {
        let mut app = space_app();
        watch_keys(&mut app);
        let Some(root) = with_layout(&mut app, MAIN_LAYOUT, SpaceHudRoot::Main, RootPolicy::Fit) else {
            return;
        };
        click(&mut app, root, id::CAPTURE_BUTTON);
        click(&mut app, root, id::PURCHASE_BUTTON);
        assert_eq!(app.world().resource::<SeenKeys>().0, [KeyCode::KeyC, KeyCode::KeyC]);
        // A palette tool: the scan button presses V.
        let Some(tool_root) = with_layout(&mut app, palette::TOOL_LAYOUT, SpaceHudRoot::Tool(0), RootPolicy::Authored)
        else {
            return;
        };
        app.update();
        let scan = app.world().resource::<SpaceHudState>().tools.iter().position(|t| t.def.id == "scan").unwrap();
        app.world_mut().resource_mut::<SpaceHudUi>().slot_tools = vec![Some(scan)];
        let button = app.world().get::<SpuiState>(tool_root).unwrap().layout.roots[0];
        app.world_mut().get_mut::<SpuiState>(tool_root).unwrap().clicks.push(button);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        assert_eq!(app.world().resource::<SeenKeys>().0.last(), Some(&KeyCode::KeyV));
        // Selecting a planetary tool is the one write into Space state.
        let cool = app.world().resource::<SpaceHudState>().tools.iter().position(|t| t.def.id == "coldray").unwrap();
        app.world_mut().resource_mut::<SpaceHudUi>().slot_tools = vec![Some(cool)];
        app.world_mut().get_mut::<SpuiState>(tool_root).unwrap().clicks.push(button);
        app.update();
        assert_eq!(
            app.world().resource::<RunProgress>().space_campaign.planetary_tool,
            crate::space::PlanetaryTool::Cool
        );
    }

    #[test]
    fn the_legacy_console_steps_aside_while_the_hud_is_up() {
        let mut app = space_app();
        app.world_mut().insert_resource(SpaceHudActive(true));
        app.update();
        let world = app.world_mut();
        let hidden = world
            .query_filtered::<&Visibility, With<crate::space::SpaceConsoleRoot>>()
            .iter(world)
            .all(|v| *v == Visibility::Hidden);
        assert!(hidden, "the terraforming console is replaced by the planet panel");
    }
}
