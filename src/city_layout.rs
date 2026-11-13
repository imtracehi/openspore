//! The retail city layout: the authored slots of a city's walls model.
//!
//! RETAIL (installed data, `notes/civ-slots-notes.md`): a city's walls are
//! the PROP `ci_<style>_citywalls_<small|large>` of the group
//! `citystyles_models~` (0x40828100). Besides the model keys it carries the
//! city's whole layout in the model's own frame (Spore model space, Z up):
//! `City_Hall` (slot 0), `Buildings` (slots 1..), `Turrets`, `Decorations`,
//! `Plazas`, the five gates (`Main_Gate`, `Side_Gates`, `Sea_Gate`,
//! `Harvest_Gate`, `Freight_Gate`), `BuildingLink0..` (the slots' adjacency
//! matrix), `radiusInner` / `radiusOuter` and `cityHallDiasHeight`. The code
//! reads them in `cCityWalls::Load` (0xBEE5A0: Turrets 0x43C4E1F, Main_Gate
//! 0x43C4E76, Side_Gates 0x43C4E68, Sea_Gate 0x43C4E96, Freight_Gate
//! 0x43C4E94, Harvest_Gate 0x43C4E95, City_Hall 0x43C4E97 pushed first into
//! `mBuildings`, then Buildings 0x43C4E98, Plazas 0x44041E3, Decorations
//! 0x446FF4E, `BuildingLink0..13` rows of the 14 x 14 matrix at +0x274, the
//! radii 0x4408ED6 / 7, the dais height 0x5EF5F2D) and `cCityWalls::Init`
//! (0xBEECC0) maps them through the city's position and heading
//! (0xBECC60). `MAX_LAYOUT_SLOTS` (14, ModAPI `cCityWalls.h`) is the matrix
//! size; the installed walls hold 6 slots (the hall + 5 buildings: the
//! *small* walls) or 12 (the hall + 11: the *large* walls).
//!
//! Which walls a city gets (RETAIL): `cCity::Init` (0xBE7D40) sets
//! `field_519` (+0x519) to 1 when the city's civilization is neutral (an NPC
//! civilization: the Civ level setup copies the slot's NPC flag +0x25 to
//! `cCivilization::mIsNeutral` +0x8A at 0xB99B8E; the Civ branch of
//! `cCivilization::Init` 0xBF82B0 stores `!playerOwned`) and then, once the
//! position is known (0xBE8142..0xBE815F), `cCityWalls::SetWallSize`
//! (vtable +0x68, 0xBECC10) with `(field_519 == 0) + 2` near water (the
//! `0xB8BCC0(position, 70.0)` coast test: the seaport models) and
//! `field_519 == 0` inland, i.e. 0 / 1 / 2 / 3 = inland small / inland large /
//! coastal small / coastal large. The models are loaded from the city style
//! property list `citystyles~` (0x02E4B4CC): `InlandModelSmall` 0x44449BA,
//! `InlandModelLarge` 0x44449BB, `CoastalModelSmall` 0x44449BC,
//! `CoastalModelLarge` 0x44449BD (`cCityWalls::SetStyle` 0xBEDFB0, and the
//! planet-wide preload 0xBED5B0 that picks small for an NPC slot and large for
//! the player's, coastal by the slot's water flag). So the player's city
//! (slot 0) stands behind the large walls and every rival city behind the
//! small ones, whatever their owner becomes. The coastal (`_seaport`) models
//! carry the same layout as their inland twins (checked for every style).
//!
//! The turret count of a style is the retail turret cap of
//! `cCity::GetMaxTurrets` (0xBD8BF0): castle (military) 8 / 5, shrine
//! (religious) 6 / 4, diplo (economic) 4 / 3 for large / small walls.
//!
//! ADAPTED: OpenSpore draws the walls at `WALL_EXTENT` city units across the
//! small wall's outer diameter (`LAYOUT_SCALE` city units per model unit,
//! uniform: the large walls are as much bigger as the models are), and the
//! gates, plazas and the hall's dais are kept as data only.

use bevy::prelude::Vec2;

use crate::civ::Approach;

mod data;

/// `cCityWalls.h` `MAX_LAYOUT_SLOTS`: rows and columns of the link matrix.
pub const MAX_LAYOUT_SLOTS: usize = 14;

/// ADAPTED: the small walls' outer diameter (2 x `radiusOuter` 48.5) in the
/// city's own units: the `CITY_WALL_EXTENT` the earlier fitted shell used
/// (the installed mesh spans about 97 units).
pub const WALL_EXTENT: f32 = 11.5;
/// City units per model unit: `WALL_EXTENT` over the small walls' outer
/// diameter. Every layout position is multiplied by it.
pub const LAYOUT_SCALE: f32 = WALL_EXTENT / (2.0 * 48.5);

/// One walls model's authored layout (all positions in model space).
#[derive(Debug, PartialEq)]
pub struct WallLayout {
    /// Installed PROP instance name (group `citystyles_models~`).
    pub name: &'static str,
    /// Its instance id.
    pub instance: u32,
    /// `City_Hall`: slot 0.
    pub hall: [f32; 3],
    /// `Buildings`: slots 1...
    pub buildings: &'static [[f32; 3]],
    /// `Turrets`: the wall guns' mounts.
    pub turrets: &'static [[f32; 3]],
    /// `Decorations`: the civic objects' spots.
    pub decorations: &'static [[f32; 3]],
    /// `Plazas`.
    pub plazas: &'static [[f32; 3]],
    pub main_gate: [f32; 3],
    pub side_gates: &'static [[f32; 3]],
    pub sea_gate: [f32; 3],
    pub freight_gate: [f32; 3],
    pub harvest_gate: [f32; 3],
    /// `BuildingLink0..`: the slots' adjacency matrix (row `i` = slot `i`).
    pub links: &'static [&'static [u8]],
    pub inner_radius: f32,
    pub outer_radius: f32,
    /// `cityHallDiasHeight`.
    pub dias_height: f32,
}

impl WallLayout {
    /// Slots of the city including the hall (6 small, 12 large).
    pub fn slots(&self) -> usize {
        1 + self.buildings.len()
    }

    /// Building slots (what `CityState::plots` holds).
    pub fn plots(&self) -> usize {
        self.buildings.len()
    }

    /// Are slots `a` and `b` linked (slot 0 is the hall)?
    pub fn linked(&self, a: usize, b: usize) -> bool {
        self.links.get(a).and_then(|row| row.get(b)).is_some_and(|v| *v != 0)
    }

    /// Every linked pair `(a, b)`, `a < b`, in slot order.
    pub fn link_pairs(&self) -> Vec<(usize, usize)> {
        let n = self.slots();
        let mut pairs = Vec::new();
        for a in 0..n {
            for b in a + 1..n {
                if self.linked(a, b) || self.linked(b, a) {
                    pairs.push((a, b));
                }
            }
        }
        pairs
    }

    /// City-local XZ of building plot `plot` (slot `plot + 1`).
    pub fn plot_site(&self, plot: usize) -> Vec2 {
        site(self.buildings[plot % self.buildings.len()])
    }

    /// City-local XZ of the hall (slot 0).
    pub fn hall_site(&self) -> Vec2 {
        site(self.hall)
    }

    /// City-local XZ of wall turret `turret`.
    pub fn turret_site(&self, turret: usize) -> Vec2 {
        site(self.turrets[turret % self.turrets.len()])
    }

    /// Height of turret `turret`'s mount above the city floor, city units.
    pub fn turret_height(&self, turret: usize) -> f32 {
        self.turrets[turret % self.turrets.len()][2] * LAYOUT_SCALE
    }

    /// City-local XZ of decoration spot `spot`.
    pub fn decoration_site(&self, spot: usize) -> Vec2 {
        site(self.decorations[spot % self.decorations.len()])
    }

    /// Number of decoration spots.
    pub fn decoration_spots(&self) -> usize {
        self.decorations.len()
    }

    /// The turret cap of the walls (`GetMaxTurrets`, 0xBD8BF0).
    pub fn max_turrets(&self) -> usize {
        self.turrets.len()
    }
}

/// Model space (x, y, z up) to the city's XZ plane: the model importer maps
/// (x, y, z) to (x, z, -y) (`editor::assembled_mesh`), then the layout scale.
pub fn site(p: [f32; 3]) -> Vec2 {
    Vec2::new(p[0], -p[1]) * LAYOUT_SCALE
}

/// The default walls of a city type: castle (military), shrine (religious),
/// diplo (economic), small or large.
pub fn wall_layout(kind: Approach, large: bool) -> &'static WallLayout {
    match (kind, large) {
        (Approach::Military, false) => &data::CASTLE_SMALL,
        (Approach::Military, true) => &data::CASTLE_LARGE,
        (Approach::Religious, false) => &data::SHRINE_SMALL,
        (Approach::Religious, true) => &data::SHRINE_LARGE,
        (Approach::Economic, false) => &data::DIPLO_SMALL,
        (Approach::Economic, true) => &data::DIPLO_LARGE,
    }
}

/// All six default layouts (tests, tools).
pub fn all_layouts() -> [&'static WallLayout; 6] {
    [
        &data::CASTLE_SMALL,
        &data::CASTLE_LARGE,
        &data::SHRINE_SMALL,
        &data::SHRINE_LARGE,
        &data::DIPLO_SMALL,
        &data::DIPLO_LARGE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installed_walls_hold_six_or_twelve_slots() {
        for layout in all_layouts() {
            let large = layout.name.contains("large");
            assert_eq!(layout.slots(), if large { 12 } else { 6 }, "{}", layout.name);
            assert!(layout.slots() <= MAX_LAYOUT_SLOTS);
            assert_eq!(layout.links.len(), layout.slots());
            assert!(layout.links.iter().all(|row| row.len() == layout.slots()));
            assert_eq!(layout.decoration_spots(), if large { 30 } else { 20 });
        }
    }

    #[test]
    fn the_link_matrix_is_symmetric_and_connects_the_hall() {
        for layout in all_layouts() {
            for a in 0..layout.slots() {
                assert!(!layout.linked(a, a), "{}: slot {a} links itself", layout.name);
                for b in 0..layout.slots() {
                    assert_eq!(layout.linked(a, b), layout.linked(b, a), "{}: {a}/{b}", layout.name);
                }
            }
            // Every slot reaches the hall through links (a connected graph).
            let mut seen = vec![false; layout.slots()];
            let mut work = vec![0usize];
            while let Some(a) = work.pop() {
                if std::mem::replace(&mut seen[a], true) {
                    continue;
                }
                work.extend((0..layout.slots()).filter(|b| layout.linked(a, *b)));
            }
            assert!(seen.iter().all(|s| *s), "{}: a slot is cut off", layout.name);
            // The hall links four slots (five behind the large diplo walls).
            let hall_links = (1..layout.slots()).filter(|b| layout.linked(0, *b)).count();
            assert!((4..=5).contains(&hall_links), "{}: {hall_links}", layout.name);
        }
    }

    #[test]
    fn the_turret_caps_are_the_retail_ones() {
        // 0xBD8BF0: 8 / 6 / 4 (5 / 4 / 3 for the small walls of the NPC cities).
        for (kind, large, small) in [
            (Approach::Military, 8, 5),
            (Approach::Religious, 6, 4),
            (Approach::Economic, 4, 3),
        ] {
            assert_eq!(wall_layout(kind, true).max_turrets(), large);
            assert_eq!(wall_layout(kind, false).max_turrets(), small);
        }
    }

    #[test]
    fn slots_stand_inside_the_inner_radius_and_turrets_on_the_wall() {
        for layout in all_layouts() {
            for b in layout.buildings {
                let r = (b[0] * b[0] + b[1] * b[1]).sqrt();
                assert!(r < layout.inner_radius, "{}: building at {r}", layout.name);
            }
            for t in layout.turrets {
                let r = (t[0] * t[0] + t[1] * t[1]).sqrt();
                // The mounts sit on the wall ring (the small diplo walls' lowest at 37.1).
                assert!(r > layout.inner_radius - 3.0 && r < layout.outer_radius + 2.0, "{}: turret at {r}", layout.name);
            }
        }
    }

    #[test]
    fn the_model_frame_maps_to_the_city_plane() {
        // Spore (x, y) -> city (x, -y) at the layout scale.
        let s = site([10.0, 20.0, 5.0]);
        assert!((s.x - 10.0 * LAYOUT_SCALE).abs() < 1e-6 && (s.y + 20.0 * LAYOUT_SCALE).abs() < 1e-6);
        // The small walls fill `WALL_EXTENT` city units.
        assert!((2.0 * 48.5 * LAYOUT_SCALE - WALL_EXTENT).abs() < 1e-5);
    }
}
