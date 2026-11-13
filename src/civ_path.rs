//! Path finding for Civilization vehicles on the planet: land vehicles go
//! around water and cliffs, sea vehicles keep to the water along the coasts.
//!
//! ADAPTED (the retail vehicle path planner was not traced): an A* search
//! over a latitude / longitude grid of the planet sphere (2-degree cells,
//! about 17 units at R = 500). A land cell is dry ground (`IsInWater` false);
//! a step between land cells is refused when the slope between their ground
//! heights exceeds `AnimalStageMaxSlope` 30 degrees (the Civ tuning's slope
//! limit, used here for vehicles). Water cells are the sea vehicles'.
#![allow(dead_code)]

use bevy::prelude::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::civ_planet::Globe;

/// Grid size: rows of latitude and columns of longitude.
pub const ROWS: usize = 90;
pub const COLS: usize = 180;
/// ADAPTED: the steepest step a land vehicle takes (degrees).
pub const MAX_SLOPE_DEG: f32 = crate::civ_rules::ANIMAL_STAGE_MAX_SLOPE;

/// What each cell is.
#[derive(Clone, Debug)]
pub struct NavGrid {
    pub centre: Vec3,
    pub radius: f32,
    land: Vec<bool>,
    altitude: Vec<f32>,
}

fn index(r: usize, c: usize) -> usize {
    r * COLS + c
}

/// Unit direction of a cell centre.
fn cell_dir(r: usize, c: usize) -> Vec3 {
    let lat = -std::f32::consts::FRAC_PI_2 + (r as f32 + 0.5) * std::f32::consts::PI / ROWS as f32;
    let lon = (c as f32 + 0.5) * std::f32::consts::TAU / COLS as f32;
    Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin())
}

impl NavGrid {
    /// Sample the planet (none without one).
    pub fn build(globe: &Globe) -> Option<Self> {
        let shape = globe.shape();
        let centre = shape.centre?;
        let radius = shape.radius;
        let mut land = vec![false; ROWS * COLS];
        let mut altitude = vec![0.0; ROWS * COLS];
        for r in 0..ROWS {
            for c in 0..COLS {
                let p = centre + cell_dir(r, c) * radius;
                land[index(r, c)] = globe.is_land(p);
                altitude[index(r, c)] = globe.altitude(p);
            }
        }
        Some(Self { centre, radius, land, altitude })
    }

    /// A grid from given cells (tests).
    pub fn from_cells(centre: Vec3, radius: f32, land: Vec<bool>, altitude: Vec<f32>) -> Self {
        Self { centre, radius, land, altitude }
    }

    /// The cell of a world point.
    pub fn cell(&self, p: Vec3) -> (usize, usize) {
        let d = (p - self.centre).normalize_or(Vec3::Y);
        let lat = d.y.clamp(-1.0, 1.0).asin();
        let lon = d.z.atan2(d.x).rem_euclid(std::f32::consts::TAU);
        let r = (((lat + std::f32::consts::FRAC_PI_2) / std::f32::consts::PI) * ROWS as f32) as usize;
        let c = ((lon / std::f32::consts::TAU) * COLS as f32) as usize;
        (r.min(ROWS - 1), c % COLS)
    }

    /// The world point of a cell centre on the sphere.
    pub fn point(&self, r: usize, c: usize) -> Vec3 {
        self.centre + cell_dir(r, c) * self.radius
    }

    fn passable(&self, r: usize, c: usize, land: bool) -> bool {
        self.land[index(r, c)] == land
    }

    fn step_ok(&self, a: (usize, usize), b: (usize, usize), land: bool, length: f32) -> bool {
        if !self.passable(b.0, b.1, land) {
            return false;
        }
        if !land {
            return true;
        }
        let rise = (self.altitude[index(b.0, b.1)] - self.altitude[index(a.0, a.1)]).abs();
        rise <= length * MAX_SLOPE_DEG.to_radians().tan()
    }

    fn neighbours(&self, r: usize, c: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        let rows = [r as isize - 1, r as isize, r as isize + 1];
        rows.into_iter().flat_map(move |rr| {
            [-1isize, 0, 1].into_iter().filter_map(move |dc| {
                if rr < 0 || rr >= ROWS as isize || (rr == r as isize && dc == 0) {
                    return None;
                }
                Some((rr as usize, ((c as isize + dc).rem_euclid(COLS as isize)) as usize))
            })
        })
    }

    /// The nearest passable cell to `(r, c)` within a few rings.
    fn nearest_passable(&self, r: usize, c: usize, land: bool) -> Option<(usize, usize)> {
        if self.passable(r, c, land) {
            return Some((r, c));
        }
        for ring in 1..=4isize {
            for dr in -ring..=ring {
                for dc in -ring..=ring {
                    let rr = r as isize + dr;
                    if rr < 0 || rr >= ROWS as isize {
                        continue;
                    }
                    let cc = (c as isize + dc).rem_euclid(COLS as isize) as usize;
                    if self.passable(rr as usize, cc, land) {
                        return Some((rr as usize, cc));
                    }
                }
            }
        }
        None
    }

    /// Connected-region labels of the grid: every group of land cells (`land`) or
    /// of water cells (not `land`) joined by an 8-neighbour step (wrapping in
    /// longitude) gets one number; 0 marks the other medium. ADAPTED stand-in
    /// for the retail landmass index (0xB886E0), which `C9EAF0` compares between
    /// a vehicle and the point under the cursor: the grid is 2 degrees coarse
    /// and ignores the slope limit.
    pub fn regions(&self, land: bool) -> Vec<u32> {
        let mut label = vec![0u32; ROWS * COLS];
        let mut next = 0u32;
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for r in 0..ROWS {
            for c in 0..COLS {
                if label[index(r, c)] != 0 || !self.passable(r, c, land) {
                    continue;
                }
                next += 1;
                label[index(r, c)] = next;
                stack.push((r, c));
                while let Some((cr, cc)) = stack.pop() {
                    for (nr, nc) in self.neighbours(cr, cc) {
                        if label[index(nr, nc)] == 0 && self.passable(nr, nc, land) {
                            label[index(nr, nc)] = next;
                            stack.push((nr, nc));
                        }
                    }
                }
            }
        }
        label
    }

    /// The region number (from [`NavGrid::regions`]) of the cell nearest to `p`
    /// on the vehicle's medium; 0 when there is none within a few cells.
    pub fn region_at(&self, labels: &[u32], p: Vec3, land: bool) -> u32 {
        let (r, c) = self.cell(p);
        self.nearest_passable(r, c, land).map_or(0, |(r, c)| labels[index(r, c)])
    }

    /// Cell waypoints from `from` to `to` for a land (`land`) or sea vehicle,
    /// ending at `to` itself; `None` when no way exists.
    pub fn find(&self, from: Vec3, to: Vec3, land: bool) -> Option<Vec<Vec3>> {
        let (sr, sc) = self.cell(from);
        let start = self.nearest_passable(sr, sc, land).unwrap_or((sr, sc));
        let (gr, gc) = self.cell(to);
        let goal = self.nearest_passable(gr, gc, land)?;
        if start == goal {
            return Some(vec![to]);
        }
        let dist = |a: (usize, usize), b: (usize, usize)| {
            cell_dir(a.0, a.1).angle_between(cell_dir(b.0, b.1)) * self.radius
        };
        #[derive(PartialEq)]
        struct Node(f32, usize);
        impl Eq for Node {}
        impl Ord for Node {
            fn cmp(&self, o: &Self) -> Ordering {
                o.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
            }
        }
        impl PartialOrd for Node {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }
        let n = ROWS * COLS;
        let mut g = vec![f32::INFINITY; n];
        let mut came = vec![usize::MAX; n];
        let mut open = BinaryHeap::new();
        let s = index(start.0, start.1);
        g[s] = 0.0;
        open.push(Node(dist(start, goal), s));
        let target = index(goal.0, goal.1);
        let mut expanded = 0;
        while let Some(Node(_, i)) = open.pop() {
            if i == target {
                break;
            }
            expanded += 1;
            if expanded > n {
                return None;
            }
            let here = (i / COLS, i % COLS);
            for nb in self.neighbours(here.0, here.1) {
                let length = dist(here, nb);
                if !self.step_ok(here, nb, land, length) {
                    continue;
                }
                let j = index(nb.0, nb.1);
                let cost = g[i] + length;
                if cost < g[j] {
                    g[j] = cost;
                    came[j] = i;
                    open.push(Node(cost + dist(nb, goal), j));
                }
            }
        }
        if came[target] == usize::MAX {
            return None;
        }
        let mut cells = vec![target];
        let mut k = target;
        while came[k] != usize::MAX && came[k] != s {
            k = came[k];
            cells.push(k);
        }
        cells.reverse();
        let mut out: Vec<Vec3> = cells.iter().map(|&i| self.point(i / COLS, i % COLS)).collect();
        out.pop();
        out.push(to);
        Some(out)
    }
}

/// Whether the great circle from `a` to `b` stays on the vehicle's medium
/// (samples every `step` units).
pub fn clear(globe: &Globe, a: Vec3, b: Vec3, land: bool, step: f32) -> bool {
    if !globe.on_planet() {
        return true;
    }
    let shape = globe.shape();
    let length = shape.distance(a, b);
    let n = (length / step.max(1.0)).ceil().max(1.0) as usize;
    let dir = b - a;
    (1..n).all(|i| globe.is_land(shape.toward(a, dir, length * i as f32 / n as f32)) == land)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A planet of land with a sea band across the equator except a land
    /// bridge at longitude cells 10..12.
    fn grid() -> NavGrid {
        let mut land = vec![true; ROWS * COLS];
        for c in 0..COLS {
            if !(10..12).contains(&c) {
                for r in 43..47 {
                    land[index(r, c)] = false;
                }
            }
        }
        NavGrid::from_cells(Vec3::ZERO, 500.0, land, vec![0.0; ROWS * COLS])
    }

    #[test]
    fn land_paths_go_around_the_sea_and_ships_stay_in_it() {
        let g = grid();
        let from = g.point(40, 60);
        let to = g.point(50, 60);
        let path = g.find(from, to, true).expect("over the land bridge");
        // The way passes the bridge columns while crossing the band.
        assert!(path.iter().any(|p| {
            let (r, c) = g.cell(*p);
            (43..47).contains(&r) && (10..12).contains(&c)
        }));
        assert!(path.iter().all(|p| {
            let (r, c) = g.cell(*p);
            g.land[index(r, c)]
        }));
        assert_eq!(*path.last().unwrap(), to);
        // A ship along the band.
        let sea = g.find(g.point(45, 20), g.point(45, 100), false).expect("along the coast");
        assert!(sea.iter().all(|p| !g.land[index(g.cell(*p).0, g.cell(*p).1)] || *p == g.point(45, 100)));
        // No way for a ship between two inland points.
        assert!(g.find(g.point(20, 20), g.point(70, 20), false).is_none());
    }

    #[test]
    fn cliffs_block_land_steps() {
        let mut altitude = vec![0.0; ROWS * COLS];
        // A wall of cliffs at row 50 except column 0.
        for c in 1..COLS {
            altitude[index(50, c)] = 200.0;
        }
        let g = NavGrid::from_cells(Vec3::ZERO, 500.0, vec![true; ROWS * COLS], altitude);
        let path = g.find(g.point(45, 90), g.point(55, 90), true).expect("around the cliffs");
        assert!(path.iter().all(|p| {
            let (r, c) = g.cell(*p);
            r != 50 || c == 0
        }));
    }

    #[test]
    fn regions_split_the_land_at_the_sea_band_and_join_over_the_bridge() {
        let g = grid();
        let land = g.regions(true);
        // The bridge joins the two halves into one region.
        assert_eq!(g.region_at(&land, g.point(20, 60), true), g.region_at(&land, g.point(70, 60), true));
        assert_ne!(g.region_at(&land, g.point(20, 60), true), 0);
        // Without the bridge the halves are two regions.
        let mut cells = vec![true; ROWS * COLS];
        for c in 0..COLS {
            for r in 43..47 {
                cells[index(r, c)] = false;
            }
        }
        let split = NavGrid::from_cells(Vec3::ZERO, 500.0, cells, vec![0.0; ROWS * COLS]);
        let land = split.regions(true);
        assert_ne!(split.region_at(&land, split.point(20, 60), true), split.region_at(&land, split.point(70, 60), true));
        // The band is one body of water.
        let sea = split.regions(false);
        assert_eq!(split.region_at(&sea, split.point(45, 5), false), split.region_at(&sea, split.point(45, 150), false));
        assert_ne!(split.region_at(&sea, split.point(45, 5), false), 0);
    }
}
