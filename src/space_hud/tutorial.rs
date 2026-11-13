//! The Space tutorial manager: what the retail `SpaceGameUI` (the Space HUD
//! object, `UI::SpaceGameUI`, 0x6AC bytes) does with callouts and the forced
//! hint chain. Pure rules; `guide.rs` feeds them the game's events and shows
//! the result. Read from the march2017 code image (`notes\space-flow-notes.md`
//! has the evidence); VERIFIED = read in code, INFERRED = my reading,
//! ADAPTED = OpenSpore choice.
//!
//! * `SpaceGameUI::AddCallout(id)` 0x106DD00 pushes a callout key onto the
//!   queue at +0x5EC; the HUD update 0x10743D0 shows the queue as ONE batch
//!   (game paused, token 0x04BF38A7) whenever the comm screen, the My
//!   Collections window, a modal window and the editors are all closed. The
//!   last callout of a batch carries the "resume and advance the tutorial"
//!   listener (vtable 0x149C1CC -> 0x10687B0) when the batch holds `FirstGoals`
//!   or `MyCollection`, a plain resume listener (0x149C1E8) otherwise. The
//!   callout manager itself (0x67A550) drops every callout when the Tutorials
//!   option 0x04EA96CB is off.
//! * The forced hint chain is the state byte at +0x618 stepped by `Advance`
//!   (0x1065D30); `0x1065E30` says a hint of the chain is on screen.
//! * "Once" flags are profile flags (keys {id, 0, 0xAA9A8ED7}: test 0xC77460,
//!   set 0xC77D90). ADAPTED: here they are bits of the campaign
//!   (`TutorialState::ui_flags`), so they hold for a run, not a profile.

/// Callout names (`Callout_SPG_*`, property group 0x3629F036).
pub const OPENING: [&str; 3] = ["Callout_SPG_Opening", "Callout_SPG_ProgressBar", "Callout_SPG_CommButton"];
pub const FIRST_GOALS: &str = "Callout_SPG_FirstGoals";
/// The tool-bin callouts of Flight 101's steps (0xC55BD0, the step's class:
/// cMissionScan 0x0347092D -> `MainToolsBin`, cMissionFetch 0x02AFD284 ->
/// `CargoBin`, cMissionEradicate 0x02F99994 -> `WeaponsBin`, cMissionExplore
/// 0x032A12F1 -> `FirstGoals`; the class names are read from the vtables).
pub const MAIN_TOOLS_BIN: &str = "Callout_SPG_MainToolsBin";
pub const CARGO_BIN: &str = "Callout_SPG_CargoBin";
pub const WEAPONS_BIN: &str = "Callout_SPG_WeaponsBin";
pub const MY_COLLECTION: &str = "Callout_SPG_MyCollection";
pub const ACTIVE_TOOL: &str = "Callout_SPG_ActiveTool";
pub const HEALTH_ENERGY: &str = "Callout_SPG_HealthEnergy";
pub const TOOL_UI: &str = "Callout_SPG_ToolUI";
pub const BADGE_UI: &str = "Callout_SPG_BadgeUI";
pub const STAR_MAPS: [&str; 2] = ["Callout_SPG_StarMap1", "Callout_SPG_StarMap2"];
pub const LOW_ENERGY: &str = "Callout_SPG_LowEnergy";
pub const PLAYER_DAMAGE: &str = "Callout_SPG_PlayerDamage";
pub const PURCHASE_READY: &str = "Callout_SPG_PurchaseReady";
pub const CAPTURE_PLANET: &str = "Callout_SPG_CapturePlanet";

/// The hints of the chain (`HintsSPG`).
pub const HINT_FLY: &str = "spg-fly";
pub const HINT_ALTITUDE: &str = "spg-altitude";
pub const HINT_ZOOM_SOLAR: &str = "spg-zoomsolar";
pub const HINT_ZOOM_GALAXY: &str = "spg-zoomgalaxy";

/// "Once" bits (`TutorialState::ui_flags`); the retail profile flag of each in
/// the comment.
pub mod flag {
    /// 0x065FEE65: the arrival callouts after `CVG2SPG` (0x100C622).
    pub const INTRO: u32 = 1 << 0;
    /// 0x065FEF10: the first tool selected (0x1079448).
    pub const ACTIVE_TOOL: u32 = 1 << 1;
    /// 0x06788F1C: the first tool with an energy cost selected (0x1079492).
    pub const HEALTH_ENERGY: u32 = 1 << 2;
    /// 0x065E611D: the Tools page of My Collections opened (0x107BBEB).
    pub const TOOL_UI: u32 = 1 << 3;
    /// 0x065E5039: the Badges page of My Collections opened (0x1064645).
    pub const BADGE_UI: u32 = 1 << 4;
    /// 0x0664B306: the star map callouts (0xFDAC14).
    pub const STAR_MAP: u32 = 1 << 5;
    /// 0x066269E7: energy at or under 20 % (`ChangeEnergy` 0xC38B50).
    pub const LOW_ENERGY: u32 = 1 << 6;
    /// 0x06627799: health at or under 20 % after Story 201 (0xC3DB75).
    pub const PLAYER_DAMAGE: u32 = 1 << 7;
    /// 0x066270E0: a star can be bought (0xFDAD9A).
    pub const PURCHASE_READY: u32 = 1 << 8;
    /// 0x06627823: the first capture (0xC8D43D).
    pub const CAPTURE_PLANET: u32 = 1 << 9;
    /// ADAPTED: the `MyCollection` callout of `SPG_FlightSchool101Complete`
    /// (0x100B730) was queued (retail queues it each time the cinematic ends,
    /// which is once).
    pub const MY_COLLECTION: u32 = 1 << 10;
    /// ADAPTED: Story 201's completion called `Advance` (0xC60423).
    pub const STORY_201: u32 = 1 << 11;
    /// ADAPTED: the tool-bin callout of Flight 101's Scan / Fetch / Eradicate
    /// step was queued (retail queues it when the step starts, 0xC55BD0).
    pub const STEP_SCAN: u32 = 1 << 12;
    pub const STEP_FETCH: u32 = 1 << 13;
    pub const STEP_ERADICATE: u32 = 1 << 14;
    /// `HasEverOpenedComm` (0x056D1872): the comm screen has been opened
    /// (0xAECDDB). ADAPTED: per campaign.
    pub const COMM_OPENED: u32 = 1 << 15;
    /// `UserHasMoved` (0x05C2D1AB): the UFO was told to fly (0xFFBEE0).
    pub const MOVED: u32 = 1 << 16;
    /// `SolarTravelled` (0x056E2B88): the UFO flew in the solar system view.
    pub const SOLAR_TRAVELLED: u32 = 1 << 17;
    /// `HasTransitionedSolarToPlanet` (0x0574AC67).
    pub const SOLAR_TO_PLANET: u32 = 1 << 18;
}

/// The energy and health share at or under which the warning callouts fire
/// (`0.2 x` the maximum, 0x146BB00: 0xC38BFE, 0xC37420).
pub const WARNING_FRACTION: f32 = 0.2;
/// The zoom input the altitude hint needs: |input| > 0.1 (0x101B69E, VERIFIED;
/// the check inside the planet camera's zoom 0x101B520 that completes the
/// hint 0x1930F01E and advances the chain 0x1065D30).
pub const ALTITUDE_INPUT: f32 = 0.1;
/// ADAPTED: how far the UFO has to fly before the flight hint counts as done
/// (retail: the move command, `cSimulatorPlayerUFO` 0xFFC200 -> 0xFFBEE0).
pub const FLY_DONE: f32 = 1.0;

/// The chain finished (or Tutorials is off).
pub const DONE: u8 = 8;

/// What the state-4 handler of the HUD update (0x1074B50) wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionsAction {
    None,
    /// Open My Collections on its Badges page (0xE18DB0(-0xC, 0): page
    /// 0xFFFFFFF4).
    OpenBadges,
    /// The player has seen the Badges page and closed the window.
    Advance,
}

/// The four hints of the chain: `Advance` with Tutorials off hides them all
/// (0x1065E53, 0x1065E70, 0x1065E83, 0x1065E96).
pub const CHAIN_HINTS: [&str; 4] = [HINT_FLY, HINT_ALTITUDE, HINT_ZOOM_SOLAR, HINT_ZOOM_GALAXY];

/// The forced hint chain, `SpaceGameUI +0x618`.
///
/// | state | `Advance` (0x1065D30) does | how it is advanced |
/// | --- | --- | --- |
/// | 0 | show `spg-fly`, state 1 | the batch holding `FirstGoals` ends |
/// | 1 | show `spg-altitude`, state 2 | the player flies (0xFFBEE0) |
/// | 2 | state 3 | the vertical input passes 0.1 (0x101B520) |
/// | 3 | state 4 | the batch holding `MyCollection` ends (after `SPG_FlightSchool101Complete`) |
/// | 4 | show `spg-zoomsolar`, state 5 | My Collections seen (0x1074B50) |
/// | 5 | state 6 | the solar system view opens (0xFDE3D0) |
/// | 6 | show `spg-zoomgalaxy`, state 7 | Story 201 is finished (0xC60423) |
/// | 7 | state 8 | the galaxy view opens (0xFDA5D0) |
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chain {
    pub state: u8,
}

impl Chain {
    pub fn new(state: u8) -> Self {
        Self { state: state.min(DONE) }
    }

    pub fn is_done(self) -> bool {
        self.state >= DONE
    }

    /// The hint `Advance` (0x1065D30) shows when it leaves `state`: the calls
    /// `ShowHint(spg-fly)` at 0x1065D65 (state 0), `spg-altitude` at 0x1065D82
    /// (state 1), `spg-zoomsolar` at 0x1065DB7 (state 4) and `spg-zoomgalaxy` at
    /// 0x1065DE0 (state 6).
    pub fn shows_on_leaving(state: u8) -> Option<&'static str> {
        match state {
            0 => Some(HINT_FLY),
            1 => Some(HINT_ALTITUDE),
            4 => Some(HINT_ZOOM_SOLAR),
            6 => Some(HINT_ZOOM_GALAXY),
            _ => None,
        }
    }

    /// The hint of the chain on screen (0x1065E30: states 1, 2, 5, 7).
    pub fn hint(self) -> Option<&'static str> {
        match self.state {
            1 => Some(HINT_FLY),
            2 => Some(HINT_ALTITUDE),
            5 => Some(HINT_ZOOM_SOLAR),
            7 => Some(HINT_ZOOM_GALAXY),
            _ => None,
        }
    }

    /// `SpaceGameUI::Advance` (0x1065D30). With Tutorials off the chain jumps
    /// to its end (0x1065DFC).
    pub fn advance(&mut self, tutorials: bool) {
        if !tutorials {
            self.state = DONE;
            return;
        }
        if self.state < DONE {
            self.state += 1;
        }
    }

    /// The UFO was told to fly (0xFFBEE0): done while the flight hint shows.
    pub fn on_flown(&mut self, tutorials: bool) {
        if self.hint() == Some(HINT_FLY) {
            self.advance(tutorials);
        }
    }

    /// The vertical input passed its dead zone (0x101B6A0).
    pub fn on_altitude(&mut self, tutorials: bool) {
        if self.hint() == Some(HINT_ALTITUDE) {
            self.advance(tutorials);
        }
    }

    /// The solar system view opened (0xFDE9EA).
    pub fn on_solar_view(&mut self, tutorials: bool) {
        if self.hint() == Some(HINT_ZOOM_SOLAR) {
            self.advance(tutorials);
        }
    }

    /// The galaxy view opened (0xFDA6E0).
    pub fn on_galaxy_view(&mut self, tutorials: bool) {
        if self.hint() == Some(HINT_ZOOM_GALAXY) {
            self.advance(tutorials);
        }
    }

    /// Story 201 finished: `Advance` is called whatever the state (0xC60423).
    pub fn on_story_201_done(&mut self, tutorials: bool) {
        self.advance(tutorials);
    }

    /// A batch of callouts was dismissed: the advance listener only exists
    /// when the batch held `FirstGoals` or `MyCollection` (0x1074A3C, 0x1074AF4).
    pub fn on_batch_end(&mut self, batch: &[String], tutorials: bool) {
        if batch_advances(batch) {
            self.advance(tutorials);
        }
    }

    /// The state-4 handler of the HUD update (0x1074B50): the player is sent
    /// to the Badges page of My Collections until its callout has been shown
    /// (flag `BADGE_UI`), then the chain goes on once the window is closed.
    pub fn collections(self, badges_seen: bool, window_open: bool) -> CollectionsAction {
        if self.state != 4 {
            return CollectionsAction::None;
        }
        match (window_open, badges_seen) {
            (true, true) => CollectionsAction::None,
            (_, false) => CollectionsAction::OpenBadges,
            (false, true) => CollectionsAction::Advance,
        }
    }
}

/// True when a batch of callouts ends with the advance listener.
pub fn batch_advances(batch: &[String]) -> bool {
    batch.iter().any(|n| n == FIRST_GOALS || n == MY_COLLECTION)
}

/// The tool-bin callout a Flight 101 step shows when it starts (0xC55BD0) and
/// the bit that keeps it to once.
pub fn step_callout(step: crate::space_rules::TutorialStep) -> Option<(u32, &'static str)> {
    use crate::space_rules::TutorialStep as S;
    match step {
        S::Flight101Scan => Some((flag::STEP_SCAN, MAIN_TOOLS_BIN)),
        S::Flight101FetchAnimal => Some((flag::STEP_FETCH, CARGO_BIN)),
        S::Flight101EradicateAnimals => Some((flag::STEP_ERADICATE, WEAPONS_BIN)),
        _ => None,
    }
}

/// Story 201 is finished (`cMissionManager::mbMission201Finished`, +0x18).
pub fn mission_201_finished(step: crate::space_rules::TutorialStep) -> bool {
    use crate::space_rules::TutorialStep as S;
    matches!(step, S::TeachColonize | S::TeachTerraform | S::TeachTradeSpice | S::Done)
}

/// Flight 101 is finished (`mbFlightSchoolFinished`, +0x17).
pub fn flight_school_finished(step: crate::space_rules::TutorialStep) -> bool {
    use crate::space_rules::TutorialStep as S;
    !matches!(step, S::Flight101VisitCities | S::Flight101Scan | S::Flight101FetchAnimal | S::Flight101EradicateAnimals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_rules::TutorialStep;

    #[test]
    fn the_chain_follows_the_retail_table() {
        let mut c = Chain::default();
        assert_eq!(c.hint(), None, "nothing shows before the first batch ends");
        let batch = vec![OPENING[0].to_string(), FIRST_GOALS.to_string()];
        c.on_batch_end(&batch, true);
        assert_eq!(c.hint(), Some(HINT_FLY));
        // An event of another step does nothing.
        c.on_altitude(true);
        assert_eq!(c.state, 1);
        c.on_flown(true);
        assert_eq!(c.hint(), Some(HINT_ALTITUDE));
        c.on_altitude(true);
        assert_eq!((c.state, c.hint()), (3, None));
        // A batch without the markers has the plain listener.
        c.on_batch_end(&[STAR_MAPS[0].to_string()], true);
        assert_eq!(c.state, 3);
        c.on_batch_end(&[MY_COLLECTION.to_string()], true);
        assert_eq!(c.state, 4);
        // State 4 opens My Collections until the Badges page was shown.
        assert_eq!(c.collections(false, false), CollectionsAction::OpenBadges);
        assert_eq!(c.collections(false, true), CollectionsAction::OpenBadges);
        assert_eq!(c.collections(true, true), CollectionsAction::None);
        assert_eq!(c.collections(true, false), CollectionsAction::Advance);
        c.advance(true);
        assert_eq!(c.hint(), Some(HINT_ZOOM_SOLAR));
        c.on_galaxy_view(true);
        assert_eq!(c.state, 5, "the galaxy view does not finish the solar hint");
        c.on_solar_view(true);
        assert_eq!((c.state, c.hint()), (6, None));
        c.on_story_201_done(true);
        assert_eq!(c.hint(), Some(HINT_ZOOM_GALAXY));
        c.on_galaxy_view(true);
        assert!(c.is_done());
        assert_eq!(c.collections(false, false), CollectionsAction::None);
        c.advance(true);
        assert_eq!(c.state, DONE, "the chain stops at its end");
    }

    #[test]
    fn story_201_advances_whatever_the_state() {
        let mut c = Chain::new(3);
        c.on_story_201_done(true);
        assert_eq!(c.state, 4);
    }

    #[test]
    fn tutorials_off_ends_the_chain_at_once() {
        let mut c = Chain::new(1);
        c.on_flown(false);
        assert!(c.is_done());
        assert_eq!(c.hint(), None);
        let mut c = Chain::new(0);
        c.on_batch_end(&[FIRST_GOALS.to_string()], false);
        assert!(c.is_done());
    }

    #[test]
    fn the_tutorial_milestones_follow_the_mission_chain() {
        assert!(!flight_school_finished(TutorialStep::Flight101EradicateAnimals));
        assert!(flight_school_finished(TutorialStep::Story201));
        assert!(!mission_201_finished(TutorialStep::Story201));
        assert!(mission_201_finished(TutorialStep::TeachColonize));
        assert!(mission_201_finished(TutorialStep::Done));
        assert_eq!(step_callout(TutorialStep::Flight101Scan).map(|c| c.1), Some(MAIN_TOOLS_BIN));
        assert_eq!(step_callout(TutorialStep::Flight101FetchAnimal).map(|c| c.1), Some(CARGO_BIN));
        assert_eq!(step_callout(TutorialStep::Flight101EradicateAnimals).map(|c| c.1), Some(WEAPONS_BIN));
        assert_eq!(step_callout(TutorialStep::Flight101VisitCities), None);
    }

    #[test]
    fn the_flags_are_distinct_bits() {
        let all = [
            flag::INTRO,
            flag::ACTIVE_TOOL,
            flag::HEALTH_ENERGY,
            flag::TOOL_UI,
            flag::BADGE_UI,
            flag::STAR_MAP,
            flag::LOW_ENERGY,
            flag::PLAYER_DAMAGE,
            flag::PURCHASE_READY,
            flag::CAPTURE_PLANET,
            flag::MY_COLLECTION,
            flag::STORY_201,
            flag::STEP_SCAN,
            flag::STEP_FETCH,
            flag::STEP_ERADICATE,
        ];
        let mut seen = 0u32;
        for f in all {
            assert_eq!(f.count_ones(), 1);
            assert_eq!(seen & f, 0);
            seen |= f;
        }
    }
}
