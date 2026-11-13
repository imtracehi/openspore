//! The cinematic state machine, without Bevy (notes section 2).
//!
//! VERIFIED behaviour reproduced here:
//! * `PlayCinematic` (0xAE08B0 -> 0xADFCE0): fails when disabled; while a
//!   cinematic runs a request is queued (`queue`) or ignored; otherwise the
//!   entry state is entered and its id is the cinematic token;
//! * entering a state runs **all** of its commands at once, in file order
//!   (FSM SetState 0x7E3BF0 -> handler 0xAE0DB0);
//! * per frame (Update 0xAE0050): the wait timer counts down (UI waits do
//!   not), blockers are polled, `stateDuration` counts down; once every
//!   blocker is done a UI wait shows its button / arrow; the state advances
//!   when the duration is up, or everything is done and no wait is active, or
//!   an advance was forced;
//! * Advance (0xADDAD0): hides the button windows, clears wait / duration /
//!   running actions, makes the camera job non-blocking, then enters the
//!   `nextState`, or (none) forces `stopCurrentCinematic` next frame;
//! * Escape (0xAD8F10, unless `disableEscExit`): clears a UI wait and forces
//!   the built-in `switchToExitState` state, whose command (0xAE0000) stops
//!   the `-escStop` sounds and jumps to the exit state (`setExitState`), or
//!   stops the cinematic when there is none;
//! * stop (0xADFF50): cleanup, input unlock, "cinematic stopped" 0x44F1189.
//!
//! The host (the Bevy layer) executes everything else and reports blockers
//! (camera moves, blocking effects, actor actions) as finished.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::command::{Command, WaitKind};
use super::script::Library;
use crate::world_objects::name_id;

/// Built-in states of `global.trigger`.
pub const STOP_CURRENT: &str = "stopCurrentCinematic";
pub const SWITCH_TO_EXIT: &str = "switchToExitState";

/// A compiled state: its commands in file order.
#[derive(Clone, Debug)]
pub struct CompiledState {
    pub name: String,
    pub id: u32,
    pub file: String,
    pub commands: Vec<Command>,
}

/// Every state of the installed scripts, compiled into commands.
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub states: HashMap<u32, CompiledState>,
    /// (file, line, keyword) of every line with an unknown keyword.
    pub unknown: Vec<(String, usize, String)>,
    pub errors: Vec<String>,
    pub files: Vec<String>,
}

impl Program {
    pub fn compile(library: &Library) -> Program {
        let mut program = Program {
            errors: library.errors.clone(),
            files: library.files.clone(),
            ..Default::default()
        };
        for state in library.states.values() {
            let mut commands = Vec::with_capacity(state.lines.len());
            for line in &state.lines {
                let command = Command::parse(line);
                if let Command::Unknown(keyword) = &command {
                    program.unknown.push((state.file.clone(), line.line, keyword.clone()));
                }
                commands.push(command);
            }
            program.states.insert(
                state.id,
                CompiledState { name: state.name.clone(), id: state.id, file: state.file.clone(), commands },
            );
        }
        program.unknown.sort();
        program
    }

    pub fn state(&self, id: u32) -> Option<&CompiledState> {
        self.states.get(&id)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.states.contains_key(&name_id(name))
    }
}

/// Game pause requested by the cinematic (`cGameTimeManager` pause kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PauseMode {
    #[default]
    None,
    /// `pauseGame`: the simulation stops.
    Cinematic,
    /// `pauseGame -pauseGraphics`: animations and effects freeze as well.
    CinematicAll,
}

/// `letAIRun` (M+0x28 all AI, M+0x29 avatar only); nothing runs by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiMode {
    #[default]
    Stopped,
    All,
    AvatarOnly,
}

/// Which UI a wait shows once the state's blockers are done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinueUi {
    Button,
    Next,
}

/// Why sounds are stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundStop {
    /// Escape pressed: the `-escStop` sounds.
    Escape,
    /// The cinematic ended: every tracked sound except `-noStop`.
    End,
}

/// Work for the host, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Started { token: u32, name: String },
    /// Execute a script command; `blocker` is reported back with
    /// [`Runtime::finish`] when the action is done.
    Run { token: u32, command: Command, blocker: Option<u32> },
    Pause(PauseMode),
    /// Start (`Some(effect id)`) or stop the pause screen effect.
    ScreenEffect(Option<u32>),
    ShowContinue(ContinueUi),
    HideContinue,
    StopSounds(SoundStop),
    /// The camera job is stopped (`stopCamera`) or released at the end.
    ReleaseCamera,
    Ended { token: u32, name: String, skipped: bool, preserve_view: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Running,
}

/// The cinematic manager state (M, `cCinematicManager`).
#[derive(Default)]
pub struct Runtime {
    program: Arc<Program>,
    pub enabled: bool,
    phase: Phase,
    token: u32,
    name: String,
    current: u32,
    /// M+0x150 / 0x15D: state entered by the next advance.
    next: Option<u32>,
    force_advance: bool,
    /// M+0x154: `None` = no exit state (Escape stops).
    exit_state: Option<u32>,
    esc_disabled: bool,
    wait_active: bool,
    wait_timer: f32,
    ui_wait: Option<WaitKind>,
    ui_shown: bool,
    state_timer: Option<f32>,
    blockers: HashSet<u32>,
    camera_blocker: Option<u32>,
    next_blocker: u32,
    pause: PauseMode,
    screen_effect: bool,
    ai: AiMode,
    preserve_view: bool,
    skipped: bool,
    queue: VecDeque<(String, bool)>,
    ops: Vec<Op>,
    /// Seconds spent in the current state (diagnostics, UI-wait fallbacks).
    pub state_time: f32,
}

impl Runtime {
    pub fn new(program: Arc<Program>) -> Self {
        Self { program, enabled: true, next_blocker: 1, ..Default::default() }
    }

    pub fn program(&self) -> &Arc<Program> {
        &self.program
    }

    pub fn set_program(&mut self, program: Arc<Program>) {
        self.program = program;
    }

    pub fn is_playing(&self) -> bool {
        self.phase == Phase::Running
    }

    pub fn token(&self) -> Option<u32> {
        self.is_playing().then_some(self.token)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Name of the state being played.
    pub fn current_state(&self) -> Option<&str> {
        self.is_playing()
            .then(|| self.program.state(self.current).map(|s| s.name.as_str()))
            .flatten()
    }

    pub fn pause_mode(&self) -> PauseMode {
        self.pause
    }

    pub fn ai_mode(&self) -> AiMode {
        self.ai
    }

    /// Waiting for the continue message (button, arrow or game event).
    pub fn waiting_for_continue(&self) -> Option<WaitKind> {
        self.ui_wait
    }

    /// The continue button / arrow is on screen.
    pub fn continue_shown(&self) -> bool {
        self.ui_shown
    }

    pub fn escape_enabled(&self) -> bool {
        !self.esc_disabled
    }

    pub fn blockers(&self) -> usize {
        self.blockers.len()
    }

    /// Cinematics waiting for the running one to end (`queue`).
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Drop the queued (not yet started) cinematics of that name; whether
    /// one was dropped. A code stop of a cinematic that has not started yet.
    pub fn unqueue(&mut self, name: &str) -> bool {
        let before = self.queue.len();
        self.queue.retain(|(n, _)| n != name);
        self.queue.len() != before
    }

    /// The scripts hold a state (cinematic) of that name.
    pub fn has_state(&self, name: &str) -> bool {
        self.program.contains(name)
    }

    pub fn take_ops(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.ops)
    }

    /// `PlayCinematic(name, pauseGame, ..., queue)`. Returns the token (the
    /// entry state id), `None` when disabled, unknown or ignored.
    pub fn play(&mut self, name: &str, pause_game: bool, queue: bool) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        let id = name_id(name);
        if !self.program.states.contains_key(&id) {
            return None;
        }
        if self.phase != Phase::Idle {
            if queue {
                self.queue.push_back((name.to_string(), pause_game));
                return Some(id);
            }
            return None;
        }
        self.start(id, name, pause_game);
        Some(id)
    }

    fn start(&mut self, id: u32, name: &str, pause_game: bool) {
        self.phase = Phase::Running;
        self.token = id;
        self.name = name.to_string();
        self.next = None;
        self.force_advance = false;
        self.exit_state = None;
        self.esc_disabled = false;
        self.reset_wait();
        self.state_timer = None;
        self.blockers.clear();
        self.camera_blocker = None;
        self.ai = AiMode::Stopped;
        self.preserve_view = false;
        self.skipped = false;
        self.ops.push(Op::Started { token: id, name: name.to_string() });
        if pause_game {
            self.set_pause(PauseMode::Cinematic, None);
        }
        self.enter(id);
    }

    fn reset_wait(&mut self) {
        self.wait_active = false;
        self.wait_timer = 0.0;
        self.ui_wait = None;
        if self.ui_shown {
            self.ui_shown = false;
            self.ops.push(Op::HideContinue);
        }
    }

    /// Message 0x445F729: ends a UI wait (`-forButton/-forContinue/-forNext`).
    pub fn continue_cinematic(&mut self) {
        if self.ui_wait.is_none() {
            return;
        }
        self.reset_wait();
    }

    /// The player pressed Escape (0xAD8F10).
    pub fn escape(&mut self) {
        if !self.is_playing() || self.esc_disabled {
            return;
        }
        self.reset_wait();
        self.skipped = true;
        self.next = Some(name_id(SWITCH_TO_EXIT));
        self.force_advance = true;
    }

    /// Code stop (0xAD7D40): the current cinematic ends next frame.
    pub fn stop_current(&mut self) {
        if !self.is_playing() {
            return;
        }
        self.next = Some(name_id(STOP_CURRENT));
        self.force_advance = true;
    }

    /// A blocking action reported by the host is finished.
    pub fn finish(&mut self, blocker: u32) {
        self.blockers.remove(&blocker);
        if self.camera_blocker == Some(blocker) {
            self.camera_blocker = None;
        }
    }

    /// The cinematic manager's Update (0xAE0050); `dt` in seconds.
    pub fn update(&mut self, dt: f32) {
        if self.phase == Phase::Idle {
            if self.enabled {
                if let Some((name, pause)) = self.queue.pop_front() {
                    let id = name_id(&name);
                    if self.program.states.contains_key(&id) {
                        self.start(id, &name, pause);
                    }
                }
            }
            return;
        }
        self.state_time += dt;
        if self.wait_active && self.ui_wait.is_none() {
            self.wait_timer -= dt;
            if self.wait_timer <= 0.0 {
                self.wait_active = false;
            }
        }
        let all_done = self.blockers.is_empty();
        if all_done && !self.ui_shown {
            let ui = match self.ui_wait {
                Some(WaitKind::ForButton) => Some(ContinueUi::Button),
                Some(WaitKind::ForNext) => Some(ContinueUi::Next),
                _ => None,
            };
            if let Some(ui) = ui {
                self.ui_shown = true;
                self.ops.push(Op::ShowContinue(ui));
            }
        }
        let mut duration_up = false;
        if let Some(timer) = self.state_timer.as_mut() {
            *timer -= dt;
            duration_up = *timer <= 0.0;
        }
        if duration_up || (all_done && !self.wait_active) || self.force_advance {
            self.advance();
        }
    }

    /// 0xADDAD0.
    fn advance(&mut self) {
        self.reset_wait();
        self.state_timer = None;
        self.force_advance = false;
        self.blockers.clear();
        self.camera_blocker = None;
        match self.next.take() {
            Some(next) => {
                if self.program.states.contains_key(&next) {
                    self.enter(next);
                } else if next == name_id(SWITCH_TO_EXIT) {
                    self.switch_to_exit();
                } else if next == name_id(STOP_CURRENT) {
                    self.stop(self.skipped);
                } else {
                    // Invalid state -> switchToExitState.
                    self.next = Some(name_id(SWITCH_TO_EXIT));
                    self.force_advance = true;
                }
            }
            None => {
                self.next = Some(name_id(STOP_CURRENT));
                self.force_advance = true;
            }
        }
    }

    /// FSM SetState: run every command of the state.
    fn enter(&mut self, id: u32) {
        let Some(state) = self.program.state(id) else {
            return;
        };
        let commands = state.commands.clone();
        self.current = id;
        self.state_time = 0.0;
        for command in commands {
            if !self.is_playing() {
                break;
            }
            self.execute(command);
        }
    }

    fn new_blocker(&mut self) -> u32 {
        let id = self.next_blocker;
        self.next_blocker = self.next_blocker.wrapping_add(1).max(1);
        self.blockers.insert(id);
        id
    }

    fn set_pause(&mut self, mode: PauseMode, effect: Option<u32>) {
        if mode == self.pause {
            return;
        }
        self.pause = mode;
        self.ops.push(Op::Pause(mode));
        match effect {
            Some(id) => {
                self.screen_effect = true;
                self.ops.push(Op::ScreenEffect(Some(id)));
            }
            None => {
                if self.screen_effect {
                    self.screen_effect = false;
                    self.ops.push(Op::ScreenEffect(None));
                }
            }
        }
    }

    fn execute(&mut self, command: Command) {
        let token = self.token;
        match command {
            Command::Wait(kind) => {
                self.wait_active = true;
                match kind {
                    WaitKind::Secs(s) => {
                        self.wait_timer = s;
                        self.ui_wait = None;
                    }
                    ui => {
                        self.wait_timer = -1.0;
                        self.ui_wait = Some(ui);
                    }
                }
            }
            Command::StateDuration(s) => self.state_timer = Some(s),
            Command::NextState(id) => self.next = Some(id),
            // Not used by the installed scripts (alternatives indexed by
            // M+0x3C4, never set): the first alternative.
            Command::NextStateIf(ids) => self.next = ids.first().copied(),
            Command::SetExitState(state) => self.exit_state = state,
            Command::DisableEscExit => self.esc_disabled = true,
            Command::EnableEscExit => self.esc_disabled = false,
            Command::PreserveCinematicCamView => self.preserve_view = true,
            Command::PauseGame { graphics, screen_effect } => {
                let mode = if graphics { PauseMode::CinematicAll } else { PauseMode::Cinematic };
                self.set_pause(mode, screen_effect);
            }
            Command::UnpauseGame => self.set_pause(PauseMode::None, None),
            Command::LetAiRun { avatar_only, stop } => {
                self.ai = if stop {
                    AiMode::Stopped
                } else if avatar_only {
                    AiMode::AvatarOnly
                } else {
                    AiMode::All
                };
            }
            Command::StopCinematic => self.stop(self.skipped),
            Command::SwitchToExitState => self.switch_to_exit(),
            Command::StopCamera => {
                if let Some(old) = self.camera_blocker.take() {
                    self.blockers.remove(&old);
                }
                self.ops.push(Op::Run { token, command: Command::StopCamera, blocker: None });
            }
            Command::Description(_) | Command::Unknown(_) | Command::DisableDuringDemo => {}
            command => {
                let blocking = match &command {
                    Command::StartCamera(spec) => {
                        // A new camera job replaces the previous one.
                        if let Some(old) = self.camera_blocker.take() {
                            self.blockers.remove(&old);
                        }
                        spec.blocking
                    }
                    Command::StartEffect(e) => e.blocking,
                    Command::MoveActor(m) => m.blocking,
                    Command::PlayAnimation(p) => p.blocking,
                    Command::StartVignette(v) => v.blocking,
                    _ => false,
                };
                let blocker = blocking.then(|| self.new_blocker());
                if matches!(command, Command::StartCamera(_)) {
                    self.camera_blocker = blocker;
                }
                self.ops.push(Op::Run { token, command, blocker });
            }
        }
    }

    /// 0xAE0000.
    fn switch_to_exit(&mut self) {
        self.ops.push(Op::StopSounds(SoundStop::Escape));
        match self.exit_state {
            None => self.stop(true),
            Some(exit) => {
                self.next = Some(exit);
                self.force_advance = true;
                if let Some(old) = self.camera_blocker.take() {
                    self.blockers.remove(&old);
                }
            }
        }
    }

    /// stopCinematic (0xADFF50): cleanup and "cinematic stopped".
    pub fn stop(&mut self, skipped: bool) {
        if self.phase == Phase::Idle {
            return;
        }
        self.reset_wait();
        self.ops.push(Op::StopSounds(SoundStop::End));
        self.set_pause(PauseMode::None, None);
        if self.screen_effect {
            self.screen_effect = false;
            self.ops.push(Op::ScreenEffect(None));
        }
        if !self.preserve_view {
            self.ops.push(Op::ReleaseCamera);
        }
        self.ops.push(Op::Ended {
            token: self.token,
            name: self.name.clone(),
            skipped,
            preserve_view: self.preserve_view,
        });
        self.phase = Phase::Idle;
        self.next = None;
        self.force_advance = false;
        self.state_timer = None;
        self.blockers.clear();
        self.camera_blocker = None;
        self.ai = AiMode::Stopped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(text: &str) -> Arc<Program> {
        let mut library = Library::default();
        library.add_file(
            "global",
            "state stopCurrentCinematic\n stopCinematic\nend\nstate switchToExitState\n switchToExitState\nend\n",
        );
        library.add_file("test", text);
        assert!(library.errors.is_empty(), "{:?}", library.errors);
        Arc::new(Program::compile(&library))
    }

    fn runtime(text: &str) -> Runtime {
        Runtime::new(program(text))
    }

    /// Run frames of `dt` until the cinematic ends; the elapsed seconds.
    fn run_to_end(rt: &mut Runtime, dt: f32, limit: usize) -> f32 {
        let mut t = 0.0;
        for _ in 0..limit {
            if !rt.is_playing() {
                return t;
            }
            rt.update(dt);
            t += dt;
        }
        panic!("cinematic did not end");
    }

    fn ended(ops: &[Op]) -> Option<(bool, bool)> {
        ops.iter().find_map(|op| match op {
            Op::Ended { skipped, preserve_view, .. } => Some((*skipped, *preserve_view)),
            _ => None,
        })
    }

    #[test]
    fn states_chain_through_waits_and_end_without_next_state() {
        let mut rt = runtime(
            "state A\n letterbox -on\n nextState B\n wait -secs 1\nend\nstate B\n wait -secs 2\n text \"x\" t!0x1\nend\n",
        );
        assert_eq!(rt.play("A", false, false), Some(name_id("A")));
        let ops = rt.take_ops();
        assert!(matches!(ops[0], Op::Started { .. }));
        assert!(matches!(&ops[1], Op::Run { command: Command::Letterbox { on: true, .. }, blocker: None, .. }));
        // A: 1 s, B: 2 s, then the stopCurrentCinematic frame.
        let t = run_to_end(&mut rt, 0.1, 100);
        assert!((3.0..3.5).contains(&t), "{t}");
        assert_eq!(ended(&rt.take_ops()), Some((false, false)));
    }

    #[test]
    fn commands_run_in_file_order_regardless_of_wait_position() {
        let mut rt = runtime("state A\n nextState B\n wait -secs 1\n mixevent x 1\nend\nstate B\nend\n");
        rt.play("A", false, false);
        let ops = rt.take_ops();
        assert!(ops.iter().any(|op| matches!(op, Op::Run { command: Command::MixEvent { .. }, .. })));
        assert_eq!(rt.current_state(), Some("A"));
        rt.update(0.5);
        assert_eq!(rt.current_state(), Some("A"));
        rt.update(0.6);
        assert_eq!(rt.current_state(), Some("B"));
    }

    #[test]
    fn blockers_hold_the_state_and_state_duration_caps_it() {
        let mut rt = runtime(
            "state A\n startcamera -target \"x\" -duration 3\n nextState B\nend\nstate B\n playAnimation \"x\" (0x1)\n stateDuration 0.5\n nextState C\nend\nstate C\nend\n",
        );
        rt.play("A", false, false);
        let blocker = rt
            .take_ops()
            .iter()
            .find_map(|op| match op {
                Op::Run { command: Command::StartCamera(_), blocker, .. } => *blocker,
                _ => None,
            })
            .expect("blocking camera");
        for _ in 0..10 {
            rt.update(0.1);
        }
        assert_eq!(rt.current_state(), Some("A"), "camera still moving");
        rt.finish(blocker);
        rt.update(0.1);
        assert_eq!(rt.current_state(), Some("B"));
        // The animation never finishes, but stateDuration 0.5 ends B.
        for _ in 0..6 {
            rt.update(0.1);
        }
        assert_eq!(rt.current_state(), Some("C"));
    }

    #[test]
    fn a_new_camera_job_replaces_the_blocking_one() {
        let mut rt = runtime("state A\n startcamera -duration 3\n startcamera -noWait\n nextState B\nend\nstate B\nend\n");
        rt.play("A", false, false);
        assert_eq!(rt.blockers(), 0);
        rt.update(0.1);
        assert_eq!(rt.current_state(), Some("B"));
    }

    #[test]
    fn button_waits_show_after_blockers_and_continue_advances() {
        let mut rt = runtime(
            "state A\n text \"t\" crg!0x1\n wait -forButton\n playAnimation \"x\" (0x1)\n nextState B\nend\nstate B\nend\n",
        );
        rt.play("A", false, false);
        let anim = rt
            .take_ops()
            .iter()
            .find_map(|op| match op {
                Op::Run { command: Command::PlayAnimation(_), blocker, .. } => *blocker,
                _ => None,
            })
            .unwrap();
        rt.update(0.1);
        assert!(!rt.continue_shown(), "the button waits for the animation");
        rt.finish(anim);
        rt.update(0.1);
        assert!(rt.continue_shown());
        assert_eq!(rt.take_ops().last(), Some(&Op::ShowContinue(ContinueUi::Button)));
        for _ in 0..50 {
            rt.update(0.1);
        }
        assert_eq!(rt.current_state(), Some("A"), "waits for the player");
        rt.continue_cinematic();
        assert!(rt.take_ops().contains(&Op::HideContinue));
        rt.update(0.1);
        assert_eq!(rt.current_state(), Some("B"));
    }

    #[test]
    fn for_continue_shows_no_button() {
        let mut rt = runtime("state A\n wait -forContinue\nend\n");
        rt.play("A", false, false);
        rt.update(0.1);
        assert!(!rt.continue_shown());
        assert_eq!(rt.waiting_for_continue(), Some(WaitKind::ForContinue));
        rt.continue_cinematic();
        run_to_end(&mut rt, 0.1, 10);
    }

    #[test]
    fn a_queued_cinematic_can_be_dropped_before_it_starts() {
        let mut rt = runtime("state A\n wait -secs 1\nend\nstate B\n wait -secs 1\nend\n");
        assert!(rt.play("A", false, false).is_some());
        assert!(rt.play("B", false, true).is_some());
        assert_eq!(rt.queued(), 1);
        assert!(rt.unqueue("B"));
        assert!(!rt.unqueue("B"));
        run_to_end(&mut rt, 0.1, 30);
        rt.update(0.1);
        assert!(!rt.is_playing(), "B never starts");
    }

    #[test]
    fn escape_jumps_to_the_exit_state() {
        let mut rt = runtime(
            "state A\n setExitState Exit\n startsound music -escStop\n wait -secs 100\nend\nstate Exit\n mixevent done 1\nend\n",
        );
        rt.play("A", false, false);
        rt.take_ops();
        rt.escape();
        rt.update(0.016); // -> switchToExitState
        rt.update(0.016); // -> Exit
        let ops = rt.take_ops();
        assert!(ops.contains(&Op::StopSounds(SoundStop::Escape)));
        assert!(ops.iter().any(|op| matches!(op, Op::Run { command: Command::MixEvent { .. }, .. })));
        assert_eq!(rt.current_state(), Some("Exit"));
        run_to_end(&mut rt, 0.016, 10);
        assert_eq!(ended(&rt.take_ops()), Some((true, false)));
    }

    #[test]
    fn escape_without_exit_state_stops_and_can_be_disabled() {
        let mut rt = runtime("state A\n disableEscExit\n wait -secs 100\n nextState B\nend\nstate B\n enableEscExit\n wait -secs 100\nend\n");
        rt.play("A", false, false);
        rt.escape();
        rt.update(0.1);
        assert_eq!(rt.current_state(), Some("A"), "escape disabled");
        rt.stop_current();
        rt.update(0.1);
        assert!(!rt.is_playing());
        rt.take_ops();
        let mut rt = runtime("state A\n setExitState null\n wait -secs 100\nend\n");
        rt.play("A", false, false);
        rt.escape();
        rt.update(0.1);
        rt.update(0.1);
        assert!(!rt.is_playing());
        assert_eq!(ended(&rt.take_ops()), Some((true, false)));
    }

    #[test]
    fn pause_freeze_frame_and_preserved_view() {
        let mut rt = runtime(
            "state A\n pauseGame -screenEffect -pauseGraphics\n wait -secs .135\n nextState B\nend\nstate B\n unpauseGame\n preserveCinematicCamView\nend\n",
        );
        rt.play("A", true, false);
        let ops = rt.take_ops();
        assert_eq!(ops[1], Op::Pause(PauseMode::Cinematic));
        assert!(ops.contains(&Op::Pause(PauseMode::CinematicAll)));
        assert!(ops.contains(&Op::ScreenEffect(Some(0xDFE9_0CAD))));
        assert_eq!(rt.pause_mode(), PauseMode::CinematicAll);
        run_to_end(&mut rt, 0.05, 20);
        let ops = rt.take_ops();
        assert!(ops.contains(&Op::Pause(PauseMode::None)));
        assert!(ops.contains(&Op::ScreenEffect(None)));
        assert!(!ops.contains(&Op::ReleaseCamera));
        assert_eq!(ended(&ops), Some((false, true)));
    }

    #[test]
    fn queue_disabled_unknown_and_ai_mode() {
        let mut rt = runtime("state A\n letAIRun -avatarOnly\n wait -secs .5\nend\nstate B\n letAIRun\nend\n");
        assert_eq!(rt.play("Nope", false, false), None);
        rt.play("A", false, false);
        assert_eq!(rt.ai_mode(), AiMode::AvatarOnly);
        assert_eq!(rt.play("B", false, false), None, "ignored while playing");
        assert_eq!(rt.play("B", false, true), Some(name_id("B")), "queued");
        run_to_end(&mut rt, 0.1, 20);
        rt.update(0.1);
        assert!(rt.is_playing());
        assert_eq!(rt.name(), "B");
        assert_eq!(rt.ai_mode(), AiMode::All);
        let mut off = runtime("state A\nend\n");
        off.enabled = false;
        assert_eq!(off.play("A", false, false), None);
    }

    #[test]
    fn invalid_next_state_goes_to_the_exit_path() {
        let mut rt = runtime("state A\n nextState Missing\nend\n");
        rt.play("A", false, false);
        run_to_end(&mut rt, 0.1, 10);
        assert!(ended(&rt.take_ops()).is_some());
    }
}
