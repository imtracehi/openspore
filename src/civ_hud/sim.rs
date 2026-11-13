//! The one place where the Civ HUD reads the Civilization rules. Everything
//! the HUD shows that depends on a rule of `civ.rs` / `civ_rts.rs` goes through
//! these small functions, so that a change of those rules (`civ-rules`) means
//! changing this file only. The `civ.rs` items read, by function, are listed in
//! `civ-hud\CHANGES.md` ("What the HUD reads").
//!
//! MERGE with `civ-rules`: `CityState::happiness()` becomes 0..100 (50
//! neutral), `NationState::relation` holds the relationship ledger,
//! `try_produce_domain` expects the live count of the city's own vehicle type
//! (`CivCampaign::fleet_of`), the abilities are used through
//! `CivCampaign::use_power` and cost money. The functions marked MERGE below
//! are the ones that change.

use bevy::math::Vec3;

use crate::{
    civ::{self, Approach, CityState, CivCampaign},
    civ_rules,
    game::RunProgress,
};

/// Why an ability cannot be used now, or `None` when `use_power` would fire
/// it: the check is the rule itself, run on a copy of the campaign (so the HUD
/// holds no cost, cooldown or unlock rule of its own).
pub fn power_refusal(campaign: &CivCampaign, slot: civ::PowerSlot, money: f32) -> Option<String> {
    let mut trial = campaign.clone();
    let mut purse = money;
    trial.use_power(slot, &mut purse, Vec3::ZERO).err()
}

/// The wallet shown by the Sporebucks readout.
pub fn money(run: &RunProgress) -> f32 {
    run.spice
}

/// The player's money is spent by the purchase and diplomacy actions.
pub fn set_money(run: &mut RunProgress, value: f32) {
    run.spice = value;
}

/// How content the citizens are, `0.0` (riot) to `1.0`: the retail mood
/// `happiness()` runs 0..100 (`civ_rules`, 50 is neutral).
pub fn mood_fraction(city: &CityState) -> f32 {
    (city.happiness() as f32 / 100.0).clamp(0.0, 1.0)
}

/// Relationship of a city's own citizens with the player, `-1.0` to `1.0`
/// (the face of the city card for the player's cities).
pub fn citizen_relation(city: &CityState) -> f32 {
    mood_fraction(city) * 2.0 - 1.0
}

/// Identity colour of a nation as an `0xAARRGGBB` value (the plates of the
/// cards and the comm screen are tinted with it).
pub fn nation_argb(nation: usize) -> u32 {
    let c = civ::nation_color(nation).to_srgba();
    let q = |v: f32| ((v.clamp(0.0, 1.0) * 255.0) + 0.5) as u32;
    0xFF00_0000 | q(c.red) << 16 | q(c.green) << 8 | q(c.blue)
}

/// Spice a city earns per minute.
pub fn income_per_minute(city: &CityState) -> f32 {
    super::per_minute(city.income())
}

/// Spice per minute of the derrick on a geyser (retail: it falls as the
/// geyser's points are mined out, `GeyserState::income`).
pub fn geyser_per_minute(geyser: &civ::GeyserState) -> f32 {
    super::per_minute(geyser.income())
}

/// What is left of a geyser's spice, `0.0..=1.0` (the card's meter).
pub fn geyser_meter(geyser: &civ::GeyserState) -> f32 {
    (geyser.points / civ_rules::DERRICK_START_POINTS).clamp(0.0, 1.0)
}

/// Fraction of a rival city's price paid so far (`0.0` without a trade route).
pub fn bought_fraction(city: &CityState) -> f32 {
    if city.trade {
        (city.spent / city.buy_price().max(1.0)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The face value of a nation, as a `-1.0 ..= 1.0` stand-in for retail's 0..100 (the five
/// faces are its steps). VERIFIED (0xBD8220, read by the city card 0xBDE388, the comm
/// buttons 0xE04E49 and the other cards): the player's own civilization is 50 (the neutral
/// face), another one maps its attitude level (0xD00BD0, `civ_comm::stance`: Furious,
/// Annoyed, Cautious, Pleased, Friendly) through 0xD00E60 to 10 / 30 / 50 / 80 / 100, which
/// pick the angry, sad, neutral, blue and green faces. A war shows only through the ledger
/// (the declaration's event), as in retail.
pub fn nation_relation(campaign: &CivCampaign, nation: usize) -> f32 {
    if nation == civ::HOME_INDEX {
        return 0.0;
    }
    let Some(state) = campaign.nations.get(nation) else {
        return 0.0;
    };
    [-1.0, -0.5, 0.0, 0.5, 1.0][crate::civ_comm::stance(state.relation.value()).min(4)]
}

/// The number of vehicles that counts against a city's cap when it buys one
/// of `kind`. MERGE: `campaign.fleet_of(kind)` (the cap is per type).
pub fn live_for_city(total_live: usize, _kind: Approach) -> usize {
    total_live
}

/// Cities that end the stage and the city counts that unlock air and sea
/// vehicles (retail 10, 4 and 2).
pub fn goal_cities() -> usize {
    civ::CITY_COUNT
}

pub fn air_cities() -> usize {
    civ::AIR_UNLOCK_CITIES
}

pub fn sea_cities() -> usize {
    civ::SEA_UNLOCK_CITIES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_follow_the_attitude_level_and_the_players_own_are_neutral() {
        let mut c = CivCampaign::default();
        c.ensure_cities();
        // 0xBD8220: the player's own civilization is 50 (frame 2).
        assert_eq!(super::super::relation_frame(nation_relation(&c, civ::HOME_INDEX)), 2);
        let level = crate::civ_comm::stance(c.nations[2].relation.value());
        assert_eq!(super::super::relation_frame(nation_relation(&c, 2)), level as u32);
        assert_eq!(nation_relation(&c, 99), 0.0);
        // Every attitude level lands on its own face (10 / 30 / 50 / 80 / 100).
        for (relation, frame) in [(-10.0, 0), (-5.0, 1), (0.0, 2), (5.0, 3), (10.0, 4)] {
            c.nations[2].relation.contributions = vec![(crate::civ_rules::RelationKind::Gift, relation)];
            assert_eq!(
                super::super::relation_frame(nation_relation(&c, 2)),
                frame,
                "relationship {relation} is attitude {}",
                crate::civ_comm::stance(relation)
            );
        }
    }

    #[test]
    fn mood_runs_from_riot_to_full_and_feeds_the_citizen_face() {
        let mut city = civ::CityState::new(0);
        city.plots = vec![None; city.plots.len()];
        let calm = mood_fraction(&city);
        assert!((0.0..=1.0).contains(&calm));
        let relation = citizen_relation(&city);
        assert!((-1.0..=1.0).contains(&relation));
        assert!((relation - (calm * 2.0 - 1.0)).abs() < 1e-6);
    }

    #[test]
    fn the_price_paid_is_a_fraction_once_trade_is_open() {
        let mut city = civ::CityState::new(1);
        assert_eq!(bought_fraction(&city), 0.0);
        city.trade = true;
        city.spent = city.buy_price() / 4.0;
        assert!((bought_fraction(&city) - 0.25).abs() < 1e-5);
        city.spent = city.buy_price() * 3.0;
        assert_eq!(bought_fraction(&city), 1.0);
    }

    #[test]
    fn nations_have_distinct_opaque_colours() {
        let colours: std::collections::HashSet<u32> = (0..civ::CITY_COUNT).map(nation_argb).collect();
        assert_eq!(colours.len(), civ::CITY_COUNT);
        assert!(colours.iter().all(|c| c >> 24 == 0xFF));
    }

    #[test]
    fn the_rules_decide_whether_an_ability_can_be_used() {
        let mut c = CivCampaign::default();
        c.ensure_cities();
        // Creature abilities open at two cities (VERIFIED 0xBF2510).
        c.cities[1].owner = civ::CityOwner::Player;
        // The stage's powers are resolved by the campaign update; do it here.
        let (cell, creature, tribal) = civ::resolve_consequence_powers(0.0, 0.0, 0, 0, 0, 0);
        c.cell_power = Some(cell);
        c.creature_power = Some(creature);
        c.tribal_power = Some(tribal);
        // Poor: the rule's own message names the price.
        let poor = power_refusal(&c, civ::PowerSlot::Cell, 10.0).expect("too poor");
        assert!(poor.contains("costs"), "{poor}");
        // Rich enough: nothing in the way, and the campaign itself is untouched.
        assert_eq!(power_refusal(&c, civ::PowerSlot::Cell, 100_000.0), None);
        assert_eq!(c.power_cell_cooldown, 0.0);
        // Recharging beats money.
        c.power_creature_cooldown = 30.0;
        let recharging = power_refusal(&c, civ::PowerSlot::Creature, 100_000.0).expect("recharging");
        assert!(recharging.contains("recharging"), "{recharging}");
        // The superweapon needs a grown nation.
        let locked = power_refusal(&c, civ::PowerSlot::Superweapon, 1.0e9).expect("locked");
        assert!(locked.contains("locked"), "{locked}");
    }

    #[test]
    fn money_is_the_wallet() {
        let mut run = RunProgress::default();
        set_money(&mut run, 321.0);
        assert_eq!(money(&run), 321.0);
    }

    #[test]
    fn stage_goals_come_from_the_campaign_constants() {
        assert_eq!(goal_cities(), civ::CITY_COUNT);
        assert!(air_cities() >= sea_cities() && air_cities() < goal_cities());
        assert_eq!(live_for_city(7, Approach::Military), 7);
        let fresh = civ::GeyserState::derrick(0, civ_rules::DERRICK_START_POINTS);
        assert!(geyser_per_minute(&fresh) > 0.0);
        assert_eq!(geyser_meter(&fresh), 1.0);
        let spent = civ::GeyserState::derrick(0, civ_rules::DERRICK_START_POINTS * 0.25);
        assert!(geyser_per_minute(&spent) <= geyser_per_minute(&fresh), "a mined out geyser pays less");
        assert_eq!(geyser_meter(&spent), 0.25);
    }
}
