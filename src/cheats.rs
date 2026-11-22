//! Cheats and debug tools.
//!
//! - Spore's cheat console (Ctrl+Shift+C, or the backquote key) with the
//!   original cheats (`addDNA`, `moreMoney`, `refillMotives`, `freeCam`,
//!   `setTime`, ...) and OpenSpore's own cheats and debug switches; `help`
//!   lists them (`commands.rs`, `console.rs`).
//! - A debug menu (F10, or both sticks pressed on a controller) that runs the
//!   same cheats with buttons, grouped by stage (`menu.rs`).
//! - A stats readout and the cheat result toasts (`overlay.rs`).
//!
//! Cheats travel as `CheatRequest` messages. The stage effects live in
//! `apply.rs`; each handler records an outcome, and anything no stage
//! handled is reported as not available here. As in Spore, a cheat that
//! "notifies" (`cCheatManager::ProcessLine` posting `kMsgCheatInvoked`) marks
//! the game as cheated; in Space the first one earns the Joker badge
//! (`cPlayer` 0xC77E30). The "Hey, you just cheated!" text is built there but
//! never shown in retail, so it is not shown here either.

use std::collections::{HashMap, VecDeque};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

pub mod apply;
pub mod commands;
pub mod console;
pub mod menu;
pub mod overlay;

pub use apply::DaySpeed;
pub use commands::{Cheat, StageName};

/// The cheat switches and the console / menu state.
#[derive(Resource, Debug, Clone)]
pub struct CheatState {
    pub console_open: bool,
    pub menu_open: bool,
    /// A cheat changed the game this session (Spore's `mHasCheated`).
    pub has_cheated: bool,
    pub god_mode: bool,
    pub infinite_energy: bool,
    pub stats: bool,
    pub hud_hidden: bool,
    /// `pauseUIVisible`: the HUD stays on screen while paused.
    pub pause_ui_visible: bool,
    /// `freedom`: editor complexity limits are off.
    pub freedom: bool,
    /// `timeScale`
    pub time_scale: f32,
    /// The debug menu holds the game while it is open.
    pub freeze_with_menu: bool,
    /// `toggleEditorBackground`: the editors show no room or backdrop.
    pub editor_background_off: bool,
    /// The menu's "Teleport" waits for a click in the world.
    pub teleport_armed: bool,
}

impl Default for CheatState {
    fn default() -> Self {
        CheatState {
            console_open: false,
            menu_open: false,
            has_cheated: false,
            god_mode: false,
            infinite_energy: false,
            stats: false,
            hud_hidden: false,
            pause_ui_visible: true,
            freedom: false,
            time_scale: 1.0,
            freeze_with_menu: true,
            editor_background_off: false,
            teleport_armed: false,
        }
    }
}

impl CheatState {
    /// Whether the debug menu holds the game (`pause_menu::simulation_active`).
    pub fn holds_game(&self) -> bool {
        self.menu_open && self.freeze_with_menu
    }

    /// The keyboard (and, for the menu, the mouse) belong to the console or
    /// the menu instead of the game.
    pub fn captures_input(&self) -> bool {
        self.console_open || self.menu_open
    }
}

/// The cheat console, menu, stats and toasts (the hidden HUD leaves them).
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct CheatUi;

/// The game's simulation runs unless the debug menu holds it (a run
/// condition of `GameSystems::Simulation`, next to the pause menu's).
pub fn simulation_active(state: Option<Res<CheatState>>) -> bool {
    !state.is_some_and(|s| s.holds_game())
}

/// One cheat to run; `id` matches its outcome in the log.
#[derive(Message, Clone, Debug)]
pub struct CheatRequest {
    pub id: u32,
    pub cheat: Cheat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// A typed command.
    Input,
    Info,
    Done,
    Refused,
    Warning,
}

/// The console text, the pending outcomes of this frame's cheats and the
/// toasts shown while the console is closed.
#[derive(Resource, Default, Debug)]
pub struct CheatLog {
    pub lines: VecDeque<(LineKind, String)>,
    /// Bumped on every change, so the console can tell when to redraw.
    pub revision: u64,
    next_id: u32,
    outcomes: HashMap<u32, Vec<(bool, String)>>,
    /// Text, seconds left.
    pub toasts: VecDeque<(LineKind, String, f32)>,
}

const LOG_LINES: usize = 300;
const TOAST_SECONDS: f32 = 4.0;
const TOASTS: usize = 4;

impl CheatLog {
    pub fn push(&mut self, kind: LineKind, text: impl Into<String>) {
        self.lines.push_back((kind, text.into()));
        while self.lines.len() > LOG_LINES {
            self.lines.pop_front();
        }
        self.revision += 1;
    }

    fn toast(&mut self, kind: LineKind, text: String) {
        self.toasts.push_back((kind, text, TOAST_SECONDS));
        while self.toasts.len() > TOASTS {
            self.toasts.pop_front();
        }
    }

    /// The cheat `id` did something; `text` says what.
    pub fn done(&mut self, id: u32, text: impl Into<String>) {
        self.outcomes.entry(id).or_default().push((true, text.into()));
    }

    /// The cheat `id` could not run; `text` says why.
    pub fn refuse(&mut self, id: u32, text: impl Into<String>) {
        self.outcomes.entry(id).or_default().push((false, text.into()));
    }

    fn take_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1);
        self.next_id
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.revision += 1;
    }
}

/// Sends cheats (the console, the menu, tests).
#[derive(SystemParam)]
pub struct CheatSender<'w> {
    log: ResMut<'w, CheatLog>,
    out: MessageWriter<'w, CheatRequest>,
}

impl CheatSender<'_> {
    pub fn send(&mut self, cheat: Cheat) -> u32 {
        let id = self.log.take_id();
        self.out.write(CheatRequest { id, cheat });
        id
    }

    pub fn log(&mut self) -> &mut CheatLog {
        &mut self.log
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CheatSystems {
    /// Hotkeys, typing, input capture (PreUpdate).
    Input,
    /// The switches kept here and the stage effects (Update).
    Apply,
    /// Outcomes into the console and toasts (Update).
    Report,
}

pub struct CheatsPlugin;

impl Plugin for CheatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CheatState>()
            .init_resource::<CheatLog>()
            .add_message::<CheatRequest>()
            .configure_sets(
                PreUpdate,
                CheatSystems::Input
                    .after(bevy::input::InputSystems)
                    .after(crate::controller::ControllerSystems::Drive)
                    .before(bevy::ui::UiSystems::Focus),
            )
            .configure_sets(Update, (CheatSystems::Apply, CheatSystems::Report).chain())
            .add_systems(Update, (core_cheats.in_set(CheatSystems::Apply), report.in_set(CheatSystems::Report)))
            .add_plugins((console::ConsolePlugin, menu::MenuPlugin, overlay::OverlayPlugin, apply::ApplyPlugin));
    }
}

/// The cheats that need no stage: the switches, help, clear, time scale.
fn core_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut state: ResMut<CheatState>,
    mut log: ResMut<CheatLog>,
    mut time: Option<ResMut<Time<Virtual>>>,
) {
    let on_off = |v: bool| if v { "on" } else { "off" };
    for r in requests.read() {
        match &r.cheat {
            Cheat::Help { pattern, full } => {
                for line in commands::help(pattern.as_deref(), *full) {
                    log.done(r.id, line);
                }
            }
            Cheat::Clear => {
                log.clear();
                log.done(r.id, String::new());
            }
            Cheat::GodMode(t) => {
                let v = t.apply(&mut state.god_mode);
                log.done(r.id, format!("God mode {}", on_off(v)));
            }
            Cheat::InfiniteEnergy(t) => {
                let v = t.apply(&mut state.infinite_energy);
                log.done(r.id, format!("Infinite energy {}", on_off(v)));
            }
            Cheat::Stats(t) => {
                let v = t.apply(&mut state.stats);
                log.done(r.id, format!("Stats {}", on_off(v)));
            }
            Cheat::Hud(t) => {
                let mut shown = !state.hud_hidden;
                let v = t.apply(&mut shown);
                state.hud_hidden = !v;
                log.done(r.id, format!("HUD {}", on_off(v)));
            }
            Cheat::PauseUiVisible(t) => {
                let v = t.apply(&mut state.pause_ui_visible);
                // The original prints "pause ui visible: yes" / "no".
                log.done(r.id, format!("pause ui visible: {}", if v { "yes" } else { "no" }));
            }
            Cheat::Freedom(t) => {
                let v = t.apply(&mut state.freedom);
                log.done(
                    r.id,
                    if v {
                        "Editor limits are disabled. Freedom!"
                    } else {
                        "Editor limits are enabled. Creations will be pollinated"
                    },
                );
            }
            Cheat::TimeScale(s) => {
                state.time_scale = *s;
                if let Some(time) = time.as_deref_mut() {
                    time.set_relative_speed(*s);
                }
                log.done(r.id, format!("Game speed x{s:.2}"));
            }
            _ => {}
        }
    }
}

/// Whether a cheat marks the game as cheated. Spore registers `freeCam`,
/// `pauseUIVisible` and `setTime` without the notify flag and every other
/// cheat here with it (it posts even when the cheat did nothing); of
/// OpenSpore's own commands the viewing and debug ones do not count.
pub fn notifies(cheat: &Cheat) -> bool {
    !matches!(
        cheat,
        Cheat::Help { .. }
            | Cheat::Clear
            | Cheat::Stats(_)
            | Cheat::Hud(_)
            | Cheat::PauseUiVisible(_)
            | Cheat::Screenshot
            | Cheat::FreeCam
            | Cheat::SetTime { .. }
            | Cheat::SetTimeSpeed(_)
    )
}

fn report(mut requests: MessageReader<CheatRequest>, mut state: ResMut<CheatState>, mut log: ResMut<CheatLog>, time: Res<Time<Real>>) {
    let toast = !state.console_open;
    for r in requests.read() {
        if notifies(&r.cheat) {
            state.has_cheated = true;
        }
        let outcomes = log.outcomes.remove(&r.id).unwrap_or_default();
        if outcomes.is_empty() {
            let text = format!("{} isn't available here.", r.cheat.name());
            log.push(LineKind::Refused, text.clone());
            if toast {
                log.toast(LineKind::Refused, text);
            }
            continue;
        }
        for (done, text) in outcomes {
            if text.is_empty() {
                continue;
            }
            let kind = if done { LineKind::Done } else { LineKind::Refused };
            log.push(kind, text.clone());
            if toast && !matches!(r.cheat, Cheat::Help { .. }) {
                log.toast(kind, text);
            }
        }
    }
    let dt = time.delta_secs();
    for t in log.toasts.iter_mut() {
        t.2 -= dt;
    }
    while log.toasts.front().is_some_and(|t| t.2 <= 0.0) {
        log.toasts.pop_front();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::commands::Toggle;
    use super::*;

    /// An app with the cheat plugin and nothing else of the game.
    pub(crate) fn cheat_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
        app.add_plugins(CheatsPlugin);
        app
    }

    pub(crate) fn send(app: &mut App, cheat: Cheat) {
        let id = {
            let mut log = app.world_mut().resource_mut::<CheatLog>();
            log.take_id()
        };
        app.world_mut().resource_mut::<Messages<CheatRequest>>().write(CheatRequest { id, cheat });
        app.update();
    }

    pub(crate) fn last_lines(app: &App, n: usize) -> Vec<String> {
        let log = app.world().resource::<CheatLog>();
        log.lines.iter().rev().take(n).map(|l| l.1.clone()).collect::<Vec<_>>().into_iter().rev().collect()
    }

    #[test]
    fn switches_flip_and_report() {
        let mut app = cheat_app();
        send(&mut app, Cheat::GodMode(Toggle::Flip));
        assert!(app.world().resource::<CheatState>().god_mode);
        assert!(app.world().resource::<CheatState>().has_cheated);
        // Retail never prints its "you just cheated" text.
        assert_eq!(last_lines(&app, 1)[0], "God mode on");
        send(&mut app, Cheat::GodMode(Toggle::Off));
        assert!(!app.world().resource::<CheatState>().god_mode);
        assert_eq!(last_lines(&app, 1)[0], "God mode off");
    }

    #[test]
    fn unhandled_cheats_are_not_available() {
        let mut app = cheat_app();
        send(&mut app, Cheat::CaptureCity);
        assert_eq!(last_lines(&app, 1)[0], "captureCity isn't available here.");
    }

    #[test]
    fn help_and_viewing_switches_are_not_cheating() {
        let mut app = cheat_app();
        send(&mut app, Cheat::Help { pattern: Some("money".into()), full: false });
        send(&mut app, Cheat::Stats(Toggle::On));
        send(&mut app, Cheat::PauseUiVisible(Toggle::Off));
        assert!(!app.world().resource::<CheatState>().has_cheated);
        send(&mut app, Cheat::SetTimeSpeed(2.0));
        assert!(!app.world().resource::<CheatState>().has_cheated);
        assert!(last_lines(&app, 2).contains(&"pause ui visible: no".to_string()));
        assert!(app.world().resource::<CheatLog>().lines.iter().any(|l| l.1.contains("moreMoney")));
    }

    #[test]
    fn time_scale_sets_virtual_time() {
        let mut app = cheat_app();
        send(&mut app, Cheat::TimeScale(2.5));
        assert_eq!(app.world().resource::<Time<Virtual>>().relative_speed(), 2.5);
    }
}
