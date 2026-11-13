//! In-game pause menu. Gameplay uses the same pause gate as the creator.
use crate::{editor::EditorState, game::RunProgress, stages::GameStage};
use bevy::{prelude::*, ui::Pressed, ui_widgets::Button};

#[derive(Resource, Default)]
pub struct PauseMenu {
    pub open: bool,
    note: String,
}
#[derive(Component)]
pub struct PauseRoot;
#[derive(Component, Clone, Copy)]
pub enum PauseAction {
    Resume,
    Save,
    MainMenu,
}

pub fn simulation_active(
    pause: Res<PauseMenu>,
    flow: Option<Res<crate::creature_flow::CreatureFlow>>,
    civ_comm: Option<Res<crate::civ_comm::CivComm>>,
    launch: Option<Res<crate::front_end::launch_screen::LaunchScreen>>,
) -> bool {
    // The Creature moments (brain upgrade, first-time hints, the end of the
    // stage) freeze the game like the retail `pauseGame` command; so does the
    // Civ comm screen (ShowCommEvent 0xAEBE10 calls cGameTimeManager::Pause)
    // and the stage launch screen (0xE3E350 pauses Gameplay).
    !pause.open && !flow.is_some_and(|f| f.holding()) && !civ_comm.is_some_and(|c| c.open) && !launch.is_some_and(|l| l.holds_game())
}

pub fn input(
    keys: Res<ButtonInput<KeyCode>>,
    editor: Res<EditorState>,
    mechanical: Option<Res<crate::mechanical_creator::MechanicalCreator>>,
    state: Res<State<GameStage>>,
    mut pause: ResMut<PauseMenu>,
    actions: Query<&PauseAction, Added<Pressed>>,
    mut run: ResMut<RunProgress>,
    mut next: ResMut<NextState<GameStage>>,
) {
    if *state.get() == GameStage::MainMenu || editor.open || mechanical.is_some_and(|m| m.open) {
        pause.open = false;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        pause.open = !pause.open;
        pause.note.clear();
    }
    for action in &actions {
        match action {
            PauseAction::Resume => pause.open = false,
            PauseAction::Save => {
                run.request_save();
                pause.note = "Save requested".into();
            }
            PauseAction::MainMenu => {
                run.request_save();
                pause.open = false;
                next.set(GameStage::MainMenu);
            }
        }
    }
}

pub fn ui(
    mut commands: Commands,
    pause: Res<PauseMenu>,
    existing: Query<Entity, With<PauseRoot>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
    spui: Option<Res<crate::cell_hud::CellHudActive>>,
    creature_spui: Option<Res<crate::creature_hud::CreatureHudActive>>,
    tribal_spui: Option<Res<crate::tribal_hud::TribalHudActive>>,
    civ_spui: Option<Res<crate::civ_hud::CivHudActive>>,
    space_spui: Option<Res<crate::space_hud::SpaceHudActive>>,
) {
    // The retail options menu (SPUI, see `cell_hud` and `creature_hud`)
    // replaces this panel.
    if spui.as_deref().is_some_and(|a| a.0)
        || creature_spui.as_deref().is_some_and(|a| a.0)
        || tribal_spui.as_deref().is_some_and(|a| a.0)
        || civ_spui.as_deref().is_some_and(|a| a.0)
        || space_spui.as_deref().is_some_and(|a| a.0)
    {
        for e in &existing {
            commands.entity(e).despawn();
        }
        return;
    }
    if !pause.is_changed() && (pause.open == !existing.is_empty()) {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !pause.open {
        return;
    }
    let mut root = commands.spawn((
        PauseRoot,
        GlobalZIndex(500),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.),
            height: Val::Percent(100.),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0., 0.015, 0.04, 0.72)),
    ));
    if let Ok(camera) = cameras.single() {
        root.insert(UiTargetCamera(camera));
    }
    root.with_children(|root| {
        root.spawn((
            Node {
                width: Val::Px(360.),
                padding: UiRect::all(Val::Px(28.)),
                row_gap: Val::Px(14.),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.09, 0.16, 0.96)),
            BorderColor::all(Color::srgb(0.35, 0.7, 0.85)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("Paused"),
                TextFont {
                    font_size: bevy::text::FontSize::Px(28.),
                    ..default()
                },
            ));
            for (label, action) in [
                ("Resume", PauseAction::Resume),
                ("Save game", PauseAction::Save),
                ("Main menu", PauseAction::MainMenu),
            ] {
                panel
                    .spawn((
                        Button,
                        action,
                        Node {
                            height: Val::Px(46.),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.06, 0.2, 0.29)),
                        BorderColor::all(Color::srgb(0.38, 0.65, 0.75)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: bevy::text::FontSize::Px(18.),
                                ..default()
                            },
                        ));
                    });
            }
            panel.spawn((
                Text::new(if pause.note.is_empty() {
                    "Esc to resume"
                } else {
                    &pause.note
                }),
                TextFont {
                    font_size: bevy::text::FontSize::Px(14.),
                    ..default()
                },
            ));
        });
    });
}
