//! The galaxy map's star hover (helper `space-look3`, `notes/space-look3-notes.md` 1).
//!
//! Original code (march2017 addresses, read with `dis.py`):
//! - 0x10743D0 (the Space UI update) asks the UFO for the star under the pointer
//!   (`cSimulatorPlayerUFO::0xFFF780`, galaxy context only) and hands it to
//!   0x106CCA0 (the hover HUD) which calls 0x106B340(star) every frame.
//! - 0xFFF780 casts the pointer ray (clamped to 1000 units) at the star objects
//!   and accepts a hit star when its record flag `kStarFlagSaveGame` (0x2) is set
//!   or the camera is closer than 100 units (`dist^2 < 10000`, 0xFFFA46).
//! - 0x106B340: nothing but 0x1068240 (stop the hover sound, stop and release the
//!   hover effect) unless the context is the galaxy, the player's inventory has
//!   the **SETI tool** (`cPlayerInventory::HasTool`, key 0x19E07A2C) and the game
//!   is not paused. With a star: its distance (2D, 0x10434F0) to the active star
//!   must not exceed the UFO's `mMaxScanDistance` (+0x4C, set in 0xFFDA50 to the
//!   `spaceToolDetectableRange` +0x1B0 of the best owned interstellar drive, 5.0
//!   without one), else it counts as no star. A star starts a hover sound when
//!   the last one is not playing any more: `sfx_starmapfilter_space` (0x40FFB540)
//!   for a SETI star (0xBBA120), `sfx_starmapfilter_standard` (0x61FCEA3D) for
//!   any other; a SETI star also starts the hover effect 0x28115AB8 (3 particle
//!   blocks gated by `kAppFlagSpaceGameGalaxyMode`) at its position, once: the
//!   handle is only released by 0x1068240, so moving the pointer straight from
//!   one star to the next leaves the effect on the first. No star: if the sound
//!   still plays it is stopped and `sfx_starmapfilter_mouseoff` (0x17B6FD56) plays;
//!   then 0x1068240.
//! - 0xBBA120(star) = the SETI predicate: not the player's (flag 0x10) and
//!   `0xBBA020(star) == (techLevel == Empire)`. 0xBBA020 is the star's cached
//!   error roll (flags 0x2000 / 0x4000): true (no error) for the player's stars,
//!   for stars that are not G / O / M / binary types (0xC8B6B0 = `type > 3`) and
//!   for the player's empire, else it draws `Math::sMainRNG` against
//!   `spaceToolSetiChanceFalseNegativeEmpire` (0x05B07986, an empire star) or
//!   `...FalsePositiveEmpire` (0x05B07984): a roll below the chance flips it.
//!   Both chances are 0.0 in the installed `seti` tool.
//!
//! Which star the pointer hits is the pick of `proxies.rs` (the retail hit
//! proxies of the star objects, decoded by `space-stars2`: the ray against the
//! `Unit Sphere` models of the UFO's `Star` world, then the acceptance rule above).
//!
//! ADAPTED: the star flag `kStarFlagSaveGame` stands for visited stars and the
//! home star; the error roll is a hash of the star (retail keeps it in the star
//! flags); the effect's size (1.0).
use crate::game_audio::Cue;
use crate::space_worldgen::{StarKind, TechLevel};

/// The SETI tool (`spacetools~`, 0x19E07A2C).
pub const SETI_TOOL: &str = "seti";
/// The hover effect (effect directory 0xC1195706, 3 particle blocks).
pub const HOVER_EFFECT: u32 = 0x2811_5AB8;
/// `kAppFlagSpaceGameGalaxyMode` (bit 18 of the effect app flags, the gate of
/// the hover effect's blocks).
pub const APP_FLAG_GALAXY_MODE: u32 = 1 << 18;
/// 0xFFFA46: a hit star is accepted when the camera is closer than this.
pub const NEAR_STAR_RANGE: f32 = 100.0;
/// 0xFFF8D6: the pointer ray is clamped to this length.
pub const RAY_LENGTH: f32 = 1000.0;
/// 0xFFDAAC: `mMaxScanDistance` of a UFO without an interstellar drive.
pub const NO_DRIVE_SCAN_DISTANCE: f32 = 5.0;
/// Seconds a requested sound counts as playing before its voice exists (the
/// cue decodes on a worker the first time).
pub const SOUND_GRACE: f64 = 0.3;

/// `sfx_starmapfilter_space` / `_standard` (by the SETI predicate).
pub fn hover_cue(signal: bool) -> Cue {
    if signal {
        Cue::StarmapFilterSpace
    } else {
        Cue::StarmapFilterStandard
    }
}

/// `cSimulatorPlayerUFO::mMaxScanDistance` (0xFFDA50): the detectable range of
/// the best owned interstellar drive (every level has 20), 5.0 without one.
pub fn scan_distance<S: AsRef<str>>(tools: &[S]) -> f32 {
    if crate::space_rules::drive_range(tools) > 0.0 {
        crate::space_tools::record(crate::space_rules::tool::INTERSTELLAR_DRIVE)
            .map_or(NO_DRIVE_SCAN_DISTANCE, |r| r.detectable_range)
    } else {
        NO_DRIVE_SCAN_DISTANCE
    }
}

/// What 0xBBA120 reads of a star record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarFacts {
    pub kind: StarKind,
    pub tech: TechLevel,
    /// Flag 0x10 (`kStarFlagIsOwned`) / the player's empire: the player's star.
    pub player_star: bool,
    /// The star's error roll in 0..1 (retail: one `Math::sMainRNG` draw kept in
    /// the star flags).
    pub roll: f32,
}

/// 0xC8B6B0: the star types that can hold an empire (G, O, M and the binaries).
fn holds_empire(kind: StarKind) -> bool {
    !matches!(kind, StarKind::GalacticCore | StarKind::BlackHole | StarKind::ProtoPlanetary)
}

/// 0xBBA020: the star's error roll holds (`true` = the scan is right).
pub fn roll_holds(f: &StarFacts, false_positive: f32, false_negative: f32) -> bool {
    if f.player_star || !holds_empire(f.kind) {
        return true;
    }
    let chance = if f.tech == TechLevel::Empire { false_negative } else { false_positive };
    // `chance > RandomFloat()` flags the star 0x4000 (wrong).
    !(chance > f.roll)
}

/// 0xBBA120: the star shows the SETI signal.
pub fn seti_signal_with(f: &StarFacts, false_positive: f32, false_negative: f32) -> bool {
    !f.player_star && roll_holds(f, false_positive, false_negative) == (f.tech == TechLevel::Empire)
}

/// 0xBBA120 with the installed chances.
pub fn seti_signal(f: &StarFacts) -> bool {
    seti_signal_with(f, crate::space_rules::SETI_FALSE_POSITIVE_EMPIRE, crate::space_rules::SETI_FALSE_NEGATIVE_EMPIRE)
}

/// The stand-in for the star's one-time `Math::sMainRNG` draw (ADAPTED): the
/// same hash the cinematics use for the star the ship is in.
pub fn star_roll(star: u32) -> f32 {
    (star.wrapping_mul(0x9E37_79B9) >> 8) as f32 / (1u32 << 24) as f32
}

/// 0xFFF780's acceptance of a hit star: the camera is closer than [`NEAR_STAR_RANGE`] or the star carries
/// `kStarFlagSaveGame` (ADAPTED: which stars carry it, see `proxies.rs`).
pub fn pick_accepted(camera_distance: f32, save_game: bool) -> bool {
    save_game || camera_distance < NEAR_STAR_RANGE
}

/// The hovered star as 0x106B340 sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverTarget {
    pub star: u32,
    pub signal: bool,
    /// 2D distance to the active star (0x10434F0).
    pub distance: f32,
}

/// The two handles 0x106B340 keeps in the Space UI (+0x5BC sound, +0x5C0 effect).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverState {
    /// The hover sound that was started (its voice id).
    pub sound: Option<Cue>,
    /// The star the hover effect sits on (released only by `reset`).
    pub effect: Option<u32>,
}

/// What the Bevy side has to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HoverAction {
    PlaySound(Cue),
    StopSound(Cue),
    Mouseoff,
    SpawnEffect(u32),
    StopEffect,
}

/// One frame of 0x106B340 / 0x1068240. `gate_open` = galaxy context, the
/// player owns the SETI tool and the game is not paused; `sound_playing` = the
/// last hover sound still plays (`IAudioSystem::IsPlaying`).
pub fn step(
    state: &mut HoverState,
    gate_open: bool,
    target: Option<HoverTarget>,
    scan_distance: f32,
    sound_playing: bool,
) -> Vec<HoverAction> {
    let mut out = Vec::new();
    // 0x1068240: stop a playing hover sound, stop and release the effect.
    let reset = |state: &mut HoverState, out: &mut Vec<HoverAction>| {
        if let Some(cue) = state.sound {
            if sound_playing {
                out.push(HoverAction::StopSound(cue));
            }
        }
        if state.effect.take().is_some() {
            out.push(HoverAction::StopEffect);
        }
    };
    if !gate_open {
        reset(state, &mut out);
        return out;
    }
    // Out of the scan range a star counts as no star (0x106B3CE).
    let target = target.filter(|t| t.distance <= scan_distance);
    match target {
        Some(t) => {
            if !sound_playing {
                let cue = hover_cue(t.signal);
                state.sound = Some(cue);
                out.push(HoverAction::PlaySound(cue));
            }
            if t.signal && state.effect.is_none() {
                state.effect = Some(t.star);
                out.push(HoverAction::SpawnEffect(t.star));
            }
        }
        None => {
            if sound_playing {
                if let Some(cue) = state.sound {
                    out.push(HoverAction::StopSound(cue));
                }
                out.push(HoverAction::Mouseoff);
                // The stop above also ends the sound for the reset.
                state.sound = None;
            }
            reset(state, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(kind: StarKind, tech: TechLevel, player: bool) -> StarFacts {
        StarFacts { kind, tech, player_star: player, roll: 0.5 }
    }

    fn target(star: u32, signal: bool, distance: f32) -> Option<HoverTarget> {
        Some(HoverTarget { star, signal, distance })
    }

    #[test]
    fn the_seti_signal_is_an_empire_star_that_is_not_the_players() {
        assert!(seti_signal(&facts(StarKind::Yellow, TechLevel::Empire, false)));
        assert!(!seti_signal(&facts(StarKind::Yellow, TechLevel::Empire, true)));
        assert!(!seti_signal(&facts(StarKind::Yellow, TechLevel::Tribe, false)));
        assert!(!seti_signal(&facts(StarKind::Red, TechLevel::None, false)));
        // Types that cannot hold an empire skip the error roll (0xBBA020 is true), so the
        // signal is just `techLevel == Empire`: none in the installed galaxy.
        assert!(!seti_signal(&facts(StarKind::BlackHole, TechLevel::None, false)));
        assert!(!seti_signal(&facts(StarKind::GalacticCore, TechLevel::None, false)));
        assert!(seti_signal(&facts(StarKind::GalacticCore, TechLevel::Empire, false)));
        assert!(seti_signal(&facts(StarKind::BinaryOM, TechLevel::Empire, false)));
    }

    #[test]
    fn the_error_rolls_flip_the_signal_under_their_chance() {
        let empire = StarFacts { roll: 0.1, ..facts(StarKind::Blue, TechLevel::Empire, false) };
        let empty = StarFacts { roll: 0.1, ..facts(StarKind::Blue, TechLevel::None, false) };
        // False negative: an empire star is missed under the chance.
        assert!(!seti_signal_with(&empire, 0.0, 0.2));
        assert!(seti_signal_with(&empire, 0.0, 0.05));
        // False positive: a star without an empire signals under the chance.
        assert!(seti_signal_with(&empty, 0.2, 0.0));
        assert!(!seti_signal_with(&empty, 0.05, 0.0));
        // Never for the player's stars or the types without empires.
        let own = StarFacts { player_star: true, ..empty };
        assert!(!seti_signal_with(&own, 1.0, 1.0));
        let hole = StarFacts { kind: StarKind::BlackHole, ..empty };
        assert!(!seti_signal_with(&hole, 1.0, 1.0));
        // The installed chances are 0.0, so the roll never matters.
        for r in [0.0, 0.3, 0.99] {
            assert!(seti_signal(&StarFacts { roll: r, ..facts(StarKind::Red, TechLevel::Empire, false) }));
        }
        assert!((0.0..1.0).contains(&star_roll(12345)));
    }

    #[test]
    fn the_scan_distance_follows_the_drive() {
        let none: [&str; 0] = [];
        assert_eq!(scan_distance(&none), 5.0);
        assert_eq!(scan_distance(&["interplanetarydrive"]), 5.0);
        assert_eq!(scan_distance(&["interplanetarydrive", "interstellardrive"]), 20.0);
        assert_eq!(scan_distance(&["interstellardrive", "interstellardrive_upgrade3"]), 20.0);
    }

    #[test]
    fn the_pick_needs_a_near_camera_or_the_save_flag() {
        assert!(pick_accepted(99.0, false));
        assert!(!pick_accepted(100.0, false));
        assert!(pick_accepted(400.0, true));
    }

    #[test]
    fn a_star_starts_its_sound_and_a_seti_star_the_effect() {
        let mut s = HoverState::default();
        let a = step(&mut s, true, target(7, false, 3.0), 20.0, false);
        assert_eq!(a, vec![HoverAction::PlaySound(Cue::StarmapFilterStandard)]);
        // The sound keeps playing: nothing new.
        assert!(step(&mut s, true, target(7, false, 3.0), 20.0, true).is_empty());
        let mut s = HoverState::default();
        let a = step(&mut s, true, target(9, true, 3.0), 20.0, false);
        assert_eq!(a, vec![HoverAction::PlaySound(Cue::StarmapFilterSpace), HoverAction::SpawnEffect(9)]);
        assert_eq!(s.effect, Some(9));
    }

    #[test]
    fn the_effect_stays_on_the_first_star_until_the_pointer_leaves() {
        let mut s = HoverState::default();
        step(&mut s, true, target(1, true, 2.0), 20.0, false);
        // Straight onto another SETI star: the handle is still held.
        let a = step(&mut s, true, target(2, true, 2.0), 20.0, true);
        assert!(a.is_empty());
        assert_eq!(s.effect, Some(1));
        // Off every star while the sound plays: stop, mouse off, release.
        let a = step(&mut s, true, None, 20.0, true);
        assert_eq!(
            a,
            vec![HoverAction::StopSound(Cue::StarmapFilterSpace), HoverAction::Mouseoff, HoverAction::StopEffect]
        );
        assert_eq!(s, HoverState::default());
    }

    #[test]
    fn the_mouseoff_only_plays_when_the_sound_still_plays() {
        let mut s = HoverState::default();
        step(&mut s, true, target(4, false, 2.0), 20.0, false);
        let a = step(&mut s, true, None, 20.0, false);
        assert!(a.is_empty(), "{a:?}");
        // Nothing hovered from the start: nothing at all.
        assert!(step(&mut HoverState::default(), true, None, 20.0, false).is_empty());
    }

    #[test]
    fn a_star_beyond_the_scan_distance_counts_as_none() {
        let mut s = HoverState::default();
        assert!(step(&mut s, true, target(5, true, 25.0), 20.0, false).is_empty());
        let a = step(&mut s, true, target(5, true, 25.0), 20.0, true);
        assert_eq!(a, vec![HoverAction::Mouseoff]);
        // Exactly at the distance still hovers (the retail test is `range < distance`).
        let a = step(&mut HoverState::default(), true, target(5, false, 20.0), 20.0, false);
        assert_eq!(a, vec![HoverAction::PlaySound(Cue::StarmapFilterStandard)]);
    }

    #[test]
    fn a_closed_gate_only_resets() {
        let mut s = HoverState { sound: Some(Cue::StarmapFilterSpace), effect: Some(3) };
        let a = step(&mut s, false, target(3, true, 1.0), 20.0, true);
        assert_eq!(a, vec![HoverAction::StopSound(Cue::StarmapFilterSpace), HoverAction::StopEffect]);
        assert!(step(&mut s, false, None, 20.0, false).is_empty());
    }
}
