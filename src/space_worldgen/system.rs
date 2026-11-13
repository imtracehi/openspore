//! Star systems and planets (cStarManager::GeneratePlanetsForStar 0xBB42A0,
//! the orbit pass 0xBAFC70 / 0xBADC70, CalculatePlanetScores 0xBA65F0).
//!
//! The original draws, for every system, 1-5 terrestrial planets (moons count
//! as planets) from the weights of `SpaceSolarSystem`, with at most two
//! asteroid belts and three gas giants inserted between them (never first);
//! planet types are masked by the star colour and by how much habitable orbit
//! band is left; inhabited stars force the type of their first planet; every
//! planet gets a name from `cSpaceNames`, a terrain script from the T-score
//! pool of its type (which fixes water / temperature / atmosphere) and a spice
//! colour from `AssignPlanetSpice`.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{
    assign_spice, mix, t_score, unit, NameKind, OrbitRadii, PlanetKind, Spice, SpaceNames,
    StarKind, TechLevel, CHANCE_ADDITIONAL_PLANETS_SPACE_TECH, CHANCE_GAS_GIANT_MOON,
    CHANCE_GAS_GIANT_RINGS, CHANCE_TERRESTRIAL_MOON, CHANCE_TERRESTRIAL_RINGS,
    HABITABLE_BAND_PER_MOON, HABITABLE_BAND_PER_PLANET, MAX_ASTEROID_BELTS, MAX_GAS_GIANTS,
    NUM_PLANETS_MAX, NUM_PLANETS_MIN, TSCORE_COLONY_PLANET, TSCORE_CIV_PLANET,
    TSCORE_EMPIRE_HOMEWORLD, TSCORE_GROB_PLANET, TSCORE_TRIBE_PLANET, WEIGHT_ART_DIRECTED,
    WEIGHT_ASTEROID_BELT, WEIGHT_BARREN, WEIGHT_GAS_GIANT, WEIGHT_TERRESTRIAL_1,
    WEIGHT_TERRESTRIAL_2, WEIGHT_TERRESTRIAL_3,
};
use crate::space_worldgen_data::POOLS;

/// WorldId slots reserved per star system (the first `MAX_WORLD_SLOTS` worlds).
pub const MAX_WORLD_SLOTS: u32 = 16;

/// What the star's generation needs to know about its star.
#[derive(Clone, Copy, Debug)]
pub struct StarContext {
    pub index: u32,
    pub kind: StarKind,
    pub tech: TechLevel,
    /// The home star of an empire (its first planet is the empire homeworld).
    pub empire_home: bool,
    /// A Grox star.
    pub grox: bool,
    /// `cStarRecord` flag 0x1000: one planet carries a rare.
    pub rare: bool,
    /// The player's home star: needs a homeworld and a few more planets so
    /// the Space tutorial has worlds to scan and colonise (ADAPTED).
    pub home_system: bool,
}

#[derive(Clone, Debug)]
pub struct PlanetRecord {
    pub kind: PlanetKind,
    pub name: String,
    /// Index (in `SystemRecord::planets`) of the planet a moon orbits.
    pub moon_of: Option<usize>,
    pub ring: bool,
    pub tech: TechLevel,
    /// Perihelion in retail solar units (a moon takes its parent's).
    pub orbit: f32,
    /// Starting angle on the orbit, radians.
    pub phase: f32,
    /// Orbit hotter than the normal band (`flags` 0x1000) or colder than the
    /// cold band (0x2000): spice uses the "hard" chances.
    pub hard: bool,
    pub water: f32,
    pub temperature: f32,
    pub atmosphere: f32,
    pub t_score: u8,
    pub spice: Option<Spice>,
    /// The terrain script the planet will use: (group, instance).
    pub script: Option<(u32, u32)>,
    pub rare: bool,
    /// An art-directed planet (the `weightArtDirected` draw).
    pub art_directed: bool,
    /// Slot of the world among the system's worlds (non-moon terrestrial
    /// planets in generation order); gas giants, belts and moons have none.
    pub world_slot: Option<u8>,
    /// Half-width of the orbit slot in retail units (0xBAFC70 table 9/9/9/18).
    pub half_width: f32,
    /// Display radius in scene units (ADAPTED).
    pub radius: f32,
}

#[derive(Clone, Debug)]
pub struct SystemRecord {
    pub kind: StarKind,
    pub tech: TechLevel,
    pub radii: OrbitRadii,
    pub planets: Vec<PlanetRecord>,
}

impl SystemRecord {
    pub fn world_count(&self) -> usize {
        self.planets.iter().filter(|p| p.world_slot.is_some()).count()
    }

    pub fn belts(&self) -> impl Iterator<Item = &PlanetRecord> {
        self.planets.iter().filter(|p| p.kind == PlanetKind::AsteroidBelt)
    }

    pub fn moons_of(&self, index: usize) -> usize {
        self.planets.iter().filter(|p| p.moon_of == Some(index)).count()
    }
}

/// 0xBA7470: a weighted pick; `None` when every weight is zero (the original
/// returns the entry count, which 0xBB3BE0 turns into a barren planet).
fn weighted(weights: &[f32], rng: &mut impl Rng) -> Option<usize> {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let roll = rng.random::<f32>() * total;
    let mut cumulative = 0.0;
    for (i, w) in weights.iter().enumerate() {
        cumulative += w;
        if roll < cumulative {
            return Some(i);
        }
    }
    Some(weights.len() - 1)
}

/// 0xBB3BE0: the type draw. Bits 0..6 of `mask` allow belt, gas giant, T0,
/// T1, T2, T3, art-directed; the result is the retail `PlanetType` number
/// (0 belt, 1 gas giant, 2..5 T0..T3, 6 art-directed).
fn choose_type(mask: u32, rng: &mut impl Rng) -> usize {
    let all = [
        WEIGHT_ASTEROID_BELT,
        WEIGHT_GAS_GIANT,
        WEIGHT_BARREN,
        WEIGHT_TERRESTRIAL_1,
        WEIGHT_TERRESTRIAL_2,
        WEIGHT_TERRESTRIAL_3,
        WEIGHT_ART_DIRECTED,
    ];
    let weights: Vec<f32> = all
        .iter()
        .enumerate()
        .map(|(i, w)| if mask & (1 << i) != 0 { *w } else { 0.0 })
        .collect();
    weighted(&weights, rng).unwrap_or(2)
}

fn pool_script(level: usize, rng: &mut impl Rng) -> (u32, u32, f32, f32, f32) {
    let pool = POOLS[level];
    pool[rng.random_range(0..pool.len())]
}

/// Orbit class of 0xBAFC70: 0 belt, 1 T0, 2 T1-T3, 3 gas giant.
fn orbit_class(kind: PlanetKind) -> usize {
    match kind {
        PlanetKind::AsteroidBelt => 0,
        PlanetKind::T0 => 1,
        PlanetKind::GasGiant => 3,
        _ => 2,
    }
}

fn blank(kind: PlanetKind, name: String) -> PlanetRecord {
    PlanetRecord {
        kind,
        name,
        moon_of: None,
        ring: false,
        tech: TechLevel::None,
        orbit: 0.0,
        phase: 0.0,
        hard: false,
        water: 0.0,
        temperature: 0.0,
        atmosphere: 0.0,
        t_score: 0,
        spice: None,
        script: None,
        rare: false,
        art_directed: false,
        world_slot: None,
        half_width: 9.0,
        radius: 3.0,
    }
}

/// `CalculatePlanetScores` for T1-T3 and art-directed planets: the water,
/// temperature and atmosphere of a random script of the T-score pool.
fn apply_script(planet: &mut PlanetRecord, level: usize, rng: &mut impl Rng) {
    let (group, instance, water, temperature, atmosphere) = pool_script(level, rng);
    planet.script = Some((group, instance));
    planet.water = water;
    planet.temperature = temperature;
    planet.atmosphere = atmosphere;
}

/// Generate the planets of one system. `homeworld_name` names the home
/// star's first planet (the player's own planet).
pub fn generate_system(
    names: &SpaceNames,
    seed: u64,
    ctx: &StarContext,
    homeworld_name: Option<&str>,
) -> SystemRecord {
    // The home system must offer worlds to visit: redraw until it has four
    // (ADAPTED; moons and gas giants count as planets in the retail loop).
    let mut record = build_system(names, seed, ctx, homeworld_name, 0);
    for salt in 1..16u64 {
        if !ctx.home_system || record.world_count() >= 4 {
            break;
        }
        record = build_system(names, seed, ctx, homeworld_name, salt);
    }
    record
}

fn build_system(
    names: &SpaceNames,
    seed: u64,
    ctx: &StarContext,
    homeworld_name: Option<&str>,
    salt: u64,
) -> SystemRecord {
    let radii = ctx.kind.orbit_radii();
    let mut record = SystemRecord {
        kind: ctx.kind,
        tech: ctx.tech,
        radii,
        planets: Vec::new(),
    };
    if !ctx.kind.has_planets() {
        return record;
    }
    let mut rng = ChaCha8Rng::seed_from_u64(mix(seed ^ 0x5157_0000 ^ (salt << 40), ctx.index as u64));
    let wanted = if ctx.home_system {
        rng.random_range(3..=NUM_PLANETS_MAX)
    } else {
        rng.random_range(NUM_PLANETS_MIN..=NUM_PLANETS_MAX)
    };
    let (mut belts_left, mut giants_left) = (MAX_ASTEROID_BELTS as i32, MAX_GAS_GIANTS as i32);
    let mut band = radii.cold - radii.normal;
    let mut count = 0u32;
    let mut guard = 0;
    while count < wanted && guard < 64 {
        guard += 1;
        let first = count == 0;
        let tech = ctx.tech;
        // An inhabited star's first planet has its type forced by tech level.
        let forced_score = if ctx.home_system && first {
            Some(3)
        } else if first && tech >= TechLevel::Tribe {
            match tech {
                TechLevel::Tribe => Some(TSCORE_TRIBE_PLANET),
                TechLevel::Civilization => Some(TSCORE_CIV_PLANET),
                TechLevel::Empire => Some(if ctx.grox {
                    TSCORE_GROB_PLANET
                } else if ctx.empire_home {
                    TSCORE_EMPIRE_HOMEWORLD
                } else {
                    TSCORE_COLONY_PLANET
                }),
                _ => None,
            }
        } else {
            None
        };
        let mut mask: u32 = if ctx.kind.is_blue_system() { 0x4F } else { 0x7F };
        if let Some(score) = forced_score {
            mask = (mask & !0x43 & !0x3C) | (1 << (2 + score));
        }
        if band <= HABITABLE_BAND_PER_PLANET {
            mask &= !0x78;
        }
        if belts_left < 1 || first {
            mask &= !0x01;
        }
        if giants_left < 1 || first {
            mask &= !0x02;
        }
        let drawn = choose_type(mask, &mut rng);
        let name = if ctx.home_system && first {
            homeworld_name
                .map(str::to_string)
                .unwrap_or_else(|| names.generate(NameKind::Planet, &mut rng))
        } else {
            names.generate(NameKind::Planet, &mut rng)
        };
        let mut planet = blank(PlanetKind::T0, name);
        match drawn {
            0 => planet.kind = PlanetKind::AsteroidBelt,
            1 => planet.kind = PlanetKind::GasGiant,
            2 => planet.kind = PlanetKind::T0,
            3 | 4 | 5 => {
                planet.kind = PlanetKind::from_t_level(drawn as u8 - 2);
                apply_script(&mut planet, drawn - 2, &mut rng);
            }
            _ => {
                // Art-directed: any script of the pools; the planet takes the
                // type of its T-score (0xBA65F0 `Unset` case).
                let level = rng.random_range(0..4usize);
                apply_script(&mut planet, level, &mut rng);
                planet.art_directed = true;
                planet.kind = PlanetKind::from_t_level(t_score(planet.temperature, planet.atmosphere));
            }
        }
        planet.t_score = if planet.kind.is_terrestrial() {
            t_score(planet.temperature, planet.atmosphere)
        } else {
            0
        };
        let habitable = matches!(planet.kind, PlanetKind::T1 | PlanetKind::T2 | PlanetKind::T3);
        if habitable {
            band -= HABITABLE_BAND_PER_PLANET;
        }
        // Tech level of the planet (0xBB42A0 after the scores): the first
        // planet of an inhabited star takes the star's; other planets are
        // inhabited by creatures, or become empire colonies in empire systems.
        planet.tech = if ctx.home_system && first {
            TechLevel::Creature
        } else if forced_score.is_some() {
            tech
        } else {
            let space_tech = match planet.kind {
                PlanetKind::T0 => ctx.grox,
                PlanetKind::T1 | PlanetKind::T2 | PlanetKind::T3 => !ctx.grox,
                _ => false,
            };
            if !space_tech {
                if planet.kind == PlanetKind::T0 {
                    TechLevel::Creature
                } else {
                    TechLevel::None
                }
            } else if tech == TechLevel::Empire
                && rng.random::<f32>() < CHANCE_ADDITIONAL_PLANETS_SPACE_TECH
            {
                TechLevel::Empire
            } else {
                TechLevel::Creature
            }
        };
        let (moon_chance, ring_chance) = match planet.kind {
            PlanetKind::AsteroidBelt => (0.0, 0.0),
            PlanetKind::GasGiant => (CHANCE_GAS_GIANT_MOON, CHANCE_GAS_GIANT_RINGS),
            _ => (CHANCE_TERRESTRIAL_MOON, CHANCE_TERRESTRIAL_RINGS),
        };
        match planet.kind {
            PlanetKind::AsteroidBelt => belts_left -= 1,
            PlanetKind::GasGiant => giants_left -= 1,
            _ => count += 1,
        }
        let moon = !((habitable && band < HABITABLE_BAND_PER_MOON) || moon_chance <= 0.0)
            && rng.random::<f32>() < moon_chance;
        if ring_chance > 0.0 && rng.random::<f32>() < ring_chance {
            planet.ring = true;
        }
        let parent_index = record.planets.len();
        record.planets.push(planet);
        if moon {
            if habitable {
                band -= HABITABLE_BAND_PER_MOON;
            }
            let moon_name = names.generate(NameKind::Planet, &mut rng);
            let mut moon = blank(PlanetKind::T0, moon_name);
            let level = choose_type(0x3C, &mut rng);
            moon.kind = PlanetKind::from_t_level(level as u8 - 2);
            if level >= 3 {
                apply_script(&mut moon, level - 2, &mut rng);
            }
            moon.moon_of = Some(parent_index);
            moon.t_score = if moon.kind.is_terrestrial() {
                t_score(moon.temperature, moon.atmosphere)
            } else {
                0
            };
            let space_tech = if moon.kind == PlanetKind::T0 { ctx.grox } else { !ctx.grox };
            moon.tech = if space_tech
                && tech == TechLevel::Empire
                && rng.random::<f32>() < CHANCE_ADDITIONAL_PLANETS_SPACE_TECH
            {
                TechLevel::Empire
            } else {
                TechLevel::Creature
            };
            record.planets.push(moon);
            count += 1;
        }
    }
    place_orbits(&mut record, &mut rng);
    finish_planets(&mut record, ctx, &mut rng);
    record
}

/// 0xBAFC70: every planet takes an orbit in the zone of its class (belts on
/// 60 + 20 n, habitable planets between the normal and cold radius, barren
/// planets and gas giants out to the outer limit), then overlapping slots are
/// pushed apart (ADAPTED: one outward sweep instead of the 100 retail passes).
fn place_orbits(record: &mut SystemRecord, rng: &mut impl Rng) {
    let radii = record.radii;
    let red = record.kind == StarKind::Red;
    let half = [9.0f32, 9.0, 9.0, 18.0];
    let mut zones: Vec<(usize, f32, f32)> = Vec::new();
    for (i, planet) in record.planets.iter_mut().enumerate() {
        if planet.moon_of.is_some() {
            continue;
        }
        let class = orbit_class(planet.kind);
        planet.half_width = half[class];
        let (lo, hi) = match class {
            0 => {
                let reference = if red { radii.cold } else { radii.hot + 19.0 };
                let lo = ((reference * 0.05).round() * 20.0).max(60.0);
                (lo, 280.0)
            }
            2 => (radii.normal, radii.cold),
            _ => (
                if red { radii.cold } else { radii.hot + half[class] },
                radii.outer,
            ),
        };
        planet.orbit = if class == 0 {
            let mut o = 60.0 + 20.0 * rng.random_range(0..12) as f32;
            while o < lo {
                o += 20.0;
            }
            while o > hi {
                o -= 20.0;
            }
            o
        } else {
            lo + rng.random::<f32>() * (hi - lo).max(0.0)
        };
        planet.phase = rng.random_range(0.0..std::f32::consts::TAU);
        zones.push((i, lo, hi));
    }
    zones.sort_by(|a, b| {
        record.planets[a.0]
            .orbit
            .total_cmp(&record.planets[b.0].orbit)
    });
    for k in 1..zones.len() {
        let (prev, cur) = (zones[k - 1].0, zones[k].0);
        let minimum =
            record.planets[prev].orbit + record.planets[prev].half_width + record.planets[cur].half_width;
        if record.planets[cur].orbit < minimum {
            let limit = zones[k].2.max(record.planets[cur].orbit);
            record.planets[cur].orbit = minimum.min(limit);
        }
    }
    for i in 0..record.planets.len() {
        if let Some(parent) = record.planets[i].moon_of {
            record.planets[i].orbit = record.planets[parent].orbit;
            record.planets[i].phase = record.planets[parent].phase;
        }
    }
}

/// Scores of barren planets from their orbit (0xBADC70), the T0 terrain script
/// of the matching temperature band (0xBA6120), radii, spice, rares and slots.
fn finish_planets(record: &mut SystemRecord, ctx: &StarContext, rng: &mut impl Rng) {
    let radii = record.radii;
    let mut slot = 0u8;
    let mut rare_given = !ctx.rare;
    for i in 0..record.planets.len() {
        let orbit = record.planets[i].orbit;
        let hard = orbit < radii.normal || orbit > radii.cold;
        let planet = &mut record.planets[i];
        planet.hard = hard;
        // Display radius (scene units).
        planet.radius = match planet.kind {
            PlanetKind::GasGiant => rng.random_range(6.0..8.5),
            PlanetKind::AsteroidBelt => 0.0,
            PlanetKind::T0 => rng.random_range(2.3..3.4),
            _ => rng.random_range(2.8..4.2),
        };
        if planet.kind == PlanetKind::T0 {
            // 0xBADC70: temperature from the orbit zone, atmosphere in
            // {0, 0.5, 1} (two choices in the normal zone).
            let (temperature, atmosphere) = if orbit < radii.normal {
                (1.0, rng.random_range(0..3) as f32 * 0.5)
            } else if orbit <= radii.cold {
                (0.5, rng.random_range(0..2) as f32)
            } else {
                (0.0, rng.random_range(0..3) as f32 * 0.5)
            };
            // 0xBA6120: a T0 script whose temperature band (>0.66 / <0.34)
            // matches, at most 100 tries.
            let band = |t: f32| (t > 0.66, t < 0.34);
            planet.temperature = temperature;
            planet.atmosphere = atmosphere;
            planet.water = 0.0;
            for _ in 0..100 {
                let (group, instance, water, t, a) = pool_script(0, rng);
                if band(t) == band(temperature) {
                    planet.script = Some((group, instance));
                    planet.water = water;
                    planet.temperature = t;
                    planet.atmosphere = a;
                    break;
                }
            }
            planet.t_score = t_score(planet.temperature, planet.atmosphere);
        }
    }
    // Moons share their parent's orbit zone (hard flag) for spice.
    for i in 0..record.planets.len() {
        if let Some(parent) = record.planets[i].moon_of {
            record.planets[i].hard = record.planets[parent].hard;
        }
    }
    for i in 0..record.planets.len() {
        let is_moon = record.planets[i].moon_of.is_some();
        let kind = record.planets[i].kind;
        if !kind.is_terrestrial() {
            continue;
        }
        let roll = unit(mix(ctx.index as u64 ^ 0x59C3_0000, i as u64));
        record.planets[i].spice = Some(assign_spice(ctx.kind, record.planets[i].hard, roll));
        if !is_moon {
            record.planets[i].world_slot = Some(slot);
            slot += 1;
            if !rare_given {
                record.planets[i].rare = true;
                rare_given = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> SpaceNames {
        SpaceNames::builtin()
    }

    fn system(index: u32, kind: StarKind, tech: TechLevel) -> SystemRecord {
        generate_system(
            &names(),
            7,
            &StarContext {
                index,
                kind,
                tech,
                empire_home: false,
                grox: false,
                rare: false,
                home_system: false,
            },
            None,
        )
    }

    #[test]
    fn systems_follow_the_planet_rules() {
        let mut belts = 0;
        let mut giants = 0;
        let mut totals = [0usize; 6];
        for index in 0..4000u32 {
            let kind = [StarKind::Yellow, StarKind::Red, StarKind::Blue, StarKind::BinaryGM][index as usize % 4];
            let s = system(index, kind, TechLevel::Creature);
            let terrestrial = s.planets.iter().filter(|p| p.kind.is_terrestrial()).count();
            // 1-5 planets, plus the moon of the last one (0xBB42A0 counts both).
            assert!((1..=6).contains(&terrestrial), "{index}: {terrestrial}");
            let b = s.planets.iter().filter(|p| p.kind == PlanetKind::AsteroidBelt).count();
            let g = s.planets.iter().filter(|p| p.kind == PlanetKind::GasGiant).count();
            assert!(b <= 2 && g <= 3, "{index}: {b} belts, {g} giants");
            belts += b;
            giants += g;
            // The first planet is never a belt or a gas giant.
            assert!(s.planets[0].kind.is_terrestrial(), "{index}");
            for p in &s.planets {
                if !p.kind.is_terrestrial() {
                    assert_eq!((p.temperature, p.atmosphere), (0.0, 0.0));
                    assert!(p.spice.is_none());
                } else {
                    assert!(p.spice.is_some());
                    totals[p.kind.t_level().unwrap() as usize] += 1;
                    assert_eq!(p.t_score, t_score(p.temperature, p.atmosphere), "{p:?}");
                    if p.kind != PlanetKind::T0 {
                        assert_eq!(p.t_score, p.kind.t_level().unwrap(), "{p:?}");
                    }
                }
            }
            // Blue systems draw no T2 / T3 planets (mask 0x4F), unless art-directed.
            if kind == StarKind::Blue {
                assert!(s
                    .planets
                    .iter()
                    .filter(|p| p.moon_of.is_none())
                    .all(|p| !matches!(p.kind, PlanetKind::T2 | PlanetKind::T3) || p.art_directed));
            }
        }
        assert!(belts > 500 && giants > 500, "{belts} belts {giants} giants");
        // Barren planets dominate; T3 is rare (weights 1000 : 300 : 3 : 1).
        assert!(totals[0] > totals[1] && totals[1] > totals[3], "{totals:?}");
    }

    #[test]
    fn habitable_orbits_lie_between_the_normal_and_cold_radius() {
        for index in 0..1500u32 {
            let s = system(index, StarKind::Yellow, TechLevel::Creature);
            for p in s.planets.iter().filter(|p| p.moon_of.is_none()) {
                if matches!(p.kind, PlanetKind::T1 | PlanetKind::T2 | PlanetKind::T3) {
                    assert!(
                        p.orbit >= s.radii.normal - 0.01,
                        "{index}: habitable at {} (normal {})",
                        p.orbit,
                        s.radii.normal
                    );
                }
            }
            // The retail orbit radii of a yellow star: R + 15, R + 85, R + 195.
            assert_eq!((s.radii.hot, s.radii.normal, s.radii.cold, s.radii.outer), (45.0, 115.0, 225.0, 400.0));
        }
        let red = StarKind::Red.orbit_radii();
        assert_eq!((red.hot, red.normal, red.cold), (33.0, 60.5, 115.5));
        let blue = StarKind::Blue.orbit_radii();
        assert_eq!((blue.hot, blue.normal, blue.cold, blue.outer), (65.0, 220.0, 440.0, 540.0));
    }

    #[test]
    fn inhabited_stars_force_their_first_planet() {
        let make = |tech, empire_home, grox| {
            generate_system(
                &names(),
                9,
                &StarContext {
                    index: 11,
                    kind: StarKind::Yellow,
                    tech,
                    empire_home,
                    grox,
                    rare: false,
                    home_system: false,
                },
                None,
            )
        };
        assert_eq!(make(TechLevel::Tribe, false, false).planets[0].t_score, 2);
        assert_eq!(make(TechLevel::Civilization, false, false).planets[0].t_score, 3);
        assert_eq!(make(TechLevel::Empire, true, false).planets[0].t_score, 3);
        assert_eq!(make(TechLevel::Empire, false, false).planets[0].t_score, 2);
        let grox = make(TechLevel::Empire, true, true);
        assert_eq!(grox.planets[0].kind, PlanetKind::T0);
        assert_eq!(make(TechLevel::Civilization, false, false).planets[0].tech, TechLevel::Civilization);
    }

    #[test]
    fn spice_follows_the_star_colour_and_orbit_difficulty() {
        // The chances of S4C: a red star's normal planets are 96% red.
        let draw = |star, hard, n: u32| -> Vec<usize> {
            let mut counts = vec![0usize; 6];
            for i in 0..n {
                let spice = assign_spice(star, hard, (i as f32 + 0.5) / n as f32);
                counts[spice as usize] += 1;
            }
            counts
        };
        let red = draw(StarKind::Red, false, 1000);
        assert_eq!(red[Spice::Red as usize], 960);
        assert_eq!(red[Spice::Green as usize] + red[Spice::Pink as usize], 40);
        let blue_hard = draw(StarKind::Blue, true, 1000);
        assert_eq!(
            [
                blue_hard[Spice::Blue as usize],
                blue_hard[Spice::Green as usize],
                blue_hard[Spice::Pink as usize],
                blue_hard[Spice::Purple as usize]
            ],
            [200, 300, 350, 150]
        );
        let yellow_hard = draw(StarKind::BinaryGG, true, 1000);
        assert_eq!(yellow_hard[Spice::Yellow as usize], 740);
        assert_eq!(draw(StarKind::Blue, false, 100)[Spice::Blue as usize], 100);
        assert_eq!(draw(StarKind::BinaryMM, false, 100)[Spice::Red as usize], 96);
    }

    #[test]
    fn t_scores_use_the_bullseye_thresholds() {
        assert_eq!(t_score(0.5, 0.5), 3);
        assert_eq!(t_score(0.5, 0.59), 3);
        assert_eq!(t_score(0.5, 0.61), 2);
        assert_eq!(t_score(0.5, 0.74), 2);
        assert_eq!(t_score(0.5, 0.76), 1);
        assert_eq!(t_score(0.5, 0.89), 1);
        assert_eq!(t_score(0.5, 0.91), 0);
        assert_eq!(t_score(1.0, 1.0), 0);
        assert_eq!(t_score(0.0, 0.5), 0);
        // The distance, not the weaker axis, counts: both axes 0.25 off is 0.354 away.
        assert_eq!(t_score(0.75, 0.75), 1);
        assert_eq!(t_score(0.8, 0.8), 0);
    }

    #[test]
    fn the_home_system_has_a_homeworld_and_worlds_to_visit() {
        for index in [0u32, 5] {
            let s = generate_system(
                &names(),
                1,
                &StarContext {
                    index,
                    kind: StarKind::Yellow,
                    tech: TechLevel::Creature,
                    empire_home: false,
                    grox: false,
                    rare: true,
                    home_system: true,
                },
                Some("Gaia"),
            );
            assert_eq!(s.planets[0].name, "Gaia");
            assert_eq!(s.planets[0].kind, PlanetKind::T3);
            assert_eq!(s.planets[0].world_slot, Some(0));
            assert!(s.world_count() >= 4, "{} worlds", s.world_count());
            assert_eq!(s.planets.iter().filter(|p| p.rare).count(), 1);
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = system(123, StarKind::Yellow, TechLevel::Creature);
        let b = system(123, StarKind::Yellow, TechLevel::Creature);
        assert_eq!(a.planets.len(), b.planets.len());
        for (x, y) in a.planets.iter().zip(&b.planets) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.orbit, y.orbit);
            assert_eq!(x.spice, y.spice);
            assert_eq!(x.script, y.script);
        }
    }
}
