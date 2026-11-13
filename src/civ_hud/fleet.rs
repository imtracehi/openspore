//! The fleet list: one `PosseItemCiv` per vehicle of the player down the right
//! edge of the screen, with the vehicle's type picture, a 31-frame damage
//! meter, the icon of what it is doing in the upper right corner and a
//! highlight while it is selected. Clicking an item selects the vehicle
//! (Ctrl or Shift toggles it, a double click selects every vehicle of its
//! type), as the Spore Guide describes ("The Fleet List").
//!
//! The pictures are the installed ones: the icons are the image windows of
//! the layout `PosseItemCivIcons` (a palette the retail code draws from).
//!
//! ADAPTED: the retail item shows a thumbnail of the vehicle's creation (not
//! available here), so the domain icon (tank, sail, plane) stands in; the list
//! shows the first [`MAX_ITEMS`] vehicles in a single column; the retail
//! right-click that moves the camera to a vehicle is not offered (the
//! pointer events of the SPUI engine are left clicks only).

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CivHudActive, CivHudRoot, CivHudState};
use crate::{
    asset_catalog::ResourceIndex,
    civ::{Approach, UnitOrder, VehicleDomain, HOME_INDEX},
    civ_rts::{CivUnit, Selected, UnitAction},
    spui::{
        model::{Drawable, ImageRef},
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
};

pub const ITEM_LAYOUT: &str = "PosseItemCiv";
pub const ICONS_LAYOUT: &str = "PosseItemCivIcons";

/// Vehicles shown (one column down the right edge).
pub const MAX_ITEMS: usize = 10;
/// Left edge of an item's body (distance from the right screen edge), top of
/// the first item and the step between items.
const RIGHT_MARGIN: f32 = 100.0;
const TOP: f32 = 118.0;
const STEP: f32 = 68.0;

/// Control ids of `PosseItemCiv` (the same as the Creature pack item) and
/// of the icon palette `PosseItemCivIcons`.
#[allow(dead_code)]
pub mod id {
    pub const ROOT: u32 = 0x03D9_8299;
    pub const BODY: u32 = 0x03D9_8304;
    pub const PORTRAIT: u32 = 0x03D9_9151;
    /// Thirty-one frame damage meter.
    pub const HEALTH: u32 = 0x03D9_9162;
    /// The action icon in the upper right corner of the body.
    pub const ACTION: u32 = 0x03D9_9163;
    /// Yellow outline of a selected vehicle.
    pub const SELECTED: u32 = 0x04F6_56E8;
    /// Hidden button (a red cross in the layout): made the invisible click
    /// target of the item.
    pub const CLICK: u32 = 0x04AC_FCAF;
    /// Windows painted with design-time fill colours (cleared), and the
    /// colour swatches the code reads (hidden).
    pub const FILLED: [u32; 4] = [ROOT, BODY, 0x03D9_9141, PORTRAIT];
    pub const SWATCHES: [u32; 3] = [0x03DF_0885, 0x03DF_0886, 0x03DF_0887];
    // Icons of the palette.
    pub const ICON_LAND: u32 = 0x05D6_93E7;
    pub const ICON_SEA: u32 = 0x05D6_943E;
    pub const ICON_AIR: u32 = 0x05D6_9467;
    pub const ICON_MOVE: u32 = 0x05D6_8816;
    pub const ICON_ATTACK: u32 = 0x05D6_8776;
    pub const ICON_PREACH: u32 = 0x05D6_87F1;
    pub const ICON_TRADE: u32 = 0x05D6_8BB2;
    pub const ICON_MINE: u32 = 0x05D6_8F22;
}

/// What a vehicle is doing, as the action icon shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FleetAction {
    Idle,
    Moving,
    Attacking,
    Preaching,
    Trading,
    Mining,
}

impl FleetAction {
    /// The icon window of the palette that carries the picture.
    pub fn icon(self) -> Option<u32> {
        match self {
            FleetAction::Idle => None,
            FleetAction::Moving => Some(id::ICON_MOVE),
            FleetAction::Attacking => Some(id::ICON_ATTACK),
            FleetAction::Preaching => Some(id::ICON_PREACH),
            FleetAction::Trading => Some(id::ICON_TRADE),
            FleetAction::Mining => Some(id::ICON_MINE),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FleetAction::Idle => "Idle",
            FleetAction::Moving => "Moving",
            FleetAction::Attacking => "Attacking",
            FleetAction::Preaching => "Converting",
            FleetAction::Trading => "Trading",
            FleetAction::Mining => "Claiming spice",
        }
    }
}

/// The action of a vehicle from what it does and what it was told.
pub fn action_of(action: UnitAction, order: UnitOrder, kind: Approach) -> FleetAction {
    match action {
        UnitAction::Firing => FleetAction::Attacking,
        UnitAction::Preaching(_) => FleetAction::Preaching,
        UnitAction::Trading(_) => FleetAction::Trading,
        UnitAction::Moving | UnitAction::Idle => match order {
            UnitOrder::Geyser(_) => FleetAction::Mining,
            UnitOrder::City(_) if kind == Approach::Military => FleetAction::Attacking,
            UnitOrder::Unit(_) | UnitOrder::Epic(_) => FleetAction::Attacking,
            UnitOrder::Move(_) | UnitOrder::Goto(_) | UnitOrder::City(_) | UnitOrder::Hut(_) => FleetAction::Moving,
            UnitOrder::Idle if action == UnitAction::Moving => FleetAction::Moving,
            UnitOrder::Idle => FleetAction::Idle,
        },
    }
}

/// The palette icon of a vehicle's domain.
pub fn domain_icon(domain: VehicleDomain) -> u32 {
    match domain {
        VehicleDomain::Land => id::ICON_LAND,
        VehicleDomain::Sea => id::ICON_SEA,
        VehicleDomain::Air => id::ICON_AIR,
    }
}

/// Frame (0..=30) of the damage meter for a health fraction.
pub fn health_frame(health: f32) -> u32 {
    (health.clamp(0.0, 1.0) * 30.0).round() as u32
}

/// One vehicle of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct FleetItem {
    pub id: u32,
    pub kind: Approach,
    pub domain: VehicleDomain,
    pub health: f32,
    pub action: FleetAction,
    pub selected: bool,
}

/// Tooltip of an item: the vehicle's kind and domain, what it is doing and its
/// damage.
pub fn item_tooltip(item: &FleetItem) -> String {
    format!(
        "{} {} vehicle\n{} - {:.0}%",
        item.kind.title(),
        item.domain.title(),
        item.action.name(),
        item.health.clamp(0.0, 1.0) * 100.0
    )
}

/// Top left of the body of item `n` for a screen of this size.
pub fn item_position(screen: Vec2, n: usize) -> Vec2 {
    Vec2::new((screen.x - RIGHT_MARGIN).max(0.0), TOP + STEP * n as f32)
}

/// Fill the fleet of [`CivHudState`] from the live vehicles.
pub fn publish_fleet(
    active: Res<CivHudActive>,
    units: Query<(&CivUnit, Has<Selected>)>,
    mut hud: ResMut<CivHudState>,
) {
    if !active.0 {
        hud.fleet.clear();
        hud.fleet_total = 0;
        return;
    }
    let mut mine: Vec<FleetItem> = units
        .iter()
        .filter(|(u, _)| u.nation == HOME_INDEX && u.hp > 0.0)
        .map(|(u, selected)| FleetItem {
            id: u.id,
            kind: u.kind,
            domain: u.domain,
            health: (u.hp / u.max_hp.max(1.0)).clamp(0.0, 1.0),
            action: action_of(u.action, u.order, u.kind),
            selected,
        })
        .collect();
    mine.sort_by_key(|i| i.id);
    let total = mine.len();
    mine.truncate(MAX_ITEMS);
    if hud.fleet != mine {
        hud.fleet = mine;
    }
    hud.fleet_total = total;
}

/// Spawn and despawn the item roots (one `PosseItemCiv` each).
pub fn manage_fleet(
    mut commands: Commands,
    active: Res<CivHudActive>,
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &CivHudRoot)>,
) {
    if !active.0 {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let index = &originals.resource_index;
    let want = hud.fleet.len().min(MAX_ITEMS);
    for (entity, kind) in &roots {
        if matches!(kind, CivHudRoot::FleetItem(n) if *n as usize >= want) {
            commands.entity(entity).despawn();
        }
    }
    for n in 0..want {
        let kind = CivHudRoot::FleetItem(n as u8);
        if !roots.iter().any(|(_, k)| *k == kind) {
            if let Some(layout) = assets.layout(index, ITEM_LAYOUT) {
                let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert(kind);
            }
        }
    }
}

/// The picture of each icon window of the palette layout.
#[derive(Resource, Default)]
pub struct FleetIcons {
    map: HashMap<u32, ImageRef>,
    loaded: bool,
}

impl FleetIcons {
    fn load(&mut self, index: &ResourceIndex, assets: &mut SpuiAssets, images: &mut Assets<Image>) {
        if self.loaded {
            return;
        }
        let Some(layout) = assets.layout(index, ICONS_LAYOUT) else {
            return;
        };
        self.loaded = true;
        for w in &layout.windows {
            if let Some(Drawable::Image(d)) = &w.drawable {
                if let Some(image) = d.image.clone() {
                    assets.texture(index, images, image.file);
                    self.map.insert(w.control_id, image);
                }
            }
        }
    }

    pub fn get(&self, control: u32) -> Option<ImageRef> {
        self.map.get(&control).cloned()
    }
}

/// Bind one item.
pub fn bind_item(ui: &mut SpuiState, item: &FleetItem, n: usize, icons: &FleetIcons, hidden: bool) {
    let (Some(root), Some(body)) = (ui.find(id::ROOT), ui.find(id::BODY)) else {
        return;
    };
    if hidden {
        ui.set_visible(root, false);
        return;
    }
    // Move the root so that the body's corner lands on the item's slot.
    let at = item_position(Vec2::new(ui.screen[0], ui.screen[1]), n);
    let mut offset = Vec2::ZERO;
    let mut w = Some(body);
    while let Some(cur) = w {
        if cur == root {
            break;
        }
        let a = ui.layout.window(cur).area;
        offset += Vec2::new(a[0], a[1]);
        w = ui.layout.window(cur).parent;
    }
    let authored = ui.layout.window(root).area;
    let (rw, rh) = (authored[2] - authored[0], authored[3] - authored[1]);
    ui.set_area(root, [at.x - offset.x, at.y - offset.y, at.x - offset.x + rw, at.y - offset.y + rh]);
    ui.set_visible(root, true);
    for control in id::SWATCHES {
        super::set_visible(ui, control, false);
    }
    // The layout paints design-time colour blocks behind the item.
    for control in id::FILLED {
        if let Some(w) = ui.find(control) {
            ui.set_tint(w, 0x00FF_FFFF);
        }
    }
    if let Some(w) = ui.find(id::HEALTH) {
        ui.set_frame(w, Some(health_frame(item.health)));
    }
    if let Some(w) = ui.find(id::SELECTED) {
        ui.set_visible(w, item.selected);
    }
    if let Some(w) = ui.find(id::PORTRAIT) {
        let image = icons.get(domain_icon(item.domain));
        // The picture needs a visible tint (the fill colour is cleared above).
        ui.set_tint(w, if image.is_some() { 0xFFFF_FFFF } else { 0x00FF_FFFF });
        ui.set_image(w, image);
    }
    // The action icon sits in the upper right corner of the body.
    if let Some(w) = ui.find(id::ACTION) {
        let image = item.action.icon().and_then(|c| icons.get(c));
        ui.set_visible(w, image.is_some());
        ui.set_image(w, image);
        // The icon is a child of the root, like the body: both areas are
        // relative to the root.
        let b = ui.layout.window(body).area;
        ui.set_area(w, [b[2] - 24.0, b[1] - 8.0, b[2] + 6.0, b[1] + 22.0]);
    }
    // The hidden "close" button becomes an invisible click target over the
    // body and carries the tooltip.
    if let Some(button) = ui.find(id::CLICK) {
        let b = ui.layout.window(body).area;
        ui.set_area(button, [b[0], b[1], b[2], b[3]]);
        ui.set_visible(button, true);
        ui.set_enabled(button, true);
        ui.set_tint(button, 0x00FF_FFFF);
        ui.set_tooltip(button, Some(item_tooltip(item)));
    }
}

/// Bind the item roots.
pub fn bind_fleet(
    hud: Res<CivHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut icons: ResMut<FleetIcons>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
) {
    let index = &originals.resource_index;
    if index.records.is_empty() {
        return;
    }
    icons.load(index, &mut assets, &mut images);
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        if let CivHudRoot::FleetItem(n) = kind {
            if let Some(item) = hud.fleet.get(*n as usize) {
                bind_item(&mut ui, item, *n as usize, &icons, hud.comm.is_some());
            }
        }
    }
}

/// System: clicks on the items select the vehicle.
#[allow(clippy::too_many_arguments)]
pub fn fleet_clicks(
    time: Res<Time>,
    hud: Res<CivHudState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut roots: Query<(&mut SpuiState, &CivHudRoot)>,
    units: Query<(Entity, &CivUnit, Has<Selected>)>,
    mut commands: Commands,
    mut last: Local<Option<(f32, u32)>>,
) {
    fleet_actions(&time, &hud, &mut roots, &keys, &units, &mut commands, &mut last);
}

/// Clicks on the items select the vehicle (Ctrl / Shift toggles it, a double
/// click selects every vehicle of its type).
pub fn fleet_actions(
    time: &Time,
    hud: &CivHudState,
    roots: &mut Query<(&mut SpuiState, &CivHudRoot)>,
    keys: &ButtonInput<KeyCode>,
    units: &Query<(Entity, &CivUnit, Has<Selected>)>,
    commands: &mut Commands,
    last_click: &mut Option<(f32, u32)>,
) {
    let toggle = keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
        || keys.pressed(KeyCode::ShiftLeft)
        || keys.pressed(KeyCode::ShiftRight);
    let now = time.elapsed_secs();
    for (mut ui, kind) in roots.iter_mut() {
        let CivHudRoot::FleetItem(n) = kind else {
            continue;
        };
        for click in ui.take_clicks() {
            if ui.layout.window(click).control_id != id::CLICK {
                continue;
            }
            let Some(item) = hud.fleet.get(*n as usize) else {
                continue;
            };
            let double = last_click.is_some_and(|(at, id)| id == item.id && now - at <= 0.35);
            *last_click = Some((now, item.id));
            let want: Vec<Entity> = if double {
                units
                    .iter()
                    .filter(|(_, u, _)| u.nation == HOME_INDEX && u.kind == item.kind && u.domain == item.domain)
                    .map(|(e, ..)| e)
                    .collect()
            } else {
                units.iter().filter(|(_, u, _)| u.id == item.id).map(|(e, ..)| e).collect()
            };
            for (entity, unit, selected) in units.iter() {
                if unit.nation != HOME_INDEX {
                    continue;
                }
                let wanted = want.contains(&entity);
                let target = if toggle && !double { selected ^ wanted } else { wanted };
                if target && !selected {
                    commands.entity(entity).insert(Selected);
                } else if !target && selected {
                    commands.entity(entity).remove::<Selected>();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_follow_what_a_vehicle_does_and_was_told() {
        use UnitAction as A;
        assert_eq!(action_of(A::Idle, UnitOrder::Idle, Approach::Military), FleetAction::Idle);
        assert_eq!(action_of(A::Moving, UnitOrder::Move([0.0, 0.0]), Approach::Military), FleetAction::Moving);
        assert_eq!(action_of(A::Firing, UnitOrder::City(2), Approach::Military), FleetAction::Attacking);
        assert_eq!(action_of(A::Moving, UnitOrder::City(2), Approach::Military), FleetAction::Attacking);
        assert_eq!(action_of(A::Moving, UnitOrder::City(2), Approach::Religious), FleetAction::Moving);
        assert_eq!(action_of(A::Preaching(2), UnitOrder::City(2), Approach::Religious), FleetAction::Preaching);
        assert_eq!(action_of(A::Trading(3), UnitOrder::City(3), Approach::Economic), FleetAction::Trading);
        assert_eq!(action_of(A::Moving, UnitOrder::Geyser(1), Approach::Economic), FleetAction::Mining);
        assert_eq!(action_of(A::Idle, UnitOrder::Unit(9), Approach::Military), FleetAction::Attacking);
        assert_eq!(action_of(A::Moving, UnitOrder::Idle, Approach::Military), FleetAction::Moving);
        assert!(FleetAction::Idle.icon().is_none());
        let icons: std::collections::HashSet<_> = [
            FleetAction::Moving,
            FleetAction::Attacking,
            FleetAction::Preaching,
            FleetAction::Trading,
            FleetAction::Mining,
        ]
        .iter()
        .filter_map(|a| a.icon())
        .collect();
        assert_eq!(icons.len(), 5, "every action has its own icon");
    }

    #[test]
    fn domains_have_their_own_icons_and_the_meter_has_thirty_one_frames() {
        let icons = [VehicleDomain::Land, VehicleDomain::Sea, VehicleDomain::Air].map(domain_icon);
        assert_eq!(icons, [id::ICON_LAND, id::ICON_SEA, id::ICON_AIR]);
        assert_eq!(health_frame(0.0), 0);
        assert_eq!(health_frame(1.0), 30);
        assert_eq!(health_frame(0.5), 15);
        assert_eq!(health_frame(9.0), 30);
    }

    #[test]
    fn items_stack_down_the_right_edge() {
        let screen = Vec2::new(1280.0, 900.0);
        let first = item_position(screen, 0);
        assert_eq!(first.x, 1180.0);
        assert_eq!(item_position(screen, 3).y, first.y + 3.0 * STEP);
        // The last item stays above the stance buttons (they start at y 827).
        assert!(item_position(screen, MAX_ITEMS - 1).y + 62.0 < 827.0);
        // A tiny screen never gives a negative x.
        assert_eq!(item_position(Vec2::new(40.0, 40.0), 0).x, 0.0);
    }

    #[test]
    fn tooltips_name_the_vehicle_its_action_and_damage() {
        let item = FleetItem {
            id: 4,
            kind: Approach::Religious,
            domain: VehicleDomain::Sea,
            health: 0.5,
            action: FleetAction::Preaching,
            selected: false,
        };
        assert_eq!(item_tooltip(&item), "Religious sea vehicle\nConverting - 50%");
    }

    fn fleet_app() -> App {
        let mut app = crate::runtime::headless_app(crate::game::RunProgress::default());
        app.world_mut().resource_mut::<NextState<crate::stages::GameStage>>().set(crate::stages::GameStage::Civilization);
        app.update();
        app.update();
        app
    }

    #[test]
    fn the_list_publishes_the_players_vehicles_in_order() {
        let mut app = fleet_app();
        app.world_mut().insert_resource(CivHudActive(true));
        let opening = app.world_mut().query::<&CivUnit>().iter(app.world()).filter(|u| u.nation == HOME_INDEX).count();
        for (id, nation) in [(70u32, 0usize), (30, 0), (90, 2)] {
            let mut unit = CivUnit::from_state(&crate::civ::UnitState {
                id,
                nation,
                kind: Approach::Military,
                hp: 50.0,
                ..default()
            });
            unit.hp = 50.0;
            app.world_mut().spawn((unit, Transform::default()));
        }
        app.update();
        let hud = app.world().resource::<CivHudState>();
        let ids: Vec<u32> = hud.fleet.iter().map(|i| i.id).collect();
        assert!(ids.windows(2).all(|w| w[0] <= w[1]), "by id: {ids:?}");
        assert!(ids.contains(&30) && ids.contains(&70), "the player's new vehicles: {ids:?}");
        assert!(!ids.contains(&90), "not a rival's");
        assert_eq!(hud.fleet_total, opening + 2);
        let thirty = hud.fleet.iter().find(|i| i.id == 30).unwrap();
        // 50 of a default land vehicle's 249 (0x4E74D0).
        assert!((thirty.health - 50.0 / 249.0).abs() < 1e-4, "{}", thirty.health);
    }

    fn installed() -> Option<(ResourceIndex, SpuiAssets)> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        (!index.records.is_empty()).then(|| (index, SpuiAssets::default()))
    }

    #[test]
    fn installed_item_layouts_have_the_windows_the_binding_uses() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let item = assets.layout(&index, ITEM_LAYOUT).expect("PosseItemCiv");
        for control in [id::ROOT, id::BODY, id::PORTRAIT, id::HEALTH, id::ACTION, id::SELECTED, id::CLICK]
            .iter()
            .chain(&id::FILLED)
            .chain(&id::SWATCHES)
        {
            assert!(item.find(*control).is_some(), "item window {control:08X}");
        }
        assert_eq!(item.window(item.find(id::HEALTH).unwrap()).anim().map(|a| a.frame_count), Some(31));
        assert!(item.window(item.find(id::CLICK).unwrap()).button().is_some(), "the click target is a button");
        let icons = assets.layout(&index, ICONS_LAYOUT).expect("PosseItemCivIcons");
        for control in [
            id::ICON_LAND, id::ICON_SEA, id::ICON_AIR, id::ICON_MOVE, id::ICON_ATTACK, id::ICON_PREACH, id::ICON_TRADE, id::ICON_MINE,
        ] {
            let w = icons.window(icons.find(control).unwrap_or_else(|| panic!("icon {control:08X}")));
            assert!(matches!(&w.drawable, Some(Drawable::Image(d)) if d.image.is_some()), "icon {control:08X} is a picture");
        }
    }

    #[test]
    fn an_item_shows_its_vehicle() {
        let Some((index, mut assets)) = installed() else {
            return;
        };
        let mut images = Assets::<Image>::default();
        let mut icons = FleetIcons::default();
        icons.load(&index, &mut assets, &mut images);
        assert!(icons.get(id::ICON_LAND).is_some() && icons.get(id::ICON_MINE).is_some());
        let layout = assets.layout(&index, ITEM_LAYOUT).unwrap();
        let mut ui = SpuiState::new(layout, RootPolicy::Authored);
        ui.screen = [1280.0, 900.0];
        let item = FleetItem {
            id: 1,
            kind: Approach::Military,
            domain: VehicleDomain::Land,
            health: 0.5,
            action: FleetAction::Attacking,
            selected: true,
        };
        bind_item(&mut ui, &item, 1, &icons, false);
        let w = |ui: &SpuiState, c: u32| ui.find(c).unwrap();
        assert_eq!(ui.frame_overrides[w(&ui, id::HEALTH)], Some(15));
        assert!(ui.visible(w(&ui, id::SELECTED)) && ui.visible(w(&ui, id::ACTION)));
        assert!(ui.image_overrides[w(&ui, id::ACTION)].is_some() && ui.image_overrides[w(&ui, id::PORTRAIT)].is_some());
        assert!(ui.visible(w(&ui, id::CLICK)));
        assert_eq!(ui.tooltip_overrides[w(&ui, id::CLICK)].as_deref(), Some("Military land vehicle\nAttacking - 50%"));
        // The root moved so that the body's corner is on the slot of item 1.
        let at = item_position(Vec2::new(1280.0, 900.0), 1);
        let root = ui.area(w(&ui, id::ROOT));
        let body = ui.layout.window(w(&ui, id::BODY)).area;
        assert!((root[0] + body[0] - at.x).abs() < 0.01 && (root[1] + body[1] - at.y).abs() < 0.01, "{root:?} {body:?} {at:?}");
        // Idle: no action icon and not selected.
        let idle = FleetItem { action: FleetAction::Idle, selected: false, ..item };
        bind_item(&mut ui, &idle, 1, &icons, false);
        assert!(!ui.visible(w(&ui, id::SELECTED)) && !ui.visible(w(&ui, id::ACTION)));
    }
}
