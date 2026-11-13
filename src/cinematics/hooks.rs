//! Stage-start hooks (the cinematics the retail stage code plays when a stage
//! begins), the stage bindings and the `--cinematic <name>` quick capture.
//!
//! * `CLG2CRG` when the Creature stage starts (caller 0xD43DE0 binds
//!   "AvatarNest" to the player's nest);
//! * `CRG_BrainUpgrade1..4` when the brain level rises (0xD3FCA0 via
//!   0xD38800, level -> name);
//! * `TRG_Begin` when the Tribal stage starts (0xCD4400:
//!   `PlayCinematic("TRG_Begin", pauseGame 1, ...)`, binds "PlayerTribe",
//!   "TribeHut", "TribeFire");
//! * `TRG2CVG_CivStart` when the Civilization stage starts (0xCFBD70);
//! * `CVG2SPG` when the Space stage starts after Civilization (0xFF5D90).
//!
//! The other retail moments (Creature events, Tribal totem / upgrade, Civ
//! unlocks, Space `pending_cinematic`, the Evolve / Advance transitions) are
//! in [`super::moments`]. ADAPTED: these hooks watch the stage and
//! `RunProgress` instead of being called by the stage modules; each stage
//! start plays once per run (`RunProgress::cinematics_played`).

use std::path::Path;

use bevy::prelude::*;

use super::{CinematicTarget, Cinematics};
use crate::game::{Player, RunProgress};
use crate::stages::GameStage;

#[derive(Default)]
pub struct HookState {
    stage: Option<GameStage>,
    /// The stage whose start cinematic is waiting for the stage's entities.
    pending: Option<GameStage>,
    frames: u32,
    brain: Option<u8>,
}

/// The start cinematic of a stage and whether it pauses the game.
fn start_cinematic(stage: &GameStage) -> Option<(&'static str, bool)> {
    match stage {
        GameStage::Creature => Some(("CLG2CRG", false)),
        GameStage::Tribal => Some(("TRG_Begin", true)),
        GameStage::Civilization => Some(("TRG2CVG_CivStart", false)),
        GameStage::Space => Some(("CVG2SPG", false)),
        _ => None,
    }
}

/// The names the retail stage code binds for the current stage
/// (see [`super::moments::bindings`]).
pub fn stage_bindings(world: &mut World) -> Vec<(&'static str, CinematicTarget)> {
    super::moments::bindings(world)
}

/// The Tribal start bindings (0xCD4400): `PlayerTribe`, `TribeHut`, `TribeFire`.
pub(super) fn tribal_bindings(world: &mut World) -> Vec<(&'static str, CinematicTarget)> {
    let mut out = Vec::new();
    let home = crate::tribal::home_position();
    let mut q = world.query::<(Entity, &crate::tribal_rts::HomeProp)>();
    let hut = q
        .iter(world)
        .find(|(_, p)| p.kind == crate::tribal_rts::HomePropKind::Hut)
        .map(|(e, _)| e);
    let mut fires = world.query::<(Entity, &crate::tribal_tools::ToolShack)>();
    let fire = fires
        .iter(world)
        .find(|(_, s)| s.0 == crate::tribal_tools::ToolKind::Campfire)
        .map(|(e, _)| e);
    // ADAPTED: the fire pit (tool type 10) is not built at the start of
    // OpenSpore's Tribal stage; the village food mat in front of the
    // hut stands in for it, facing the hut.
    let mat = crate::tribal::food_pile_position();
    let toward_hut = crate::creature::facing_yaw(home - mat);
    out.push((
        "TribeFire",
        fire.map_or(CinematicTarget::Point { position: mat, yaw: toward_hut }, CinematicTarget::Entity),
    ));
    out.push(("TribeHut", hut.map_or(CinematicTarget::Point { position: home, yaw: 0.0 }, CinematicTarget::Entity)));
    out.push(("PlayerTribe", CinematicTarget::Point { position: home, yaw: toward_hut }));
    out
}

/// Play `name` with the stage's bindings.
pub fn play_with_bindings(world: &mut World, name: &str, pause_game: bool, queue: bool) -> Option<super::CinematicToken> {
    super::moments::prepare(world, &super::moments::Play::new(name));
    let bindings = stage_bindings(world);
    let mut cin = world.get_resource_mut::<Cinematics>()?;
    let token = cin.play(name, pause_game, queue)?;
    for (n, target) in bindings {
        cin.bind_object(token, n, target);
    }
    Some(token)
}

fn has_player(world: &mut World) -> bool {
    let mut q = world.query_filtered::<(), With<Player>>();
    q.iter(world).next().is_some()
}

/// Watch the stage and the brain level (see the module docs).
pub fn stage_hooks(world: &mut World, mut local: Local<HookState>) {
    let Some(stage) = world.get_resource::<State<GameStage>>().map(|s| s.get().clone()) else {
        return;
    };
    let (hooks, loaded) = world.get_resource::<Cinematics>().map_or((false, false), |c| (c.settings.hooks, c.loaded()));
    let brain = world.get_resource::<RunProgress>().map(|r| r.creature_progression.brain_level);
    if local.stage.as_ref() != Some(&stage) {
        // A stage change ends whatever cinematic still plays (its actors and
        // targets belonged to the old stage).
        let previous = local.stage.clone();
        if previous.is_some() {
            if let Some(mut c) = world.get_resource_mut::<Cinematics>() {
                if c.is_playing() {
                    c.runtime.stop(true);
                }
            }
        }
        local.stage = Some(stage.clone());
        local.frames = 0;
        local.brain = brain;
        // Only at a fresh stage start: not when a save is loaded into the
        // stage, and once per run.
        let fresh = world.get_resource::<RunProgress>().is_some_and(|r| {
            let done = |name: &str| r.cinematics_played.iter().any(|n| n == name);
            match stage {
                GameStage::Creature => r.creature_progression.brain_level == 0 && !done("CLG2CRG"),
                GameStage::Tribal => !done("TRG_Begin") && r.tribal_campaign.resolved_count() == 0,
                GameStage::Civilization => r.civ_campaign.clock < 10.0 && !done("TRG2CVG_CivStart"),
                // The UFO's launch from the home city (CVG_UnlockUFO -> CVG2SPG).
                GameStage::Space => previous == Some(GameStage::Civilization) && !done("CVG2SPG"),
                _ => false,
            }
        });
        local.pending = if fresh { Some(stage.clone()) } else { None };
    }
    local.frames = local.frames.saturating_add(1);
    if !hooks || !loaded {
        local.brain = brain;
        return;
    }
    if world.get_resource::<crate::editor::EditorState>().is_some_and(|e| e.open) {
        return;
    }
    // Stage starts: wait for the stage's entities (a few frames after entry).
    if let Some(pending) = local.pending.clone() {
        // The stage's opening cinematic follows the launch screen.
        if world.get_resource::<crate::front_end::launch_screen::LaunchScreen>().is_some_and(|l| l.holds_game()) {
            local.frames = 0;
        }
        if local.frames >= 3 && has_player(world) {
            let ready = match pending {
                GameStage::Creature => {
                    let mut q = world.query_filtered::<(), With<crate::creature_home::HomeNest>>();
                    q.iter(world).next().is_some()
                }
                GameStage::Civilization => {
                    let mut q = world.query_filtered::<(), With<crate::civ::CityMarker>>();
                    q.iter(world).next().is_some()
                }
                _ => true,
            };
            if ready {
                if let Some((name, pause)) = start_cinematic(&pending) {
                    super::moments::mark_played(world, name);
                    play_with_bindings(world, name, pause, true);
                }
                local.pending = None;
            }
        }
        if local.frames > 600 {
            local.pending = None;
        }
    }
    // Brain level-up (Creature).
    if stage == GameStage::Creature {
        if let (Some(before), Some(now)) = (local.brain, brain) {
            for level in (before + 1)..=now {
                if let Some(name) = crate::creature_rules::brain_upgrade_cinematic(level) {
                    play_with_bindings(world, name, false, true);
                }
            }
        }
    }
    local.brain = brain;
}

/// `--cinematic <name> [--cinematic-every N] [--cinematic-frames M]` in a
/// quick capture: after the stage warm-up, play the cinematic with the
/// stage's bindings and write a frame every N updates (default 30) to
/// `<dir>\cin-<name>-<frame>.png` until it ends (at most M updates, default
/// 3000). Continue buttons are pressed after 1.5 s, `-forContinue` waits
/// continue after 3 s.
pub fn quick_capture(app: &mut App, args: &[String], output: &Path, target: &Handle<Image>) {
    let step = &mut |app: &mut App| {
        app.update();
        crate::runtime::gpu_flush(app);
    };
    // A plain screenshot (the stage capture's provenance files are not
    // written for every cinematic frame).
    let capture = &mut |app: &mut App, path: &Path| {
        use bevy::render::view::screenshot::{save_to_disk, Screenshot};
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        app.world_mut().spawn(Screenshot::image(target.clone())).observe(save_to_disk(path.to_path_buf()));
        for _ in 0..600 {
            app.update();
            crate::runtime::gpu_flush(app);
            if std::fs::metadata(path).is_ok_and(|m| m.len() > 1000) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        eprintln!("cinematic capture did not complete: {}", path.display());
    };
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let Some(name) = flag("--cinematic") else { return };
    let every: usize = flag("--cinematic-every").and_then(|v| v.parse().ok()).unwrap_or(30).max(1);
    let max: usize = flag("--cinematic-frames").and_then(|v| v.parse().ok()).unwrap_or(3000);
    // Scripts load on the first frames; make sure they are in.
    for _ in 0..3 {
        if app.world().get_resource::<Cinematics>().is_some_and(|c| c.loaded()) {
            break;
        }
        step(app);
    }
    {
        let world = app.world_mut();
        if let Some(mut c) = world.get_resource_mut::<Cinematics>() {
            c.runtime.enabled = true;
        }
        if play_with_bindings(world, &name, false, false).is_none() {
            eprintln!("--cinematic {name}: not played (unknown state or scripts not loaded)");
            return;
        }
    }
    let safe = name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_");
    let mut last_state = String::new();
    for frame in 0..=max {
        let (playing, state, elapsed) = {
            let world = app.world();
            let c = world.resource::<Cinematics>();
            let scene = world.resource::<super::host::Scene>();
            (c.is_playing(), c.current().map(|(_, _, s)| s.to_string()).unwrap_or_default(), scene.elapsed)
        };
        if state != last_state {
            println!("cinematic {name}: frame {frame} t={elapsed:.2}s state {state}");
            last_state = state.clone();
        }
        if frame % every == 0 || !playing {
            let path = output.join(format!("cin-{safe}-{frame:04}.png"));
            capture(app, &path);
            println!("captured {} ({state}) {}", path.display(), super::host::debug_ui(app.world()));
        }
        if !playing {
            println!("cinematic {name} ended after {frame} frames ({elapsed:.2}s)");
            break;
        }
        step(app);
    }
}
