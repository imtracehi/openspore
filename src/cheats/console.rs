//! Spore's cheat console. Ctrl+Shift+C (as in Spore) or the backquote key
//! opens a text line at the top of the screen: type a cheat and press Enter;
//! `help` lists them. Up / Down recall earlier lines, Tab completes a cheat
//! name, Esc closes. While it is open the keyboard belongs to the console.
//!
//! This module also owns the hotkeys and the input capture of the debug
//! menu (F10, or both sticks pressed on a controller): while the menu is
//! open neither the keyboard nor the mouse buttons reach the game (the menu's
//! own buttons use pointer events, which are not affected).

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::{commands, CheatLog, CheatSender, CheatState, CheatSystems, LineKind};
use crate::controller::{Pad, PadState};

/// The line being typed and the history of entered lines.
#[derive(Resource, Default, Debug)]
pub struct ConsoleInput {
    pub text: String,
    history: Vec<String>,
    recall: Option<usize>,
}

impl ConsoleInput {
    fn recall(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let last = self.history.len() - 1;
        self.recall = match (self.recall, older) {
            (None, true) => Some(last),
            (None, false) => None,
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i >= last => None,
            (Some(i), false) => Some(i + 1),
        };
        self.text = self.recall.map(|i| self.history[i].clone()).unwrap_or_default();
    }

    /// Tab: completes the cheat name (the common prefix of the matches).
    fn complete(&mut self, log: &mut CheatLog) {
        if self.text.contains(' ') {
            return;
        }
        let matches = commands::complete(&self.text);
        match matches.as_slice() {
            [] => {}
            [one] => self.text = format!("{one} "),
            many => {
                let first = many[0].to_ascii_lowercase();
                let mut common = first.len();
                for m in &many[1..] {
                    let m = m.to_ascii_lowercase();
                    common = common.min(first.chars().zip(m.chars()).take_while(|(a, b)| a == b).count());
                }
                if common > self.text.len() {
                    self.text = many[0][..common].to_string();
                } else {
                    log.push(LineKind::Info, many.join("  "));
                }
            }
        }
    }
}

#[derive(Component)]
struct ConsoleRoot {
    signature: String,
}

pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleInput>()
            .add_systems(PreUpdate, input.in_set(CheatSystems::Input))
            .add_systems(PostUpdate, draw);
    }
}

/// Hotkeys, typing and input capture.
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    mut state: ResMut<CheatState>,
    mut console: ResMut<ConsoleInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut typed: MessageReader<KeyboardInput>,
    pad: Option<Res<PadState>>,
    mut sticks_were_down: Local<bool>,
    mut sender: CheatSender,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut opened_now = false;
    // A key that closes the console or menu this frame does not reach the
    // game either (Escape would open the pause menu).
    let was_capturing = state.captures_input();
    // Ctrl+Shift+C (Spore) or the backquote key: the console.
    if (ctrl && shift && keys.just_pressed(KeyCode::KeyC)) || (!state.console_open && keys.just_pressed(KeyCode::Backquote)) {
        state.console_open = !state.console_open;
        opened_now = state.console_open;
        keys.reset(KeyCode::KeyC);
        keys.reset(KeyCode::Backquote);
    }
    // F10 or both sticks pressed: the debug menu.
    let sticks_down = pad.as_ref().is_some_and(|p| p.pressed(Pad::LS) && p.pressed(Pad::RS));
    if keys.just_pressed(KeyCode::F10) || (sticks_down && !*sticks_were_down) {
        state.menu_open = !state.menu_open;
        keys.reset(KeyCode::F10);
    }
    *sticks_were_down = sticks_down;

    if state.console_open && !opened_now {
        for k in typed.read() {
            if k.state != ButtonState::Pressed {
                continue;
            }
            match k.key_code {
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    submit(&mut console, &mut sender);
                    continue;
                }
                KeyCode::Escape => {
                    state.console_open = false;
                    console.text.clear();
                    console.recall = None;
                    break;
                }
                KeyCode::Backquote if console.text.is_empty() => {
                    state.console_open = false;
                    break;
                }
                KeyCode::Backspace => {
                    console.text.pop();
                    continue;
                }
                KeyCode::ArrowUp => {
                    console.recall(true);
                    continue;
                }
                KeyCode::ArrowDown => {
                    console.recall(false);
                    continue;
                }
                KeyCode::Tab => {
                    console.complete(sender.log());
                    continue;
                }
                _ => {}
            }
            // The controller layer sends Enter / Escape with a logical key
            // only; everything else typed carries text.
            match &k.logical_key {
                Key::Enter => submit(&mut console, &mut sender),
                Key::Escape => {
                    state.console_open = false;
                    console.text.clear();
                    console.recall = None;
                }
                _ if ctrl => {}
                _ => {
                    if let Some(text) = &k.text {
                        if console.text.chars().count() < 120 {
                            console.text.extend(text.chars().filter(|c| !c.is_control()));
                        }
                    }
                }
            }
        }
    } else {
        typed.clear();
    }

    if state.menu_open && keys.just_pressed(KeyCode::Escape) && !state.console_open {
        state.menu_open = false;
    }
    // The game does not see keys typed into the console or pressed in the
    // menu; the menu also keeps mouse buttons from the game.
    if was_capturing || state.captures_input() {
        keys.reset_all();
    }
    if state.menu_open {
        mouse.reset_all();
    }
}

fn submit(console: &mut ConsoleInput, sender: &mut CheatSender) {
    let line = std::mem::take(&mut console.text);
    console.recall = None;
    let line = line.trim().to_string();
    if line.is_empty() {
        return;
    }
    sender.log().push(LineKind::Input, format!("> {line}"));
    if console.history.last() != Some(&line) {
        console.history.push(line.clone());
    }
    match commands::parse(&line) {
        Ok(Some(cheat)) => {
            sender.send(cheat);
        }
        Ok(None) => {}
        Err(e) => sender.log().push(LineKind::Refused, e),
    }
}

/// Lines of scrollback shown.
const SHOWN: usize = 14;

pub(super) fn line_color(kind: LineKind) -> Color {
    match kind {
        LineKind::Input => Color::srgb(0.75, 0.86, 1.0),
        LineKind::Info => Color::srgb(0.85, 0.87, 0.9),
        LineKind::Done => Color::srgb(0.62, 0.95, 0.55),
        LineKind::Refused => Color::srgb(1.0, 0.6, 0.5),
        LineKind::Warning => Color::srgb(1.0, 0.86, 0.35),
    }
}

fn draw(
    mut commands: Commands,
    state: Res<CheatState>,
    console: Res<ConsoleInput>,
    log: Res<CheatLog>,
    time: Res<Time<Real>>,
    roots: Query<(Entity, &ConsoleRoot)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
) {
    let scale = windows.iter().next().map(|w| (w.height() / 1080.0).clamp(0.6, 2.0)).unwrap_or(1.0);
    let caret = (time.elapsed_secs() * 2.0) as u32 % 2 == 0;
    let signature = format!("{}|{}|{caret}|{scale:.2}", log.revision, console.text);
    let mut current = false;
    for (entity, root) in &roots {
        if state.console_open && root.signature == signature && !current {
            current = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !state.console_open || current {
        return;
    }
    let font = |px: f32| TextFont {
        font_size: bevy::text::FontSize::Px(px * scale),
        ..default()
    };
    let mut root = commands.spawn((
        ConsoleRoot { signature },
        super::CheatUi,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(14.0 * scale), Val::Px(8.0 * scale)),
            row_gap: Val::Px(2.0 * scale),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.84)),
        BorderColor::all(Color::srgb(0.35, 0.7, 0.85)),
        GlobalZIndex(i32::MAX - 10),
        Pickable::IGNORE,
    ));
    if let Some(camera) = cameras.iter().next() {
        root.insert(UiTargetCamera(camera));
    }
    let start = log.lines.len().saturating_sub(SHOWN);
    let lines: Vec<(LineKind, String)> = log.lines.iter().skip(start).cloned().collect();
    root.with_children(|root| {
        if lines.is_empty() {
            root.spawn((
                Text::new("Cheat console. Type help for the list of cheats; Esc closes."),
                font(16.0),
                TextColor(line_color(LineKind::Info)),
                TextLayout::no_wrap(),
            ));
        }
        for (kind, text) in lines {
            root.spawn((Text::new(text), font(16.0), TextColor(line_color(kind)), TextLayout::no_wrap()));
        }
        let shown = format!("> {}{}", console.text, if caret { "_" } else { " " });
        root.spawn((
            Text::new(shown),
            font(19.0),
            TextColor(Color::WHITE),
            TextLayout::no_wrap(),
            Node {
                margin: UiRect::top(Val::Px(4.0 * scale)),
                ..default()
            },
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{cheat_app, last_lines};
    use super::*;
    use bevy::input::keyboard::NativeKey;

    fn key(app: &mut App, code: KeyCode, logical: Key, text: Option<&str>) {
        let window = app.world_mut().spawn_empty().id();
        let mut messages = app.world_mut().resource_mut::<Messages<KeyboardInput>>();
        messages.write(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window,
        });
        app.update();
        let mut messages = app.world_mut().resource_mut::<Messages<KeyboardInput>>();
        messages.write(KeyboardInput {
            key_code: code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            let s = c.to_string();
            key(app, KeyCode::KeyA, Key::Character(s.clone().into()), Some(&s));
        }
    }

    #[test]
    fn the_console_opens_types_runs_and_closes() {
        let mut app = cheat_app();
        app.update();
        key(&mut app, KeyCode::Backquote, Key::Character("`".into()), Some("`"));
        assert!(app.world().resource::<CheatState>().console_open);
        type_text(&mut app, "godmode on");
        assert_eq!(app.world().resource::<ConsoleInput>().text, "godmode on");
        key(&mut app, KeyCode::Enter, Key::Enter, None);
        assert!(app.world().resource::<CheatState>().god_mode);
        assert!(last_lines(&app, 3).iter().any(|l| l == "> godmode on"));
        // History and completion.
        key(&mut app, KeyCode::ArrowUp, Key::ArrowUp, None);
        assert_eq!(app.world().resource::<ConsoleInput>().text, "godmode on");
        key(&mut app, KeyCode::Escape, Key::Escape, None);
        assert!(!app.world().resource::<CheatState>().console_open);
        assert_eq!(app.world().resource::<ConsoleInput>().text, "");
        key(&mut app, KeyCode::Backquote, Key::Character("`".into()), Some("`"));
        type_text(&mut app, "refi");
        key(&mut app, KeyCode::Tab, Key::Tab, None);
        assert_eq!(app.world().resource::<ConsoleInput>().text, "refillMotives ");
    }

    #[test]
    fn typing_never_reaches_the_game() {
        let mut app = cheat_app();
        app.update();
        key(&mut app, KeyCode::Backquote, Key::Character("`".into()), Some("`"));
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state: ButtonState::Pressed,
            text: Some("w".into()),
            repeat: false,
            window,
        });
        app.update();
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        assert_eq!(app.world().resource::<ConsoleInput>().text, "w");
    }

    #[test]
    fn f10_opens_the_menu_and_escape_closes_it() {
        let mut app = cheat_app();
        app.update();
        key(&mut app, KeyCode::F10, Key::F10, None);
        assert!(app.world().resource::<CheatState>().menu_open);
        // The Escape that closes the menu does not reach the game (it would
        // open the pause menu).
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        assert!(!app.world().resource::<CheatState>().menu_open);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));
    }
}
