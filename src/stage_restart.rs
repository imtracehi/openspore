//! The game-over restarts of Tribal and Civ, the retail way: reload the last
//! save of the stage.
//!
//! VERIFIED (0xB27150, called by the Tribal handler 0xCD81F0 on "restart
//! tribe" 0x0420E32A after `TRG_PRE_PopDestroyed` / `TRG_PRE_HutDestroyed`,
//! and by the Civ handler 0xCFD500 at the end of `CVG_Lose`): the persistence
//! manager copies the current planet's save name and switches the game mode
//! manager to the loading mode, i.e. the planet is loaded again from its last
//! save.
//!
//! ADAPTED: OpenSpore saves continuously (every change and every 10 s), so the
//! save file always holds the lost game. The "last save" kept here is the
//! stage's autosave: a copy of the run written beside the save
//! (`<save>-stagestart.json`) when a stage is entered or a game is loaded into
//! it, the moments a retail player's game was last written. The restart loads
//! it with the normal persistence code (`persistence::load_from`) and enters
//! the stage again from it; without an autosave the callers keep their earlier
//! stand-in restart.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::game::RunProgress;
use crate::persistence::{load_from, save_to, SaveState};
use crate::stages::GameStage;

/// The stage autosave beside the save `path` (`save.json` ->
/// `save-stagestart.json`).
pub fn autosave_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "save".into());
    path.with_file_name(format!("{stem}-stagestart.json"))
}

/// The stages whose game over restarts from the autosave.
fn restartable(stage: &GameStage) -> bool {
    matches!(stage, GameStage::Tribal | GameStage::Civilization)
}

/// What the autosave was last written for (stage order index, stage).
#[derive(Default)]
pub struct Written {
    key: Option<(usize, GameStage)>,
    frames: u32,
}

/// Write the autosave once per entry into a restartable stage (a new stage
/// or a game loaded into it), a few frames in, when the stage has been set up.
pub fn autosave(stage: Res<State<GameStage>>, run: Res<RunProgress>, saves: Option<Res<SaveState>>, mut written: Local<Written>) {
    let stage = stage.get().clone();
    if !restartable(&stage) || run.current_stage() != stage {
        written.key = None;
        written.frames = 0;
        return;
    }
    let key = (run.stage_order_index, stage);
    if written.key.as_ref() == Some(&key) {
        return;
    }
    written.frames += 1;
    if written.frames < 3 {
        return;
    }
    written.key = Some(key);
    written.frames = 0;
    let Some(saves) = saves.filter(|s| s.enabled) else { return };
    let path = autosave_path(&saves.path);
    match save_to(&path, &run) {
        Ok(()) => info!("stage autosave: {}", path.display()),
        Err(e) => warn!("stage autosave {}: {e}", path.display()),
    }
}

/// Load the autosave of `stage` and enter the stage again from it (0xB27150).
/// `false` when there is no usable autosave (the caller restarts its own way).
pub fn reload(world: &mut World, stage: GameStage) -> bool {
    let Some(path) = world.get_resource::<SaveState>().filter(|s| s.enabled).map(|s| autosave_path(&s.path)) else {
        return false;
    };
    let Some(run) = load_stage(&path, &stage) else {
        return false;
    };
    info!("game over: {stage:?} reloads {}", path.display());
    world.insert_resource(run);
    if let Some(mut next) = world.get_resource_mut::<NextState<GameStage>>() {
        next.set(stage);
    }
    true
}

/// The autosave at `path` if it holds a run in `stage` (marked to be saved,
/// so the save file follows the restart).
pub fn load_stage(path: &Path, stage: &GameStage) -> Option<RunProgress> {
    let mut run = load_from(path).ok()?;
    if run.current_stage() != *stage {
        return None;
    }
    run.request_save();
    Some(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_restart_loads_the_stage_autosave_written_beside_the_save() {
        assert_eq!(autosave_path(Path::new("x/save.json")), Path::new("x/save-stagestart.json"));
        // Saves of this test stay in the copy's target folder.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("stage-restart-test");
        let _ = std::fs::create_dir_all(&dir);
        let save = dir.join(format!("save-{}.json", std::process::id()));
        let auto = autosave_path(&save);
        let mut start = RunProgress::default();
        start.reset_to(GameStage::Tribal);
        start.tribal_campaign.home_hut_damage = 0.0;
        start.tribe = vec![Default::default(); 3];
        save_to(&auto, &start).unwrap();
        // The lost game in the save file itself.
        let mut lost = start.clone();
        lost.tribe.clear();
        lost.tribal_campaign.home_hut_damage = 999.0;
        save_to(&save, &lost).unwrap();
        let back = load_stage(&auto, &GameStage::Tribal).expect("the autosave");
        assert_eq!(back.tribe.len(), 3);
        assert_eq!(back.tribal_campaign.home_hut_damage, 0.0);
        assert!(back.save_requested);
        // An autosave of another stage is not used.
        assert!(load_stage(&auto, &GameStage::Civilization).is_none());
        // The Bevy side: the run is replaced and the stage entered again.
        let mut world = World::new();
        let mut saves = SaveState::default();
        saves.path = save.clone();
        world.insert_resource(saves);
        world.insert_resource(lost);
        world.insert_resource(NextState::<GameStage>::default());
        assert!(reload(&mut world, GameStage::Tribal));
        assert_eq!(world.resource::<RunProgress>().tribe.len(), 3);
        assert!(matches!(world.resource::<NextState<GameStage>>(), NextState::Pending(GameStage::Tribal)));
        for f in [save.clone(), auto.clone(), crate::persistence::backup_path(&save), crate::persistence::backup_path(&auto)] {
            let _ = std::fs::remove_file(f);
        }
    }

    /// The whole loop in the game's schedule: entering Tribal writes the
    /// autosave beside the save (in this copy's target folder), the tribe is
    /// lost, the restart reloads the autosave, the stage is entered again and
    /// the save file follows.
    #[test]
    fn a_tribal_game_over_reloads_the_stage_autosave_in_the_game_schedule() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("stage-restart-test");
        let _ = std::fs::create_dir_all(&dir);
        let save = dir.join(format!("app-{}.json", std::process::id()));
        let auto = autosave_path(&save);
        let _ = std::fs::remove_file(&auto);
        let mut app = crate::runtime::headless_app(RunProgress::default());
        {
            let mut saves = app.world_mut().resource_mut::<SaveState>();
            saves.path = save.clone();
            saves.enabled = true;
        }
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Tribal);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Tribal);
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Tribal);
        let start = load_from(&auto).expect("the stage autosave is written on entry");
        assert_eq!(start.current_stage(), GameStage::Tribal);
        let members = start.tribe.len();
        assert!(members > 0);
        // The game is lost: the tribe gone, the hut razed.
        {
            let mut run = app.world_mut().resource_mut::<RunProgress>();
            run.tribe.clear();
            let health = run.tribal_campaign.home_hut_health();
            run.tribal_campaign.home_hut_damage = health;
            run.request_save();
        }
        app.update();
        assert!(load_from(&save).unwrap().tribe.is_empty(), "the save file holds the lost game");
        assert!(reload(app.world_mut(), GameStage::Tribal));
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(*app.world().resource::<State<GameStage>>().get(), GameStage::Tribal);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.tribe.len(), members);
        assert!(!run.tribal_campaign.home_hut_destroyed());
        assert_eq!(load_from(&save).unwrap().tribe.len(), members, "the save follows the restart");
        for f in [save.clone(), auto.clone(), crate::persistence::backup_path(&save), crate::persistence::backup_path(&auto)] {
            let _ = std::fs::remove_file(f);
        }
    }
}
