//! Civilization: the rival cities appear over time (`notes\civ-reveal-re.md`).
//!
//! VERIFIED (the Civ mode update 0xAE61C0, the level setup 0xB9D970, 0xAE2EF0,
//! 0xAE3AB0, the city creation 0xB999F0): the level setup places every city
//! slot of the planet (M+0xAC = slots) but only the player's city is built; the
//! other slots wait hidden (M+0xB0 = slots - 1). Every frame of Civ mode (not
//! paused) the "new city" check 0xAE2EF0 runs; when it says yes, the hidden
//! slot with the lowest order (slot +0x2C) is built (0xAE3AB0: M+0xB0 - 1,
//! timer restarted). The first rival city (only the player's appeared so far)
//! is shown by `CVG_PRE_NewCityAppears` when tutorials are on: its state
//! `CVG_NewCityAppears` sends kCinematicMessageNewCityAppears 0x05668F43, the
//! city is placed and bound as "NewCityLocation", and when it is built the
//! creation 0xB999F0 binds "actualNewCityLocation" and sends 0x445F729 (the
//! cinematic's `wait -forContinue`) while `CVG_PRE_NewCityAppears` plays.
//!
//! The check 0xAE2EF0 (`n` = cities appeared incl. the player's, `k` = the
//! player's cities, `d` = 0 / -1 / -2 easy / normal / hard):
//! * `n > 1`: `n == 2 && k > 1`, or `k >= n - 1`, or `k > n / 2` -> at once;
//! * the gate: net worth (0xBF2C20) > 9000 / 7000 / 0 or income (0xBF70E0)
//!   > 2400 / 1600 / 0, else nothing;
//! * then hard, or `k > 1`, or worth > (d + 4) x 3000, or income > 2800 + 800 d,
//!   or derricks > d + 3, or derricks >= d + n: the timer starts (if stopped)
//!   and the city comes when it has run `SecondsNextCity` x 1000 ms (x 3 easy,
//!   / 3 hard), x 1.5 while fewer cities are hidden than appeared, else / 2 for
//!   the first rival city and / 8 on hard.
//!
//! The slot order (+0x2C) is written by 0xAE3430 when the level setup has
//! placed every slot (VERIFIED, `order_keys`): a slot on the player city's
//! continent (+0x20) gets 100 + RandomInt(100), one elsewhere 10000 +
//! RandomInt(100) (1 / 1000 + RandomInt(100) when its +0x25 flag is clear;
//! INFERRED never for NPC slots: the slot constructor 0xAE41B0 sets it and
//! the NPC slot writer 0xB9AA20 leaves it), and the nearest flagged slot on
//! the player's continent gets 0: the nearest neighbour first, then the
//! continent's other cities, then those overseas, each group in random order.
//!
//! ADAPTED: the profile check 0xC77DF0 before the gate is taken as passed; net
//! worth is money + vehicles x 1000 + the owned cities' buildings (as
//! `civ::civ_development`); the purse (as its first budget, 0xBF7370) comes with
//! its city (no derrick: a revealed rival claims its own); its first vehicle is
//! the retail free one of its construction second (0xBE5890).

use bevy::prelude::*;

use crate::civ::{CityMarker, CivCampaign, CITY_COUNT, HOME_INDEX, LAND_VEHICLE_COST};
use crate::civ_rules::{self, Difficulty};
use crate::game::RunProgress;
use crate::stages::GameStage;

/// The tutorial cinematic of the first rival city (0xAE61C0).
pub const NEW_CITY_APPEARS: &str = civ_rules::cinematics::NEW_CITY_APPEARS;
/// kCinematicMessageNewCityAppears (civ.trigger, state `CVG_NewCityAppears`).
pub const NEW_CITY_MESSAGE: u32 = 0x0566_8F43;

/// Cities appear over time (default; the smoke test plays it too).
/// `OPENSPORE_CIV_REVEAL=0` keeps the old start with every city on the map;
/// so do the unit tests' stage apps, which were written for it (the rule's
/// own tests set `hidden` themselves).
pub fn enabled() -> bool {
    !cfg!(test) && std::env::var("OPENSPORE_CIV_REVEAL").map_or(true, |v| v.trim() != "0")
}

/// What 0xAE2EF0 reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevealInput {
    pub difficulty: Difficulty,
    /// Cities appeared, the player's included (M+0xAC - M+0xB0).
    pub appeared: usize,
    /// Cities still hidden (M+0xB0).
    pub hidden: usize,
    /// The player civilization's cities (0xBF0320).
    pub player_cities: usize,
    /// Net worth (0xBF2C20).
    pub worth: f32,
    /// Income (0xBF70E0).
    pub income: f32,
    /// The player's derricks (0xBF7290).
    pub derricks: usize,
}

/// The answer of 0xAE2EF0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Check {
    /// A city appears at once.
    Now,
    /// The timer runs (started if stopped); the city appears after `ms`.
    Timer { ms: u32 },
    /// Nothing (the gate is not passed).
    Wait,
}

/// 0xAE2EF0 (see the module docs).
pub fn check(i: RevealInput) -> Check {
    let n = i.appeared as i64;
    let k = i.player_cities as i64;
    if n > 1 {
        if n == 2 {
            if k > 1 {
                return Check::Now;
            }
        } else if k >= n - 1 || k > n / 2 {
            return Check::Now;
        }
    }
    let (worth_gate, income_gate) = match i.difficulty {
        Difficulty::Easy => (9000.0, 2400.0),
        Difficulty::Medium => (7000.0, 1600.0),
        Difficulty::Hard => (0.0, 0.0),
    };
    if !(i.worth > worth_gate || i.income > income_gate) {
        return Check::Wait;
    }
    let d: i64 = match i.difficulty {
        Difficulty::Easy => 0,
        Difficulty::Medium => -1,
        Difficulty::Hard => -2,
    };
    let derricks = i.derricks as i64;
    let started = i.difficulty == Difficulty::Hard
        || k > 1
        || i.worth > ((d + 4) * 3000) as f32
        || i.income > (d * 800 + 2800) as f32
        || derricks > d + 3
        || derricks >= d + n;
    if !started {
        return Check::Wait;
    }
    let secs = civ_rules::SECONDS_NEXT_CITY.round() as i64;
    let mut ms = match i.difficulty {
        Difficulty::Easy => secs * 3000,
        Difficulty::Medium => secs * 1000,
        Difficulty::Hard => secs * 1000 / 3,
    };
    if (i.hidden as i64) < n {
        ms = ms * 3 / 2;
    } else {
        if n == 1 {
            ms /= 2;
        }
        if i.difficulty == Difficulty::Hard {
            ms /= 8;
        }
    }
    Check::Timer { ms: ms.max(0) as u32 }
}

/// The input of `check` for the campaign.
pub fn input(run: &RunProgress, difficulty: Difficulty) -> RevealInput {
    let c = &run.civ_campaign;
    let owned = c.cities.iter().filter(|city| city.owned());
    let buildings: f32 = owned.flat_map(|city| city.plots.iter().flatten()).map(|b| b.cost()).sum();
    RevealInput {
        difficulty,
        appeared: c.cities.len().saturating_sub(c.hidden_count()),
        hidden: c.hidden_count(),
        player_cities: c.owned_count(),
        worth: run.spice + c.fleet(HOME_INDEX) as f32 * LAND_VEHICLE_COST + buildings,
        income: c.income_per_cycle(),
        derricks: c.derricks(HOME_INDEX),
    }
}

/// One frame of the reveal rule (from `civ::civ_campaign_update`, in the
/// simulation). Returns the city revealed, with the vehicles to spawn.
pub fn update(
    run: &mut RunProgress,
    dt: f32,
    difficulty: Difficulty,
    spawns: Option<&mut crate::civ_rts::UnitSpawns>,
) -> Option<usize> {
    if run.civ_campaign.hidden.is_empty() {
        run.civ_campaign.reveal_timer = None;
        return None;
    }
    // The cinematic places the city itself (message 0x05668F43).
    if run.civ_campaign.reveal_pending {
        return None;
    }
    if let Some(t) = run.civ_campaign.reveal_timer.as_mut() {
        *t += dt;
    }
    let i = input(run, difficulty);
    let due = match check(i) {
        Check::Now => true,
        Check::Wait => false,
        Check::Timer { ms } => {
            let t = *run.civ_campaign.reveal_timer.get_or_insert(0.0);
            t * 1000.0 > ms as f32
        }
    };
    if !due {
        return None;
    }
    // 0xAE61C0: only the player's city so far and tutorials on -> the
    // cinematic (it asks for the city with its message).
    if i.appeared < 2 && crate::front_end::settings::tutorials_enabled() && crate::cinematics::request(NEW_CITY_APPEARS) {
        run.civ_campaign.reveal_pending = true;
        return None;
    }
    reveal(run, spawns)
}

/// 0xAE3AB0: build the next hidden city (its opening vehicles to `spawns`)
/// and restart the timer.
pub fn reveal(run: &mut RunProgress, spawns: Option<&mut crate::civ_rts::UnitSpawns>) -> Option<usize> {
    let (index, units) = run.civ_campaign.reveal_next()?;
    run.civ_campaign.reveal_timer = Some(0.0);
    run.civ_campaign.reveal_pending = false;
    if let Some(spawns) = spawns {
        spawns.0.extend(units);
    }
    run.civ_campaign.message = format!("A new city has appeared: {}", crate::civ::city_label(index));
    run.request_save();
    info!("civ: city {index} appears ({} still hidden)", run.civ_campaign.hidden.len());
    Some(index)
}

/// The next city the rule will reveal (the "NewCityLocation" of the
/// cinematic before it is placed): the hidden slot with the lowest order key.
pub fn next_hidden(campaign: &CivCampaign) -> Option<usize> {
    campaign.hidden.iter().copied().min_by_key(|i| (campaign.reveal_key(*i), *i))
}

/// VERIFIED 0xAE3430: the order keys of the city slots (index 0 is the
/// player's city, key 0). `same_continent[i]`: slot `i` is on the player
/// city's continent; `distance[i]`: its distance from the player's city;
/// `roll(i)`: RandomInt(100) for the slot.
pub fn order_keys(same_continent: &[bool], distance: &[f32], roll: impl Fn(usize) -> u32) -> Vec<u32> {
    let n = same_continent.len().min(distance.len());
    let mut keys: Vec<u32> = (0..n)
        .map(|i| if i == 0 { 0 } else if same_continent[i] { 100 + roll(i) % 100 } else { 10_000 + roll(i) % 100 })
        .collect();
    let nearest = (1..n)
        .filter(|i| same_continent[*i])
        .min_by(|a, b| distance[*a].total_cmp(&distance[*b]));
    if let Some(i) = nearest {
        keys[i] = 0;
    }
    keys
}

/// The cinematic's message 0x05668F43: place the city now.
pub fn reveal_from_cinematic(world: &mut World) -> Option<usize> {
    if world.get_resource::<State<GameStage>>().map(|s| s.get().clone()) != Some(GameStage::Civilization) {
        return None;
    }
    world.resource_scope(|world, mut run: Mut<RunProgress>| {
        let spawns = world.get_resource_mut::<crate::civ_rts::UnitSpawns>();
        match spawns {
            Some(mut s) => reveal(&mut run, Some(&mut s)),
            None => reveal(&mut run, None),
        }
    })
}

/// A hidden city is not drawn and its banner is off (every city piece is a
/// child of the root `CityMarker`).
pub fn sync_visibility(run: Res<RunProgress>, mut cities: Query<(&CityMarker, &mut Visibility)>) {
    let campaign = &run.civ_campaign;
    for (marker, mut visibility) in &mut cities {
        let want = if campaign.present(marker.0) { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want {
            *visibility = want;
        }
    }
}

/// The hidden slots of a fresh campaign: every rival.
pub fn fresh_hidden() -> Vec<usize> {
    (0..CITY_COUNT).filter(|i| *i != HOME_INDEX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(difficulty: Difficulty) -> RevealInput {
        RevealInput {
            difficulty,
            appeared: 1,
            hidden: 4,
            player_cities: 1,
            worth: 7600.0,
            income: 50.0,
            derricks: 1,
        }
    }

    #[test]
    fn the_first_rival_city_waits_for_the_timer_and_the_gate() {
        // Normal: worth over 7000 passes the gate; derricks >= d + n starts the
        // timer; the first city comes at 20 s / 2.
        assert_eq!(check(base(Difficulty::Medium)), Check::Timer { ms: 10_000 });
        // Easy: 9000 is not reached -> nothing.
        assert_eq!(check(base(Difficulty::Easy)), Check::Wait);
        let rich = RevealInput { worth: 9500.0, ..base(Difficulty::Easy) };
        assert_eq!(check(rich), Check::Timer { ms: 30_000 });
        // Hard: 20 s / 3 / 2 / 8.
        assert_eq!(check(base(Difficulty::Hard)), Check::Timer { ms: 416 });
        // Normal under the gate.
        let poor = RevealInput { worth: 3000.0, ..base(Difficulty::Medium) };
        assert_eq!(check(poor), Check::Wait);
    }

    #[test]
    fn a_player_holding_most_cities_gets_the_next_one_at_once() {
        let two = RevealInput { appeared: 2, hidden: 3, player_cities: 2, ..base(Difficulty::Medium) };
        assert_eq!(check(two), Check::Now);
        let four = RevealInput { appeared: 4, hidden: 1, player_cities: 3, ..base(Difficulty::Medium) };
        assert_eq!(check(four), Check::Now);
        // Two of three is more than half: at once too.
        let three = RevealInput { appeared: 3, hidden: 2, player_cities: 2, ..base(Difficulty::Medium) };
        assert_eq!(check(three), Check::Now);
        // One of three: the timer, x 1.5 (fewer hidden than appeared).
        let behind = RevealInput { appeared: 3, hidden: 2, player_cities: 1, derricks: 2, ..base(Difficulty::Medium) };
        assert_eq!(check(behind), Check::Timer { ms: 30_000 });
        // One of two (more hidden than appeared): the plain 20 s.
        let second = RevealInput { appeared: 2, hidden: 3, player_cities: 1, ..base(Difficulty::Medium) };
        assert_eq!(check(second), Check::Timer { ms: 20_000 });
    }

    /// The pace on normal with the retail start (money 1000, three vehicles,
    /// the four starter buildings, one derrick: worth 9200) when the player
    /// takes nothing on the ten-slot planet: 10 s (20 / 2), then 20 s four
    /// times, then 30 s (x 1.5 once fewer are hidden than appeared: from the
    /// sixth city on); taking every city out brings the next at once.
    #[test]
    fn the_reveal_pace_on_normal_follows_seconds_next_city() {
        assert_eq!(civ_rules::SECONDS_NEXT_CITY, 20.0, "CityGameGeneralTuning SecondsNextCity");
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        run.civ_campaign.hidden = fresh_hidden();
        let mut spice = run.spice;
        run.civ_campaign.begin(&mut spice);
        run.spice = spice;
        let dt = 0.05;
        let mut t = 0.0f32;
        let mut times = Vec::new();
        while times.len() < 9 && t < 400.0 {
            t += dt;
            if update(&mut run, dt, Difficulty::Medium, None).is_some() {
                times.push(t);
            }
        }
        let gaps: Vec<f32> = times
            .iter()
            .scan(0.0, |last, t| {
                let gap = t - *last;
                *last = *t;
                Some(gap)
            })
            .collect();
        assert_eq!(gaps.len(), 9, "{times:?}");
        for (gap, want) in gaps.iter().zip([10.0, 20.0, 20.0, 20.0, 20.0, 30.0, 30.0, 30.0, 30.0]) {
            assert!((gap - want).abs() < 0.2, "{gaps:?}");
        }
        // The player holding every city that is out: the next one at once.
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        let last = CITY_COUNT - 1;
        run.civ_campaign.hidden = vec![last];
        for i in 1..last {
            run.civ_campaign.cities[i].owner = crate::civ::CityOwner::Player;
        }
        assert_eq!(update(&mut run, dt, Difficulty::Medium, None), Some(last));
    }

    #[test]
    fn the_nearest_neighbour_comes_first_then_the_continent_then_overseas() {
        // Slots 1..4: 1 and 3 on the player's continent (3 nearer), 2 and 4
        // overseas.
        let same = [true, true, false, true, false];
        let distance = [0.0, 300.0, 200.0, 250.0, 500.0];
        let keys = order_keys(&same, &distance, |i| [0, 42, 7, 99, 3][i]);
        assert_eq!(keys, vec![0, 142, 10_007, 0, 10_003]);
        let mut campaign = CivCampaign::default();
        campaign.ensure_cities();
        campaign.reveal_order = keys;
        campaign.hidden = vec![1, 2, 3, 4];
        assert_eq!(next_hidden(&campaign), Some(3));
        campaign.hidden = vec![1, 2, 4];
        assert_eq!(next_hidden(&campaign), Some(1));
        campaign.hidden = vec![2, 4];
        assert_eq!(next_hidden(&campaign), Some(4), "overseas in their random order");
        // Saves without keys keep the index order.
        campaign.reveal_order.clear();
        assert_eq!(next_hidden(&campaign), Some(2));
    }

    #[test]
    fn hidden_cities_come_one_by_one_and_stay_out_of_the_rivals_play() {
        let mut run = RunProgress::default();
        run.civ_campaign.ensure_cities();
        run.civ_campaign.hidden = fresh_hidden();
        let mut spice = run.spice;
        assert!(run.civ_campaign.begin(&mut spice));
        // No vehicles or derricks for the hidden nations.
        assert!(run.civ_campaign.units.iter().all(|u| u.nation == HOME_INDEX));
        assert!(run.civ_campaign.geysers.iter().all(|g| g.owner.is_none() || g.owner == Some(HOME_INDEX)));
        assert!(!run.civ_campaign.nation_alive(1) && !run.civ_campaign.present(1) && run.civ_campaign.present(HOME_INDEX));
        assert_eq!(run.civ_campaign.nearest_rival(run.civ_campaign.cities[1].center(), 1.0e9), None);
        // The rule on hard reveals the lowest slot after its timer.
        run.spice = 100.0;
        assert_eq!(update(&mut run, 0.1, Difficulty::Hard, None), None, "the timer starts");
        let mut spawns = crate::civ_rts::UnitSpawns(Vec::new());
        assert_eq!(update(&mut run, 0.5, Difficulty::Hard, Some(&mut spawns)), Some(1));
        assert!(run.civ_campaign.present(1) && run.civ_campaign.nation_alive(1));
        // No opening fleet: the purse is the first budget, the first vehicle
        // comes from the city's construction second (0xBF7370 / 0xBE5890).
        assert!(spawns.0.is_empty());
        assert_eq!(run.civ_campaign.nations[1].domestic, crate::civ::RIVAL_STARTING_SPICE);
        assert!(run.civ_campaign.geysers.iter().all(|g| g.owner != Some(1)), "a revealed rival owns no derrick (it claims one)");
        assert_eq!(run.civ_campaign.reveal_timer, Some(0.0));
        // Taking a city (the superweapon takes hidden ones too) reveals it.
        let mut spice = 0.0;
        run.civ_campaign.take_city(4, crate::civ::Conquest::Military, &mut spice);
        assert!(run.civ_campaign.present(4) && !run.civ_campaign.hidden.contains(&4));
        assert_eq!(next_hidden(&run.civ_campaign), Some(2));
    }
}
