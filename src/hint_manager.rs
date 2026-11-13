//! The retail hint manager, shared by the four stage guides
//! (`creature_hud::guide`, `tribal_hud::guide`, `civ_hud::guide`,
//! `space_hud::guide`).
//!
//! `UI::cHintManager` (loader 0x67B480, `Update` 0x67C410, `ShowHint` 0x67C7F0,
//! `HideHint` 0x67C880, the bar set-up 0x67BBF0, constructor defaults 0x67B110,
//! selection 0x67AF90) is a list of hint property files (group 0x0522DE06) and
//! one condition checker per game mode (one function, 0xE07E50, registered once
//! at game start through `AddChecker` 0x67CA10). The hints carry everything the
//! manager needs: three lists of condition names (A: all must hold, B: none
//! may hold, C: the hint ends when all hold), two delays, a display time, a
//! priority and a type; the guides only feed it the answers of the checker
//! (their own `Conditions` types, one per game mode) and call [`HintMachine::show`] /
//! [`HintMachine::complete`] at the places the retail code calls
//! `ShowHint` / `HideHint`.
//!
//! The states of a hint (`+0x90`, jump table 0x67C728): 1 waits the first delay and
//! asks the checker, 2 waits the second delay and asks again, 3 is a
//! candidate for the bar, 4 fades in, 5 is shown (for the display time, or
//! until list C holds), 6 / 7 fade out. One hint at a time owns the bar (the
//! manager's `+0x4C`): the best priority among the states 3..7.

use crate::{asset_catalog::ResourceIndex, cell_hud::dialogs::HintFade, dbpf::ResourceKey};

/// Property group of the hint files and their fields (`cHintManager` loader
/// 0x67B480): the condition lists A / B / C, the delays, the display time, the
/// fades, the priority, the type and the "always" flag.
pub const HINT_GROUP: u32 = 0x0522_DE06;
const PROP_ALL_OF: u32 = 0x0522_DE07;
const PROP_NONE_OF: u32 = 0x0522_DE08;
const PROP_UNTIL: u32 = 0x0522_DE09;
const PROP_FIRST_DELAY: u32 = 0x0522_DE0A;
const PROP_DISPLAY: u32 = 0x0522_DE0B;
const PROP_SECOND_DELAY: u32 = 0x0522_DE0C;
const PROP_PRIORITY: u32 = 0x0522_DE0D;
const PROP_KIND: u32 = 0x0523_4CE1;
const PROP_ALWAYS: u32 = 0x0604_72A7;
const PROP_FADE_IN: u32 = 0x07E8_708C;
const PROP_FADE_OUT: u32 = 0x07E8_7096;

/// Hint types (`+0x70`): a Triggered hint is shown by an explicit call, a
/// Normal one repeats, a Oneshot one stops once its condition C was met.
pub const KIND_TRIGGERED: u32 = 0x1F9E_21E8;
pub const KIND_NORMAL: u32 = 0x4527_C498;
#[allow(dead_code)]
pub const KIND_ONESHOT: u32 = 0x511A_890F;

/// Condition names the checker (0xE07E50) answers before it looks at the game
/// mode (VERIFIED, the head of the function, 0xE07E50..0xE07FA6) plus the
/// ones every stage's lists use. The ids are the lowercase FNV-1 hashes of the
/// names the hint files use (a test checks them).
#[allow(dead_code)]
pub mod cond {
    /// The game mode is Cell / Creature / Tribe / Civ / Space (0xB5B910 against
    /// 0x1654C00 / 01 / 02 / 04 / 05).
    pub const CLG: u32 = 0x3D97_A8EF;
    pub const CRG: u32 = 0x2B97_8C55;
    pub const TRG: u32 = 0x256C_A1DE;
    pub const CVG: u32 = 0x2797_8581;
    pub const SPG: u32 = 0x2970_79FB;
    /// Constant false in the Creature, Tribe and Civ modes (0xE07FB7, 0xE08678,
    /// 0xE081C2 -> 0xE08B8B); in the Space mode a field of the Space game
    /// (0xE08BC4 -> 0xD096A0).
    pub const USER_BUSY: u32 = 0x3481_F823;
    /// Constant false in every mode (0xE07F97, before any game-mode dispatch):
    /// the hints that list it can never leave state 1.
    pub const INACTIVE: u32 = 0xBC8E_7902;
    /// A planner window is open (Tribe 0xE0868C, Civ 0xE081D6).
    pub const IN_PLANNER: u32 = 0x48E1_8DE8;
}

/// One hint of the retail hint manager: the fields of its property file and
/// its state (`+0x90`: 0 idle, 1 waiting for the first delay, 2 for the second,
/// 3 candidate, 4 fading in, 5 shown, 6 / 7 fading out) and timer (`+0x94`).
#[derive(Clone, Debug)]
pub struct HintRule {
    pub id: String,
    /// List A (`+0x0C`): every condition must hold.
    pub all_of: Vec<u32>,
    /// List B (`+0x20`): none may hold.
    pub none_of: Vec<u32>,
    /// List C (`+0x34`): the hint ends when all hold.
    pub until: Vec<u32>,
    pub first_delay: f32,
    pub second_delay: f32,
    /// Seconds shown after the fade in; 0 = until C holds (or an explicit
    /// `HideHint`).
    pub display: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    pub priority: i32,
    pub kind: u32,
    /// `0x060472A7`: also shown while Game Hints is off.
    pub always: bool,
    /// The state and timer belong to the manager; `pub(crate)` only so the
    /// guides' tests can build rules with struct update syntax.
    pub(crate) state: u8,
    pub(crate) timer: f32,
}

impl HintRule {
    /// A hint with the constructor defaults of 0x67B110: Triggered, no
    /// conditions, an effectively endless first delay (the float 0x501502F9),
    /// 1 s second delay and fade in, 2 s fade out, shown until completed.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            all_of: Vec::new(),
            none_of: Vec::new(),
            until: Vec::new(),
            first_delay: f32::from_bits(0x5015_02F9),
            second_delay: 1.0,
            display: 0.0,
            fade_in: 1.0,
            fade_out: 2.0,
            priority: 0,
            kind: KIND_TRIGGERED,
            always: false,
            state: 0,
            timer: 0.0,
        }
    }

    fn triggered(&self) -> bool {
        self.kind == KIND_TRIGGERED
    }

    /// Read the hint's property file (`hints` group).
    pub fn read(index: &ResourceIndex, id: &str) -> Option<Self> {
        let key = ResourceKey {
            group_id: HINT_GROUP,
            instance_id: crate::world_objects::name_id(id),
            type_id: crate::asset_catalog::PROP,
        };
        let props = index.properties(key).ok()?;
        let keys = |prop: u32| -> Vec<u32> {
            props
                .0
                .get(&prop)
                .map(|p| {
                    p.values
                        .iter()
                        .filter_map(|v| v.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
                        .collect()
                })
                .unwrap_or_default()
        };
        let number = |prop: u32, default: f32| props.0.get(&prop).and_then(|p| p.number()).unwrap_or(default);
        let mut rule = Self::new(id);
        rule.all_of = keys(PROP_ALL_OF);
        rule.none_of = keys(PROP_NONE_OF);
        rule.until = keys(PROP_UNTIL);
        rule.first_delay = number(PROP_FIRST_DELAY, rule.first_delay);
        rule.second_delay = number(PROP_SECOND_DELAY, rule.second_delay);
        rule.display = number(PROP_DISPLAY, rule.display);
        rule.fade_in = number(PROP_FADE_IN, rule.fade_in);
        rule.fade_out = number(PROP_FADE_OUT, rule.fade_out);
        rule.priority = number(PROP_PRIORITY, 0.0) as i32;
        rule.kind = props.0.get(&PROP_KIND).and_then(|p| p.key()).map_or(KIND_TRIGGERED, |k| k.instance_id);
        rule.always = props.0.get(&PROP_ALWAYS).and_then(|p| p.number()).is_some_and(|v| v != 0.0);
        Some(rule)
    }

    /// 0x67AF90: A all true, none of B true, C not all true.
    fn may_show(&self, eval: &dyn Fn(u32) -> bool) -> bool {
        self.all_of.iter().all(|c| eval(*c))
            && (self.none_of.is_empty() || !self.none_of.iter().any(|c| eval(*c)))
            && (self.until.is_empty() || !self.until.iter().all(|c| eval(*c)))
    }

    fn until_met(&self, eval: &dyn Fn(u32) -> bool) -> bool {
        !self.until.is_empty() && self.until.iter().all(|c| eval(*c))
    }
}

/// Port of `cHintManager::Update` (0x67C410, VERIFIED): the hints are
/// processed in list order, one of them (the best priority of those that are a
/// candidate or shown) owns the hint bar. ADAPTED: the clock starts at 0, a
/// priority tie keeps the hint that is already shown, and the bar fades with
/// [`HintFade`] (the same 1 s / 2 s as the states 4 / 6 / 7, see [`HintBar`]).
#[derive(Clone, Debug)]
pub struct HintMachine {
    rules: Vec<HintRule>,
    /// The hint that owns the bar (`+0x4C`).
    current: Option<usize>,
    /// The manager's clock (`+0x34`) and when the last hint was promoted (`+0x38`).
    clock: f32,
    last_start: f32,
    /// How many times the bar was cleared at once (0x67BBF0 with `visible`
    /// false: `ShowHint` demoting the hint on the bar, `HideHint` on it).
    cuts: u32,
}

/// Seconds after one hint was promoted before a hint that is not Triggered may
/// be promoted (the float 0x41A00000 at 0x1481754).
const REPEAT_GAP: f32 = 20.0;

impl HintMachine {
    pub fn new(mut rules: Vec<HintRule>) -> Self {
        // The loader (0x67B930): a hint that is not Triggered starts waiting.
        for rule in &mut rules {
            rule.state = u8::from(!rule.triggered());
            rule.timer = 0.0;
        }
        Self { rules, current: None, clock: 0.0, last_start: f32::NEG_INFINITY, cuts: 0 }
    }

    /// The manager with the hints of `ids` that are installed.
    pub fn load(index: &ResourceIndex, ids: &[&str]) -> Self {
        Self::new(ids.iter().filter_map(|id| HintRule::read(index, id)).collect())
    }

    /// `cHintManager::ShowHint` (0x67C7F0): the hint becomes a candidate at
    /// once (no delays) and the hint that owns the bar is sent away (its state
    /// is 0 when it is Triggered, else 1) and the bar is cleared at once.
    pub fn show(&mut self, id: &str) {
        let Some(i) = self.rules.iter().position(|r| r.id == id) else {
            return;
        };
        if self.current == Some(i) {
            return;
        }
        if let Some(c) = self.current.take() {
            let rule = &mut self.rules[c];
            rule.state = u8::from(!rule.triggered());
            rule.timer = 0.0;
            self.cuts += 1;
        }
        self.rules[i].state = 3;
        self.rules[i].timer = 0.0;
    }

    /// `cHintManager::HideHint(id, done)` (0x67C880): ends the hint at once.
    /// On the bar it is cleared with no fade (0x67BBF0); the state becomes 0
    /// (idle for good) when `done`, or when the hint is Triggered and was on
    /// the bar, else 1 (waiting for its conditions again). A hint that is not
    /// on the bar only has its state reset.
    pub fn complete(&mut self, id: &str, done: bool) {
        let Some(i) = self.rules.iter().position(|r| r.id == id) else {
            return;
        };
        let on_bar = self.current == Some(i);
        let rule = &mut self.rules[i];
        let done = if on_bar && rule.triggered() { true } else { done };
        rule.state = u8::from(!done);
        rule.timer = 0.0;
        if on_bar {
            self.current = None;
            self.cuts += 1;
        }
    }

    /// The hint that is on the bar (fading in or shown).
    pub fn visible(&self) -> Option<&str> {
        self.current.filter(|i| matches!(self.rules[*i].state, 4 | 5)).map(|i| self.rules[i].id.as_str())
    }

    /// The manager's current hint (`+0x4C`), whatever its state: what the
    /// retail code compares against (`current->id == spg-fly`, ...).
    pub fn bar(&self) -> Option<&str> {
        self.current.map(|i| self.rules[i].id.as_str())
    }

    /// Whether `id` owns the bar.
    pub fn on_bar(&self, id: &str) -> bool {
        self.bar() == Some(id)
    }

    /// Times the bar was cleared at once; the binder's fade restarts when it changes.
    pub fn cuts(&self) -> u32 {
        self.cuts
    }

    /// The state of a hint (`+0x90`), for tests and diagnostics.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn state(&self, id: &str) -> u8 {
        self.rules.iter().find(|r| r.id == id).map_or(255, |r| r.state)
    }

    /// One update of `dt` seconds. `eval` answers a condition name; `hints_on`
    /// is the Game Hints setting (off: only `always` hints can be selected).
    pub fn update(&mut self, dt: f32, eval: &dyn Fn(u32) -> bool, hints_on: bool) {
        self.clock += dt;
        let mut best: Option<usize> = None;
        let mut best_priority = -1;
        for i in 0..self.rules.len() {
            let may_show = self.rules[i].may_show(eval);
            let until_met = self.rules[i].until_met(eval);
            let r = &mut self.rules[i];
            let mut selectable = false;
            match r.state {
                1 => {
                    r.timer += dt;
                    if r.timer > r.first_delay {
                        if may_show {
                            r.state = 2;
                        }
                        r.timer = 0.0;
                    }
                }
                2 => {
                    r.timer += dt;
                    if r.timer > r.second_delay {
                        r.state = if may_show { 3 } else { 1 };
                        r.timer = 0.0;
                    }
                }
                3 => {
                    if until_met {
                        // Done before it was shown.
                        r.state = u8::from(!r.triggered());
                        r.timer = 0.0;
                    } else {
                        selectable = true;
                    }
                }
                4 => {
                    r.timer += dt;
                    if r.timer > r.fade_in {
                        r.state = 5;
                        r.timer = 0.0;
                    }
                    selectable = true;
                }
                5 => {
                    r.timer += dt;
                    if r.timer > r.display && r.display != 0.0 {
                        r.state = 7;
                        r.timer = 0.0;
                    } else if until_met {
                        r.state = 6;
                        r.timer = 0.0;
                    }
                    selectable = true;
                }
                6 | 7 => {
                    r.timer += dt;
                    if r.timer > r.fade_out {
                        // 6 (its condition C was met): only a Normal hint
                        // comes back; 7 (the display time ran out): any hint
                        // that is not Triggered does.
                        let again = if r.state == 6 { r.kind == KIND_NORMAL } else { !r.triggered() };
                        r.state = u8::from(again);
                        r.timer = 0.0;
                        if self.current == Some(i) {
                            self.current = None;
                        }
                    } else {
                        selectable = true;
                    }
                }
                _ => {}
            }
            let r = &self.rules[i];
            if selectable && (hints_on || r.always) {
                let better = r.priority > best_priority
                    || (r.priority == best_priority && self.current == Some(i) && best != self.current);
                if better {
                    best_priority = r.priority;
                    best = Some(i);
                }
            }
        }
        let Some(b) = best else {
            return;
        };
        if !self.rules[b].triggered() && self.clock - REPEAT_GAP <= self.last_start {
            return;
        }
        if let Some(c) = self.current {
            if c != b {
                self.rules[c].state = 3;
                self.current = None;
            }
        }
        if self.rules[b].state == 3 {
            self.rules[b].state = 4;
            self.rules[b].timer = 0.0;
            self.current = Some(b);
            self.last_start = self.clock;
        }
    }
}

/// The hint bar's fade on the binder's side: the retail 1 s in / 2 s out
/// ([`HintFade`]) towards the published hint, restarted when the manager cleared
/// the bar at once ([`HintMachine::cuts`]).
#[derive(Clone, Debug, Default)]
pub struct HintBar {
    shown: Option<String>,
    fade: HintFade,
    cuts: u32,
}

impl HintBar {
    /// Advance by `dt`; `published` is the hint the guide wants on the bar.
    pub fn step(&mut self, published: Option<&str>, cuts: u32, dt: f32) {
        if cuts != self.cuts {
            // 0x67BBF0 cleared the bar: no fade out, the next hint starts at 0.
            self.cuts = cuts;
            self.shown = None;
            self.fade = HintFade::default();
        }
        match published {
            Some(id) => {
                if self.shown.as_deref() != Some(id) {
                    self.shown = Some(id.to_string());
                    self.fade = HintFade::default();
                }
                self.fade.step(true, dt);
            }
            None => {
                if self.fade.step(false, dt) <= 0.0 {
                    self.shown = None;
                }
            }
        }
    }

    /// The hint whose windows are lit (fading in, shown or fading out).
    pub fn shown(&self) -> Option<&str> {
        self.shown.as_deref()
    }

    pub fn alpha(&self) -> f32 {
        self.fade.alpha()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_condition_ids_are_the_hashes_of_their_names() {
        for (id, name) in [
            (cond::CLG, "clg"),
            (cond::CRG, "crg"),
            (cond::TRG, "trg"),
            (cond::CVG, "cvg"),
            (cond::SPG, "spg"),
            (cond::USER_BUSY, "UserBusy"),
            (cond::INACTIVE, "Inactive"),
            (cond::IN_PLANNER, "InPlanner"),
            (KIND_TRIGGERED, "Triggered"),
            (KIND_NORMAL, "Normal"),
            (KIND_ONESHOT, "Oneshot"),
        ] {
            assert_eq!(id, crate::world_objects::name_id(name), "{name}");
        }
    }

    fn triggered(id: &str, until: &[u32], priority: i32, display: f32) -> HintRule {
        HintRule { until: until.to_vec(), priority, display, ..HintRule::new(id) }
    }

    fn run(machine: &mut HintMachine, seconds: f32, eval: &dyn Fn(u32) -> bool) {
        for _ in 0..(seconds * 10.0).round() as u32 {
            machine.update(0.1, eval, true);
        }
    }

    #[test]
    fn hiding_the_hint_on_the_bar_clears_it_at_once_and_for_good() {
        // The chain hints (`spg-fly`, `cvg-camzoom`, ...) have no condition and no
        // display time: only `HideHint` (0x67C880) ends them, with no fade.
        let mut m = HintMachine::new(vec![triggered("a", &[], 1, 0.0), triggered("b", &[], 1, 0.0)]);
        m.show("a");
        run(&mut m, 0.2, &|_| false);
        assert_eq!((m.visible(), m.on_bar("a")), (Some("a"), true));
        run(&mut m, 30.0, &|_| false);
        assert_eq!(m.visible(), Some("a"), "it stays");
        let cuts = m.cuts();
        m.complete("a", true);
        assert_eq!((m.visible(), m.bar(), m.state("a")), (None, None, 0));
        assert_eq!(m.cuts(), cuts + 1, "the bar is cleared at once");
        // A hint that is not on the bar is only reset; the flag 0 of a Triggered
        // hint that is not on the bar sends it to state 1 (the retail quirk).
        m.complete("b", true);
        assert_eq!((m.state("b"), m.cuts()), (0, cuts + 1));
        m.complete("b", false);
        assert_eq!(m.state("b"), 1);
        // On the bar a Triggered hint always ends.
        m.show("b");
        run(&mut m, 0.2, &|_| false);
        m.complete("b", false);
        assert_eq!(m.state("b"), 0);
    }

    #[test]
    fn showing_another_hint_demotes_and_cuts_the_one_on_the_bar() {
        let mut m = HintMachine::new(vec![triggered("a", &[], 100, 0.0), triggered("b", &[], 100, 5.0)]);
        m.show("a");
        run(&mut m, 0.2, &|_| false);
        let cuts = m.cuts();
        m.show("b");
        assert_eq!((m.state("a"), m.bar()), (0, None), "0x67C7F0 sets a Triggered hint to idle");
        assert_eq!(m.cuts(), cuts + 1);
        run(&mut m, 0.1, &|_| false);
        assert_eq!(m.visible(), Some("b"));
        // 1 s in, 5 s shown, 2 s out; the interrupted hint does not come back.
        run(&mut m, 8.5, &|_| false);
        assert_eq!((m.visible(), m.state("b"), m.state("a")), (None, 0, 0));
    }

    #[test]
    fn the_bar_fades_one_second_in_two_out_and_restarts_after_a_cut() {
        let mut bar = HintBar::default();
        bar.step(Some("a"), 0, 0.0);
        bar.step(Some("a"), 0, 0.5);
        assert!((bar.alpha() - 0.5).abs() < 1e-4);
        bar.step(Some("a"), 0, 0.6);
        bar.step(Some("a"), 0, 0.1);
        assert_eq!(bar.alpha(), 1.0);
        // Withdrawn by the manager (a fade out): 2 s linear.
        bar.step(None, 0, 0.0);
        bar.step(None, 0, 1.0);
        assert!((bar.alpha() - 0.5).abs() < 1e-4 && bar.shown() == Some("a"));
        bar.step(None, 0, 1.1);
        assert_eq!((bar.alpha(), bar.shown()), (0.0, None));
        // A cut hides at once.
        bar.step(Some("a"), 0, 0.0);
        bar.step(Some("a"), 0, 0.5);
        assert!(bar.alpha() > 0.4);
        bar.step(None, 1, 0.0);
        assert_eq!((bar.alpha(), bar.shown()), (0.0, None));
        // The next hint of the same frame starts from nothing.
        bar.step(Some("b"), 2, 0.0);
        bar.step(Some("b"), 2, 0.25);
        assert!((bar.alpha() - 0.25).abs() < 1e-4 && bar.shown() == Some("b"));
    }
}
