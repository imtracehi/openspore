//! The parts of the Space HUD that belong to the place the UFO is at: the
//! planet panel (terraform score bullseye, T1-T3, the three food web rows of
//! six slots, colony slots, climate icons, camera buttons), the starmap filter
//! toggles with their camera cluster, the species/communication panel and the
//! Capture System / Purchase System panel.
//!
//! [`bind_world`] pushes [`SpaceHudState`] into the main layout; the contexts
//! (RE S5B) are: planet view shows the planet panel, the system and galaxy
//! views show the starmap cluster, the galaxy hides the capture panel.

use bevy::prelude::*;

use super::{HudZoom, SpaceHudState, SpaceHudUi};
use crate::{
    asset_catalog::ResourceIndex,
    dbpf::ResourceKey,
    space_view::{GalaxyFilter, SpaceView},
    spui::{
        ids::state,
        model::{ImageRef, WinId},
        view::{SpuiAssets, SpuiState},
    },
};

/// Control ids of the planet panel (`034C963F`).
#[allow(dead_code)]
pub mod pid {
    pub const PANEL: u32 = 0x034C_963F;
    pub const NAME: u32 = 0x0562_A2DA;
    pub const HIDE: u32 = 0x0562_A141;
    /// Terraforming / Minimap toggle.
    pub const MODE_TOGGLE: u32 = 0x0435_9752;
    pub const MINIMAP_HOLDER: u32 = 0x0190_722E;
    /// Group holding the terraform bullseye, food web and colony slots.
    pub const TERRAFORM: u32 = 0x58D9_B3E0;
    pub const COLONY_COUNT: u32 = 0x0680_D440;
    /// Bullseye window and its marker.
    pub const BULLSEYE: u32 = 0xA80A_7E44;
    pub const MARKER: u32 = 0xA80A_7E45;
    /// The velocity arrow, a child of the bullseye holder (0x1066F30).
    pub const ARROW: u32 = 0x04D9_0456;
    /// T1 / T2 / T3 rings (toggle buttons sharing one rectangle).
    pub const T_BUTTONS: [u32; 3] = [0x0523_3980, 0x0523_39A8, 0x0523_39C0];
    /// Food web rows T1 (bottom) to T3 (top): three plants, two herbivores,
    /// one carnivore each.
    pub const ROW_SLOTS: [[u32; 6]; 3] = [
        [0x037E_89D0, 0x037E_89D1, 0x037E_89D2, 0x037E_89D9, 0x037E_89DA, 0x037E_89DF],
        [0x037E_89D3, 0x037E_89D4, 0x037E_89D5, 0x037E_89DB, 0x037E_89DC, 0x037E_89E0],
        [0x037E_89D6, 0x037E_89D7, 0x037E_89D8, 0x037E_89DD, 0x037E_89DE, 0x037E_89E1],
    ];
    /// Lock windows over the rows ("Improve Terraform Score to Unlock") and
    /// the row highlight windows shown while a T button is hovered.
    pub const ROW_LOCKS: [u32; 3] = [0x0369_69FF, 0x0369_6A10, 0x0369_6A1A];
    pub const ROW_HOVER: [u32; 3] = [0x051D_EC70, 0x051D_ECD0, 0x051D_ECD8];
    /// Colony slots, bottom to top.
    pub const COLONY_SLOTS: [u32; 3] = [0x0535_CBD8, 0x0535_CBD9, 0x0535_CBDA];
}

/// The atlas of the Space layout and the region of a white disc in it (a
/// 16x16 filled circle the slot buttons are drawn with, tinted by the
/// kind of slot): the retail code draws the species' icon here.
const ATLAS: ResourceKey =
    ResourceKey { type_id: crate::ui_art::UI_PNG_TYPE, group_id: 0x31A4_4893, instance_id: 0xB5C4_1506 };
const DISC_UV: [f32; 4] = [882.0 / 1024.0, 959.0 / 1024.0, 898.0 / 1024.0, 975.0 / 1024.0];
/// ADAPTED tints of a filled food web slot: plants, herbivores, carnivores.
const TINT_PLANT: u32 = 0xFF66_DD55;
const TINT_HERBIVORE: u32 = 0xFFEE_CC44;
const TINT_CARNIVORE: u32 = 0xFFEE_5544;
const TINT_COLONY: u32 = 0xFF55_CCFF;

fn disc() -> ImageRef {
    ImageRef { file: ATLAS, uv: DISC_UV, dims: None }
}

// ---------------------------------------------------------------------------
// Pure rules (unit tested).
// ---------------------------------------------------------------------------

/// The starmap filter a toggle button stands for. ADAPTED: the retail filters
/// are six independent toggles; the rewrite's map has one exclusive filter
/// ([`GalaxyFilter`]), so the toggles act like radio buttons and the
/// retail ones without a counterpart share the closest.
pub fn filter_of(control: u32) -> Option<GalaxyFilter> {
    use super::id::*;
    match control {
        FILTER_MY_EMPIRE => Some(GalaxyFilter::Colonies),
        FILTER_EMPIRES | FILTER_ALLIES => Some(GalaxyFilter::Empires),
        FILTER_VISITED | FILTER_TRAIL => Some(GalaxyFilter::Visited),
        FILTER_MISSIONS => Some(GalaxyFilter::Trade),
        _ => None,
    }
}

/// The toggle that shows an active filter (the primary one of its group).
pub fn control_of(filter: GalaxyFilter) -> Option<u32> {
    use super::id::*;
    match filter {
        GalaxyFilter::All => None,
        GalaxyFilter::Colonies => Some(FILTER_MY_EMPIRE),
        GalaxyFilter::Empires => Some(FILTER_EMPIRES),
        GalaxyFilter::Visited => Some(FILTER_VISITED),
        GalaxyFilter::Trade => Some(FILTER_MISSIONS),
    }
}

/// A click on a filter toggle: switch to its filter, or back to all stars when
/// that filter is already on.
pub fn set_filter(view: &mut SpaceView, control: u32) {
    let Some(target) = filter_of(control) else {
        return;
    };
    view.filter = if view.filter == target { GalaxyFilter::All } else { target };
}

/// Which slots of row `row` (0 = T1) are filled: three plants, two herbivores,
/// one carnivore, from the planet's food web counts (one row holds three plant
/// niches, two herbivores and one carnivore, the rewrite's own tier sizes).
pub fn row_fill(flora: u8, herbivores: u8, carnivores: u8, row: usize) -> [bool; 6] {
    let share = |count: u8, per_row: usize| (count as usize).saturating_sub(row * per_row).min(per_row);
    let (plants, herbs, carns) = (share(flora, 3), share(herbivores, 2), share(carnivores, 1));
    std::array::from_fn(|i| match i {
        0..=2 => i < plants,
        3..=4 => i - 3 < herbs,
        _ => carns > 0,
    })
}

/// A food web row is unlocked by the terraform level (T1 unlocks the first row).
pub fn row_unlocked(t_score: u8, row: usize) -> bool {
    t_score as usize > row
}

/// State of the three colony slots: filled by colonies, locked beyond the limit.
pub fn colony_slots(colonies: u8, limit: u8) -> [(bool, bool); 3] {
    std::array::from_fn(|i| (colonies as usize > i, i >= limit as usize))
}

/// The Capture / Purchase button to show: capture when every planet of the
/// system has surrendered, else purchase once a trade route has matured (RE
/// S5B 0x1069AC0).
pub fn capture_buttons(can_capture: bool, can_purchase: bool) -> (bool, bool) {
    (can_capture, !can_capture && can_purchase)
}

/// The bullseye arrow shows while the marble's speed `sqrt(vT^2 + vA^2)` is above
/// zero (0x1066F30: `fsqrt`, then `ja` to show, else the window is hidden).
pub fn arrow_visible(v_temperature: f32, v_atmosphere: f32) -> bool {
    (v_temperature * v_temperature + v_atmosphere * v_atmosphere).sqrt() > 0.0
}

/// Rotation of the arrow in radians, clockwise on screen from straight up: the
/// signed angle 0x6994A0 returns for the reference `(0, -1, 0)` and the unit
/// velocity `(vT, -vA, 0)` about `(0, 0, 1)` (`acos` of the dot product, and
/// the full turn minus it when `axis . (reference x velocity)` is not above
/// zero; INFERRED: the full turn is the constant of the .data cell 0x16017A8,
/// which the image holds only after start-up). Heating pushes the arrow
/// right, thickening the atmosphere up.
pub fn arrow_angle(v_temperature: f32, v_atmosphere: f32) -> f32 {
    let dir = Vec3::new(v_temperature, -v_atmosphere, 0.0).normalize_or_zero();
    let reference = Vec3::new(0.0, -1.0, 0.0);
    let angle = reference.dot(dir).clamp(-1.0, 1.0).acos();
    if Vec3::Z.dot(reference.cross(dir)) > 0.0 {
        angle
    } else {
        std::f32::consts::TAU - angle
    }
}

/// `spg_bullseye-direction-arrow-01..03`: three 13x100 pictures of an arrow
/// growing up from the window's centre, 10 / 20 / 30 px long (group 0x3064CB38,
/// 0xE660D395 / 96 / 97 at 0x15B9390 / 9C / A8).
pub const ARROW_GROUP: u32 = 0x3064_CB38;
pub const ARROW_IMAGES: [u32; 3] = [0xE660_D395, 0xE660_D396, 0xE660_D397];

/// Which of the three pictures 0x1066F30 puts in the arrow window: above 0.8
/// the third, above 0.4 the second, else the first (VERIFIED branches 0x106738B /
/// 0x10673A4). INFERRED: the compared float is the stack slot the angle of
/// [`arrow_angle`] was stored in (S0+0x28 by the stack arithmetic, the speed
/// lives in another slot), so the length follows the direction, not the speed;
/// this reads like a retail slip but is what the code does.
pub fn arrow_variant(angle: f32) -> usize {
    if angle > 0.8 {
        2
    } else if angle > 0.4 {
        1
    } else {
        0
    }
}

/// The velocity arrow (0x1066F30): hidden at rest, otherwise one of the three
/// pictures, turned along (temperature, -atmosphere) velocity.
pub fn bind_arrow(
    ui: &mut SpuiState,
    hud: &SpaceHudState,
    assets: &mut SpuiAssets,
    images: &mut Assets<Image>,
    index: &ResourceIndex,
) {
    let Some(arrow) = super::window(ui, pid::ARROW) else { return };
    let (v_t, v_a) = hud.planet.as_ref().map_or((0.0, 0.0), |p| p.velocity);
    let moving = arrow_visible(v_t, v_a);
    ui.set_visible(arrow, moving);
    if !moving {
        return;
    }
    let angle = arrow_angle(v_t, v_a);
    ui.set_rotation(arrow, angle);
    let key = ResourceKey {
        type_id: crate::ui_art::UI_PNG_TYPE,
        group_id: ARROW_GROUP,
        instance_id: ARROW_IMAGES[arrow_variant(angle)],
    };
    let image = assets.texture(index, images, key).map(|_| ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None });
    ui.set_image(arrow, image);
}

// ---------------------------------------------------------------------------
// Binding.
// ---------------------------------------------------------------------------

/// Move a window so that its centre lies at screen `(x, y)`; the correction
/// is taken against where the window was solved last frame (any layout style).
fn centre_at(ui: &mut SpuiState, w: WinId, x: f32, y: f32) {
    let (cx, cy) = ui.rect(w).center();
    let (dx, dy) = (x - cx, y - cy);
    if dx.abs() < 0.01 && dy.abs() < 0.01 {
        return;
    }
    let mut area = ui.area(w);
    area[0] += dx;
    area[2] += dx;
    area[1] += dy;
    area[3] += dy;
    ui.set_area(w, area);
}

/// A click on a control of the planet panel.
pub fn click(hud_ui: &mut SpaceHudUi, control: u32) {
    match control {
        pid::MODE_TOGGLE => hud_ui.minimap = !hud_ui.minimap,
        pid::HIDE => hud_ui.planet_hidden = !hud_ui.planet_hidden,
        _ => {}
    }
}

/// Fill the slot of a food web or colony button: the filled ones show a disc.
fn set_slot(ui: &mut SpuiState, control: u32, filled: bool, tint: u32) {
    let Some(w) = super::window(ui, control) else {
        return;
    };
    ui.set_image(w, filled.then(disc));
    ui.set_tint(w, tint);
}

/// Planet panel, starmap cluster, species panel and capture panel.
pub fn bind_world(ui: &mut SpuiState, hud: &SpaceHudState, hud_ui: &SpaceHudUi) {
    let planet_view = hud.zoom == HudZoom::Planet;
    // Starmap filters: the cluster of the galaxy view (ADAPTED: the system view
    // has none; the frame of the filters would stand there empty).
    let galaxy = hud.zoom == HudZoom::Galaxy;
    if let Some(w) = super::window(ui, super::id::FILTERS) {
        ui.set_visible(w, galaxy);
    }
    let active = control_of(hud.filter);
    for control in super::id::FILTER_BUTTONS {
        if let Some(w) = super::window(ui, control) {
            ui.set_visible(w, galaxy);
            ui.set_selected(w, Some(control) == active);
        }
    }
    bind_planet(ui, hud, hud_ui, planet_view);
    bind_species(ui, hud, planet_view);
    bind_capture(ui, hud);
}

fn bind_planet(ui: &mut SpuiState, hud: &SpaceHudState, hud_ui: &SpaceHudUi, planet_view: bool) {
    let Some(panel) = super::window(ui, pid::PANEL) else {
        return;
    };
    let Some(planet) = hud.planet.as_ref().filter(|_| planet_view) else {
        ui.set_visible(panel, false);
        return;
    };
    ui.set_visible(panel, true);
    if let Some(w) = super::window(ui, pid::NAME) {
        ui.set_text(w, planet.name.clone());
    }
    if let Some(w) = super::window(ui, pid::MODE_TOGGLE) {
        ui.set_selected(w, hud_ui.minimap);
    }
    // The terraform group and the minimap holder alternate; both fold away
    // with the panel's Hide button.
    let folded = hud_ui.planet_hidden;
    super::set_visible(ui, pid::TERRAFORM, !hud_ui.minimap && !folded);
    super::set_visible(ui, pid::MINIMAP_HOLDER, hud_ui.minimap && !folded);
    if let Some(w) = super::window(ui, pid::HIDE) {
        ui.set_selected(w, folded);
    }
    if let Some(w) = super::window(ui, pid::COLONY_COUNT) {
        ui.set_text(w, format!("{}/{}", planet.colonies, planet.colony_limit));
    }
    // T rings: lit up to the terraform level; hovering one lights its row.
    for (i, control) in pid::T_BUTTONS.iter().enumerate() {
        let Some(w) = super::window(ui, *control) else {
            continue;
        };
        ui.set_selected(w, planet.t_score as usize > i);
        let hover = ui.states[w] & state::HOVER != 0;
        super::set_visible(ui, pid::ROW_HOVER[i], hover);
    }
    // The bullseye marker: temperature across, atmosphere up. The T rings
    // share the rectangle of the bullseye circle.
    if let (Some(marker), Some(ring)) = (super::window(ui, pid::MARKER), super::window(ui, pid::T_BUTTONS[0])) {
        let circle = ui.rect(ring);
        let (x, y) = super::bullseye_position(planet.temperature, planet.atmosphere);
        let (cx, cy) = (circle.x1 + x * circle.width(), circle.y1 + y * circle.height());
        centre_at(ui, marker, cx, cy);
        // The velocity arrow sits on the marble too: the update positions the
        // marker and the arrow window with the same call (0x1066F30, the second
        // call of vtable +0x64 on window 04D90456), each centred on its own size.
        if let Some(arrow) = super::window(ui, pid::ARROW) {
            centre_at(ui, arrow, cx, cy);
        }
    }
    // Food web rows.
    for (row, slots) in pid::ROW_SLOTS.iter().enumerate() {
        let fill = row_fill(planet.flora, planet.herbivores, planet.carnivores, row);
        for (i, control) in slots.iter().enumerate() {
            let tint = match i {
                0..=2 => TINT_PLANT,
                3..=4 => TINT_HERBIVORE,
                _ => TINT_CARNIVORE,
            };
            set_slot(ui, *control, fill[i], tint);
        }
        super::set_visible(ui, pid::ROW_LOCKS[row], !row_unlocked(planet.t_score, row));
    }
    let slots = colony_slots(planet.colonies, planet.colony_limit);
    for (i, control) in pid::COLONY_SLOTS.iter().enumerate() {
        set_slot(ui, *control, slots[i].0, TINT_COLONY);
        if let Some(w) = super::window(ui, *control) {
            ui.set_enabled(w, !slots[i].1);
        }
    }
}

/// The species/communication panel of the empire whose world the UFO is over.
fn bind_species(ui: &mut SpuiState, hud: &SpaceHudState, planet_view: bool) {
    let Some(panel) = super::window(ui, super::id::SHIP_PANEL) else {
        return;
    };
    let Some(target) = hud.comm.as_ref().filter(|_| planet_view) else {
        ui.set_visible(panel, false);
        return;
    };
    ui.set_visible(panel, true);
    if let Some(w) = super::window(ui, super::id::SPECIES_NAME) {
        ui.set_text(w, target.name.clone());
    }
    // The happiness face has no data yet; the glow marks an incoming message.
    super::set_visible(ui, super::id::HAPPINESS, false);
    super::set_visible(ui, super::id::COMM_GLOW, target.incoming);
    if let Some(w) = super::window(ui, super::id::COMM_BUTTON) {
        ui.set_enabled(w, true);
    }
}

fn bind_capture(ui: &mut SpuiState, hud: &SpaceHudState) {
    use super::id::*;
    let Some(panel) = super::window(ui, CAPTURE_PANEL) else {
        return;
    };
    let Some(capture) = hud.capture.as_ref().filter(|_| hud.zoom != HudZoom::Galaxy) else {
        ui.set_visible(panel, false);
        return;
    };
    ui.set_visible(panel, true);
    let (can_capture, can_purchase) = capture_buttons(capture.can_capture, capture.can_purchase);
    for (control, on) in [(CAPTURE_BUTTON, can_capture), (PURCHASE_BUTTON, can_purchase)] {
        if let Some(w) = super::window(ui, control) {
            ui.set_visible(w, on);
            ui.set_enabled(w, on);
        }
    }
    // One animated icon per colonised planet: lit once it has surrendered.
    for (i, control) in CAPTURE_PLANETS.iter().enumerate() {
        if let Some(w) = super::window(ui, *control) {
            ui.set_visible(w, i < capture.planets.len());
            if let Some(surrendered) = capture.planets.get(i) {
                ui.set_frame(w, Some(u32::from(*surrendered)));
                let text = if *surrendered { "Planet surrendered" } else { "Planet has not surrendered" };
                ui.set_tooltip(w, Some(text.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_hud::id;

    #[test]
    fn the_filter_toggles_map_onto_the_maps_filters() {
        assert_eq!(filter_of(id::FILTER_MY_EMPIRE), Some(GalaxyFilter::Colonies));
        assert_eq!(filter_of(id::FILTER_EMPIRES), Some(GalaxyFilter::Empires));
        assert_eq!(filter_of(id::FILTER_ALLIES), Some(GalaxyFilter::Empires));
        assert_eq!(filter_of(id::FILTER_VISITED), Some(GalaxyFilter::Visited));
        assert_eq!(filter_of(id::FILTER_MISSIONS), Some(GalaxyFilter::Trade));
        assert_eq!(filter_of(0x1234), None);
        // Every filter but "all" has a toggle that shows it.
        for f in GalaxyFilter::ALL {
            match control_of(f) {
                Some(c) => assert_eq!(filter_of(c), Some(f)),
                None => assert_eq!(f, GalaxyFilter::All),
            }
        }
    }

    #[test]
    fn a_filter_toggle_switches_on_and_off() {
        let mut view = SpaceView::default();
        assert_eq!(view.filter, GalaxyFilter::All);
        set_filter(&mut view, id::FILTER_VISITED);
        assert_eq!(view.filter, GalaxyFilter::Visited);
        set_filter(&mut view, id::FILTER_MY_EMPIRE);
        assert_eq!(view.filter, GalaxyFilter::Colonies);
        set_filter(&mut view, id::FILTER_MY_EMPIRE);
        assert_eq!(view.filter, GalaxyFilter::All);
        set_filter(&mut view, 0x1234);
        assert_eq!(view.filter, GalaxyFilter::All);
    }

    #[test]
    fn food_web_rows_fill_three_plants_two_herbivores_one_carnivore() {
        assert_eq!(row_fill(0, 0, 0, 0), [false; 6]);
        assert_eq!(row_fill(2, 1, 0, 0), [true, true, false, true, false, false]);
        assert_eq!(row_fill(3, 2, 1, 0), [true; 6]);
        // The second row starts when the first is full.
        assert_eq!(row_fill(3, 2, 1, 1), [false; 6]);
        assert_eq!(row_fill(4, 3, 2, 1), [true, false, false, true, false, true]);
        // The whole web: nine plants, six herbivores, three carnivores.
        for row in 0..3 {
            assert_eq!(row_fill(9, 6, 3, row), [true; 6], "row {row}");
        }
    }

    #[test]
    fn rows_unlock_with_the_terraform_level() {
        assert!(!row_unlocked(0, 0));
        assert!(row_unlocked(1, 0) && !row_unlocked(1, 1));
        assert!(row_unlocked(2, 1) && !row_unlocked(2, 2));
        assert!(row_unlocked(3, 2));
    }

    #[test]
    fn colony_slots_fill_and_lock() {
        assert_eq!(colony_slots(0, 1), [(false, false), (false, true), (false, true)]);
        assert_eq!(colony_slots(2, 2), [(true, false), (true, false), (false, true)]);
        assert_eq!(colony_slots(3, 3), [(true, false); 3]);
    }

    #[test]
    fn capture_wins_over_purchase() {
        assert_eq!(capture_buttons(false, false), (false, false));
        assert_eq!(capture_buttons(true, false), (true, false));
        assert_eq!(capture_buttons(false, true), (false, true));
        assert_eq!(capture_buttons(true, true), (true, false));
    }

    #[test]
    fn the_capture_panel_follows_surrender_and_trade() {
        use crate::space::SpaceCampaign;
        let mut campaign = SpaceCampaign::default();
        let world = crate::space_galaxy::world_id(5, 0);
        campaign.current_system = 5;
        campaign.current_system_empire = Some(2);
        assert_eq!(campaign.world_empire(world), Some(2));
        let panel = super::super::snapshot_capture(&campaign, &[(world, false)]).expect("an empire system");
        assert_eq!(panel.planets, vec![false]);
        assert!(!panel.can_capture && !panel.can_purchase);
        campaign.empire_colony_hp.insert(world, 0.0);
        let panel = super::super::snapshot_capture(&campaign, &[(world, false)]).unwrap();
        assert_eq!(panel.planets, vec![true]);
        assert!(panel.can_capture);
        campaign.trade_routes.insert(world, 1.0);
        let panel = super::super::snapshot_capture(&campaign, &[(world, false)]).unwrap();
        assert!(panel.can_purchase);
        // No empire in the system: no panel.
        campaign.current_system_empire = None;
        assert!(super::super::snapshot_capture(&campaign, &[(world, false)]).is_none());
    }

    #[test]
    fn the_planet_panel_reads_the_ecology_of_the_world() {
        use crate::space::SpaceCampaign;
        let mut campaign = SpaceCampaign::default();
        let index = campaign.ensure_ecology(7);
        campaign.ecology[index].temperature = 0.5;
        campaign.ecology[index].atmosphere = 0.5;
        campaign.ecology[index].flora = 4;
        let panel = super::super::snapshot_planet(&campaign, 7, "Nyx");
        assert_eq!(panel.name, "Nyx");
        assert_eq!(panel.t_score, 3, "dead centre of both axes");
        assert_eq!(panel.flora, 4);
        assert!(panel.empire.is_none());
    }

    #[test]
    fn the_bullseye_arrow_shows_with_the_marble_and_turns_along_its_velocity() {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        assert!(!arrow_visible(0.0, 0.0));
        assert!(arrow_visible(0.0, 0.001) && arrow_visible(-0.2, 0.0));
        let turn = |t: f32, a: f32| arrow_angle(t, a).rem_euclid(TAU);
        // Straight up is atmosphere velocity, to the right temperature velocity (0x6994A0
        // about the z axis from (0, -1, 0)), clockwise on screen.
        assert!(turn(0.0, 0.1) < 1.0e-5 || (TAU - turn(0.0, 0.1)) < 1.0e-5);
        assert!((turn(0.1, 0.0) - FRAC_PI_2).abs() < 1.0e-5);
        assert!((turn(0.0, -0.1) - PI).abs() < 1.0e-5);
        assert!((turn(-0.1, 0.0) - 3.0 * FRAC_PI_2).abs() < 1.0e-5);
        // Only the direction counts, not the speed.
        assert!((turn(0.3, 0.3) - turn(0.01, 0.01)).abs() < 1.0e-5);
        assert!((turn(0.1, 0.1) - FRAC_PI_2 / 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn the_planet_panel_carries_the_marble_velocity() {
        use crate::space::SpaceCampaign;
        let mut campaign = SpaceCampaign::default();
        campaign.ensure_ecology(7);
        assert_eq!(super::super::snapshot_planet(&campaign, 7, "Nyx").velocity, (0.0, 0.0));
        campaign.hit_marble(7, -0.01, 0.02);
        assert_eq!(super::super::snapshot_planet(&campaign, 7, "Nyx").velocity, (0.02, -0.01));
    }

    #[test]
    fn the_arrow_picture_follows_the_two_thresholds() {
        assert_eq!((arrow_variant(0.0), arrow_variant(0.4), arrow_variant(0.41)), (0, 0, 1));
        assert_eq!((arrow_variant(0.8), arrow_variant(0.81), arrow_variant(std::f32::consts::TAU)), (1, 2, 2));
        assert_eq!(ARROW_IMAGES.map(|id| id & 3), [1, 2, 3].map(|n| (0xE660_D394 + n) & 3));
    }
}
