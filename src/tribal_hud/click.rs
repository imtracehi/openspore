//! The Tribal right click: what a click on the hovered object does.
//!
//! Retail: the input system posts message 0x506D38C9 for the object under the
//! pointer and `cTribeInputStrategy::DoActionGeneric` (0xCD0C20 in the march2017
//! image; the ModAPI header lists 0xCD0250, which is the middle of
//! `SetHoverObjectCursorAndRollover`) asks `ChooseInputActionID` (0xCCF130, the
//! hover side, `super::cursor`) for the `TribeInputAction` id and switches over
//! it (byte table 0xCD15BC over the jump table 0xCD1554, `id - 1`, ids 1..=0x30).
//! A click on open ground (no object) is the walk 0xCD0450 instead. Every case
//! calls `DoActionForAllSelected` (0xCD0940, the selected members each get one
//! `CitizenAction`) or shows a hint (`UI::cHintManager::ShowHint`). VERIFIED by
//! reading the code image:
//!
//! ```text
//! id            click
//! 0 2..6 8 0xD 0x26 0x2D..0x2F   nothing (modes, pan, refused gift, the hut card)
//! 1             walk to a fixed point (view manager constants): not reached here
//! 7             refused (sound 0x29A)
//! 9             Repair (24)                   0xA  Attack (7)
//! 0xB           Hunt (10)                     0xC  RaidHuts (15)
//! 0xE 0x10      food < `giftFoodAmount` (prop 0xFD444753, default 10) -> hint
//!               trg-notenoughfoodforgift; else GiftFood (9) (0xE with a member
//!               that already carries a gift bundle (type 5) skips the check)
//! 0xF           hint trg-socialwrongtarget
//! 0x11..0x1A    DoToolActionForAllSelected (0xCD09B0): a damaged rack Repair
//!               (24), the firepit (type 10) Party (12), the others GrabTool (3)
//! 0x1B          Bundle (14)                   0x1C 0x1D  GatherMeat (27)
//! 0x1E..0x20    Eat (0)                       0x21  RaidFood (8)
//! 0x22          Fish (1)                      0x23  action 13 (`CollectEgg`)
//! 0x24 0x25     Gather (2)                    0x27  Recruit (26)
//! 0x28          Heal (5), by the members that carry the healing tool alone
//! 0x29          animal fed enough (state 6): Tame (25); else food < the feed price
//!               (species prop 0xEB69140E, default 5) -> hint
//!               trg-notenoughfoodtofeedanimal, else FeedWild (20)
//! 0x2A          hint trg-allies               0x2B  hint trg-socialneedchieftain
//! 0x2C          hint trg-giftwrongtarget      0x30  (an ornament creature) not used
//! ```
//!
//! The hint ids are the FNV hashes of the `trg-*` names of group `hints~`
//! (VERIFIED against the install). ADAPTED (marked at the use): the gift is paid
//! and given at once (retail: the members carry the food over), the animal is
//! fed and tamed in one order, the heal order walks the healer to the member
//! (the healing aura does the rest), the messages are the campaign messages the
//! HUD maps back to the retail hints, the id 1 / 7 side effects are not done.

use bevy::prelude::*;

use super::cursor::{action::*, Hover, Target};
use crate::{
    tribal,
    tribal_controls::{CitizenAction as A, ContextCommand, NEED_CHIEFTAIN},
    tribal_rts::Order,
};

/// A hint the click shows (`UI::cHintManager::ShowHint`, group `hints~`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    NotEnoughFoodForGift,
    NotEnoughFoodToFeedAnimal,
    SocialWrongTarget,
    Allies,
    SocialNeedChieftain,
    GiftWrongTarget,
}

impl Hint {
    pub const ALL: [Hint; 6] = [
        Hint::NotEnoughFoodForGift,
        Hint::NotEnoughFoodToFeedAnimal,
        Hint::SocialWrongTarget,
        Hint::Allies,
        Hint::SocialNeedChieftain,
        Hint::GiftWrongTarget,
    ];

    /// The id the code passes to `ShowHint` (0xCD0E.., 0xCD1475, 0xCD10B4,
    /// 0xCD14AF, 0xCD14D9, 0xCD14F7).
    pub const fn id(self) -> u32 {
        match self {
            Hint::NotEnoughFoodForGift => 0xA066_F949,
            Hint::NotEnoughFoodToFeedAnimal => 0x6F90_2625,
            Hint::SocialWrongTarget => 0x30D2_0036,
            Hint::Allies => 0x3B05_2A35,
            Hint::SocialNeedChieftain => 0xF0FD_A5C1,
            Hint::GiftWrongTarget => 0x6B01_D763,
        }
    }

    /// The registered name of the hint (its instance id is `name_id(name)`).
    pub const fn name(self) -> &'static str {
        match self {
            Hint::NotEnoughFoodForGift => "trg-notenoughfoodforgift",
            Hint::NotEnoughFoodToFeedAnimal => "trg-notenoughfoodtofeedanimal",
            Hint::SocialWrongTarget => "trg-socialwrongtarget",
            Hint::Allies => "trg-allies",
            Hint::SocialNeedChieftain => "trg-socialneedchieftain",
            Hint::GiftWrongTarget => "trg-giftwrongtarget",
        }
    }

    /// The campaign message that stands for the hint (the HUD shows the retail
    /// hint text for it, `guide::retail_hint_for`).
    pub fn message(self) -> String {
        match self {
            Hint::NotEnoughFoodForGift => format!("Not enough food for a gift: {:.0} needed.", tribal::GIFT_FOOD),
            Hint::NotEnoughFoodToFeedAnimal => format!(
                "Not enough food to feed the animal: {:.0} needed.",
                crate::tribal_rts::livestock::TAME_FOOD
            ),
            Hint::SocialWrongTarget => "Social actions need a member of the tribe: click one of its members.".into(),
            Hint::Allies => "That tribe is your ally: you cannot attack it.".into(),
            Hint::SocialNeedChieftain => NEED_CHIEFTAIN.into(),
            Hint::GiftWrongTarget => "Gifts go to a tribe's members, its hut or its food mat.".into(),
        }
    }

    /// The hint a campaign message stands for.
    pub fn from_message(message: &str) -> Option<Hint> {
        Hint::ALL.into_iter().find(|h| h.message() == message)
    }
}

/// What `DoActionGeneric` does for an id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// Nothing happens.
    Nothing,
    /// A hint is shown.
    Hint(Hint),
    /// The selected members are told to do the action.
    Act(A),
}

/// The state the click reads besides the id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickState {
    /// The tribe's food (`cTribe` vtbl +0xA4).
    pub food: f32,
    /// `giftFoodAmount` (tuning prop 0xFD444753, default 10).
    pub gift_cost: f32,
    /// The price of feeding a wild animal (species prop 0xEB69140E, default 5;
    /// ADAPTED: OpenSpore's one-step domestication price).
    pub feed_cost: f32,
}

/// `cTribeInputStrategy::DoActionGeneric` (0xCD0C20): what a click does for the
/// `TribeInputAction` id the pointer shows. See the module notes for the table.
pub fn do_action_generic(id: i32, s: &ClickState) -> Click {
    match id {
        REPAIR => Click::Act(A::Repair),
        ATTACK_MEMBER => Click::Act(A::Attack),
        ATTACK_ANIMAL => Click::Act(A::Hunt),
        RAID_STRUCTURE => Click::Act(A::RaidHuts),
        GIFT_TRIBE | GIFT_MEMBER => {
            if s.food < s.gift_cost {
                Click::Hint(Hint::NotEnoughFoodForGift)
            } else {
                Click::Act(A::GiftFood)
            }
        }
        NONE_TRIBE => Click::Hint(Hint::SocialWrongTarget),
        // DoToolActionForAllSelected (0xCD09B0).
        TOOL_FISH..=TOOL_FIREPIT => Click::Act(if id == TOOL_FIREPIT { A::Party } else { A::GrabTool }),
        GATHER_BUNDLE => Click::Act(A::Bundle),
        // The egg shares the case of the dead animal (byte table 0xCD15BC).
        GATHER_MEAT | GATHER_EGG => Click::Act(A::GatherMeat),
        EAT_FRUIT | EAT_MEAT | EAT_BOTH => Click::Act(A::Eat),
        STEAL => Click::Act(A::RaidFood),
        FISH => Click::Act(A::Fish),
        GATHER_TREE => Click::Act(A::CollectEgg),
        GATHER_FRUIT | UNREACHABLE_GATHER => Click::Act(A::Gather),
        SOCIAL_MEMBER => Click::Act(A::Recruit),
        HEAL_MEMBER => Click::Act(A::Heal),
        DOMESTICATE => {
            if s.food < s.feed_cost {
                Click::Hint(Hint::NotEnoughFoodToFeedAnimal)
            } else {
                // ADAPTED: retail feeds until the animal is full (state 6),
                // then tames (25); OpenSpore's order does both.
                Click::Act(A::FeedWild)
            }
        }
        DEFAULT_ROLLOVER => Click::Hint(Hint::Allies),
        SOCIAL_MEMBER_NO_CHIEF => Click::Hint(Hint::SocialNeedChieftain),
        DEFAULT_ROLLOVER_2 => Click::Hint(Hint::GiftWrongTarget),
        // 0, 1, 2..=6, 7, 8, 0xD, 0x26, 0x2D..=0x2F and the rest.
        _ => Click::Nothing,
    }
}

/// What a right click comes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Nothing,
    Hint(Hint),
    Command(ContextCommand),
}

/// Where the members stand to give a gift to village `v` (in front of its hut,
/// toward the player's village, as the performers stand). ADAPTED.
fn gift_stand(v: usize) -> Vec3 {
    let centre = tribal::village_position(v);
    let toward_home = Vec2::new(-centre.x, -centre.z).normalize_or(Vec2::X);
    centre + Vec3::new(toward_home.x, 0.0, toward_home.y) * 5.0
}

/// The OpenSpore order that carries `action` out on the hovered object.
fn order_for(h: &Hover, action: A) -> Option<Order> {
    let order = h.order;
    match action {
        A::Gather | A::GatherMeat | A::CollectEgg => matches!(order, Order::Gather(_)).then_some(order),
        A::Fish => matches!(order, Order::Fish(_)).then_some(order),
        A::Attack | A::Hunt => matches!(order, Order::Hunt(_)).then_some(order),
        A::RaidHuts => matches!(order, Order::Siege(_)).then_some(order),
        A::RaidFood => matches!(order, Order::Raid(_)).then_some(order),
        A::FeedWild | A::Tame => matches!(order, Order::Domesticate(_)).then_some(order),
        A::GrabTool | A::Party => matches!(order, Order::Fetch(_)).then_some(order),
        // ADAPTED: eating at the food mat is OpenSpore's walk home (a member
        // that carries food delivers it first).
        A::Eat => Some(Order::Home),
        A::Recruit => h.village.map(Order::Perform),
        // ADAPTED: the healer walks to the member (the healing aura heals).
        A::Heal => h.member_at.map(Order::Move),
        A::GiftFood | A::GiftMember => h.village.map(|v| Order::Move(gift_stand(v))),
        // No damaged racks and no bundle objects in OpenSpore.
        A::Repair | A::Bundle => None,
        A::WalkTo => Some(Order::Move(h.point)),
        A::Sleep | A::Mate | A::ReturnToTribe => None,
    }
}

/// What a right click on `h` comes to: the ground walk, or `DoActionGeneric`
/// on the id the cursor shows.
pub fn resolve(h: &Hover, s: &ClickState) -> Outcome {
    // No hovered object: the members walk there (0xCD0450). Ground the members
    // cannot reach is id 7: refused.
    if h.target == Target::Ground {
        return Outcome::Command(ContextCommand { action: A::WalkTo, order: Order::Move(h.point) });
    }
    match do_action_generic(h.action, s) {
        Click::Nothing => Outcome::Nothing,
        Click::Hint(hint) => Outcome::Hint(hint),
        Click::Act(action) => match order_for(h, action) {
            Some(order) => Outcome::Command(ContextCommand { action, order }),
            None => Outcome::Nothing,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tribal_tools::ToolKind;

    fn state(food: f32) -> ClickState {
        ClickState { food, gift_cost: 10.0, feed_cost: 15.0 }
    }

    fn click(id: i32) -> Click {
        do_action_generic(id, &state(100.0))
    }

    #[test]
    fn the_click_table_follows_the_switch_at_0xcd0c20() {
        // The byte table 0xCD15BC (index id - 1) over the jump table 0xCD1554.
        for id in [0, 1, 2, 3, 4, 5, 6, 7, 8, 0xD, 0x26, 0x2D, 0x2E, 0x2F, 0x30, 0x31] {
            assert_eq!(click(id), Click::Nothing, "id {id:#x}");
        }
        let acts = [
            (0x9, A::Repair),
            (0xA, A::Attack),
            (0xB, A::Hunt),
            (0xC, A::RaidHuts),
            (0xE, A::GiftFood),
            (0x10, A::GiftFood),
            (0x1B, A::Bundle),
            (0x1C, A::GatherMeat),
            // The egg is in the dead animal's case.
            (0x1D, A::GatherMeat),
            (0x1E, A::Eat),
            (0x1F, A::Eat),
            (0x20, A::Eat),
            (0x21, A::RaidFood),
            (0x22, A::Fish),
            (0x23, A::CollectEgg),
            (0x24, A::Gather),
            (0x25, A::Gather),
            (0x27, A::Recruit),
            (0x28, A::Heal),
            (0x29, A::FeedWild),
        ];
        for (id, action) in acts {
            assert_eq!(click(id), Click::Act(action), "id {id:#x}");
        }
        // The tool pick-ups: the firepit dances, every other rack is grabbed.
        for id in 0x11..=0x19 {
            assert_eq!(click(id), Click::Act(A::GrabTool), "id {id:#x}");
        }
        assert_eq!(click(0x1A), Click::Act(A::Party));
        // The hint cases.
        assert_eq!(click(0xF), Click::Hint(Hint::SocialWrongTarget));
        assert_eq!(click(0x2A), Click::Hint(Hint::Allies));
        assert_eq!(click(0x2B), Click::Hint(Hint::SocialNeedChieftain));
        assert_eq!(click(0x2C), Click::Hint(Hint::GiftWrongTarget));
    }

    #[test]
    fn gifts_and_feeding_need_the_food_first() {
        for id in [0xE, 0x10] {
            assert_eq!(do_action_generic(id, &state(9.9)), Click::Hint(Hint::NotEnoughFoodForGift));
            assert_eq!(do_action_generic(id, &state(10.0)), Click::Act(A::GiftFood));
        }
        assert_eq!(do_action_generic(0x29, &state(14.0)), Click::Hint(Hint::NotEnoughFoodToFeedAnimal));
        assert_eq!(do_action_generic(0x29, &state(15.0)), Click::Act(A::FeedWild));
    }

    #[test]
    fn the_hint_ids_are_the_hashes_of_the_installed_trg_hint_names() {
        for hint in Hint::ALL {
            assert_eq!(crate::world_objects::name_id(hint.name()), hint.id(), "{}", hint.name());
            assert_eq!(Hint::from_message(&hint.message()), Some(hint));
        }
        // The messages reach the retail hints through the HUD's mapping.
        for hint in Hint::ALL {
            assert_eq!(super::super::guide::retail_hint_for(&hint.message()), Some(hint.name()), "{hint:?}");
        }
    }

    fn hover(target: Target, order: Order, village: Option<usize>, action: i32) -> Hover {
        Hover { action, target, village, entity: None, order, point: Vec3::new(1.0, 0.0, 2.0), member_at: None }
    }

    #[test]
    fn the_ground_is_walked_and_unreachable_ground_is_refused() {
        let p = Vec3::new(1.0, 0.0, 2.0);
        let walk = resolve(&hover(Target::Ground, Order::Move(p), None, NONE), &state(0.0));
        assert_eq!(walk, Outcome::Command(ContextCommand { action: A::WalkTo, order: Order::Move(p) }));
        let refused = resolve(&hover(Target::OtherContinent, Order::Move(p), None, UNREACHABLE), &state(0.0));
        assert_eq!(refused, Outcome::Nothing);
    }

    #[test]
    fn a_click_carries_the_cursors_action_out_on_the_hovered_object() {
        let mut world = World::new();
        let e = world.spawn_empty().id();
        let s = state(100.0);
        let command = |h: &Hover| match resolve(h, &s) {
            Outcome::Command(c) => Some((c.action, c.order)),
            _ => None,
        };
        let gather = hover(Target::Fruit, Order::Gather(e), None, GATHER_FRUIT);
        assert_eq!(command(&gather), Some((A::Gather, Order::Gather(e))));
        let meat = hover(Target::Carcass, Order::Gather(e), None, GATHER_MEAT);
        assert_eq!(command(&meat), Some((A::GatherMeat, Order::Gather(e))));
        let tool = hover(Target::OwnTool(ToolKind::StoneAxes), Order::Fetch(ToolKind::StoneAxes), None, TOOL_ATTACK2);
        assert_eq!(command(&tool), Some((A::GrabTool, Order::Fetch(ToolKind::StoneAxes))));
        let fire = hover(Target::OwnTool(ToolKind::Campfire), Order::Fetch(ToolKind::Campfire), None, TOOL_FIREPIT);
        assert_eq!(command(&fire), Some((A::Party, Order::Fetch(ToolKind::Campfire))));
        // The food mat is eaten at; the hut shows its card and does nothing.
        let mat = hover(Target::OwnFood, Order::Home, None, EAT_BOTH);
        assert_eq!(command(&mat), Some((A::Eat, Order::Home)));
        assert_eq!(command(&hover(Target::OwnHut, Order::Home, None, HUD)), None);
        // A member of another tribe: attacked in the attack stance, performed
        // for in the social stance (the chieftain selected).
        let attack = hover(Target::RivalMember, Order::Hunt(e), Some(2), ATTACK_MEMBER);
        assert_eq!(command(&attack), Some((A::Attack, Order::Hunt(e))));
        let social = hover(Target::RivalMember, Order::Hunt(e), Some(2), SOCIAL_MEMBER);
        assert_eq!(command(&social), Some((A::Recruit, Order::Perform(2))));
        let no_chief = hover(Target::RivalMember, Order::Hunt(e), Some(2), SOCIAL_MEMBER_NO_CHIEF);
        assert_eq!(resolve(&no_chief, &s), Outcome::Hint(Hint::SocialNeedChieftain));
        // A hut is raided, a food mat robbed, a gift walks to the village.
        let hut = hover(Target::RivalHut, Order::Siege(1), Some(1), ATTACK_OTHER);
        assert_eq!(command(&hut), Some((A::RaidHuts, Order::Siege(1))));
        let steal = hover(Target::RivalFoodMat, Order::Raid(1), Some(1), STEAL);
        assert_eq!(command(&steal), Some((A::RaidFood, Order::Raid(1))));
        let gift = hover(Target::RivalHut, Order::Siege(1), Some(1), GIFT_TRIBE);
        assert_eq!(command(&gift).map(|c| c.0), Some(A::GiftFood));
        let poor = ClickState { food: 3.0, ..s };
        assert_eq!(resolve(&gift, &poor), Outcome::Hint(Hint::NotEnoughFoodForGift));
        // The healer walks to the injured member.
        let mut heal = hover(Target::OwnMember { injured: true }, Order::Move(Vec3::ZERO), None, HEAL_MEMBER);
        heal.member_at = Some(Vec3::new(4.0, 0.0, 4.0));
        assert_eq!(command(&heal), Some((A::Heal, Order::Move(Vec3::new(4.0, 0.0, 4.0)))));
        // A refused gift and the allies' hints do nothing else.
        let allied = hover(Target::RivalMember, Order::Hunt(e), Some(0), DEFAULT_ROLLOVER);
        assert_eq!(resolve(&allied, &s), Outcome::Hint(Hint::Allies));
        let denied = hover(Target::RivalMember, Order::Hunt(e), Some(0), GIFT_DENIED);
        assert_eq!(resolve(&denied, &s), Outcome::Nothing);
    }
}
