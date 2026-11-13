//! The galaxy screen's creator wheel (`GlobalUIGGE-9` window 03685038, "Choose
//! A Creator") and the editor sessions it opens from the menu.
//!
//! VERIFIED (`notes\front-end-notes.md` section 5): the wheel's click handler
//! 0xDE1A90 sends every sub button to 0xDE0A90 (select: the button becomes the
//! chosen creator, its info card shows) and Launch Selected Creator
//! (06260BC8) to 0xDE1380, which maps the chosen button to an editor setup and
//! starts it (0xDE0E40). The five hub buttons have no case in the handler:
//! they open their group of sub buttons (layout effects). The outfitters
//! (0625CD52..54) first ask Sporepedia for a creature (0xDE0B70).
//!
//! OpenSpore opens the converted editors (`creature_editor`, `rigid_editor`,
//! `outfitter`) in the main menu on a scratch record: the loaded game is put
//! aside and saving is off for the session, then both come back when the
//! editor closes. What the player accepts is kept in the Sporepedia store
//! (`store`), as retail saves menu creations to Sporepedia. An outfitter
//! first asks Sporepedia for a creature (0xDE0B70: the creature shopper),
//! then dresses it.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use bevy::prelude::*;

use super::{store, FrontEnd, FrontEndActive, Overlay, Tab};
use crate::{creature_editor_data::SetupKind, game::RunProgress, spui::view::SpuiState, stages::GameStage};

pub mod id {
    pub const WHEEL: u32 = 0x0368_5038;
    pub const LAUNCH: u32 = 0x0626_0BC8;
    pub const OUTFITTER: u32 = 0x0626_0BD0;
    pub const CLOSE: u32 = 0x062D_88E9;
    /// The hub buttons' group (hidden in the file) and the "Choose A
    /// Creator" card.
    pub const HUBS: u32 = 0x062D_7F3C;
    pub const CHOOSE_CARD: u32 = 0x0595_D7D7;
    /// Info cards no button of this wheel selects (Vehicle without a domain,
    /// Cell Creator, the Tribal-stage Creature Creator).
    pub const SPARE_CARDS: [u32; 3] = [0x0595_D7D2, 0x0629_5358, 0x0629_5410];
}

/// A hub of the wheel: its button, the hidden group of its sub buttons and
/// the info card shown while it is open.
#[derive(Clone, Copy, Debug)]
pub struct Hub {
    pub button: u32,
    pub group: u32,
    pub card: u32,
}

pub const HUBS: [Hub; 5] = [
    // Creature (paw): Early Creature Creator, Creature Creator, outfitters.
    Hub { button: 0x036A_D4A4, group: 0x0625_F848, card: 0x0595_D7D3 },
    // Buildings.
    Hub { button: 0x0465_63E4, group: 0x0625_EFF0, card: 0x0595_D7D0 },
    // Land, sea and air vehicles.
    Hub { button: 0x0625_F360, group: 0x0465_6420, card: 0x0628_29B9 },
    Hub { button: 0x0629_C5E4, group: 0x0629_C53D, card: 0x0595_D7D1 },
    Hub { button: 0x062A_F8B4, group: 0x062A_A208, card: 0x0595_D7D4 },
];

/// A creator button of the wheel.
#[derive(Clone, Copy, Debug)]
pub struct Creator {
    pub button: u32,
    /// The editor setup 0xDE1380 starts (its registered name).
    pub setup: &'static str,
    /// The OpenSpore editor session; `None` = not available from the menu.
    pub kind: Option<SetupKind>,
    pub card: u32,
    pub hub: Option<usize>,
    /// `--front-end create:<name>` (captures).
    pub name: &'static str,
}

const fn c(button: u32, setup: &'static str, kind: Option<SetupKind>, card: u32, hub: Option<usize>, name: &'static str) -> Creator {
    Creator { button, setup, kind, card, hub, name }
}

/// Every sub button with the editor 0xDE1380 maps it to (VERIFIED; the setup
/// names are the registered names of the hashes) and its info card.
pub const CREATORS: [Creator; 19] = [
    c(0x0652_6B7D, "CellToCreatureEditor", Some(SetupKind::CellToCreature), 0x0652_713F, Some(0), "early-creature"),
    c(0x0625_CD51, "CreatureEditorExtraLarge", Some(SetupKind::Creature), 0x0595_D7D3, Some(0), "creature"),
    // The outfitters ask the Sporepedia creature shopper first (0xDE0B70).
    c(0x0625_CD52, "TribalOutfitter", Some(SetupKind::Tribal), 0x0595_D7D5, Some(0), "tribal-outfitter"),
    c(0x0625_CD53, "CivOutfitter", Some(SetupKind::Civ), 0x0629_54E8, Some(0), "civ-outfitter"),
    c(0x0625_CD54, "SpaceOutfitter", Some(SetupKind::Space), 0x0629_55C0, Some(0), "space-outfitter"),
    c(0x0409_6DD0, "BuildingCityHall", Some(SetupKind::BuildingCityHall), 0x0628_2D10, Some(1), "city-hall"),
    c(0x0409_6DD3, "BuildingHouse", Some(SetupKind::BuildingHouse), 0x0628_2D11, Some(1), "house"),
    c(0x0409_6DD7, "BuildingFactory", Some(SetupKind::BuildingFactory), 0x0628_2D12, Some(1), "factory"),
    c(0x0409_6DDA, "BuildingEntertainment", Some(SetupKind::BuildingEntertainment), 0x0628_2D13, Some(1), "entertainment"),
    c(0x0628_565E, "VehicleMilitaryLand", Some(SetupKind::VehicleMilitaryLand), 0x0629_C7A0, Some(2), "land-military"),
    c(0x0628_565F, "VehicleEconomicLand", Some(SetupKind::VehicleEconomicLand), 0x0629_C7A1, Some(2), "land-economic"),
    c(0x0628_5660, "VehicleCulturalLand", Some(SetupKind::VehicleCulturalLand), 0x0629_C7A2, Some(2), "land-religious"),
    c(0x0629_C446, "VehicleMilitaryWater", Some(SetupKind::VehicleMilitaryWater), 0x0629_CB20, Some(3), "sea-military"),
    c(0x0629_C447, "VehicleEconomicWater", Some(SetupKind::VehicleEconomicWater), 0x0629_CB21, Some(3), "sea-economic"),
    c(0x0629_C448, "VehicleCulturalWater", Some(SetupKind::VehicleCulturalWater), 0x0629_CB22, Some(3), "sea-religious"),
    c(0x062A_FB0D, "VehicleMilitaryAir", Some(SetupKind::VehicleMilitaryAir), 0x0629_CB23, Some(4), "air-military"),
    c(0x062A_FB1C, "VehicleEconomicAir", Some(SetupKind::VehicleEconomicAir), 0x0629_CB24, Some(4), "air-economic"),
    c(0x062A_FB2B, "VehicleCulturalAir", Some(SetupKind::VehicleCulturalAir), 0x0629_CB25, Some(4), "air-religious"),
    c(0x0465_6452, "UFOEditorSetup", Some(SetupKind::Ufo), 0x0595_D7D6, None, "ufo"),
];

pub fn by_name(name: &str) -> Option<usize> {
    CREATORS.iter().position(|c| c.name == name)
}

/// True while an editor opened from the wheel runs in the main menu: the
/// retail editors (`creature_editor::routes`) take the main menu then.
static MENU_SESSION: AtomicBool = AtomicBool::new(false);

pub fn menu_session() -> bool {
    MENU_SESSION.load(Ordering::Relaxed)
}

/// The outfitter a menu session runs (0 none, 1 Tribal, 2 Civ, 3 Space).
static MENU_OUTFITTER: AtomicU8 = AtomicU8::new(0);

/// The outfitter setup of the running menu session (`outfitter::routes`).
pub fn menu_outfitter() -> Option<SetupKind> {
    match MENU_OUTFITTER.load(Ordering::Relaxed) {
        1 => Some(SetupKind::Tribal),
        2 => Some(SetupKind::Civ),
        3 => Some(SetupKind::Space),
        _ => None,
    }
}

fn set_menu_outfitter(kind: Option<SetupKind>) {
    let code = match kind {
        Some(SetupKind::Tribal) => 1,
        Some(SetupKind::Civ) => 2,
        Some(SetupKind::Space) => 3,
        _ => 0,
    };
    MENU_OUTFITTER.store(code, Ordering::Relaxed);
}

/// The stage whose outfit record a menu outfitter edits (the main menu
/// stands for the outfitter's stage while it runs).
pub fn outfit_stage(stage: &GameStage) -> GameStage {
    match (stage, menu_outfitter()) {
        (GameStage::MainMenu, Some(SetupKind::Tribal)) => GameStage::Tribal,
        (GameStage::MainMenu, Some(SetupKind::Civ)) => GameStage::Civilization,
        (GameStage::MainMenu, Some(SetupKind::Space)) => GameStage::Space,
        _ => stage.clone(),
    }
}

/// An editor to open from the menu on a scratch record (a new creation, a
/// Sporepedia creation to edit, a creature to outfit).
pub struct MenuLaunch {
    pub kind: SetupKind,
    pub scratch: Box<RunProgress>,
    /// The stored creation the result replaces (editing a store entry).
    pub replace: Option<u64>,
}

impl MenuLaunch {
    pub fn new(kind: SetupKind) -> Self {
        Self { kind, scratch: Box::default(), replace: None }
    }
}

/// The loaded game put aside while a menu editor session runs on a scratch
/// record.
pub struct MenuSession {
    pub kind: SetupKind,
    saved: Box<RunProgress>,
    save_enabled: bool,
    /// The scratch record as the editor received it, and the creation it
    /// replaces.
    start: Box<RunProgress>,
    replace: Option<u64>,
    /// The editor has been seen open (its end closes the session).
    opened: bool,
    frames: u32,
    last: Option<(bool, bool, bool)>,
}

/// A spending allowance for menu outfitters (the setup's `editorMaxBudget`
/// caps it; ADAPTED: retail menu editors have no stage currency).
const MENU_BUDGET: f32 = 1.0e6;

/// What a menu session made: the creation to keep, if the player accepted a
/// change (a creature creator counts accepted sessions, a rigid editor
/// commits its design, an outfitter its outfit).
pub fn session_result(kind: SetupKind, start: &RunProgress, end: &RunProgress) -> Option<(String, store::StoredKind)> {
    if kind.is_outfitter() {
        let setup = kind.setup_name();
        let outfit = |r: &RunProgress| {
            let mut r = r.clone();
            store::outfit_record(&mut r, setup).map(|o| o.parts.clone()).unwrap_or_default()
        };
        let worn = outfit(end);
        if worn == outfit(start) {
            return None;
        }
        let mut creature = store::creature_of(end);
        creature.outfitter = setup.to_string();
        creature.outfit = worn;
        return Some((end.creature_creation.name.clone(), store::StoredKind::Creature(creature)));
    }
    if kind.is_rigid() {
        let setup = kind.setup_name();
        let design = end.rigid.designs.get(setup)?;
        if start.rigid.designs.get(setup) == Some(design) || !design.parts.iter().any(|p| p.block.is_some()) {
            return None;
        }
        return Some((design.name.clone(), store::StoredKind::Design { setup: setup.to_string(), design: design.clone() }));
    }
    (end.creature_creation.visits > start.creature_creation.visits)
        .then(|| (end.creature_creation.name.clone(), store::StoredKind::Creature(store::creature_of(end))))
}

/// The wheel's part of `GlobalUIGGE-9`.
pub fn bind_wheel(ui: &mut SpuiState, fe: &FrontEnd) {
    let show = |ui: &mut SpuiState, control: u32, on: bool| {
        for w in ui.find_all(control) {
            ui.set_visible(w, on);
        }
    };
    let wheel = fe.tab == Tab::Create && fe.overlay.is_none() && fe.session.is_none();
    show(ui, id::WHEEL, wheel);
    if !wheel {
        return;
    }
    show(ui, id::HUBS, true);
    for (i, hub) in HUBS.iter().enumerate() {
        show(ui, hub.group, fe.hub == Some(i));
        if let Some(w) = ui.find(hub.button) {
            ui.set_selected(w, fe.hub == Some(i));
        }
    }
    for (i, creator) in CREATORS.iter().enumerate() {
        if let Some(w) = ui.find(creator.button) {
            ui.set_enabled(w, true);
            ui.set_selected(w, fe.creator == Some(i));
        }
    }
    // The chosen creator's card, else the open hub's, else "Choose A
    // Creator".
    let card = fe.creator.map(|c| CREATORS[c].card).or(fe.hub.map(|h| HUBS[h].card));
    let cards = CREATORS.iter().map(|c| c.card).chain(HUBS.iter().map(|h| h.card)).chain(id::SPARE_CARDS);
    for control in cards.collect::<Vec<_>>() {
        show(ui, control, Some(control) == card);
    }
    show(ui, id::CHOOSE_CARD, card.is_none());
    if let Some(w) = ui.find(id::LAUNCH) {
        ui.set_enabled(w, fe.creator.is_some_and(|c| CREATORS[c].kind.is_some()));
    }
    show(ui, id::OUTFITTER, false);
}

/// A click on the wheel; `true` when it was one of its controls.
pub fn click(fe: &mut FrontEnd, control: u32) -> bool {
    if control == id::CLOSE {
        fe.tab = Tab::Play;
        fe.hub = None;
        fe.creator = None;
        return true;
    }
    if let Some(h) = HUBS.iter().position(|h| h.button == control) {
        fe.hub = if fe.hub == Some(h) { None } else { Some(h) };
        fe.creator = None;
        return true;
    }
    if let Some(i) = CREATORS.iter().position(|c| c.button == control) {
        fe.creator = Some(i);
        if CREATORS[i].hub.is_some() {
            fe.hub = CREATORS[i].hub;
        }
        return true;
    }
    if control == id::LAUNCH {
        match fe.creator.and_then(|c| CREATORS[c].kind) {
            // An outfitter: choose the creature in Sporepedia first.
            Some(kind) if kind.is_outfitter() => {
                fe.shopper = Some(kind);
                fe.open(Overlay::Sporepedia);
            }
            Some(_) => fe.launch_creator = fe.creator,
            None => {}
        }
        return true;
    }
    false
}

/// A creature to outfit: its record with the outfitter's allowance (and the
/// creator's starter parts when it has none).
pub fn outfit_scratch(mut run: RunProgress, catalog: &crate::asset_catalog::PartCatalog) -> RunProgress {
    if run.parts.is_empty() && !catalog.parts.is_empty() {
        run.parts = crate::editor::starter_creature_parts(&run, catalog);
        run.sync_legacy_accessory();
    }
    run.spice = MENU_BUDGET;
    run
}

/// Start the chosen creator (Launch Selected Creator), end a session when its
/// editor has closed.
#[allow(clippy::too_many_arguments)]
pub fn sessions(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    active: Res<FrontEndActive>,
    mut fe: ResMut<FrontEnd>,
    mut run: ResMut<RunProgress>,
    mut save: ResMut<crate::persistence::SaveState>,
    mut state: ResMut<crate::editor::EditorState>,
    editor: Option<ResMut<crate::creature_editor::CreatureEditor>>,
    rigid: Option<ResMut<crate::rigid_editor::RigidEditor>>,
    outfitter: Option<Res<crate::outfitter::Outfitter>>,
    mut pedia_store: ResMut<store::Store>,
    mut originals: ResMut<crate::Originals>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    menu_previews: Query<Entity, (With<crate::editor::MenuPreview>, With<crate::game::Player>)>,
) {
    let (Some(mut editor), Some(mut rigid)) = (editor, rigid) else { return };
    if let Some(i) = fe.launch_creator.take() {
        if let Some(kind) = CREATORS[i].kind.filter(|k| !k.is_outfitter()) {
            info!("front end: Launch Selected Creator: {}", CREATORS[i].setup);
            fe.menu_launch = Some(MenuLaunch::new(kind));
        }
    }
    if let Some(launch) = fe.menu_launch.take() {
        if !active.0 || fe.session.is_some() || state.open || *stage.get() != GameStage::MainMenu {
            return;
        }
        let kind = launch.kind;
        // The game in memory steps aside for a scratch record; nothing is
        // saved while the editor runs.
        let saved = Box::new(std::mem::replace(&mut *run, *launch.scratch));
        fe.session = Some(MenuSession {
            kind,
            saved,
            save_enabled: save.enabled,
            start: Box::new(run.clone()),
            replace: launch.replace,
            opened: false,
            frames: 0,
            last: None,
        });
        save.enabled = false;
        MENU_SESSION.store(true, Ordering::Relaxed);
        set_menu_outfitter(kind.is_outfitter().then_some(kind));
        // The creature's own part models (a stage has them loaded already).
        if !kind.is_rigid() && !run.parts.is_empty() {
            crate::editor::load_saved_models(&run, &mut originals, &GameStage::Creature, &mut meshes, &mut images, &mut materials);
        }
        if kind.is_rigid() {
            rigid.reopen = Some(kind);
        } else {
            crate::creature_editor::request_setup(&mut editor, &mut state, kind);
        }
        info!("front end: {} opened from the galaxy screen (scratch record)", kind.setup_name());
        return;
    }
    let Some(session) = fe.session.as_mut() else { return };
    session.frames += 1;
    // A menu outfitter dresses its own preview creature (`outfitter::sync`);
    // the free creator's menu preview (`editor::scene`) steps aside.
    if session.kind.is_outfitter() {
        for e in &menu_previews {
            commands.entity(e).remove::<crate::game::Player>().insert(Visibility::Hidden);
        }
    }
    if state.open {
        session.opened = true;
    }
    let now = (state.open, editor.active, rigid.active);
    if session.last != Some(now) {
        debug!("front end: menu editor state (open, creator, rigid) = {now:?}");
        session.last = Some(now);
    }
    let left_menu = *stage.get() != GameStage::MainMenu;
    let outfitting = outfitter.is_some_and(|o| o.active);
    let closed = session.opened && !state.open && !editor.active && !rigid.active && !outfitting;
    // A session whose editor never opened gives up after a few seconds.
    if closed || left_menu || (!session.opened && session.frames > 300) {
        let session = fe.session.take().expect("session");
        info!("front end: menu editor {:?} closed; the game is back", session.kind);
        // What the player accepted goes to Sporepedia.
        let outfit_after = fe.outfit_after.take();
        if let Some((name, kind)) = session_result(session.kind, &session.start, &run) {
            let id = pedia_store.put(name, kind, session.replace);
            match pedia_store.store() {
                Ok(()) => info!("front end: creation {id} kept in the Sporepedia store"),
                Err(error) => fe.status = format!("Cannot save the Sporepedia store: {error}"),
            }
            // The shopper's Create New goes on to the outfitter.
            if let Some(outfitter) = outfit_after.filter(|_| session.kind == SetupKind::Creature) {
                fe.menu_launch = super::pedia::outfit_stored(id, outfitter, &pedia_store, &originals.part_catalog);
            }
        }
        set_menu_outfitter(None);
        *run = *session.saved;
        save.enabled = session.save_enabled;
        state.rigid = false;
        MENU_SESSION.store(false, Ordering::Relaxed);
        fe.creator = None;
        fe.rescan = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_buttons_open_the_decompiled_editors() {
        // 0xDE1380: the setup hashes are the FNV-1 ids of the registered names.
        let hashes = [
            (0x0652_6B7D, 0xE46C_381E),
            (0x0625_CD51, 0x465C_50BA),
            (0x0409_6DD0, 0x99E9_2F05),
            (0x0409_6DD3, 0x4E3F_7777),
            (0x0409_6DD7, 0x8707_BE7D),
            (0x0409_6DDA, 0x72C4_9181),
            (0x0628_565E, 0x7D43_3FAD),
            (0x0628_565F, 0xF670_AA43),
            (0x0628_5660, 0x9AD7_D4AA),
            (0x0629_C446, 0x8F96_3DCB),
            (0x0629_C447, 0x2A51_47A9),
            (0x0629_C448, 0x1F2A_25B6),
            (0x062A_FB0D, 0x441C_D3E6),
            (0x062A_FB1C, 0x1A4E_0708),
            (0x062A_FB2B, 0x449C_040F),
            (0x0465_6452, 0x96B2_4187),
        ];
        for (button, hash) in hashes {
            let creator = CREATORS.iter().find(|c| c.button == button).expect("button on the wheel");
            assert_eq!(crate::world_objects::name_id(creator.setup), hash, "{}", creator.setup);
            assert!(creator.kind.is_some());
        }
        // The outfitters open their accessories editors.
        for (name, kind) in [("tribal-outfitter", SetupKind::Tribal), ("civ-outfitter", SetupKind::Civ), ("space-outfitter", SetupKind::Space)] {
            assert_eq!(CREATORS[by_name(name).unwrap()].kind, Some(kind));
        }
    }

    #[test]
    fn hubs_open_their_group_and_buttons_choose_a_creator() {
        let mut fe = FrontEnd { tab: Tab::Create, ..default() };
        assert!(click(&mut fe, HUBS[1].button));
        assert_eq!(fe.hub, Some(1));
        let house = by_name("house").unwrap();
        assert!(click(&mut fe, CREATORS[house].button));
        assert_eq!((fe.hub, fe.creator), (Some(1), Some(house)));
        assert!(click(&mut fe, id::LAUNCH));
        assert_eq!(fe.launch_creator, Some(house));
        // An outfitter asks Sporepedia for a creature first.
        let tribal = by_name("tribal-outfitter").unwrap();
        fe.launch_creator = None;
        click(&mut fe, CREATORS[tribal].button);
        click(&mut fe, id::LAUNCH);
        assert_eq!(fe.launch_creator, None);
        assert_eq!((fe.shopper, fe.overlay), (Some(SetupKind::Tribal), Some(Overlay::Sporepedia)));
        fe.overlay = None;
        assert!(click(&mut fe, id::CLOSE));
        assert_eq!((fe.tab, fe.hub, fe.creator), (Tab::Play, None, None));
    }

    #[test]
    fn accepted_menu_sessions_become_sporepedia_creations() {
        let start = RunProgress::default();
        // A creature creator counts accepted sessions.
        let mut end = start.clone();
        assert!(session_result(SetupKind::Creature, &start, &end).is_none(), "cancelled");
        end.creature_creation.visits += 1;
        end.creature_creation.name = "Blib".into();
        let (name, kind) = session_result(SetupKind::Creature, &start, &end).expect("accepted");
        assert_eq!(name, "Blib");
        assert!(matches!(kind, store::StoredKind::Creature(_)));
        // A rigid editor commits a design with blocks.
        let mut end = start.clone();
        let part = crate::game::VehiclePart {
            instance: 1,
            definition_id: Some(2),
            slot: 0,
            yaw: 0.0,
            scale: 1.0,
            offset: [0.0; 3],
            block: Some(crate::rigid_editor::RigidBlock::default()),
        };
        end.rigid.designs.insert("BuildingHouse".into(), crate::rigid_editor::RigidDesign { name: "Hut".into(), parts: vec![part] });
        let (name, kind) = session_result(SetupKind::BuildingHouse, &start, &end).expect("committed");
        assert_eq!(name, "Hut");
        assert!(matches!(kind, store::StoredKind::Design { ref setup, .. } if setup == "BuildingHouse"));
        assert!(session_result(SetupKind::BuildingHouse, &end, &end).is_none(), "unchanged");
        // An outfitter keeps the creature and its outfit.
        let mut end = start.clone();
        end.outfits.civ.parts.push(serde_json::from_str(r#"{"instance":7,"bone":0}"#).unwrap());
        let (_, kind) = session_result(SetupKind::Civ, &start, &end).expect("dressed");
        let store::StoredKind::Creature(c) = kind else { panic!("a creature") };
        assert_eq!((c.outfitter.as_str(), c.outfit.len()), ("CivAccessoriesEditorSetup", 1));
    }

    #[test]
    fn a_menu_outfitter_edits_its_stage_record() {
        set_menu_outfitter(Some(SetupKind::Space));
        assert_eq!(menu_outfitter(), Some(SetupKind::Space));
        assert_eq!(outfit_stage(&GameStage::MainMenu), GameStage::Space);
        assert_eq!(outfit_stage(&GameStage::Tribal), GameStage::Tribal);
        set_menu_outfitter(None);
        assert_eq!(outfit_stage(&GameStage::MainMenu), GameStage::MainMenu);
    }
}
