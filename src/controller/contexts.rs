//! Which screen is showing, and what each pad control does there.
//!
//! Every binding is a key or mouse button the game already reads for that
//! screen (see the stage control modules), so the pad plays exactly like
//! the keyboard and mouse:
//!
//! - Menus, the galaxy screen, dialogs and editors: the left stick moves the
//!   cursor, A clicks (hold A to drag), B goes back.
//! - Cell, Creature and Space (planet view): the left stick moves (the WASD
//!   keys), the right stick turns and zooms the camera (`,` `.` `=` `-`).
//!   Holding LT turns the left stick into the cursor for the HUD.
//! - Tribal and Civilization: the left stick moves the cursor, A selects,
//!   X commands (right click), the right stick pans.
//! - Everywhere: Menu pauses, holding View lists every control.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::prompts::Anchor;
use super::{Binding, Out, Pad, PadState, StickUse};
use crate::stages::GameStage;

/// The screens with their own controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ContextKind {
    /// Nothing to drive (no stage state, e.g. tests).
    #[default]
    None,
    Menu,
    Cinematic,
    /// A name field has the keyboard.
    Typing,
    Cell,
    Creature,
    /// Creature with LT held: the left stick is the cursor.
    CreatureCursor,
    Tribal,
    Civilization,
    SpacePlanet,
    SpacePlanetCursor,
    SpaceSystem,
    SpaceGalaxy,
    /// The creature editor and everything built on it (outfitters, the
    /// building / vehicle / UFO editors, the front-end creators).
    CreatureEditor,
    CellEditor,
    /// The older creator panel (`editor.rs`).
    Creator,
    MechanicalCreator,
}

/// The current screen's controls.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ControllerContext {
    pub kind: ContextKind,
    pub bindings: Vec<Binding>,
    pub left: StickUse,
    pub right: StickUse,
    pub left_label: Option<&'static str>,
    pub right_label: Option<&'static str>,
    pub anchor: Anchor,
}

/// Tests pin the context with this.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ContextOverride(pub ContextKind);

use KeyCode as K;
use MouseButton as M;

const WASD: StickUse = StickUse::Keys { up: K::KeyW, down: K::KeyS, left: K::KeyA, right: K::KeyD };
const ARROWS: StickUse = StickUse::Keys { up: K::ArrowUp, down: K::ArrowDown, left: K::ArrowLeft, right: K::ArrowRight };
/// The camera keys shared by the stages and editors: `,` / `.` turn,
/// `=` / `-` zoom.
const CAMERA: StickUse = StickUse::Keys { up: K::Equal, down: K::Minus, left: K::Comma, right: K::Period };

fn all_controls() -> Binding {
    Binding::new(Pad::View, Out::Direct, "All controls")
}

impl ControllerContext {
    /// The controls of `kind`.
    pub fn for_kind(kind: ContextKind) -> ControllerContext {
        use Binding as B;
        let mut c = ControllerContext { kind, ..default() };
        match kind {
            ContextKind::None => {}
            ContextKind::Menu => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::B, Out::Key(K::Escape), "Back"),
                    B::hidden(Pad::X, Out::Mouse(M::Right)),
                    B::hidden(Pad::Menu, Out::Key(K::Escape)),
                    B::hidden(Pad::LT, Out::Wheel(-12.0)),
                    B::hidden(Pad::RT, Out::Wheel(12.0)),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = StickUse::Wheel { rate: 14.0 };
                c.right_label = Some("Scroll");
            }
            ContextKind::Cinematic => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Key(K::Enter), "Continue"),
                    B::new(Pad::B, Out::Key(K::Escape), "Skip"),
                    B::hidden(Pad::Menu, Out::Key(K::Escape)),
                    B::hidden(Pad::X, Out::Mouse(M::Left)),
                ];
                c.left = StickUse::Cursor;
            }
            ContextKind::Typing => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Key(K::Enter), "Done"),
                    B::new(Pad::B, Out::Key(K::Escape), "Cancel"),
                    B::hidden(Pad::X, Out::Mouse(M::Left)),
                ];
                c.left = StickUse::Cursor;
            }
            ContextKind::Cell => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::B, Out::Key(K::Enter), "OK"),
                    B::new(Pad::Y, Out::Key(K::F2), "Mate"),
                    B::extra(Pad::X, Out::Key(K::Space), "Skip"),
                    B::new(Pad::LT, Out::Key(K::Minus), "Zoom"),
                    B::new(Pad::RT, Out::Key(K::Equal), "Zoom"),
                    B::extra(Pad::Up, Out::Key(K::F4), "Evolve"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    all_controls(),
                ];
                c.left = WASD;
                c.left_label = Some("Swim");
                c.right = StickUse::Cursor;
                c.right_label = Some("Point");
            }
            ContextKind::Creature | ContextKind::CreatureCursor => {
                let pointing = kind == ContextKind::CreatureCursor;
                c.bindings = vec![
                    if pointing {
                        B::new(Pad::A, Out::Mouse(M::Left), "Click")
                    } else {
                        B::new(Pad::A, Out::Key(K::Space), "Jump")
                    },
                    B::new(Pad::X, Out::Key(K::Digit1), "Abilities"),
                    B::new(Pad::Y, Out::Key(K::Digit2), "Abilities"),
                    if pointing {
                        B::new(Pad::B, Out::Mouse(M::Right), "Act")
                    } else {
                        B::new(Pad::B, Out::Key(K::Digit3), "Abilities")
                    },
                    B::new(Pad::RB, Out::Key(K::Digit4), "Abilities"),
                    B::new(Pad::LB, Out::Key(K::KeyF), "Target"),
                    B::new(Pad::LT, Out::Direct, "Cursor"),
                    B::extra(Pad::RT, Out::Key(K::KeyM), "Call"),
                    B::new(Pad::LS, Out::Key(K::ShiftLeft), "Sprint"),
                    B::extra(Pad::RS, Out::Key(K::Slash), "Reset camera"),
                    B::extra(Pad::Up, Out::Key(K::Tab), "Stance"),
                    B::extra(Pad::Down, Out::Key(K::KeyZ), "Sneak"),
                    B::extra(Pad::Left, Out::Key(K::KeyL), "Missions"),
                    B::extra(Pad::Right, Out::Key(K::KeyR), "Autorun"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    all_controls(),
                ];
                if pointing {
                    c.left = StickUse::Cursor;
                    c.left_label = Some("Point");
                } else {
                    c.left = WASD;
                    c.left_label = Some("Move");
                }
                c.right = CAMERA;
                c.right_label = Some("Camera");
            }
            ContextKind::Tribal => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::X, Out::Mouse(M::Right), "Command"),
                    B::new(Pad::B, Out::Key(K::Escape), "Deselect"),
                    B::new(Pad::Y, Out::Key(crate::tribal_controls::KEY_SELECT_TRIBE), "Whole tribe"),
                    B::new(Pad::LB, Out::Key(K::Comma), "Rotate"),
                    B::new(Pad::RB, Out::Key(K::Period), "Rotate"),
                    B::new(Pad::LT, Out::Key(K::Minus), "Zoom"),
                    B::new(Pad::RT, Out::Key(K::Equal), "Zoom"),
                    B::extra(Pad::Up, Out::Key(K::Space), "Follow next"),
                    B::extra(Pad::Left, Out::Key(crate::tribal_controls::KEY_STANCE_SOCIAL), "Social stance"),
                    B::extra(Pad::Right, Out::Key(crate::tribal_controls::KEY_STANCE_ATTACK), "Attack stance"),
                    B::extra(Pad::Down, Out::Key(crate::tribal_controls::KEY_PERFORM), "Perform"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = ARROWS;
                c.right_label = Some("Pan");
            }
            ContextKind::Civilization => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::X, Out::Mouse(M::Right), "Order"),
                    B::new(Pad::B, Out::Key(K::Escape), "Cancel"),
                    B::new(Pad::Y, Out::Key(K::Space), "Next unit"),
                    B::new(Pad::LB, Out::Key(K::Comma), "Rotate"),
                    B::new(Pad::RB, Out::Key(K::Period), "Rotate"),
                    B::new(Pad::LT, Out::Key(K::Minus), "Zoom"),
                    B::new(Pad::RT, Out::Key(K::Equal), "Zoom"),
                    B::extra(Pad::Up, Out::Key(K::KeyF), "Send fleet"),
                    B::extra(Pad::Right, Out::Key(K::KeyQ), "Select fleet"),
                    B::extra(Pad::Left, Out::Key(K::Tab), "Stance"),
                    B::extra(Pad::Down, Out::Key(K::Backspace), "Home city"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = ARROWS;
                c.right_label = Some("Pan");
            }
            ContextKind::SpacePlanet | ContextKind::SpacePlanetCursor => {
                let pointing = kind == ContextKind::SpacePlanetCursor;
                c.bindings = vec![
                    if pointing {
                        B::new(Pad::A, Out::Mouse(M::Left), "Click")
                    } else {
                        B::new(Pad::A, Out::Key(K::KeyF), "Fire")
                    },
                    if pointing {
                        B::new(Pad::B, Out::Mouse(M::Right), "Fly to")
                    } else {
                        B::new(Pad::B, Out::Key(K::KeyU), "Beam")
                    },
                    B::new(Pad::X, Out::Key(K::KeyY), "Use tool"),
                    B::new(Pad::Y, Out::Key(K::KeyT), "Next tool"),
                    B::new(Pad::LT, Out::Direct, "Cursor"),
                    B::extra(Pad::LB, Out::Key(K::KeyV), "Scan"),
                    B::extra(Pad::RB, Out::Key(K::KeyR), "Power"),
                    B::extra(Pad::RT, Out::Key(K::Space), "Zoom out"),
                    B::extra(Pad::Up, Out::Key(K::KeyK), "Comms"),
                    B::extra(Pad::Down, Out::Key(K::KeyM), "Missions"),
                    B::extra(Pad::Left, Out::Key(K::KeyB), "Spice"),
                    B::extra(Pad::Right, Out::Key(K::KeyC), "Colony"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    all_controls(),
                ];
                if pointing {
                    c.left = StickUse::Cursor;
                    c.left_label = Some("Point");
                } else {
                    c.left = WASD;
                    c.left_label = Some("Fly");
                }
                c.right = CAMERA;
                c.right_label = Some("Camera");
            }
            ContextKind::SpaceSystem => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::X, Out::Mouse(M::Right), "Fly to"),
                    B::extra(Pad::Up, Out::Key(K::KeyK), "Comms"),
                    B::extra(Pad::Down, Out::Key(K::KeyM), "Missions"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    B::hidden(Pad::B, Out::Key(K::Escape)),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = CAMERA;
                c.right_label = Some("Camera");
            }
            ContextKind::SpaceGalaxy => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::X, Out::Key(K::Enter), "Jump"),
                    B::extra(Pad::Up, Out::KeyWith(K::ControlLeft, K::KeyW), "Next star"),
                    B::extra(Pad::Down, Out::KeyWith(K::ControlLeft, K::KeyS), "Next star"),
                    B::extra(Pad::Left, Out::KeyWith(K::ControlLeft, K::KeyA), "Next star"),
                    B::extra(Pad::Right, Out::KeyWith(K::ControlLeft, K::KeyD), "Next star"),
                    B::extra(Pad::Y, Out::Key(K::Delete), "Clear"),
                    B::new(Pad::Menu, Out::Key(K::Escape), "Pause"),
                    B::hidden(Pad::B, Out::Key(K::Escape)),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = CAMERA;
                c.right_label = Some("Camera");
            }
            ContextKind::CreatureEditor => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Pick / drag"),
                    B::new(Pad::X, Out::Key(K::Delete), "Remove"),
                    B::new(Pad::B, Out::Key(K::Escape), "Options"),
                    B::new(Pad::LB, Out::KeyWith(K::ControlLeft, K::KeyZ), "Undo"),
                    B::new(Pad::RB, Out::KeyWith(K::ControlLeft, K::KeyY), "Redo"),
                    B::new(Pad::LT, Out::Wheel(-10.0), "Size"),
                    B::new(Pad::RT, Out::Wheel(10.0), "Size"),
                    B::extra(Pad::Up, Out::Key(K::ArrowUp), "Bigger"),
                    B::extra(Pad::Down, Out::Key(K::ArrowDown), "Smaller"),
                    B::extra(Pad::Y, Out::Mouse(M::Right), "Right click"),
                    B::hidden(Pad::Menu, Out::Key(K::Escape)),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = CAMERA;
                c.right_label = Some("Camera");
            }
            ContextKind::CellEditor => {
                // Escape would cancel and discard the changes, so it is not
                // on a button: Cancel is the on-screen button.
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Pick / drag"),
                    B::new(Pad::X, Out::Key(K::Delete), "Sell part"),
                    B::new(Pad::LB, Out::KeyWith(K::ControlLeft, K::KeyZ), "Undo"),
                    B::new(Pad::RB, Out::KeyWith(K::ControlLeft, K::KeyY), "Redo"),
                    B::new(Pad::LT, Out::Wheel(-10.0), "Size"),
                    B::new(Pad::RT, Out::Wheel(10.0), "Size"),
                    B::extra(Pad::Up, Out::Key(K::ArrowUp), "Bigger"),
                    B::extra(Pad::Down, Out::Key(K::ArrowDown), "Smaller"),
                    B::new(Pad::Menu, Out::Key(K::F2), "Done"),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
            }
            ContextKind::Creator => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Pick / drag"),
                    B::new(Pad::X, Out::Key(K::Backspace), "Remove"),
                    B::new(Pad::B, Out::Key(K::Escape), "Close"),
                    B::new(Pad::LB, Out::KeyWith(K::ControlLeft, K::KeyZ), "Undo"),
                    B::new(Pad::RB, Out::KeyWith(K::ControlLeft, K::KeyY), "Redo"),
                    B::new(Pad::LT, Out::Wheel(-10.0), "Zoom"),
                    B::new(Pad::RT, Out::Wheel(10.0), "Zoom"),
                    B::extra(Pad::Y, Out::Key(K::KeyM), "Symmetry"),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
                c.right = StickUse::Drag { button: Some(M::Right), speed: 700.0 };
                c.right_label = Some("Camera");
            }
            ContextKind::MechanicalCreator => {
                c.bindings = vec![
                    B::new(Pad::A, Out::Mouse(M::Left), "Select"),
                    B::new(Pad::B, Out::Key(K::Escape), "Close"),
                    B::new(Pad::LT, Out::Wheel(-10.0), "Zoom"),
                    B::new(Pad::RT, Out::Wheel(10.0), "Zoom"),
                    all_controls(),
                ];
                c.left = StickUse::Cursor;
                c.left_label = Some("Point");
            }
        }
        // Clear of each screen's HUD: the creature and space screens keep
        // their top-right corner free; the tribe, civilization and editor
        // screens have a strip along the bottom edge.
        c.anchor = match kind {
            ContextKind::Creature
            | ContextKind::CreatureCursor
            | ContextKind::SpacePlanet
            | ContextKind::SpacePlanetCursor
            | ContextKind::SpaceSystem
            | ContextKind::SpaceGalaxy => Anchor::TopRight,
            ContextKind::Tribal
            | ContextKind::Civilization
            | ContextKind::Typing
            | ContextKind::CreatureEditor
            | ContextKind::CellEditor
            | ContextKind::Creator
            | ContextKind::MechanicalCreator => Anchor::AboveHud,
            ContextKind::None | ContextKind::Menu | ContextKind::Cinematic | ContextKind::Cell => Anchor::BottomRight,
        };
        c
    }
}

/// What decides the screen.
#[derive(SystemParam)]
pub struct Screen<'w> {
    stage: Option<Res<'w, State<GameStage>>>,
    cinematics: Option<Res<'w, crate::cinematics::Cinematics>>,
    pause: Option<Res<'w, crate::pause_menu::PauseMenu>>,
    flow: Option<Res<'w, crate::creature_flow::CreatureFlow>>,
    civ_comm: Option<Res<'w, crate::civ_comm::CivComm>>,
    launch: Option<Res<'w, crate::front_end::launch_screen::LaunchScreen>>,
    front_end: Option<Res<'w, crate::front_end::FrontEnd>>,
    creature_editor: Option<Res<'w, crate::creature_editor::CreatureEditor>>,
    cell_editor: Option<Res<'w, crate::cell_editor::CellEditor>>,
    creator: Option<Res<'w, crate::editor::EditorState>>,
    mechanical: Option<Res<'w, crate::mechanical_creator::MechanicalCreator>>,
    space_view: Option<Res<'w, crate::space_view::SpaceView>>,
    cheats: Option<Res<'w, crate::cheats::CheatState>>,
    pinned: Option<Res<'w, ContextOverride>>,
}

impl Screen<'_> {
    fn kind(&self, pad: &PadState) -> ContextKind {
        if let Some(pinned) = &self.pinned {
            return pinned.0;
        }
        let Some(stage) = &self.stage else {
            return ContextKind::None;
        };
        // The cheat console takes typing; the debug menu is a menu.
        if let Some(cheats) = &self.cheats {
            if cheats.console_open {
                return ContextKind::Typing;
            }
            if cheats.menu_open {
                return ContextKind::Menu;
            }
        }
        if self.cinematics.as_ref().is_some_and(|c| c.is_playing()) {
            return ContextKind::Cinematic;
        }
        let creature_editor = self.creature_editor.as_ref().filter(|e| e.active);
        let cell_editor = self.cell_editor.as_ref().filter(|e| e.active);
        if self.front_end.as_ref().is_some_and(|f| f.typing())
            || creature_editor.is_some_and(|e| e.naming)
            || cell_editor.is_some_and(|e| e.naming)
        {
            return ContextKind::Typing;
        }
        if self.mechanical.as_ref().is_some_and(|m| m.open) {
            return ContextKind::MechanicalCreator;
        }
        if cell_editor.is_some() {
            return ContextKind::CellEditor;
        }
        if creature_editor.is_some() {
            return ContextKind::CreatureEditor;
        }
        if self.creator.as_ref().is_some_and(|e| e.open) {
            return ContextKind::Creator;
        }
        let stage = stage.get();
        let paused = self.pause.as_ref().is_some_and(|p| p.open)
            || self.flow.as_ref().is_some_and(|f| f.holding())
            || self.civ_comm.as_ref().is_some_and(|c| c.open)
            || self.launch.as_ref().is_some_and(|l| l.holds_game())
            || self.front_end.as_ref().is_some_and(|f| f.overlay.is_some());
        if *stage == GameStage::MainMenu || paused {
            return ContextKind::Menu;
        }
        let pointing = pad.pressed(Pad::LT);
        match stage {
            GameStage::MainMenu => ContextKind::Menu,
            GameStage::Cell => ContextKind::Cell,
            GameStage::Creature if pointing => ContextKind::CreatureCursor,
            GameStage::Creature => ContextKind::Creature,
            GameStage::Tribal => ContextKind::Tribal,
            GameStage::Civilization => ContextKind::Civilization,
            GameStage::Space => match self.space_view.as_ref().map(|v| v.zoom()) {
                Some(crate::space_view::SpaceZoom::System) => ContextKind::SpaceSystem,
                Some(crate::space_view::SpaceZoom::Galaxy) => ContextKind::SpaceGalaxy,
                _ if pointing => ContextKind::SpacePlanetCursor,
                _ => ContextKind::SpacePlanet,
            },
        }
    }
}

/// Picks the screen's controls (rebuilt only when the screen changes).
pub fn update(mut context: ResMut<ControllerContext>, pad: Res<PadState>, screen: Screen) {
    let kind = screen.kind(&pad);
    if context.kind != kind || (kind == ContextKind::None && !context.bindings.is_empty()) {
        *context = ControllerContext::for_kind(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const ALL: [ContextKind; 17] = [
        ContextKind::None,
        ContextKind::Menu,
        ContextKind::Cinematic,
        ContextKind::Typing,
        ContextKind::Cell,
        ContextKind::Creature,
        ContextKind::CreatureCursor,
        ContextKind::Tribal,
        ContextKind::Civilization,
        ContextKind::SpacePlanet,
        ContextKind::SpacePlanetCursor,
        ContextKind::SpaceSystem,
        ContextKind::SpaceGalaxy,
        ContextKind::CreatureEditor,
        ContextKind::CellEditor,
        ContextKind::Creator,
        ContextKind::MechanicalCreator,
    ];

    #[test]
    fn each_control_has_one_job_per_screen() {
        for kind in ALL {
            let c = ControllerContext::for_kind(kind);
            let mut seen = HashSet::new();
            for b in &c.bindings {
                assert!(seen.insert(b.pad), "{kind:?}: {:?} bound twice", b.pad);
            }
        }
    }

    #[test]
    fn every_screen_can_reach_its_buttons() {
        // Every screen with controls has a cursor (always, or with LT) and
        // a left click, so every on-screen button can be pressed.
        for kind in ALL {
            if kind == ContextKind::None {
                continue;
            }
            let pointing = match kind {
                ContextKind::Creature => ContextKind::CreatureCursor,
                ContextKind::SpacePlanet => ContextKind::SpacePlanetCursor,
                other => other,
            };
            let c = ControllerContext::for_kind(pointing);
            assert!(
                c.left == StickUse::Cursor || c.right == StickUse::Cursor,
                "{kind:?} has no cursor"
            );
            let clicks = c
                .bindings
                .iter()
                .any(|b| b.out == Out::Mouse(MouseButton::Left) || (kind == ContextKind::Cinematic || kind == ContextKind::Typing) && b.pad == Pad::X);
            assert!(clicks, "{kind:?} cannot click");
        }
    }

    #[test]
    fn the_cell_editor_never_discards_on_a_button() {
        let c = ControllerContext::for_kind(ContextKind::CellEditor);
        assert!(c.bindings.iter().all(|b| b.out != Out::Key(KeyCode::Escape)));
    }

    #[test]
    fn the_main_menu_never_presses_enter() {
        // Enter on the main menu starts a new game (main.rs menu_keys).
        let c = ControllerContext::for_kind(ContextKind::Menu);
        assert!(c.bindings.iter().all(|b| b.out != Out::Key(KeyCode::Enter)));
    }
}
