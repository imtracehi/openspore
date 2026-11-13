//! The galaxy layout: every star's position, kind, tech level and owner.
//!
//! Original: `SG_galaxy_messages~` creates the stars (positions from the emit
//! maps, counts per type from its `distribute` blocks), the message handler
//! 0xBB5F00 gives each star or binary the Creature tech level (random tech
//! beyond the Grox radius, 25% carry a rare), `GenerateSolSystem` 0xBB2BF0
//! picks Sol, and the new-galaxy routine 0xBB8270 places the empires from the
//! 216 start locations (a 200 x 200 tile repeated over the galaxy) and the
//! player's possible start worlds, then 0xBB27B0 builds the Grox empire.

use std::collections::HashMap;

use bevy::prelude::Vec2;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::maps::{EmitMap, EmitPixels, SectorGrid, StarMaps};
use super::{
    galaxy_star_count, mix, unit, EmpireId, StarKind, TechLevel, EMPIRE_SIZE_WEIGHTS,
    GALAXY_EXTENT, GROB_EMPIRE_SIZE, GROB_ONLY_RADIUS, GROB_SPREAD_RADIUS,
    PERCENT_CHANCE_STAR_HAS_RARE, SOL_POSITION, STARTER_MAX_DISTANCE, STARTER_MIN_DISTANCE,
    STAR_COUNTS, TECH_WEIGHTS,
};
use crate::space_worldgen_data::EMPIRE_START_LOCATIONS;

/// One star of the galaxy (16 bytes: the galaxy holds 306,301 of them).
#[derive(Clone, Copy, Debug)]
pub struct GalaxyStar {
    /// Position in the galactic plane, centred on the core.
    pub position: Vec2,
    pub kind: StarKind,
    pub tech: TechLevel,
    /// The empire owning the star; 0 is the Grox.
    pub empire: Option<EmpireId>,
    /// `cStarRecord` flag 0x1000: one terrestrial planet carries a rare.
    pub rare: bool,
}

/// The empire archetypes of retail `Archetypes` (0xBA6030 picks one from the
/// size class of the start location).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmpireArchetype {
    Warrior,
    Trader,
    Scientist,
    Shaman,
    Bard,
    Zealot,
    Diplomat,
    Ecologist,
    Grox,
}

impl EmpireArchetype {
    /// 0xBA6030: class 0 -> warrior / zealot, 1 -> diplomat / trader,
    /// 2 -> scientist / shaman / bard / ecologist, anything else -> warrior.
    pub fn from_class(class: u8, rng: &mut impl Rng) -> Self {
        match class {
            0 => {
                if rng.random_range(0..2) == 1 {
                    EmpireArchetype::Zealot
                } else {
                    EmpireArchetype::Warrior
                }
            }
            1 => {
                if rng.random_range(0..2) == 0 {
                    EmpireArchetype::Diplomat
                } else {
                    EmpireArchetype::Trader
                }
            }
            2 => match rng.random_range(0..4) {
                0 => EmpireArchetype::Scientist,
                1 => EmpireArchetype::Shaman,
                2 => EmpireArchetype::Bard,
                _ => EmpireArchetype::Ecologist,
            },
            _ => EmpireArchetype::Warrior,
        }
    }

    /// The category table at 0x15B46D0 (military 0, cultural 1, economic 2):
    /// a start world needs all three within 20 units (0xBB2670).
    pub fn category(self) -> u8 {
        match self {
            EmpireArchetype::Warrior | EmpireArchetype::Zealot | EmpireArchetype::Grox => 0,
            EmpireArchetype::Scientist
            | EmpireArchetype::Shaman
            | EmpireArchetype::Bard
            | EmpireArchetype::Ecologist => 1,
            EmpireArchetype::Trader | EmpireArchetype::Diplomat => 2,
        }
    }

    /// The last digit of an empire id (`space::empire_archetype` reads the
    /// archetype from `id % 10`, so ids are allocated by archetype).
    pub fn id_residue(self) -> u32 {
        match self {
            EmpireArchetype::Grox => 0,
            EmpireArchetype::Zealot => 1,
            EmpireArchetype::Warrior => 2,
            EmpireArchetype::Scientist => 3,
            EmpireArchetype::Diplomat => 4,
            EmpireArchetype::Trader => 5,
            EmpireArchetype::Bard => 6,
            EmpireArchetype::Ecologist => 7,
            EmpireArchetype::Shaman => 9,
        }
    }

    pub fn from_residue(residue: u32) -> Option<Self> {
        Some(match residue {
            0 => EmpireArchetype::Grox,
            1 => EmpireArchetype::Zealot,
            2 => EmpireArchetype::Warrior,
            3 => EmpireArchetype::Scientist,
            4 => EmpireArchetype::Diplomat,
            5 => EmpireArchetype::Trader,
            6 => EmpireArchetype::Bard,
            7 => EmpireArchetype::Ecologist,
            9 => EmpireArchetype::Shaman,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct EmpireRecord {
    pub id: EmpireId,
    pub archetype: EmpireArchetype,
    /// Index of the empire's home star.
    pub home: u32,
    /// Stars the empire holds at the start of the game.
    pub stars: u32,
}

#[derive(Clone, Debug)]
pub struct GalaxyLayout {
    pub stars: Vec<GalaxyStar>,
    pub core: u32,
    pub sol: u32,
    pub home: u32,
    /// Black holes paired as wormholes.
    pub wormholes: Vec<(u32, u32)>,
    pub empires: Vec<EmpireRecord>,
    /// Stars of the Grox empire (also marked `empire == Some(0)`).
    pub grox_stars: u32,
    /// Worlds a new player could start on (`mPossibleStartLocations`).
    pub possible_starts: Vec<u32>,
}

fn star_of(stars: &[GalaxyStar], i: u32) -> &GalaxyStar {
    &stars[i as usize]
}

/// 0x1ff1 star-type mask of the retail filters: single and binary stars.
fn is_system(star: &GalaxyStar) -> bool {
    star.kind.has_planets()
}

/// Swap two star records and every index that points at them.
fn swap_records(layout: &mut GalaxyLayout, a: u32, b: u32) {
    if a == b {
        return;
    }
    layout.stars.swap(a as usize, b as usize);
    let remap = |i: &mut u32| {
        if *i == a {
            *i = b;
        } else if *i == b {
            *i = a;
        }
    };
    for e in &mut layout.empires {
        remap(&mut e.home);
    }
    for s in &mut layout.possible_starts {
        remap(s);
    }
    remap(&mut layout.core);
    remap(&mut layout.sol);
    remap(&mut layout.home);
    for (x, y) in &mut layout.wormholes {
        remap(x);
        remap(y);
    }
}

/// Generate the whole galaxy. `sol_index` is the star index Sol takes (the
/// yellow star nearest to `SOL_POSITION` is swapped into it; the home star
/// takes index 0).
pub fn generate_layout(maps: &StarMaps, seed: u64, sol_index: u32) -> GalaxyLayout {
    // 1. Positions (SG_galaxy_messages~).
    let mut stars: Vec<GalaxyStar> = Vec::with_capacity(galaxy_star_count());
    let mut core = 0u32;
    let mut emit_cache: Vec<(EmitMap, EmitPixels)> = Vec::new();
    for (kind, subdivisions, density) in STAR_COUNTS {
        let count = (subdivisions * density) as usize;
        if kind == StarKind::GalacticCore {
            core = stars.len() as u32;
            stars.push(GalaxyStar {
                position: Vec2::ZERO,
                kind,
                tech: TechLevel::None,
                empire: None,
                rare: false,
            });
            continue;
        }
        let map = EmitMap::of(kind);
        if !emit_cache.iter().any(|(m, _)| *m == map) {
            emit_cache.push((map, maps.emit_pixels(map)));
        }
        let emit = &emit_cache.iter().find(|(m, _)| *m == map).unwrap().1;
        let mut rng = ChaCha8Rng::seed_from_u64(mix(seed, kind.retail_type() as u64));
        for _ in 0..count {
            let position = emit
                .sample(&mut rng)
                .clamp(Vec2::splat(-GALAXY_EXTENT), Vec2::splat(GALAXY_EXTENT));
            stars.push(GalaxyStar {
                position,
                kind,
                tech: TechLevel::None,
                empire: None,
                rare: false,
            });
        }
    }
    // 2. Tech level and rares (0xBB5F00 / 0xBB39A0).
    let grob2 = GROB_ONLY_RADIUS * GROB_ONLY_RADIUS;
    let tech_total: f32 = TECH_WEIGHTS.iter().map(|(_, w)| w).sum();
    for (i, star) in stars.iter_mut().enumerate() {
        if !star.kind.has_planets() {
            continue;
        }
        star.tech = TechLevel::Creature;
        let h = mix(seed ^ 0x7EC4_0001, i as u64);
        if star.position.length_squared() > grob2 {
            let mut roll = unit(h) * tech_total;
            for (level, weight) in TECH_WEIGHTS {
                if roll < weight {
                    star.tech = level;
                    break;
                }
                roll -= weight;
            }
        }
        star.rare = unit(mix(h, 0x5A5A)) < PERCENT_CHANCE_STAR_HAS_RARE / 100.0;
    }
    let positions: Vec<Vec2> = stars.iter().map(|s| s.position).collect();
    let position = |i: u32| positions[i as usize];
    let grid = SectorGrid::build(positions.len(), position);

    // 3. Sol: the closest yellow star to the Sol position (0xBB2BF0).
    let sol = grid
        .nearest(&position, SOL_POSITION, 4000.0, |i| {
            star_of(&stars, i).kind == StarKind::Yellow
        })
        .map(|(i, _)| i)
        .unwrap_or(sol_index);
    stars[sol as usize].tech = TechLevel::Creature;

    // 4. Empires and possible start worlds (0xBB8270).
    let mut rng = ChaCha8Rng::seed_from_u64(mix(seed, 0xE3B1));
    let mut empires: Vec<EmpireRecord> = Vec::new();
    let mut residue_count = [0u32; 10];
    let mut possible_starts: Vec<u32> = Vec::new();
    for &(lx, ly, class) in EMPIRE_START_LOCATIONS.iter() {
        for tx in 0..10 {
            for ty in 0..10 {
                let p = Vec2::new(lx - 1000.0 + 200.0 * tx as f32, ly - 1000.0 + 200.0 * ty as f32);
                if p.length_squared() <= grob2 {
                    continue;
                }
                let mut empire_path = true;
                if let Some((i, _)) = grid.nearest(&position, p, 5.0, |i| {
                    star_of(&stars, i).kind == StarKind::Yellow
                }) {
                    let star = star_of(&stars, i);
                    let r = star.position.length();
                    if star.tech != TechLevel::Empire
                        && class == 0
                        && r > STARTER_MIN_DISTANCE
                        && r < STARTER_MAX_DISTANCE
                        && i != sol
                    {
                        stars[i as usize].tech = TechLevel::Creature;
                        possible_starts.push(i);
                        empire_path = false;
                    }
                }
                if !empire_path {
                    continue;
                }
                let found = grid.nearest(&position, p, 3.0, |i| {
                    let s = star_of(&stars, i);
                    is_system(s)
                        && s.tech != TechLevel::Empire
                        && s.tech != TechLevel::None
                        && i != sol
                        && s.position.length_squared() > grob2
                });
                let Some((home, _)) = found else {
                    continue;
                };
                let archetype = EmpireArchetype::from_class(class, &mut rng);
                let residue = archetype.id_residue() as usize;
                let id = (residue as u32 + 10 * residue_count[residue]) as EmpireId;
                residue_count[residue] += 1;
                stars[home as usize].tech = TechLevel::Empire;
                stars[home as usize].empire = Some(id);
                empires.push(EmpireRecord {
                    id,
                    archetype,
                    home,
                    stars: 1,
                });
            }
        }
    }
    // An empire needs two other creature stars within 3 units of its home
    // (0xBB8270), else it dissolves.
    empires.retain(|e| {
        let mut near = 0;
        let origin = positions[e.home as usize];
        grid.for_each_within(&position, origin, 3.0, |i, _| {
            if i != e.home && star_of(&stars, i).tech == TechLevel::Creature && is_system(star_of(&stars, i)) {
                near += 1;
            }
        });
        if near < 2 {
            let home = &mut stars[e.home as usize];
            home.tech = TechLevel::Creature;
            home.empire = None;
            false
        } else {
            true
        }
    });

    // 5. The Grox (0xBB27B0): the creature star nearest the core is their home,
    // then random creature stars within the spread radius (2,400 of them).
    let mut grox_stars = 0u32;
    let grox_home = grid.nearest(&position, Vec2::ZERO, 20.0, |i| {
        let s = star_of(&stars, i);
        is_system(s) && s.tech == TechLevel::Creature && s.position.length() >= 0.01
    });
    if let Some((home, _)) = grox_home {
        stars[home as usize].tech = TechLevel::Empire;
        stars[home as usize].empire = Some(0);
        grox_stars += 1;
        let mut pool: Vec<u32> = Vec::new();
        grid.for_each_within(&position, Vec2::ZERO, GROB_SPREAD_RADIUS, |i, d| {
            let s = star_of(&stars, i);
            if d >= 0.01 && is_system(s) && s.tech == TechLevel::Creature {
                pool.push(i);
            }
        });
        pool.sort_unstable();
        pool.shuffle(&mut rng);
        for &i in pool.iter().take(GROB_EMPIRE_SIZE) {
            stars[i as usize].tech = TechLevel::Empire;
            stars[i as usize].empire = Some(0);
            grox_stars += 1;
        }
    }

    // 6. Empire sizes: `empireSizeWeights` gives the star count (index) of
    // each empire; ADAPTED: the nearest free creature stars within the empire
    // radius (20 +- 10) join (the retail empire AI grows them at run time, S3D).
    let weight_total: u32 = EMPIRE_SIZE_WEIGHTS.iter().sum();
    for e in &mut empires {
        let mut roll = rng.random_range(0..weight_total);
        let mut size = 1usize;
        for (n, w) in EMPIRE_SIZE_WEIGHTS.iter().enumerate() {
            if roll < *w {
                size = n.max(1);
                break;
            }
            roll -= *w;
        }
        if size < 2 {
            continue;
        }
        // The empire radius: 20 +- 10 units (`spacenpcai~`).
        let radius = crate::space_rules::EMPIRE_RADIUS
            + (unit(mix(seed ^ 0xE4A5, e.id as u64)) * 2.0 - 1.0) * crate::space_rules::EMPIRE_RADIUS_VARIANCE;
        let mut near: Vec<(u32, f32)> = Vec::new();
        grid.for_each_within(&position, positions[e.home as usize], radius, |i, d| {
            let s = star_of(&stars, i);
            if i != e.home
                && i != sol
                && is_system(s)
                && s.tech == TechLevel::Creature
                && s.position.length_squared() > grob2
            {
                near.push((i, d));
            }
        });
        near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for &(i, _) in near.iter().take(size - 1) {
            stars[i as usize].tech = TechLevel::Empire;
            stars[i as usize].empire = Some(e.id);
            e.stars += 1;
        }
    }

    // 7. The player's start world: a possible start with an empire of every
    // category within 20 units (0xBB2670), the first by hash.
    let archetype_of: HashMap<EmpireId, EmpireArchetype> =
        empires.iter().map(|e| (e.id, e.archetype)).collect();
    let categories_near = |i: u32| -> u8 {
        let mut bits = 0u8;
        grid.for_each_within(&position, positions[i as usize], 20.0, |j, _| {
            if let Some(id) = star_of(&stars, j).empire {
                let archetype = if id == 0 {
                    Some(EmpireArchetype::Grox)
                } else {
                    archetype_of.get(&id).copied()
                };
                if let Some(a) = archetype {
                    bits |= 1 << a.category();
                }
            }
        });
        bits
    };
    possible_starts.retain(|&i| stars[i as usize].tech == TechLevel::Creature && i != sol);
    possible_starts.sort_unstable();
    possible_starts.dedup();
    let by_hash = |list: &mut Vec<u32>| list.sort_by_key(|i| mix(seed ^ 0x40E5, *i as u64));
    by_hash(&mut possible_starts);
    let home = possible_starts
        .iter()
        .copied()
        .find(|&i| categories_near(i) == 7)
        .or_else(|| possible_starts.first().copied())
        .or_else(|| {
            // No start location on this map (the analytic fallback): any free
            // yellow star in the starter ring.
            (0..stars.len() as u32).find(|&i| {
                let s = star_of(&stars, i);
                s.kind == StarKind::Yellow
                    && s.tech == TechLevel::Creature
                    && i != sol
                    && (STARTER_MIN_DISTANCE..STARTER_MAX_DISTANCE).contains(&s.position.length())
            })
        })
        .unwrap_or(0);

    // 8. Wormholes: black holes in random pairs (ADAPTED: the retail pairing
    // was not traced).
    let mut holes: Vec<u32> = (0..stars.len() as u32)
        .filter(|&i| stars[i as usize].kind == StarKind::BlackHole)
        .collect();
    holes.shuffle(&mut rng);
    let wormholes: Vec<(u32, u32)> = holes.chunks_exact(2).map(|c| (c[0], c[1])).collect();

    let mut layout = GalaxyLayout {
        stars,
        core,
        sol,
        home,
        wormholes,
        empires,
        grox_stars,
        possible_starts,
    };
    // Indices 0 and `sol_index` are fixed by the campaign: swap the records in.
    swap_records(&mut layout, 0, home);
    let sol_now = layout.sol;
    swap_records(&mut layout, sol_index, sol_now);
    layout.home = 0;
    layout.sol = sol_index;
    let sol_star = &mut layout.stars[sol_index as usize];
    sol_star.empire = None;
    sol_star.tech = TechLevel::Creature;
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn layout() -> &'static GalaxyLayout {
        static LAYOUT: OnceLock<GalaxyLayout> = OnceLock::new();
        LAYOUT.get_or_init(|| generate_layout(&StarMaps::analytic(), 0x5EED_6A1A, 42))
    }

    #[test]
    fn star_counts_per_type_come_from_the_galaxy_effect() {
        let layout = layout();
        assert_eq!(layout.stars.len(), 306_301);
        assert_eq!(galaxy_star_count(), 306_301);
        let count = |kind| layout.stars.iter().filter(|s| s.kind == kind).count();
        assert_eq!(count(StarKind::Yellow), 60_000);
        assert_eq!(count(StarKind::Red), 100_000);
        assert_eq!(count(StarKind::Blue), 128_000);
        assert_eq!(count(StarKind::BlackHole), 2_400);
        assert_eq!(count(StarKind::ProtoPlanetary), 9_000);
        assert_eq!(count(StarKind::GalacticCore), 1);
        assert_eq!(count(StarKind::BinaryOO), 900);
        for kind in [
            StarKind::BinaryOM,
            StarKind::BinaryOG,
            StarKind::BinaryGG,
            StarKind::BinaryGM,
            StarKind::BinaryMM,
        ] {
            assert_eq!(count(kind), 1_200, "{kind:?}");
        }
        assert!(layout
            .stars
            .iter()
            .all(|s| s.position.abs().max_element() <= GALAXY_EXTENT));
        assert_eq!(layout.stars[layout.core as usize].position, Vec2::ZERO);
    }

    #[test]
    fn generation_is_deterministic() {
        let again = generate_layout(&StarMaps::analytic(), 0x5EED_6A1A, 42);
        let a = layout();
        assert_eq!(a.home, again.home);
        assert_eq!(a.empires.len(), again.empires.len());
        for i in (0..a.stars.len()).step_by(997) {
            assert_eq!(a.stars[i].position, again.stars[i].position);
            assert_eq!(a.stars[i].tech, again.stars[i].tech);
        }
    }

    #[test]
    fn tech_levels_follow_the_grox_radius_and_weights() {
        let layout = layout();
        for s in layout.stars.iter().filter(|s| s.kind.has_planets()) {
            if s.position.length() <= GROB_ONLY_RADIUS && s.empire != Some(0) {
                assert_eq!(s.tech, TechLevel::Creature);
            }
        }
        let outside: Vec<_> = layout
            .stars
            .iter()
            .filter(|s| s.kind.has_planets() && s.position.length() > GROB_ONLY_RADIUS)
            .collect();
        let creature = outside.iter().filter(|s| s.tech == TechLevel::Creature).count() as f32;
        let share = creature / outside.len() as f32;
        // 150 / 170 before empires took a few stars.
        assert!((0.80..0.90).contains(&share), "creature share {share}");
        let rares = layout.stars.iter().filter(|s| s.rare).count() as f32;
        let systems = layout.stars.iter().filter(|s| s.kind.has_planets()).count() as f32;
        assert!((0.23..0.27).contains(&(rares / systems)), "rare share {}", rares / systems);
    }

    #[test]
    fn home_sol_and_the_grox_are_placed() {
        let layout = layout();
        let home = &layout.stars[0];
        assert_eq!(home.kind, StarKind::Yellow);
        assert_eq!(home.tech, TechLevel::Creature);
        assert_eq!(home.empire, None);
        let sol = &layout.stars[42];
        assert_eq!((sol.kind, sol.empire), (StarKind::Yellow, None));
        assert_eq!(layout.sol, 42);
        // Sol is the yellow star nearest to its retail position.
        let nearest = layout
            .stars
            .iter()
            .filter(|s| s.kind == StarKind::Yellow)
            .map(|s| s.position.distance(SOL_POSITION))
            .fold(f32::MAX, f32::min);
        assert!((sol.position.distance(SOL_POSITION) - nearest).abs() < 1e-3);
        // The Grox: one empire of about 2,400 stars around the core.
        assert!(
            (2000..=2401).contains(&layout.grox_stars),
            "{} Grox stars",
            layout.grox_stars
        );
        let grox = layout.stars.iter().filter(|s| s.empire == Some(0)).count() as u32;
        assert_eq!(grox, layout.grox_stars);
        assert!(layout
            .stars
            .iter()
            .filter(|s| s.empire == Some(0))
            .all(|s| s.position.length() <= GROB_SPREAD_RADIUS));
    }

    #[test]
    fn empires_start_from_the_tiled_start_locations() {
        let layout = layout();
        assert!(layout.empires.len() > 200, "{} empires", layout.empires.len());
        let mut ids = std::collections::HashSet::new();
        for e in &layout.empires {
            assert!(ids.insert(e.id), "duplicate empire id {}", e.id);
            assert_eq!(e.id as u32 % 10, e.archetype.id_residue());
            assert_ne!(e.id, 0);
            let home = &layout.stars[e.home as usize];
            assert_eq!(home.empire, Some(e.id));
            assert_eq!(home.tech, TechLevel::Empire);
            assert!(home.position.length() > GROB_ONLY_RADIUS * 0.9);
            assert!((1..=15).contains(&e.stars), "{} stars", e.stars);
        }
        let owned = layout
            .stars
            .iter()
            .filter(|s| s.empire.is_some_and(|e| e != 0))
            .count() as u32;
        assert_eq!(owned, layout.empires.iter().map(|e| e.stars).sum::<u32>());
        // Class 3 spots (a third of them) are warriors, as 0xBA6030 returns.
        let warriors = layout
            .empires
            .iter()
            .filter(|e| e.archetype == EmpireArchetype::Warrior)
            .count() as f32;
        assert!(warriors / layout.empires.len() as f32 > 0.25);
    }

    #[test]
    fn black_holes_pair_up_as_wormholes() {
        let layout = layout();
        assert_eq!(layout.wormholes.len(), 1_200);
        let mut seen = std::collections::HashSet::new();
        for &(a, b) in &layout.wormholes {
            assert_ne!(a, b);
            assert!(seen.insert(a) && seen.insert(b));
            assert_eq!(layout.stars[a as usize].kind, StarKind::BlackHole);
            assert_eq!(layout.stars[b as usize].kind, StarKind::BlackHole);
        }
    }
}
