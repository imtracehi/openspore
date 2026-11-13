//! The Civ goal manager: which `CivTutorial_*` goal the player sees when, and
//! what completes it. Pure rules (`civ_hud::guide` feeds them the game's events
//! and shows the cards); read from the march2017 code image (`notes\civ-goals-notes.md`
//! has the evidence). VERIFIED = read in code, ADAPTED = OpenSpore choice.
//!
//! * The tutorial manager (singleton 0x169D2C8) keeps **profile flags** (key
//!   {id, 0, 0xAA9A8ED7}; test 0xCF7990, set 0xCF7950). `0xCFAAF0(id, advance)`
//!   is the one completion routine: unless flag `id` is already set it adds and
//!   completes the goal named `id` (0xB707A0 / 0xB708A0), sets the flag and, with
//!   `advance`, re-enters the chain. So a completion counts **whether or not the
//!   goal was ever shown** (a flag set early keeps its goal from ever appearing).
//! * The chain 0xCF9D00 (steps 6..12; steps 1..5 are the forced camera hints
//!   and the vehicle cinematic, `civ_hud::guide`) only runs with the Tutorials
//!   option on. Every step is `if flag unset: add the goal; return`, so the goals
//!   arrive one per call and only after the one before has been completed:
//!   `CaptureNode` -> `ProfitableCity` -> `HappyCity` -> `SeaUnit` (when 0xBF03A0(1)
//!   > 0: tech level 2 and a coastal city) -> `AirUnit` (0xBF03A0(2) > 0: tech level 3)
//!   -> the capture goal of the capital's type (0xBF1080: 0 military, 1 economic,
//!   2 religious; only once `slots - hidden > 1`) -> `CaptureAll`. A sea or air
//!   goal that is not available yet is skipped, not waited for; the chain is
//!   re-entered when it is (tech level change 0xBEF8F4). The step of an available
//!   goal returns even when the goal is already on the list, so an incomplete sea
//!   goal holds back the air goal, the capture goal and `CaptureAll` until a boat
//!   is bought. Goals added once stay on the list until completed, in the order
//!   they were added.
//! * Chain callers: the level setup 0xB9BDB1, a city changing hands 0xBE8DAF, the
//!   player's vehicle tech level changing 0xBEF8F4, the step completions, the end
//!   of `CVG_SendVehicleToCommodity` 0xCFDA84 and the exit of the City Planner
//!   0xCF59E9. A rival city merely appearing is **not** a caller, so the capture
//!   goal waits for the next call after it appeared.
//! * Completions: `CaptureNode` when a node's owner becomes the player's
//!   political id while the player has a city (0xBFF410, 0xBFF61F; `advance`);
//!   `ProfitableCity` / `HappyCity` at the exit of the City Planner (message
//!   0xB2699149, 0xCF5923..0xCF59E9) when the planned city is the player's and
//!   its income `cCity::GetIncome` 0xBD8CD0 is at least 400 (0x190, one factory
//!   linked) / its mood `0xBD7E50` is at least 60 (0x13EBC3C); `SeaUnit` /
//!   `AirUnit` when a vehicle of that locomotion is bought (0xD13FFE / 0xD13FC1,
//!   0xE2B25C / 0xE2B3DE, no `advance`); the capture goals in the owner change
//!   0xBE8A20 when the new owner is the player, by the **method** stored at
//!   `cCity` +0x29C (0 military = 0xBE9AD0's rule, 1 religious, 2 economic;
//!   0xBE8D5A / 0xBE8D15 / 0xBE8CCC); `CaptureAll` when the player's vehicle tech
//!   level becomes 4 (0xBEF8C0 -> 0xBEF914, every city taken).
//! * None of the nine goal props has a `goalValue`, nothing ever updates a
//!   Civ goal's progress, so no Civ goal card carries a count.
//!
//! ADAPTED: the flags are per campaign (retail: per profile); a campaign saved
//! before the chain existed gets its flags derived from what it has done
//! ([`GoalChain::catch_up`]); the node / vehicle / capture events are read off
//! the campaign's counters (the retail calls sit in the node, vehicle and city
//! code).

use serde::{Deserialize, Serialize};

use crate::civ::Approach;

/// A goal of the `CivTutorial_*` props (group 0x5267DE50, type 0x00B1B104).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goal {
    CaptureNode,
    ProfitableCity,
    HappyCity,
    SeaUnit,
    AirUnit,
    MilitaryCapture,
    EconomicCapture,
    ReligionCapture,
    CaptureAll,
}

impl Goal {
    /// The property instance id (also the profile flag the completion sets).
    pub const fn id(self) -> u32 {
        match self {
            Goal::CaptureNode => 0x308F_4CE8,
            Goal::ProfitableCity => 0x3464_6619,
            Goal::HappyCity => 0xBB73_68ED,
            Goal::SeaUnit => 0x6D38_14ED,
            Goal::AirUnit => 0xCED8_B9D4,
            Goal::MilitaryCapture => 0xE9B7_7B0B,
            Goal::EconomicCapture => 0x92A2_3F21,
            Goal::ReligionCapture => 0x9A2C_AC7F,
            Goal::CaptureAll => 0x3120_767D,
        }
    }

    /// The registered name of the property file.
    pub const fn name(self) -> &'static str {
        match self {
            Goal::CaptureNode => "CivTutorial_CaptureNode",
            Goal::ProfitableCity => "CivTutorial_ProfitableCity",
            Goal::HappyCity => "CivTutorial_HappyCity",
            Goal::SeaUnit => "CivTutorial_SeaUnit",
            Goal::AirUnit => "CivTutorial_AirUnit",
            Goal::MilitaryCapture => "CivTutorial_MilitaryCapture",
            Goal::EconomicCapture => "CivTutorial_EconomicCapture",
            Goal::ReligionCapture => "CivTutorial_ReligionCapture",
            Goal::CaptureAll => "CivTutorial_CaptureAll",
        }
    }

    /// Completions that re-enter the chain (`advance` of 0xCFAAF0): the node,
    /// the two city goals, the capture goals and the planet goal; not the
    /// vehicle goals (0xD13FFE / 0xE2B25C push 0).
    pub const fn advances(self) -> bool {
        !matches!(self, Goal::SeaUnit | Goal::AirUnit)
    }

    /// The capture goal of a capital type (0xCFA16A: 0xBF1080 = 0 military,
    /// 1 economic, 2 religious).
    pub fn capture_of(capital: Approach) -> Goal {
        match capital {
            Approach::Military => Goal::MilitaryCapture,
            Approach::Economic => Goal::EconomicCapture,
            Approach::Religious => Goal::ReligionCapture,
        }
    }

    /// The goal a city taken by `method` completes (`cCity` +0x29C: 0 military,
    /// 1 religious, 2 economic; 0xBE8C84 .. 0xBE8D5A).
    pub fn capture_by(method: Approach) -> Goal {
        match method {
            Approach::Military => Goal::MilitaryCapture,
            Approach::Religious => Goal::ReligionCapture,
            Approach::Economic => Goal::EconomicCapture,
        }
    }
}

/// Profile flags the chain keeps besides the goal ids (0xCF9D00).
mod flag {
    /// The goals callout was shown (0x06302C60, step 6).
    pub const CLAIM_CALLOUT: u32 = 0x0630_2C60;
    /// The first city was highlighted (0x0654DA12, step 7, 0xBDC5F0).
    pub const CITY_HIGHLIGHT: u32 = 0x0654_DA12;
    /// The minimap spice callout was shown (0x065509C9, step 7).
    pub const SPICE_CALLOUT: u32 = 0x0655_09C9;
}

/// Income at which `ProfitableCity` completes: `cCity::GetIncome` >= 0x190
/// (0xCF5974), one factory linked to a house or the hall (400 each).
pub const PROFIT_INCOME: i32 = 400;
/// Mood at which `HappyCity` completes (0xCF599B, the float at 0x13EBC3C).
pub const HAPPY_MOOD: i32 = 60;

/// What the chain needs to know when it is entered.
#[derive(Clone, Copy, Debug)]
pub struct ChainInputs {
    /// 0xBF03A0(1) > 0: tech level 2 and a coastal city of the player's.
    pub sea_available: bool,
    /// 0xBF03A0(2) > 0: tech level 3.
    pub air_available: bool,
    /// `slots - hidden` (0xB26910 +0xAC / +0xB0): the cities that have appeared.
    pub appeared: usize,
    /// The player's civilization type (0xBF1080).
    pub capital: Approach,
}

/// What entering the chain did, for the callouts the retail steps open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChainStep {
    /// The goal that was newly added.
    pub added: Option<Goal>,
    /// Step 6: pause and show the `GoalClaimNode` callout (once).
    pub claim_callout: bool,
    /// Step 7: highlight the player's first city (once, effect 0xC66A87F2 by 0xBDC5F0).
    pub highlight_city: bool,
    /// Step 7: pause and show the `MiniMapSpice` callout (once).
    pub spice_callout: bool,
}

/// The campaign's counters the events are read from (see the module note).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub derricks: u32,
    pub sea: u32,
    pub air: u32,
    pub military: u32,
    pub religious: u32,
    pub economic: u32,
    pub tech: u8,
}

/// What the game looks like now, for [`GoalChain::observe`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Observed {
    pub seen: Seen,
    /// The player owns a city (0xB25C10 returns one).
    pub has_city: bool,
}

/// The goal manager plus the tutorial flags (persisted with the campaign).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalChain {
    /// Profile flags set so far (goal ids and the once-flags).
    flags: Vec<u32>,
    /// Goals added and not completed, in the order they were added.
    active: Vec<Goal>,
    /// The forced steps and the vehicle cinematic are behind us: the chain may
    /// run (steps 1..5 of 0xCF9D00 are flags of the guide).
    chain_open: bool,
    /// Counters of the last observation; `None` before the first.
    seen: Option<Seen>,
    /// A completion asked to re-enter the chain (`advance`).
    advance_pending: bool,
}

impl GoalChain {
    /// 0xCF7990.
    pub fn flag(&self, id: u32) -> bool {
        self.flags.contains(&id)
    }

    fn set(&mut self, id: u32) {
        if !self.flags.contains(&id) {
            self.flags.push(id);
        }
    }

    /// The goal has been completed (its flag is set).
    pub fn done(&self, goal: Goal) -> bool {
        self.flag(goal.id())
    }

    /// The goals on the list, first added first.
    pub fn active(&self) -> &[Goal] {
        &self.active
    }

    /// The chain is allowed to run.
    pub fn open(&self) -> bool {
        self.chain_open
    }

    /// Steps 1..5 are done (or never apply to this campaign): steps 6..12 may run.
    pub fn open_chain(&mut self) {
        if !self.chain_open {
            self.chain_open = true;
            self.advance_pending = true;
        }
    }

    /// The chain waits to be entered (a completion asked for it, or it was just
    /// opened).
    pub fn wants_advance(&self) -> bool {
        self.chain_open && self.advance_pending
    }

    /// Ask for the chain to be entered at the next opportunity (a chain caller:
    /// the planner exit, a city changing hands, the tech level changing).
    pub fn request_advance(&mut self) {
        self.advance_pending = true;
    }

    /// 0xB707A0: add the goal unless it is on the list or already completed
    /// (a completed goal never comes back: the step tests its flag first).
    fn add(&mut self, goal: Goal) -> Option<Goal> {
        if self.done(goal) || self.active.contains(&goal) {
            return None;
        }
        self.active.push(goal);
        Some(goal)
    }

    /// 0xCFAAF0 for a goal id: unless the flag is set, add and complete the
    /// goal (a goal that was never shown flashes through the list) and set the
    /// flag; with `advance` the chain is entered again.
    pub fn complete(&mut self, goal: Goal) {
        if self.done(goal) {
            return;
        }
        self.active.retain(|g| *g != goal);
        self.set(goal.id());
        if goal.advances() {
            self.advance_pending = true;
        }
    }

    /// The chain 0xCF9D00 from step 6 on, one call. Returns what it opened.
    /// The caller checks the Tutorials option (0xCF9D3A) and [`Self::open`].
    pub fn advance(&mut self, i: &ChainInputs) -> ChainStep {
        self.advance_pending = false;
        let mut step = ChainStep::default();
        // 6: the first goal and, once, its callout.
        if !self.done(Goal::CaptureNode) {
            step.added = self.add(Goal::CaptureNode);
            if !self.flag(flag::CLAIM_CALLOUT) {
                self.set(flag::CLAIM_CALLOUT);
                step.claim_callout = true;
            }
            return step;
        }
        // 7: the profit goal, once the city highlight and once the spice callout.
        if !self.done(Goal::ProfitableCity) {
            step.added = self.add(Goal::ProfitableCity);
            if !self.flag(flag::CITY_HIGHLIGHT) {
                self.set(flag::CITY_HIGHLIGHT);
                step.highlight_city = true;
            }
            if !self.flag(flag::SPICE_CALLOUT) {
                self.set(flag::SPICE_CALLOUT);
                step.spice_callout = true;
            }
            return step;
        }
        // 8: the mood goal.
        if !self.done(Goal::HappyCity) {
            step.added = self.add(Goal::HappyCity);
            return step;
        }
        // 9 / 10: the vehicle goals, skipped while not available.
        if !self.done(Goal::SeaUnit) && i.sea_available {
            step.added = self.add(Goal::SeaUnit);
            return step;
        }
        if !self.done(Goal::AirUnit) && i.air_available {
            step.added = self.add(Goal::AirUnit);
            return step;
        }
        // 11: the capture goal of the capital's type, once a second city has
        // appeared; 12: the planet goal after it.
        let capture = Goal::capture_of(i.capital);
        if !self.done(capture) {
            if i.appeared > 1 {
                step.added = self.add(capture);
            }
            return step;
        }
        if !self.done(Goal::CaptureAll) {
            step.added = self.add(Goal::CaptureAll);
        }
        step
    }

    /// The City Planner was left (message 0xB2699149, 0xCF5923): for a planned
    /// city of the player's, income >= 400 completes `ProfitableCity` and mood
    /// >= 60 completes `HappyCity`; the chain is entered afterwards (0xCF59E9).
    pub fn planner_exit(&mut self, player_city: bool, income: i32, mood: i32) {
        if player_city {
            if income >= PROFIT_INCOME {
                self.complete(Goal::ProfitableCity);
            }
            if mood >= HAPPY_MOOD {
                self.complete(Goal::HappyCity);
            }
        }
        self.advance_pending = true;
    }

    /// Read the campaign's counters and complete what happened since the last
    /// call. The first call only records the baseline (and, for a campaign
    /// that is already under way, derives the flags, see [`Self::catch_up`]).
    pub fn observe(&mut self, now: &Observed, under_way: Option<&CatchUp>) {
        let Some(before) = self.seen else {
            self.seen = Some(now.seen);
            if let Some(progress) = under_way {
                self.catch_up(progress);
            }
            return;
        };
        let s = now.seen;
        // 0xBFF410: a node's owner becomes the player's while the player has a city.
        if s.derricks > before.derricks && now.has_city {
            self.complete(Goal::CaptureNode);
        }
        // 0xD13FFE / 0xE2B25C and 0xD13FC1 / 0xE2B3DE: a boat / an aircraft bought.
        if s.sea > before.sea {
            self.complete(Goal::SeaUnit);
        }
        if s.air > before.air {
            self.complete(Goal::AirUnit);
        }
        // 0xBE8C84..0xBE8D5A: a city taken by a method completes that capture goal.
        if s.military > before.military {
            self.complete(Goal::MilitaryCapture);
        }
        if s.religious > before.religious {
            self.complete(Goal::ReligionCapture);
        }
        if s.economic > before.economic {
            self.complete(Goal::EconomicCapture);
        }
        // 0xBE8DAF: a city changing hands re-enters the chain; so does the tech
        // level changing for the player (0xBEF8F4).
        if s.military + s.religious + s.economic > before.military + before.religious + before.economic || s.tech != before.tech {
            self.advance_pending = true;
        }
        // 0xBEF8C0: the vehicle tech level reaches 4 (every city taken).
        if s.tech >= 4 && before.tech < 4 {
            self.complete(Goal::CaptureAll);
        }
        self.seen = Some(s);
    }

    /// ADAPTED: a campaign saved before the chain existed (or loaded under way)
    /// gets the flags of what it has done, so the player is not sent back to
    /// the first goal.
    pub fn catch_up(&mut self, p: &CatchUp) {
        // Nobody starts with a derrick (VERIFIED, `notes\civ-rows3-notes.md` 2), so one derrick means a
        // node was claimed. (A save from before that change keeps its opening derrick and skips the goal.)
        if p.derricks >= 1 {
            self.set(Goal::CaptureNode.id());
        }
        if p.designed {
            self.set(flag::CLAIM_CALLOUT);
            self.set(flag::CITY_HIGHLIGHT);
            self.set(flag::SPICE_CALLOUT);
            self.set(Goal::ProfitableCity.id());
            if p.happy {
                self.set(Goal::HappyCity.id());
            }
        }
        if p.sea_vehicle {
            self.set(Goal::SeaUnit.id());
        }
        if p.air_vehicle {
            self.set(Goal::AirUnit.id());
        }
        if p.owned > 1 {
            self.set(Goal::capture_of(p.capital).id());
        }
        if p.tech >= 4 {
            self.set(Goal::CaptureAll.id());
        }
        self.chain_open = true;
        self.advance_pending = true;
    }
}

/// What a campaign that is already under way has done (see [`GoalChain::catch_up`]).
#[derive(Clone, Copy, Debug)]
pub struct CatchUp {
    pub derricks: usize,
    pub designed: bool,
    pub happy: bool,
    pub sea_vehicle: bool,
    pub air_vehicle: bool,
    pub owned: usize,
    pub capital: Approach,
    pub tech: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> ChainInputs {
        ChainInputs { sea_available: false, air_available: false, appeared: 1, capital: Approach::Military }
    }

    fn open() -> GoalChain {
        let mut chain = GoalChain::default();
        chain.open_chain();
        chain
    }

    #[test]
    fn the_ids_are_the_names_hashes() {
        for goal in [
            Goal::CaptureNode,
            Goal::ProfitableCity,
            Goal::HappyCity,
            Goal::SeaUnit,
            Goal::AirUnit,
            Goal::MilitaryCapture,
            Goal::EconomicCapture,
            Goal::ReligionCapture,
            Goal::CaptureAll,
        ] {
            assert_eq!(goal.id(), crate::world_objects::name_id(goal.name()), "{}", goal.name());
        }
    }

    #[test]
    fn the_chain_adds_one_goal_at_a_time_in_the_retail_order() {
        let mut chain = open();
        let first = chain.advance(&inputs());
        assert_eq!((first.added, first.claim_callout), (Some(Goal::CaptureNode), true));
        // Entering it again changes nothing and the callout comes once.
        let again = chain.advance(&inputs());
        assert_eq!((again.added, again.claim_callout), (None, false));
        assert_eq!(chain.active(), [Goal::CaptureNode]);
        // The node completes: the profit goal follows with the city highlight
        // and the spice callout (once).
        chain.complete(Goal::CaptureNode);
        assert!(chain.wants_advance());
        assert!(chain.active().is_empty());
        let step = chain.advance(&inputs());
        assert_eq!(step, ChainStep { added: Some(Goal::ProfitableCity), claim_callout: false, highlight_city: true, spice_callout: true });
        chain.complete(Goal::ProfitableCity);
        assert_eq!(chain.advance(&inputs()).added, Some(Goal::HappyCity));
        chain.complete(Goal::HappyCity);
        // Nothing is available: no second city has appeared, the chain stops at the capture goal.
        assert_eq!(chain.advance(&inputs()).added, None);
        assert!(chain.active().is_empty());
        // A second city has appeared: the next entry adds the capital type's goal.
        let appeared = ChainInputs { appeared: 2, ..inputs() };
        assert_eq!(chain.advance(&appeared).added, Some(Goal::MilitaryCapture));
        chain.complete(Goal::MilitaryCapture);
        assert_eq!(chain.advance(&appeared).added, Some(Goal::CaptureAll));
        chain.complete(Goal::CaptureAll);
        assert_eq!(chain.advance(&appeared).added, None);
    }

    #[test]
    fn a_goal_waits_for_the_one_before_it() {
        let mut chain = open();
        let late = ChainInputs { sea_available: true, air_available: true, appeared: 5, ..inputs() };
        for _ in 0..3 {
            chain.advance(&late);
        }
        assert_eq!(chain.active(), [Goal::CaptureNode], "nothing else before the node is claimed");
    }

    #[test]
    fn vehicle_goals_are_skipped_when_unavailable_and_added_when_they_open() {
        let mut chain = open();
        for goal in [Goal::CaptureNode, Goal::ProfitableCity, Goal::HappyCity] {
            chain.complete(goal);
        }
        let capital = ChainInputs { appeared: 3, ..inputs() };
        assert_eq!(chain.advance(&capital).added, Some(Goal::MilitaryCapture), "sea and air are skipped");
        // Tech level 2 with a coastal city: the chain is entered again (tech change).
        let sea = ChainInputs { sea_available: true, ..capital };
        assert_eq!(chain.advance(&sea).added, Some(Goal::SeaUnit));
        // The step returns even when its goal is already on the list: an available
        // sea goal holds back the air goal and everything after it.
        let both = ChainInputs { air_available: true, ..sea };
        assert_eq!(chain.advance(&both).added, None);
        assert_eq!(chain.active(), [Goal::MilitaryCapture, Goal::SeaUnit]);
        // Buying a boat completes its goal without re-entering the chain.
        chain.advance_pending = false;
        chain.complete(Goal::SeaUnit);
        assert!(!chain.wants_advance());
        assert_eq!(chain.active(), [Goal::MilitaryCapture]);
        // They stack in the order they were added.
        assert_eq!(chain.advance(&both).added, Some(Goal::AirUnit));
        assert_eq!(chain.active(), [Goal::MilitaryCapture, Goal::AirUnit]);
    }

    #[test]
    fn a_completion_counts_even_when_the_goal_was_never_shown() {
        let mut chain = open();
        chain.complete(Goal::SeaUnit);
        for goal in [Goal::CaptureNode, Goal::ProfitableCity, Goal::HappyCity] {
            chain.complete(goal);
        }
        let sea = ChainInputs { sea_available: true, appeared: 1, ..inputs() };
        assert_eq!(chain.advance(&sea).added, None, "the boat was bought first: no sea goal ever");
        assert!(chain.active().is_empty());
        assert!(chain.done(Goal::SeaUnit));
    }

    #[test]
    fn the_capture_goal_follows_the_capitals_type_and_completes_by_method() {
        for (capital, goal) in [
            (Approach::Military, Goal::MilitaryCapture),
            (Approach::Economic, Goal::EconomicCapture),
            (Approach::Religious, Goal::ReligionCapture),
        ] {
            assert_eq!(Goal::capture_of(capital), goal);
        }
        // A religious capital whose first captured city fell to the army does not
        // complete its own goal: the capture goal is by method (cCity +0x29C).
        let mut chain = open();
        for goal in [Goal::CaptureNode, Goal::ProfitableCity, Goal::HappyCity] {
            chain.complete(goal);
        }
        let capital = ChainInputs { appeared: 2, capital: Approach::Religious, ..inputs() };
        assert_eq!(chain.advance(&capital).added, Some(Goal::ReligionCapture));
        let mut seen = Observed::default();
        chain.observe(&seen, None);
        seen.seen.military = 1;
        chain.observe(&seen, None);
        assert_eq!(chain.active(), [Goal::ReligionCapture]);
        assert!(chain.done(Goal::MilitaryCapture));
        seen.seen.religious = 1;
        chain.observe(&seen, None);
        assert!(chain.active().is_empty() && chain.done(Goal::ReligionCapture));
        assert_eq!(Goal::capture_by(Approach::Economic), Goal::EconomicCapture);
    }

    #[test]
    fn the_city_planner_exit_judges_the_planned_city() {
        let mut chain = open();
        chain.advance(&inputs());
        chain.complete(Goal::CaptureNode);
        chain.advance(&inputs());
        // One factory is 400: profitable, not yet happy.
        chain.planner_exit(true, 400, 55);
        assert!(chain.done(Goal::ProfitableCity) && !chain.done(Goal::HappyCity));
        assert!(chain.wants_advance());
        assert_eq!(chain.advance(&inputs()).added, Some(Goal::HappyCity));
        // A rival city, or too little income, completes nothing.
        let mut poor = open();
        poor.planner_exit(false, 4000, 100);
        poor.planner_exit(true, 399, 59);
        assert!(!poor.done(Goal::ProfitableCity) && !poor.done(Goal::HappyCity));
        poor.planner_exit(true, 800, 60);
        assert!(poor.done(Goal::ProfitableCity) && poor.done(Goal::HappyCity), "both can complete on one exit");
    }

    #[test]
    fn counters_complete_the_node_the_vehicles_and_the_planet() {
        let mut chain = open();
        let mut now = Observed { has_city: true, ..default_observed() };
        now.seen.derricks = 1;
        chain.observe(&now, None);
        assert!(!chain.done(Goal::CaptureNode), "the first observation is the baseline");
        chain.advance(&inputs());
        now.seen.derricks = 2;
        chain.observe(&now, None);
        assert!(chain.done(Goal::CaptureNode));
        now.seen.air = 1;
        chain.observe(&now, None);
        assert!(chain.done(Goal::AirUnit));
        // The tech level changing re-enters the chain; level 4 completes the planet goal.
        chain.advance_pending = false;
        now.seen.tech = 2;
        chain.observe(&now, None);
        assert!(chain.wants_advance());
        now.seen.tech = 4;
        chain.observe(&now, None);
        assert!(chain.done(Goal::CaptureAll));
        // No city: a node does not count (0xB25C10 returns nothing).
        let mut orphan = open();
        orphan.observe(&default_observed(), None);
        let mut gained = default_observed();
        gained.seen.derricks = 1;
        orphan.observe(&gained, None);
        assert!(!orphan.done(Goal::CaptureNode));
    }

    fn default_observed() -> Observed {
        Observed::default()
    }

    #[test]
    fn a_campaign_under_way_skips_what_it_has_done() {
        let mut chain = GoalChain::default();
        let progress = CatchUp {
            derricks: 3,
            designed: true,
            happy: false,
            sea_vehicle: false,
            air_vehicle: false,
            owned: 2,
            capital: Approach::Economic,
            tech: 2,
        };
        chain.observe(&Observed::default(), Some(&progress));
        assert!(chain.open());
        let step = chain.advance(&ChainInputs { appeared: 4, capital: Approach::Economic, ..inputs() });
        assert_eq!(step.added, Some(Goal::HappyCity), "the mood goal is the next one");
        assert!(!step.spice_callout && !step.claim_callout);
        // A chain saved as JSON comes back the same.
        let json = serde_json::to_string(&chain).unwrap();
        assert_eq!(serde_json::from_str::<GoalChain>(&json).unwrap(), chain);
    }
}
