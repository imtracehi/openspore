//! The retail City Planner screen of the Civilization stage, opened from the
//! city card's City Planner button (`RolloverCivCity` 062D5618).
//!
//! RETAIL (installed data, `notes\civ-planner-re.md` P1-P3):
//! * `CityEditorUI` (layouts_atlas~ 0x1B52ED1D): the planner's frame: the city
//!   name banner (053D5A68), Change Cities with Previous / Next City
//!   (74BBED3E / 74BBED3D), Save and Exit (1036A039), the Spore Guide button,
//!   and the City Legend (065F6003): "Blue links increase the city's income.
//!   Green links increase the city's happiness, red links decrease
//!   happiness, white links are neutral." (the Colony Legend 065F6004 and the
//!   tribe colour picker 054ACA40 belong to the Space / Tribe planners).
//! * `CityPlannerCityStats` (0x8CCDFCFA): name, nation face, specialty icon,
//!   population, five happiness orbs, five defence orbs, income per minute.
//! * The link rule is the city model of `cCity::ProcessBuildingUpdate`
//!   (0xBE2590, `civ_rules::evaluate_city`): a factory linked to a house or
//!   the hall earns 400 (blue), an entertainment building linked to a house
//!   or the hall makes people happy (green), a factory linked to an
//!   entertainment building unhappy (red); buildings count only when linked
//!   to the hall.
//! * Prices `CityGameBuildingTuning`: house 1600, factory 1200, entertainment
//!   800, turret 2000; selling refunds 100 / 75 / 50 % by difficulty
//!   (0xCF3350).
//!
//! * The palette (`notes\civ-final-re.md` P2, installed planner palette
//!   pages / categories / items, groups 0x406B6C64 / 0x406B6E64 /
//!   0x406B6D64): five categories by `paletteCategorySequenceNumber`:
//!   Buildings 100 (`buttons_city_buildings~`: City Hall, House,
//!   Entertainment, Factory, the city type's turret), the city type's
//!   vehicles 200 (`buttons_city_vehicle~` Land 1000 / Sea 1500 / Air 3000 and
//!   the Spaceship, with their lock texts), Decorations 300 (`civ_decor`),
//!   Music 500 (the anthem editor, `civ_anthem`) and Outfits 1000
//!   (`OutfitData!CityOutfit`: the citizens' outfitter
//!   `CivAccessoriesEditorSetup`). A vehicle swatch's New / Edit opens the
//!   vehicle editor of that class; a building swatch's Edit opens its
//!   building editor (`paletteAssetSwatchEditable`, the turret has none).
//!
//! * The palette's art (`art`): the category tabs `CIP_butn-section-*`, the
//!   swatches' backgrounds, header icons, New icons and lock icons named by
//!   the item PROPs, and the civic objects' model renders.
//!
//! * A swatch of a class that was designed shows the design's thumbnail
//!   (`art::design_thumbnails`), as retail's `paletteAssetSwatch` does.
//!
//! ADAPTED: the palette is Bevy UI in the installed category names, order and
//! art (the palette panel's own frame is not drawn; a swatch shows its New
//! icon until its class was designed);
//! which of the three vehicle categories (all sequence 200) shows is the
//! planned city's type (INFERRED); the slots are the walls model's authored
//! ones (RETAIL, `city_layout`: the hall + 5 or 11 building slots, 20 or 30
//! decoration spots, by the city's walls);
//! the camera is the Civ camera moved over the city (retail runs the script
//! `city_large_planner_camera`, whose zoom ladder and rotate rates are not
//! swapped in; see [`PLANNER_DISTANCE`]); the stats panel shows the city as it would
//! be with the armed building on the hovered slot (preview); an editor
//! opened from the palette closes the planner; the game keeps running.

use bevy::prelude::*;
use bevy::{ui::Pressed, ui_widgets::Button};

use crate::civ::{self, Approach, Building, CityState, VehicleDomain};
use crate::civ_planet::CITY_SCALE;
use crate::civ_rules::{self, CityBuilding};
use crate::game::RunProgress;
use crate::spui::{
    solve::RootPolicy,
    view::{self, SpuiAssets, SpuiState},
};
use crate::stages::{GameStage, StageOwned};

pub mod art;

pub const FRAME_LAYOUT: &str = "CityEditorUI";
pub const STATS_LAYOUT: &str = "CityPlannerCityStats";

/// Control ids of the two layouts.
#[allow(dead_code)]
pub mod id {
    // CityEditorUI.
    pub const CITY_NAME: u32 = 0x053D_5A68;
    pub const CHANGE_CITIES: u32 = 0x34BB_EDE9;
    pub const PREVIOUS_CITY: u32 = 0x74BB_ED3E;
    pub const NEXT_CITY: u32 = 0x74BB_ED3D;
    pub const SAVE_EXIT: u32 = 0x1036_A039;
    pub const SPORE_GUIDE: u32 = 0x044E_FE63;
    pub const CITY_LEGEND: u32 = 0x065F_6003;
    pub const COLONY_LEGEND: u32 = 0x065F_6004;
    pub const TRIBE_COLOR: u32 = 0x054A_CA40;
    pub const TRIBE_COLOR_BUTTON: u32 = 0xA7C5_C4A6;
    // CityPlannerCityStats.
    pub const STATS_ROOT: u32 = 0x0199_A7DE;
    pub const STATS_NAME: u32 = 0x02CF_2F44;
    pub const STATS_FACE: u32 = 0x046A_DF86;
    /// Specialty icons: economic, religious, military.
    pub const STATS_KINDS: [u32; 3] = [0x02CF_2F30, 0x02CF_2F34, 0x02CF_2F38];
    pub const STATS_POPULATION: u32 = 0x0199_A7D2;
    pub const STATS_CAPACITY: u32 = 0x33B5_C7A6;
    pub const STATS_MOOD: [u32; 5] = [0x04FE_1AA0, 0x04FE_1AA1, 0x04FE_1AA2, 0x04FE_1AA3, 0x04FE_1AA4];
    pub const STATS_DEFENCE: [u32; 5] = [0x04FE_38F0, 0x04FE_38F1, 0x04FE_38F2, 0x04FE_38F3, 0x04FE_38F4];
    pub const STATS_INCOME: u32 = 0x02CF_2F74;
    pub const STATS_PLATES: [u32; 2] = [0x02CF_324C, 0x04B1_F8AE];
}

/// Where the stats panel sits (screen pixels, under the name banner).
const STATS_AT: Vec2 = Vec2::new(8.0, 72.0);
/// The planner's distance from the city: the retail planner mode's enter
/// function (0xD0E170) saves the camera, loads the script named by 0xD098F0
/// (`city_large_planner_camera` 0x0C24C2B5 for a city, `city_small_...`
/// 0x225D13AD otherwise) through 0xB117C0 and sets `CameraPreRotateZ / X` (0xB0F660;
/// property list `CommunityEditor`, defaults 0.15 / 0.07); it makes no
/// `SnapPolar` call, and the script loader 0xB10810 makes the script's
/// `default_distance` (115, VERIFIED in the dump) the camera's goal distance,
/// so the camera glides from where it was to 115 over the city. Its `pitch` is
/// the camera's own (the script never assigns it; `default_pitch` 40 is read by
/// nothing), so the pitch is left as it was (was: the ADAPTED 100 / 55 degrees).
/// The pre-rotation's use by the view (+0x264 / +0x268) is not traced.
const PLANNER_DISTANCE: f32 = 115.0;
/// A slot is under the pointer within this many pixels.
const PICK_PIXELS: f32 = 42.0;

/// The palette's categories in their installed order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum PlannerPage {
    #[default]
    Buildings,
    Vehicles,
    Decorations,
    Music,
    Outfits,
}

impl PlannerPage {
    pub const ALL: [PlannerPage; 5] =
        [PlannerPage::Buildings, PlannerPage::Vehicles, PlannerPage::Decorations, PlannerPage::Music, PlannerPage::Outfits];

    /// `paletteCategorySequenceNumber`.
    pub fn sequence(self) -> u32 {
        match self {
            PlannerPage::Buildings => 100,
            PlannerPage::Vehicles => 200,
            PlannerPage::Decorations => 300,
            PlannerPage::Music => 500,
            PlannerPage::Outfits => 1000,
        }
    }

    /// `paletteCategoryName` (locale 0x4297B017 ids 1, 6 / 4 / 5, 3, 7, 9).
    pub fn title(self, kind: Approach) -> &'static str {
        match (self, kind) {
            (PlannerPage::Buildings, _) => "Buildings",
            (PlannerPage::Vehicles, Approach::Military) => "Military Vehicles",
            (PlannerPage::Vehicles, Approach::Religious) => "Cultural Vehicles",
            (PlannerPage::Vehicles, Approach::Economic) => "Economic Vehicles",
            (PlannerPage::Decorations, _) => "Decorations",
            (PlannerPage::Music, _) => "Music",
            (PlannerPage::Outfits, _) => "Outfits",
        }
    }
}

/// What the palette arms.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PlannerItem {
    Building(Building),
    Turret,
    /// A civic object (`civ_decor::ITEMS` index).
    Decoration(u8),
}

/// A vehicle swatch of the Vehicles page: (domain or the Spaceship, the
/// installed `blockName` by city type, `modelPrice`, `paletteAssetSwatchLockedText`).
pub fn vehicle_swatches(kind: Approach) -> [(Option<VehicleDomain>, &'static str, f32, &'static str); 4] {
    let names = match kind {
        Approach::Military => ["Military Land", "Military Sea", "Military Air"],
        Approach::Religious => ["Religious Land", "Religious Sea", "Religious Air"],
        Approach::Economic => ["Economic Land", "Economic Sea", "Economic Air"],
    };
    [
        (Some(VehicleDomain::Land), names[0], civ::LAND_VEHICLE_COST, "Need 40 Citizens to unlock"),
        (Some(VehicleDomain::Sea), names[1], civ::SEA_VEHICLE_COST, "Gain control of 3 cities to invent sea vehicles"),
        (Some(VehicleDomain::Air), names[2], civ::AIR_VEHICLE_COST, "Gain control of 5 cities to invent air vehicles"),
        (None, "Spaceship", 0.0, "Unlocked in the Space stage"),
    ]
}

/// Whether a vehicle swatch is open (land always; sea and air by the
/// campaign's unlocks; the Spaceship only in Space: `city_locked_ufo`).
pub fn vehicle_unlocked(campaign: &civ::CivCampaign, domain: Option<VehicleDomain>) -> bool {
    match domain {
        Some(VehicleDomain::Land) => true,
        Some(VehicleDomain::Sea) => campaign.sea_unlocked(),
        Some(VehicleDomain::Air) => campaign.air_unlocked(),
        None => false,
    }
}

/// What a palette click asks for beyond arming an item.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteAction {
    Page(PlannerPage),
    /// New / Edit of a vehicle class (opens the vehicle editor).
    Vehicle(VehicleDomain),
    /// Edit of a building class (opens the building editor).
    EditBuilding(Option<Building>),
    /// The citizens' outfitter.
    Outfit,
}

impl PlannerItem {
    pub fn title(self) -> &'static str {
        match self {
            PlannerItem::Building(b) => b.title(),
            PlannerItem::Turret => "Turret",
            PlannerItem::Decoration(i) => crate::civ_decor::ITEMS.get(i as usize).map_or("Decoration", |d| d.1),
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            PlannerItem::Building(b) => b.cost(),
            PlannerItem::Turret => civ::TURRET_COST,
            PlannerItem::Decoration(_) => crate::civ_decor::DECORATION_COST,
        }
    }
}

/// The colour classes of the City Legend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    /// Blue: a factory and a house or the hall (+400 income).
    Income,
    /// Green: an entertainment building and a house or the hall (happier).
    Happy,
    /// Red: a factory and an entertainment building (less happy).
    Unhappy,
    /// White: anything else (an empty slot included).
    Neutral,
}

impl LinkKind {
    pub fn color(self) -> Color {
        match self {
            LinkKind::Income => Color::srgb(0.25, 0.55, 1.0),
            LinkKind::Happy => Color::srgb(0.3, 0.95, 0.35),
            LinkKind::Unhappy => Color::srgb(1.0, 0.25, 0.2),
            LinkKind::Neutral => Color::srgba(0.95, 0.95, 0.95, 0.55),
        }
    }
}

/// Pure: the legend colour of a link between two slots (0xBE2590's rule).
pub fn link_kind(a: Option<CityBuilding>, b: Option<CityBuilding>) -> LinkKind {
    use CityBuilding::*;
    match (a, b) {
        (Some(Factory), Some(House | CityHall)) | (Some(House | CityHall), Some(Factory)) => LinkKind::Income,
        (Some(Entertainment), Some(House | CityHall)) | (Some(House | CityHall), Some(Entertainment)) => {
            LinkKind::Happy
        }
        (Some(Factory), Some(Entertainment)) | (Some(Entertainment), Some(Factory)) => LinkKind::Unhappy,
        _ => LinkKind::Neutral,
    }
}

/// Pure: every link of a city with its legend colour (node 0 is the hall,
/// node `1 + p` plot `p`, `CityState::link_graph`).
pub fn links(city: &CityState) -> Vec<(usize, usize, LinkKind)> {
    let (nodes, links) = city.link_graph();
    links.into_iter().map(|(a, b)| (a, b, link_kind(nodes[a], nodes[b]))).collect()
}

/// Pure: the city with `item` placed on `plot` (a building on a free plot, a
/// turret on the next free tower or a ruined one), or `None` when it cannot
/// go there.
pub fn preview(city: &CityState, item: PlannerItem, plot: Option<usize>) -> Option<CityState> {
    let mut trial = city.clone();
    match item {
        PlannerItem::Building(kind) => {
            let plot = plot?;
            if trial.plots.get(plot) != Some(&None) {
                return None;
            }
            trial.plots[plot] = Some(kind);
            if let Some(d) = trial.building_damage.get_mut(plot) {
                *d = 0.0;
            }
        }
        PlannerItem::Decoration(_) => return None,
        PlannerItem::Turret => {
            if let Some(i) = trial.turret_hp.iter().position(|hp| *hp <= 0.0) {
                trial.turret_hp[i] = civ::TURRET_HP;
            } else if (trial.defense as usize) < trial.max_turrets() {
                trial.defense += 1;
                trial.turret_hp.push(civ::TURRET_HP);
            } else {
                return None;
            }
        }
    }
    Some(trial)
}

/// What the stats panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannerStats {
    pub name: String,
    pub kind: Approach,
    pub population: u32,
    pub capacity: u32,
    /// The link model's `mHappyCount - mUnhappyCount` (`cCity` +0x668 -
    /// +0x66C): [`mood_icon_frame`] lights `|delta|` of the five orbs.
    pub mood_delta: i32,
    /// Lit defence orbs: one per standing turret, at most five.
    pub defence: u32,
    pub per_minute: f32,
    pub relation: f32,
    /// The nation's colour (`0xAARRGGBB`) for the name plate.
    pub color: u32,
}

/// Frame of mood orb `i` (of five) of the stats panel: the city card's rule
/// (`RolloverCivCity`, 0xBDE100, VERIFIED in `civ-audit-b`): for `i <
/// |delta|` the icon value is 100 (delta > 0: the green face, frame 2) or 50
/// (delta < 0: the red face, frame 1), else 0 (dark, frame 0). A city with as
/// many happy as unhappy links lights nothing.
pub fn mood_icon_frame(i: u32, delta: i32) -> u32 {
    if i < delta.unsigned_abs() {
        if delta > 0 {
            2
        } else {
            1
        }
    } else {
        0
    }
}

/// Lit defence orbs of the stats panel: one (value 100) per turret whose
/// combatant is alive, at most five (the city card's rule, `civ-audit-b`).
pub fn defence_lit(city: &CityState) -> u32 {
    city.live_turrets().min(5) as u32
}

/// Pure: the panel's values for a city. INFERRED: `CityPlannerCityStats`
/// reuses the control ids of the city card (population `0199A7D2`, mood orbs
/// `04FE1AA0..A4`, defence orbs `04FE38F0..F4`, face `046ADF86`, income
/// `02CF2F74`), so it is filled by the same rules as the card; the routine
/// that fills the planner's own window was not located. The face of one of
/// your cities is the card's neutral 50.0 (0xBD8220 for the player's own
/// civilization), not a mood face. ADAPTED: the capacity (the retail source of
/// `33B5C7A6` is not decoded) shows the population.
pub fn stats(city: &CityState) -> PlannerStats {
    let population = crate::civ_hud::world::population(city);
    let model = city.model();
    PlannerStats {
        name: city.label(),
        kind: city.kind,
        population,
        capacity: population,
        mood_delta: model.happy - model.unhappy,
        defence: defence_lit(city),
        per_minute: crate::civ_hud::sim::income_per_minute(city),
        relation: if city.owned() { 0.0 } else { crate::civ_hud::sim::citizen_relation(city) },
        color: crate::civ_hud::sim::nation_argb(city.nation()),
    }
}

/// Pure: the next (or previous) city of yours after `from` (Change Cities).
pub fn cycle_city(cities: &[CityState], from: usize, forward: bool) -> usize {
    let owned: Vec<usize> = cities.iter().filter(|c| c.owned()).map(|c| c.index).collect();
    let Some(at) = owned.iter().position(|i| *i == from) else {
        return owned.first().copied().unwrap_or(from);
    };
    let n = owned.len();
    owned[if forward { (at + 1) % n } else { (at + n - 1) % n }]
}

/// The planner's state. `request` opens it on a city (the city card's
/// button); `city` is the city being planned.
#[derive(Resource, Default, Debug)]
pub struct CivPlanner {
    pub city: Option<usize>,
    pub request: Option<usize>,
    pub armed: Option<PlannerItem>,
    pub hover: Option<usize>,
    /// The decoration spot under the pointer (Decorations page).
    pub hover_spot: Option<usize>,
    pub page: PlannerPage,
    restore: Option<(Vec3, f32, f32)>,
}

impl CivPlanner {
    pub fn is_open(&self) -> bool {
        self.city.is_some()
    }
}

/// Which planner layout a spawned SPUI root is.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlannerRoot {
    Frame,
    Stats,
}

/// The palette (Bevy UI) and its items; the page it was built for and the
/// city's type (vehicle names).
#[derive(Component)]
pub struct PlannerPalette(pub PlannerPage, pub Approach);

#[derive(Component, Clone, Copy)]
pub struct PaletteItem(pub PlannerItem);

/// Every entity of the palette (clicks over it are not slot clicks).
#[derive(Component)]
pub struct PaletteNode;

/// The palette's Anthem button (opens the anthem editor, `civ_anthem`).
#[derive(Component)]
pub struct AnthemButton;

#[derive(Component)]
pub struct PaletteNote;

/// The planner's own gizmo lines (drawn over the scene).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct PlannerGizmos;

pub struct CivPlannerPlugin;

impl Plugin for CivPlannerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivPlanner>()
            .init_resource::<art::PlannerArt>()
            .init_resource::<art::DecorThumbs>()
            .init_resource::<art::DesignThumbs>()
            .add_systems(
                Update,
                (
                    art::animate.after(manage),
                    art::design_thumbnails
                        .after(manage)
                        .run_if(resource_exists::<Assets<Mesh>>)
                        .run_if(resource_exists::<Assets<Image>>),
                    art::decor_thumbnails
                        .after(manage)
                        .run_if(resource_exists::<Assets<Mesh>>)
                        .run_if(resource_exists::<Assets<StandardMaterial>>)
                        .run_if(resource_exists::<Assets<Image>>),
                ),
            )
            .add_systems(
                Update,
                planner_input
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Civilization)),
            )
            .add_systems(Update, (manage, bind).chain().after(view::input).before(view::sync));
        // The links are gizmo lines (only where the renderer runs).
        if app.is_plugin_added::<bevy::gizmos::GizmoPlugin>() {
            app.init_gizmo_group::<PlannerGizmos>()
                .add_systems(Startup, configure_gizmos)
                .add_systems(Update, draw_slots.run_if(in_state(GameStage::Civilization)));
        }
    }
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<PlannerGizmos>();
    config.depth_bias = -1.0;
    config.line.width = 3.0;
}

/// Move the Civ camera over a city (keeping what it showed before).
fn focus_city(
    city: &CityState,
    camera: &mut Option<ResMut<crate::civ_controls::CivCamera>>,
    focus: &mut Query<&mut Transform, With<civ::CivCommander>>,
    restore: &mut Option<(Vec3, f32, f32)>,
) {
    let Ok(mut t) = focus.single_mut() else {
        return;
    };
    if let Some(cam) = camera.as_deref_mut() {
        if restore.is_none() {
            *restore = Some((t.translation, cam.goal_distance, cam.goal_pitch));
        }
        cam.goal_distance = PLANNER_DISTANCE;
    } else if restore.is_none() {
        *restore = Some((t.translation, 0.0, 0.0));
    }
    t.translation = city.center();
}

fn close(
    planner: &mut CivPlanner,
    camera: &mut Option<ResMut<crate::civ_controls::CivCamera>>,
    focus: &mut Query<&mut Transform, With<civ::CivCommander>>,
) {
    if let Some((at, distance, pitch)) = planner.restore.take() {
        if let Ok(mut t) = focus.single_mut() {
            t.translation = at;
        }
        if let Some(cam) = camera.as_deref_mut() {
            if distance > 0.0 {
                cam.goal_distance = distance;
                cam.goal_pitch = pitch;
            }
        }
    }
    planner.city = None;
    planner.armed = None;
    planner.hover = None;
    planner.hover_spot = None;
}

/// Open / close, the frame's buttons, the palette and the clicks on the
/// slots (buy on a free slot with an armed building, sell with a right-click
/// or Delete).
#[allow(clippy::too_many_arguments)]
fn planner_input(
    mut planner: ResMut<CivPlanner>,
    mut run: ResMut<RunProgress>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    mut roots: Query<(&mut SpuiState, &PlannerRoot)>,
    palette: Query<&PaletteItem, Added<Pressed>>,
    hover: Option<Res<bevy::picking::hover::HoverMap>>,
    palette_nodes: Query<(), With<PaletteNode>>,
    mut camera: Option<ResMut<crate::civ_controls::CivCamera>>,
    mut focus: Query<&mut Transform, With<civ::CivCommander>>,
    anthem_button: Query<(), (With<AnthemButton>, Added<Pressed>)>,
    mut anthem: ResMut<crate::civ_anthem::AnthemEditor>,
    (actions, mut rigid, mut editor): (
        Query<&PaletteAction, Added<Pressed>>,
        Option<ResMut<crate::rigid_editor::RigidEditor>>,
        Option<ResMut<crate::editor::EditorState>>,
    ),
) {
    let run = &mut *run;
    if let Some(c) = planner.request {
        match run.civ_campaign.cities.get(c) {
            Some(city) if city.owned() => {
                planner.request = None;
                planner.city = Some(c);
                let restore = &mut planner.restore;
                focus_city(city, &mut camera, &mut focus, restore);
            }
            // Not yours: refused.
            Some(_) => planner.request = None,
            // The campaign has not laid its cities out yet: wait.
            None => {}
        }
    }
    let Some(mut c) = planner.city else {
        return;
    };
    if !run.civ_campaign.cities.get(c).is_some_and(|city| city.owned()) {
        close(&mut planner, &mut camera, &mut focus);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        // The planner takes Esc (the pause menu would open otherwise).
        keys.clear_just_pressed(KeyCode::Escape);
        if planner.armed.take().is_none() {
            close(&mut planner, &mut camera, &mut focus);
        }
        return;
    }
    // Frame buttons.
    for (mut ui, kind) in &mut roots {
        if *kind != PlannerRoot::Frame {
            continue;
        }
        for click in ui.take_clicks() {
            match ui.layout.window(click).control_id {
                id::SAVE_EXIT => {
                    run.request_save();
                    close(&mut planner, &mut camera, &mut focus);
                    return;
                }
                id::PREVIOUS_CITY | id::NEXT_CITY => {
                    let forward = ui.layout.window(click).control_id == id::NEXT_CITY;
                    c = cycle_city(&run.civ_campaign.cities, c, forward);
                    planner.city = Some(c);
                    let restore = &mut planner.restore;
                    focus_city(&run.civ_campaign.cities[c], &mut camera, &mut focus, restore);
                }
                _ => {}
            }
        }
    }
    let difficulty = civ_rules::Difficulty::from_index(run.cell.difficulty);
    // The Anthem button opens / closes the anthem editor.
    if !anthem_button.is_empty() {
        anthem.open = !anthem.open;
    }
    // Category tabs and the swatches that open an editor (the planner
    // closes; the editor opens on the class).
    for action in &actions {
        let kind = run.civ_campaign.cities[c].kind;
        let class = match *action {
            PaletteAction::Page(page) => {
                if planner.page != page {
                    planner.page = page;
                    planner.armed = None;
                    planner.hover_spot = None;
                }
                None
            }
            PaletteAction::Vehicle(domain) => Some(crate::rigid_editor::vehicle_class(kind, domain)),
            PaletteAction::EditBuilding(building) => Some(building_class(building)),
            PaletteAction::Outfit => {
                if let Some(editor) = editor.as_deref_mut() {
                    editor.show();
                    editor.visuals_dirty = true;
                    close(&mut planner, &mut camera, &mut focus);
                    return;
                }
                None
            }
        };
        if let (Some(class), Some(rigid)) = (class, rigid.as_deref_mut()) {
            rigid.reopen = Some(class);
            close(&mut planner, &mut camera, &mut focus);
            return;
        }
    }
    // Palette: arm an item (again: disarm); a turret goes straight onto the
    // walls.
    for item in &palette {
        match item.0 {
            PlannerItem::Turret => {
                let spice = run.spice;
                let city = &mut run.civ_campaign.cities[c];
                let (line, left) = match civ::buy_turret(city, spice) {
                    Ok(left) => (format!("{}: turret raised ({} standing)", city.label(), city.live_turrets()), left),
                    Err(e) => (e, spice),
                };
                if left != spice {
                    run.spice = left;
                    run.request_save();
                }
                run.civ_campaign.message = line;
            }
            other => {
                planner.armed = if planner.armed == Some(other) { None } else { Some(other) };
            }
        }
    }
    // The slot under the pointer.
    let over_palette = hover
        .as_ref()
        .is_some_and(|h| h.0.values().any(|hits| hits.keys().any(|e| palette_nodes.contains(*e))));
    let cursor = windows.iter().find_map(|w| w.cursor_position());
    let city = &run.civ_campaign.cities[c];
    let shape = run.civ_campaign.shape();
    // A decoration armed: the decoration spots instead of the plots.
    if let Some(PlannerItem::Decoration(item)) = planner.armed {
        planner.hover_spot = match (cursor, cameras.single().ok()) {
            (Some(cursor), Some((camera, transform))) if !over_palette => (0..city.layout().decoration_spots())
                .filter_map(|s| {
                    let at = decor_world(city, s, shape);
                    camera.world_to_viewport(transform, at).ok().map(|p| (s, p.distance(cursor)))
                })
                .filter(|(_, d)| *d <= PICK_PIXELS)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(s, _)| s),
            (None, _) => planner.hover_spot,
            _ => None,
        };
        let left = mouse.as_ref().is_some_and(|m| m.just_pressed(MouseButton::Left)) && !over_palette;
        if let (true, Some(spot)) = (left, planner.hover_spot) {
            if let Some(line) = place_decoration(run, c, item, spot) {
                run.civ_campaign.message = line;
            }
        }
        return;
    }
    planner.hover_spot = None;
    planner.hover = match (cursor, cameras.single().ok()) {
        (Some(cursor), Some((camera, transform))) if !over_palette => (0..city.plots.len())
            .filter_map(|p| {
                let at = crate::civ_rts::plot_world(city, p, shape);
                camera.world_to_viewport(transform, at).ok().map(|s| (p, s.distance(cursor)))
            })
            .filter(|(_, d)| *d <= PICK_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p),
        // No pointer (an offscreen capture): the hover stays as it was set.
        (None, _) => planner.hover,
        _ => None,
    };
    let left = mouse.as_ref().is_some_and(|m| m.just_pressed(MouseButton::Left)) && !over_palette;
    let right = mouse.as_ref().is_some_and(|m| m.just_pressed(MouseButton::Right)) && !over_palette;
    let delete = keys.just_pressed(KeyCode::Delete);
    let Some(plot) = planner.hover else {
        return;
    };
    let action = if left {
        SlotAction::Place
    } else if right || delete {
        SlotAction::Sell
    } else {
        return;
    };
    if let Some(message) = slot_action(run, c, planner.armed, plot, action, difficulty) {
        run.civ_campaign.message = message;
    }
}

/// The building editor class of a building swatch (`None`: the city hall).
pub fn building_class(building: Option<Building>) -> crate::creature_editor_data::SetupKind {
    use crate::creature_editor_data::SetupKind as K;
    match building {
        None => K::BuildingCityHall,
        Some(Building::House) => K::BuildingHouse,
        Some(Building::Factory) => K::BuildingFactory,
        Some(Building::Entertainment) => K::BuildingEntertainment,
    }
}

/// Where decoration spot `spot` of a city stands in the world.
pub fn decor_world(city: &CityState, spot: usize, shape: crate::civ_planet::Shape) -> Vec3 {
    let site = city.decoration_site(spot);
    shape.local_point(city.center(), Vec3::new(site.x, 0.4, site.y) * CITY_SCALE)
}

/// Buy decoration `item` on `spot` of city `c` (`DECORATION_COST`); the
/// message for the player, `None` when nothing happened.
pub fn place_decoration(run: &mut RunProgress, c: usize, item: u8, spot: usize) -> Option<String> {
    let spice = run.spice;
    let city = run.civ_campaign.cities.get_mut(c)?;
    if !city.owned() {
        return None;
    }
    let name = crate::civ_decor::ITEMS.get(item as usize)?.1;
    if spice < crate::civ_decor::DECORATION_COST {
        return Some(format!("{}: a {name} costs {:.0}", city.label(), crate::civ_decor::DECORATION_COST));
    }
    crate::civ_decor::place(&mut city.decorations, item as usize, Some(spot))?;
    let line = format!("{}: {name} placed", city.label());
    run.spice = spice - crate::civ_decor::DECORATION_COST;
    run.request_save();
    Some(line)
}

/// A click on a slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotAction {
    /// Left click: build the armed building on a free slot.
    Place,
    /// Right click / Delete: sell the building standing there.
    Sell,
}

/// Apply a slot click to city `c` (the planner's buy / sell); returns the
/// message for the player, `None` when the click does nothing.
pub fn slot_action(
    run: &mut RunProgress,
    c: usize,
    armed: Option<PlannerItem>,
    plot: usize,
    action: SlotAction,
    difficulty: civ_rules::Difficulty,
) -> Option<String> {
    let spice = run.spice;
    let city = run.civ_campaign.cities.get_mut(c)?;
    let (line, spice) = match action {
        SlotAction::Place => {
            let Some(PlannerItem::Building(kind)) = armed else {
                return None;
            };
            match civ::buy_plot(city, kind, Some(plot), spice) {
                Ok((left, _)) => (
                    format!(
                        "{}: {} built (mood {}%, +{:.0}/min)",
                        city.label(),
                        kind.title(),
                        city.happiness(),
                        crate::civ_hud::sim::income_per_minute(city)
                    ),
                    left,
                ),
                Err(e) => return Some(e),
            }
        }
        SlotAction::Sell => {
            if !city.standing(plot) {
                return None;
            }
            match civ::sell_plot(city, Some(plot), difficulty) {
                Ok((refund, kind, _)) => (
                    format!("{}: {} sold for {refund:.0}", city.label(), kind.title()),
                    (spice + refund).min(civ_rules::WEALTH_CAP),
                ),
                Err(e) => return Some(e),
            }
        }
    };
    run.spice = spice;
    run.request_save();
    Some(line)
}

/// Spawn the planner's layouts and palette while it is open (the Civ HUD
/// steps aside, `civ_hud::manage`).
#[allow(clippy::too_many_arguments)]
fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    mut planner: ResMut<CivPlanner>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &PlannerRoot)>,
    palettes: Query<(Entity, &PlannerPalette)>,
    run: Res<RunProgress>,
    (mut art, mut images): (Option<ResMut<art::PlannerArt>>, Option<ResMut<Assets<Image>>>),
) {
    if *stage.get() != GameStage::Civilization {
        // (A request made before the stage starts waits for it.)
        planner.city = None;
        planner.restore = None;
    }
    let want = planner.is_open();
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        for (e, _) in &palettes {
            commands.entity(e).despawn();
        }
        return;
    }
    // The palette is rebuilt when the page or the planned city's type
    // changes.
    let kind = planner.city.and_then(|c| run.civ_campaign.cities.get(c)).map_or(Approach::Military, |c| c.kind);
    let mut current = false;
    for (e, p) in &palettes {
        if p.0 == planner.page && p.1 == kind {
            current = true;
        } else {
            commands.entity(e).despawn();
        }
    }
    if !current {
        let source = ArtSource { art: art.as_deref_mut(), originals: &originals, images: images.as_deref_mut() };
        spawn_palette(&mut commands, planner.page, kind, &run.civ_campaign, source);
    }
    if !roots.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    if let Some(layout) = assets.layout(index, FRAME_LAYOUT) {
        // ADAPTED: the root window's design-time fill (0x537F7FFF, a third of
        // light blue) would tint the whole planet; it is not painted.
        let mut frame = (*layout).clone();
        for r in frame.roots.clone() {
            frame.windows[r].fill_color = 0;
        }
        let root = view::spawn(&mut commands, std::sync::Arc::new(frame), RootPolicy::Fit, Some(camera), 110);
        commands.entity(root).insert(PlannerRoot::Frame);
    }
    if let Some(layout) = assets.layout(index, STATS_LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 111);
        commands.entity(root).insert(PlannerRoot::Stats);
    }
}

fn label(value: impl Into<String>, size: f32) -> impl Bundle {
    label_in(value, size, Color::srgb(0.95, 0.93, 0.85))
}

fn label_in(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn palette_button(p: &mut ChildSpawnerCommands, extra: impl Bundle, height: f32, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((
        Button::default(),
        PaletteNode,
        Node {
            height: Val::Px(height),
            padding: UiRect::horizontal(Val::Px(6.0)),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.12, 0.16, 0.22, 0.85)),
        BorderColor::all(Color::srgba(0.5, 0.6, 0.7, 0.5)),
        extra,
    ))
    .with_children(children);
}

/// The installed images of a swatch (`art::Swatch`), resolved once per
/// palette build; all `None` without the install (the swatch is then text).
#[derive(Clone, Default)]
struct SwatchLook {
    background: Option<Handle<Image>>,
    header: Option<Handle<Image>>,
    picture: [Option<Handle<Image>>; 4],
    locked: Option<Handle<Image>>,
    locked_background: Option<Handle<Image>>,
}

/// Where the palette's art comes from while it is built.
struct ArtSource<'a> {
    art: Option<&'a mut art::PlannerArt>,
    originals: &'a crate::Originals,
    images: Option<&'a mut Assets<Image>>,
}

impl ArtSource<'_> {
    fn image(&mut self, a: art::Art) -> Option<Handle<Image>> {
        let (Some(store), Some(images)) = (self.art.as_deref_mut(), self.images.as_deref_mut()) else {
            return None;
        };
        store.image(self.originals, images, a)
    }

    fn swatch(&mut self, s: art::Swatch) -> SwatchLook {
        SwatchLook {
            background: self.image(s.background),
            header: self.image(s.header),
            picture: s.picture.all().map(|a| self.image(a)),
            locked: s.locked.and_then(|a| self.image(a)),
            locked_background: self.image(s.locked_background),
        }
    }

    fn tab(&mut self, t: art::Tab) -> [Option<Handle<Image>>; 4] {
        t.all().map(|a| self.image(a))
    }

    /// Civic object thumbnails are drawn (the install is there).
    fn installed(&self) -> bool {
        !self.originals.resource_index.records.is_empty() && self.images.is_some()
    }
}

/// The swatch's picture: its art in four states (or `fallback` text).
/// The swatch's design thumbnail, over its picture (hidden until the class
/// has a design; the picture stays the button).
fn design_overlay(p: &mut ChildSpawnerCommands, slot: u8) {
    p.spawn((
        PaletteNode,
        art::DesignThumb(slot),
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        ImageNode::default(),
        Visibility::Hidden,
        Pickable::IGNORE,
    ));
}

fn swatch_picture(p: &mut ChildSpawnerCommands, look: &SwatchLook, locked: bool, extra: impl Bundle, fallback: &str, design: Option<u8>) {
    let states = if locked { [look.locked.clone(), None, None, None] } else { look.picture.clone() };
    if states[0].is_some() {
        p.spawn((
            PaletteNode,
            Node {
                width: Val::Px(SWATCH_PICTURE),
                height: Val::Px(SWATCH_PICTURE),
                flex_shrink: 0.0,
                ..default()
            },
            ImageNode::new(states[0].clone().unwrap_or_default()),
            art::ArtState::Button { states, off: false, active: false },
            extra,
        ))
        .with_children(|b| {
            if let Some(slot) = design {
                design_overlay(b, slot);
            }
        });
    } else {
        p.spawn((
            PaletteNode,
            Node {
                width: Val::Px(SWATCH_PICTURE),
                height: Val::Px(SWATCH_PICTURE),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.5, 0.6, 0.7, 0.5)),
            extra,
        ))
        .with_children(|b| {
            b.spawn((label(fallback, 9.0), PaletteNode));
            if let Some(slot) = design {
                design_overlay(b, slot);
            }
        });
    }
}

/// The swatch's caption: header icon and name, then the price or the lock
/// text.
fn swatch_caption(p: &mut ChildSpawnerCommands, look: &SwatchLook, name: &str, second: &str, dim: bool) {
    let (fg, sub) = if dim {
        (Color::srgb(0.6, 0.6, 0.63), Color::srgb(0.78, 0.62, 0.45))
    } else {
        (Color::srgb(0.95, 0.93, 0.85), Color::srgb(0.85, 0.9, 0.7))
    };
    p.spawn((
        PaletteNode,
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            row_gap: Val::Px(2.0),
            ..default()
        },
    ))
    .with_children(|c| {
        c.spawn((
            PaletteNode,
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(3.0),
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            if let (Some(header), false) = (look.header.clone(), dim) {
                row.spawn((
                    PaletteNode,
                    Node { width: Val::Px(16.0), height: Val::Px(16.0), flex_shrink: 0.0, ..default() },
                    ImageNode::new(header),
                ));
            }
            row.spawn((label_in(name, 11.0, fg), PaletteNode));
        });
        if !second.is_empty() {
            c.spawn((label_in(second, 9.0, sub), PaletteNode));
        }
    });
}

/// An asset swatch tile on its installed background.
fn swatch_tile(p: &mut ChildSpawnerCommands, look: &SwatchLook, locked: bool, extra: impl Bundle, children: impl FnOnce(&mut ChildSpawnerCommands)) {
    let background = if locked { look.locked_background.clone() } else { look.background.clone() };
    let mut tile = p.spawn((
        PaletteNode,
        Node {
            min_height: Val::Px(SWATCH_HEIGHT),
            flex_grow: 1.0,
            padding: UiRect::all(Val::Px(3.0)),
            column_gap: Val::Px(5.0),
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(if background.is_some() { Color::NONE } else { Color::srgba(0.12, 0.16, 0.22, 0.85) }),
        BorderColor::all(Color::NONE),
        extra,
    ));
    if let Some(background) = background {
        // The rounded tiles (64 x 64, 136 x 90) stretch around their corners.
        tile.insert(ImageNode {
            image: background,
            image_mode: bevy::ui::widget::NodeImageMode::Sliced(bevy::sprite::TextureSlicer {
                border: bevy::sprite::BorderRect::all(12.0),
                ..default()
            }),
            ..default()
        });
    }
    tile.with_children(children);
}

/// Swatch picture size (the 90 x 90 New icons drawn at half size).
const SWATCH_PICTURE: f32 = 44.0;
const SWATCH_HEIGHT: f32 = 50.0;

/// The palette of `page` for a city of type `kind`: the category tabs, then
/// the page's swatches in the installed item order, in the installed planner
/// art (`art`; text without the install).
fn spawn_palette(commands: &mut Commands, page: PlannerPage, kind: Approach, campaign: &civ::CivCampaign, mut source: ArtSource) {
    let tabs: Vec<(PlannerPage, [Option<Handle<Image>>; 4])> = {
        let mut pages = PlannerPage::ALL;
        pages.sort_by_key(|p| p.sequence());
        pages.into_iter().map(|p| (p, source.tab(art::tab_art(p)))).collect()
    };
    let buildings: Vec<(PlannerItem, SwatchLook)> = [
        PlannerItem::Building(Building::House),
        PlannerItem::Building(Building::Entertainment),
        PlannerItem::Building(Building::Factory),
        PlannerItem::Turret,
    ]
    .into_iter()
    .map(|item| {
        let look = match item {
            PlannerItem::Building(b) => source.swatch(art::building_swatch(Some(b))),
            _ => source.swatch(art::turret_swatch()),
        };
        (item, look)
    })
    .collect();
    let hall = source.swatch(art::building_swatch(None));
    let vehicles: Vec<_> = vehicle_swatches(kind)
        .into_iter()
        .map(|(domain, name, price, locked)| (domain, name, price, locked, source.swatch(art::vehicle_swatch(domain))))
        .collect();
    let thumbnails = source.installed();
    commands
        .spawn((
            StageOwned,
            PlannerPalette(page, kind),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                top: Val::Px(200.0),
                width: Val::Px(184.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(6.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.08, 0.12, 0.72)),
            GlobalZIndex(40),
            PaletteNode,
        ))
        .with_children(|p| {
            // Category tabs (`CIP_butn-section-*`).
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(2.0),
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                PaletteNode,
            ))
            .with_children(|row| {
                for (tab, states) in tabs {
                    let on = tab == page;
                    if states[0].is_some() {
                        row.spawn((
                            Button::default(),
                            PaletteNode,
                            PaletteAction::Page(tab),
                            Node { width: Val::Px(32.0), height: Val::Px(34.0), ..default() },
                            ImageNode::new(states[0].clone().unwrap_or_default()),
                            art::ArtState::Tab { states, on },
                        ));
                        continue;
                    }
                    row.spawn((
                        Button::default(),
                        PaletteNode,
                        PaletteAction::Page(tab),
                        Node {
                            padding: UiRect::axes(Val::Px(5.0), Val::Px(3.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(if on {
                            Color::srgba(0.3, 0.42, 0.55, 0.95)
                        } else {
                            Color::srgba(0.12, 0.16, 0.22, 0.85)
                        }),
                        BorderColor::all(if on { Color::srgb(1.0, 0.85, 0.3) } else { Color::srgba(0.5, 0.6, 0.7, 0.5) }),
                    ))
                    .with_children(|b| {
                        let title = match tab {
                            PlannerPage::Vehicles => "Vehicles",
                            other => other.title(kind),
                        };
                        b.spawn((label(title, 10.0), PaletteNode));
                    });
                }
            });
            p.spawn((label(page.title(kind), 13.0), PaletteNode));
            match page {
                PlannerPage::Buildings => {
                    // `cityplanner_page_buildings_parts`: City Hall, house,
                    // entertainment, factory, the type's turret. The tile
                    // arms the item; its picture (the New icon) opens the
                    // building editor (`paletteAssetSwatchEditable`; the
                    // turret has none, the hall is not draggable).
                    swatch_tile(p, &hall, false, (), |t| {
                        swatch_picture(t, &hall, false, (Button::default(), PaletteAction::EditBuilding(None)), "Edit", Some(art::building_slot(None)));
                        swatch_caption(t, &hall, "City Hall", "Edit", false);
                    });
                    for (item, look) in &buildings {
                        let item = *item;
                        swatch_tile(p, look, false, (Button::default(), PaletteItem(item)), |t| {
                            match item {
                                PlannerItem::Building(b) => swatch_picture(
                                    t,
                                    look,
                                    false,
                                    (Button::default(), PaletteAction::EditBuilding(Some(b))),
                                    "Edit",
                                    Some(art::building_slot(Some(b))),
                                ),
                                _ => swatch_picture(t, look, false, (), "", None),
                            }
                            swatch_caption(t, look, item.title(), &format!("${:.0}", item.cost()), false);
                        });
                    }
                }
                PlannerPage::Vehicles => {
                    for (domain, name, price, locked, look) in &vehicles {
                        let open = vehicle_unlocked(campaign, *domain);
                        match (*domain, open) {
                            (Some(d), true) => swatch_tile(p, look, false, (Button::default(), PaletteAction::Vehicle(d)), |t| {
                                swatch_picture(t, look, false, (), "Edit", Some(art::vehicle_slot(d)));
                                swatch_caption(t, look, name, &format!("${price:.0}"), false);
                            }),
                            _ => swatch_tile(p, look, true, (), |t| {
                                swatch_picture(t, look, true, (), "", None);
                                swatch_caption(t, look, name, locked, true);
                            }),
                        }
                    }
                }
                PlannerPage::Decorations => {
                    // 3 columns x 5 rows, column-major like the item list;
                    // each swatch shows the object's model render.
                    p.spawn((
                        PaletteNode,
                        Node {
                            display: Display::Grid,
                            grid_template_columns: RepeatedGridTrack::flex(3, 1.0),
                            column_gap: Val::Px(3.0),
                            row_gap: Val::Px(3.0),
                            ..default()
                        },
                    ))
                    .with_children(|grid| {
                        for row in 0..5u8 {
                            for col in 0..3u8 {
                                let index = col * 5 + row;
                                let item = PlannerItem::Decoration(index);
                                grid.spawn((
                                    Button::default(),
                                    PaletteItem(item),
                                    PaletteNode,
                                    Node {
                                        height: Val::Px(48.0),
                                        padding: UiRect::all(Val::Px(1.0)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        border: UiRect::all(Val::Px(2.0)),
                                        border_radius: BorderRadius::all(Val::Px(5.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.12, 0.16, 0.22, 0.85)),
                                    BorderColor::all(Color::srgba(0.5, 0.6, 0.7, 0.5)),
                                ))
                                .with_children(|b| {
                                    if thumbnails {
                                        b.spawn((
                                            PaletteNode,
                                            art::DecorThumb(index),
                                            Node { width: Val::Px(42.0), height: Val::Px(42.0), ..default() },
                                            ImageNode::default(),
                                            Visibility::Hidden,
                                        ));
                                    } else {
                                        b.spawn((label(item.title(), 8.0), PaletteNode));
                                    }
                                });
                            }
                        }
                    });
                    p.spawn((label(format!("${:.0} each", crate::civ_decor::DECORATION_COST), 10.0), PaletteNode));
                }
                PlannerPage::Music => {
                    palette_button(p, AnthemButton, 30.0, |b| {
                        b.spawn((label("City Anthem", 11.0), PaletteNode));
                    });
                }
                PlannerPage::Outfits => {
                    palette_button(p, PaletteAction::Outfit, 30.0, |b| {
                        b.spawn((label("Citizen Outfit", 11.0), PaletteNode));
                    });
                }
            }
            p.spawn((label("", 10.0), PaletteNote));
        });
}

/// Fill the frame, the stats panel (with the preview) and the palette.
#[allow(clippy::too_many_arguments)]
fn bind(
    planner: Res<CivPlanner>,
    run: Res<RunProgress>,
    mut roots: Query<(&mut SpuiState, &PlannerRoot)>,
    mut items: Query<(&PaletteItem, &mut BorderColor, &mut BackgroundColor, Has<ImageNode>)>,
    mut notes: Query<&mut Text, With<PaletteNote>>,
    anthem: Res<crate::civ_anthem::AnthemEditor>,
    mut palette: Query<&mut Node, With<PlannerPalette>>,
    mut anthem_button: Query<&mut BorderColor, (With<AnthemButton>, Without<PaletteItem>)>,
) {
    // The palette steps right of the anthem editor's panel while it is open.
    for mut node in &mut palette {
        let left = Val::Px(if anthem.open { 226.0 } else { 8.0 });
        if node.left != left {
            node.left = left;
        }
    }
    for mut border in &mut anthem_button {
        *border = BorderColor::all(if anthem.open { Color::srgb(1.0, 0.85, 0.3) } else { Color::srgba(0.5, 0.6, 0.7, 0.5) });
    }
    let Some(c) = planner.city else {
        return;
    };
    let Some(city) = run.civ_campaign.cities.get(c) else {
        return;
    };
    let previewed = planner.armed.and_then(|item| match item {
        PlannerItem::Building(_) => preview(city, item, planner.hover),
        _ => None,
    });
    let shown = stats(previewed.as_ref().unwrap_or(city));
    let owned = run.civ_campaign.owned_count();
    for (mut ui, kind) in &mut roots {
        match kind {
            PlannerRoot::Frame => {
                if let Some(w) = ui.find(id::CITY_NAME) {
                    ui.set_text(w, format!("{} - {} city", city.label(), city.kind.title()));
                }
                for (control, on) in [
                    (id::CITY_LEGEND, true),
                    (id::COLONY_LEGEND, false),
                    (id::TRIBE_COLOR, false),
                    (id::TRIBE_COLOR_BUTTON, false),
                    (id::CHANGE_CITIES, owned > 1),
                ] {
                    crate::civ_hud::set_visible(&mut ui, control, on);
                }
            }
            PlannerRoot::Stats => {
                let at = if anthem.open { Vec2::new(226.0, STATS_AT.y) } else { STATS_AT };
                bind_stats(&mut ui, &shown, at)
            }
        }
    }
    let money = run.spice;
    for (item, mut border, mut background, art) in &mut items {
        let armed = planner.armed == Some(item.0);
        let idle = if art { Color::NONE } else { Color::srgba(0.5, 0.6, 0.7, 0.5) };
        *border = BorderColor::all(if armed { Color::srgb(1.0, 0.85, 0.3) } else { idle });
        let afford = money >= item.0.cost();
        // An installed swatch background stays visible; unaffordable items
        // are tinted red.
        background.0 = match (afford, art) {
            (true, true) => Color::NONE,
            (true, false) => Color::srgba(0.12, 0.16, 0.22, 0.85),
            (false, true) => Color::srgba(0.5, 0.08, 0.08, 0.45),
            (false, false) => Color::srgba(0.25, 0.08, 0.08, 0.85),
        };
    }
    let note = match (planner.armed, planner.hover) {
        (Some(PlannerItem::Building(kind)), Some(plot)) if city.plots.get(plot) == Some(&None) => format!(
            "{} here: mood {}%, +{:.0}/min",
            kind.title(),
            previewed.as_ref().map_or(city.happiness(), |p| p.happiness()),
            shown.per_minute
        ),
        (Some(item), _) => format!("Click a free slot for the {}", item.title().to_lowercase()),
        (None, Some(plot)) if city.standing(plot) && planner.page == PlannerPage::Buildings => {
            "Right-click (or Delete) sells".into()
        }
        _ => match planner.page {
            PlannerPage::Buildings => format!("${money:.0} - pick a building"),
            PlannerPage::Vehicles => format!("${money:.0} - edit a vehicle design"),
            PlannerPage::Decorations => format!("${money:.0} - pick a decoration"),
            PlannerPage::Music => "Edit the city's anthem".into(),
            PlannerPage::Outfits => "Dress your citizens".into(),
        },
    };
    for mut text in &mut notes {
        if text.0 != note {
            text.0 = note.clone();
        }
    }
}

fn bind_stats(ui: &mut SpuiState, s: &PlannerStats, at: Vec2) {
    let Some(root) = ui.find(id::STATS_ROOT) else {
        return;
    };
    let a = ui.layout.window(root).area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    ui.set_area(root, [at.x, at.y, at.x + w, at.y + h]);
    if let Some(w) = ui.find(id::STATS_NAME) {
        ui.set_text(w, s.name.clone());
    }
    // The name sits on a plate in the nation's colour (as on the city card).
    for control in id::STATS_PLATES {
        for w in ui.find_all(control) {
            ui.set_tint(w, s.color);
        }
    }
    if let Some(w) = ui.find(id::STATS_FACE) {
        ui.set_frame_grid(w, 5, 1);
        ui.set_frame(w, Some(crate::civ_hud::relation_frame(s.relation)));
    }
    let shown = match s.kind {
        Approach::Economic => 0,
        Approach::Religious => 1,
        Approach::Military => 2,
    };
    for (i, control) in id::STATS_KINDS.iter().enumerate() {
        crate::civ_hud::set_visible(ui, *control, i == shown);
    }
    if let Some(w) = ui.find(id::STATS_POPULATION) {
        ui.set_text(w, s.population.to_string());
    }
    if let Some(w) = ui.find(id::STATS_CAPACITY) {
        ui.set_text(w, s.capacity.to_string());
    }
    if let Some(w) = ui.find(id::STATS_INCOME) {
        ui.set_text(w, format!("{:.0}/min", s.per_minute));
    }
    for (i, control) in id::STATS_MOOD.iter().enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_frame_grid(w, 3, 1);
            ui.set_frame(w, Some(mood_icon_frame(i as u32, s.mood_delta)));
        }
    }
    for (i, control) in id::STATS_DEFENCE.iter().enumerate() {
        if let Some(w) = ui.find(*control) {
            ui.set_frame_grid(w, 2, 1);
            ui.set_frame(w, Some(u32::from((i as u32) < s.defence)));
        }
    }
}

/// The slots (rings: free white, built in the building's colour, hovered
/// gold) and the links in the legend's colours; with an armed building over
/// a free slot, the links as they would be.
fn draw_slots(planner: Res<CivPlanner>, run: Res<RunProgress>, mut gizmos: Gizmos<PlannerGizmos>) {
    let Some(c) = planner.city else {
        return;
    };
    let Some(city) = run.civ_campaign.cities.get(c) else {
        return;
    };
    let shape = run.civ_campaign.shape();
    let trial = planner.armed.and_then(|item| match item {
        PlannerItem::Building(_) => preview(city, item, planner.hover),
        _ => None,
    });
    let shown = trial.as_ref().unwrap_or(city);
    let lift = 0.35 * CITY_SCALE;
    // The decoration spots while a decoration is armed (taken spots dim).
    if let Some(PlannerItem::Decoration(_)) = planner.armed {
        let up = shape.up(city.center());
        let side = up.any_orthonormal_vector();
        let fwd = up.cross(side);
        for spot in 0..city.layout().decoration_spots() {
            let at = decor_world(city, spot, shape) + up * lift;
            let taken = city.decorations.get(spot).copied().flatten().is_some();
            let color = if planner.hover_spot == Some(spot) {
                Color::srgb(1.0, 0.85, 0.3)
            } else if taken {
                Color::srgba(0.6, 0.8, 0.6, 0.6)
            } else {
                Color::srgba(1.0, 1.0, 1.0, 0.8)
            };
            let r = 0.6 * CITY_SCALE;
            let points: Vec<Vec3> = (0..=20)
                .map(|k| {
                    let t = k as f32 / 20.0 * std::f32::consts::TAU;
                    at + (side * t.cos() + fwd * t.sin()) * r
                })
                .collect();
            gizmos.linestrip(points, color);
        }
    }
    let node_at = |n: usize| -> Vec3 {
        let site = if n == 0 { Vec2::ZERO } else { city.plot_site(n - 1) };
        shape.local_point(city.center(), Vec3::new(site.x, 0.0, site.y) * CITY_SCALE) + shape.up(city.center()) * lift
    };
    for (a, b, kind) in links(shown) {
        let (pa, pb) = (node_at(a), node_at(b));
        gizmos.line(pa, pb, kind.color());
    }
    let up = shape.up(city.center());
    let side = up.any_orthonormal_vector();
    let fwd = up.cross(side);
    for plot in 0..city.plots.len() {
        let at = node_at(plot + 1);
        let color = if planner.hover == Some(plot) {
            Color::srgb(1.0, 0.85, 0.3)
        } else if city.standing(plot) {
            match city.plots[plot] {
                Some(Building::House) => Color::srgb(0.85, 0.85, 0.95),
                Some(Building::Factory) => Color::srgb(0.25, 0.55, 1.0),
                Some(Building::Entertainment) => Color::srgb(0.3, 0.95, 0.35),
                None => Color::WHITE,
            }
        } else if city.ruined(plot) {
            Color::srgb(0.35, 0.3, 0.28)
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.8)
        };
        let r = 1.15 * CITY_SCALE;
        let points: Vec<Vec3> = (0..=24)
            .map(|k| {
                let t = k as f32 / 24.0 * std::f32::consts::TAU;
                at + (side * t.cos() + fwd * t.sin()) * r
            })
            .collect();
        gizmos.linestrip(points, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::HOME_INDEX;

    #[test]
    fn links_take_the_legend_colours_of_the_city_model() {
        use CityBuilding::*;
        assert_eq!(link_kind(Some(Factory), Some(House)), LinkKind::Income);
        assert_eq!(link_kind(Some(CityHall), Some(Factory)), LinkKind::Income);
        assert_eq!(link_kind(Some(Entertainment), Some(CityHall)), LinkKind::Happy);
        assert_eq!(link_kind(Some(Factory), Some(Entertainment)), LinkKind::Unhappy);
        assert_eq!(link_kind(Some(House), Some(House)), LinkKind::Neutral);
        assert_eq!(link_kind(None, Some(Factory)), LinkKind::Neutral);
        // The starter capital (the authored slots): the factory on plot 2
        // (slot 3) links to the hall, and so does the entertainment building
        // on plot 4 (slot 5).
        let city = CityState::new(HOME_INDEX);
        let l = links(&city);
        assert!(l.contains(&(0, 3, LinkKind::Income)), "{l:?}");
        assert!(l.contains(&(0, 5, LinkKind::Happy)), "entertainment on plot 4");
        // Blue links pay: the model agrees with the colours.
        let blue = l.iter().filter(|x| x.2 == LinkKind::Income).count() as i32;
        assert_eq!(city.model().money, blue * civ_rules::FACTORY_LINK_MONEY);
    }

    #[test]
    fn the_preview_shows_the_city_with_the_armed_building() {
        let city = CityState::new(HOME_INDEX);
        let free = (0..city.plots.len()).find(|p| city.plots[*p].is_none()).unwrap();
        let taken = (0..city.plots.len()).find(|p| city.plots[*p].is_some()).unwrap();
        assert!(preview(&city, PlannerItem::Building(Building::House), Some(taken)).is_none());
        assert!(preview(&city, PlannerItem::Building(Building::House), None).is_none());
        // A factory beside the starter factory's neighbours earns more.
        let trial = preview(&city, PlannerItem::Building(Building::Factory), Some(free)).unwrap();
        assert_eq!(trial.built(), city.built() + 1);
        assert!(trial.model().money >= city.model().money);
        assert_eq!(stats(&trial).name, city.label());
        // The panel's orbs follow the city card's rule: the link model's
        // happy - unhappy lights that many faces, the turrets light the
        // defence orbs one by one.
        assert_eq!([0, 1, 2, 3, 4].map(|i| mood_icon_frame(i, 0)), [0; 5], "an even city lights nothing");
        assert_eq!([0, 1, 2, 3, 4].map(|i| mood_icon_frame(i, 2)), [2, 2, 0, 0, 0]);
        assert_eq!([0, 1, 2, 3, 4].map(|i| mood_icon_frame(i, -3)), [1, 1, 1, 0, 0]);
        assert_eq!([0, 1, 2, 3, 4].map(|i| mood_icon_frame(i, 9)), [2; 5]);
        let model = city.model();
        assert_eq!(stats(&city).mood_delta, model.happy - model.unhappy);
        assert_eq!(stats(&city).defence, city.live_turrets().min(5) as u32);
        assert_eq!(crate::civ_hud::relation_frame(stats(&city).relation), 2, "your own city shows the neutral face");
        // Turrets go to the walls' mounts until all of them stand.
        let mut walls = city.clone();
        while let Some(next) = preview(&walls, PlannerItem::Turret, None) {
            walls = next;
        }
        assert_eq!(walls.defense as usize, walls.max_turrets());
    }

    #[test]
    fn the_other_pages_follow_the_installed_palette() {
        use crate::creature_editor_data::SetupKind;
        let mut pages = PlannerPage::ALL;
        pages.sort_by_key(|p| p.sequence());
        assert_eq!(pages, PlannerPage::ALL, "installed category order");
        assert_eq!(PlannerPage::Vehicles.title(Approach::Religious), "Cultural Vehicles");
        let swatches = vehicle_swatches(Approach::Economic);
        assert_eq!(swatches.map(|s| s.2), [1000.0, 1500.0, 3000.0, 0.0]);
        assert_eq!(swatches[2].1, "Economic Air");
        let campaign = civ::CivCampaign::default();
        assert!(vehicle_unlocked(&campaign, Some(VehicleDomain::Land)));
        assert!(!vehicle_unlocked(&campaign, None), "the Spaceship waits for Space");
        assert_eq!(building_class(None), SetupKind::BuildingCityHall);
        assert_eq!(building_class(Some(Building::Factory)), SetupKind::BuildingFactory);
        // Decorations cost 25 and take their spot.
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        run.spice = 30.0;
        let line = place_decoration(&mut run, HOME_INDEX, 4, 3).unwrap();
        assert!(line.contains("Irrepressible Illumination"), "{line}");
        assert_eq!(run.spice, 5.0);
        assert_eq!(run.civ_campaign.cities[HOME_INDEX].decorations[3], Some(4));
        assert!(place_decoration(&mut run, HOME_INDEX, 4, 5).unwrap().contains("costs"));
        assert_eq!(place_decoration(&mut run, 2, 4, 5), None, "not yours");
    }

    #[test]
    fn change_cities_cycles_through_yours() {
        let mut cities: Vec<CityState> = (0..civ::CITY_COUNT).map(CityState::new).collect();
        cities[2].owner = civ::CityOwner::Player;
        cities[4].owner = civ::CityOwner::Player;
        assert_eq!(cycle_city(&cities, 0, true), 2);
        assert_eq!(cycle_city(&cities, 2, true), 4);
        assert_eq!(cycle_city(&cities, 4, true), 0);
        assert_eq!(cycle_city(&cities, 0, false), 4);
        assert_eq!(cycle_city(&cities, 3, true), 0, "a rival city falls back to your first");
    }

    #[test]
    fn slot_clicks_buy_on_free_slots_and_sell_standing_buildings() {
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        run.spice = 10_000.0;
        let city = &run.civ_campaign.cities[HOME_INDEX];
        let free = (0..city.plots.len()).find(|p| city.plots[*p].is_none()).unwrap();
        let built = (0..city.plots.len()).find(|p| city.standing(*p)).unwrap();
        let before = city.built();
        let medium = civ_rules::Difficulty::Medium;
        let house = Some(PlannerItem::Building(Building::House));
        // Nothing armed: a left click does nothing.
        assert_eq!(slot_action(&mut run, HOME_INDEX, None, free, SlotAction::Place, medium), None);
        let line = slot_action(&mut run, HOME_INDEX, house, free, SlotAction::Place, medium).unwrap();
        assert!(line.contains("House built"), "{line}");
        assert_eq!(run.civ_campaign.cities[HOME_INDEX].built(), before + 1);
        assert_eq!(run.spice, 10_000.0 - civ_rules::HOUSE_COST);
        // A taken slot refuses.
        let line = slot_action(&mut run, HOME_INDEX, house, free, SlotAction::Place, medium).unwrap();
        assert!(line.contains("taken"), "{line}");
        // Selling refunds 75 % on medium.
        let kind = run.civ_campaign.cities[HOME_INDEX].plots[built].unwrap();
        let spice = run.spice;
        slot_action(&mut run, HOME_INDEX, None, built, SlotAction::Sell, medium).unwrap();
        assert_eq!(run.spice, spice + kind.cost() * 0.75);
        assert_eq!(run.civ_campaign.cities[HOME_INDEX].plots[built], None);
        assert_eq!(slot_action(&mut run, HOME_INDEX, None, built, SlotAction::Sell, medium), None);
    }

    #[test]
    fn the_planner_opens_on_your_city_and_esc_closes_it() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        app.update();
        app.update();
        // A rival city is not yours to plan.
        app.world_mut().resource_mut::<CivPlanner>().request = Some(2);
        app.update();
        assert!(!app.world().resource::<CivPlanner>().is_open());
        app.world_mut().resource_mut::<CivPlanner>().request = Some(HOME_INDEX);
        app.update();
        assert_eq!(app.world().resource::<CivPlanner>().city, Some(HOME_INDEX));
        // Esc closes the planner without opening the pause menu.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Escape);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        app.update();
        assert!(!app.world().resource::<CivPlanner>().is_open());
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
    }
}
