//! Group socials: several herd mates propose together (retail SocialPropose
//! 0xDA4360 and its update 0xDA4950, notes C10A).
//!
//! * A creature whose herd archetype has `SocialGroupSize` > 1 calls up to
//!   `SocialGroupSize - 1` herd mates of its species from its nearby list when
//!   it proposes (`creature_actions::begin_gather`). Each joiner walks to a
//!   slot behind the proposer (0xDA3AF0: the direction to the player turned by
//!   135 / 225 / 180 degrees, `2 x (radius + 1)` away).
//! * The proposer plays 0x061E0673 then 0x061E0674 while it waits, for up to
//!   5 s (tuning 0x3215F0AF) or until every joiner stands in its slot; the
//!   joiners in place are the proposal's `n`: the bar is `(n + 1)` times as
//!   long and every demonstration adds `(n + 1)` levels (0xDA4BBE), so the
//!   player needs `(n + 1)` times as many answers (or a posse).
//! * Joiners play the proposer's demonstration clip (broadcast 0xD9B190, event
//!   type 3). Befriending is still one creature: 0xD9BBD0 flags only the
//!   proposer (+0xB58 |= 0x80) and records one impression for the species.
//!
//! ADAPTED: the nearby list (+0x1124) radius is not traced; herd mates within
//! `creatureBasePerception` (10) of the proposer answer. A joiner's radius is
//! half its body scale (retail: its bounding radius, +0x74).
use crate::creature_actions::{levels, member_abilities, propose, CreatureMind, EncounterMode};
use crate::game::{Npc, Player, RunProgress};
use crate::persistence::WorldId;
use bevy::prelude::*;

/// Herd mates within this distance of the proposer can answer its call
/// (ADAPTED, see the module notes).
pub const GROUP_CALL_RADIUS: f32 = crate::creature_rules::BASE_PERCEPTION;

struct Seen {
    entity: Entity,
    id: u32,
    at: Vec2,
    radius: f32,
    species: u32,
    free: bool,
}

/// One frame of every group social: calls herd mates, places and counts the
/// joiners, starts the proposal when the gathering ends, hands the
/// demonstration clip to the joiners and releases them when it is over.
#[allow(clippy::type_complexity)]
pub fn social_groups(
    time: Res<Time>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    player: Query<&Transform, (With<Player>, Without<Npc>)>,
    mut npcs: Query<
        (Entity, &WorldId, &Transform, &Npc, &mut CreatureMind, Option<&crate::creature_ecology::HerdMember>),
        Without<Player>,
    >,
) {
    let dt = time.delta_secs();
    let target = player.single().map(|t| t.translation.xz()).unwrap_or(Vec2::ZERO);
    let seen: Vec<Seen> = npcs
        .iter()
        .map(|(entity, id, t, npc, mind, herd)| Seen {
            entity,
            id: id.0,
            at: t.translation.xz(),
            radius: 0.5 * t.scale.max_element(),
            species: herd.map_or(mind.species as u32, |h| h.species),
            free: !mind.allied
                && !mind.defeated
                && !npc.hostile
                && npc.hp > 0.0
                && !mind.performing
                && mind.gather <= 0.0
                && mind.joining.is_none()
                && mind.downed <= 0.0
                && mind.joiners.is_empty(),
        })
        .collect();
    let index_of = |id: u32| seen.iter().position(|s| s.id == id);
    // Joiner updates gathered from the proposers: (joiner, proposer, slot, cue).
    let mut assign: Vec<(usize, u32, Option<[f32; 2]>, Option<(u32, u32)>)> = Vec::new();
    // Reaction clips the proposers broadcast to their joiners: (joiner, clip).
    let mut reactions: Vec<(u32, u32)> = Vec::new();
    for (_entity, id, t, _npc, mut mind, _) in &mut npcs {
        let me = index_of(id.0).expect("seen above");
        // Call the herd (0xDA4360): free herd mates of the species, nearest first.
        if mind.gather > 0.0 && mind.group_wanted > 0 && mind.joiners.is_empty() {
            let wanted = mind.group_wanted as usize;
            mind.group_wanted = 0;
            let mut near: Vec<(f32, usize)> = seen
                .iter()
                .enumerate()
                .filter(|(i, s)| *i != me && s.free && s.species == seen[me].species)
                .map(|(i, s)| (s.at.distance(seen[me].at), i))
                .filter(|(d, _)| *d <= GROUP_CALL_RADIUS)
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            // A joiner already claimed by another proposer this frame is skipped.
            near.retain(|(_, i)| !assign.iter().any(|(j, ..)| j == i));
            mind.joiners = near.iter().take(wanted).map(|(_, i)| seen[*i].id).collect();
            info!("group social: creature {} calls {wanted} herd mates, {:?} answer", id.0, mind.joiners);
            for (_, i) in near.iter().take(wanted) {
                assign.push((*i, id.0, None, None));
            }
            if mind.joiners.is_empty() {
                // Nobody answered: +0x1F4 = 0 and the proposal starts at once.
                mind.gather = 0.0;
                let ranks = levels(member_abilities(&run, &originals, id.0), EncounterMode::Social);
                propose(&mut mind, ranks);
                continue;
            }
        }
        // A proposal runs while a round is shown or open (an unanswered one
        // left behind by the player ends with its answer window).
        let active = mind.gather > 0.0 || (mind.performing && mind.response_time > 0.0 && !mind.allied && !mind.defeated);
        if !active {
            // The proposal ended with a befriend (its reaction clip, 0xD9BBD0)
            // or with every round used (soc_annoyed_reject, 0xDA4950): the
            // proposer broadcasts that clip to its group.
            let reaction = mind.allied
                || mind.cue_clip == crate::creature_rules::SOCIAL_CLIP_ROUNDS_OVER;
            if reaction && mind.cue_clip != 0 {
                reactions.extend(mind.joiners.iter().map(|j| (*j, mind.cue_clip)));
            }
            if !mind.joiners.is_empty() || mind.group != 0 {
                mind.joiners.clear();
                mind.group = 0;
            }
            continue;
        }
        if mind.joiners.is_empty() {
            continue;
        }
        let slots = crate::creature_rules::social_group_slots(mind.joiners.len());
        let here = t.translation.xz();
        let mut arrived = 0u8;
        let clip = crate::creature_actions::ability_clip(
            &originals.creature_abilities,
            EncounterMode::Social,
            mind.requested as usize,
            mind.requested_level.max(1),
        );
        let joiners = mind.joiners.clone();
        let mut keep = Vec::new();
        for (k, joiner) in joiners.iter().enumerate() {
            let Some(j) = index_of(*joiner) else { continue };
            let turn = slots[k.min(slots.len() - 1)];
            let slot = crate::creature_rules::social_group_slot(here, target, turn, seen[j].radius);
            if seen[j].at.distance(slot) <= crate::creature_rules::SOCIAL_GROUP_SLOT_STOP + 0.2 {
                arrived += 1;
            }
            keep.push(*joiner);
            let cue = (mind.performing && mind.gather <= 0.0)
                .then(|| clip.map(|c| (c, mind.demo_serial)))
                .flatten();
            assign.push((j, id.0, Some([slot.x, slot.y]), cue));
        }
        mind.joiners = keep;
        if mind.gather > 0.0 {
            mind.gather -= dt;
            if mind.gather <= 0.0 || arrived as usize == mind.joiners.len() {
                // 0xDA4950 state 3: the joiners in place are `n`; the others
                // are let go and the proposal starts.
                mind.gather = 0.0;
                mind.group = arrived;
                let joiners = std::mem::take(&mut mind.joiners);
                for (k, joiner) in joiners.into_iter().enumerate() {
                    let Some(j) = index_of(joiner) else { continue };
                    let turn = slots[k.min(slots.len() - 1)];
                    let slot = crate::creature_rules::social_group_slot(here, target, turn, seen[j].radius);
                    if seen[j].at.distance(slot) <= crate::creature_rules::SOCIAL_GROUP_SLOT_STOP + 0.2 {
                        mind.joiners.push(joiner);
                    } else {
                        assign.retain(|(i, ..)| *i != j);
                    }
                }
                let ranks = levels(member_abilities(&run, &originals, id.0), EncounterMode::Social);
                propose(&mut mind, ranks);
                info!(
                    "group social: creature {} proposes with {} herd mates in place (bar {}, its side {})",
                    id.0, mind.group, mind.bar_max, mind.npc_fill
                );
            }
        }
    }
    // Apply to the joiners; release the ones whose proposal is over.
    let claimed: Vec<(Entity, u32, u32, Option<[f32; 2]>, Option<(u32, u32)>)> = assign
        .iter()
        .map(|(j, owner, slot, cue)| (seen[*j].entity, seen[*j].id, *owner, *slot, *cue))
        .collect();
    for (_, id, _, _, mut mind, _) in &mut npcs {
        if mind.joining.is_some() && !claimed.iter().any(|(_, j, ..)| *j == id.0) {
            mind.joining = None;
            mind.slot_at = None;
        }
        if let Some((_, clip)) = reactions.iter().find(|(j, _)| *j == id.0) {
            crate::creature_actions::play_cue(&mut mind, *clip);
        }
    }
    for (entity, _, owner, slot, cue) in claimed {
        let Ok((_, _, _, _, mut mind, _)) = npcs.get_mut(entity) else { continue };
        mind.joining = Some(owner);
        if slot.is_some() {
            mind.slot_at = slot;
        }
        if let Some((clip, serial)) = cue {
            if mind.cue_serial != serial {
                mind.cue_clip = clip;
                mind.cue_serial = serial;
            }
        }
    }
}

/// Dev hook for captures: `OPENSPORE_GROUP_TEST=1` puts the avatar 4 units
/// from the nearest friendly herd member whose archetype proposes in groups
/// and that has a free herd mate nearby, targets it and starts its proposal
/// (world state only, no saves).
#[allow(clippy::type_complexity)]
pub fn group_test(
    mut frames: Local<u32>,
    mut done: Local<bool>,
    originals: Option<Res<crate::Originals>>,
    run: Option<Res<RunProgress>>,
    mut encounter: Option<ResMut<crate::creature_actions::Encounter>>,
    mut player: Query<&mut Transform, With<Player>>,
    mut npcs: Query<
        (&WorldId, &Transform, &Npc, &mut CreatureMind, &crate::creature_ecology::HerdMember),
        Without<Player>,
    >,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("OPENSPORE_GROUP_TEST").is_ok()) {
        return;
    }
    *frames += 1;
    if *done || *frames < 20 || *frames > 3000 {
        return;
    }
    let (Some(originals), Some(run)) = (originals, run) else { return };
    let Ok(mut avatar) = player.single_mut() else { return };
    let at = avatar.translation.xz();
    let members: Vec<(u32, Vec2, u32, bool)> = npcs
        .iter()
        .map(|(id, t, npc, mind, herd)| {
            (id.0, t.translation.xz(), herd.species, !npc.hostile && !mind.allied && !mind.defeated && npc.hp > 0.0)
        })
        .collect();
    let mut best: Option<(f32, u32)> = None;
    for (id, t, npc, mind, herd) in &npcs {
        if npc.hostile || mind.allied || mind.defeated || npc.hp <= 0.0 {
            continue;
        }
        if crate::creature_rules::social_group_joiners(herd.archetype.record().social_group_size) == 0 {
            continue;
        }
        let social = levels(member_abilities(&run, &originals, id.0), EncounterMode::Social);
        if social.iter().all(|r| *r == 0) {
            continue;
        }
        let here = t.translation.xz();
        let mates = members
            .iter()
            .filter(|(m, p, s, free)| *m != id.0 && *s == herd.species && *free && p.distance(here) <= GROUP_CALL_RADIUS)
            .count();
        if mates == 0 {
            continue;
        }
        let d = here.distance(at);
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, id.0));
        }
    }
    let Some((_, target)) = best else { return };
    *done = true;
    for (id, t, _, mut mind, herd) in &mut npcs {
        if id.0 != target {
            continue;
        }
        avatar.translation = Vec3::new(t.translation.x, avatar.translation.y, t.translation.z + 4.0);
        avatar.rotation = Quat::IDENTITY;
        let social = levels(member_abilities(&run, &originals, id.0), EncounterMode::Social);
        let wanted = crate::creature_rules::social_group_joiners(herd.archetype.record().social_group_size);
        crate::creature_actions::begin_gather(&mut mind, social, wanted);
        if let Some(e) = encounter.as_mut() {
            e.target = Some(id.0);
            e.mode = EncounterMode::Social;
        }
        info!("group test: creature {} ({:?}) calls {wanted} herd mates at {:?}", id.0, herd.archetype, t.translation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_stand_behind_the_proposer() {
        // Facing the target along +Z, the slots turn 135 / 225 / 180 degrees:
        // behind-left, behind-right and straight behind, 2 x (r + 1) away.
        let p = Vec2::ZERO;
        let target = Vec2::new(0.0, 10.0);
        let r = 0.5;
        let slots: Vec<Vec2> = crate::creature_rules::SOCIAL_GROUP_SLOT_TURNS
            .iter()
            .map(|turn| crate::creature_rules::social_group_slot(p, target, *turn, r))
            .collect();
        for s in &slots {
            assert!((s.length() - 3.0).abs() < 1e-4);
            assert!(s.y < 0.0, "behind the proposer: {s}");
        }
        assert!((slots[0].x + slots[1].x).abs() < 1e-4, "mirror images");
        assert!(slots[2].x.abs() < 1e-4 && (slots[2].y + 3.0).abs() < 1e-4);
        assert_eq!(crate::creature_rules::social_group_slots(2).len(), 2);
        assert_eq!(crate::creature_rules::social_group_slots(3).len(), 3);
    }

    #[test]
    fn joiners_follow_the_archetype_group_size() {
        use crate::creature_ecology::Archetype;
        let joiners = |a: Archetype| crate::creature_rules::social_group_joiners(a.record().social_group_size);
        assert_eq!(joiners(Archetype::Social), 0);
        assert_eq!(joiners(Archetype::MediumSocial), 2);
        assert_eq!(joiners(Archetype::LargeSocial), 3);
        assert_eq!(joiners(Archetype::NightPredator), 1);
    }

    #[test]
    fn a_group_bar_is_longer_and_demonstrations_fill_for_everyone() {
        // Two herd mates joined (n = 2): bar x3, each demonstration 3 x level;
        // the player's answers alone need three times as many levels.
        let ranks = [2, 0, 0, 0];
        let mut alone = CreatureMind { rounds: 2, ..default() };
        propose(&mut alone, ranks);
        let mut group = CreatureMind { rounds: 2, group: 2, ..default() };
        propose(&mut group, ranks);
        assert_eq!(alone.bar_max, 8.0);
        assert_eq!(group.bar_max, 24.0);
        assert_eq!(alone.npc_fill, 2.0);
        assert_eq!(group.npc_fill, 6.0);
        assert_eq!(group.demo_serial, 1);
        // Equal shares: one answer of level 2 fills 2 / 24 of the group bar.
        assert!(alone.impression > group.impression - 1e-3);
    }

    #[test]
    fn a_gathering_creature_waits_then_proposes_with_the_joiners_in_place() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(RunProgress::default());
        app.insert_resource(crate::Originals::default());
        app.add_systems(Update, social_groups);
        app.world_mut().spawn((Player, Transform::from_xyz(0.0, 0.0, 10.0)));
        let ranks = [1, 0, 0, 0];
        let mut proposer = CreatureMind { species: 3, rounds: 1, ..default() };
        crate::creature_actions::begin_gather(&mut proposer, ranks, 2);
        assert!(proposer.gather > 0.0 && !proposer.performing);
        let npc = || Npc { speed: 1.0, hostile: false, hp: 10.0 };
        let p = app.world_mut().spawn((WorldId(1), Transform::IDENTITY, npc(), proposer)).id();
        let near = app
            .world_mut()
            .spawn((WorldId(2), Transform::from_xyz(2.0, 0.0, 0.0), npc(), CreatureMind { species: 3, ..default() }))
            .id();
        let far = app
            .world_mut()
            .spawn((WorldId(3), Transform::from_xyz(40.0, 0.0, 0.0), npc(), CreatureMind { species: 3, ..default() }))
            .id();
        app.update();
        let mind = app.world().get::<CreatureMind>(p).unwrap().clone();
        assert_eq!(mind.joiners, vec![2], "only the herd mate within the call radius answers");
        let joiner = app.world().get::<CreatureMind>(near).unwrap().clone();
        assert_eq!(joiner.joining, Some(1));
        let slot = joiner.slot_at.expect("a slot");
        assert!(app.world().get::<CreatureMind>(far).unwrap().joining.is_none());
        // The joiner steps into its slot: the gathering ends at once with n = 1.
        app.world_mut().get_mut::<Transform>(near).unwrap().translation = Vec3::new(slot[0], 0.0, slot[1]);
        app.update();
        let mind = app.world().get::<CreatureMind>(p).unwrap().clone();
        assert_eq!(mind.gather, 0.0);
        assert_eq!(mind.group, 1);
        assert!(mind.performing);
        assert_eq!(mind.bar_max, 4.0, "2 x level 1 x (n + 1)");
        assert_eq!(mind.npc_fill, 2.0);
        // The joiner copies the demonstration once the proposal runs.
        app.update();
        let joiner = app.world().get::<CreatureMind>(near).unwrap().clone();
        assert_eq!(joiner.cue_serial, mind.demo_serial);
        // The proposal ends: the joiner is let go.
        app.world_mut().get_mut::<CreatureMind>(p).unwrap().performing = false;
        app.update();
        app.update();
        let joiner = app.world().get::<CreatureMind>(near).unwrap().clone();
        assert!(joiner.joining.is_none() && joiner.slot_at.is_none());
        assert!(app.world().get::<CreatureMind>(p).unwrap().joiners.is_empty());
    }

    #[test]
    fn the_befriend_reaction_reaches_the_joiners() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(RunProgress::default());
        app.insert_resource(crate::Originals::default());
        app.add_systems(Update, social_groups);
        app.world_mut().spawn((Player, Transform::from_xyz(0.0, 0.0, 10.0)));
        let mut proposer = CreatureMind { species: 3, rounds: 1, ..default() };
        crate::creature_actions::begin_gather(&mut proposer, [1, 0, 0, 0], 1);
        let npc = || Npc { speed: 1.0, hostile: false, hp: 10.0 };
        let p = app.world_mut().spawn((WorldId(1), Transform::IDENTITY, npc(), proposer)).id();
        let near = app
            .world_mut()
            .spawn((WorldId(2), Transform::from_xyz(2.0, 0.0, 0.0), npc(), CreatureMind { species: 3, ..default() }))
            .id();
        app.update();
        let slot = app.world().get::<CreatureMind>(near).unwrap().slot_at.expect("a slot");
        app.world_mut().get_mut::<Transform>(near).unwrap().translation = Vec3::new(slot[0], 0.0, slot[1]);
        app.update();
        app.update();
        let before = app.world().get::<CreatureMind>(near).unwrap().cue_serial;
        // The player fills the bar: the proposer befriends and reacts (0xD9BBD0).
        {
            let mut mind = app.world_mut().get_mut::<CreatureMind>(p).unwrap();
            mind.allied = true;
            crate::creature_actions::play_cue(&mut mind, crate::creature_rules::SOCIAL_CLIP_CURIOUS_TO_FRIENDLY);
        }
        app.update();
        let joiner = app.world().get::<CreatureMind>(near).unwrap().clone();
        assert_eq!(joiner.cue_clip, crate::creature_rules::SOCIAL_CLIP_CURIOUS_TO_FRIENDLY);
        assert_ne!(joiner.cue_serial, before, "a new cue starts the clip");
        assert!(joiner.joining.is_none(), "released");
    }
}
