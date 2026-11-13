//! The in-stage pause: the `GlobalUIPause` frame (screen border and pause
//! icon) while a stage is paused, and one handler for the options-menu
//! commands every stage HUD forwards ([`MenuCommand`]), so Resume / Save /
//! Exit to Main Menu / Quit / Settings / Sporepedia behave the same in all
//! stages. Escape closes front-end overlays first.
//!
//! VERIFIED (march2017 image, `swarm-volume/target/re/dis.py`): the game waits
//! while Sporepedia is open and goes on when it closes. The asset browser's
//! show / hide routine (0x64A7B5) posts message 0x539A4CE (shown) or 0x539A4CF
//! (hidden) with its "opened by a request" flag; the galaxy / game-entry UI
//! handler (0xE04095) answers 0x539A4CE with `cGameTimeManager::Pause(0x4BF38A6)`
//! (0xE040E0) and 0x539A4CF with `Resume(0x4BF38A6)` (0xE040BB) when the flag
//! is 0. The pause has its own reason: it is not the options menu's, so
//! closing Sporepedia resumes the stage without the pause frame
//! ([`PediaHold`]). The HUD's Sporepedia button only posts 0x5120264 (0xE53890);
//! the GGE handler (0xDE8CBD) opens the browser through a default
//! `SporepediaRequest` (0xE02C10 -> 0xE02010) outside the galaxy mode.

use bevy::prelude::*;

use super::{FrontEnd, MenuCommand, Overlay};
use crate::{
    game::RunProgress,
    spui::{
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    stages::GameStage,
};

pub const LAYOUT: &str = "GlobalUIPause";

/// The pause frame root.
#[derive(Component)]
pub struct PauseFrame;

/// The pause Sporepedia took when it was opened in a stage (the retail pause
/// reason 0x4BF38A6): `held` = the options menu was not open then, so closing it
/// resumes the stage; otherwise the options menu is still up behind it.
#[derive(Default)]
pub struct PediaHold {
    open: bool,
    held: bool,
}

/// Apply the forwarded menu commands.
#[allow(clippy::too_many_arguments)]
pub fn commands(
    mut menu: MessageReader<MenuCommand>,
    mut fe: ResMut<FrontEnd>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    mut run: ResMut<RunProgress>,
    mut next: ResMut<NextState<GameStage>>,
    mut exit: MessageWriter<AppExit>,
    stage: Res<State<GameStage>>,
    mut hold: Local<PediaHold>,
) {
    // Sporepedia closed (its own button or Escape): its pause reason ends
    // (0x539A4CF).
    if hold.open && fe.overlay != Some(Overlay::Sporepedia) {
        hold.open = false;
        pause.open = !hold.held;
    }
    for command in menu.read() {
        match command {
            MenuCommand::Resume => pause.open = false,
            MenuCommand::Save => run.request_save(),
            MenuCommand::ExitToMenu => {
                run.request_save();
                pause.open = false;
                fe.overlay = None;
                next.set(GameStage::MainMenu);
            }
            MenuCommand::Quit => {
                exit.write(AppExit::Success);
            }
            MenuCommand::Settings => fe.open(Overlay::Settings),
            MenuCommand::Sporepedia => {
                // The game waits while Sporepedia is open (0xE040E0, see the
                // module notes); `pause.open` is OpenSpore's simulation hold.
                if *stage.get() != GameStage::MainMenu {
                    *hold = PediaHold { open: true, held: !pause.open };
                    pause.open = true;
                }
                fe.open(Overlay::Sporepedia);
            }
            // The Spore Guide is online help (retail opens its web page): there
            // is nothing to open offline. ADAPTED by design (online feature).
            MenuCommand::SporeGuide => {}
        }
    }
}

/// Show `GlobalUIPause` while a stage is paused through its menu.
#[allow(clippy::too_many_arguments)]
pub fn banner(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    pause: Res<crate::pause_menu::PauseMenu>,
    editor: Res<crate::editor::EditorState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    mut frames: Query<(Entity, &mut SpuiState), With<PauseFrame>>,
) {
    let in_stage = *stage.get() != GameStage::MainMenu && !editor.open;
    if !in_stage {
        for (e, _) in &frames {
            commands.entity(e).despawn();
        }
        return;
    }
    if frames.is_empty() {
        if originals.resource_index.records.is_empty() {
            return;
        }
        let (Ok(camera), Some(layout)) = (camera.single(), assets.layout(&originals.resource_index, LAYOUT)) else {
            return;
        };
        // Under the HUD layouts (100+) so the options menu stays on top.
        let root = view::spawn(&mut commands, layout, RootPolicy::Fit, Some(camera), 96);
        commands.entity(root).insert(PauseFrame);
        return;
    }
    for (_, mut ui) in &mut frames {
        if !ui.ready {
            continue;
        }
        for root in ui.layout.roots.clone() {
            if ui.visible(root) != pause.open {
                ui.set_visible(root, pause.open);
            }
        }
    }
}

/// The in-stage UI layers while the options menu is open (VERIFIED, march2017
/// image). The retail UI has named layers (`UIWorld_*`, table 0x1544CC0: id,
/// priority, name) and every layout is loaded into one: the stage HUDs into
/// their own layers (`UIWorld_GlobalUI` ... priority 100), the orb and options
/// menu into `UIWorld_GlobalUIOptions` (0x5F1BFC10, 100), while dialogs (the
/// callout dialogs `Callout_*`, message boxes, GameSettings) go into
/// `UIWorld_MessageBoxes` (0x5B598F7, priority 1e8: `UILayout::Load(key, flag,
/// 0x5B598F7)`, 0x817309 / 0x60335D). The layer set the UI shows is chosen by
/// 0xB5EC30 (one flag per layer, applied with `SetLayerVisible` 0x810700):
/// with the options menu open (0xE00A90: its panel 0x0615B50D is visible) only
/// the tooltip layer (0x5B598F6) and the options layer are shown, so the stage
/// HUD is hidden behind the menu; when a dialog stands over the menu (0xE00AD0:
/// the window manager's modal window is a callout / message box root 0x06318E78,
/// or the GameSettings window 0x043C8B98 is visible) only the tooltip layer is
/// shown, which hides the options menu itself: a callout never overlaps the
/// menu's rows. OpenSpore's roots carry the layout's `GlobalZIndex`: the stage
/// HUD layouts sit at 85..=150 (the Tribal tool / tribe / citizen / posse
/// layouts 90..95, rollovers 98, mission card 99, HUD 100, orb 101 ...), so
/// this hides them while the menu is open (the pause frame, the dialogs among
/// them and the front-end screens stay). The HUDs' non-SPUI picture nodes (the
/// creature and tribal portraits, the civilization minimap, the space minimap
/// dots) follow their layout root's visibility, so they go with the layer.
pub fn layers(
    pause: Res<crate::pause_menu::PauseMenu>,
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    fe: Res<FrontEnd>,
    mut roots: Query<(Entity, &SpuiState, &GlobalZIndex, &mut Visibility), Without<PauseFrame>>,
    mut hidden: Local<Vec<Entity>>,
) {
    const OPTIONS_PANEL: u32 = 0x0615_B50D;
    const DIALOG_ROOT: u32 = crate::cell_hud::dialogs::callout::ROOT;
    const HUD_Z: std::ops::RangeInclusive<i32> = 85..=150;
    let menu = pause.open && *stage.get() != GameStage::MainMenu && !editor.open && fe.overlay != Some(Overlay::Sporepedia);
    let dialog = |ui: &SpuiState| ui.find(DIALOG_ROOT).is_some_and(|w| ui.visible(w));
    let dialog_up = menu && (fe.overlay == Some(Overlay::Settings) || roots.iter().any(|(_, ui, _, _)| ui.ready && dialog(ui)));
    for (entity, ui, z, mut visibility) in &mut roots {
        if !ui.ready || !HUD_Z.contains(&z.0) || dialog(ui) {
            continue;
        }
        let options = ui.find(OPTIONS_PANEL).is_some();
        let hide = menu && if options { dialog_up } else { true };
        if hide {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            if !hidden.contains(&entity) {
                hidden.push(entity);
            }
        } else if hidden.contains(&entity) {
            *visibility = Visibility::Inherited;
            hidden.retain(|e| *e != entity);
        }
    }
    hidden.retain(|e| roots.contains(*e));
}

/// Escape: close the open front-end screen (overlay, galaxy options, new game,
/// star selection) before anything else reacts. In a stage, closing an
/// overlay returns to the options menu (the game stays paused).
pub fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut fe: ResMut<FrontEnd>,
    mut pause: ResMut<crate::pause_menu::PauseMenu>,
    stage: Res<State<GameStage>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if fe.overlay.is_some() {
        let sporepedia = fe.overlay == Some(Overlay::Sporepedia);
        fe.overlay = None;
        // `pause_menu::input` toggled the pause on the same key: the options
        // menu stays up behind Settings, while closing Sporepedia resumes the
        // stage (its pause reason ends, 0x539A4CF).
        if *stage.get() != GameStage::MainMenu && !sporepedia {
            pause.open = true;
        }
        return;
    }
    if *stage.get() != GameStage::MainMenu {
        return;
    }
    if fe.options_open {
        fe.options_open = false;
    } else if let Some(draft) = fe.new_game.as_mut() {
        if draft.stage.is_some() {
            draft.stage = None;
        } else {
            fe.new_game = None;
        }
    } else if fe.selected.is_some() {
        fe.selected = None;
    } else if fe.tab != super::Tab::Play {
        fe.tab = super::Tab::Play;
    }
}
