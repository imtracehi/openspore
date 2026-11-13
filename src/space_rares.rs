//! The retail rare list, the rares placed on planets and the rare detector
//! (helper `space-final3`, `notes\space-final3-notes.md` R1-R3).
//!
//! Original data (VERIFIED, installed `spacetrading~` 0x034D97FA props): the
//! 150 items whose `spaceTradingType` 0x034F1A4F is `rare` 0x7ECBE6F5, with
//! `spaceTradingChance` 0x034F1A4E, `spaceTradingAbundance` 0x034F1A49,
//! `spaceTradingBaseCost` 0x034F1A4A, `spaceTradingRareGroupID` 0x03C72DC6
//! and the English text of `spaceToolDescription` 0x3068D95D (locale table
//! 0x5E6117CD); the 15 group items (type `group` 0xEB1A8062) name the sets.
//! The table keeps the install's data quirk: `rare-bardscroll10` is in group
//! 23 (Scrolls of Harmony).
//!
//! Original code (VERIFIED):
//! * `cStarManager::GeneratePlanetsForStar` 0xBB42A0: a star with flag 0x1000
//!   shuffles its planets and hands the first that is not a belt / gas giant
//!   to 0x103AA10.
//! * 0x103AA10 (Space mode, planet type T0..T3, not destroyed, the planet's
//!   empire is not the player's): passes over every rare, adding each whose
//!   draw is below its chance (count = rounded abundance) as a planet object
//!   0xC73340(type 4, flags 0x30: 0x10 marks the rare), until a pass added
//!   one. Galactic objects and story books have chance 0: never placed.
//! * 0xBB9EB0: the planet has a rare object (flag 0x10). 0xBBA160 (once per
//!   planet, flags 0x4000 / 0x8000): with a rare, `FalseNegativeRare`
//!   0x05B07982 (installed 0.0) flips the reading; without one, only on a
//!   creature planet (tech 1) of a star that is not an Empire star after the
//!   `SPG_RareClose` flag 0x04AC800C, `FalsePositiveRare` 0x05B0797F (0.05);
//!   every other first reading stays unflipped. 0xBBA270 (the detector) =
//!   rare XOR flipped.
//! * 0xFDFBB0 (planet hints): detector on and no rare -> queued
//!   `SPG_RareAmbush` (flag 0x05B15554), the ambush 0x10096B0 and 0xBB9CC0
//!   (the reading is reset to unflipped); a rare object on the planet ->
//!   queued `SPG_RareClose` (flag 0x04AC800C).
//! * `cSpaceTrading::ObtainTradingObject` 0x103FC20: a rare not found before
//!   adds badge progress `ReqRaresCollected`, marks it found (mRareMap = 1,
//!   message 0x6779F04) and logs event 0x554FE3DE.
//!
//! ADAPTED: the draws are hashes of the world id (retail: the main RNG at
//! planet generation); the rare planet is chosen among the star's worlds (moons
//! are not worlds here); one pickup per world (a second rare of the same pass
//! is dropped); the pickup is the tractor beam on the world (no surface
//! `cPlanetaryArtifact` model); the ambush is 3-5 pirate fighters at the world.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// One rare item of `spacetrading~`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rare {
    pub name: &'static str,
    pub group: u32,
    pub chance: f32,
    pub abundance: f32,
    pub cost: f32,
    pub title: &'static str,
}

/// A rare set (`spaceTradingType` group).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RareGroup {
    pub id: u32,
    pub name: &'static str,
    pub title: &'static str,
    pub cost: f32,
}

/// `spaceToolSetiChanceFalsePositiveRare` 0x05B0797F of the installed tool.
pub const FALSE_POSITIVE_RARE: f32 = 0.05;
/// `spaceToolSetiChanceFalseNegativeRare` 0x05B07982 of the installed tool.
pub const FALSE_NEGATIVE_RARE: f32 = 0.0;

pub const RARES: [Rare; 150] = [
    Rare { name: "rare-bardscroll01", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 1" },
    Rare { name: "rare-bardscroll02", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 2" },
    Rare { name: "rare-bardscroll03", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 3" },
    Rare { name: "rare-bardscroll04", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 4" },
    Rare { name: "rare-bardscroll05", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 5" },
    Rare { name: "rare-bardscroll06", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 6" },
    Rare { name: "rare-bardscroll07", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 7" },
    Rare { name: "rare-bardscroll08", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 8" },
    Rare { name: "rare-bardscroll09", group: 22, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 9" },
    Rare { name: "rare-bardscroll10", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Block of Chance Vol. 10" },
    Rare { name: "rare-diplomatscroll01", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order I" },
    Rare { name: "rare-diplomatscroll02", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order II" },
    Rare { name: "rare-diplomatscroll03", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order III" },
    Rare { name: "rare-diplomatscroll04", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order IV" },
    Rare { name: "rare-diplomatscroll05", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order V" },
    Rare { name: "rare-diplomatscroll06", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order VI" },
    Rare { name: "rare-diplomatscroll07", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order VII" },
    Rare { name: "rare-diplomatscroll08", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order VIII" },
    Rare { name: "rare-diplomatscroll09", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order IX" },
    Rare { name: "rare-diplomatscroll10", group: 21, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Order X" },
    Rare { name: "rare-ecologistscroll01", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 1" },
    Rare { name: "rare-ecologistscroll02", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 2" },
    Rare { name: "rare-ecologistscroll03", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 3" },
    Rare { name: "rare-ecologistscroll04", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 4" },
    Rare { name: "rare-ecologistscroll05", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 5" },
    Rare { name: "rare-ecologistscroll06", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 6" },
    Rare { name: "rare-ecologistscroll07", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 7" },
    Rare { name: "rare-ecologistscroll08", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 8" },
    Rare { name: "rare-ecologistscroll09", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 9" },
    Rare { name: "rare-ecologistscroll10", group: 26, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Life 10" },
    Rare { name: "rare-fossil1", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Ancient Flyswatter" },
    Rare { name: "rare-fossil10", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Fossilized Remains of a Big Scary Thing" },
    Rare { name: "rare-fossil2", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "The Fossil That Everybody Finds" },
    Rare { name: "rare-fossil3", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Fossilized Domesticated Animal Treat" },
    Rare { name: "rare-fossil4", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Fossilized Tribal Sheriff's Badge" },
    Rare { name: "rare-fossil5", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Ancient Epic Boot Scraper" },
    Rare { name: "rare-fossil6", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Petrified Sun Dried Nautilus" },
    Rare { name: "rare-fossil7", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Big Dead Fish on a Little Non Living Rock" },
    Rare { name: "rare-fossil8", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Fossilized Candy Corn" },
    Rare { name: "rare-fossil9", group: 2, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Mr. Fluffers" },
    Rare { name: "rare-galacticobject1", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Galactic Core" },
    Rare { name: "rare-galacticobject10", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Talked to Steve" },
    Rare { name: "rare-galacticobject2", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Black Hole" },
    Rare { name: "rare-galacticobject3", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Proto-Planetary Disk" },
    Rare { name: "rare-galacticobject4", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "BinaryOO" },
    Rare { name: "rare-galacticobject5", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "BinaryOG" },
    Rare { name: "rare-galacticobject6", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "BinaryOM" },
    Rare { name: "rare-galacticobject7", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "BinaryGG" },
    Rare { name: "rare-galacticobject8", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "BinaryGM" },
    Rare { name: "rare-galacticobject9", group: 8, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "BinaryMM" },
    Rare { name: "rare-geode1", group: 4, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "A Yellow Geode" },
    Rare { name: "rare-geode10", group: 4, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "A Shiny Geode" },
    Rare { name: "rare-geode2", group: 4, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "A Green Geode" },
    Rare { name: "rare-geode3", group: 4, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "A Pink Geode" },
    Rare { name: "rare-geode4", group: 4, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "A Blue Geode" },
    Rare { name: "rare-geode5", group: 4, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "A Fuchsia Geode" },
    Rare { name: "rare-geode6", group: 4, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "An Orange Geode" },
    Rare { name: "rare-geode7", group: 4, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "A Golden Geode" },
    Rare { name: "rare-geode8", group: 4, chance: 0.003, abundance: 1.0, cost: 90000.0, title: "An Aqua Geode" },
    Rare { name: "rare-geode9", group: 4, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "A Reddish Geode" },
    Rare { name: "rare-jewel1", group: 0, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "Purple-ish Gems of Rabban Ankott" },
    Rare { name: "rare-jewel10", group: 0, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "Turquoise" },
    Rare { name: "rare-jewel2", group: 0, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "Tangeriamonds" },
    Rare { name: "rare-jewel3", group: 0, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Amethysts" },
    Rare { name: "rare-jewel4", group: 0, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "Emeralds" },
    Rare { name: "rare-jewel5", group: 0, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "Jade" },
    Rare { name: "rare-jewel6", group: 0, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "Lapis Lazuli" },
    Rare { name: "rare-jewel7", group: 0, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Opals" },
    Rare { name: "rare-jewel8", group: 0, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "Rubies" },
    Rare { name: "rare-jewel9", group: 0, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "Sapphires" },
    Rare { name: "rare-relic1", group: 1, chance: 0.003, abundance: 1.0, cost: 225000.0, title: "Ancient Urn of the Spurg" },
    Rare { name: "rare-relic10", group: 1, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "The Lost Chest of Moozilla" },
    Rare { name: "rare-relic2", group: 1, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Super Old Clay Pot" },
    Rare { name: "rare-relic3", group: 1, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "Will's Old Sword" },
    Rare { name: "rare-relic4", group: 1, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "Scrolls of the Ancients" },
    Rare { name: "rare-relic5", group: 1, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "The History of Spore" },
    Rare { name: "rare-relic6", group: 1, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "Tablet of the Tribes" },
    Rare { name: "rare-relic7", group: 1, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "Screeble's Column" },
    Rare { name: "rare-relic8", group: 1, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "Sporehenge" },
    Rare { name: "rare-relic9", group: 1, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "The Mask of Todd" },
    Rare { name: "rare-scientistscroll01", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 1" },
    Rare { name: "rare-scientistscroll02", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 2" },
    Rare { name: "rare-scientistscroll03", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 3" },
    Rare { name: "rare-scientistscroll04", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 4" },
    Rare { name: "rare-scientistscroll05", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 5" },
    Rare { name: "rare-scientistscroll06", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 6" },
    Rare { name: "rare-scientistscroll07", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 7" },
    Rare { name: "rare-scientistscroll08", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 8" },
    Rare { name: "rare-scientistscroll09", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 9" },
    Rare { name: "rare-scientistscroll10", group: 24, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Book of Science Pt. 10" },
    Rare { name: "rare-shamanscroll01", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.1" },
    Rare { name: "rare-shamanscroll02", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.2" },
    Rare { name: "rare-shamanscroll03", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.3" },
    Rare { name: "rare-shamanscroll04", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.4" },
    Rare { name: "rare-shamanscroll05", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.5" },
    Rare { name: "rare-shamanscroll06", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.6" },
    Rare { name: "rare-shamanscroll07", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.7" },
    Rare { name: "rare-shamanscroll08", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.8" },
    Rare { name: "rare-shamanscroll09", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.9" },
    Rare { name: "rare-shamanscroll10", group: 23, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scroll of Harmony Vol.10" },
    Rare { name: "rare-statue1", group: 3, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "The Hand of Quimby" },
    Rare { name: "rare-statue10", group: 3, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "The Statue of The Three Crickets" },
    Rare { name: "rare-statue2", group: 3, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "The Bust of Yaman" },
    Rare { name: "rare-statue3", group: 3, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "The Bust of Bradford" },
    Rare { name: "rare-statue4", group: 3, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "The Wayward Leg of Lamstein" },
    Rare { name: "rare-statue5", group: 3, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "The Stone of Pierre" },
    Rare { name: "rare-statue6", group: 3, chance: 0.003, abundance: 1.0, cost: 45000.0, title: "The Statue of One-Eyed Kippy" },
    Rare { name: "rare-statue7", group: 3, chance: 0.003, abundance: 1.0, cost: 67500.0, title: "The Torch of Chalmers" },
    Rare { name: "rare-statue8", group: 3, chance: 0.003, abundance: 1.0, cost: 22500.0, title: "The Upper Torso of Little Lee" },
    Rare { name: "rare-statue9", group: 3, chance: 0.003, abundance: 1.0, cost: 225000.0, title: "The Foot of Povey" },
    Rare { name: "rare-storybook1", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Crab Legs" },
    Rare { name: "rare-storybook10", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Rock Stack" },
    Rare { name: "rare-storybook2", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Craters" },
    Rare { name: "rare-storybook3", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Dill Pickles" },
    Rare { name: "rare-storybook4", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Gears n Cogs" },
    Rare { name: "rare-storybook5", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Marshmallows" },
    Rare { name: "rare-storybook6", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Pastry" },
    Rare { name: "rare-storybook7", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Pineapple" },
    Rare { name: "rare-storybook8", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Tentacles" },
    Rare { name: "rare-storybook9", group: 9, chance: 0.0, abundance: 1.0, cost: 112500.0, title: "Rattlesnake" },
    Rare { name: "rare-traderscroll01", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.1" },
    Rare { name: "rare-traderscroll02", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.2" },
    Rare { name: "rare-traderscroll03", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.3" },
    Rare { name: "rare-traderscroll04", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.4" },
    Rare { name: "rare-traderscroll05", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.5" },
    Rare { name: "rare-traderscroll06", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.6" },
    Rare { name: "rare-traderscroll07", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.7" },
    Rare { name: "rare-traderscroll08", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.8" },
    Rare { name: "rare-traderscroll09", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.9" },
    Rare { name: "rare-traderscroll10", group: 27, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Tablet of Prosperity No.10" },
    Rare { name: "rare-warriorscroll1", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.1" },
    Rare { name: "rare-warriorscroll10", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.10" },
    Rare { name: "rare-warriorscroll2", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.2" },
    Rare { name: "rare-warriorscroll3", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.3" },
    Rare { name: "rare-warriorscroll4", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.4" },
    Rare { name: "rare-warriorscroll5", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.5" },
    Rare { name: "rare-warriorscroll6", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.6" },
    Rare { name: "rare-warriorscroll7", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.7" },
    Rare { name: "rare-warriorscroll8", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.8" },
    Rare { name: "rare-warriorscroll9", group: 20, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Stone of Force Vol.9" },
    Rare { name: "rare-zealotscroll01", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 1" },
    Rare { name: "rare-zealotscroll02", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 2" },
    Rare { name: "rare-zealotscroll03", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 3" },
    Rare { name: "rare-zealotscroll04", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 4" },
    Rare { name: "rare-zealotscroll05", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 5" },
    Rare { name: "rare-zealotscroll06", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 6" },
    Rare { name: "rare-zealotscroll07", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 7" },
    Rare { name: "rare-zealotscroll08", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 8" },
    Rare { name: "rare-zealotscroll09", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 9" },
    Rare { name: "rare-zealotscroll10", group: 25, chance: 0.001, abundance: 1.0, cost: 45000.0, title: "Scrolls of Faith Book 10" },
];

pub const GROUPS: [RareGroup; 15] = [
    RareGroup { id: 0, name: "rare-jewelgroup", title: "Rare Jewels", cost: 4500000.0 },
    RareGroup { id: 1, name: "rare-relicgroup", title: "Rare Relics", cost: 6750000.0 },
    RareGroup { id: 2, name: "rare-fossilgroup", title: "Rare Fossils", cost: 4500000.0 },
    RareGroup { id: 3, name: "rare-statuegroup", title: "Rare Statues", cost: 4500000.0 },
    RareGroup { id: 4, name: "rare-geodegroup", title: "Rare Geodes", cost: 6075000.0 },
    RareGroup { id: 8, name: "rare-galacticobjectsgroup", title: "Galactic Objects", cost: 2475000.0 },
    RareGroup { id: 9, name: "rare-storybookgroup", title: "Story Book Planets", cost: 1125000.0 },
    RareGroup { id: 20, name: "rare-warriorscrollgroup", title: "Stones of Force", cost: 4500000.0 },
    RareGroup { id: 21, name: "rare-diplomatscrollgroup", title: "Scrolls of Order", cost: 4500000.0 },
    RareGroup { id: 22, name: "rare-bardscrollgroup", title: "Blocks of Chance", cost: 4500000.0 },
    RareGroup { id: 23, name: "rare-shamanscrollgroup", title: "Scrolls of Harmony", cost: 4500000.0 },
    RareGroup { id: 24, name: "rare-scientistscrollgroup", title: "Books of Science", cost: 4500000.0 },
    RareGroup { id: 25, name: "rare-zealotscrollgroup", title: "Scrolls of Faith", cost: 4500000.0 },
    RareGroup { id: 26, name: "rare-ecologistscrollgroup", title: "Stones of Life", cost: 4500000.0 },
    RareGroup { id: 27, name: "rare-traderscrollgroup", title: "Tablets of Prosperity", cost: 4500000.0 },
];

/// The rare item named `name` (`rare-jewel3`).
pub fn rare(name: &str) -> Option<&'static Rare> {
    RARES.iter().find(|r| r.name == name)
}

pub fn group(id: u32) -> Option<&'static RareGroup> {
    GROUPS.iter().find(|g| g.id == id)
}

/// The members of group `id` in the table.
pub fn members(id: u32) -> impl Iterator<Item = &'static Rare> {
    RARES.iter().filter(move |r| r.group == id)
}

/// 0x103AA10: passes over the rare list until a pass adds a rare; returns the
/// rares added by that pass with their counts. `draw` gives 0..1 numbers.
pub fn roll_planet_rares(mut draw: impl FnMut() -> f32) -> Vec<(&'static Rare, u32)> {
    if RARES.iter().all(|r| r.chance <= 0.0) {
        return Vec::new();
    }
    loop {
        let mut added = Vec::new();
        for r in RARES.iter() {
            if draw() < r.chance {
                added.push((r, r.abundance.round().max(0.0) as u32));
            }
        }
        if !added.is_empty() {
            return added;
        }
    }
}

/// A uniform 0..1 draw stream seeded by `seed`.
fn draws(seed: u64) -> impl FnMut() -> f32 {
    let mut i = 0u64;
    move || {
        i += 1;
        crate::space_worldgen::unit(crate::space_worldgen::mix(seed, i))
    }
}

/// The rare placed on world `world_id` (the first of its pass), if its star
/// carries one and this is the star's rare world.
pub fn world_rare(world_id: u32, has_rare: bool) -> Option<&'static Rare> {
    if !has_rare {
        return None;
    }
    roll_planet_rares(draws(0x5241_5245_0000_0000 ^ world_id as u64)).first().map(|(r, _)| *r)
}

/// The saved rare state of a campaign (`SpaceCampaign::rares`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RareState {
    /// Rares found (mRareMap = 1), by item name; selling keeps them found.
    pub found: BTreeSet<String>,
    /// The detector's one-time reading per world (0xBBA160 flags): true =
    /// flipped (0x8000), false = read (0x4000).
    pub detector: BTreeMap<u32, bool>,
    /// Groups whose every member was found (recorded for the collections view; retail pays no set bonus).
    pub completed_groups: BTreeSet<u32>,
    /// An ambush waiting for its fighters (the world it waits at).
    pub ambush: Option<u32>,
}

impl RareState {
    /// 0xBBA160 + 0xBBA270: the detector's reading of a world. `has_rare` =
    /// 0xBB9EB0; `creature_planet` = planet tech 1 in a star that is not an
    /// Empire star; `rare_close_seen` = the `SPG_RareClose` flag. The first
    /// reading is kept.
    pub fn detector(&mut self, world: u32, has_rare: bool, creature_planet: bool, rare_close_seen: bool) -> bool {
        let flipped = *self.detector.entry(world).or_insert_with(|| {
            let roll = crate::space_worldgen::unit(crate::space_worldgen::mix(0xDE7E_C70A, world as u64));
            if has_rare {
                roll < FALSE_NEGATIVE_RARE
            } else if creature_planet && rare_close_seen {
                roll < FALSE_POSITIVE_RARE
            } else {
                false
            }
        });
        has_rare != flipped
    }

    /// 0xBB9CC0: the reading is reset to unflipped (after an ambush).
    pub fn reset_detector(&mut self, world: u32) {
        self.detector.insert(world, false);
    }

    /// 0x103FC20: a rare obtained. Returns true when it was not found before
    /// (badge progress, event log).
    pub fn obtain(&mut self, rare: &Rare) -> bool {
        self.found.insert(rare.name.to_string())
    }

    /// A group every member of which is found, not paid yet (marked paid).
    pub fn newly_completed(&mut self, group_id: u32) -> bool {
        let all = members(group_id).all(|r| self.found.contains(r.name));
        all && self.completed_groups.insert(group_id)
    }

    pub fn is_found(&self, instance: u32) -> bool {
        self.found.iter().any(|n| crate::world_objects::name_id(n) == instance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retail_rare_list_has_its_groups_and_chances() {
        assert_eq!(RARES.len(), 150);
        assert_eq!(GROUPS.len(), 15);
        // Every rare belongs to a listed group.
        assert!(RARES.iter().all(|r| group(r.group).is_some()));
        assert_eq!(rare("rare-jewel1").unwrap().chance, 0.003);
        assert_eq!(rare("rare-bardscroll01").unwrap().cost, 45_000.0);
        // Galactic objects and story books are never placed on planets.
        assert!(members(8).chain(members(9)).all(|r| r.chance == 0.0));
        assert_eq!(rare("rare-galacticobject1").unwrap().title, "Galactic Core");
        // The install's quirk: Block of Chance 10 is filed with group 23.
        assert_eq!(rare("rare-bardscroll10").unwrap().group, 23);
        assert_eq!(members(22).count() + members(23).count(), 20);
    }

    #[test]
    fn planet_rolls_pass_until_one_rare_is_added() {
        // A stream that fails everything twice, then passes item 5.
        let mut n = 0usize;
        let got = roll_planet_rares(|| {
            n += 1;
            if n == 2 * RARES.len() + 6 { 0.0 } else { 0.99 }
        });
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.name, RARES[5].name);
        assert_eq!(got[0].1, 1);
        // Weighted by chance: the 0.003 groups come up three times as often as
        // the 0.001 scrolls per item; chance-0 groups never.
        let mut by = BTreeMap::new();
        for w in 0..4000u32 {
            let r = world_rare(w, true).unwrap();
            *by.entry(r.chance.to_bits()).or_insert(0u32) += 1;
            assert!(r.chance > 0.0);
        }
        let hi = by[&0.003f32.to_bits()] as f32 / 50.0;
        let lo = by[&0.001f32.to_bits()] as f32 / 80.0;
        assert!((2.2..4.0).contains(&(hi / lo)), "per-item ratio {}", hi / lo);
        assert!(world_rare(7, false).is_none());
    }

    #[test]
    fn the_detector_reads_once_and_false_positives_need_rare_close() {
        let mut s = RareState::default();
        // A rare: false negatives are 0 in the install.
        assert!(s.detector(1, true, true, true));
        // No rare before RareClose: read unflipped and kept so.
        assert!(!s.detector(2, false, true, false));
        assert!(!s.detector(2, false, true, true));
        // After RareClose about 5% of creature planets read a rare.
        let hits = (100..2100).filter(|w| s.detector(*w, false, true, true)).count();
        assert!((60..140).contains(&hits), "false positives {hits}");
        let w = (100..2100).find(|w| s.detector(*w, false, true, true)).unwrap();
        s.reset_detector(w);
        assert!(!s.detector(w, false, true, true));
        // Found rares and completed groups.
        let r = rare("rare-geode2").unwrap();
        assert!(s.obtain(r) && !s.obtain(r));
        assert!(s.is_found(crate::world_objects::name_id("rare-geode2")));
        assert!(!s.newly_completed(4));
        for m in members(4) {
            s.obtain(m);
        }
        assert!(s.newly_completed(4) && !s.newly_completed(4));
    }
}
