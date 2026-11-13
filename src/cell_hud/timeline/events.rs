//! The Cell stage's history record and how the timeline turns it into cards.
//!
//! Decoded from the march2017 code image (cell-timeline4; addresses are SDK
//! addresses; VERIFIED = read in the code, ADAPTED = OpenSpore's stand-in):
//!
//! * **What is recorded** (`0xE394A0`, the Cell's `AddEvent`; the Civ stage has its
//!   own `0xE3C770`, which is gated on the Civ / editor game mode): one record per
//!   event with the event id (a property file of group `TimelineEvents`), the
//!   game time in whole seconds (`[sCellSerializableData + 0x74] / 1000`) and the
//!   disposition value at that moment, `0xE5C530` = `0xE511C0(overPlantFood +
//!   plantFood, overAnimalFood + food - plantFood)` = `clamp((animal - plant) /
//!   1000, -1, 1)` (the same balance that picks the earned trait,
//!   `cell_flow::diet_balance`; the editor game mode records a random value in
//!   -0.15..0.15 instead). The callers: the game start `0xE52BE0`
//!   (`clg_gamestart`), every meal `0xE7D070` (`clg_eatplant` when the eaten thing
//!   is a plant, else `clg_eatmeat`), every kill `0xE514A0` (`clg_killedsomething`),
//!   every part found `0xE51560` (`clg_discoverpart`, the part in the record), the
//!   player's death `0xE5C9AE` (`clg_death`), the editor return `0xE809A0`
//!   (`clg_evolveineditor`) and the Advance `0xE5DA10` (`clg_earnedtrait*`).
//! * **Cards** (`0xE3F970` with the merge `0xE3F2A0`): the list is walked from the
//!   newest record to the oldest; a record not yet consumed becomes the anchor of a
//!   card. Events whose file says `94F79745` = merge (`clg_discoverpart`,
//!   `clg_killedsomething`, `clg_death`, `clg_eatplant`, `clg_eatmeat`) pull every
//!   older record of the same id into the anchor's card until a record of the
//!   barrier list `D52D9183` (`clg_evolveineditor`, the earned traits) is met; the
//!   card shows the count (`+0xD4`) and sits where the anchor (the newest of them)
//!   is. `clg_discoverpart` also has `D53426DD` = stack: its card is a vertical
//!   stack of one part card per record, and the stack ends when one more card would
//!   make it `count * cardHeight >= columnHeight`. The finished cards are placed
//!   from the oldest anchor to the newest (the second loop walks the vector
//!   backwards).
//! * **Height** (`0xE460D0`, `0xE46D80`, `0xE37D20`): see [`super::axis`].
//!
//! ADAPTED: a save or demo without a recorded history gets a synthetic one
//! ([`history`]): the start, the unlocked part kinds, then the counted kills and
//! deaths, all at the final disposition.

use serde::{Deserialize, Serialize};

/// What happened (the event id of the property file).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    /// `clg_gamestart`.
    GameStart,
    /// `clg_eatplant` / `clg_eatmeat`.
    Meal { plant: bool },
    /// `clg_discoverpart`: the unlock slot (`UnlockType`) of the part found.
    Part(u8),
    /// `clg_killedsomething`.
    Kill,
    /// `clg_death`.
    Death,
    /// `clg_evolveineditor`.
    EvolveInEditor,
}

/// One history record (`0xE394A0`): the event, the game time in seconds and the
/// disposition value (-1 herbivore .. +1 carnivore).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub kind: EventKind,
    pub seconds: i32,
    pub value: f32,
}

/// The creature of one editor visit: the parts mounted, the body chain (`None` = the neutral cell's baked body) and
/// the skin paint the Creator left. What the History's generation card shows (see `thumbs`); saved in
/// `CellSave.generations`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerationRecord {
    pub parts: Vec<crate::game::PartInstance>,
    #[serde(default)]
    pub spine: Option<Vec<crate::cell_body::Vertebra>>,
    /// The paint at the visit (`CellCreation.paint`); `None` in a record written before the paint was kept
    /// (the thumbnail then shows the live paint, as it did).
    #[serde(default)]
    pub paint: Option<crate::cell_paint::CellPaint>,
}

impl GenerationRecord {
    /// The first generation: the neutral cell, its baked body, no parts, never painted.
    pub fn neutral() -> Self {
        Self { parts: vec![], spine: None, paint: Some(crate::cell_paint::CellPaint::default()) }
    }
}

/// What one card shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackKind {
    GameStart,
    /// The `n`-th editor visit (generation `n + 2`).
    EvolveInEditor(usize),
    Meals { plant: bool },
    Parts,
    Kills,
    Deaths,
}

/// A card of the history: the anchor's disposition value and everything merged into it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub kind: StackKind,
    /// The anchor's (newest record's) disposition value: the card's height.
    pub value: f32,
    /// Records merged into the card (the anchor included).
    pub count: usize,
    /// Part stacks: the unlock slots, anchor (newest) first = top of the stack.
    pub parts: Vec<u8>,
}

/// `0xE3F2A0`'s merge flag (`94F79745`) of an event.
fn merges(kind: EventKind) -> bool {
    !matches!(kind, EventKind::GameStart | EventKind::EvolveInEditor)
}

/// Whether two records are the same event id (`edi == anchor id`).
fn same_event(a: EventKind, b: EventKind) -> bool {
    match (a, b) {
        (EventKind::Meal { plant: x }, EventKind::Meal { plant: y }) => x == y,
        (EventKind::Part(_), EventKind::Part(_)) => true,
        _ => std::mem::discriminant(&a) == std::mem::discriminant(&b),
    }
}

/// `0xE3F970` + `0xE3F2A0`: the cards of a history, left to right (oldest anchor
/// first). `column_height` is the stage column window's height and `part_height`
/// the height of one part card (the stack limit).
pub fn stacks(events: &[TimelineEvent], column_height: f32, part_height: f32) -> Vec<Stack> {
    let n = events.len();
    let mut consumed = vec![false; n];
    let mut newest_first: Vec<Stack> = Vec::new();
    let mut visits = 0usize;
    for i in (0..n).rev() {
        if consumed[i] {
            continue;
        }
        let anchor = events[i];
        let mut stack = Stack {
            kind: match anchor.kind {
                EventKind::GameStart => StackKind::GameStart,
                EventKind::EvolveInEditor => StackKind::EvolveInEditor(0),
                EventKind::Meal { plant } => StackKind::Meals { plant },
                EventKind::Part(_) => StackKind::Parts,
                EventKind::Kill => StackKind::Kills,
                EventKind::Death => StackKind::Deaths,
            },
            value: anchor.value,
            count: 1,
            parts: Vec::new(),
        };
        if let EventKind::Part(slot) = anchor.kind {
            stack.parts.push(slot);
        }
        if merges(anchor.kind) {
            let stacking = matches!(anchor.kind, EventKind::Part(_));
            for j in (0..i).rev() {
                if consumed[j] {
                    continue;
                }
                let older = events[j];
                // The barrier list `D52D9183` (the editor visit, the earned traits).
                if older.kind == EventKind::EvolveInEditor {
                    break;
                }
                // `D53426DD`: the next card would not fit the column any more.
                if stacking && (stack.count + 1) as f32 * part_height >= column_height {
                    break;
                }
                if same_event(older.kind, anchor.kind) {
                    consumed[j] = true;
                    stack.count += 1;
                    if let EventKind::Part(slot) = older.kind {
                        stack.parts.push(slot);
                    }
                }
            }
        }
        newest_first.push(stack);
    }
    // The cards are placed from the last vector element (the oldest anchor).
    newest_first.reverse();
    for stack in newest_first.iter_mut() {
        if let StackKind::EvolveInEditor(slot) = &mut stack.kind {
            *slot = visits;
            visits += 1;
        }
    }
    newest_first
}

/// The history a summary shows: the recorded events with the start in front, the
/// unlocked part kinds that have no record (a save from before the record, a demo)
/// after it, and the counted kills / deaths that have no record at the end. The
/// stand-ins take `fallback_value` (ADAPTED).
pub fn history(
    recorded: &[TimelineEvent],
    unlocked_slots: &[usize],
    kills: i32,
    deaths: i32,
    fallback_value: f32,
) -> Vec<TimelineEvent> {
    let mut out: Vec<TimelineEvent> = Vec::with_capacity(recorded.len() + 8);
    if recorded.first().is_none_or(|e| e.kind != EventKind::GameStart) {
        out.push(TimelineEvent { kind: EventKind::GameStart, seconds: 0, value: 0.0 });
    }
    out.extend_from_slice(recorded);
    let mut insert_at = out
        .iter()
        .rposition(|e| e.kind == EventKind::GameStart)
        .map_or(0, |p| p + 1);
    let mut slots: Vec<usize> = unlocked_slots.to_vec();
    slots.sort_unstable();
    slots.dedup();
    for slot in slots {
        let has = out.iter().any(|e| e.kind == EventKind::Part(slot as u8));
        if !has {
            out.insert(insert_at, TimelineEvent { kind: EventKind::Part(slot as u8), seconds: 0, value: fallback_value });
            insert_at += 1;
        }
    }
    let count = |out: &[TimelineEvent], kind: EventKind| out.iter().filter(|e| e.kind == kind).count() as i32;
    for _ in count(&out, EventKind::Kill)..kills {
        out.push(TimelineEvent { kind: EventKind::Kill, seconds: 0, value: fallback_value });
    }
    for _ in count(&out, EventKind::Death)..deaths {
        out.push(TimelineEvent { kind: EventKind::Death, seconds: 0, value: fallback_value });
    }
    out
}

/// Capture only (`--cell-timeline-history demo`): a long varied history that walks
/// from the herbivore side over an editor visit to the carnivore side.
pub fn demo_history() -> Vec<TimelineEvent> {
    use EventKind::*;
    let at = |kind, value, seconds| TimelineEvent { kind, seconds, value };
    vec![
        at(GameStart, 0.0, 0),
        at(Meal { plant: true }, -0.05, 20),
        at(Meal { plant: true }, -0.1, 40),
        at(Part(2), -0.1, 55),
        at(Meal { plant: true }, -0.25, 80),
        at(Part(4), -0.25, 95),
        at(Part(5), -0.4, 120),
        at(Kill, -0.4, 140),
        at(EvolveInEditor, -0.4, 150),
        at(Meal { plant: false }, -0.3, 170),
        at(Meal { plant: false }, -0.1, 190),
        at(Kill, 0.1, 210),
        at(Part(6), 0.25, 240),
        at(Part(7), 0.4, 260),
        at(Part(8), 0.55, 280),
        at(Kill, 0.7, 300),
        at(Death, 0.7, 320),
        at(Part(10), 0.8, 340),
        at(Part(12), 0.9, 360),
        at(Kill, 1.0, 380),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: EventKind, value: f32) -> TimelineEvent {
        TimelineEvent { kind, seconds: 0, value }
    }

    #[test]
    fn merged_events_form_one_card_at_the_newest_one() {
        // start, eat, kill, eat, kill, death: the kills and meals merge and sit
        // where their newest record is (the card order follows the anchors).
        let list = [
            ev(EventKind::GameStart, 0.0),
            ev(EventKind::Meal { plant: true }, 0.1),
            ev(EventKind::Kill, 0.1),
            ev(EventKind::Meal { plant: true }, 0.2),
            ev(EventKind::Kill, 0.3),
            ev(EventKind::Death, 0.4),
        ];
        let cards = stacks(&list, 687.0, 100.0);
        let kinds: Vec<_> = cards.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StackKind::GameStart,
                StackKind::Meals { plant: true },
                StackKind::Kills,
                StackKind::Deaths
            ]
        );
        assert_eq!(cards[1].count, 2);
        assert_eq!(cards[1].value, 0.2, "the card sits at its newest record");
        assert_eq!(cards[2].count, 2);
        assert_eq!(cards[2].value, 0.3);
    }

    #[test]
    fn plant_and_meat_meals_are_two_ids() {
        let list = [ev(EventKind::Meal { plant: true }, 0.0), ev(EventKind::Meal { plant: false }, 0.0)];
        let cards = stacks(&list, 687.0, 100.0);
        assert_eq!(cards.len(), 2);
    }

    #[test]
    fn an_editor_visit_is_a_barrier_and_a_card() {
        let list = [
            ev(EventKind::Kill, 0.0),
            ev(EventKind::EvolveInEditor, 0.0),
            ev(EventKind::Kill, 0.2),
            ev(EventKind::EvolveInEditor, 0.2),
            ev(EventKind::Kill, 0.4),
        ];
        let cards = stacks(&list, 687.0, 100.0);
        let kinds: Vec<_> = cards.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StackKind::Kills,
                StackKind::EvolveInEditor(0),
                StackKind::Kills,
                StackKind::EvolveInEditor(1),
                StackKind::Kills
            ],
            "kills never merge across an editor visit"
        );
    }

    #[test]
    fn part_stacks_end_when_the_column_is_full() {
        // Cards of 100 px in a 350 px column: a stack takes 3 (4 * 100 >= 350).
        let list: Vec<_> = (0..7).map(|i| ev(EventKind::Part(i), i as f32 * 0.1)).collect();
        let cards = stacks(&list, 350.0, 100.0);
        let sizes: Vec<_> = cards.iter().map(|c| c.parts.len()).collect();
        assert_eq!(sizes, vec![1, 3, 3], "the oldest part is left over, newest first inside");
        assert_eq!(cards[2].parts, vec![6, 5, 4]);
        assert!((cards[2].value - 0.6).abs() < 1e-6, "the stack sits at its newest part");
    }

    #[test]
    fn a_history_without_a_record_is_rebuilt() {
        let slots = [7usize, 2, 7];
        let h = history(&[], &slots, 3, 1, 0.5);
        let kinds: Vec<_> = h.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EventKind::GameStart,
                EventKind::Part(2),
                EventKind::Part(7),
                EventKind::Kill,
                EventKind::Kill,
                EventKind::Kill,
                EventKind::Death
            ]
        );
        // A recorded kill is not counted twice.
        let recorded = [ev(EventKind::GameStart, 0.0), ev(EventKind::Kill, -0.2)];
        let h = history(&recorded, &[], 2, 0, 0.5);
        assert_eq!(h.iter().filter(|e| e.kind == EventKind::Kill).count(), 2);
        assert_eq!(h[1].value, -0.2);
    }
}
