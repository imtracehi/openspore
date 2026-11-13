//! Per-profile data of the galaxy screen: which stages a new game may start
//! at, and the statistics retail keeps beside them.
//!
//! VERIFIED (`notes\front-end-notes.md` section 7):
//! * `GGEUserData.dat` (Games save area, read 0xDE6390, written through a
//!   `.tmp` 0xDE4800) is a ClassSerializer of `sGlobalGGEUI` with six
//!   attributes (static table 0x15A38B8, built by 0x136DA30):
//!   `mLevelsToUnlockOnActivateOrShutdown` (+0x254, map), `mLastSavedGame`
//!   (+0xD0, string16), `mNumCaptainsBroughtToLevel10` (+0xE0),
//!   `mNumAdventuresQuickplayed` (+0x1F8), `mNumAdventuresCreated` (+0x1FC),
//!   `mGGEScenarioTerrainDB` (+0x3C).
//! * Stage unlocks are achievements: the stage ends send message 0x06579712
//!   with `ALG-UnlockCreature` 0x26B05664 (Cell end 0xE809A0),
//!   `ALG-UnlockTribe` 0x24947C07 (0xD3FCA0), `ALG-UnlockCiv` 0x2AB05C28
//!   (0xCDAC90), `ALG-UnlockSpace` 0x2887B6DE (0xBE8A20) (achievement props
//!   `05BEFD27!<id>`). The GGE handler (0xDE92AB) puts the id into
//!   `mLevelsToUnlockOnActivateOrShutdown` unless the `-unlock` cheat
//!   (GGE +0x1F5, "Unlocks all game levels") or the cheater flag is set; with
//!   `DelayUnlockAwardsUntilGGEReturn` (gge prop, 1) the award waits until the
//!   galaxy screen activates, where 0xDE5210 earns every pending id.
//! * The new-game stage buttons Creature / Tribal / Civ / Space (053F06EE /
//!   F5 / FC / 0707) are enabled when their achievement is earned or pending
//!   (0xDE5C60) or every level is unlocked (0xDF4780, tables 0x147E750 /
//!   0x147E760); Cell is always offered. The theme step waits for
//!   `ALG-UnlockCreature` (`HideThemeUntilCreatureGameUnlocked`, 0xDEEE40).
//!
//! * The achievements themselves are a separate ClassSerializer
//!   (`AchievementsSerializer`, strings at image 0x1401278 ..): `mAchievementSet`
//!   plus the cross-game data of the profile (`mCrossGameData.mCellGameIDs`,
//!   `mEatenCreatures`, `mTribeGamesCompleted`, `mEncounteredCreatures`) and the
//!   `-cheater` flag of the `achievement_test` cheat. Its file name is not a
//!   string in the image (the save-area path is built elsewhere), so the file
//!   was not located.
//!
//! OpenSpore keeps this as `<save stem>-profile.json` beside the saves, one
//! file for what retail splits between the achievement file and
//! `GGEUserData.dat`. ADAPTED (by design, the container only): the earned
//! stage-unlock achievements and the six `GGEUserData` attributes keep their
//! retail meaning, the binary files are not reproduced (nothing outside
//! OpenSpore reads them). A profile created beside existing saved games starts
//! from the stages those games have reached (ADAPTED, a migration rule retail
//! has no use for: there a saved game at stage N exists only because the
//! achievement for N was earned, so the two never disagree).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{game::RunProgress, stages::GameStage};

/// `ALG-UnlockCreature`, `ALG-UnlockTribe`, `ALG-UnlockCiv`,
/// `ALG-UnlockSpace`: the achievements that offer stages 1..4.
pub const UNLOCK_ACHIEVEMENTS: [u32; 4] = [0x26B0_5664, 0x2494_7C07, 0x2AB0_5C28, 0x2887_B6DE];

/// The profile file's contents.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profile {
    /// Earned unlock achievements.
    pub achievements: Vec<u32>,
    /// `mLevelsToUnlockOnActivateOrShutdown`: awards waiting for the galaxy
    /// screen.
    pub levels_to_unlock: Vec<u32>,
    /// `mLastSavedGame`: file name of the game saved last.
    pub last_saved_game: String,
    /// `mNumCaptainsBroughtToLevel10`, `mNumAdventuresQuickplayed`,
    /// `mNumAdventuresCreated` (OpenSpore has no adventures; kept for the
    /// file's shape).
    pub captains_brought_to_level_10: u32,
    pub adventures_quickplayed: u32,
    pub adventures_created: u32,
    /// The `-unlock` cheat: every level is offered (`--unlock-all`).
    pub unlock_all: bool,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Profile {
    pub fn path_for(save: &Path) -> PathBuf {
        let stem = save.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
        save.with_file_name(format!("{stem}-profile.json"))
    }

    /// Load the profile beside `save`; a missing profile starts from the
    /// stages the saved games there have reached (ADAPTED, see the module notes).
    pub fn load_beside(save: &Path, slots: &[super::slots::SlotInfo]) -> Self {
        let path = Self::path_for(save);
        let mut profile = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Profile>(&bytes).unwrap_or_default(),
            Err(_) => {
                let mut p = Profile::default();
                let reached = super::slots::unlocked_stages(slots);
                for (i, id) in UNLOCK_ACHIEVEMENTS.iter().enumerate() {
                    if reached[i + 1] {
                        p.achievements.push(*id);
                    }
                }
                p
            }
        };
        profile.path = Some(path);
        if std::env::args().any(|a| a == "--unlock-all") {
            profile.unlock_all = true;
        }
        profile
    }

    pub fn store(&self) {
        let Some(path) = &self.path else { return };
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(path, bytes) {
                    warn!("front end: cannot write {}: {error}", path.display());
                }
            }
            Err(error) => warn!("front end: profile: {error}"),
        }
    }

    /// 0xDE5C60: earned, or waiting for the galaxy screen.
    pub fn unlocked(&self, achievement: u32) -> bool {
        self.achievements.contains(&achievement) || self.levels_to_unlock.contains(&achievement)
    }

    /// Stages a new game may start at (0xDF4780): Cell always.
    pub fn offered(&self) -> [bool; 5] {
        std::array::from_fn(|i| i == 0 || self.unlock_all || self.unlocked(UNLOCK_ACHIEVEMENTS[i - 1]))
    }

    /// Message 0x06579712: an unlock award (pending until the galaxy screen,
    /// `DelayUnlockAwardsUntilGGEReturn`). Returns whether it changed.
    pub fn award(&mut self, achievement: u32) -> bool {
        if self.unlock_all || self.unlocked(achievement) {
            return false;
        }
        self.levels_to_unlock.push(achievement);
        true
    }

    /// 0xDE5210 (galaxy screen activates): earn the pending awards.
    pub fn activate(&mut self) -> bool {
        if self.levels_to_unlock.is_empty() {
            return false;
        }
        for id in std::mem::take(&mut self.levels_to_unlock) {
            if !self.achievements.contains(&id) {
                self.achievements.push(id);
            }
        }
        true
    }
}

/// The award a stage change grants: reaching stage N from stage N-1 ends
/// stage N-1 (0xE809A0 / 0xD3FCA0 / 0xCDAC90 / 0xBE8A20).
pub fn award_for(from: &GameStage, to: &GameStage) -> Option<u32> {
    let order = RunProgress::stage_order();
    let a = order.iter().position(|s| s == from)?;
    let b = order.iter().position(|s| s == to)?;
    (b == a + 1 && b >= 1).then(|| UNLOCK_ACHIEVEMENTS[b - 1])
}

/// Track the profile: load it with the slots, award stage unlocks when a game
/// moves on to the next stage, earn them when the galaxy screen shows, keep
/// the last saved game.
pub fn track(
    mut profile: ResMut<Profile>,
    fe: Res<super::FrontEnd>,
    active: Res<super::FrontEndActive>,
    stage: Res<State<GameStage>>,
    save: Res<crate::persistence::SaveState>,
    mut last: Local<Option<GameStage>>,
    mut was_active: Local<bool>,
) {
    if profile.path.is_none() {
        let Some(base) = fe.base.as_deref() else { return };
        let existed = Profile::path_for(base).exists();
        let slots = super::slots::scan(base);
        *profile = Profile::load_beside(base, &slots);
        info!("front end: profile {:?}: stages offered {:?}", profile.path, profile.offered());
        // A new profile keeps what it started from.
        if !existed && !cfg!(test) {
            profile.store();
        }
    }
    let now = stage.get().clone();
    let mut dirty = false;
    if let Some(prev) = last.replace(now.clone()) {
        if prev != now {
            if let Some(id) = award_for(&prev, &now) {
                dirty |= profile.award(id);
                info!("front end: stage unlock award {id:08x} ({prev:?} -> {now:?})");
            }
        }
    }
    if active.0 && !*was_active {
        dirty |= profile.activate();
    }
    *was_active = active.0;
    if now != GameStage::MainMenu && save.enabled {
        let name = save.path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if profile.last_saved_game != name {
            profile.last_saved_game = name;
            dirty = true;
        }
    }
    // Unit-test apps keep the profile in memory.
    if dirty && !cfg!(test) {
        profile.store();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_ends_award_pending_unlocks_that_the_galaxy_earns() {
        let mut p = Profile::default();
        assert_eq!(p.offered(), [true, false, false, false, false]);
        assert_eq!(award_for(&GameStage::Cell, &GameStage::Creature), Some(0x26B0_5664));
        assert_eq!(award_for(&GameStage::Tribal, &GameStage::Civilization), Some(0x2AB0_5C28));
        assert_eq!(award_for(&GameStage::MainMenu, &GameStage::Tribal), None, "loading a game awards nothing");
        assert_eq!(award_for(&GameStage::Creature, &GameStage::Creature), None);
        assert!(p.award(UNLOCK_ACHIEVEMENTS[0]));
        assert!(!p.award(UNLOCK_ACHIEVEMENTS[0]));
        // Pending awards already offer the stage (0xDE5C60).
        assert_eq!(p.offered(), [true, true, false, false, false]);
        assert!(p.activate());
        assert_eq!((p.achievements.len(), p.levels_to_unlock.len()), (1, 0));
        p.unlock_all = true;
        assert_eq!(p.offered(), [true; 5]);
    }

    #[test]
    fn a_new_profile_starts_from_the_saves_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("openspore-fe-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = dir.join("qc_save.json");
        let mut run = RunProgress::default();
        run.reset_to(GameStage::Tribal);
        crate::persistence::save_to(&save, &run).unwrap();
        let slots = super::super::slots::scan(&save);
        let mut p = Profile::load_beside(&save, &slots);
        assert_eq!(p.offered(), [true, true, true, false, false]);
        p.last_saved_game = "qc_save.json".into();
        p.store();
        // Deleting the save keeps the unlocks.
        std::fs::remove_file(&save).unwrap();
        let q = Profile::load_beside(&save, &[]);
        assert_eq!(q.offered(), [true, true, true, false, false]);
        assert_eq!(q.last_saved_game, "qc_save.json");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
