//! The galaxy emit maps and the 64 x 64 star sector grid.
//!
//! `SG_galaxy_messages~` places stars with these 1-bit images (`effectMap`
//! resources of the `games` effect directory): `SG_base_star_map` (2048 x 2048,
//! the spiral arms, +-1000 units) multiplied by `SG_noise_tile_clouds`
//! (yellow and red stars) or `SG_noise_tile_darker` (blue stars, proto-planetary
//! disks), both 1024 x 1024 tiles that repeat every 200 units, and
//! `SG_dustlane_map` (black holes and binaries). Image layout (`.#03E421E9`):
//! big-endian width at byte 4, height at byte 8, bits from byte 24, most
//! significant bit first, row 0 at the minimum y.

use bevy::prelude::Vec2;
use rand::Rng;

use super::{StarKind, EMIT_MAP_HALF, GALAXY_GRID_SIZE, GALAXY_SECTORS, NOISE_TILE};

/// Resource type of the 1-bit images.
pub const BIT_IMAGE_TYPE: u32 = 0x03E4_21E9;

#[derive(Clone, Debug)]
pub struct BitImage {
    pub width: usize,
    pub height: usize,
    bits: Vec<u8>,
}

impl BitImage {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 24 {
            return None;
        }
        let width = u32::from_be_bytes(data[4..8].try_into().ok()?) as usize;
        let height = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
        let bytes = width.checked_mul(height)? / 8;
        if width == 0 || height == 0 || data.len() < 24 + bytes {
            return None;
        }
        Some(Self {
            width,
            height,
            bits: data[24..24 + bytes].to_vec(),
        })
    }

    /// Build from a predicate (the analytic fallback maps).
    pub fn from_fn(width: usize, height: usize, f: impl Fn(usize, usize) -> bool) -> Self {
        let mut bits = vec![0u8; width * height / 8];
        for y in 0..height {
            for x in 0..width {
                if f(x, y) {
                    let i = y * width + x;
                    bits[i / 8] |= 0x80 >> (i % 8);
                }
            }
        }
        Self {
            width,
            height,
            bits,
        }
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        let i = y * self.width + x;
        self.bits[i / 8] & (0x80 >> (i % 8)) != 0
    }

    pub fn count_set(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }
}

/// The emit pixels of one `distribute` map: the set pixels of the map at its
/// own resolution, over the +-1000 rectangle.
#[derive(Clone, Debug)]
pub struct EmitPixels {
    pub width: usize,
    pub height: usize,
    pub set: Vec<u32>,
}

impl EmitPixels {
    /// One star position: a random set pixel plus a jitter inside it. The
    /// original draws from a quasi-random sequence per `start` offset; a
    /// seeded generator keeps the same uniform density (ADAPTED).
    pub fn sample(&self, rng: &mut impl Rng) -> Vec2 {
        let pixel = self.set[rng.random_range(0..self.set.len())] as usize;
        let (x, y) = (pixel % self.width, pixel / self.width);
        let u = (x as f32 + rng.random::<f32>()) / self.width as f32;
        let v = (y as f32 + rng.random::<f32>()) / self.height as f32;
        Vec2::new(
            (u * 2.0 - 1.0) * EMIT_MAP_HALF,
            (v * 2.0 - 1.0) * EMIT_MAP_HALF,
        )
    }
}

/// Which emit map a star kind uses (`distribute ... emit map`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmitMap {
    /// `0x0A256562`: base star map x clouds tile (yellow and red stars).
    StarClouds,
    /// `0xFF33EE2A`: base star map x darker tile (blue stars, disks).
    StarDarker,
    /// `0xB6412D00`: the dust lane map (black holes, binaries).
    Dust,
}

impl EmitMap {
    pub fn of(kind: StarKind) -> Self {
        match kind {
            StarKind::Yellow | StarKind::Red => EmitMap::StarClouds,
            StarKind::Blue | StarKind::ProtoPlanetary => EmitMap::StarDarker,
            _ => EmitMap::Dust,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StarMaps {
    pub star: BitImage,
    pub clouds: BitImage,
    pub darker: BitImage,
    pub dust: BitImage,
    /// True when these are the installed images, not the analytic fallback.
    pub installed: bool,
}

impl StarMaps {
    /// Load the four installed images; `read` looks up (group, instance, type).
    pub fn from_install(read: impl Fn(u32, u32, u32) -> Option<Vec<u8>>) -> Option<Self> {
        let image = |name: &str| -> Option<BitImage> {
            let id = crate::world_objects::name_id(name);
            BitImage::parse(&read(0, id, BIT_IMAGE_TYPE)?)
        };
        Some(Self {
            star: image("SG_base_star_map")?,
            clouds: image("SG_noise_tile_clouds")?,
            darker: image("SG_noise_tile_darker")?,
            dust: image("SG_dustlane_map")?,
            installed: true,
        })
    }

    /// ADAPTED: a procedural stand-in with the same structure (two-arm
    /// logarithmic spiral, tiled cloud patches, thin dust lanes) for headless
    /// runs without the installed game.
    pub fn analytic() -> Self {
        let size = 512usize;
        let arm = |p: Vec2, offset: f32| -> f32 {
            let r = p.length() / EMIT_MAP_HALF;
            if r > 0.72 {
                return 0.0;
            }
            let theta = p.y.atan2(p.x);
            let phase = (r.max(0.02).ln() / 0.32 - theta + offset) * 2.0;
            let arm = (0.5 + 0.5 * phase.cos()).powf(3.0);
            let bulge = (-r * r / 0.012).exp();
            (arm * (1.0 - r / 0.72).powf(0.6) * 0.85 + bulge).min(1.0)
        };
        let pos = |x: usize, y: usize, n: usize| {
            Vec2::new(
                ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0) * EMIT_MAP_HALF,
                ((y as f32 + 0.5) / n as f32 * 2.0 - 1.0) * EMIT_MAP_HALF,
            )
        };
        let hash = |x: usize, y: usize, salt: u64| -> f32 {
            super::unit(super::mix(salt, (x as u64) << 20 | y as u64))
        };
        let star = BitImage::from_fn(size, size, |x, y| hash(x, y, 1) < arm(pos(x, y, size), 0.0) * 0.4);
        let tile = 128usize;
        let clouds = BitImage::from_fn(tile, tile, |x, y| hash(x / 3, y / 3, 2) < 0.31);
        let darker = BitImage::from_fn(tile, tile, |x, y| hash(x / 4, y / 4, 3) < 0.04);
        let dust = BitImage::from_fn(size, size, |x, y| {
            hash(x, y, 4) < arm(pos(x, y, size), 0.9) * 0.2
        });
        Self {
            star,
            clouds,
            darker,
            dust,
            installed: false,
        }
    }

    /// The set pixels of an emit map.
    pub fn emit_pixels(&self, map: EmitMap) -> EmitPixels {
        if map == EmitMap::Dust {
            let d = &self.dust;
            let mut set = Vec::new();
            for y in 0..d.height {
                for x in 0..d.width {
                    if d.get(x, y) {
                        set.push((y * d.width + x) as u32);
                    }
                }
            }
            return EmitPixels {
                width: d.width,
                height: d.height,
                set,
            };
        }
        let tile = if map == EmitMap::StarClouds {
            &self.clouds
        } else {
            &self.darker
        };
        let s = &self.star;
        // Tile pixel of every star-map column / row: the noise tile repeats
        // every 200 units over the +-1000 rectangle.
        let lookup = |n: usize, tile_n: usize| -> Vec<usize> {
            (0..n)
                .map(|i| {
                    let world = (i as f32 + 0.5) / n as f32 * 2.0 * EMIT_MAP_HALF - EMIT_MAP_HALF;
                    let within = world.rem_euclid(NOISE_TILE) / NOISE_TILE;
                    ((within * tile_n as f32) as usize).min(tile_n - 1)
                })
                .collect()
        };
        let tx = lookup(s.width, tile.width);
        let ty = lookup(s.height, tile.height);
        let mut set = Vec::new();
        for y in 0..s.height {
            for x in 0..s.width {
                if s.get(x, y) && tile.get(tx[x], ty[y]) {
                    set.push((y * s.width + x) as u32);
                }
            }
        }
        EmitPixels {
            width: s.width,
            height: s.height,
            set,
        }
    }
}

// ---------------------------------------------------------------------------
// Sector grid (cStarManager::GetStarGridPosition 0xBA7250)
// ---------------------------------------------------------------------------

/// Stars bucketed into the retail 64 x 64 sector grid (cell = 21.875 units),
/// so range queries and nearest-star searches do not scan the whole galaxy.
#[derive(Clone, Debug, Default)]
pub struct SectorGrid {
    starts: Vec<u32>,
    items: Vec<u32>,
}

pub const SECTOR_SIZE: f32 = GALAXY_GRID_SIZE / GALAXY_SECTORS as f32;

impl SectorGrid {
    /// `round(x * 64 / 1400) + 32`, clamped to the grid.
    pub fn cell_of(position: Vec2) -> (usize, usize) {
        let cell = |v: f32| -> usize {
            let i = (v * (GALAXY_SECTORS as f32 / GALAXY_GRID_SIZE)).round() as i32 + 32;
            i.clamp(0, GALAXY_SECTORS as i32 - 1) as usize
        };
        (cell(position.x), cell(position.y))
    }

    pub fn build(count: usize, position: impl Fn(u32) -> Vec2) -> Self {
        let cells = GALAXY_SECTORS * GALAXY_SECTORS;
        let mut counts = vec![0u32; cells + 1];
        let mut cell_of_star = Vec::with_capacity(count);
        for i in 0..count {
            let (cx, cy) = Self::cell_of(position(i as u32));
            let c = cy * GALAXY_SECTORS + cx;
            cell_of_star.push(c as u32);
            counts[c + 1] += 1;
        }
        for c in 0..cells {
            counts[c + 1] += counts[c];
        }
        let starts = counts.clone();
        let mut fill = counts;
        let mut items = vec![0u32; count];
        for (i, c) in cell_of_star.iter().enumerate() {
            items[fill[*c as usize] as usize] = i as u32;
            fill[*c as usize] += 1;
        }
        Self { starts, items }
    }

    pub fn cell_items(&self, cx: usize, cy: usize) -> &[u32] {
        let c = cy * GALAXY_SECTORS + cx;
        match (self.starts.get(c), self.starts.get(c + 1)) {
            (Some(a), Some(b)) => &self.items[*a as usize..*b as usize],
            _ => &[],
        }
    }

    /// Call `f(index, distance)` for every star within `radius` of `center`.
    pub fn for_each_within(
        &self,
        position: &impl Fn(u32) -> Vec2,
        center: Vec2,
        radius: f32,
        mut f: impl FnMut(u32, f32),
    ) {
        let (x0, y0) = Self::cell_of(center - Vec2::splat(radius));
        let (x1, y1) = Self::cell_of(center + Vec2::splat(radius));
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                for &i in self.cell_items(cx, cy) {
                    let d = position(i).distance(center);
                    if d <= radius {
                        f(i, d);
                    }
                }
            }
        }
    }

    /// The closest star to `point` accepted by `accept`, within `max_distance`.
    pub fn nearest(
        &self,
        position: &impl Fn(u32) -> Vec2,
        point: Vec2,
        max_distance: f32,
        accept: impl Fn(u32) -> bool,
    ) -> Option<(u32, f32)> {
        let (cx, cy) = Self::cell_of(point);
        let mut best: Option<(u32, f32)> = None;
        let max_ring = ((max_distance / SECTOR_SIZE).ceil() as usize + 1).min(GALAXY_SECTORS);
        for ring in 0..=max_ring {
            if let Some((_, d)) = best {
                if (ring as f32 - 1.0) * SECTOR_SIZE > d {
                    break;
                }
            }
            let lo_x = cx.saturating_sub(ring);
            let hi_x = (cx + ring).min(GALAXY_SECTORS - 1);
            let lo_y = cy.saturating_sub(ring);
            let hi_y = (cy + ring).min(GALAXY_SECTORS - 1);
            for y in lo_y..=hi_y {
                for x in lo_x..=hi_x {
                    // Only the ring's own cells (the inner ones were visited).
                    let on_ring = x == lo_x && cx >= ring
                        || x == hi_x && cx + ring < GALAXY_SECTORS
                        || y == lo_y && cy >= ring
                        || y == hi_y && cy + ring < GALAXY_SECTORS
                        || ring == 0;
                    if !on_ring {
                        continue;
                    }
                    for &i in self.cell_items(x, y) {
                        if !accept(i) {
                            continue;
                        }
                        let d = position(i).distance(point);
                        if d <= max_distance && best.is_none_or(|(_, bd)| d < bd) {
                            best = Some((i, d));
                        }
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_matches_the_retail_cell_formula() {
        assert_eq!(SectorGrid::cell_of(Vec2::ZERO), (32, 32));
        assert_eq!(SectorGrid::cell_of(Vec2::new(-700.0, 700.0)), (0, 63));
        assert_eq!(SectorGrid::cell_of(Vec2::new(9999.0, -9999.0)), (63, 0));
        // 21.875 units per cell, cells centred on multiples of it.
        assert_eq!(SectorGrid::cell_of(Vec2::new(10.0, 11.0)), (32, 33));
    }

    #[test]
    fn nearest_and_range_queries_agree_with_a_scan() {
        let positions: Vec<Vec2> = (0..4000u64)
            .map(|i| {
                Vec2::new(
                    (super::super::unit(super::super::mix(1, i)) - 0.5) * 1300.0,
                    (super::super::unit(super::super::mix(2, i)) - 0.5) * 1300.0,
                )
            })
            .collect();
        let position = |i: u32| positions[i as usize];
        let grid = SectorGrid::build(positions.len(), position);
        for probe in [Vec2::new(0.0, 0.0), Vec2::new(300.0, -250.0), Vec2::new(-640.0, 641.0)] {
            let scan = (0..positions.len() as u32)
                .min_by(|a, b| {
                    position(*a)
                        .distance(probe)
                        .total_cmp(&position(*b).distance(probe))
                })
                .unwrap();
            let found = grid.nearest(&position, probe, 2000.0, |_| true).unwrap();
            assert_eq!(found.0, scan);
            let mut inside = 0;
            grid.for_each_within(&position, probe, 60.0, |_, _| inside += 1);
            let expected = positions.iter().filter(|p| p.distance(probe) <= 60.0).count();
            assert_eq!(inside, expected);
        }
    }

    #[test]
    fn analytic_maps_have_spiral_density_and_tile_coverage() {
        let maps = StarMaps::analytic();
        assert!(!maps.installed);
        let star_fraction = maps.star.count_set() as f32 / (maps.star.width * maps.star.height) as f32;
        assert!((0.01..0.2).contains(&star_fraction), "{star_fraction}");
        let emit = maps.emit_pixels(EmitMap::StarClouds);
        assert!(emit.set.len() > 100);
        assert!(!maps.emit_pixels(EmitMap::StarDarker).set.is_empty());
        assert!(!maps.emit_pixels(EmitMap::Dust).set.is_empty());
    }
}
