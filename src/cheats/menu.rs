//! The debug menu (F10, or both sticks pressed on a controller): every cheat
//! and debug switch as a button, on five tabs. The buttons send the same
//! `Cheat`s as the console; the result shows in the status line at the
//! bottom. While the menu is open the game holds (unless "Keep the game
//! running" is chosen) and neither keyboard nor mouse buttons reach it.

use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;
use bevy::window::PrimaryWindow;

use super::commands::{Cheat, StageName, Toggle};
use super::console::line_color;
use super::{CheatLog, CheatSender, CheatState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Spore,
    Player,
    Stage,
    World,
    Debug,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Spore, Tab::Player, Tab::Stage, Tab::World, Tab::Debug];

    fn label(self) -> &'static str {
        match self {
            Tab::Spore => "Spore cheats",
            Tab::Player => "Player",
            Tab::Stage => "Stage",
            Tab::World => "World",
            Tab::Debug => "Debug",
        }
    }
}

/// What a menu button does.
#[derive(Component, Clone, Debug, PartialEq)]
pub enum MenuAction {
    Tab(Tab),
    Run(Cheat),
    /// Runs the cheat and closes the menu (the screenshot).
    RunAndClose(Cheat),
    /// Closes the menu; the next click in the world teleports there.
    ArmTeleport,
    OpenConsole,
    KeepRunning,
    Close,
}

/// One button of a tab: label, action, and the switch it shows (on / off).
pub struct Item {
    pub label: &'static str,
    pub action: MenuAction,
    pub shows: Option<fn(&CheatState) -> bool>,
}

fn run(label: &'static str, cheat: Cheat) -> Item {
    Item { label, action: MenuAction::Run(cheat), shows: None }
}

fn switch(label: &'static str, cheat: Cheat, shows: fn(&CheatState) -> bool) -> Item {
    Item { label, action: MenuAction::Run(cheat), shows: Some(shows) }
}

/// A group of buttons with a heading.
pub struct Group {
    pub heading: &'static str,
    pub items: Vec<Item>,
}

/// The buttons of each tab.
pub fn groups(tab: Tab) -> Vec<Group> {
    use Cheat as C;
    match tab {
        Tab::Spore => vec![
            Group {
                heading: "Spore's cheat console commands",
                items: vec![
                    run("addDNA", C::AddDna),
                    run("moreMoney", C::MoreMoney),
                    run("refillMotives", C::RefillMotives),
                    run("spaceCreate", C::SpaceCreate),
                    run("unlockSuperWeapons", C::UnlockSuperWeapons),
                    run("evoAdvantage", C::EvoAdvantage),
                    run("freeCam", C::FreeCam),
                    switch("freedom", C::Freedom(Toggle::Flip), |s| s.freedom),
                    switch("pauseUIVisible", C::PauseUiVisible(Toggle::Flip), |s| s.pause_ui_visible),
                    run("toggleEditorBackground", C::ToggleEditorBackground),
                ],
            },
            Group {
                heading: "setConsequenceTrait",
                items: super::apply::CONSEQUENCE_TRAITS
                    .iter()
                    .map(|(name, label)| run(label, C::SetConsequenceTrait((*name).to_string())))
                    .collect(),
            },
        ],
        Tab::Player => vec![
            Group {
                heading: "Survival",
                items: vec![
                    switch("God mode", C::GodMode(Toggle::Flip), |s| s.god_mode),
                    switch("Infinite energy", C::InfiniteEnergy(Toggle::Flip), |s| s.infinite_energy),
                    run("Refill health and motives", C::RefillMotives),
                    run("Heal everyone", C::HealAll),
                    run("Smite nearby enemies", C::Smite(40.0)),
                    Item {
                        label: "Teleport (click a spot)",
                        action: MenuAction::ArmTeleport,
                        shows: None,
                    },
                ],
            },
            Group {
                heading: "Resources",
                items: vec![
                    run("DNA +100", C::Dna(100)),
                    run("DNA +1,000", C::Dna(1000)),
                    run("Sporebucks +10,000", C::Money(10_000)),
                    run("Sporebucks +1,000,000", C::Money(1_000_000)),
                    run("Food +100", C::Food(100)),
                    run("Spice +10", C::Spice(10)),
                    run("Add a member", C::AddMember),
                ],
            },
        ],
        Tab::Stage => vec![
            Group {
                heading: "Progress",
                items: vec![
                    run("Progress +10%", C::Progress(10.0)),
                    run("Progress +25%", C::Progress(25.0)),
                    run("Complete the stage", C::CompleteStage),
                    run("Unlock all parts", C::UnlockParts),
                    run("Capture nearest city", C::CaptureCity),
                    run("Reveal the galaxy", C::RevealGalaxy),
                    run("Warp home", C::WarpHome),
                ],
            },
            Group {
                heading: "Start a new game in",
                items: StageName::ALL.iter().map(|s| run(s.label(), C::GotoStage(*s))).collect(),
            },
        ],
        Tab::World => vec![
            Group {
                heading: "Game speed",
                items: vec![
                    run("x0.25", C::TimeScale(0.25)),
                    run("x0.5", C::TimeScale(0.5)),
                    run("x1 (normal)", C::TimeScale(1.0)),
                    run("x2", C::TimeScale(2.0)),
                    run("x4", C::TimeScale(4.0)),
                ],
            },
            Group {
                heading: "Time of day (setTime)",
                items: vec![
                    run("Dawn 6:00", C::SetTime { hour: 6, minute: 0 }),
                    run("Noon 12:00", C::SetTime { hour: 12, minute: 0 }),
                    run("Dusk 18:00", C::SetTime { hour: 18, minute: 0 }),
                    run("Midnight 0:00", C::SetTime { hour: 0, minute: 0 }),
                    run("Stop the clock", C::SetTimeSpeed(0.0)),
                    run("Normal clock", C::SetTimeSpeed(1.0)),
                    run("Clock x10", C::SetTimeSpeed(10.0)),
                ],
            },
            Group {
                heading: "Menu",
                items: vec![Item {
                    label: "Keep the game running",
                    action: MenuAction::KeepRunning,
                    shows: Some(|s| !s.freeze_with_menu),
                }],
            },
        ],
        Tab::Debug => vec![Group {
            heading: "Debug",
            items: vec![
                switch("Stats readout", C::Stats(Toggle::Flip), |s| s.stats),
                switch("HUD", C::Hud(Toggle::Flip), |s| !s.hud_hidden),
                run("Free camera", C::FreeCam),
                Item {
                    label: "Screenshot",
                    action: MenuAction::RunAndClose(C::Screenshot),
                    shows: None,
                },
                Item {
                    label: "Open the console",
                    action: MenuAction::OpenConsole,
                    shows: None,
                },
            ],
        }],
    }
}

#[derive(Resource, Default)]
struct MenuView {
    tab: Tab,
}

#[derive(Component)]
struct MenuRoot {
    signature: String,
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuView>()
            .add_systems(Update, press.in_set(super::CheatSystems::Apply).before(super::core_cheats))
            .add_systems(PostUpdate, draw);
    }
}

fn press(
    pressed: Query<&MenuAction, Added<Pressed>>,
    mut view: ResMut<MenuView>,
    mut state: ResMut<CheatState>,
    mut sender: CheatSender,
) {
    for action in &pressed {
        if !state.menu_open {
            continue;
        }
        match action {
            MenuAction::Tab(t) => view.tab = *t,
            MenuAction::Run(cheat) => {
                sender.send(cheat.clone());
            }
            MenuAction::RunAndClose(cheat) => {
                sender.send(cheat.clone());
                state.menu_open = false;
            }
            MenuAction::ArmTeleport => {
                state.menu_open = false;
                state.teleport_armed = true;
                sender
                    .log()
                    .push(super::LineKind::Warning, "Click where to teleport (Esc cancels).");
                sender
                    .log()
                    .toast(super::LineKind::Warning, "Click where to teleport (Esc cancels).".into());
            }
            MenuAction::OpenConsole => {
                state.menu_open = false;
                state.console_open = true;
            }
            MenuAction::KeepRunning => state.freeze_with_menu = !state.freeze_with_menu,
            MenuAction::Close => state.menu_open = false,
        }
    }
}

const ACCENT: Color = Color::srgb(0.35, 0.7, 0.85);

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    state: Res<CheatState>,
    view: Res<MenuView>,
    log: Res<CheatLog>,
    roots: Query<(Entity, &MenuRoot)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
) {
    let scale = windows.iter().next().map(|w| (w.height() / 1080.0).clamp(0.6, 2.0)).unwrap_or(1.0);
    let tab_groups = groups(view.tab);
    let switches: Vec<bool> = tab_groups
        .iter()
        .flat_map(|g| g.items.iter())
        .map(|i| i.shows.is_some_and(|f| f(&state)))
        .collect();
    let status = log.lines.back().cloned();
    let signature = format!("{:?}|{switches:?}|{status:?}|{scale:.2}", view.tab);
    let mut current = false;
    for (entity, root) in &roots {
        if state.menu_open && root.signature == signature && !current {
            current = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !state.menu_open || current {
        return;
    }
    let px = |v: f32| Val::Px(v * scale);
    let font = |v: f32| TextFont {
        font_size: bevy::text::FontSize::Px(v * scale),
        ..default()
    };
    let mut root = commands.spawn((
        MenuRoot { signature },
        super::CheatUi,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.015, 0.04, 0.5)),
        GlobalZIndex(i32::MAX - 14),
    ));
    if let Some(camera) = cameras.iter().next() {
        root.insert(UiTargetCamera(camera));
    }
    let button = |parent: &mut ChildSpawnerCommands, label: String, action: MenuAction, lit: bool, width: f32| {
        parent
            .spawn((
                Button,
                action,
                Node {
                    width: px(width),
                    min_height: px(40.0),
                    padding: UiRect::axes(px(8.0), px(4.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(px(6.0)),
                    ..default()
                },
                BackgroundColor(if lit { Color::srgb(0.1, 0.36, 0.3) } else { Color::srgb(0.06, 0.2, 0.29) }),
                BorderColor::all(if lit { Color::srgb(0.45, 0.9, 0.7) } else { Color::srgb(0.38, 0.65, 0.75) }),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(label),
                    font(15.0),
                    TextLayout::justify(Justify::Center),
                    Pickable::IGNORE,
                ));
            });
    };
    root.with_children(|root| {
        root.spawn((
            Node {
                width: px(940.0),
                max_width: Val::Percent(94.0),
                max_height: Val::Percent(88.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20.0)),
                row_gap: px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(px(10.0)),
                overflow: Overflow::clip_y(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.09, 0.16, 0.97)),
            BorderColor::all(ACCENT),
        ))
        .with_children(|panel| {
            // Title and close button.
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((Text::new("Debug menu"), font(28.0)));
                    button(row, "Close (F10)".into(), MenuAction::Close, false, 130.0);
                });
            // Tabs.
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    for tab in Tab::ALL {
                        button(row, tab.label().into(), MenuAction::Tab(tab), tab == view.tab, 150.0);
                    }
                });
            // The tab's groups.
            for group in &tab_groups {
                panel.spawn((Text::new(group.heading), font(17.0), TextColor(ACCENT)));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(8.0),
                        row_gap: px(8.0),
                        ..default()
                    })
                    .with_children(|grid| {
                        for item in &group.items {
                            let lit = item.shows.is_some_and(|f| f(&state));
                            let label = match item.shows {
                                Some(_) => format!("{}: {}", item.label, if lit { "on" } else { "off" }),
                                None => item.label.to_string(),
                            };
                            button(grid, label, item.action.clone(), lit, 172.0);
                        }
                    });
            }
            // Status line and hints.
            let (kind, text) = status.clone().unwrap_or((super::LineKind::Info, "Pick a cheat.".into()));
            panel.spawn((Text::new(text), font(16.0), TextColor(line_color(kind)), TextLayout::no_wrap()));
            panel.spawn((
                Text::new("Ctrl+Shift+C or ` opens the console  -  Esc or F10 closes  -  controller: press both sticks"),
                font(13.0),
                TextColor(Color::srgb(0.65, 0.72, 0.8)),
            ));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_buttons_run_cheats_and_arm_the_teleport() {
        let mut app = super::super::tests::cheat_app();
        app.update();
        app.world_mut().resource_mut::<CheatState>().menu_open = true;
        app.world_mut().spawn((MenuAction::Run(Cheat::GodMode(Toggle::Flip)), Pressed));
        app.update();
        assert!(app.world().resource::<CheatState>().god_mode);
        assert!(app.world().resource::<CheatState>().menu_open);
        app.world_mut().spawn((MenuAction::ArmTeleport, Pressed));
        app.update();
        let state = app.world().resource::<CheatState>();
        assert!(state.teleport_armed && !state.menu_open);
    }

    #[test]
    fn every_tab_has_buttons_and_every_cheat_parses_back() {
        for tab in Tab::ALL {
            let groups = groups(tab);
            assert!(!groups.is_empty());
            for g in &groups {
                assert!(!g.items.is_empty(), "{:?}: {} is empty", tab, g.heading);
                for item in &g.items {
                    if let MenuAction::Run(c) | MenuAction::RunAndClose(c) = &item.action {
                        assert!(super::super::commands::find(c.name()).is_some(), "{}", item.label);
                    }
                }
            }
        }
    }
}
