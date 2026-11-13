// Bounded persisted Creature pack: recruited allies that follow the player.
// Pure Rust module — no Bevy dependencies, no ECS. Brain thresholds are
// the retail ones (creature_rules); pack capacity by rank is explicit OpenSpore tuning.

use serde::{Deserialize, Serialize};

/// Maximum number of members a pack can hold.
pub const PACK_MAX_MEMBERS: usize = 3;

/// Human-readable HUD line for the current pack state.
pub fn hud(pack: &CreaturePack, dna: f32) -> String {
    let cap = capacity(dna);
    if cap == 0 {
        format!(
            "Pack locked (needs {} DNA)",
            crate::creature_progression::BRAIN_LEVEL_DNA[0] as u32
        )
    } else {
        format!(
            "Pack {}/{} [{}] | R recruit/dismiss ally",
            pack.members.len(),
            cap,
            pack.stance.label()
        )
    }
}

/// Pure: adapted pack capacity for a DNA amount.
/// Capacity follows the shared Creature brain-rank progression (retail stage
/// totals): 0 below 100, 1 at 100, 2 at 300, and 3 at 600 and beyond.
/// Non-finite values yield 0.
pub fn capacity(dna: f32) -> usize {
    crate::creature_progression::pack_capacity(dna).min(PACK_MAX_MEMBERS)
}

/// Pure: retain only member IDs that appear in `valid`, in their original
/// stable order, with duplicates removed (first occurrence wins), and then
/// truncate to the current capacity. Returns `true` iff the roster changed.
pub fn sanitize(pack: &mut CreaturePack, dna: f32, valid: &[u32]) -> bool {
    let cap = capacity(dna);
    let mut kept: Vec<u32> = Vec::with_capacity(pack.members.len().min(cap));
    for &member in &pack.members {
        if kept.len() >= cap {
            break;
        }
        // Skip if already kept (deduplication, first occurrence wins)
        if kept.contains(&member) {
            continue;
        }
        // Keep only if the member is in the valid list
        if valid.contains(&member) {
            kept.push(member);
        }
    }
    let changed = kept != pack.members;
    pack.members = kept;
    changed
}

/// Pure: toggle `id`'s membership in the pack.
/// Returns `Ok(true)` if recruited, `Ok(false)` if dismissed.
/// Returns `Err("...")` if the id is unknown, not a living ally, or the
/// pack is full/locked. The caller promises to pass only living allied IDs.
pub fn toggle(
    pack: &mut CreaturePack,
    dna: f32,
    id: u32,
    valid: &[u32],
) -> Result<bool, &'static str> {
    // If the id is already recruited, dismiss it.
    if pack.members.contains(&id) {
        pack.members.retain(|&member| member != id);
        return Ok(false); // dismissed
    }
    // If the id is not in the valid list, reject.
    if !valid.contains(&id) {
        return Err("creature not a living ally");
    }
    // Otherwise try to recruit, bounded by capacity.
    let cap = capacity(dna);
    if cap == 0 {
        return Err("pack locked: needs more DNA");
    }
    if pack.members.len() >= cap {
        return Err("pack full");
    }
    pack.members.push(id);
    Ok(true) // recruited
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PackStance {
    /// Pack members follow the player in formation and assist in combat or social.
    #[default]
    Follow,
    /// Pack members charge and attack the player's target aggressively.
    Attack,
    /// Pack members hold position at their current location and guard.
    Stay,
}

impl PackStance {
    pub fn label(self) -> &'static str {
        match self {
            PackStance::Follow => "Follow",
            PackStance::Attack => "Attack",
            PackStance::Stay => "Stay",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CreaturePack {
    /// Persistent member ids (`persistence::WorldId` values),
    /// in recruitment order.
    pub members: Vec<u32>,
    /// Current pack stance/order (Follow, Attack, Stay).
    pub stance: PackStance,
    /// Position where the pack was ordered to stay, if in Stay stance.
    pub stay_position: Option<[f32; 3]>,
}

impl CreaturePack {
    /// Cycle the pack stance between Follow -> Attack -> Stay -> Follow.
    pub fn cycle_stance(&mut self, player_pos: [f32; 3]) -> PackStance {
        self.stance = match self.stance {
            PackStance::Follow => PackStance::Attack,
            PackStance::Attack => {
                self.stay_position = Some(player_pos);
                PackStance::Stay
            }
            PackStance::Stay => {
                self.stay_position = None;
                PackStance::Follow
            }
        };
        self.stance
    }

    /// Set an explicit pack stance.
    pub fn set_stance(&mut self, stance: PackStance, pos: Option<[f32; 3]>) {
        self.stance = stance;
        if stance == PackStance::Stay {
            self.stay_position = pos;
        } else {
            self.stay_position = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_follows_adapted_brain_rank_thresholds() {
        assert_eq!(capacity(0.), 0);
        assert_eq!(capacity(99.9), 0);
        assert_eq!(capacity(100.), 1);
        assert_eq!(capacity(299.9), 1);
        assert_eq!(capacity(300.), 2);
        assert_eq!(capacity(599.9), 2);
        assert_eq!(capacity(600.), 3);
        assert_eq!(capacity(999.9), 3);
        assert_eq!(capacity(1000.), 3);
        assert_eq!(capacity(f32::NAN), 0);
        assert_eq!(capacity(f32::NEG_INFINITY), 0);
        assert_eq!(capacity(f32::INFINITY), 0);
    }

    #[test]
    fn sanitize_keeps_only_valid_deduplicates_and_trims() {
        let mut pack = CreaturePack {
            members: vec![301, 300, 301, 302, 303],
            ..Default::default()
        };
        // Only 300 and 302 are valid; cap=3 so all 3 fit.
        assert!(sanitize(&mut pack, 600., &[300, 302, 303]));
        // Now members = [300, 302, 303] (301 dropped as invalid, 301 duplicate removed)
        assert_eq!(pack.members, vec![300, 302, 303]);

        // Second pass changes nothing.
        assert!(!sanitize(&mut pack, 600., &[300, 302, 303]));

        // Capacity shrinks the roster deterministically.
        assert!(sanitize(&mut pack, 100., &[300, 302, 303]));
        // cap=1, keeps only the first valid member.
        assert_eq!(pack.members, vec![300]);
    }

    #[test]
    fn sanitize_empty_when_no_valid_members() {
        let mut pack = CreaturePack {
            members: vec![999, 888],
            ..Default::default()
        };
        assert!(sanitize(&mut pack, 600., &[300, 301]));
        assert!(pack.members.is_empty());
    }

    #[test]
    fn toggle_recruits_when_slot_available() {
        let mut pack = CreaturePack::default();
        assert_eq!(toggle(&mut pack, 600., 300, &[300, 301, 302]), Ok(true));
        assert_eq!(pack.members, vec![300]);
    }

    #[test]
    fn toggle_dismisses_existing_member() {
        let mut pack = CreaturePack { members: vec![300], ..Default::default() };
        assert_eq!(
            toggle(&mut pack, 600., 300, &[300, 301]),
            Ok(false) // dismissed
        );
        assert!(pack.members.is_empty());
    }

    #[test]
    fn toggle_rejects_unknown_id() {
        let mut pack = CreaturePack::default();
        let result = toggle(&mut pack, 600., 999, &[300, 301]);
        assert_eq!(result, Err("creature not a living ally"));
        assert!(pack.members.is_empty());
    }

    #[test]
    fn toggle_rejects_when_pack_full() {
        let mut pack = CreaturePack {
            members: vec![300, 301, 302],
            ..Default::default()
        };
        let result = toggle(&mut pack, 600., 303, &[300, 301, 302, 303]);
        assert_eq!(result, Err("pack full"));
        // Pack should be unchanged.
        assert_eq!(pack.members, vec![300, 301, 302]);
    }

    #[test]
    fn toggle_rejects_when_locked() {
        let mut pack = CreaturePack::default();
        let result = toggle(&mut pack, 10., 300, &[300]);
        assert_eq!(result, Err("pack locked: needs more DNA"));
        assert!(pack.members.is_empty());
    }

    #[test]
    fn toggle_deduplicates_stable_order() {
        let mut pack = CreaturePack {
            members: vec![301, 300, 301, 302],
            ..Default::default()
        };
        // 301 is already in the pack; calling toggle on it dismisses
        // the first occurrence, keeping the roster stable.
        assert_eq!(
            toggle(&mut pack, 600., 301, &[300, 301, 302]),
            Ok(false) // dismissed (first 301 removed)
        );
        // Remaining: [300, 302] (second 301 dropped, stable order kept)
        assert_eq!(pack.members, vec![300, 302]);
    }

    #[test]
    fn hud_reports_capacity_and_roster() {
        let pack = CreaturePack {
            members: vec![300, 301],
            ..Default::default()
        };
        let line = hud(&pack, 600.);
        assert!(line.contains("Pack 2/3"), "unexpected HUD: {line}");
        assert!(line.contains("R recruit/dismiss"), "unexpected HUD: {line}");
        let locked = hud(&CreaturePack::default(), 10.);
        assert!(locked.contains("locked"), "unexpected HUD: {locked}");
        let empty = hud(&CreaturePack::default(), 600.);
        assert!(empty.contains("Pack 0/3"), "unexpected HUD: {empty}");
    }

    #[test]
    fn serde_roundtrip_preserves_members() {
        let original = CreaturePack {
            members: vec![300, 301, 302],
            ..Default::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: CreaturePack = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.members, vec![300, 301, 302]);
    }

    #[test]
    fn serde_default_loads_empty() {
        let pack: CreaturePack = serde_json::from_str("{}").unwrap();
        assert!(pack.members.is_empty());
        let mut saved = serde_json::to_value(crate::game::RunProgress::default()).unwrap();
        saved.as_object_mut().unwrap().remove("creature_pack");
        let run: crate::game::RunProgress = serde_json::from_value(saved).unwrap();
        assert!(run.creature_pack.members.is_empty());
    }

    #[test]
    fn stance_cycles_through_orders_and_preserves_stay_position() {
        let mut pack = CreaturePack::default();
        assert_eq!(pack.stance, PackStance::Follow);
        assert_eq!(pack.cycle_stance([10.0, 0.0, 5.0]), PackStance::Attack);
        assert_eq!(pack.stay_position, None);
        assert_eq!(pack.cycle_stance([10.0, 0.0, 5.0]), PackStance::Stay);
        assert_eq!(pack.stay_position, Some([10.0, 0.0, 5.0]));
        assert_eq!(pack.cycle_stance([10.0, 0.0, 5.0]), PackStance::Follow);
        assert_eq!(pack.stay_position, None);

        pack.set_stance(PackStance::Stay, Some([1.0, 2.0, 3.0]));
        assert_eq!(pack.stance, PackStance::Stay);
        assert_eq!(pack.stay_position, Some([1.0, 2.0, 3.0]));
    }
}
