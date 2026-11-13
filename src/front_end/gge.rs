//! The galaxy screen ("galactic game entry"): `GlobalUIGGE-9` (options orb and
//! panel, Sporepedia, creator wheel), `GGEZoomPanel` (Play / Create / Share)
//! and one `GGEGameStarUIv4` per galaxy star, over the menu galaxy of
//! `menu_visuals`. Saved games are stars; free slots are the starter stars
//! of a new game ("Pick A Star For Your New Game", `GalaxyNumVisibleStarterWorlds`
//! 6). Layout roles: `notes\front-end-notes.md` section 2.

use std::sync::Arc;

use bevy::prelude::*;

use super::{slots, FrontEnd, FrontEndActive, Overlay, Tab};
use crate::{
    game::RunProgress,
    spui::{
        model::WinId,
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiPointer, SpuiState},
    },
    stages::GameStage,
};

pub const HUD_LAYOUT: &str = "GlobalUIGGE-9";
pub const ZOOM_LAYOUT: &str = "GGEZoomPanel";
pub const STAR_LAYOUT: &str = "GGEGameStarUIv4";
pub const CONFIRM_LAYOUT: &str = "MessageBox-2";

pub mod id {
    // GlobalUIGGE-9
    pub const ORB: u32 = 0x010E_D852;
    pub const OPTIONS_PANEL: u32 = 0x010E_D874;
    pub const OPTIONS_CLOSE: u32 = 0x0595_E0A8;
    pub const GUIDE: u32 = 0x0447_060A;
    pub const SPORE_COM: u32 = 0x0469_975E;
    pub const SETTINGS: u32 = 0x043B_72D0;
    pub const QUIT: u32 = 0x010F_366D;
    pub const SPOREPEDIA: u32 = 0x02F1_CD64;
    pub const LOGIN: u32 = 0x0481_02CA;
    pub const DEV: u32 = 0x036C_4A10;
    pub const CARGO_TEXT: u32 = 0x03E3_0D88;
    // The creator wheel's controls: `super::creators::id`.
    // GGEZoomPanel
    pub const ZOOM_MENU: u32 = 0x0615_A5FF;
    pub const TAB_SHARE: u32 = 0x0615_B616;
    pub const TAB_PLAY: u32 = 0x061D_D69B;
    pub const TAB_CREATE: u32 = 0x0615_B966;
    // GGEGameStarUIv4
    pub const STAR_NEW: u32 = 0x0465_6452;
    /// The star's disc in the "New Game" group and the planet discs of the
    /// rollover / selected groups.
    pub const STAR_ICON: u32 = 0x0469_7C7B;
    pub const PLANET_SLOT: u32 = 0x0594_6B50;
    /// Creature name of the selected-game panel (hidden in the file).
    pub const SEL_CREATURE: u32 = 0x05FF_9D44;
    pub const ROLL_CREATURE: u32 = 0x05FF_9AD2;
    pub const STAR_ROLLOVER: u32 = 0x046A_9CA7;
    pub const STAR_SELECTED: u32 = 0x0375_3F40;
    pub const STAR_PLAY: u32 = 0x0375_408C;
    pub const STAR_CANCEL: u32 = 0x0670_9A13;
    pub const STAR_DELETE: u32 = 0x03C4_925E;
    /// Rollover texts: planet, game type, creature, date, "file missing",
    /// stage icon.
    pub const ROLL_TEXTS: [u32; 4] = [0x02E7_12E4, 0x0594_A640, 0x05FF_9AD2, 0x046A_9A9B];
    pub const ROLL_CORRUPT: u32 = 0x060A_1103;
    pub const ROLL_ICON: u32 = 0x026E_0002;
    /// The creature picture windows of the rollover / selected panels (empty
    /// image windows above the creature name text: 0xDF70FF / 0xDF7121 fill
    /// them from the game's picture key, entry + 0x3C) and the placeholders
    /// shown instead when the game has none (0xDF7175 / 0xDF7194).
    pub const ROLL_PICTURE: u32 = 0x046E_A568;
    pub const SEL_PICTURE: u32 = 0x02E7_1340;
    pub const ROLL_PLACEHOLDER: u32 = 0x068D_AC20;
    pub const SEL_PLACEHOLDER: u32 = 0x068D_AC21;
    /// Selected-game texts (same order) and icon.
    pub const SEL_TEXTS: [u32; 4] = [0x046E_A550, 0x05FF_9352, 0x05FF_9D44, 0x046E_A507];
    pub const SEL_CORRUPT: u32 = 0x060A_1102;
    pub const SEL_ICON: u32 = 0x026E_0001;
    // MessageBox-2
    pub const MB_TITLE: u32 = 0x0151_0D07;
    pub const MB_TEXT: u32 = 0x0151_0D08;
    pub const MB_BUTTONS: [u32; 4] = [0x0510_7B17, 0x0510_7B18, 0x0510_7B19, 0x0510_7B1A];
    pub const MB_CLOSE: u32 = 0xFFFF_FFF1;
}

/// Saved-game screen margins (`SavedGameLeft/Right/Top/BottomMargin`, gge
/// 250 / 170 / 30 / 64; the statics 0x16A0E98 / EA8 / EB8 / EC8 read by the
/// star placement 0xDF6110): the top-left corner of a saved game's 48 x 48
/// star root stays inside them.
const MARGIN_LEFT: f32 = 250.0;
const MARGIN_RIGHT: f32 = 170.0;
const MARGIN_TOP: f32 = 30.0;
const MARGIN_BOTTOM: f32 = 64.0;
/// Size of the star root window `02E5A4A8` (the file authors it 48 x 48).
const STAR_ROOT: f32 = 48.0;
/// Pointer distance (px) that counts as over a star.
const STAR_PICK_RADIUS: f32 = 24.0;
/// Galaxy units of the disc radius (`menu_visuals` maps the disc to 0.5).
const GALAXY_RADIUS: f32 = 1000.0;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GgeRoot {
    Hud,
    Zoom,
    /// The star UI of slot N.
    Star(usize),
    NewGame,
    Confirm,
}

/// Screen position of each slot's star this frame (surface pixels).
#[derive(Resource, Default)]
pub struct StarScreen(pub Vec<Option<Vec2>>);

#[allow(clippy::too_many_arguments)]
pub fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    options: Res<crate::HudOptions>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut active: ResMut<FrontEndActive>,
    mut fe: ResMut<FrontEnd>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<(Entity, &GgeRoot)>,
    cin: Option<Res<crate::cinematics::Cinematics>>,
) {
    // The galaxy UI waits for `GGE_intro` and hides while a game starts
    // (0xDEBFE0 hides the layouts) or an editor runs.
    let want = *stage.get() == GameStage::MainMenu
        && !editor.open
        && !options.diagnostics
        && fe.launch.is_none()
        && fe.session.is_none()
        && !super::intro::intro_playing(cin.as_deref());
    if !want {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        if active.0 {
            // Leaving the galaxy closes its transient screens.
            fe.options_open = false;
            fe.selected = None;
            fe.new_game = None;
            if matches!(fe.overlay, Some(Overlay::ConfirmDelete(_))) {
                fe.overlay = None;
            }
        }
        active.0 = false;
        return;
    }
    if originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else { return };
    let index = &originals.resource_index;
    if roots.iter().any(|(_, r)| *r == GgeRoot::Hud) {
        // The star layouts follow the games (retail builds one per galaxy
        // star, up to 200): one per saved game and per starter star.
        let have: std::collections::HashSet<usize> = roots
            .iter()
            .filter_map(|(_, r)| match r {
                GgeRoot::Star(s) => Some(*s),
                _ => None,
            })
            .collect();
        let starters = slots::starter_slots(&fe.slots);
        for slot in 0..slots::MAX_SLOTS {
            let wanted = fe.slots.get(slot).is_some_and(|s| !s.is_empty()) || starters.contains(&slot);
            if wanted && !have.contains(&slot) {
                if let Some(layout) = assets.layout(index, STAR_LAYOUT) {
                    let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 90);
                    commands.entity(root).insert(GgeRoot::Star(slot));
                }
            }
        }
        return;
    }
    let Some(hud) = assets.layout(index, HUD_LAYOUT) else { return };
    let spawn = |commands: &mut Commands, assets: &mut SpuiAssets, name: &str, kind, policy, z| {
        if let Some(layout) = assets.layout(index, name) {
            let root = view::spawn(commands, layout, policy, Some(camera), z);
            commands.entity(root).insert(kind);
        }
    };
    let root = view::spawn(&mut commands, hud, RootPolicy::Fit, Some(camera), 100);
    commands.entity(root).insert(GgeRoot::Hud);
    spawn(&mut commands, &mut assets, ZOOM_LAYOUT, GgeRoot::Zoom, RootPolicy::Authored, 101);
    spawn(&mut commands, &mut assets, super::new_game::LAYOUT, GgeRoot::NewGame, RootPolicy::Fit, 110);
    spawn(&mut commands, &mut assets, CONFIRM_LAYOUT, GgeRoot::Confirm, RootPolicy::Centered, 150);
    active.0 = true;
    fe.rescan = true;
}

fn win(ui: &SpuiState, control: u32) -> Option<WinId> {
    ui.find(control)
}

fn show(ui: &mut SpuiState, control: u32, on: bool) {
    if let Some(w) = ui.find(control) {
        ui.set_visible(w, on);
    }
}

fn text(ui: &mut SpuiState, control: u32, value: &str) {
    if let Some(w) = ui.find(control) {
        ui.set_text(w, value.to_string());
    }
}

fn by_element(ui: &SpuiState, element: usize) -> Option<WinId> {
    ui.layout.windows.iter().find(|w| w.element == element).map(|w| w.id)
}

/// Centre the 50x50 state group `control` of a star layout on `p`.
fn anchor(ui: &mut SpuiState, control: u32, p: Vec2) {
    let Some(w) = win(ui, control) else { return };
    let parent = ui.layout.window(w).parent;
    let origin = parent.map(|p| ui.rect(p)).map_or(Vec2::ZERO, |r| Vec2::new(r.x1, r.y1));
    let a = ui.layout.window(w).area;
    let (hw, hh) = ((a[2] - a[0]) * 0.5, (a[3] - a[1]) * 0.5);
    ui.set_area(w, [p.x - origin.x - hw, p.y - origin.y - hh, p.x - origin.x + hw, p.y - origin.y + hh]);
}

/// Size a window to a square of radius `r` around the screen point `c`.
fn place_abs(ui: &mut SpuiState, w: WinId, c: Vec2, r: f32) {
    let parent = ui.layout.window(w).parent;
    let o = parent.map(|p| ui.rect(p)).map_or(Vec2::ZERO, |r| Vec2::new(r.x1, r.y1));
    let area = [c.x - r - o.x, c.y - r - o.y, c.x + r - o.x, c.y + r - o.y];
    if ui.area(w) != area {
        ui.set_area(w, area);
    }
}

/// The frame of the stage icon strip (`AnimatedIcon` 026E0001 / 026E0002, five
/// 30 x 30 frames) for an OpenSpore stage index (Cell .. Space). VERIFIED
/// 0xDF9200 / 0xDF9308: the entry's game id (the low byte of the game mode:
/// 0 Cell, 1 Creature, 2 Tribe, 4 Civilization, 5 Space) picks the frame
/// through the jump table at 0xDF94C8: 1 -> 1, 2 -> 2, 4 -> 3, 5 -> 4, every
/// other id (Cell included) -> 0.
pub fn stage_icon_frame(stage: usize) -> u32 {
    let game_id = [0u32, 1, 2, 4, 5][stage.min(4)];
    match game_id {
        1 => 1,
        2 => 2,
        4 => 3,
        5 => 4,
        _ => 0,
    }
}

/// The planet name shown for a slot: the name given at its start, else the
/// name the Space stage's generator gives that slot's homeworld (slot 0 gets
/// exactly `space::home_planet_name`, seed 0x4F4DE5EE).
pub fn planet_name(fe: &FrontEnd, names: &crate::space_worldgen::SpaceNames, slot: usize) -> String {
    match fe.slots.get(slot).and_then(slots::SlotInfo::saved) {
        Some(s) if !s.planet.trim().is_empty() => s.planet.clone(),
        _ => names.name_for(crate::space_worldgen::NameKind::Planet, 0x4F4D_E5EE + slot as u64),
    }
}

/// Generator of planet names (the installed lexicons), built once.
#[derive(Resource, Clone)]
pub struct PlanetNames(pub Arc<crate::space_worldgen::SpaceNames>);

/// Fill the galaxy screen layouts from the front-end state; project the slot
/// stars; hover and click on stars.
#[allow(clippy::too_many_arguments)]
pub fn bind(
    mut commands: Commands,
    active: Res<FrontEndActive>,
    mut fe: ResMut<FrontEnd>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    names: Option<Res<PlanetNames>>,
    pointer: Res<SpuiPointer>,
    mut prev_pressed: Local<bool>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    galaxy: Query<&Transform, With<crate::menu_visuals::NativeMenuGalaxy>>,
    mut roots: Query<(&mut SpuiState, &GgeRoot)>,
    mut stars_out: Local<Vec<Option<Vec2>>>,
    mut impostors: ResMut<super::impostor::Impostors>,
    mut images: ResMut<Assets<Image>>,
    mut thumbs: ResMut<super::thumbs::Thumbnails>,
    mut pictures: Local<std::collections::HashMap<usize, GamePicture>>,
) {
    let pressed_edge = pointer.pressed && !*prev_pressed;
    *prev_pressed = pointer.pressed;
    if !active.0 {
        return;
    }
    let Some(names) = names else {
        if !originals.resource_index.records.is_empty() {
            commands.insert_resource(PlanetNames(Arc::new(crate::space::space_names(&originals))));
        }
        return;
    };
    if fe.rescan {
        fe.rescan = false;
        if let Some(base) = fe.base.clone() {
            fe.slots = slots::scan(&base);
        }
        if fe.selected.is_some_and(|s| fe.slots.get(s).is_none_or(slots::SlotInfo::is_empty)) {
            fe.selected = None;
        }
    }
    // The saved games' planet impostors (worker thread, cached beside the
    // saves).
    impostors.poll(&mut images, &mut assets);
    // The slots' stars are real stars of the generated galaxy (worker thread);
    // the stars wait for them rather than move when they arrive.
    slots::request_real_stars(&originals.resource_index);
    let stars_pending = slots::stars_pending(&originals.resource_index);
    // Project the stars of the slots onto the screen.
    let screen = roots.iter().find(|(_, k)| **k == GgeRoot::Hud).map(|(u, _)| Vec2::from(u.screen));
    let (Ok((camera, cam_t)), Ok(disc)) = (cameras.single(), galaxy.single()) else { return };
    let Some(screen) = screen.filter(|s| s.x > 0.0) else { return };
    let scale = camera.logical_viewport_size().map_or(Vec2::ONE, |v| screen / v.max(Vec2::ONE));
    let starters = slots::starter_slots(&fe.slots);
    let in_flow = fe.new_game.as_ref().map(|d| d.slot);
    let play = fe.tab == Tab::Play && fe.overlay.is_none();
    stars_out.clear();
    for slot in 0..slots::MAX_SLOTS {
        let saved = fe.slots.get(slot).is_some_and(|s| !s.is_empty());
        let shown = !stars_pending
            && match in_flow {
                Some(s) => s == slot,
                None => play && (saved || starters.contains(&slot)),
            };
        let p = shown
            .then(|| {
                let [gx, gy] = slots::star_position(slot);
                let local = Vec3::new(gx / GALAXY_RADIUS * 0.5, 0.0, gy / GALAXY_RADIUS * 0.5);
                camera.world_to_viewport(cam_t, disc.transform_point(local)).ok()
            })
            .flatten()
            .map(|v| v * scale)
            .map(|v| {
                if !saved {
                    // 0xDF6187: only a star with a saved game is moved inside
                    // the margins.
                    return v;
                }
                // The clamped point is the root's top-left corner (0xDF61A6:
                // position = star - half the root; 0xDF620B..0xDF6277).
                let half = STAR_ROOT * 0.5;
                Vec2::new(
                    (v.x - half).clamp(MARGIN_LEFT, (screen.x - MARGIN_RIGHT).max(MARGIN_LEFT)) + half,
                    (v.y - half).clamp(MARGIN_TOP, (screen.y - MARGIN_BOTTOM).max(MARGIN_TOP)) + half,
                )
            });
        stars_out.push(p);
    }
    // Hover / click on stars (not through a button of a layout).
    let cursor = pointer.position.filter(|_| !pointer.over_ui);
    let near = |p: &Option<Vec2>| cursor.zip(*p).is_some_and(|(c, p)| c.distance(p) <= STAR_PICK_RADIUS);
    let hovered = (0..slots::MAX_SLOTS).find(|s| near(&stars_out[*s]));
    let saved: Vec<bool> = (0..slots::MAX_SLOTS).map(|s| fe.slots.get(s).is_some_and(|i| !i.is_empty())).collect();
    fe.hovered = hovered.filter(|s| saved[*s]);
    fe.hovered_starter = hovered.filter(|s| !saved[*s]);
    if pressed_edge && in_flow.is_none() && fe.overlay.is_none() {
        match hovered {
            Some(s) if saved[s] => fe.selected = Some(s),
            Some(s) => {
                fe.selected = None;
                fe.new_game = Some(super::new_game::Draft::new(s));
            }
            None => {}
        }
    }
    commands.insert_resource(StarScreen(stars_out.clone()));
    // Planets are baked only for the stars on screen (retail draws the planets
    // of the stars it shows); the ones on screen turn.
    let cache = fe.base.as_deref().map(super::impostor::cache_dir);
    let mut impostor_keys = Vec::with_capacity(slots::MAX_SLOTS);
    for slot in 0..slots::MAX_SLOTS {
        let seed = fe.slots.get(slot).and_then(slots::SlotInfo::saved).map(|i| i.seed);
        let key = match (stars_out[slot], seed) {
            (Some(_), Some(seed)) => {
                impostors.touch(seed);
                impostors.request(seed, cache.as_deref(), &originals.resource_index)
            }
            _ => None,
        };
        impostor_keys.push(key);
    }
    impostors.animate(&mut images);
    // The creature pictures of the games whose panel is up (the editors'
    // thumbnail camera, `thumbs`; the Sporepedia card of the creature is the
    // same image).
    if thumbs.cache.is_none() {
        thumbs.cache = fe.base.as_deref().map(super::thumbs::cache_dir);
    }
    let mut picture_keys = vec![None; slots::MAX_SLOTS];
    for (slot, key) in picture_keys.iter_mut().enumerate() {
        let panel = stars_out[slot].is_some() && (fe.hovered == Some(slot) || fe.selected == Some(slot));
        let Some(info) = fe.slots.get(slot).filter(|_| panel) else { continue };
        *key = game_picture(info, &mut pictures, &mut thumbs);
    }
    let index = &originals.resource_index;
    for (mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match *kind {
            GgeRoot::Hud => bind_hud(&mut ui, &fe),
            GgeRoot::Zoom => bind_zoom(&mut ui, &fe),
            GgeRoot::Star(slot) => {
                let p = stars_out.get(slot).copied().flatten();
                let impostor = impostor_keys.get(slot).copied().flatten();
                let picture = picture_keys.get(slot).copied().flatten();
                bind_star(&mut ui, &fe, slot, p, &names.0, &mut assets, index, impostor, picture);
            }
            GgeRoot::Confirm => bind_confirm(&mut ui, &fe, &names.0, &mut assets, index),
            GgeRoot::NewGame => {}
        }
    }
}

fn bind_hud(ui: &mut SpuiState, fe: &FrontEnd) {
    show(ui, id::OPTIONS_PANEL, fe.options_open);
    if let Some(orb) = win(ui, id::ORB) {
        ui.set_selected(orb, fe.options_open);
    }
    // Online and debug controls: not part of OpenSpore (ADAPTED: hidden).
    for control in [id::LOGIN, id::DEV, id::CARGO_TEXT] {
        show(ui, control, false);
    }
    // Spore.com needs a browser; the Spore Guide is online help (disabled).
    for control in [id::SPORE_COM, id::GUIDE] {
        if let Some(w) = win(ui, control) {
            ui.set_enabled(w, false);
        }
    }
    super::creators::bind_wheel(ui, fe);
}

fn bind_zoom(ui: &mut SpuiState, fe: &FrontEnd) {
    // The menu shows while no game is being set up and no full-screen
    // overlay (Sporepedia) covers the galaxy.
    for root in ui.layout.roots.clone() {
        ui.set_visible(root, fe.overlay != Some(Overlay::Sporepedia));
    }
    show(ui, id::ZOOM_MENU, fe.new_game.is_none());
    for (control, tab) in [(id::TAB_PLAY, Tab::Play), (id::TAB_CREATE, Tab::Create), (id::TAB_SHARE, Tab::Share)] {
        if let Some(w) = win(ui, control) {
            ui.set_selected(w, fe.tab == tab);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bind_star(
    ui: &mut SpuiState,
    fe: &FrontEnd,
    slot: usize,
    p: Option<Vec2>,
    names: &crate::space_worldgen::SpaceNames,
    assets: &mut SpuiAssets,
    index: &crate::asset_catalog::ResourceIndex,
    impostor: Option<crate::dbpf::ResourceKey>,
    picture: Option<crate::dbpf::ResourceKey>,
) {
    let info = fe.slots.get(slot);
    let saved = info.is_some_and(|s| !s.is_empty());
    let corrupt = info.is_some_and(|s| s.state == slots::SlotState::Corrupt);
    let selected = p.is_some() && fe.selected == Some(slot) && fe.new_game.is_none();
    let hovered = p.is_some() && !selected && fe.hovered == Some(slot);
    // Every star shows its glow; the "New Game" caption names a free star
    // under the pointer.
    let starter_hover = fe.hovered_starter == Some(slot) && fe.new_game.is_none();
    show(ui, id::STAR_NEW, p.is_some() && !selected);
    // Elements 7 / 13: the caption and its underline (element 10, the star
    // glow, stays).
    for element in [7, 13] {
        if let Some(w) = by_element(ui, element) {
            ui.set_visible(w, starter_hover);
        }
    }
    show(ui, id::STAR_ROLLOVER, hovered && saved);
    show(ui, id::STAR_SELECTED, selected && saved);
    let Some(p) = p else { return };
    anchor(ui, id::STAR_NEW, p);
    // 0xDF6FF9 (star state 1): the three state groups (selected, "New Game",
    // rollover) all take the root's rectangle, so the panels hang off the
    // star's own anchor; nothing else moves them.
    for group in [id::STAR_ROLLOVER, id::STAR_SELECTED] {
        anchor(ui, group, p);
    }
    // The planet: retail draws the saved game's planet at the star, inside the
    // ring frame of the panels: the pair of Space-stage impostor models (the
    // lit sphere and its 1.2 x atmosphere shell) of the star's first planet
    // record, scaled by `GalaxyPlanetSmallImpostorScale` and turning once per
    // 12 s (0xDF6FF9 / 0xE21020, `impostor.rs`); `impostor` is OpenSpore's
    // CPU rendering of those two models with the retail program maths once
    // the worker has the planet. ADAPTED: a 2D image on the glow windows
    // 05946B50 (the halo of the rollover / selected group) and on the star
    // marker (the front end has no 3D layer behind the SPUI); until it is
    // ready (and for free stars) those keep the authored glow (the marker a
    // small warm glow, larger for a saved game).
    let image = impostor
        .filter(|_| !corrupt)
        .map(|file| crate::spui::model::ImageRef { file, uv: [0.0, 0.0, 1.0, 1.0], dims: None });
    for w in ui.find_all(id::PLANET_SLOT) {
        if image.is_some() {
            ui.set_tint(w, 0xFFFF_FFFF);
        }
        ui.set_image(w, image.clone());
    }
    if let Some(w) = win(ui, id::STAR_ICON) {
        ui.set_tint(w, if image.is_some() { 0xFFFF_FFFF } else if saved { 0xD0FF_F2D8 } else { 0xB0FF_F2D8 });
        ui.set_image(w, image.clone());
        place_abs(ui, w, p, if image.is_some() { 20.0 } else if saved { 15.0 } else { 7.0 });
    }
    // The creature picture of a game (046EA568 rollover / 02E71340 selected)
    // comes from a picture key of the saved game; a game without one shows the
    // authored placeholders 068DAC20 / 068DAC21 (0xDF7109 .. 0xDF7194).
    // ADAPTED: OpenSpore saves hold no picture key, so the picture is the
    // game's creature rendered with the editors' thumbnail camera
    // (`thumbs`; a game still in the Cell stage has no creature: placeholder).
    let image = picture.map(|file| crate::spui::model::ImageRef { file, uv: [0.0, 0.0, 1.0, 1.0], dims: None });
    for (window, placeholder) in [(id::ROLL_PICTURE, id::ROLL_PLACEHOLDER), (id::SEL_PICTURE, id::SEL_PLACEHOLDER)] {
        show(ui, window, saved && image.is_some());
        if let Some(w) = win(ui, window) {
            ui.set_image(w, image.clone());
        }
        show(ui, placeholder, saved && image.is_none());
    }
    if !saved || !(hovered || selected) {
        return;
    }
    let planet = planet_name(fe, names, slot);
    let (game_type, creature, date, stage) = match info.and_then(slots::SlotInfo::saved) {
        Some(s) => {
            let stage = RunProgress::stage_order().iter().position(|g| *g == s.stage).unwrap_or(0);
            let name = super::loc(assets, index, super::GGE_TABLE, super::STAGE_NAME_IDS[stage], super::STAGE_NAMES[stage]);
            (name, s.creature.clone(), s.modified.map(slots::format_date).unwrap_or_default(), stage)
        }
        None => (String::new(), String::new(), String::new(), 0),
    };
    let (texts, corrupt_text, icon) = if selected {
        (id::SEL_TEXTS, id::SEL_CORRUPT, id::SEL_ICON)
    } else {
        (id::ROLL_TEXTS, id::ROLL_CORRUPT, id::ROLL_ICON)
    };
    for (control, value) in texts.iter().zip([&planet, &game_type, &creature, &date]) {
        text(ui, *control, value);
    }
    // The creature names are hidden in the file until a game has one.
    for control in [id::SEL_CREATURE, id::ROLL_CREATURE] {
        show(ui, control, !creature.is_empty());
    }
    show(ui, corrupt_text, corrupt);
    // The stage icon strip: the frame of the game's stage (VERIFIED order,
    // `stage_icon_frame`).
    if let Some(w) = win(ui, icon) {
        ui.set_frame(w, Some(stage_icon_frame(stage)));
    }
    if let Some(w) = win(ui, id::STAR_PLAY) {
        ui.set_enabled(w, !corrupt);
    }
}

/// A saved game's creature picture: the thumbnail name and source, read once
/// per save (it changes when the save does).
pub struct GamePicture {
    modified: Option<std::time::SystemTime>,
    source: Option<(String, super::thumbs::Source)>,
}

/// The creature picture of a slot's game once it is rendered (requesting it
/// first); `None` for a game without a creature (the Cell stage).
fn game_picture(
    info: &slots::SlotInfo,
    cache: &mut std::collections::HashMap<usize, GamePicture>,
    thumbs: &mut super::thumbs::Thumbnails,
) -> Option<crate::dbpf::ResourceKey> {
    let summary = info.saved()?;
    let fresh = cache.get(&info.index).is_some_and(|c| c.modified == summary.modified);
    if !fresh {
        let source = (summary.stage != GameStage::Cell)
            .then(|| crate::persistence::load_from(&info.path).ok())
            .flatten()
            .map(|run| (super::pedia::creature_thumb_name(&run), super::thumbs::Source::Creature(info.path.clone())));
        cache.insert(info.index, GamePicture { modified: summary.modified, source });
    }
    let (name, source) = cache.get(&info.index)?.source.as_ref()?;
    thumbs.request(name, source);
    thumbs.image(name).map(|_| super::thumbs::image_key(name))
}

fn bind_confirm(
    ui: &mut SpuiState,
    fe: &FrontEnd,
    names: &crate::space_worldgen::SpaceNames,
    assets: &mut SpuiAssets,
    index: &crate::asset_catalog::ResourceIndex,
) {
    let slot = match fe.overlay {
        Some(Overlay::ConfirmDelete(s)) => Some(s),
        _ => None,
    };
    for root in ui.layout.roots.clone() {
        ui.set_visible(root, slot.is_some());
    }
    let Some(slot) = slot else { return };
    let planet = planet_name(fe, names, slot);
    // The GGE strings "Delete Game" / "Delete ~generic0~?" (2B7A0311:1, :2)
    // with the planet name; buttons "Delete" and "Cancel".
    let title = super::loc(assets, index, super::GGE_TABLE, 0x0000_0001, "Delete Game");
    let body = super::loc(assets, index, super::GGE_TABLE, 0x0000_0002, "Delete ~generic0~?").replace("~generic0~", &planet);
    text(ui, id::MB_TITLE, &title);
    text(ui, id::MB_TEXT, &body);
    let yes = super::loc(assets, index, super::GGE_TABLE, 0x0677_35D4, "Delete");
    let no = super::loc(assets, index, 0xC015_2A6D, 0x05B0_8744, "Cancel");
    for (i, control) in id::MB_BUTTONS.iter().enumerate() {
        if let Some(w) = win(ui, *control) {
            let label = match i {
                2 => Some(&yes),
                3 => Some(&no),
                _ => None,
            };
            ui.set_visible(w, label.is_some());
            if let Some(label) = label {
                ui.set_enabled(w, true);
                ui.set_text(w, label.clone());
            }
        }
    }
}

/// Clicks of the galaxy screen layouts.
#[allow(clippy::too_many_arguments)]
pub fn actions(
    active: Res<FrontEndActive>,
    mut fe: ResMut<FrontEnd>,
    mut roots: Query<(&mut SpuiState, &GgeRoot)>,
    mut save: ResMut<crate::persistence::SaveState>,
    mut exit: MessageWriter<AppExit>,
    mut sounds: MessageWriter<crate::game_audio::PlayCue>,
) {
    if !active.0 {
        return;
    }
    let mut clicks = Vec::new();
    for (mut ui, kind) in &mut roots {
        for w in ui.take_clicks() {
            clicks.push((*kind, ui.layout.window(w).control_id));
        }
    }
    for (kind, control) in clicks {
        // The controls with a retail sound play it (`ui_sounds`); the others the generic click.
        if !crate::ui_sounds::has_retail_click(control) {
            sounds.write(crate::game_audio::PlayCue(crate::game_audio::Cue::UiClick));
        }
        match (kind, control) {
            (GgeRoot::Hud, id::ORB) => fe.options_open = !fe.options_open,
            (GgeRoot::Hud, id::OPTIONS_CLOSE) => fe.options_open = false,
            (GgeRoot::Hud, id::SETTINGS) => {
                fe.options_open = false;
                fe.open(Overlay::Settings);
            }
            (GgeRoot::Hud, id::QUIT) => {
                exit.write(AppExit::Success);
            }
            (GgeRoot::Hud, id::SPOREPEDIA) => fe.open(Overlay::Sporepedia),
            (GgeRoot::Hud, c) => {
                super::creators::click(&mut fe, c);
            }
            (GgeRoot::Zoom, id::TAB_PLAY) => fe.tab = Tab::Play,
            (GgeRoot::Zoom, id::TAB_CREATE) => fe.tab = Tab::Create,
            (GgeRoot::Zoom, id::TAB_SHARE) => {
                // ADAPTED: sharing is online; Share opens the local Sporepedia.
                fe.tab = Tab::Share;
                fe.open(Overlay::Sporepedia);
            }
            // 0xDF7800: Play Game zooms into the star (state 0xC), then loads.
            (GgeRoot::Star(slot), id::STAR_PLAY) => super::intro::begin(&mut fe, super::intro::LaunchKind::Load(slot)),
            (GgeRoot::Star(_), id::STAR_CANCEL) => fe.selected = None,
            (GgeRoot::Star(slot), id::STAR_DELETE) => fe.open(Overlay::ConfirmDelete(slot)),
            (GgeRoot::Confirm, c) if c == id::MB_BUTTONS[2] => {
                if let Some(Overlay::ConfirmDelete(slot)) = fe.overlay {
                    delete_slot(&mut fe, slot, &mut save);
                }
                fe.overlay = None;
            }
            (GgeRoot::Confirm, c) if c == id::MB_BUTTONS[3] || c == id::MB_CLOSE => fe.overlay = None,
            _ => {}
        }
    }
}

/// Play Game: load the slot's save and continue it.
pub fn play_slot(
    fe: &mut FrontEnd,
    slot: usize,
    run: &mut RunProgress,
    save: &mut crate::persistence::SaveState,
    next: &mut NextState<GameStage>,
) {
    let Some(info) = fe.slots.get(slot).cloned() else { return };
    match crate::persistence::load_from(&info.path) {
        Ok(loaded) => {
            let stage = if loaded.finished { GameStage::MainMenu } else { loaded.current_stage() };
            *run = loaded;
            save.path = info.path;
            if fe.save_suspended {
                save.enabled = true;
                fe.save_suspended = false;
            }
            fe.selected = None;
            next.set(stage);
        }
        Err(error) => fe.status = format!("Cannot load {}: {error}", info.path.display()),
    }
}

/// Delete a saved game after the confirmation.
pub fn delete_slot(fe: &mut FrontEnd, slot: usize, save: &mut crate::persistence::SaveState) {
    let Some(info) = fe.slots.get(slot).cloned() else { return };
    if let Err(error) = slots::delete(&info.path) {
        fe.status = format!("Cannot delete {}: {error}", info.path.display());
        return;
    }
    // The game in memory belongs to the deleted file: do not write it back
    // when the menu saves on exit.
    if save.path == info.path && save.enabled {
        save.enabled = false;
        fe.save_suspended = true;
    }
    fe.selected = None;
    fe.rescan = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_icons_follow_the_retail_jump_table() {
        // Retail ids 0 / 1 / 2 / 4 / 5 (Cell .. Space) -> frames 0 .. 4.
        assert_eq!([0, 1, 2, 3, 4].map(stage_icon_frame), [0, 1, 2, 3, 4]);
        assert_eq!(stage_icon_frame(9), 4);
    }
}
