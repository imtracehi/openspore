//! The Civilization context cursors: what the mouse pointer looks like over
//! the planet, by the selected vehicles and the object under the pointer
//! (helper `civ-click`, round 2).
//!
//! Tags: **VERIFIED** (code image, march2017 / SDK addresses, disassembled
//! with `swarm-volume\target\re\dis.py`), **INFERRED** (reading of the
//! neighbouring code), **ADAPTED** (OpenSpore choice).
//!
//! Retail: the Civ game-mode UI class (vtable 0x1481B84, constructor 0xE34EF0)
//! owns the cursor. Its start-up (0xE35040) registers eight cursors with
//! `cCursorManager::Load(id, name, true, 0, 0)`: `cursor_atk` 0x3570021,
//! `cursor-no-opt` 0x3A204B0, `cursor-more` 0x3FEBE41, `cursor_goodie-hut`
//! 0x56E3A22, `cursor-claim-spicemine` 0x5D53800, `cursor-camera-translate`
//! 0x600CD69, `cursor-camera-aim` 0x600CD6A and `cur-planner` 0x65245DD (files
//! of group `cursors~`, resource type `cur`; the Tribal port in
//! `tribal_hud::cursor` loads them the same way). Its per-frame update
//! (0xE35320, vtable +0x10; the "CU_*" label of the earlier notes was a UI
//! neighbour, the cursor rules are here) sets `cursor-default` (0x1002) and then
//! overrides it. VERIFIED order, with `S` = the vehicles of the terrain cursor
//! (`cGameTerrainCursor` +0x6C counts type 0x137E8E0, +0x60 lists them):
//!
//! ```text
//! the two ability modes (0xE09520 / 0xDD11C0: the Tribe stance powers)    spyglass / ban cursors (never on in Civ)
//! the drag box is being drawn (+0xA8)                                     default
//! the camera is drag-scrolling (0xCF1140: [+0xC0] | [+0xC4])              cursor-camera-translate
//! an ability tool is armed (0xB7C8D0)                                     cursor-more
//! the hovered object, cast to:
//!   city hall (0xCF7E80, type 0x1007AE63)  a city that is not the player's -> cursor-more (any selection)
//!   building (0xB67820) / turret (0xB18660)  not the player's, S holds a military (purpose 0) or religious
//!                      (purpose 1) vehicle and (one that is not a boat (locomotion != 1), or the building's
//!                      city has a boat spawn point, `cCity` +0x32C != the default: a coastal city) -> cursor_atk
//!   vehicle (0xAE66E0)                     not the player's, S holds a military or religious vehicle -> cursor_atk
//!   animal (0xAC88A0, a creature)          alive, S holds a military or religious vehicle -> cursor_atk
//!   commodity node (0xE34ED0, a spice geyser) with S: unclaimed -> claim-spicemine when a non-air vehicle
//!                      can reach it else no-opt; the player's -> default; another civ's: an economic vehicle
//!                      that can reach it -> cursor-more, else a military / religious one -> cursor_atk,
//!                      else no-opt
//!   tribe hut (0xB677E0)                   S can reach it and the tribe's flag +0x557 is clear -> goodie-hut else no-opt
//!   nothing: S holds a vehicle that can reach the point (0xC9EAF0) -> default, else no-opt
//! ```
//! `0xC9EAF0(vehicle, point)` = the terrain flags at the point allow the
//! vehicle's locomotion (0xC9EA70: land `flags & 0x98000000 == 0`, water
//! `flags & 0x58000000 == 0`, air `bit 29` clear) and, for land and water, the
//! landmass index under the vehicle (0xB886E0) equals the one under the point.
//!
//! What this port does with it: [`choose_action`] is the rule table above for
//! the objects OpenSpore has (ground, vehicles, epic predators, city halls,
//! buildings and turrets, geysers, goodie huts: the tribe hut branch is
//! `Target::Hut`, the huts are `civ_planet::hut_sites`, helper `civ-goodies`). ADAPTED: the
//! reach test is `Globe::is_land` per locomotion plus the 2-degree landmass
//! labels of `civ_path` (no slope / terrain flag bits), the picking (a pointer
//! ray on the city picks the nearest of the hall, a standing building or a
//! turret; vehicles and epics by screen distance), the combatant state test
//! (0x8E8230 on buildings and turrets: "destroyed" is not tracked per object
//! here), the epic hover sound / message.
//!
//! The right click (`civ_rts::rts_input`) reads the same [`CivHover`]: an order
//! the cursor marks with the no-entry cursor is refused. The retail order side
//! (the Civ UI's command message handler) was not traced, so the order
//! mapping itself is OpenSpore's (ADAPTED, see CHANGES.md).

use bevy::{
    asset::RenderAssetUsages,
    ecs::system::SystemParam,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::{CursorIcon, PrimaryWindow},
};
use std::collections::HashMap;

use crate::{
    civ::{Approach, CivCampaign, CivCommander, VehicleDomain, HOME_INDEX},
    civ_path::NavGrid,
    civ_planet::{Globe, Shape, CITY_SCALE},
    civ_rts::{self, CivUnit, RtsState, Selected},
    civ_wildlife::CivEpic,
    game::RunProgress,
    stages::GameStage,
    tribal_hud::cursor::{cursor_key, decode_cursor},
};

/// The cursor ids of the Civ start-up (0xE35040) and their files.
pub mod cursor_id {
    pub const DEFAULT: u32 = 0x1002;
    pub const ATTACK: u32 = 0x357_0021;
    pub const NO_OPT: u32 = 0x3A2_04B0;
    pub const MORE: u32 = 0x3FE_BE41;
    pub const GOODIE_HUT: u32 = 0x56E_3A22;
    pub const CLAIM_SPICEMINE: u32 = 0x5D5_3800;
    pub const PAN: u32 = 0x600_CD69;
    pub const AIM: u32 = 0x600_CD6A;
    pub const PLANNER: u32 = 0x652_45DD;

    /// The `cursors~` file a registered id loads (`None` = the default arrow).
    pub fn file(id: u32) -> Option<&'static str> {
        Some(match id {
            ATTACK => "cursor_atk",
            NO_OPT => "cursor-no-opt",
            MORE => "cursor-more",
            GOODIE_HUT => "cursor_goodie-hut",
            CLAIM_SPICEMINE => "cursor-claim-spicemine",
            PAN => "cursor-camera-translate",
            AIM => "cursor-camera-aim",
            PLANNER => "cur-planner",
            _ => return None,
        })
    }
}

/// What the pointer offers, in the retail cursor ids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivAction {
    /// `cursor-default`: nothing to do here (or open ground the selection can walk to).
    #[default]
    Default,
    /// The camera is drag-scrolling.
    Pan,
    /// An ability tool is armed (or a not-yours city: the "more" info cursor).
    More,
    /// `cursor_atk`: military / religious vehicles attack or convert.
    Attack,
    /// `cursor-claim-spicemine`.
    ClaimGeyser,
    /// `cursor_goodie-hut`: the selection can capture the hut.
    GoodieHut,
    /// `cursor-no-opt`: the selection cannot do it.
    Invalid,
}

impl CivAction {
    /// The retail cursor id the action sets.
    pub const fn cursor(self) -> u32 {
        match self {
            CivAction::Default => cursor_id::DEFAULT,
            CivAction::Pan => cursor_id::PAN,
            CivAction::More => cursor_id::MORE,
            CivAction::Attack => cursor_id::ATTACK,
            CivAction::ClaimGeyser => cursor_id::CLAIM_SPICEMINE,
            CivAction::GoodieHut => cursor_id::GOODIE_HUT,
            CivAction::Invalid => cursor_id::NO_OPT,
        }
    }
}

/// Who holds a spice geyser (retail: the commodity node's political id: -1
/// unclaimed, the player's id, another civ's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeyserHolder {
    Nobody,
    Player,
    Rival,
}

/// Which selected non-air vehicles can reach a point (the loop at
/// 0xE359A2: `any`, `strike` = a military or religious one, `economic`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reach {
    pub any: bool,
    pub strike: bool,
    pub economic: bool,
    /// A selected aircraft (the hut rule asks the plain reach test 0xC9EAF0, which air passes; the
    /// geyser rule above counts non-air vehicles only).
    pub air: bool,
}

/// The retail nouns of the hovered object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Open ground / sea; `reachable` = a selected vehicle can get there.
    Ground { reachable: bool },
    /// A vehicle (`own` = the player's).
    Vehicle { own: bool },
    /// An epic predator (retail: a creature animal); `alive`.
    Wild { alive: bool },
    /// The hall of a city (`owned` = the player's).
    CityHall { owned: bool },
    /// A building or a turret of a city; `coastal` = the city has a boat
    /// spawn point (retail `cCity` +0x32C, VERIFIED 0xE35C34: boats may
    /// attack its buildings).
    Structure { owned: bool, coastal: bool },
    /// A spice geyser with its derrick state and who can reach it.
    Geyser { holder: GeyserHolder, reach: Reach },
    /// A goodie hut that still stands (retail: a tribe hut whose tribe flag +0x557 is clear).
    Hut { reach: Reach },
}

impl Default for Target {
    fn default() -> Self {
        Target::Ground { reachable: true }
    }
}

/// What the selected vehicles are (the flags of the loop at 0xE35576, retail
/// `[esp+0x13..0x15]`: military = purpose 0, religious = purpose 1, and
/// `non_sea` = a locomotion other than water).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub count: usize,
    pub military: bool,
    pub religious: bool,
    pub non_sea: bool,
}

impl Selection {
    pub fn add(&mut self, kind: Approach, domain: VehicleDomain) {
        self.count += 1;
        self.military |= kind == Approach::Military;
        self.religious |= kind == Approach::Religious;
        self.non_sea |= domain != VehicleDomain::Sea;
    }

    /// Military or religious vehicles are in the selection (0xE358A4).
    pub fn strikes(&self) -> bool {
        self.military || self.religious
    }
}

/// `0xE35320`, the rules above in the order of the code: a drag-scrolling
/// camera, an armed ability tool, then the hovered object.
pub fn choose_action(target: Target, sel: &Selection, panning: bool, armed: bool) -> CivAction {
    if panning {
        return CivAction::Pan;
    }
    if armed {
        return CivAction::More;
    }
    // The object rules read the vehicles of the terrain cursor (retail tcMode 1).
    let vehicles = sel.count > 0;
    match target {
        // A city that is not the player's shows the "more" cursor whatever is selected (0xE356F3).
        Target::CityHall { owned } => {
            if owned {
                CivAction::Default
            } else {
                CivAction::More
            }
        }
        Target::Geyser { holder, reach } if vehicles => match holder {
            GeyserHolder::Nobody => {
                if reach.any {
                    CivAction::ClaimGeyser
                } else {
                    CivAction::Invalid
                }
            }
            GeyserHolder::Player => CivAction::Default,
            GeyserHolder::Rival => {
                if reach.economic {
                    CivAction::More
                } else if reach.strike {
                    CivAction::Attack
                } else {
                    CivAction::Invalid
                }
            }
        },
        // The tribe hut branch of 0xE35320: a selection that can reach it -> goodie-hut, else no-opt.
        Target::Hut { reach } if vehicles => {
            if reach.any || reach.air {
                CivAction::GoodieHut
            } else {
                CivAction::Invalid
            }
        }
        Target::Vehicle { own: false } | Target::Wild { alive: true } if vehicles && sel.strikes() => {
            CivAction::Attack
        }
        // Buildings and turrets need a vehicle that is not a boat, or a coastal city (0xE35C42:
        // `[esp+0x15]` non-boat flag, else the city's boat spawn point differs from the default).
        Target::Structure { owned: false, coastal } if vehicles && sel.strikes() && (sel.non_sea || coastal) => {
            CivAction::Attack
        }
        Target::Ground { reachable: false } if vehicles => CivAction::Invalid,
        _ => CivAction::Default,
    }
}

/// The message of a refused right click, if the cursor marks it so.
pub fn refusal(action: CivAction, target: Target) -> Option<&'static str> {
    (action == CivAction::Invalid).then_some(match target {
        Target::Geyser { holder: GeyserHolder::Nobody, .. } => "None of the selected vehicles can reach that geyser.",
        Target::Geyser { .. } => "None of the selected vehicles can do that to this derrick.",
        Target::Hut { .. } => "None of the selected vehicles can reach that goodie hut.",
        _ => "The selected vehicles cannot go there.",
    })
}

// ---------------------------------------------------------------------------
// The world side
// ---------------------------------------------------------------------------

/// The hover state of this frame: read by the right click.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct CivHover {
    pub action: CivAction,
    pub target: Target,
}

/// The planet's landmass labels, built once per layout (see
/// `NavGrid::regions`).
pub struct Regions {
    key: u64,
    grid: NavGrid,
    land: Vec<u32>,
    sea: Vec<u32>,
}

impl Regions {
    pub fn build(globe: &Globe, key: u64) -> Option<Self> {
        let grid = NavGrid::build(globe)?;
        let (land, sea) = (grid.regions(true), grid.regions(false));
        Some(Self { key, grid, land, sea })
    }

    /// The layout key the labels were built for (`civ-orders` rebuilds its own copy by it).
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The landmass label of a point (0 = no land near; the stand-in of 0xB886E0's continent id).
    pub fn land_region(&self, p: Vec3) -> u32 {
        self.grid.region_at(&self.land, p, true)
    }
}

/// `0xC9EA70` + the landmass test of `0xC9EAF0`: the point's medium suits the
/// locomotion, and land and sea vehicles stay on one landmass / water body.
/// Without a planet everything can be reached (ADAPTED: the flat fallback has
/// no coast).
pub fn medium_reach(regions: Option<&Regions>, globe: &Globe, domain: VehicleDomain, from: Vec3, to: Vec3) -> bool {
    if !globe.on_planet() {
        return true;
    }
    let land = match domain {
        VehicleDomain::Air => return true,
        VehicleDomain::Land => true,
        VehicleDomain::Sea => false,
    };
    if globe.is_land(to) != land {
        return false;
    }
    match regions {
        Some(r) => {
            let labels = if land { &r.land } else { &r.sea };
            let (a, b) = (r.grid.region_at(labels, from, land), r.grid.region_at(labels, to, land));
            a != 0 && a == b
        }
        None => true,
    }
}

/// Radii of the pointer picks (the same ones `civ_rts::rts_input` and
/// `civ_controls::pointer_state` use for geysers and cities).
const GEYSER_RADIUS: f32 = 3.2 * CITY_SCALE;
/// ADAPTED: the pick radius of a goodie hut (the hut model is about 4 units wide).
pub const HUT_RADIUS: f32 = 2.4 * CITY_SCALE;
const CITY_RADIUS: f32 = 7.0 * CITY_SCALE;

/// What the pointer's ground point is on: the nearest of a city's hall, a
/// standing building or a living turret inside the city circle; a geyser; else
/// open ground. ADAPTED: retail picks the 3D object under the pointer.
pub fn pick_ground(
    campaign: &CivCampaign,
    shape: Shape,
    ground: Vec3,
    reach: impl Fn(Vec3) -> Reach,
    ground_reachable: impl Fn(Vec3) -> bool,
) -> Target {
    for g in 0..crate::civ::GEYSER_COUNT {
        let at = campaign.geyser_at(g);
        if shape.distance(ground, at) <= GEYSER_RADIUS {
            let holder = match campaign.geysers.get(g).and_then(|s| s.owner) {
                None => GeyserHolder::Nobody,
                Some(HOME_INDEX) => GeyserHolder::Player,
                Some(_) => GeyserHolder::Rival,
            };
            return Target::Geyser { holder, reach: reach(at) };
        }
    }
    for h in 0..campaign.hut_sites.len() {
        if campaign.hut_standing(h) && campaign.hut_at(h).is_some_and(|at| shape.distance(ground, at) <= HUT_RADIUS) {
            return Target::Hut { reach: reach(campaign.hut_at(h).unwrap_or(ground)) };
        }
    }
    for city in &campaign.cities {
        if shape.distance(ground, city.center()) > CITY_RADIUS {
            continue;
        }
        let owned = city.owned();
        let coastal = campaign.is_coastal(city.index);
        // The hall stands in the middle; buildings and turrets around it.
        let mut best = (shape.distance(ground, city.center()), Target::CityHall { owned });
        for plot in 0..city.plots.len() {
            if city.standing(plot) {
                let d = shape.distance(ground, civ_rts::plot_world(city, plot, shape));
                if d < best.0 {
                    best = (d, Target::Structure { owned, coastal });
                }
            }
        }
        for turret in 0..city.turret_hp.len() {
            if city.turret_hp[turret] > 0.0 {
                let d = shape.distance(ground, civ_rts::turret_world(city, turret, shape));
                if d < best.0 {
                    best = (d, Target::Structure { owned, coastal });
                }
            }
        }
        return best.1;
    }
    Target::Ground { reachable: ground_reachable(ground) }
}

/// Screen distance within which a vehicle or an epic is picked.
pub const PICK_PIXELS: f32 = 30.0;
pub const EPIC_PIXELS: f32 = 44.0;

/// The decoded cursors by file name (`None` = not readable: the default arrow).
#[derive(Default)]
pub struct IconCache {
    images: HashMap<&'static str, Option<(Handle<Image>, (u16, u16))>>,
}

/// The state outside the world that the cursor reads.
#[derive(SystemParam)]
struct Flags<'w> {
    spui: Option<Res<'w, crate::spui::view::SpuiPointer>>,
    controls: Option<Res<'w, crate::civ_controls::CivControls>>,
    state: Option<Res<'w, RtsState>>,
    planner: Option<Res<'w, crate::civ_planner::CivPlanner>>,
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    flags: Flags,
    run: Res<RunProgress>,
    units: Query<(&CivUnit, &Transform, Has<Selected>), Without<CivCommander>>,
    epics: Query<(&CivEpic, &Transform)>,
    originals: Option<Res<crate::Originals>>,
    images: Option<ResMut<Assets<Image>>>,
    mut hover: ResMut<CivHover>,
    mut cache: Local<IconCache>,
    mut regions: Local<Option<Regions>>,
    mut shown: Local<Option<u32>>,
) {
    let campaign = &run.civ_campaign;
    let shape = campaign.shape();
    let surface = surface.as_deref().filter(|s| s.stage == GameStage::Civilization);
    let globe = Globe::from_surface(surface);
    if globe.on_planet() && regions.as_ref().map(|r| r.key) != Some(campaign.layout_key) {
        *regions = Regions::build(&globe, campaign.layout_key);
    }
    let regions_ref = regions.as_ref().filter(|_| globe.on_planet());
    let mut sel = Selection::default();
    let mut selected: Vec<(VehicleDomain, Approach, Vec3)> = Vec::new();
    for (unit, t, is_selected) in &units {
        if is_selected && unit.nation == HOME_INDEX {
            sel.add(unit.kind, unit.domain);
            selected.push((unit.domain, unit.kind, t.translation));
        }
    }
    let panning = flags.controls.as_deref().is_some_and(|c| c.panning);
    let armed = flags.state.as_deref().is_some_and(|s| s.armed.is_some());
    let mut now = CivHover::default();
    'pick: {
        let Some((_, window)) = windows.iter().next() else { break 'pick };
        let Some(pointer) = window.cursor_position() else { break 'pick };
        if flags.spui.as_deref().is_some_and(|s| s.over_ui) || flags.planner.as_deref().is_some_and(|p| p.is_open()) {
            break 'pick;
        }
        let Ok((camera, camera_transform)) = cameras.single() else { break 'pick };
        let eye = camera_transform.translation();
        let project = |p: Vec3| {
            shape
                .visible_from(eye, p)
                .then(|| camera.world_to_viewport(camera_transform, p).ok())
                .flatten()
        };
        let reach_of = |to: Vec3| {
            let mut reach = Reach::default();
            for &(domain, kind, from) in &selected {
                reach.air |= domain == VehicleDomain::Air;
                if domain != VehicleDomain::Air && medium_reach(regions_ref, &globe, domain, from, to) {
                    reach.any = true;
                    reach.strike |= kind != Approach::Economic;
                    reach.economic |= kind == Approach::Economic;
                }
            }
            reach
        };
        let ground_reachable = |to: Vec3| {
            selected.is_empty()
                || selected.iter().any(|&(domain, _, from)| medium_reach(regions_ref, &globe, domain, from, to))
        };
        // A vehicle under the pointer (any nation), then an epic predator.
        let vehicle = units
            .iter()
            .filter_map(|(u, t, _)| {
                let d = project(t.translation + Vec3::Y * 0.6)?.distance(pointer);
                (d <= PICK_PIXELS).then_some((u.nation == HOME_INDEX, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((own, _)) = vehicle {
            now.target = Target::Vehicle { own };
            break 'pick;
        }
        let epic = epics
            .iter()
            .filter_map(|(e, t)| {
                let d = project(t.translation + Vec3::Y * 1.5)?.distance(pointer);
                (d <= EPIC_PIXELS).then_some((e.hp > 0.0, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((alive, _)) = epic {
            now.target = Target::Wild { alive };
            break 'pick;
        }
        let ground = camera
            .viewport_to_world(camera_transform, pointer)
            .ok()
            .and_then(|ray| civ_rts::ground_hit(ray.origin, *ray.direction, surface));
        if let Some(ground) = ground {
            now.target = pick_ground(campaign, shape, ground, reach_of, ground_reachable);
        }
    }
    now.action = choose_action(now.target, &sel, panning, armed);
    if *hover != now {
        *hover = now;
    }
    // Set the window cursor when the wanted one changes.
    let want = now.action.cursor();
    let (Some(originals), Some(mut images)) = (originals, images) else { return };
    if *shown == Some(want) {
        return;
    }
    let icon = match cursor_id::file(want) {
        None => CursorIcon::default(),
        Some(name) => {
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
                Some((handle, hotspot)) => CursorIcon::Custom(bevy::window::CustomCursor::Image(
                    bevy::window::CustomCursorImage { handle: handle.clone(), hotspot: *hotspot, ..default() },
                )),
                None => CursorIcon::default(),
            }
        }
    };
    for (window, _) in &windows {
        commands.entity(window).insert(icon.clone());
    }
    *shown = Some(want);
}

/// Back to the default arrow on leaving the Civ stage.
fn reset(mut commands: Commands, windows: Query<Entity, With<PrimaryWindow>>, mut hover: ResMut<CivHover>) {
    *hover = CivHover::default();
    for window in &windows {
        commands.entity(window).insert(CursorIcon::default());
    }
}

pub struct CivCursorPlugin;

impl Plugin for CivCursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CivHover>()
            .add_systems(OnExit(GameStage::Civilization), reset)
            .add_systems(
                Update,
                update
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CivAction::*;

    fn sel(units: &[(Approach, VehicleDomain)]) -> Selection {
        let mut s = Selection::default();
        for &(kind, domain) in units {
            s.add(kind, domain);
        }
        s
    }

    const LAND_MIL: (Approach, VehicleDomain) = (Approach::Military, VehicleDomain::Land);
    const LAND_REL: (Approach, VehicleDomain) = (Approach::Religious, VehicleDomain::Land);
    const LAND_ECO: (Approach, VehicleDomain) = (Approach::Economic, VehicleDomain::Land);
    const SEA_MIL: (Approach, VehicleDomain) = (Approach::Military, VehicleDomain::Sea);
    const SEA_ECO: (Approach, VehicleDomain) = (Approach::Economic, VehicleDomain::Sea);

    #[test]
    fn the_cursor_ids_are_the_start_up_ids_of_0xe35040_and_name_installed_files() {
        assert_eq!(cursor_id::file(cursor_id::ATTACK), Some("cursor_atk"));
        assert_eq!(cursor_id::file(cursor_id::NO_OPT), Some("cursor-no-opt"));
        assert_eq!(cursor_id::file(cursor_id::MORE), Some("cursor-more"));
        assert_eq!(cursor_id::file(cursor_id::CLAIM_SPICEMINE), Some("cursor-claim-spicemine"));
        assert_eq!(cursor_id::file(cursor_id::PAN), Some("cursor-camera-translate"));
        assert_eq!(cursor_id::file(cursor_id::DEFAULT), None);
        // The same Load(id, name) pairs the Tribal start-up uses for the shared files.
        assert_eq!(cursor_id::ATTACK, 0x3570021);
        assert_eq!(cursor_id::PAN, 0x600CD69);
        assert_eq!(CivAction::Default.cursor(), 0x1002);
    }

    #[test]
    fn panning_and_an_armed_tool_come_before_every_object_rule() {
        let s = sel(&[LAND_MIL]);
        let enemy = Target::Vehicle { own: false };
        assert_eq!(choose_action(enemy, &s, true, true), Pan);
        assert_eq!(choose_action(enemy, &s, false, true), More);
        assert_eq!(choose_action(enemy, &s, false, false), Attack);
    }

    #[test]
    fn ground_is_default_when_a_selected_vehicle_can_reach_it_and_no_opt_otherwise() {
        let s = sel(&[LAND_MIL]);
        assert_eq!(choose_action(Target::Ground { reachable: true }, &s, false, false), Default);
        assert_eq!(choose_action(Target::Ground { reachable: false }, &s, false, false), Invalid);
        // Nothing selected: the plain arrow even over water.
        let none = Selection::default();
        assert_eq!(choose_action(Target::Ground { reachable: false }, &none, false, false), Default);
    }

    #[test]
    fn enemy_vehicles_and_epics_are_attacked_by_military_or_religious_vehicles() {
        let enemy = Target::Vehicle { own: false };
        assert_eq!(choose_action(enemy, &sel(&[LAND_MIL]), false, false), Attack);
        assert_eq!(choose_action(enemy, &sel(&[LAND_REL]), false, false), Attack);
        assert_eq!(choose_action(enemy, &sel(&[LAND_ECO]), false, false), Default);
        assert_eq!(choose_action(Target::Vehicle { own: true }, &sel(&[LAND_MIL]), false, false), Default);
        assert_eq!(choose_action(Target::Wild { alive: true }, &sel(&[LAND_MIL]), false, false), Attack);
        assert_eq!(choose_action(Target::Wild { alive: false }, &sel(&[LAND_MIL]), false, false), Default);
        assert_eq!(choose_action(enemy, &Selection::default(), false, false), Default);
    }

    #[test]
    fn city_halls_show_more_and_structures_need_a_vehicle_that_is_not_a_boat() {
        // 0xE356F3: a city that is not the player's shows "more" with or without a selection.
        let rival = Target::CityHall { owned: false };
        assert_eq!(choose_action(rival, &Selection::default(), false, false), More);
        assert_eq!(choose_action(rival, &sel(&[LAND_ECO]), false, false), More);
        assert_eq!(choose_action(Target::CityHall { owned: true }, &sel(&[LAND_MIL]), false, false), Default);
        // 0xE35C42: buildings and turrets of another civ.
        let wall = Target::Structure { owned: false, coastal: false };
        assert_eq!(choose_action(wall, &sel(&[LAND_MIL]), false, false), Attack);
        assert_eq!(choose_action(wall, &sel(&[LAND_REL]), false, false), Attack);
        assert_eq!(choose_action(wall, &sel(&[LAND_ECO]), false, false), Default);
        assert_eq!(choose_action(wall, &sel(&[SEA_MIL]), false, false), Default);
        assert_eq!(choose_action(wall, &sel(&[SEA_MIL, LAND_MIL]), false, false), Attack);
        // Boats attack the buildings of a coastal city (the city's boat spawn point).
        let shore = Target::Structure { owned: false, coastal: true };
        assert_eq!(choose_action(shore, &sel(&[SEA_MIL]), false, false), Attack);
        assert_eq!(choose_action(shore, &sel(&[SEA_ECO]), false, false), Default);
        assert_eq!(
            choose_action(Target::Structure { owned: true, coastal: true }, &sel(&[LAND_MIL]), false, false),
            Default
        );
    }

    #[test]
    fn geysers_are_claimed_attacked_or_refused_by_who_can_reach_them() {
        let geyser = |holder, any, strike, economic| Target::Geyser {
            holder,
            reach: Reach { any, strike, economic, air: false },
        };
        let s = sel(&[LAND_MIL, LAND_ECO]);
        // Unclaimed: claim when any non-air vehicle gets there, else the no-entry cursor.
        assert_eq!(choose_action(geyser(GeyserHolder::Nobody, true, true, false), &s, false, false), ClaimGeyser);
        assert_eq!(choose_action(geyser(GeyserHolder::Nobody, false, false, false), &s, false, false), Invalid);
        // Yours: nothing to do.
        assert_eq!(choose_action(geyser(GeyserHolder::Player, true, true, true), &s, false, false), Default);
        // Another civ's derrick: an economic vehicle first ("more"), then a fighter, else refused.
        assert_eq!(choose_action(geyser(GeyserHolder::Rival, true, true, true), &s, false, false), More);
        assert_eq!(choose_action(geyser(GeyserHolder::Rival, true, true, false), &s, false, false), Attack);
        assert_eq!(choose_action(geyser(GeyserHolder::Rival, false, false, false), &s, false, false), Invalid);
        // Without vehicles selected a geyser is the plain arrow.
        assert_eq!(
            choose_action(geyser(GeyserHolder::Nobody, false, false, false), &Selection::default(), false, false),
            Default
        );
    }

    #[test]
    fn only_the_no_entry_cursor_refuses_a_click() {
        let ground = Target::Ground { reachable: false };
        assert!(refusal(Invalid, ground).is_some());
        assert!(refusal(Default, Target::Ground { reachable: true }).is_none());
        assert!(refusal(Attack, Target::Vehicle { own: false }).is_none());
        let geyser = Target::Geyser { holder: GeyserHolder::Nobody, reach: Reach::default() };
        assert!(refusal(Invalid, geyser).unwrap().contains("geyser"));
    }

    #[test]
    fn boats_stay_on_their_water_body_and_land_vehicles_on_their_island() {
        // Two islands in the sea (cells 0..30 and 60..90 of longitude).
        let mut land = vec![false; crate::civ_path::ROWS * crate::civ_path::COLS];
        for r in 20..70 {
            for c in (0..30).chain(60..90) {
                land[r * crate::civ_path::COLS + c] = true;
            }
        }
        let grid = NavGrid::from_cells(Vec3::ZERO, 500.0, land, vec![0.0; crate::civ_path::ROWS * crate::civ_path::COLS]);
        let regions = Regions { key: 0, land: grid.regions(true), sea: grid.regions(false), grid: grid.clone() };
        let globe = Globe::flat();
        // Without a planet everything is reachable (the flat fallback).
        assert!(medium_reach(Some(&regions), &globe, VehicleDomain::Sea, Vec3::ZERO, Vec3::X));
        // Region labels separate the islands: the point test runs on the labels.
        let (a, b) = (grid.point(40, 10), grid.point(40, 70));
        assert_ne!(grid.region_at(&regions.land, a, true), grid.region_at(&regions.land, b, true));
        assert_eq!(grid.region_at(&regions.sea, grid.point(10, 5), false), grid.region_at(&regions.sea, grid.point(80, 100), false));
    }

    #[test]
    fn the_ground_point_resolves_to_a_geyser_a_city_object_or_open_ground() {
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        let shape = campaign.shape();
        let reach = |_: Vec3| Reach { any: true, strike: true, economic: false, air: false };
        let pick = |campaign: &CivCampaign, at: Vec3| pick_ground(campaign, shape, at, reach, |_| true);
        // A free geyser, then the player's own derrick, then another civ's.
        let at = campaign.geyser_at(0);
        campaign.geysers[0].owner = None;
        assert!(matches!(pick(&campaign, at), Target::Geyser { holder: GeyserHolder::Nobody, .. }));
        campaign.geysers[0].owner = Some(HOME_INDEX);
        assert!(matches!(pick(&campaign, at), Target::Geyser { holder: GeyserHolder::Player, .. }));
        campaign.geysers[0].owner = Some(3);
        assert!(matches!(pick(&campaign, at), Target::Geyser { holder: GeyserHolder::Rival, .. }));
        campaign.geysers[0].owner = None;
        // The middle of your city is your hall, a rival's is a hall that is not yours.
        let home = campaign.cities[HOME_INDEX].center();
        assert_eq!(pick(&campaign, home), Target::CityHall { owned: true });
        let rival = campaign.cities.iter().find(|c| !c.owned()).expect("a rival city").clone();
        assert_eq!(pick(&campaign, rival.center()), Target::CityHall { owned: false });
        // Its wall turret (a living one) is a structure of another civ.
        let turret = (0..rival.turret_hp.len()).find(|t| rival.turret_hp[*t] > 0.0).expect("a standing turret");
        // (The flat test campaign has no coast: every city counts as coastal.)
        assert_eq!(
            pick(&campaign, civ_rts::turret_world(&rival, turret, shape)),
            Target::Structure { owned: false, coastal: true }
        );
        // Open ground far from every city and geyser.
        let far = campaign.cities.iter().map(|c| c.center()).fold(Vec3::ZERO, |a, b| a + b) * -1.0;
        let far = shape.toward(far, Vec3::X, 0.0);
        let sites: Vec<Vec3> = campaign.cities.iter().map(|c| c.center()).chain((0..crate::civ::GEYSER_COUNT).map(|g| campaign.geyser_at(g))).collect();
        if sites.iter().all(|s| shape.distance(far, *s) > 8.0 * CITY_SCALE) {
            assert!(matches!(pick(&campaign, far), Target::Ground { .. }));
        }
    }

    fn installed() -> Option<crate::asset_catalog::ResourceIndex> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then_some(index)
    }

    #[test]
    fn every_civ_cursor_file_is_an_installed_32x32_cursor() {
        let Some(index) = installed() else { return };
        for id in [
            cursor_id::ATTACK,
            cursor_id::NO_OPT,
            cursor_id::MORE,
            cursor_id::GOODIE_HUT,
            cursor_id::CLAIM_SPICEMINE,
            cursor_id::PAN,
            cursor_id::PLANNER,
        ] {
            let name = cursor_id::file(id).unwrap();
            let bytes = index.read(cursor_key(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            let cur = decode_cursor(&bytes).unwrap_or_else(|| panic!("{name} does not decode"));
            assert_eq!((cur.width, cur.height), (32, 32), "{name}");
            assert!(cur.rgba.chunks(4).any(|p| p[3] > 0), "{name} is blank");
        }
    }

    #[test]
    fn the_hover_system_runs_in_the_civ_scene_and_names_an_unselected_pointer_default() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..4 {
            app.update();
        }
        let hover = *app.world().resource::<CivHover>();
        // No window pointer in a headless run: open ground, the plain arrow.
        assert_eq!(hover.action, CivAction::Default);
    }

    #[test]
    fn a_hut_shows_the_goodie_cursor_to_a_selection_that_reaches_it() {
        let s = sel(&[LAND_ECO]);
        let reach = |any, air| Target::Hut { reach: Reach { any, strike: false, economic: any, air } };
        // 0xE35320: S can reach it -> goodie-hut (any purpose), else no-opt; aircraft pass the plain reach test.
        assert_eq!(choose_action(reach(true, false), &s, false, false), GoodieHut);
        assert_eq!(choose_action(reach(false, true), &s, false, false), GoodieHut);
        assert_eq!(choose_action(reach(false, false), &s, false, false), Invalid);
        assert_eq!(choose_action(reach(true, false), &Selection::default(), false, false), Default);
        assert_eq!(GoodieHut.cursor(), cursor_id::GOODIE_HUT);
        assert!(refusal(Invalid, reach(false, false)).unwrap().contains("hut"));
        // The pointer picks a standing hut and stops seeing it once it is taken.
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        let shape = campaign.shape();
        let at = campaign.cities[HOME_INDEX].center() + Vec3::X * 90.0;
        campaign.hut_sites = vec![at.to_array()];
        let any = |_: Vec3| Reach { any: true, strike: true, economic: true, air: false };
        assert!(matches!(pick_ground(&campaign, shape, at, any, |_| true), Target::Hut { .. }));
        campaign.huts_taken.push(0);
        assert!(!matches!(pick_ground(&campaign, shape, at, any, |_| true), Target::Hut { .. }));
    }
}
