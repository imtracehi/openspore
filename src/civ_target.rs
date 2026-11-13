//! Click targeting of the Civilization ability tools (helper `civ-target`, round 2).
//!
//! Tags: **VERIFIED** (code-image address, march2017 exe), **INFERRED** (reading of neighbouring
//! code), **ADAPTED** (OpenSpore choice).
//!
//! Retail flow (all of it in the Civ game-mode UI, `GameCiv` singleton 0x169D2C8):
//! * HUD ability button (control 0x5A55B10 / B20 / B30 / B40, handler 0xE05510): the button is
//!   mapped to a tool index (0xE053A0); a button of the tool that is already armed sends message
//!   0x1B (cancel), any other arms that tool with 0xCF1D40. VERIFIED.
//! * Tool index order (table 0x14693E8, consequence -> tool by 0xBEFAD0): `napalm` 0, `tacnuke` 1,
//!   `icbm` 2, `faithheal` 3, `blackcloud` 4, `rising` 5, `espionage` 6, `adblitz` 7,
//!   `globaladblitz` 8, `emp` 9, `i_field` 10, `diplodervish` 11. VERIFIED.
//! * 0xCF1D40 (arm): nothing when the tool is armed already; refused when
//!   `cCivilization::CanUseTool` (0xBF8B40) says no; the three superweapons (2, 5, 8) are launched
//!   AT ONCE (`UseTool(tool, <fixed vector>)` 0xBF8BB0, no click); every other tool is armed (the
//!   click triggers are enabled, trigger config 0x058B3913). VERIFIED.
//! * Every frame while a tool is armed (cursor-tool update 0xCEA320): the ground point under the
//!   cursor (`cGameViewManager` +0x38); none -> the cursor effects are hidden; otherwise
//!   `valid = 0xBDA440(tool, point)` and the target effects (table 0x1477EB0 / 0x1477E80) are
//!   shown on a valid point, the invalid-target effect 0x78BEEEC5 on an invalid one. The effects
//!   are placed at `0xBDA350(tool, point)` (a valid city tool snaps to the city centre) and scaled
//!   with the nearest city's radius (+0x25C of its tuning) or the tool's area x 0.33
//!   (`0xBF18C0(tool)` +0x1AC). VERIFIED. The three effect ids have no registered name; they are
//!   Swarm effects, which the Civ view does not play: OpenSpore draws two rings instead (ADAPTED).
//! * Validity (0xBDA440): `blackcloud` (4), `adblitz` (7) and `diplodervish` (11) need the nearest
//!   city to the point, the point inside that city's radius and the city not to be the player's
//!   (the city interface +0x58 returned false; the same call picks "the player's city" in 0xB25C10:
//!   INFERRED reading "is owned by the player"). Every other tool is valid on any ground. VERIFIED.
//! * Click (message 0x576439B, 0xCF682C): the point is read again, a valid target is used with
//!   `UseTool(tool, point)` and the tool is dropped; an invalid one shows the event-log message
//!   (cUIEventLog group 0x1AC1D4A1) and keeps the tool armed; no ground under the cursor does
//!   nothing. VERIFIED. Right click / Escape cancel: ADAPTED (the brief and the usual Spore
//!   cancel; the retail cancel message handler 0xCF5818 clears the armed tool).
//!
//! ADAPTED here: the city radius used for the "inside the city" test (the retail value is a tuning
//! field not read; the right-click city order of `civ_rts` uses the same 7 city units), the rings.
//! The cursor of an armed tool is the retail `cursor-more` (0xE35605, `civ_cursor`).

use bevy::{
    light::NotShadowCaster,
    prelude::*,
};

use crate::{
    civ::{CivCampaign, CivCommander, PowerSlot},
    civ_planet::{Globe, CITY_SCALE},
    civ_rts::{FxRoot, RtsAssets, RtsState},
    civ_rules::PowerData,
    civ_voices,
    game::RunProgress,
    stages::{GameStage, StageOwned},
};

/// Table 0x14693E8: the tool of each tool index.
pub const TOOLS: [&str; 12] = [
    "napalm",
    "tacnuke",
    "icbm",
    "faithheal",
    "blackcloud",
    "rising",
    "espionage",
    "adblitz",
    "globaladblitz",
    "emp",
    "i_field",
    "diplodervish",
];

/// The retail index of a tool (`cCivilization` keeps its twelve tools in this order).
pub fn tool_index(tool: &str) -> Option<usize> {
    TOOLS.iter().position(|t| *t == tool)
}

/// What a tool needs from its target (0xCF1D40, 0xBDA440).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRule {
    /// The superweapons: launched by the button, no target.
    Immediate,
    /// Black Cloud, Ad Blitz, Diplo Dervish: a click inside a city that is not the player's.
    RivalCity,
    /// Everything else: any ground.
    Ground,
}

pub fn rule(tool: &str) -> TargetRule {
    match tool_index(tool) {
        Some(2 | 5 | 8) => TargetRule::Immediate,
        Some(4 | 7 | 11) => TargetRule::RivalCity,
        _ => TargetRule::Ground,
    }
}

/// Tools that show the second (area) effect, table 0x1477E80: `napalm`, `tacnuke`, `faithheal`,
/// `espionage`, `emp`, `i_field`.
pub fn has_area_ring(tool: &str) -> bool {
    matches!(tool_index(tool), Some(0 | 1 | 3 | 6 | 9 | 10))
}

/// ADAPTED: how close to a city centre a click counts as inside the city (retail: the +0x25C
/// radius of the city's tuning object, not read). The same reach the right-click city order uses.
pub const CITY_TARGET_RADIUS: f32 = 7.0 * CITY_SCALE;

/// The answer of 0xBDA440 / 0xBDA350 for one ground point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetCheck {
    pub valid: bool,
    /// Where the cursor effects sit and the ability lands: a valid city tool snaps to the city
    /// centre, the others stay on the ground point.
    pub at: Vec3,
    /// The rival city the point selects (city tools; the Gadget Bomb: the rival city its blast
    /// reaches).
    pub city: Option<usize>,
}

/// The city nearest to `ground` (any owner, hidden ones excluded) and its distance
/// (`cPlanetModel::GetNearestCity`).
pub fn nearest_city(campaign: &CivCampaign, ground: Vec3) -> Option<(usize, f32)> {
    let shape = campaign.shape();
    campaign
        .cities
        .iter()
        .filter(|c| campaign.present(c.index))
        .map(|c| (c.index, shape.distance(ground, c.center())))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// 0xBDA440 / 0xBDA350 for the tool `data` and the ground point under the cursor.
pub fn check(campaign: &CivCampaign, data: &PowerData, ground: Vec3) -> TargetCheck {
    match rule(data.tool) {
        TargetRule::RivalCity => match nearest_city(campaign, ground) {
            Some((c, d)) if d <= CITY_TARGET_RADIUS && !campaign.cities[c].owned() => TargetCheck {
                valid: true,
                at: campaign.cities[c].center(),
                city: Some(c),
            },
            _ => TargetCheck { valid: false, at: ground, city: None },
        },
        _ => {
            // ADAPTED: OpenSpore's Gadget Bomb is a city blast, so it needs the rival city its
            // radius reaches (retail: an area blast at the point).
            let city = (data.tool == "tacnuke")
                .then(|| {
                    let shape = campaign.shape();
                    campaign
                        .cities
                        .iter()
                        .filter(|c| campaign.present(c.index) && !c.owned())
                        .map(|c| (c.index, shape.distance(ground, c.center())))
                        .filter(|(_, d)| *d <= data.radius() + CITY_TARGET_RADIUS)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(c, _)| c)
                })
                .flatten();
            TargetCheck { valid: true, at: ground, city }
        }
    }
}

/// What a left click does with an armed tool (message 0x576439B).
#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    /// No ground under the cursor: nothing happens (the tool stays armed).
    Ignore,
    /// The target is not valid: the message is shown, the tool stays armed.
    Invalid(String),
    /// Use the tool at `at`.
    Fire { at: Vec3, city: Option<usize> },
}

pub fn resolve_click(campaign: &CivCampaign, slot: PowerSlot, ground: Option<Vec3>) -> Click {
    let Some(data) = civ_voices::slot_data(campaign, slot) else {
        return Click::Ignore;
    };
    let Some(ground) = ground else {
        return Click::Ignore;
    };
    let c = check(campaign, data, ground);
    if c.valid {
        Click::Fire { at: c.at, city: c.city }
    } else {
        // The retail text is an event-log entry; the wording keeps `civ_audio::is_error_message`
        // playing `ui_civ_error` for it.
        Click::Invalid(format!("{} cannot be used there: aim it inside a rival city", data.name))
    }
}

// ---------------------------------------------------------------------------
// Presentation: the cursor shape and the target rings
// ---------------------------------------------------------------------------

/// What the cursor tool shows this frame (0xCEA320).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct ToolCursor {
    /// A tool is armed and the cursor is over the world.
    pub active: bool,
    pub valid: bool,
    pub at: Vec3,
    /// World radius of the area ring (0: none) and of the city / cursor ring.
    pub area: f32,
    pub ring: f32,
    pub tool: Option<&'static str>,
}

/// Radius of the small cursor ring of a tool without a city (ADAPTED).
const CURSOR_RING: f32 = 1.4 * CITY_SCALE;

/// Pure: what the rings show for an armed tool over `ground`.
pub fn cursor_state(campaign: &CivCampaign, slot: PowerSlot, ground: Option<Vec3>) -> ToolCursor {
    let (Some(data), Some(ground)) = (civ_voices::slot_data(campaign, slot), ground) else {
        return ToolCursor::default();
    };
    let c = check(campaign, data, ground);
    ToolCursor {
        active: true,
        valid: c.valid,
        at: c.at,
        area: if c.valid && has_area_ring(data.tool) { data.radius() } else { 0.0 },
        ring: match (c.valid, rule(data.tool)) {
            (true, TargetRule::RivalCity) => CITY_TARGET_RADIUS,
            _ => CURSOR_RING,
        },
        tool: Some(data.tool),
    }
}

/// The mouse as the cursor tool reads it (the ground under the pointer).
#[derive(bevy::ecs::system::SystemParam)]
pub struct CursorGround<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<crate::GameCamera>>,
    spui: Option<Res<'w, crate::spui::view::SpuiPointer>>,
    surface: Option<Res<'w, crate::terrain::LandSurface>>,
    focus: Query<'w, 's, &'static Transform, With<CivCommander>>,
}

impl CursorGround<'_, '_> {
    /// The ground under the cursor; without a cursor (captures, the first frame) the camera focus
    /// (ADAPTED); `None` over the HUD or off the planet.
    pub fn ground(&self) -> Option<Vec3> {
        if self.spui.as_ref().is_some_and(|s| s.over_ui) {
            return None;
        }
        let cursor = self.windows.iter().find_map(|w| w.cursor_position());
        match cursor {
            Some(cursor) => {
                let (camera, transform) = self.camera.single().ok()?;
                let ray = camera.viewport_to_world(transform, cursor).ok()?;
                Globe::from_surface(self.surface.as_deref()).ray(ray.origin, *ray.direction)
            }
            None => self.focus.single().ok().map(|t| t.translation),
        }
    }
}

/// Marks a ring of the target cursor: 0 the area ring, 1 the city / cursor ring.
#[derive(Component)]
pub struct TargetRing(pub u8);

fn track_cursor(
    run: Res<RunProgress>,
    state: Option<Res<RtsState>>,
    pointer: CursorGround,
    mut cursor: ResMut<ToolCursor>,
) {
    let next = match state.as_ref().and_then(|s| s.armed) {
        Some(slot) => cursor_state(&run.civ_campaign, slot, pointer.ground()),
        None => ToolCursor::default(),
    };
    if *cursor != next {
        *cursor = next;
    }
}

/// The two rings: green on a valid target, red on an invalid one, hidden with no tool armed.
#[allow(clippy::too_many_arguments)]
fn present_rings(
    mut commands: Commands,
    cursor: Res<ToolCursor>,
    state: Option<Res<RtsState>>,
    assets: Option<Res<RtsAssets>>,
    run: Res<RunProgress>,
    roots: Query<&Transform, (With<FxRoot>, Without<TargetRing>)>,
    mut rings: Query<(&TargetRing, &mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let (Some(state), Some(assets)) = (state, assets) else {
        return;
    };
    let Some(root) = state.fx_root else {
        return;
    };
    if rings.is_empty() {
        for which in 0..2u8 {
            commands.spawn((
                StageOwned,
                TargetRing(which),
                Name::new("tool target ring"),
                Mesh3d(assets.ring.clone()),
                MeshMaterial3d(assets.marker_mat.clone()),
                Transform::from_scale(Vec3::splat(0.01)),
                Visibility::Hidden,
                NotShadowCaster,
                ChildOf(root),
            ));
        }
        return;
    }
    let origin = roots.get(root).map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let shape = run.civ_campaign.shape();
    let material = if cursor.valid { &assets.marker_mat } else { &assets.bar_rival };
    for (ring, mut transform, mut visibility, mut mat) in &mut rings {
        let radius = if ring.0 == 0 { cursor.area } else { cursor.ring };
        let show = cursor.active && radius > 0.0;
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want {
            *visibility = want;
        }
        if !show {
            continue;
        }
        if mat.0 != *material {
            mat.0 = material.clone();
        }
        let up = shape.up(cursor.at);
        transform.translation = cursor.at + up * 0.12 * CITY_SCALE - origin;
        transform.rotation = shape.frame(cursor.at);
        transform.scale = Vec3::new(radius, 0.05 * CITY_SCALE, radius);
    }
}

/// Escape drops an armed tool before anything else reads it (before the Civ controls deselect
/// and before the pause menu opens).
fn cancel_on_escape(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    state: Option<ResMut<RtsState>>,
    mut run: ResMut<RunProgress>,
) {
    let Some(mut state) = state else { return };
    if state.armed.is_some() && keys.just_pressed(KeyCode::Escape) {
        state.armed = None;
        run.civ_campaign.message = "Ability dropped".into();
        keys.clear_just_pressed(KeyCode::Escape);
    }
}

/// Capture scenarios `tool` / `tool-invalid`: a funded nation with the Tribal ability and the camera
/// on the rival city nearest to the capital (`tool`: Ad Blitz armed there is a valid target) or on
/// the capital (`tool-invalid`: the player's own city is not a target). Use with `--press 9@30`.
pub fn capture_scenario(world: &mut World, name: &str) {
    let target = {
        let mut run = world.resource_mut::<RunProgress>();
        run.spice += 40_000.0;
        let c = &mut run.civ_campaign;
        c.ensure_cities();
        if c.tribal_power.is_none() {
            c.tribal_power = Some(crate::civ::CivTribalPower::AdBlitz);
        }
        let capital = c.cities[crate::civ::HOME_INDEX].center();
        if name == "tool-invalid" {
            Some(capital)
        } else {
            let nearest = c
                .cities
                .iter()
                .filter(|city| !city.owned())
                .min_by(|a, b| c.distance(capital, a.center()).total_cmp(&c.distance(capital, b.center())))
                .map(|city| (city.index, city.center()));
            // A hidden rival city is not a target (and not drawn): reveal the one shown.
            if let Some((index, _)) = nearest {
                c.hidden.retain(|h| *h != index);
            }
            nearest.map(|(_, at)| at)
        }
    };
    if let Some(at) = target {
        let on_sphere = world.resource::<RunProgress>().civ_campaign.shape().on_sphere();
        let mut query = world.query_filtered::<&mut Transform, With<CivCommander>>();
        if let Ok(mut t) = query.single_mut(world) {
            t.translation = if on_sphere { at } else { Vec3::new(at.x, t.translation.y, at.z) };
        }
    }
}

pub struct CivTargetPlugin;

impl Plugin for CivTargetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolCursor>()
            .add_systems(
                Update,
                cancel_on_escape
                    .after(crate::player_hud::actions)
                    .before(crate::civ_controls::commands_input)
                    .in_set(crate::GameSystems::Controls)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            )
            .add_systems(
                Update,
                (track_cursor, present_rings)
                    .chain()
                    .after(crate::civ_rts::present_fx)
                    .in_set(crate::GameSystems::Presentation)
                    .run_if(in_state(GameStage::Civilization))
                    .run_if(resource_exists::<RunProgress>),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::{CivCellPower, CivCreaturePower, CivTribalPower, PowerAim, HOME_INDEX};

    fn campaign() -> CivCampaign {
        let mut c = CivCampaign::default();
        c.ensure_cities();
        c
    }

    /// The player also owns these cities (the Tribal abilities need four, VERIFIED 0xBF2510).
    fn owning(mut c: CivCampaign, cities: &[usize]) -> CivCampaign {
        for city in cities {
            c.cities[*city].owner = crate::civ::CityOwner::Player;
        }
        c
    }

    fn tool_data(tool: &str) -> &'static PowerData {
        use crate::civ_rules as r;
        [
            &r::INVULNERABILITY,
            &r::HEALING_AURA,
            &r::STATIC_BOMB,
            &r::MIGHTY_BOMB,
            &r::DIPLO_DERVISH,
            &r::BRIBE_BOMB,
            &r::GADGET_BOMB,
            &r::BLACK_CLOUD,
            &r::AD_BLITZ,
            &r::ICBM,
            &r::FANATICAL_UPRISING,
            &r::GLOBAL_MERGER,
        ]
        .into_iter()
        .find(|d| d.tool == tool)
        .unwrap_or_else(|| panic!("no power for {tool}"))
    }

    /// Table 0x14693E8: every retail tool is a rewrite ability and the rules follow 0xCF1D40.
    #[test]
    fn the_tool_table_matches_the_abilities() {
        for tool in TOOLS {
            let _ = tool_data(tool);
        }
        assert_eq!(tool_index("napalm"), Some(0));
        assert_eq!(tool_index("diplodervish"), Some(11));
        assert_eq!(tool_index("nothing"), None);
        for (tool, want) in [
            ("icbm", TargetRule::Immediate),
            ("rising", TargetRule::Immediate),
            ("globaladblitz", TargetRule::Immediate),
            ("blackcloud", TargetRule::RivalCity),
            ("adblitz", TargetRule::RivalCity),
            ("diplodervish", TargetRule::RivalCity),
            ("napalm", TargetRule::Ground),
            ("tacnuke", TargetRule::Ground),
            ("faithheal", TargetRule::Ground),
            ("espionage", TargetRule::Ground),
            ("emp", TargetRule::Ground),
            ("i_field", TargetRule::Ground),
        ] {
            assert_eq!(rule(tool), want, "{tool}");
        }
        // The superweapons of every doctrine are the immediate ones.
        for w in [
            crate::civ::CivSuperweapon::Icbm,
            crate::civ::CivSuperweapon::FanaticalUprising,
            crate::civ::CivSuperweapon::GlobalMerger,
        ] {
            assert_eq!(rule(w.data().tool), TargetRule::Immediate);
        }
    }

    #[test]
    fn a_city_tool_needs_a_click_inside_a_rival_city() {
        let c = campaign();
        let data = tool_data("adblitz");
        let rival = 2;
        let center = c.cities[rival].center();
        // On the rival city: valid, snapped to its centre.
        let near = center + Vec3::new(CITY_TARGET_RADIUS * 0.5, 0.0, 0.0);
        let hit = check(&c, data, near);
        assert!(hit.valid);
        assert_eq!(hit.city, Some(rival));
        assert_eq!(hit.at, center);
        // Past the city radius: invalid, the effects stay on the ground point.
        let far = center + Vec3::new(CITY_TARGET_RADIUS * 1.5, 0.0, 0.0);
        let miss = check(&c, data, far);
        assert!(!miss.valid);
        assert_eq!(miss.at, far);
        // On the player's own capital: invalid (0xBDA440 refuses the player's city).
        let home = check(&c, data, c.cities[HOME_INDEX].center());
        assert!(!home.valid && home.city.is_none());
    }

    #[test]
    fn a_ground_tool_is_valid_anywhere() {
        let c = campaign();
        let nowhere = Vec3::new(1.0e4, 0.0, 1.0e4);
        for tool in ["napalm", "faithheal", "espionage", "emp", "i_field", "tacnuke"] {
            let hit = check(&c, tool_data(tool), nowhere);
            assert!(hit.valid, "{tool}");
            assert_eq!(hit.at, nowhere, "{tool}");
        }
        // The Gadget Bomb reaches a rival city only within its blast radius.
        let bomb = tool_data("tacnuke");
        assert_eq!(check(&c, bomb, nowhere).city, None);
        let rival = c.cities[3].center();
        assert_eq!(check(&c, bomb, rival).city, Some(3));
        // Never the player's own city, whatever is closest.
        let home = check(&c, bomb, c.cities[HOME_INDEX].center()).city;
        assert!(home.is_none_or(|city| !c.cities[city].owned()));
    }

    #[test]
    fn a_click_fires_invalid_or_is_ignored() {
        let mut c = campaign();
        c.tribal_power = Some(CivTribalPower::AdBlitz);
        let center = c.cities[2].center();
        assert_eq!(resolve_click(&c, PowerSlot::Tribal, None), Click::Ignore);
        assert_eq!(
            resolve_click(&c, PowerSlot::Tribal, Some(center)),
            Click::Fire { at: center, city: Some(2) }
        );
        match resolve_click(&c, PowerSlot::Tribal, Some(c.cities[HOME_INDEX].center())) {
            Click::Invalid(message) => {
                assert!(message.contains("Ad Blitz"), "{message}");
                // `civ_audio` plays `ui_civ_error` for it.
                assert!(crate::civ_audio::is_error_message(&message), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// The click names the city: the ability lands on it, wherever the camera looks.
    #[test]
    fn use_power_at_hits_the_clicked_city() {
        let mut c = owning(campaign(), &[1, 2, 4]);
        c.tribal_power = Some(CivTribalPower::BlackCloud);
        c.creature_power = Some(CivCreaturePower::MightyBomb);
        c.cell_power = Some(CivCellPower::HealingAura);
        let mut money = 100_000.0;
        // The camera sits on city 1, the click selects city 3.
        c.last_target = Some(1);
        let used = c
            .use_power_at(PowerSlot::Tribal, &mut money, c.cities[3].center(), PowerAim::Click { city: Some(3) })
            .unwrap();
        assert_eq!(c.black_cloud_city.map(|(city, _)| city), Some(3));
        assert_eq!(used.at, c.cities[3].center());
        // A click on the player's own city is not a target: the rules refuse it.
        let mut c2 = owning(campaign(), &[1, 2, 4]);
        c2.tribal_power = Some(CivTribalPower::BlackCloud);
        let mut money = 100_000.0;
        let err = c2
            .use_power_at(PowerSlot::Tribal, &mut money, Vec3::ZERO, PowerAim::Click { city: Some(HOME_INDEX) })
            .unwrap_err();
        assert!(err.contains("No rival city"), "{err}");
        // A Mighty Bomb lands at the click.
        let spot = Vec3::new(3.0, 0.0, 4.0);
        let used = c
            .use_power_at(PowerSlot::Creature, &mut money, spot, PowerAim::Click { city: None })
            .unwrap();
        assert_eq!(used.at, spot);
        // The camera aim is the old behaviour.
        let mut c3 = owning(campaign(), &[1, 2, 4]);
        c3.tribal_power = Some(CivTribalPower::BlackCloud);
        let mut money = 100_000.0;
        assert!(c3.use_power(PowerSlot::Tribal, &mut money, Vec3::ZERO).is_ok());
    }

    #[test]
    fn a_clicked_gadget_bomb_can_miss_every_city() {
        let mut c = owning(campaign(), &[1, 2, 4]);
        c.tribal_power = Some(CivTribalPower::GadgetBomb);
        let mut money = 100_000.0;
        let nowhere = Vec3::new(1.0e4, 0.0, 1.0e4);
        let used = c
            .use_power_at(PowerSlot::Tribal, &mut money, nowhere, PowerAim::Click { city: None })
            .unwrap();
        assert!(used.message.contains("open ground"), "{}", used.message);
        assert_eq!(money, 100_000.0 - CivTribalPower::GadgetBomb.cost());
        assert!(matches!(used.effect, Some(crate::civ::UnitEffect::Damage { .. })));
    }

    fn civ_app() -> App {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.add_plugins(CivTargetPlugin);
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Civilization);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Civilization);
        for _ in 0..6 {
            app.update();
        }
        app
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release(key);
        input.clear();
    }

    fn armed(app: &App) -> Option<PowerSlot> {
        app.world().resource::<RtsState>().armed
    }

    /// The key arms, the same key drops, Escape drops without opening the pause menu, a refused
    /// tool does not arm (0xCF1D40 / 0xE05510).
    #[test]
    fn keys_arm_toggle_and_escape_drops() {
        let mut app = civ_app();
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.spice = 1.0e6;
            run.civ_campaign.tribal_power = Some(CivTribalPower::AdBlitz);
            run.civ_campaign.cell_power = Some(CivCellPower::StaticBomb);
            for city in [1, 3, 4] {
                run.civ_campaign.cities[city].owner = crate::civ::CityOwner::Player;
            }
        }
        assert_eq!(armed(&app), None);
        tap(&mut app, KeyCode::Digit9);
        assert_eq!(armed(&app), Some(PowerSlot::Tribal));
        assert!(app.world().resource::<RunProgress>().civ_campaign.message.contains("armed"));
        // The cursor tool shows the armed tool (no window: at the camera focus).
        app.update();
        assert!(app.world().resource::<ToolCursor>().active);
        // Another key switches the tool, the same key drops it.
        tap(&mut app, KeyCode::Digit7);
        assert_eq!(armed(&app), Some(PowerSlot::Cell));
        tap(&mut app, KeyCode::Digit7);
        assert_eq!(armed(&app), None);
        assert!(app.world().resource::<RunProgress>().civ_campaign.message.contains("dropped"));
        // Escape drops an armed tool and leaves the pause menu closed.
        tap(&mut app, KeyCode::Digit9);
        assert_eq!(armed(&app), Some(PowerSlot::Tribal));
        tap(&mut app, KeyCode::Escape);
        assert_eq!(armed(&app), None);
        assert!(!app.world().resource::<crate::pause_menu::PauseMenu>().open);
        app.update();
        assert!(!app.world().resource::<ToolCursor>().active);
        // A tool that cannot be used (recharging) is refused and not armed.
        app.world_mut().resource_mut::<RunProgress>().civ_campaign.power_tribal_cooldown = 30.0;
        tap(&mut app, KeyCode::Digit9);
        assert_eq!(armed(&app), None);
        assert!(app.world().resource::<RunProgress>().civ_campaign.message.contains("recharging"));
        // Nothing was spent by arming.
        assert_eq!(app.world().resource::<RunProgress>().spice, 1.0e6);
    }

    #[test]
    fn the_cursor_state_follows_the_check() {
        let mut c = campaign();
        c.tribal_power = Some(CivTribalPower::AdBlitz);
        c.cell_power = Some(CivCellPower::StaticBomb);
        assert_eq!(cursor_state(&c, PowerSlot::Tribal, None), ToolCursor::default());
        let on = cursor_state(&c, PowerSlot::Tribal, Some(c.cities[2].center()));
        assert!(on.active && on.valid && on.area == 0.0 && on.ring == CITY_TARGET_RADIUS);
        let off = cursor_state(&c, PowerSlot::Tribal, Some(c.cities[HOME_INDEX].center()));
        assert!(off.active && !off.valid && off.ring == CURSOR_RING);
        let bomb = cursor_state(&c, PowerSlot::Cell, Some(Vec3::new(5.0, 0.0, 5.0)));
        assert!(bomb.valid && bomb.area > 0.0, "{bomb:?}");
    }
}
