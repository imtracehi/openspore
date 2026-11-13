//! The terraform marble: the planet's climate scores as a rolling marble
//! (helper `space-tools2`, 2026-10-26).
//!
//! Retail keeps a planet's atmosphere score A (`cPlanetRecord +0xB0`) and
//! temperature score T (`+0xB4`) as the position of a marble on the bullseye
//! board of the terraform screen; the tools do not set the scores, they push
//! the marble. `cTerraformingManager::OnHit` (`cToolStrategy::OnHit`
//! 0x10593B0) adds the tool's `toolHit<Kind>AtmosphereVelocity` /
//! `...TemperatureVelocity` to the marble's velocity (the planet object's
//! `+0x1FD4` atmosphere velocity and `+0x1FD8` temperature velocity,
//! 0xBBC4B0 / 0xBBC4D0), and the per-frame update 0xC7FC50 integrates it.
//! The T-score is the marble's distance from the bullseye centre (0.5, 0.5)
//! against 0.4 / 0.25 / 0.1 (0xFC2040 `CalculateTScore`).
//!
//! Everything below is VERIFIED from the code image (addresses SDK /
//! march2017) and `TerraformTuning` (`02AE0C7E!A6D1BC70`), except the lines
//! marked `ADAPTED`.
//!
//! ## The update 0xC7FC50 (`this` = the planet object, `dt` in milliseconds)
//! ```text
//! dt'  = dt x BullseyeTimeScale (0xBF26A214) x 0.001
//! F    = bullseye_force(T, A)                       (0xC7F750, below)
//! vT  += F.T x (1 / BullseyeMarbleMass) x dt'       vA += F.A x ... x dt'
//! pinned: T >= 0.999 and vT > 0 -> vT = 0; T <= 0.001 and vT < 0 -> 0 (A alike)
//! vA  -= BullseyeMarbleDrag x vA |vA| x dt'         (then vT)
//! A   += vA x dt'                                   T += vT x dt'
//! v   *= (1 - BullseyeMarbleDamping) ^ dt'
//! A, T clamped to [0, 1]; the water score is clamped to the water range (0xFC20A0)
//! ```
//! ## The force 0xC7F750 (`GetTScore` 0xBBC7E0 = `tier`)
//! With `d = (T - 0.5, A - 0.5)`, `r = |d|` (`u = d / r`; below `r^2 = 2^-16` the
//! direction stays `d` and `r = 0`):
//! ```text
//! k = r ^ BullseyeSteepnessPower x (AtmosphereDivergenceRate uA^2 + WaterDivergenceRate uT^2)
//! prev = (the int `tier` read as a float: 0)
//! for i in 0..tier:                       n = number of the three plant size classes
//!     if n > 0:                           that hold at least i + 1 species
//!         thr = level_threshold(i + 1) - prev; prev = thr
//!         if r > thr: s = clamp(r, thr, thr + FloraBullseyeRadius) -> (s - thr) / radius
//!                     k -= smoothstep(s) x n x FloraBullseyeForce
//! F = k u
//! hot orbit  (planet flag 0x1000): F.T += b0 + b1 (1 - T) + b2 (1 - T)^2
//! cold orbit (planet flag 0x2000): F.T -= b0 + b1 T + b2 T^2
//! Grox planet (tech level 5, the star's empire = `cStarManager +0x1D8`):
//!     A > 0.5: F.A += quad(1 - A), else F.A -= quad(A)          (b = HotColdZoneBias0..2)
//! ```
//! The bug-like `prev` chain (the lower bound of a ring is the threshold minus
//! the previous ring's bound, so the second ring applies from `-0.15`) is the
//! code's own: the loop stores into the slot that held the integer `tier`.

// The module is the whole marble (some accessors serve the tests and drivers only).
#![allow(dead_code)]

use crate::space_worldgen::t_score;

/// `spaceTerraformWaterDivergenceRate` 0xBF26A20E (getter 0x1049B10): scales the
/// temperature axis' share of the outward force.
pub const WATER_DIVERGENCE_RATE: f32 = 0.4;
/// `spaceTerraformAtmosphereDivergenceRate` 0xBF26A20F (0x1049B60).
pub const ATMOSPHERE_DIVERGENCE_RATE: f32 = 0.4;
/// `spaceTerraformBullseyeSteepnessPower` 0xBF26A210 (0x1049BB0).
pub const BULLSEYE_STEEPNESS_POWER: f32 = 0.4;
/// `spaceTerraformFloraBullseyeRadius` 0xBF26A211 (0x1049CA0).
pub const FLORA_BULLSEYE_RADIUS: f32 = 0.075;
/// `spaceTerraformFloraBullseyeForce` 0xBF26A212 (0x1049CF0).
pub const FLORA_BULLSEYE_FORCE: f32 = 0.5;
/// `spaceTerraformBullseyeMarbleMass` 0xBF26A213 (0x1049D40).
pub const MARBLE_MASS: f32 = 0.15;
/// `spaceTerraformBullseyeTimeScale` 0xBF26A214 (0x1049DE0).
pub const TIME_SCALE: f32 = 0.005;
/// `spaceTerraformBullseyeMarbleDamping` 0xBF26A215 (0x1049D90).
pub const MARBLE_DAMPING: f32 = 0.96;
/// `spaceTerraformBullseyeMarbleDrag` 0xBF26A216 (0x1049E30).
pub const MARBLE_DRAG: f32 = 1.5;
/// `spaceTerraformHotColdZoneBias0..2` (0x05A72FA9 / 0x05A72FAD / 0x05A72FB0).
pub const ZONE_BIAS: [f32; 3] = [0.0, 0.0, 2.0];
/// `spaceTerraformLevel1..3Threshold` (loaded over the static 0.3 / 0.2 / 0.1 at
/// 0x15B1660..0x15B1668 by 0xFC24A0); the ring a species tier can hold.
pub const LEVEL_THRESHOLDS: [f32; 3] = [0.4, 0.25, 0.1];

/// Where the planet's orbit lies (`cPlanetRecord::mFlags` 0x1000 / 0x2000).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zone {
    #[default]
    Habitable,
    /// Flag 0x1000, inside the normal band.
    Hot,
    /// Flag 0x2000, beyond the cold band.
    Cold,
}

/// What the force reads of the planet besides the marble.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Conditions {
    /// `GetTScore` (0..=3): the tier the climate reaches now.
    pub tier: u8,
    /// Plant species per size class (small, medium, large): the three keys
    /// 0x6D60A1CC / 0x029C388A / 0x3A8BE428 of 0xBBC9E0.
    pub plants: [u8; 3],
    pub zone: Zone,
    /// A Grox planet (empire tech level whose star belongs to the Grox).
    pub grox: bool,
}

/// `cFlora` slots by size class. ADAPTED: the port keeps one plant count
/// (`PlanetEcology::flora`, three per tier); the plants are dealt round-robin
/// over the small, medium and large class so that a complete tier (three
/// plants) holds one of each, as the retail food web's rows do.
pub fn plant_classes(flora: u8) -> [u8; 3] {
    [flora.div_ceil(3), flora.saturating_add(1) / 3, flora / 3]
}

/// Number of classes holding at least `i + 1` plants (the counts 0xC7F750
/// gathers with 0xBBC9E0 for `i + 1` = 1..3).
fn class_counts(plants: [u8; 3]) -> [i32; 3] {
    let mut out = [0; 3];
    for (i, count) in out.iter_mut().enumerate() {
        *count = plants.iter().filter(|p| **p as usize > i).count() as i32;
    }
    out
}

/// 0xFC1F60: the radius of the ring of level `level` (1..=3; anything else
/// reads level 1).
pub fn level_threshold(level: usize) -> f32 {
    match level {
        2 => LEVEL_THRESHOLDS[1],
        3 => LEVEL_THRESHOLDS[2],
        _ => LEVEL_THRESHOLDS[0],
    }
}

/// `b0 + b1 x + b2 x^2` as the code evaluates it (`(x b2 + b1) x + b0`).
fn bias(x: f32) -> f32 {
    (x * ZONE_BIAS[2] + ZONE_BIAS[1]) * x + ZONE_BIAS[0]
}

/// 0xC7F750: the force on the marble at (`temperature`, `atmosphere`),
/// returned as (temperature, atmosphere).
pub fn bullseye_force(temperature: f32, atmosphere: f32, cond: Conditions) -> (f32, f32) {
    let dt = temperature - 0.5;
    let da = atmosphere - 0.5;
    let r2 = da * da + dt * dt;
    let (ut, ua, r) = if r2 > 1.525_879e-5 {
        let r = r2.sqrt();
        (dt / r, da / r, r)
    } else {
        (dt, da, 0.0)
    };
    let mut k = r.powf(BULLSEYE_STEEPNESS_POWER)
        * (ATMOSPHERE_DIVERGENCE_RATE * ua * ua + WATER_DIVERGENCE_RATE * ut * ut);
    let counts = class_counts(cond.plants);
    let mut prev = 0.0f32;
    for i in 0..cond.tier.min(3) as usize {
        let n = counts[i];
        if n <= 0 {
            continue;
        }
        let thr = level_threshold(i + 1) - prev;
        prev = thr;
        if r > thr {
            let upper = thr + FLORA_BULLSEYE_RADIUS;
            let x = r.max(thr).min(upper);
            let s = (x - thr) / (upper - thr);
            k -= (3.0 - s * 2.0) * s * s * (n as f32 * FLORA_BULLSEYE_FORCE);
        }
    }
    let mut ft = k * ut;
    let mut fa = k * ua;
    match cond.zone {
        Zone::Hot => ft += bias(1.0 - temperature),
        Zone::Cold => ft -= bias(temperature),
        Zone::Habitable => {}
    }
    if cond.grox {
        if atmosphere > 0.5 {
            fa += bias(1.0 - atmosphere);
        } else {
            fa -= bias(atmosphere);
        }
    }
    (ft, fa)
}

/// The marble: the planet record's scores and the planet object's velocities.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marble {
    pub temperature: f32,
    pub atmosphere: f32,
    /// `+0x1FD8`.
    pub v_temperature: f32,
    /// `+0x1FD4`.
    pub v_atmosphere: f32,
}

impl Marble {
    pub fn at(temperature: f32, atmosphere: f32) -> Self {
        Self { temperature, atmosphere, ..Self::default() }
    }

    /// `cTerraformingManager::OnHit`: a hit adds the tool's velocities
    /// (0xBBC4B0 atmosphere, temperature through the same call pair).
    pub fn hit(&mut self, atmosphere_velocity: f32, temperature_velocity: f32) {
        self.v_atmosphere += atmosphere_velocity;
        self.v_temperature += temperature_velocity;
    }

    /// The T-score of the marble's position (0xFC2040).
    pub fn t_score(&self) -> u8 {
        t_score(self.temperature, self.atmosphere)
    }

    /// Distance of the marble from the bullseye centre.
    pub fn radius(&self) -> f32 {
        ((self.temperature - 0.5).powi(2) + (self.atmosphere - 0.5).powi(2)).sqrt()
    }

    /// One update of 0xC7FC50 with `dt_ms` milliseconds.
    pub fn step(&mut self, dt_ms: f32, mut cond: Conditions) {
        cond.tier = self.t_score();
        let dt = dt_ms * TIME_SCALE * 0.001;
        let (ft, fa) = bullseye_force(self.temperature, self.atmosphere, cond);
        let inv_mass = 1.0 / MARBLE_MASS;
        self.v_temperature = ft * inv_mass * dt + self.v_temperature;
        self.v_atmosphere = fa * inv_mass * dt + self.v_atmosphere;
        // Pinned against the walls (the 0.999 / 0.001 tests of 0xC7FD1A..0xC7FD8D).
        if self.temperature >= 0.999 && self.v_temperature > 0.0 {
            self.v_temperature = 0.0;
        }
        if self.temperature <= 0.001 && self.v_temperature < 0.0 {
            self.v_temperature = 0.0;
        }
        if self.atmosphere >= 0.999 && self.v_atmosphere > 0.0 {
            self.v_atmosphere = 0.0;
        }
        if self.atmosphere <= 0.001 && self.v_atmosphere < 0.0 {
            self.v_atmosphere = 0.0;
        }
        // Quadratic drag (atmosphere first, as the code does), then the move.
        self.v_atmosphere -= MARBLE_DRAG * self.v_atmosphere * self.v_atmosphere.abs() * dt;
        self.v_temperature -= MARBLE_DRAG * self.v_temperature * self.v_temperature.abs() * dt;
        self.atmosphere += self.v_atmosphere * dt;
        self.temperature += self.v_temperature * dt;
        let damping = (1.0 - MARBLE_DAMPING).powf(dt);
        self.v_atmosphere *= damping;
        self.v_temperature *= damping;
        self.atmosphere = self.atmosphere.clamp(0.0, 1.0);
        self.temperature = self.temperature.clamp(0.0, 1.0);
    }
}

/// The free rays a terraforming driver may hold, with the (atmosphere,
/// temperature) velocity of one use (`toolHitGround*Velocity`).
pub const DRIVER_RAYS: [(&str, f32, f32); 6] = [
    ("heatray", 0.0, 0.01),
    ("coldray", 0.0, -0.01),
    ("cloudray", 0.01, 0.0),
    ("cloudvacuum", -0.01, 0.0),
    ("coldcloudray", 0.0075, -0.0075),
    ("hotcloudvacuum", -0.0075, 0.0075),
];

/// Seconds the planner looks ahead after a hold.
pub const PLAN_HORIZON_SECONDS: f32 = 60.0;
/// Hold lengths the planner tries, seconds (ten uses a second).
pub const PLAN_HOLDS: [u32; 4] = [2, 4, 8, 16];

/// The ring a driver aims for before it stops pushing (inside the retail ring
/// of the tier, with room for the marble's momentum): tier 1 0.3, tier 2 0.18,
/// tier 3 0.06.
pub fn goal_radius(tier: u8) -> f32 {
    match tier {
        0 | 1 => 0.3,
        2 => 0.18,
        _ => 0.06,
    }
}

/// For capture and test drivers (and any future terraform hint): the ray and
/// the number of seconds to hold it (ten uses a second, `AutoFireRate` 0.1 s)
/// from this state, i.e. the pair whose marble comes closest to the bullseye
/// within [`PLAN_HORIZON_SECONDS`] after the hold, if it beats doing nothing
/// by more than a hair. The physics is deterministic, so the prediction is
/// exact up to where the uses fall inside each second. A dead planet diverges
/// faster than a short burst can hold it (0.4 r^0.4 against 0.1 velocity per
/// second of ray), so the holds run to 16 s.
pub fn plan_ray(marble: Marble, cond: Conditions) -> Option<(&'static str, u32)> {
    let closest = |mut m: Marble, hold: Option<(f32, f32, u32)>| {
        let mut best = m.radius();
        let mut frame = 0usize;
        let hold_frames = hold.map_or(0, |(_, _, seconds)| seconds as usize * 60);
        let total = hold_frames + (PLAN_HORIZON_SECONDS * 60.0) as usize;
        while frame < total {
            if let Some((atmosphere, temperature, _)) = hold {
                if frame < hold_frames && frame % 6 == 0 {
                    m.hit(atmosphere, temperature);
                }
            }
            m.step(1000.0 / 60.0, cond);
            best = best.min(m.radius());
            frame += 1;
        }
        best
    };
    let mut best = None;
    let mut best_radius = closest(marble, None) - 1.0e-3;
    for (name, atmosphere, temperature) in DRIVER_RAYS {
        for seconds in PLAN_HOLDS {
            let radius = closest(marble, Some((atmosphere, temperature, seconds)));
            if radius < best_radius {
                best = Some((name, seconds));
                best_radius = radius;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(marble: &mut Marble, seconds: f32, cond: Conditions) {
        let frames = (seconds * 60.0) as usize;
        for _ in 0..frames {
            marble.step(1000.0 / 60.0, cond);
        }
    }

    #[test]
    fn plants_are_dealt_over_the_three_size_classes() {
        assert_eq!(plant_classes(0), [0, 0, 0]);
        assert_eq!(plant_classes(1), [1, 0, 0]);
        assert_eq!(plant_classes(3), [1, 1, 1]);
        assert_eq!(plant_classes(9), [3, 3, 3]);
        assert_eq!(plant_classes(7), [3, 2, 2]);
        assert_eq!(class_counts([3, 3, 3]), [3, 3, 3]);
        assert_eq!(class_counts([2, 0, 1]), [2, 1, 0]);
    }

    #[test]
    fn the_centre_holds_a_marble_still() {
        let (ft, fa) = bullseye_force(0.5, 0.5, Conditions::default());
        assert_eq!((ft, fa), (0.0, 0.0));
        let mut marble = Marble::at(0.5, 0.5);
        run(&mut marble, 30.0, Conditions::default());
        assert_eq!((marble.temperature, marble.atmosphere), (0.5, 0.5));
    }

    #[test]
    fn a_dead_planet_diverges_from_the_bullseye() {
        // k = 0.4 r^0.4 along the outward direction (0.4 on both axes).
        let (ft, fa) = bullseye_force(0.8, 0.5, Conditions::default());
        let r: f32 = 0.3;
        assert!((ft - 0.4 * r.powf(0.4)).abs() < 1.0e-6, "{ft}");
        assert!(fa.abs() < 1.0e-6);
        let mut marble = Marble::at(0.7, 0.5);
        run(&mut marble, 120.0, Conditions::default());
        assert!(marble.temperature > 0.7 + 0.1, "{marble:?}");
        assert!((marble.atmosphere - 0.5).abs() < 1.0e-4);
    }

    #[test]
    fn the_axes_diverge_at_their_own_rates() {
        // The rates are equal in the data (0.4 / 0.4), so the force on a
        // diagonal is the radial one.
        let (ft, fa) = bullseye_force(0.8, 0.8, Conditions::default());
        assert!((ft - fa).abs() < 1.0e-6);
        let r = (0.3f32 * 0.3 * 2.0).sqrt();
        assert!((ft - 0.4 * r.powf(0.4) * (0.5f32).sqrt()).abs() < 1.0e-5);
    }

    #[test]
    fn life_pulls_a_flourishing_planet_back_to_the_centre() {
        // Tier 3 with plants in every class: the second ring applies from -0.15
        // (the code's `prev` chain), a constant 0.5 x 3 inward force.
        let cond = Conditions { tier: 3, plants: [3, 3, 3], ..Default::default() };
        let (ft, _) = bullseye_force(0.55, 0.5, cond);
        let r: f32 = 0.05;
        let expected = (0.4 * r.powf(0.4) - 3.0 * FLORA_BULLSEYE_FORCE) * 1.0;
        assert!((ft - expected).abs() < 1.0e-5, "{ft} {expected}");
        assert!(ft < 0.0, "towards the centre");
        // A kicked marble settles back near the centre.
        let mut marble = Marble::at(0.5, 0.5);
        marble.hit(0.0, 0.75);
        let cond = Conditions { plants: [3, 3, 3], ..Default::default() };
        run(&mut marble, 300.0, cond);
        assert!(marble.radius() < 0.1, "{marble:?}");
    }

    #[test]
    fn a_single_ring_without_the_second_does_not_pull() {
        // Only tier-1 plants: the first ring (0.4) applies from r > 0.4, never
        // inside it; the planet drifts like a dead one.
        let cond = Conditions { tier: 1, plants: [1, 0, 0], ..Default::default() };
        let dead = bullseye_force(0.7, 0.5, Conditions::default());
        assert_eq!(bullseye_force(0.7, 0.5, cond), dead);
    }

    #[test]
    fn zone_bias_pushes_the_temperature_with_the_orbit() {
        let base = bullseye_force(0.5, 0.5, Conditions::default());
        let hot = bullseye_force(0.5, 0.5, Conditions { zone: Zone::Hot, ..Default::default() });
        let cold = bullseye_force(0.5, 0.5, Conditions { zone: Zone::Cold, ..Default::default() });
        // b2 (1 - T)^2 = 2 x 0.25 at T = 0.5.
        assert!((hot.0 - base.0 - 0.5).abs() < 1.0e-6);
        assert!((cold.0 - base.0 + 0.5).abs() < 1.0e-6);
        assert_eq!(hot.1, base.1);
    }

    #[test]
    fn grox_planets_push_the_atmosphere_away_from_the_middle() {
        let grox = Conditions { grox: true, ..Default::default() };
        let thick = bullseye_force(0.5, 0.7, grox);
        let thin = bullseye_force(0.5, 0.3, grox);
        assert!(thick.1 > bullseye_force(0.5, 0.7, Conditions::default()).1);
        assert!(thin.1 < bullseye_force(0.5, 0.3, Conditions::default()).1);
    }

    #[test]
    fn a_ray_held_for_a_second_adds_a_tenth_of_velocity() {
        // heatray: +0.01 temperature velocity per use, ten uses a second.
        let mut marble = Marble::at(0.5, 0.5);
        for _ in 0..10 {
            marble.hit(0.0, 0.01);
        }
        assert!((marble.v_temperature - 0.1).abs() < 1.0e-6);
        // The velocity moves the marble slowly: a minute of coasting.
        let before = marble.temperature;
        run(&mut marble, 60.0, Conditions::default());
        assert!(marble.temperature > before + 0.0005, "{marble:?}");
        assert!(marble.temperature < before + 0.15, "a ray tick is a nudge: {marble:?}");
    }

    #[test]
    fn an_asteroid_moves_the_marble_a_fifth_of_the_board() {
        // asteroid: -0.75 atmosphere, +0.75 temperature (VERIFIED data).
        let mut marble = Marble::at(0.5, 0.5);
        marble.hit(-0.75, 0.75);
        run(&mut marble, 60.0, Conditions::default());
        assert!(marble.temperature > 0.5 + 0.05 && marble.temperature < 0.9, "{marble:?}");
        assert!(marble.atmosphere < 0.5 - 0.05 && marble.atmosphere > 0.1, "{marble:?}");
    }

    #[test]
    fn the_marble_stops_at_the_walls_and_stays_on_the_board() {
        let mut marble = Marble::at(0.98, 0.5);
        marble.hit(0.0, 50.0);
        run(&mut marble, 60.0, Conditions::default());
        assert!(marble.temperature <= 1.0 && marble.temperature >= 0.98);
        let mut pinned = Marble { temperature: 1.0, v_temperature: 5.0, ..Marble::at(1.0, 0.5) };
        pinned.step(16.0, Conditions::default());
        assert!(pinned.v_temperature <= 0.0 + 1.0e-3, "{pinned:?}");
    }

    #[test]
    fn the_t_score_is_the_marble_distance_from_the_centre() {
        assert_eq!(Marble::at(0.5, 0.5).t_score(), 3);
        assert_eq!(Marble::at(0.5, 0.65).t_score(), 2);
        assert_eq!(Marble::at(0.5, 0.85).t_score(), 1);
        assert_eq!(Marble::at(0.5, 0.95).t_score(), 0);
        assert_eq!(level_threshold(1), 0.4);
        assert_eq!(level_threshold(3), 0.1);
    }

    #[test]
    fn the_planner_walks_the_marble_into_the_bullseye_with_rays() {
        // A dead cold world: hold the planned ray, let the marble roll, repeat.
        for (tier, start) in [(1u8, (0.2f32, 0.55f32)), (2, (0.3, 0.6))] {
            let cond = Conditions::default();
            let mut marble = Marble::at(start.0, start.1);
            let mut held = 0;
            for _ in 0..200 {
                if marble.t_score() >= tier && marble.radius() < goal_radius(tier) {
                    break;
                }
                if let Some((name, seconds)) = plan_ray(marble, cond) {
                    let (_, atmosphere, temperature) = DRIVER_RAYS.iter().copied().find(|r| r.0 == name).unwrap();
                    for frame in 0..seconds * 60 {
                        if frame % 6 == 0 {
                            marble.hit(atmosphere, temperature);
                        }
                        marble.step(1000.0 / 60.0, cond);
                        if marble.t_score() >= tier && marble.radius() < goal_radius(tier) {
                            break;
                        }
                    }
                    held += seconds;
                }
                run(&mut marble, 5.0, cond);
            }
            assert!(
                marble.t_score() >= tier && marble.radius() < goal_radius(tier),
                "tier {tier}: {marble:?} after {held} s of ray"
            );
            assert!(held >= 1 && held <= 400, "{held}");
        }
    }

    /// The constants are the installed `TerraformTuning` values.
    #[test]
    #[ignore = "requires the installed game"]
    fn the_constants_are_the_installed_tuning() {
        let Some(install) = crate::spore_install::discover().ok() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let key = crate::dbpf::ResourceKey {
            group_id: 0x02AE_0C7E,
            instance_id: 0xA6D1_BC70,
            type_id: crate::asset_catalog::PROP,
        };
        let props = index.properties(key).expect("TerraformTuning");
        let number = |id: u32| props.0.get(&id).and_then(|p| p.number()).unwrap_or(f32::NAN);
        assert_eq!(number(0xBF26_A20E), WATER_DIVERGENCE_RATE);
        assert_eq!(number(0xBF26_A20F), ATMOSPHERE_DIVERGENCE_RATE);
        assert_eq!(number(0xBF26_A210), BULLSEYE_STEEPNESS_POWER);
        assert_eq!(number(0xBF26_A211), FLORA_BULLSEYE_RADIUS);
        assert_eq!(number(0xBF26_A212), FLORA_BULLSEYE_FORCE);
        assert_eq!(number(0xBF26_A213), MARBLE_MASS);
        assert_eq!(number(0xBF26_A214), TIME_SCALE);
        assert_eq!(number(0xBF26_A215), MARBLE_DAMPING);
        assert_eq!(number(0xBF26_A216), MARBLE_DRAG);
        for (i, id) in [0x05A7_2FA9, 0x05A7_2FAD, 0x05A7_2FB0].into_iter().enumerate() {
            assert_eq!(number(id), ZONE_BIAS[i]);
        }
        for (i, id) in [0x032A_54BE, 0x032A_5609, 0x032A_560A].into_iter().enumerate() {
            assert_eq!(number(id), LEVEL_THRESHOLDS[i]);
        }
    }
}
