//! The scroll / layout machinery of the retail History timeline (`cUITimeline`,
//! code `0xE36000..0xE49000`), shared by every stage's summary.
//!
//! Decoded from the march2017 code image (cell-timeline3; addresses are SDK
//! addresses; VERIFIED = read in the code, ADAPTED = OpenSpore's stand-in):
//!
//! * There is NO zoom. The "three levels" of the first reading are the three
//!   disposition bands (herbivore / omnivore / carnivore): `0xE42C83` compares
//!   the vertical marker's height with 1/3 and 2/3 of the view and tints the
//!   matching band window `F537D890..92` (see [`band_tint`]). `0xE4287C` is a
//!   piece of the per-frame update `0xE423A0` (scroll limits and the follow
//!   smoothing, [`rubber`] / [`follow`]), not a zoom switch.
//! * Events are laid out by `0xE3F970`: the running x starts at 5, every event
//!   (a card layout) is placed at x and the x advances by the card's width plus
//!   5 ([`layout_events`]); the events of a stage fill that stage's column,
//!   whose width becomes the final x (`0xE3EE30` sets the width of a column
//!   and moves the next one behind it; the five columns are zeroed by the show
//!   function `0xE44200`, so the stages without events stay empty).
//! * The content window is shifted by the (smoothed) scroll position; the
//!   scroll target is limited to `0 ..= content - view` and pulled back by
//!   three times the follow factor when it leaves that range (`0xE4296E`).
//!   `0xE42100` puts the scroll to the far right (`content - view`) when the
//!   window is shown.
//! * The intro sequencer (`0xE3B990`, state `+0x284` 1..8, timer `+0x288` in
//!   ms, [`Sequencer`]): 1 waits for the stage's timeline callout to be
//!   dismissed (`0xE36F80` turns state 1 into 2), 2 puts the scroll to the
//!   start (`max(0, end - view * CinematicPageOffset)`, end = right edge of the
//!   current stage's last event minus 1), 3 pauses `CinematicStatePauseTimeMS`,
//!   4 scrolls linearly to the end in `CinematicStateScrollTimeMS`, 5 pauses
//!   `CinematicStatePauseAfterScrollTimeMS`, then the earned trait's callout is
//!   shown (6, `CinematicStateShowEarnedTraitTimeMS`), after the Civ stage the
//!   Space trait follows (7, `CinematicStateShowSpaceTraitTimeMS`), 8 is done.
//!
//! Card heights (cell-timeline4): [`edge_factor`] (`0xE37D20`), [`card_top`] /
//! [`clamp_card_top`] / [`stack_tops`] (`0xE46D80`); the records and the merge rules
//! are in [`super::events`].
//!
//! cell-timeline5: the disposition line, its intro wipe and the vertical marker are in
//! [`super::line`]; the event load / unload and the card glide (`0xE423A0`, `0xE46880`) are
//! `timeline::load_tick` / `timeline::glide`. Not ported (listed in the CHANGES file): the date
//! read-out of the marker (`DateDisplayOnEventMouseover` is never read: retail shows none), the
//! mouse hit test of the line (`0xE37FD0`: only highlights a segment under the cursor).

/// Game variables of the timeline (static initialisers `0x1372844..0x13735C9`).
pub const PAUSE_MS: i32 = 1500;
pub const SCROLL_MS: i32 = 1500;
pub const PAUSE_AFTER_SCROLL_MS: i32 = 1500;
pub const SHOW_EARNED_TRAIT_MS: i32 = 1500;
pub const SHOW_SPACE_TRAIT_MS: i32 = 1500;
/// `CinematicPageOffset` (a fraction of the view width).
pub const PAGE_OFFSET: f32 = 1.0;
/// `LineIntroTimeMS` (the intro of the disposition line, not ported).
#[allow(dead_code)]
pub const LINE_INTRO_MS: i32 = 1000;
/// `TintAlpha`.
pub const TINT_ALPHA: f32 = 0.25;
/// The three band colours `0xE42C83` tints with (top, middle, bottom): `0xE461C0` reads them from
/// the card's palette, which `0xE47930` fills with the colour property `D4F639CD` of the three
/// trait files (`NiceTraitTemplate` 0.30 / 0.92 / 0.37, `MiddleTraitTemplate` 0.29 / 0.737 /
/// 0.851, `MeanTraitTemplate` 0.803 / 0.247 / 0.09; checked against the installed files by the
/// layout test). cell-timeline4 took `LineColorTop / Middle / Bottom` here: those game variables
/// are never read by the code.
pub const BAND_COLOURS: [[f32; 3]; 3] = [[0.30, 0.92, 0.37], [0.29, 0.737, 0.851], [0.803, 0.247, 0.09]];

/// First event x and the gap behind every event (`0x1481BEC` = 5.0, used by
/// `0xE3F970` and `0xE37AF0`).
pub const EVENT_START_X: f32 = 5.0;
pub const EVENT_GAP: f32 = 5.0;
/// Width of the layout space the timeline window covers (authored root 1280).
pub const VIEW_WIDTH: f32 = 1280.0;
/// Room kept at the right end of the content so that the last card clears the
/// trait bar. `0xE42100` (VERIFIED, cell-timeline4) sets the content width to
/// `max(view, right edge of the last event's column) - left + extra` with `extra`
/// = 0 whenever the last event carries a trait (`+0xF0`, the earned-trait event at
/// the end of every Cell run); in retail the trait bar is that last event's card
/// (`0x0EE4273F`, full column height, laid out like the others), so its own width
/// is already inside the column. ADAPTED: OpenSpore keeps the bar at the right
/// screen edge and adds its width (118) here instead.
pub const RIGHT_MARGIN: f32 = 118.0;
/// ADAPTED escape hatch: `true` keeps the columns of the stages after the shown
/// one visible at their authored 160 px (the look before the port). The retail
/// code hides all five columns (`0xE44200`) and shows only the columns that
/// hold events (`0xE3FBCE`), so the retail behaviour `false` is used; the
/// screen right of the shown column is backed by the opaque panel of
/// `timeline::backdrop_node` (retail: the blurred world, `modalUIBackgroundBlur`).
pub const SHOW_EMPTY_LATER_COLUMNS: bool = false;
/// Authored width of a stage column.
pub const AUTHORED_COLUMN: f32 = 160.0;
/// Per-millisecond factor of the follow smoothing (`0x13F6A50` = 0.008) and
/// the pull-back multiplier (`0x13EB950` = 3.0).
pub const FOLLOW_PER_MS: f32 = 0.008;
pub const PULL_BACK: f32 = 3.0;

/// Property ids of the stage keys the events carry (`+0xE4`): Cell, Creature,
/// Tribe, Civ, Space (`0xE36F00` maps them to 0..4).
#[allow(dead_code)]
pub const STAGE_IDS: [u32; 5] = [0xA426_730B, 0xAD56_080C, 0xF71F_A311, 0xBEB5_28CB, 0x2DB6_DAD3];

/// Stage index of a stage key (`0xE36F00`, 0 for unknown keys).
#[allow(dead_code)]
pub fn stage_index(id: u32) -> usize {
    STAGE_IDS.iter().position(|s| *s == id).unwrap_or(0)
}

/// `0xE3F970` / `0xE3FCF7`: x of every event card (relative to the left edge of
/// the event's stage column) and the final x, which is the column's width.
pub fn layout_events(widths: &[f32]) -> (Vec<f32>, f32) {
    let mut x = EVENT_START_X;
    let mut at = Vec::with_capacity(widths.len());
    for width in widths {
        at.push(x);
        x += width + EVENT_GAP;
    }
    (at, x)
}

/// The five stage columns: left edges and widths in content coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Columns {
    pub left: [f32; 5],
    pub width: [f32; 5],
}

impl Columns {
    pub fn right(&self, stage: usize) -> f32 {
        self.left[stage] + self.width[stage]
    }

    /// Width of the whole row of columns.
    pub fn content_width(&self) -> f32 {
        self.right(4)
    }

    /// A column is drawn when it has any width (`0xE44200` hides and zeroes
    /// them all, `0xE3EE30` only gives the stages with events a width).
    pub fn visible(&self, stage: usize) -> bool {
        self.width[stage] > 0.5
    }
}

/// Columns of the summary of `stage`: the stage's column is as wide as its
/// events (`extent`, the final x of [`layout_events`]), the columns of the
/// stages after it stay empty (zeroed by `0xE44200`), and the columns before
/// it are 5 wide (`0xE3EE30` gives the stages it skips the default 5.0).
/// ADAPTED: the earlier stages' own events are not recorded by OpenSpore, so
/// those columns are the 5 px default instead of holding their history.
pub fn stage_columns(stage: usize, extent: f32) -> Columns {
    let stage = stage.min(4);
    let mut width = [0.0f32; 5];
    for (i, w) in width.iter_mut().enumerate() {
        *w = match i.cmp(&stage) {
            std::cmp::Ordering::Less => EVENT_GAP,
            std::cmp::Ordering::Equal => extent.max(EVENT_GAP),
            std::cmp::Ordering::Greater => {
                if SHOW_EMPTY_LATER_COLUMNS {
                    AUTHORED_COLUMN
                } else {
                    0.0
                }
            }
        };
    }
    let mut left = [0.0f32; 5];
    for i in 1..5 {
        left[i] = left[i - 1] + width[i - 1];
    }
    Columns { left, width }
}

/// Largest scroll position: the content's width (plus [`RIGHT_MARGIN`]) beyond
/// the view (`0xE42100` puts the window there when it is shown).
pub fn max_scroll(content: f32, view: f32) -> f32 {
    max_scroll_margin(content, view, RIGHT_MARGIN)
}

/// [`max_scroll`] with the room kept behind the last event given: `0xE42100` adds nothing when
/// the last event carries a trait (the Cell's trait bar is that card).
pub fn max_scroll_margin(content: f32, view: f32, margin: f32) -> f32 {
    (content + margin - view).max(0.0)
}

/// The follow factor of one frame (`0xE42916`: milliseconds * 0.008, 0..1).
pub fn follow_factor(dt_ms: i32) -> f32 {
    (dt_ms.max(0) as f32 * FOLLOW_PER_MS).clamp(0.0, 1.0)
}

/// `0xE4296E` / `0xE429AF`: a scroll target outside `0 ..= max` is pulled back
/// by `factor * 3` of its distance each frame.
pub fn rubber(target: f32, max: f32, dt_ms: i32) -> f32 {
    let t = follow_factor(dt_ms);
    let mut x = target;
    if x < 0.0 {
        x += -x * t * PULL_BACK;
    }
    if x > max {
        x += (max - x) * t * PULL_BACK;
    }
    x
}

/// `0xE429C7`: the shown scroll position chases the target by `factor` per frame.
pub fn follow(shown: f32, target: f32, dt_ms: i32) -> f32 {
    shown + (target - shown) * follow_factor(dt_ms)
}

/// Target tint of band `band` (0 top, 1 middle, 2 bottom) when the vertical
/// marker sits in band `marker`: `TintAlpha` of the band's line colour for the
/// marker's band, fully transparent for the others (`0xE42C83`: the target
/// colours at `+0x1E8`, `+0x208`, `+0x228` chase the shown ones at `0x1F8`,
/// `0x218`, `0x238` by the follow factor).
pub fn band_target(band: usize, marker: usize) -> [f32; 4] {
    let c = BAND_COLOURS[band.min(2)];
    [c[0], c[1], c[2], if band == marker { TINT_ALPHA } else { 0.0 }]
}

/// The band (0..2) a marker at `fraction` of the view height is in (`0xE42ED8`
/// compares with 1/3 and 2/3). The Cell summary feeds it the vertical marker's
/// height ([`super::line`]); the Creature summary has no marker yet and passes the
/// earned slot (ADAPTED there).
pub fn marker_band(fraction: f32) -> usize {
    if fraction < 1.0 / 3.0 {
        0
    } else if fraction < 2.0 / 3.0 {
        1
    } else {
        2
    }
}

/// One step of a band's shown colour towards its target.
pub fn band_tint(shown: [f32; 4], target: [f32; 4], dt_ms: i32) -> [f32; 4] {
    let t = follow_factor(dt_ms);
    std::array::from_fn(|i| shown[i] + (target[i] - shown[i]) * t)
}

/// Packed ARGB of a float colour (`0x458590`).
pub fn pack_argb(c: [f32; 4]) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (b(c[3]) << 24) | (b(c[0]) << 16) | (b(c[1]) << 8) | b(c[2])
}

/// `ScreenEdgeBufferPercent` (`0x148547c`) and `ScreenEdgeExponent` (`0x145be38`),
/// the game variables of the card height law `0xE37D20`.
pub const SCREEN_EDGE_BUFFER: f32 = 0.75;
pub const SCREEN_EDGE_EXPONENT: f32 = 0.5;

/// `0xE37D20`: the card height law. The disposition value is clamped to -1..1;
/// inside +-1/3 it is the identity, outside the part beyond 1/3 is stretched by
/// 1.5, raised to `ScreenEdgeExponent`, scaled by `ScreenEdgeBufferPercent * 2/3`
/// and added to 1/3, so that +-1 ends at +-0.8333 of the half height.
pub fn edge_factor(value: f32) -> f32 {
    let v = value.clamp(-1.0, 1.0);
    let third = 1.0 / 3.0;
    let curve = |over: f32| (over * 1.5).powf(SCREEN_EDGE_EXPONENT) * SCREEN_EDGE_BUFFER * 0.666_666_6 + third;
    if v > third {
        curve(v - third)
    } else if v < -third {
        -curve(-v - third)
    } else {
        v
    }
}

/// `0xE460D0`: the height factor of an event card: the law above for every
/// stage but the Civ stage, whose factor is the value itself.
pub fn event_height_factor(value: f32, civ_stage: bool) -> f32 {
    if civ_stage {
        value
    } else {
        edge_factor(value)
    }
}

/// Top and height of the stage column window the cards live in (the `74ED2970..74`
/// windows of the `timeline` layout, areas (x, 164, x + 160, 851) /
/// (x, 163, x + 160, 850); `0xE46D80` reads the parent window's height).
pub const COLUMN_TOP: f32 = 163.5;
pub const COLUMN_HEIGHT: f32 = 687.0;

/// `0xE3F970` (`0xE3FCB4`) and `0xE37AF0` (`0xE37B32`): the y a card that is not on the line gets,
/// `(columnH - cardH) / 2` (the earned-trait bar: centred in the column).
pub fn centred_top(parent_height: f32, card_height: f32) -> f32 {
    (parent_height - card_height) * 0.5
}

/// `0xE46D80`: the top of a card centred at `(h / 2) * (1 + factor)` in a parent
/// of height `h`.
pub fn card_top(parent_height: f32, factor: f32, card_height: f32) -> f32 {
    parent_height * 0.5 * (1.0 + factor) - card_height * 0.5
}

/// `0xE46D80` (flag d = 0): the card's drawn extent (`y1`, `y2` of the union of
/// the layout's windows, relative to the card's top; `+0x7C` / `+0x84`) is kept
/// inside the parent: `-y1 <= top <= h - y2`.
pub fn clamp_card_top(top: f32, parent_height: f32, extent: (f32, f32)) -> f32 {
    top.max(-extent.0).min(parent_height - extent.1)
}

/// `0xE46D80`'s child loop (stacked cards, flag d = 1): `n` cards of `card_height`
/// centred on `top` (the single card's top), the whole stack moved back inside
/// `0 ..= parent_height` when it sticks out. Returns the top of every card,
/// first (top of the stack) to last.
pub fn stack_tops(top: f32, count: usize, card_height: f32, parent_height: f32) -> Vec<f32> {
    if count == 0 {
        return Vec::new();
    }
    let half = (count - 1) as f32 * 0.5;
    let first = top - half * card_height;
    let last_bottom = top + (count as f32 + 1.0) * 0.5 * card_height;
    let shift = if first < 0.0 {
        -first
    } else if last_bottom > parent_height {
        parent_height - last_bottom
    } else {
        0.0
    };
    (0..count).map(|i| (i as f32 - half) * card_height + top + shift).collect()
}

/// The sequencer states (`+0x284`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqState {
    /// 1: the stage's timeline callout is up; ends when it is dismissed.
    Callout = 1,
    /// 2: set the scroll start and target.
    Begin = 2,
    /// 3: `CinematicStatePauseTimeMS`.
    Pause = 3,
    /// 4: linear scroll.
    Scroll = 4,
    /// 5: `CinematicStatePauseAfterScrollTimeMS`.
    PauseAfter = 5,
    /// 6: the earned trait's callout is up.
    EarnedTrait = 6,
    /// 7: the Space trait's callout is up (after the Civ stage).
    SpaceTrait = 7,
    /// 8: done.
    Done = 8,
}

/// The last event of the timeline (`[+0x188] - 4`): the stage it belongs to and
/// whether it carries an earned trait (`+0xF0` non-zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LastEvent {
    pub stage: usize,
    pub has_trait: bool,
}

/// What the sequencer asks the window code to do this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeqOut {
    /// Show the stage's timeline callout (state 1, `0x67AAB0`).
    pub show_callout: bool,
    /// Show the earned trait of this stage (`0xE3A6E0(stage, false)` -> show).
    pub show_trait: Option<usize>,
    /// Hide the trait of this stage again.
    pub hide_trait: Option<usize>,
    /// The check button / scroll window is enabled (`+0x5C`, flag 2).
    pub enable_ok: Option<bool>,
}

/// The intro sequencer `0xE3B990`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sequencer {
    pub state: SeqState,
    /// `+0x288`: milliseconds left in the state.
    pub timer_ms: i32,
    /// `+0x27C`: the callout was shown.
    callout_shown: bool,
    /// `+0x270`: the scroll target of the window.
    pub scroll: f32,
    /// `+0x290`: where the scripted scroll starts.
    start: f32,
    /// `+0x28C`: where the scripted scroll ends.
    pub target: f32,
}

impl Sequencer {
    /// The handler `0xE449B0` puts the state to 1 (callout option on) or 2.
    /// `end` is `+0x28C` as the layout left it (right edge of the current
    /// stage's last event in content coordinates), `initial_scroll` what
    /// `0xE42100` set.
    pub fn new(end: f32, initial_scroll: f32, callout: bool) -> Self {
        Self {
            state: if callout { SeqState::Callout } else { SeqState::Begin },
            timer_ms: 0,
            callout_shown: false,
            scroll: initial_scroll,
            start: initial_scroll,
            target: end,
        }
    }

    /// `0xE36F80`: the callout was dismissed.
    pub fn close_callout(&mut self) {
        if self.state == SeqState::Callout {
            self.state = SeqState::Begin;
        }
    }

    /// `0xE3B990(dt)`. `has_callout` says whether the stage has a timeline
    /// callout at all (the Civ stage has none: its flag stays 0 and state 1
    /// ends at once).
    pub fn update(&mut self, dt_ms: i32, view: f32, last: Option<LastEvent>, has_callout: bool) -> SeqOut {
        let mut out = SeqOut::default();
        self.timer_ms -= dt_ms;
        match self.state {
            SeqState::Callout => {
                if let Some(event) = last.filter(|e| e.has_trait) {
                    let _ = event;
                    if self.callout_shown {
                        return out;
                    }
                    if has_callout {
                        out.show_callout = true;
                        self.callout_shown = true;
                    }
                }
                if !self.callout_shown {
                    self.state = SeqState::Begin;
                }
            }
            SeqState::Begin => {
                out.enable_ok = Some(true);
                // `+0x28C - (+0x98 + 1.0)` clamped at 0; `+0x98` is never
                // written (zero) in the retail code.
                self.target = (self.target - 1.0).max(0.0);
                self.start = (self.target - view * PAGE_OFFSET).max(0.0);
                self.scroll = self.start;
                self.timer_ms = PAUSE_MS;
                self.state = SeqState::Pause;
            }
            SeqState::Pause => {
                if self.timer_ms <= 0 {
                    self.timer_ms = SCROLL_MS;
                    self.state = SeqState::Scroll;
                }
            }
            SeqState::Scroll => {
                let t = (SCROLL_MS - self.timer_ms) as f32 / SCROLL_MS as f32;
                self.scroll = (self.target - self.start) * t + self.start;
                if self.timer_ms <= 0 {
                    out.enable_ok = Some(false);
                    self.timer_ms = PAUSE_AFTER_SCROLL_MS;
                    self.state = SeqState::PauseAfter;
                }
            }
            SeqState::PauseAfter => {
                if self.timer_ms <= 0 {
                    match last.filter(|e| e.has_trait) {
                        Some(event) => {
                            out.show_trait = Some(event.stage);
                            self.timer_ms = SHOW_EARNED_TRAIT_MS;
                            self.state = SeqState::EarnedTrait;
                        }
                        None => self.state = SeqState::Done,
                    }
                }
            }
            SeqState::EarnedTrait => {
                if self.timer_ms <= 0 {
                    match last {
                        Some(event) => {
                            out.hide_trait = Some(event.stage);
                            if event.stage == 3 {
                                // After the Civ stage the Space trait follows.
                                out.show_trait = Some(4);
                                self.timer_ms = SHOW_SPACE_TRAIT_MS;
                                self.state = SeqState::SpaceTrait;
                            } else {
                                self.state = SeqState::Done;
                            }
                        }
                        None => self.state = SeqState::Done,
                    }
                }
            }
            SeqState::SpaceTrait => {
                if self.timer_ms <= 0 {
                    out.hide_trait = Some(4);
                    self.state = SeqState::Done;
                }
            }
            SeqState::Done => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_last() -> Option<LastEvent> {
        Some(LastEvent { stage: 0, has_trait: true })
    }

    #[test]
    fn events_advance_by_their_width_plus_five_from_x_five() {
        let (at, end) = layout_events(&[133.0, 85.0, 40.0]);
        assert_eq!(at, vec![5.0, 143.0, 233.0]);
        assert_eq!(end, 278.0, "the final x is the column width, gap included");
        assert_eq!(layout_events(&[]), (vec![], 5.0));
    }

    #[test]
    fn only_the_shown_stage_and_the_stages_before_it_have_a_column() {
        let c = stage_columns(0, 278.0);
        assert_eq!(c.width, [278.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(c.visible(0) && !c.visible(1));
        let c = stage_columns(1, 400.0);
        assert_eq!(c.width, [5.0, 400.0, 0.0, 0.0, 0.0]);
        assert_eq!(c.left, [0.0, 5.0, 405.0, 405.0, 405.0]);
        assert_eq!(c.content_width(), 405.0);
        // An empty stage still keeps the 5 px of the start.
        assert_eq!(stage_columns(0, 0.0).width[0], 5.0);
    }

    #[test]
    fn stage_keys_map_to_the_five_stages() {
        for (i, id) in STAGE_IDS.iter().enumerate() {
            assert_eq!(stage_index(*id), i);
        }
        assert_eq!(stage_index(0), 0);
    }

    #[test]
    fn scroll_range_and_smoothing_follow_the_update() {
        assert_eq!(max_scroll(1000.0, 1280.0), 0.0);
        assert_eq!(max_scroll(1800.0, 1280.0), 520.0 + RIGHT_MARGIN);
        // 16 ms: 0.128 of the way per frame.
        assert!((follow_factor(16) - 0.128).abs() < 1e-6);
        assert_eq!(follow_factor(500), 1.0);
        assert!((follow(0.0, 100.0, 16) - 12.8).abs() < 1e-4);
        // Out of range targets are pulled back three times as fast.
        let x = rubber(-100.0, 500.0, 16);
        assert!((x - (-100.0 + 100.0 * 0.128 * 3.0)).abs() < 1e-3, "{x}");
        let x = rubber(700.0, 500.0, 16);
        assert!((x - (700.0 - 200.0 * 0.128 * 3.0)).abs() < 1e-3, "{x}");
        assert_eq!(rubber(250.0, 500.0, 16), 250.0);
    }

    #[test]
    fn the_sequencer_runs_the_retail_states_in_order() {
        // Wide content: end 2000, view 1280 -> the scroll goes 719 -> 1999.
        let mut seq = Sequencer::new(2000.0, 900.0, true);
        assert_eq!(seq.state, SeqState::Callout);
        let out = seq.update(16, 1280.0, cell_last(), true);
        assert!(out.show_callout, "the callout is shown once");
        assert_eq!(seq.state, SeqState::Callout);
        let out = seq.update(16, 1280.0, cell_last(), true);
        assert!(!out.show_callout, "and not again");
        assert_eq!(seq.state, SeqState::Callout, "it waits for the dismissal");
        seq.close_callout();
        assert_eq!(seq.state, SeqState::Begin);
        let out = seq.update(16, 1280.0, cell_last(), true);
        assert_eq!(out.enable_ok, Some(true));
        assert_eq!((seq.state, seq.timer_ms), (SeqState::Pause, PAUSE_MS));
        assert_eq!(seq.scroll, 719.0);
        // 1.5 s pause.
        for _ in 0..93 {
            seq.update(16, 1280.0, cell_last(), true);
        }
        assert_eq!(seq.state, SeqState::Pause);
        seq.update(16, 1280.0, cell_last(), true);
        assert_eq!(seq.state, SeqState::Scroll);
        assert_eq!(seq.timer_ms, SCROLL_MS);
        // Halfway through the scroll the position is halfway.
        seq.update(750, 1280.0, cell_last(), true);
        assert_eq!(seq.state, SeqState::Scroll);
        assert!((seq.scroll - (719.0 + (1999.0 - 719.0) * 0.5)).abs() < 0.01, "{}", seq.scroll);
        let out = seq.update(750, 1280.0, cell_last(), true);
        assert_eq!(out.enable_ok, Some(false));
        assert_eq!(seq.state, SeqState::PauseAfter);
        assert!((seq.scroll - 1999.0).abs() < 0.01);
        // Pause, then the earned trait of the Cell stage.
        let out = seq.update(PAUSE_AFTER_SCROLL_MS, 1280.0, cell_last(), true);
        assert_eq!(out.show_trait, Some(0));
        assert_eq!(seq.state, SeqState::EarnedTrait);
        let out = seq.update(SHOW_EARNED_TRAIT_MS, 1280.0, cell_last(), true);
        assert_eq!(out.hide_trait, Some(0));
        assert_eq!(out.show_trait, None, "no Space trait after the Cell stage");
        assert_eq!(seq.state, SeqState::Done);
        assert_eq!(seq.update(16, 1280.0, cell_last(), true), SeqOut::default());
    }

    #[test]
    fn narrow_content_does_not_move_and_a_civ_summary_shows_the_space_trait() {
        let mut seq = Sequencer::new(300.0, 0.0, false);
        assert_eq!(seq.state, SeqState::Begin);
        seq.update(16, 1280.0, cell_last(), true);
        assert_eq!(seq.scroll, 0.0, "end - view is negative: the start is 0");
        let civ = Some(LastEvent { stage: 3, has_trait: true });
        let mut seq = Sequencer::new(300.0, 0.0, true);
        // The Civ stage has no timeline callout: state 1 ends at once.
        seq.update(16, 1280.0, civ, false);
        assert_eq!(seq.state, SeqState::Begin);
        seq.state = SeqState::PauseAfter;
        seq.timer_ms = 0;
        let out = seq.update(1, 1280.0, civ, false);
        assert_eq!(out.show_trait, Some(3));
        seq.timer_ms = 0;
        let out = seq.update(1, 1280.0, civ, false);
        assert_eq!((out.hide_trait, out.show_trait), (Some(3), Some(4)));
        assert_eq!(seq.state, SeqState::SpaceTrait);
        seq.timer_ms = 0;
        let out = seq.update(1, 1280.0, civ, false);
        assert_eq!(out.hide_trait, Some(4));
        assert_eq!(seq.state, SeqState::Done);
    }

    #[test]
    fn a_last_event_without_a_trait_skips_the_callout_and_the_trait() {
        let mut seq = Sequencer::new(300.0, 0.0, true);
        let none = Some(LastEvent { stage: 0, has_trait: false });
        let out = seq.update(16, 1280.0, none, true);
        assert!(!out.show_callout);
        assert_eq!(seq.state, SeqState::Begin);
        seq.state = SeqState::PauseAfter;
        seq.timer_ms = 0;
        seq.update(1, 1280.0, none, true);
        assert_eq!(seq.state, SeqState::Done);
    }

    #[test]
    fn the_height_law_is_the_identity_inside_a_third_and_stretches_beyond_it() {
        // 0xE37D20: identity in +-1/3, then pow((|v| - 1/3) * 1.5, 0.5) * 0.75 * 2/3 + 1/3.
        assert_eq!(edge_factor(0.0), 0.0);
        assert_eq!(edge_factor(0.2), 0.2);
        assert_eq!(edge_factor(-0.3), -0.3);
        assert!((edge_factor(1.0) - (0.5 + 1.0 / 3.0)).abs() < 1e-5, "{}", edge_factor(1.0));
        assert!((edge_factor(-1.0) + (0.5 + 1.0 / 3.0)).abs() < 1e-5);
        assert_eq!(edge_factor(7.0), edge_factor(1.0), "the value is clamped to -1..1");
        // 0.6: (0.2667 * 1.5)^0.5 * 0.5 + 1/3.
        assert!((edge_factor(0.6) - ((0.4f32).sqrt() * 0.5 + 1.0 / 3.0)).abs() < 1e-4);
        // Continuous at the third and monotonic.
        assert!((edge_factor(1.0 / 3.0 + 1e-5) - 1.0 / 3.0).abs() < 0.01);
        assert!(edge_factor(0.5) < edge_factor(0.6));
        // The Civ stage keeps the value.
        assert_eq!(event_height_factor(0.9, true), 0.9);
        assert_eq!(event_height_factor(0.9, false), edge_factor(0.9));
    }

    #[test]
    fn cards_are_centred_at_half_the_parent_height_times_one_plus_the_factor() {
        // 0xE46D80: top = (h / 2) * (1 + f) - cardH / 2.
        assert_eq!(card_top(687.0, 0.0, 100.0), 343.5 - 50.0);
        assert!((card_top(687.0, 0.5, 100.0) - (343.5 * 1.5 - 50.0)).abs() < 1e-3);
        // The drawn extent stays inside the column.
        assert_eq!(clamp_card_top(-20.0, 687.0, (0.0, 100.0)), 0.0);
        assert_eq!(clamp_card_top(650.0, 687.0, (0.0, 100.0)), 587.0);
        assert_eq!(clamp_card_top(-5.0, 687.0, (-30.0, 100.0)), 30.0, "an extent above the root pushes the card down");
        assert_eq!(clamp_card_top(300.0, 687.0, (0.0, 100.0)), 300.0);
    }

    #[test]
    fn a_stack_is_centred_on_the_single_card_and_pushed_back_into_the_column() {
        // Three 100 px cards around a top of 300: 200, 300, 400.
        assert_eq!(stack_tops(300.0, 3, 100.0, 687.0), vec![200.0, 300.0, 400.0]);
        // One card is the card itself.
        assert_eq!(stack_tops(300.0, 1, 100.0, 687.0), vec![300.0]);
        // Too high: the first card moves down to 0.
        assert_eq!(stack_tops(20.0, 3, 100.0, 687.0), vec![0.0, 100.0, 200.0]);
        // Too low: the last card ends at the column's bottom.
        assert_eq!(stack_tops(600.0, 3, 100.0, 687.0), vec![387.0, 487.0, 587.0]);
        assert!(stack_tops(0.0, 0, 100.0, 687.0).is_empty());
    }

    #[test]
    fn bands_tint_towards_the_markers_band() {
        assert_eq!(marker_band(0.1), 0);
        assert_eq!(marker_band(0.5), 1);
        assert_eq!(marker_band(0.9), 2);
        assert_eq!(band_target(1, 1)[3], TINT_ALPHA);
        assert_eq!(band_target(0, 1)[3], 0.0);
        let shown = band_tint([0.0, 0.0, 0.0, 0.0], band_target(2, 2), 16);
        assert!((shown[3] - 0.25 * 0.128).abs() < 1e-5);
        assert_eq!(pack_argb([1.0, 0.0, 0.0, 1.0]), 0xFFFF_0000);
        assert_eq!(pack_argb([0.0, 0.0, 0.0, 0.0]), 0);
    }
}
