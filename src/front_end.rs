//! The original front end on the retail SPUI layouts: the galaxy game entry
//! ("GGE", `GlobalUIGGE-9` + `GGEZoomPanel` + one `GGEGameStarUIv4` per saved
//! game star), the new-game flow (`GGENewGameFlowUIv1-flip`), the settings
//! dialog (`GameSettings` and its pages), the in-stage pause frame
//! (`GlobalUIPause`) with the options menu commands of every stage HUD, and
//! Sporepedia (`AssetBrowser`).
//!
//! Reverse-engineering notes: `notes\front-end-notes.md` (layout inventory,
//! the `gge` tuning file, cinematics and mix modes, the graphics options,
//! the profile data, the Maxis creations). Saved games map to galaxy stars
//! through [`slots`]; the per-profile stage unlocks live in [`profile`], the
//! menu creations in the Sporepedia [`store`].

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub mod creators;
pub mod gge;
pub mod graphics;
pub mod impostor;
pub mod intro;
pub mod launch_screen;
pub mod maxis;
pub mod new_game;
pub mod pause;
pub mod pedia;
pub mod png_creation;
pub mod profile;
pub mod settings;
pub mod slots;
pub mod store;
pub mod thumbs;
pub mod turntable;

use crate::spui::view;

/// Control ids shared by several front-end layouts.
pub mod id {
    // GlobalUIOptions-2 (the in-stage options menu of every HUD).
    pub use crate::cell_hud::id::{
        MENU_EXIT, MENU_GUIDE, MENU_QUIT, MENU_RESUME, MENU_SAVE, MENU_SETTINGS,
    };
    /// Sporepedia buttons of the stage HUDs (Cell, Creature/Tribal/Civ,
    /// Space, GGE) and "My Collections".
    pub const SPOREPEDIA: [u32; 4] = [0x0447_060C, 0x0624_4660, 0x0269_9440, 0x02F1_CD64];
    pub const MY_COLLECTIONS: u32 = 0x0624_4208;
}

/// What a click in an options menu or HUD asks of the front end. Every stage
/// HUD forwards these instead of handling the menu itself, so the in-stage
/// menu behaves the same in all stages (see [`pause::commands`]).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Resume,
    Save,
    ExitToMenu,
    Quit,
    Settings,
    SporeGuide,
    Sporepedia,
}

/// The front-end command of an options-menu or HUD control, if it is one.
pub fn menu_command(control: u32) -> Option<MenuCommand> {
    use MenuCommand::*;
    Some(match control {
        id::MENU_RESUME => Resume,
        id::MENU_SAVE => Save,
        id::MENU_EXIT => ExitToMenu,
        id::MENU_QUIT => Quit,
        id::MENU_SETTINGS => Settings,
        id::MENU_GUIDE => SporeGuide,
        id::MY_COLLECTIONS => Sporepedia,
        c if id::SPOREPEDIA.contains(&c) => Sporepedia,
        _ => return None,
    })
}

/// New-game choices kept with the game (`RunProgress::game_setup`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GameSetup {
    /// "Name Your Planet" (`MaxPlanetNameLength` 32).
    pub planet_name: String,
    /// 0 easy, 1 normal, 2 hard ("Choose Your Difficulty Level").
    pub difficulty: u8,
    /// "Choose Your Empire Specialty": retail's value (`GameLoadParameters + 0x90`,
    /// 0xDF2740) when `specialty_retail` is set: 0 military, 1 religious (cultural), 2
    /// economic; saves written before that hold OpenSpore's own order (1 economic,
    /// 2 religious) and have the flag off.
    pub specialty: Option<u8>,
    #[serde(default = "legacy_specialty_numbering")]
    pub specialty_retail: bool,
    /// Index of the stage the game was started at (0 Cell .. 4 Space).
    pub start_stage: usize,
}

/// Saves without `specialty_retail` use OpenSpore's old specialty numbering.
fn legacy_specialty_numbering() -> bool {
    false
}

impl GameSetup {
    /// The specialty in retail's numbering (0 military, 1 religious, 2 economic).
    pub fn retail_specialty(&self) -> Option<u8> {
        self.specialty.map(|v| match (self.specialty_retail, v) {
            (false, 1) => 2,
            (false, 2) => 1,
            (_, v) => v,
        })
    }
}

impl Default for GameSetup {
    fn default() -> Self {
        Self { planet_name: String::new(), difficulty: 1, specialty: None, specialty_retail: true, start_stage: 0 }
    }
}

/// True while the retail galaxy screen is up: the legacy bevy-UI main menu
/// (`player_hud`) steps aside.
#[derive(Resource, Default)]
pub struct FrontEndActive(pub bool);

/// Top-left panel of the galaxy screen (`GGEZoomPanel`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Play,
    Create,
    Share,
}

/// Modal screens over the galaxy or a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Settings,
    Sporepedia,
    /// Delete-game confirmation for the slot.
    ConfirmDelete(usize),
}

/// Front-end state.
#[derive(Resource, Default)]
pub struct FrontEnd {
    /// Slot 0 file (the `--save-path`), taken from `SaveState` once.
    pub base: Option<PathBuf>,
    pub slots: Vec<slots::SlotInfo>,
    /// Read the slots again (entering the galaxy, after a delete).
    pub rescan: bool,
    pub tab: Tab,
    /// Saved-game star under the pointer and the one clicked.
    pub hovered: Option<usize>,
    pub selected: Option<usize>,
    /// Starter star under the pointer.
    pub hovered_starter: Option<usize>,
    /// The new game being set up (its star is `draft.slot`).
    pub new_game: Option<new_game::Draft>,
    /// The galaxy screen's own options panel (orb).
    pub options_open: bool,
    pub overlay: Option<Overlay>,
    /// Open hub and chosen creator of the creator wheel (indices into
    /// [`creators::HUBS`] / [`creators::CREATORS`]).
    pub hub: Option<usize>,
    pub creator: Option<usize>,
    /// Launch Selected Creator was pressed (index into `creators::CREATORS`).
    pub launch_creator: Option<usize>,
    /// An editor to open on a scratch record (a new creation, Sporepedia's
    /// Edit / Outfitter, the shopper's pick).
    pub menu_launch: Option<creators::MenuLaunch>,
    /// Sporepedia runs as the creature shopper of this outfitter.
    pub shopper: Option<crate::creature_editor_data::SetupKind>,
    /// The shopper's Create New: open this outfitter on the creature the
    /// Creature Creator makes.
    pub outfit_after: Option<crate::creature_editor_data::SetupKind>,
    /// Sporepedia runs as the shopper of a new-game card (creature, city hall,
    /// vehicle / UFO) and the creation picked in it (`new_game::Shop`).
    pub flow_pick: Option<new_game::Shop>,
    pub picked: Option<pedia::Entry>,
    /// An editor opened from the wheel runs on a scratch record.
    pub session: Option<creators::MenuSession>,
    /// A game being started (zoom into its star, `intro::launch`).
    pub launch: Option<intro::Launch>,
    /// Play the `GGE2CLG` comet once the new Cell game is up.
    pub comet: bool,
    /// Saving was switched off after the active game was deleted (so leaving
    /// the menu does not write it back); restored when a game starts.
    pub save_suspended: bool,
    pub status: String,
}

impl FrontEnd {
    /// Open an overlay (the game stays paused through it in a stage).
    pub fn open(&mut self, overlay: Overlay) {
        self.overlay = Some(overlay);
    }

    /// The planet-name field has the keyboard: OpenSpore's menu keys
    /// (`main.rs` Enter / L) must not react.
    pub fn typing(&self) -> bool {
        self.new_game.as_ref().is_some_and(|d| d.stage.is_some())
    }
}

/// Capture/debug hook: `--front-end <screen>` opens a front-end screen at
/// start (galaxy, create, newgame[:stage[@step]][-shop], settings[:tab], pedia,
/// delete, options, pause).
#[derive(Resource, Clone, Debug, Default)]
pub struct FrontEndDemo(pub Option<String>);

pub struct FrontEndPlugin;

impl Plugin for FrontEndPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let demo = args.iter().position(|a| a == "--front-end").and_then(|i| args.get(i + 1).cloned());
        // The settings beside an explicit save path are live before the first
        // frame (the effect library and the samplers read them at start).
        let mut user = settings::UserSettings::default();
        if args.iter().any(|a| a == "--save-path") {
            user.load_beside(&crate::game::RunProgress::save_path());
        }
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(
                bevy::render::Render,
                graphics::apply_texture_detail.in_set(bevy::render::RenderSystems::ExtractCommands),
            );
        }
        app.insert_resource(user)
            .init_resource::<FrontEnd>()
            .init_resource::<FrontEndActive>()
            .init_resource::<impostor::Impostors>()
            .init_resource::<thumbs::Thumbnails>()
            .insert_resource(intro::policy_from_args(&args))
            .init_resource::<settings::SettingsUi>()
            .init_resource::<profile::Profile>()
            .init_resource::<store::Store>()
            .init_resource::<pedia::PediaState>()
            .init_resource::<turntable::Turntable>()
            .init_resource::<turntable::View>()
            .init_resource::<launch_screen::LaunchScreen>()
            .init_resource::<new_game::Shoppers>()
            .insert_resource(FrontEndDemo(demo))
            .add_message::<MenuCommand>()
            .add_systems(
                Update,
                (
                    (
                        setup,
                        profile::track,
                        intro::intro,
                        intro::launch,
                        creators::sessions,
                        gge::manage,
                        gge::bind,
                        gge::actions,
                        new_game::bind,
                        new_game::actions,
                    )
                        .chain(),
                    (
                        settings::manage,
                        settings::bind,
                        settings::actions,
                        settings::apply,
                        settings::credits_roll,
                        pedia::manage,
                        pedia::bind,
                        pedia::actions,
                        thumbs::render,
                        turntable::update,
                        pedia::cards,
                        pause::commands,
                        pause::banner,
                        pause::layers,
                        pause::keys,
                    )
                        .chain(),
                )
                    .chain()
                    .after(view::input)
                    .before(view::sync),
            )
            .add_systems(Update, pedia::demo_pointer.before(view::input))
            .add_systems(
                Update,
                (graphics::apply_live, graphics::apply_dof, (launch_screen::manage, launch_screen::bind).chain().after(view::input).before(view::sync)),
            )
            // A Maxis creature opened before the starter's body existed is
            // fitted again to it (`maxis_creation`).
            .add_systems(Update, crate::maxis_creation::refit.before(crate::editor::refresh_parts))
            .add_systems(
                PostUpdate,
                intro::zoom_camera.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// First frame: remember the slot-0 path, load the user settings and the
/// Sporepedia store, size the window, apply a `--front-end` demo screen.
#[allow(clippy::too_many_arguments)]
fn setup(
    mut fe: ResMut<FrontEnd>,
    save: Res<crate::persistence::SaveState>,
    mut user: ResMut<settings::UserSettings>,
    mut pedia_store: ResMut<store::Store>,
    mut demo: ResMut<FrontEndDemo>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    stage: Res<State<crate::stages::GameStage>>,
    originals: Res<crate::Originals>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut frames: Local<u32>,
) {
    if fe.base.is_none() {
        fe.base = Some(save.path.clone());
        fe.rescan = true;
        user.load_beside(&save.path);
        *pedia_store = store::Store::load_beside(&save.path);
        // The saved screen mode (not for captures and smoke runs, which size
        // their own window).
        let scripted = std::env::args().any(|a| matches!(a.as_str(), "--quick-capture" | "--smoke-test" | "--render-smoke" | "--size"));
        if !scripted && (user.fullscreen || user.resolution.is_some()) {
            for mut window in &mut windows {
                settings::apply_window(&user, &mut window);
            }
        }
    }
    let Some(screen) = demo.0.take() else { return };
    let (name, arg) = screen.split_once(':').unwrap_or((screen.as_str(), ""));
    match name {
        // `create[:<creator>]`: the wheel (with that creator chosen);
        // `creator:<creator>` also launches it.
        "create" | "creator" => {
            fe.tab = Tab::Create;
            if let Some(i) = creators::by_name(arg) {
                fe.creator = Some(i);
                fe.hub = creators::CREATORS[i].hub;
                if name == "creator" {
                    // Launch once the galaxy screen is up.
                    if *frames < 30 {
                        *frames += 1;
                        demo.0 = Some(screen);
                    } else {
                        fe.launch_creator = Some(i);
                    }
                }
            }
        }
        // `launch:<slot>`: Play Game on a saved star (the zoom); `newcell`: a
        // new Cell game in the first free slot (zoom, then the comet).
        "launch" => intro::begin(&mut fe, intro::LaunchKind::Load(arg.parse().unwrap_or(0))),
        "newcell" => {
            let slot = fe.base.as_ref().map(|b| slots::starter_slots(&slots::scan(b)).first().copied().unwrap_or(0)).unwrap_or(0);
            let mut draft = new_game::Draft::new(slot);
            draft.stage = Some(0);
            draft.diet = Some(0);
            draft.planet = "Comet".into();
            intro::begin(&mut fe, intro::LaunchKind::New(draft));
        }
        "options" => fe.options_open = true,
        // `newgame[:<stage>[@<step>]][-taken]`: the new-game flow (`@<step>`: the
        // panel shown, an index into the stage's steps; `-taken`: named like
        // the first saved game, which shows "Planet Name Taken").
        "newgame" => {
            let mut draft = new_game::Draft::new(1);
            let (arg, shop) = arg.strip_suffix("-shop").map_or((arg, false), |a| (a, true));
            let (arg, taken) = arg.strip_suffix("-taken").map_or((arg, false), |a| (a, true));
            let (arg, step) = arg.split_once('@').map_or((arg, 0), |(a, n)| (a, n.parse().unwrap_or(0)));
            draft.step = step;
            if taken {
                if let Some(base) = fe.base.as_ref() {
                    draft.planet = slots::scan(base).iter().find_map(|s| s.saved().map(|s| s.planet.clone())).unwrap_or_default();
                }
            }
            draft.stage = match arg {
                "cell" => Some(0),
                "creature" => Some(1),
                "tribal" => Some(2),
                "civ" => Some(3),
                "space" => Some(4),
                _ => None,
            };
            // `-shop`: the shopper of the panel's first card is open (the creature card, or
            // the city hall card of the assets panel).
            if shop {
                let stage = draft.stage.unwrap_or(0);
                let card = if new_game::STAGE_MODES[stage.min(4)].creature && draft.step == 0 { new_game::Card::Creature } else { new_game::Card::Building };
                draft.shopping = Some(card);
                fe.flow_pick = Some(new_game::Shop::of(card, stage, draft.specialty));
                fe.open(Overlay::Sporepedia);
            }
            fe.new_game = Some(draft);
        }
        "settings" => {
            fe.open(Overlay::Settings);
            settings::demo_tab(arg);
        }
        // `pedia[:creatures|buildings|vehicles|spaceships][-detail][-maxis|-mine]`.
        "pedia" => {
            pedia::demo(arg);
            fe.open(Overlay::Sporepedia);
        }
        // `shopper:<tribal|civ|space>`: an outfitter's creature shopper;
        // `outfit:<...>`: that outfitter on the first creature of the player's
        // creations (or the starter), once the galaxy screen is up.
        "shopper" | "outfit" => {
            let kind = match arg {
                "civ" => crate::creature_editor_data::SetupKind::Civ,
                "space" => crate::creature_editor_data::SetupKind::Space,
                _ => crate::creature_editor_data::SetupKind::Tribal,
            };
            if *frames < 30 || originals.resource_index.records.is_empty() {
                *frames += 1;
                demo.0 = Some(screen);
            } else if name == "shopper" {
                fe.shopper = Some(kind);
                fe.open(Overlay::Sporepedia);
            } else {
                let mut scratch = crate::game::RunProgress::default();
                if let Some(c) = pedia_store.creations.iter().find(|c| c.is_creature()) {
                    scratch = c.to_run();
                }
                let scratch = creators::outfit_scratch(scratch, &originals.part_catalog);
                fe.menu_launch = Some(creators::MenuLaunch { kind, scratch: Box::new(scratch), replace: None });
            }
        }
        // `maxis:<creatures|buildings|vehicles|spaceships>[:n]`: Sporepedia's
        // Edit on the n-th installed Maxis creation of that type
        // (`maxis_creation`), once the galaxy screen is up.
        "maxis" => {
            if *frames < 30 || originals.resource_index.records.is_empty() {
                *frames += 1;
                demo.0 = Some(screen);
            } else {
                fe.menu_launch = crate::maxis_creation::demo_launch(&originals, arg);
            }
        }
        "delete" => fe.open(Overlay::ConfirmDelete(0)),
        "select" => fe.selected = arg.parse().ok().or(Some(0)),
        // Pause once the stage has run for a second (its scene is built).
        "pause" => {
            if *stage.get() != crate::stages::GameStage::MainMenu {
                *frames += 1;
            }
            if *frames >= 60 {
                pause.open = true;
            } else {
                demo.0 = Some(screen);
            }
        }
        _ => {}
    }
}

/// Localized string of a locale table, else the fallback.
pub(crate) fn loc(assets: &mut view::SpuiAssets, index: &crate::asset_catalog::ResourceIndex, table: u32, id: u32, fallback: &str) -> String {
    assets.localized(index, table, id).unwrap_or_else(|| fallback.to_string())
}

/// Locale table of the GGE strings (`2B7A0311`).
pub const GGE_TABLE: u32 = 0x2B7A_0311;
/// Stage names "Cell Stage" .. "Space Stage" in [`GGE_TABLE`].
pub const STAGE_NAME_IDS: [u32; 5] = [0x05B0_8702, 0x05B0_8703, 0x05B0_8704, 0x05B0_8705, 0x05B0_8706];
pub const STAGE_NAMES: [&str; 5] = ["Cell Stage", "Creature Stage", "Tribal Stage", "Civilization Stage", "Space Stage"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_controls_map_to_one_command_set() {
        assert_eq!(menu_command(id::MENU_RESUME), Some(MenuCommand::Resume));
        assert_eq!(menu_command(id::MENU_EXIT), Some(MenuCommand::ExitToMenu));
        assert_eq!(menu_command(id::MENU_SETTINGS), Some(MenuCommand::Settings));
        assert_eq!(menu_command(0x0269_9440), Some(MenuCommand::Sporepedia));
        assert_eq!(menu_command(crate::cell_hud::id::OPTIONS_ORB), None);
        assert_eq!(menu_command(crate::cell_hud::id::PAUSE_TOGGLE), None);
    }

    #[test]
    fn old_saves_keep_their_specialty_numbering() {
        // Before this flag OpenSpore stored 1 economic / 2 religious; retail stores 1 religious / 2 economic.
        let old: GameSetup = serde_json::from_str(r#"{"specialty":1}"#).unwrap();
        assert!(!old.specialty_retail);
        assert_eq!(old.retail_specialty(), Some(2), "old 1 = economic = retail 2");
        let new = GameSetup { specialty: Some(2), ..GameSetup::default() };
        assert!(new.specialty_retail);
        assert_eq!(new.retail_specialty(), Some(2));
        let again: GameSetup = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
        assert_eq!(again, new);
    }

    #[test]
    fn old_saves_default_to_normal_difficulty() {
        let setup: GameSetup = serde_json::from_str("{}").unwrap();
        assert_eq!(setup.difficulty, 1);
    }
}
