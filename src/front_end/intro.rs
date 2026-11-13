//! The galaxy screen's cinematics (`notes\front-end-notes.md` section 5):
//!
//! * `GGE_intro` when the galaxy screen opens (VERIFIED 0xDEA820: message
//!   0x49731AD plays it with pauseGame unless the intro was switched off;
//!   its end, 0x44F1189 with the intro's token, or the skip message 0x66F6B6F
//!   stops it and shows the galaxy UI). The cinematic player runs the script
//!   (GGEIntro.trigger: fade from white, a 7 s eased `pathCamera` flight from
//!   ~20000 galaxy units out to the resting camera, `gge_music_intro` for the
//!   menu music); "ref" is the galaxy centre. A click or Escape / Enter /
//!   Space skips it. The retail condition (VERIFIED, `front-end-notes.md` 7c:
//!   0x49731AD is posted only by the stop of the startup movie, 0xFD79C0; the
//!   movie's skip entries 0xFD75F0 / 0xFD7580 post 0x61D72DE first, which sets
//!   the intro state to 3 = no intro) is "once per run, after the startup
//!   movie ended by itself". OpenSpore has no startup movie: the first galaxy
//!   screen of a run stands in for the movie's end (ADAPTED), and the skip
//!   input skips the intro itself.
//! * Starting a game (VERIFIED 0xDEBFE0 / 0xDEB8E0): the galaxy UI hides,
//!   `GGE_zoom_modal` holds the letterbox while the camera zooms into the star
//!   (GGEtoCellZoomDuration 2 s); then a new Cell game switches to the Cell
//!   game and plays `GGE2CLG` (the comet `g2c_comet`), every other start or
//!   load goes straight into the game. The zoom mode (0xDDE8B0, called by
//!   0xDEBFE0) takes the duration and ends 50 galaxy units from the star
//!   (VERIFIED: it stores 50.0 at 0x15A3124). VERIFIED (fe-rows2, 0xDDE300 and
//!   0xDDF610, the mode's two virtual functions, vtable 0x147DA1C): the mode keeps
//!   the camera distance as one scalar, `d = end + (start - end) * (1 - t)` with
//!   `t = elapsed / duration`, linear, no ease (the mode ends and posts 0x4360E14
//!   when t passes 1); the camera transform of 0xDDF610 is the galaxy camera's
//!   own basis turned by its two angles (0x15A308C, 0x15A3090) and moved back
//!   along its axis by `d` from the stored focus point (0x15A30EC..F4, copied
//!   when the mode starts), `start` = the camera's distance then (0x15A3094).
//!   So the distance runs linearly into the star, OpenSpore's eye lerp with a
//!   linear `t`. INFERRED: the focus is the star and the view turns to it while
//!   the camera flies (the globals' writers, the galaxy camera's own
//!   star-centring pan on a click, are not traced; OpenSpore's resting camera
//!   does not centre on a clicked star). The comet starts
//!   at `GGE2CLG_2` because the
//!   stage change already plays `music_gge_cell_trans` (`game_audio::intro`)
//!   and OpenSpore's Cell start does not play `CLG_intro`, which resets the
//!   `incelltransition` mix the first state sets.

use bevy::{
    input::{keyboard::KeyboardInput, mouse::MouseButtonInput, ButtonState},
    prelude::*,
};

use super::{gge, new_game, slots, FrontEnd};
use crate::{
    cinematics::{CinematicTarget, Cinematics},
    game::RunProgress,
    menu_visuals::{self, GALAXY_CENTRE, GALAXY_SCALE, REST_LOOK},
    stages::GameStage,
};

pub const INTRO: &str = "GGE_intro";
pub const ZOOM_MODAL: &str = "GGE_zoom_modal";
/// The comet state of `GGE2CLG` (see the module notes).
pub const COMET: &str = "GGE2CLG_2";
/// `GGEtoCellZoomDuration` (gge tuning), seconds.
pub const ZOOM_DURATION: f32 = 2.0;
/// Distance of the camera from the star when the zoom ends, galaxy units
/// (0xDDE8B0 stores 50.0 in the zoom mode, 0x15A3124).
pub const ZOOM_END_DISTANCE: f32 = 50.0;

/// What a launch starts once the zoom has finished.
#[derive(Clone, Debug)]
pub enum LaunchKind {
    Load(usize),
    New(new_game::Draft),
}

/// A game being started from the galaxy screen.
#[derive(Clone, Debug)]
pub struct Launch {
    pub kind: LaunchKind,
    pub elapsed: f32,
    started: bool,
}

impl Launch {
    pub fn slot(&self) -> usize {
        match &self.kind {
            LaunchKind::Load(s) => *s,
            LaunchKind::New(d) => d.slot,
        }
    }
}

/// Start a game with the zoom transition.
pub fn begin(fe: &mut FrontEnd, kind: LaunchKind) {
    if fe.launch.is_none() {
        fe.launch = Some(Launch { kind, elapsed: 0.0, started: false });
        fe.selected = None;
    }
}

/// `GGE_intro` is playing.
pub fn intro_playing(cin: Option<&Cinematics>) -> bool {
    cin.and_then(|c| c.current()).is_some_and(|(_, name, _)| name == INTRO)
}

/// Whether the intro may play this run (not in automated runs or captures
/// unless `--front-end intro` asks for it).
#[derive(Resource, Clone, Copy, Debug)]
pub struct IntroPolicy {
    pub allowed: bool,
    pub played: bool,
}

pub fn policy_from_args(args: &[String]) -> IntroPolicy {
    let demo = args.iter().position(|a| a == "--front-end").and_then(|i| args.get(i + 1)).cloned();
    let capture = args.iter().any(|a| a == "--quick-capture" || a == "--smoke-test" || a == "--render-smoke");
    let allowed = match demo.as_deref() {
        Some("intro") => true,
        Some(_) => false,
        None => !capture && !args.iter().any(|a| a == "--no-intro"),
    };
    IntroPolicy { allowed, played: false }
}

/// Play `GGE_intro` when the galaxy screen first opens; skip it on a click
/// or key.
#[allow(clippy::too_many_arguments)]
pub fn intro(
    stage: Res<State<GameStage>>,
    editor: Res<crate::editor::EditorState>,
    originals: Res<crate::Originals>,
    mut policy: ResMut<IntroPolicy>,
    cin: Option<ResMut<Cinematics>>,
    mut cameras: Query<&mut Transform, With<crate::GameCamera>>,
    // Raw input messages (the cinematic input gate clears the button
    // states); optional for headless apps without the input plugin.
    buttons: Option<MessageReader<MouseButtonInput>>,
    keys: Option<MessageReader<KeyboardInput>>,
) {
    let Some(mut cin) = cin else { return };
    let clicked = buttons.is_some_and(|mut b| b.read().any(|b| b.state == ButtonState::Pressed));
    let pressed = keys.is_some_and(|mut k| {
        k.read().any(|k| k.state == ButtonState::Pressed && matches!(k.key_code, KeyCode::Escape | KeyCode::Enter | KeyCode::Space))
    });
    let skip = clicked || pressed;
    if intro_playing(Some(&cin)) {
        if skip {
            cin.stop();
        }
        return;
    }
    if !policy.allowed || policy.played || *stage.get() != GameStage::MainMenu || editor.open {
        return;
    }
    if !cin.loaded() || originals.resource_index.records.is_empty() {
        return;
    }
    policy.played = true;
    // The camera the intro returns to (`startCamera -backToStart`).
    if let Ok(mut camera) = cameras.single_mut() {
        *camera = menu_visuals::rest_camera();
    }
    if let Some(token) = cin.play(INTRO, true, false) {
        cin.bind_object(token, "ref", CinematicTarget::Point { position: GALAXY_CENTRE, yaw: 0.0 });
        info!("front end: GGE_intro");
    }
}

/// Run a launch: the zoom with `GGE_zoom_modal`, then the game; the comet
/// once the new Cell game is up.
#[allow(clippy::too_many_arguments)]
pub fn launch(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    mut fe: ResMut<FrontEnd>,
    cin: Option<ResMut<Cinematics>>,
    mut run: ResMut<RunProgress>,
    mut save: ResMut<crate::persistence::SaveState>,
    mut next: ResMut<NextState<GameStage>>,
    originals: Res<crate::Originals>,
    mut editor: ResMut<crate::editor::EditorState>,
) {
    let mut cin = cin;
    if fe.launch.is_none() {
        if fe.comet && *stage.get() == GameStage::Cell {
            fe.comet = false;
            if let Some(cin) = cin.as_mut() {
                if cin.play(COMET, true, true).is_some() {
                    info!("front end: GGE2CLG comet");
                }
            }
        } else if fe.comet && *stage.get() != GameStage::MainMenu {
            fe.comet = false;
        }
        return;
    }
    if *stage.get() != GameStage::MainMenu {
        fe.launch = None;
        return;
    }
    if fe.slots.is_empty() {
        if let Some(base) = fe.base.clone() {
            fe.slots = slots::scan(&base);
        }
    }
    let launch = fe.launch.as_mut().expect("launch");
    if !launch.started {
        launch.started = true;
        if let Some(cin) = cin.as_mut() {
            cin.play(ZOOM_MODAL, false, false);
        }
    }
    launch.elapsed += time.delta_secs();
    if launch.elapsed < ZOOM_DURATION {
        return;
    }
    let launch = fe.launch.take().expect("launch");
    if let Some(cin) = cin.as_mut() {
        if cin.current().is_some_and(|(_, name, _)| name == ZOOM_MODAL) {
            cin.stop();
        }
    }
    match launch.kind {
        LaunchKind::Load(slot) => gge::play_slot(&mut fe, slot, &mut run, &mut save, &mut next),
        LaunchKind::New(draft) => {
            let cell = draft.stage.unwrap_or(0) == 0;
            new_game::start(&mut fe, &draft, &mut run, &mut save, &originals, &mut editor, &mut next);
            fe.comet = cell;
        }
    }
}

/// The camera of a launch (PostUpdate, after the menu camera and the
/// cinematic player): from the resting camera into the star.
pub fn zoom_camera(
    fe: Res<FrontEnd>,
    galaxy: Query<&Transform, (With<menu_visuals::NativeMenuGalaxy>, Without<crate::GameCamera>)>,
    mut cameras: Query<&mut Transform, With<crate::GameCamera>>,
) {
    let Some(launch) = fe.launch.as_ref() else { return };
    let (Ok(disc), Ok(mut camera)) = (galaxy.single(), cameras.single_mut()) else { return };
    let star = star_world(disc, launch.slot());
    let rest = menu_visuals::rest_camera();
    let t = zoom_progress(launch.elapsed);
    let eye_end = star + (rest.translation - star).normalize_or(Vec3::Y) * ZOOM_END_DISTANCE * GALAXY_SCALE;
    let eye = rest.translation.lerp(eye_end, t);
    let look = (GALAXY_CENTRE + REST_LOOK).lerp(star, t);
    *camera = Transform::from_translation(eye).looking_at(look, Vec3::Y);
}

/// The zoom's progress `t = elapsed / duration` (0xDDE300: linear, no ease; the mode
/// ends when it passes 1).
pub fn zoom_progress(elapsed: f32) -> f32 {
    (elapsed / ZOOM_DURATION).clamp(0.0, 1.0)
}

/// World position of a slot's star on the menu galaxy disc.
pub fn star_world(disc: &Transform, slot: usize) -> Vec3 {
    let [gx, gy] = slots::star_position(slot);
    disc.transform_point(Vec3::new(gx / 1000.0 * 0.5, 0.0, gy / 1000.0 * 0.5))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_plays_in_real_runs_only() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(policy_from_args(&args(&["openspore"])).allowed);
        assert!(!policy_from_args(&args(&["openspore", "--smoke-test"])).allowed);
        assert!(!policy_from_args(&args(&["openspore", "--quick-capture", "x", "cell"])).allowed);
        assert!(policy_from_args(&args(&["openspore", "--quick-capture", "x", "cell", "--front-end", "intro"])).allowed);
        assert!(!policy_from_args(&args(&["openspore", "--front-end", "settings"])).allowed);
    }

    #[test]
    fn the_zoom_ends_fifty_galaxy_units_from_the_star() {
        assert_eq!(ZOOM_END_DISTANCE, 50.0);
        assert!((ZOOM_END_DISTANCE * GALAXY_SCALE - 0.55).abs() < 1.0e-4);
    }

    #[test]
    fn the_zoom_distance_is_linear_in_time() {
        // 0xDDE300: value = end + (start - end) * (1 - t), t = elapsed / duration.
        assert_eq!(zoom_progress(0.0), 0.0);
        assert_eq!(zoom_progress(ZOOM_DURATION * 0.5), 0.5);
        assert_eq!(zoom_progress(ZOOM_DURATION * 0.25), 0.25);
        assert_eq!(zoom_progress(ZOOM_DURATION * 3.0), 1.0);
        let (start, end) = (400.0f32, ZOOM_END_DISTANCE);
        let at = |t: f32| end + (start - end) * (1.0 - t);
        assert_eq!(at(0.0), start);
        assert_eq!(at(1.0), end);
        assert!((at(0.5) - (start + end) * 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_launch_keeps_its_star() {
        let mut fe = FrontEnd::default();
        begin(&mut fe, LaunchKind::Load(3));
        assert_eq!(fe.launch.as_ref().map(Launch::slot), Some(3));
        // A second start while zooming is ignored.
        begin(&mut fe, LaunchKind::New(new_game::Draft::new(5)));
        assert_eq!(fe.launch.as_ref().map(Launch::slot), Some(3));
    }
}
