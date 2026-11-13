//! Roaming epic and rogue creatures. Spore's Creature stage scatters a few
//! giant, very aggressive creatures across the planet; a well-built pack can
//! bring one down, and only top-rank performances can win one over. Rogues
//! are solitary aggressive loners without a nest.
//!
//! The visuals are installed original creature models at 3.5x (epic) or
//! 1.35x (rogue) scale. Epic creatures attack with the retail epic stomp
//! (6 damage every 3 s) and every hit uses retail HP-scale numbers; their
//! health, territory, reach, DNA base and strength rating are OpenSpore
//! adaptations of values the species records hold in the original. Roamers are
//! deliberately not `Npc` nest members: they
//! do not use nest respawning or the nest-member population budget, and
//! their persistent outcome lives in `SpeciesWorld::roamers`.
use crate::{
    asset_catalog::Capabilities,
    creature_actions::{self, CreatureMind, Encounter, EncounterMode},
    game::*,
    persistence::WorldId,
    species::{Relationship, RoamerKind},
    stages::StageOwned,
};
use bevy::prelude::*;

#[derive(Component, Debug, Clone)]
pub struct RoamerActor {
    pub id: u32,
    pub kind: RoamerKind,
    pub lair: Vec3,
    /// Angered: chases the player until it is leashed back to its lair.
    pub aggressive: bool,
    pub attack_timer: f32,
    pub goal: Vec3,
    pub wait: f32,
    /// Wander-goal sequence counter (deterministic, no RNG state).
    pub step: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoamerStats {
    pub max_hp: f32,
    pub speed: f32,
    /// Multiplier on the ability damage it deals (retail has none: 1.0).
    pub damage_scale: f32,
    /// Strength rating for the DNA multiplier (`2 * brain - strength`): the
    /// archetype's `FoodChainLevel` (epic 10, miniboss 9; VERIFIED 0xC09B40).
    pub strength: i32,
    /// Its combat ranks are at least this (giants bite hard).
    pub min_combat_rank: u8,
    /// The player's performance rank needed for a social match to count.
    pub required_social_rank: u8,
    /// Its own performance rank (the social contest's opposing rank).
    pub social_rank: u8,
    /// Distance at which it notices and charges the player.
    pub aggro: f32,
    /// Wander radius around its lair.
    pub territory: f32,
    /// It gives up a chase once the player is this far from its lair.
    pub leash: f32,
    pub dna: f32,
    /// Horizontal radius of the scaled body; encounter distances are
    /// measured from its edge, not its centre.
    pub body_radius: f32,
}


/// ADAPTED: encounter distances are measured from the body edge: about 0.75
/// of the actor scale for the 2.5-unit fitted footprint.
const BODY_RADIUS_PER_SCALE: f32 = 0.75;

/// Roamer stats. Retail (notes C4B, herd archetypes `npc_epic_predator` /
/// `npc_miniboss`): max HP (`HitpointOverride` 1000 / 250), the damage
/// multiplier on every hit (100 / 2), size (20 capped / 5.7) and territory.
/// ADAPTED: speeds, perception (`aggro`), leash, strength rating, the social
/// rank gates (a stand-in for `socialPower` 5000 / 800, whose use was not
/// traced) and the legacy-world DNA base.
pub fn stats(kind: RoamerKind) -> RoamerStats {
    let archetype = kind.archetype();
    let record = archetype.record();
    // A roamer's wander radius is its archetype's `TerritoryRadius` (epic 150,
    // miniboss 200) on the planet (the island capped it at 8 units).
    let territory = (record.territory * crate::creature_ecology::CREATURE_WORLD_SCALE).max(3.0);
    let body_radius = crate::creature_ecology::species_scale(archetype) * BODY_RADIUS_PER_SCALE;
    match kind {
        RoamerKind::Epic => RoamerStats {
            max_hp: record.hit_points,
            speed: 2.3,
            damage_scale: record.damage_multiplier,
            strength: record.food_chain as i32,
            min_combat_rank: 3,
            required_social_rank: 5,
            social_rank: 5,
            aggro: 7.0,
            territory,
            leash: territory + 8.0,
            dna: 100.0,
            body_radius,
        },
        RoamerKind::Rogue => RoamerStats {
            max_hp: record.hit_points,
            speed: 3.4,
            damage_scale: record.damage_multiplier,
            strength: record.food_chain as i32,
            min_combat_rank: 3,
            required_social_rank: 4,
            social_rank: 4,
            aggro: 5.5,
            territory,
            leash: territory + 8.0,
            dna: 100.0,
            body_radius,
        },
    }
}

/// Pure: a roamer's ranks from its displayed model's native anatomy, raised
/// to its kind's minimum combat and performance ranks (capped at 5).
pub fn boost(native: Capabilities, stats: RoamerStats) -> Capabilities {
    let mut a = native;
    let combat = stats.min_combat_rank.min(5);
    a.bite = a.bite.max(combat).min(5);
    a.strike = a.strike.max(combat.saturating_sub(1)).min(5);
    a.charge = a.charge.max(combat.saturating_sub(1)).min(5);
    let social = stats.social_rank.min(5);
    let mut any = false;
    for rank in [&mut a.sing, &mut a.dance, &mut a.charm, &mut a.pose] {
        if *rank > 0 {
            *rank = (*rank).max(social).min(5);
            any = true;
        }
    }
    if !any {
        a.sing = social;
        a.pose = social;
    }
    a
}

/// Capabilities of a roamer by persistent id, or `None` for other ids.
pub fn abilities(run: &RunProgress, originals: &crate::Originals, id: u32) -> Option<Capabilities> {
    let roamer = run.species_world.roamer(id)?;
    let native = crate::baked_model::species_key(originals, roamer.visual)
        .and_then(|key| originals.original_species.get(&key))
        .and_then(|resolved| resolved.as_ref().ok())
        .map(|species| species.capabilities)
        .unwrap_or_default();
    Some(boost(native, stats(roamer.kind)))
}

/// Native part definitions a roamer's original model carries (for its part
/// reward), empty when no installed anatomy resolved.
pub fn carried_parts(run: &RunProgress, originals: &crate::Originals, id: u32) -> Vec<u32> {
    run.species_world
        .roamer(id)
        .and_then(|r| crate::baked_model::species_key(originals, r.visual))
        .and_then(|key| originals.original_species.get(&key))
        .and_then(|resolved| resolved.as_ref().ok())
        .map(|species| {
            species
                .parts
                .iter()
                .map(|p| p.definition.instance_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Pure: the `step`-th wander goal inside a territory (the caller keeps it on
/// land, `creature_planet::Land::land_toward`).
pub fn wander_goal(lair: Vec3, territory: f32, id: u32, step: u32) -> Vec3 {
    let key = (id as f32 * 12.9898 + step as f32 * 78.233).sin() * 43_758.547;
    let roll = key - key.floor();
    let angle = roll * std::f32::consts::TAU;
    let reach = territory * (0.35 + 0.65 * ((roll * 7.31).fract()));
    let at = lair + Vec3::new(angle.cos() * reach, 0.0, angle.sin() * reach);
    Vec3::new(at.x, 0.0, at.z)
}

/// ADAPTED: how far from its body edge an epic reaches with its stomp (the
/// retail stomp is a 15-unit cone from the giant).
const EPIC_STOMP_REACH: f32 = 3.0;
/// `epicstomp` clip (`creatureAbilityAnimationIDs`) when the prop is not read.
const EPIC_STOMP_CLIP: u32 = 0x04BC_04AB;

/// Spawn every living roamer (scheduled before checkpoint restore so its
/// saved position and health apply). Old saves gain roamers here once.
pub fn setup(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !run.species_world.roamers_generated && run.species_world.generated {
        run.species_world.ensure_roamers();
        run.request_save();
    }
    // Retail sizes (archetype OverrideSize), also for roamers saved earlier.
    for roamer in run.species_world.roamers.iter_mut() {
        roamer.scale = crate::creature_ecology::species_scale(roamer.kind.archetype());
    }
    let roamers = run.species_world.roamers.clone();
    for roamer in roamers {
        if !roamer.relationship.is_alive() {
            continue;
        }
        spawn_roamer(
            &mut commands,
            &mut originals,
            &mut meshes,
            &mut images,
            &mut materials,
            &roamer,
        );
    }
}

/// Spawn one roamer actor with its installed model (or a tinted sphere).
fn spawn_roamer(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    roamer: &crate::species::Roamer,
) {
    let s = stats(roamer.kind);
    let lair = Vec3::new(roamer.lair[0], 0.0, roamer.lair[2]);
    let entity = commands
        .spawn((
            StageOwned,
            WorldId(roamer.id),
            RoamerActor {
                id: roamer.id,
                kind: roamer.kind,
                lair,
                aggressive: false,
                attack_timer: 0.0,
                goal: lair,
                wait: 1.0,
                step: 0,
            },
            Health::new(s.max_hp),
            CreatureMind {
                species: u8::MAX,
                allied: roamer.relationship == Relationship::Allied,
                rounds: roamer.kind.archetype().record().social_proposals,
                ..default()
            },
            Visibility::Inherited,
            Transform::from_xyz(lair.x, -0.6 + roamer.scale, lair.z)
                .with_scale(Vec3::splat(roamer.scale)),
            Name::new(format!("{}-{}", roamer.name, roamer.id)),
        ))
        .id();
    // The installed model pool supplies the giant; resolve its native
    // anatomy too so its abilities and part reward are the original's.
    if let Some(key) = crate::baked_model::species_key(originals, roamer.visual) {
        match crate::baked_model::ensure(originals, key, meshes, images, materials) {
            Ok(_) => {
                if !originals.original_species.contains_key(&key) {
                    let native = crate::original_species::resolve(
                        &originals.resource_index,
                        &originals.part_catalog,
                        key,
                    );
                    originals.original_species.insert(key, native);
                }
            }
            Err(error) => warn!("{} model {key:?}: {error}", roamer.name),
        }
    }
    if !crate::baked_model::spawn_species_visual(commands, originals, meshes, entity, roamer.visual) {
        let tint = match roamer.kind {
            RoamerKind::Epic => Color::srgb(0.42, 0.16, 0.30),
            RoamerKind::Rogue => Color::srgb(0.55, 0.30, 0.12),
        };
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(24, 12))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: tint,
                perceptual_roughness: 0.7,
                ..default()
            })),
        ));
    }
}

/// The epic predator is never extinct (`NumToKillForExtinction` -1): a slain
/// one returns to its lair after `RespawnRate` (600 s). The miniboss is extinct
/// at its first kill and stays dead.
#[allow(clippy::too_many_arguments)]
pub fn respawn(
    time: Res<Time>,
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut originals: ResMut<crate::Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roamers: Query<(
        &WorldId,
        &mut RoamerActor,
        &mut Health,
        &mut CreatureMind,
        &mut Visibility,
        &mut Transform,
    )>,
) {
    if !run.species_world.is_retail() {
        return;
    }
    let back = run.species_world.tick_roamer_respawns(time.delta_secs());
    for id in back {
        let Some(roamer) = run.species_world.roamer(id).cloned() else {
            continue;
        };
        if let Some((_, mut actor, mut health, mut mind, mut visibility, mut t)) =
            roamers.iter_mut().find(|(world_id, ..)| world_id.0 == id)
        {
            *mind = CreatureMind {
                species: u8::MAX,
                rounds: roamer.kind.archetype().record().social_proposals,
                ..default()
            };
            health.hp = health.max;
            actor.aggressive = false;
            actor.goal = actor.lair;
            actor.wait = 1.0;
            t.translation.x = actor.lair.x;
            t.translation.z = actor.lair.z;
            *visibility = Visibility::Inherited;
        } else {
            spawn_roamer(
                &mut commands,
                &mut originals,
                &mut meshes,
                &mut images,
                &mut materials,
                &roamer,
            );
        }
        run.request_save();
    }
}

/// Roamer behaviour: wander the territory, charge a player who comes close,
/// attack the player and pack members in reach, give up at the leash, and
/// hold still while a high-rank performance has its attention.
#[allow(clippy::too_many_arguments)]
pub fn roam(
    time: Res<Time>,
    originals: Res<crate::Originals>,
    run: Res<RunProgress>,
    mut encounter: ResMut<Encounter>,
    mut player: Query<(&Transform, &mut Health), With<Player>>,
    mut roamers: Query<
        (
            &WorldId,
            &mut Transform,
            &mut RoamerActor,
            &mut CreatureMind,
            &mut Visibility,
        ),
        (Without<Player>, Without<Npc>),
    >,
    mut npcs: Query<
        (
            &WorldId,
            &Transform,
            &mut Npc,
            &mut CreatureMind,
            &mut Visibility,
        ),
        (Without<Player>, Without<RoamerActor>),
    >,
    surface: Option<Res<crate::terrain::LandSurface>>,
) {
    let dt = time.delta_secs();
    let Ok((pt, mut player_health)) = player.single_mut() else {
        return;
    };
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let player_position = pt.translation;
    let player_alive = !player_health.dead();
    let mut pack: Vec<(u32, Vec3)> = npcs
        .iter()
        .filter(|(id, _, npc, mind, _)| {
            run.creature_pack.members.contains(&id.0)
                && mind.allied
                && !mind.defeated
                && mind.downed <= 0.0
                && npc.hp > 0.0
        })
        .map(|(id, t, _, _, _)| (id.0, t.translation))
        .collect();
    for (id, t, _, mind, _) in &roamers {
        if run.creature_pack.members.contains(&id.0) && mind.allied && !mind.defeated {
            pack.push((id.0, t.translation));
        }
    }
    let mut pack_hits: Vec<(u32, f32)> = Vec::new();
    for (id, mut t, mut actor, mut mind, mut visibility) in &mut roamers {
        if mind.defeated {
            *visibility = Visibility::Hidden;
            continue;
        }
        let position = t.translation;
        // Beyond the casting manager's deactivation distance a roamer is
        // dormant (the island parked it outside its square).
        if position.distance(player_position) > crate::creature_ecology::casting_thresholds().1 {
            continue;
        }
        let s = stats(actor.kind);
        actor.attack_timer = (actor.attack_timer - dt).max(0.0);

        let in_pack = run.creature_pack.members.contains(&id.0) && mind.allied;
        if in_pack {
            let slot = run.creature_pack.members.iter().position(|m| *m == id.0).unwrap_or(0);
            let offset = match slot {
                0 => Vec3::new(-2.2, 0., 3.4),
                1 => Vec3::new(2.2, 0., 3.4),
                _ => Vec3::new(0., 0., 5.2),
            };
            let player_forward = pt.rotation * Vec3::NEG_Z;
            let pack_goal = match run.creature_pack.stance {
                crate::creature_pack::PackStance::Stay => {
                    let base = run.creature_pack.stay_position.map(Vec3::from_array).unwrap_or(player_position);
                    base + offset
                }
                crate::creature_pack::PackStance::Attack => {
                    let enemy = encounter.target.and_then(|t_id| {
                        npcs.iter().find(|(npc_id, _, npc, m, _)| npc_id.0 == t_id && !m.allied && npc.hp > 0.0)
                            .map(|(_, tr, _, _, _)| tr.translation)
                    });
                    enemy.unwrap_or_else(|| player_position + pt.rotation * offset)
                }
                crate::creature_pack::PackStance::Follow => {
                    let enemy = (encounter.mode == EncounterMode::Combat).then(|| {
                        encounter.target.and_then(|t_id| {
                            npcs.iter().find(|(npc_id, _, npc, m, _)| npc_id.0 == t_id && !m.allied && npc.hp > 0.0)
                                .map(|(_, tr, _, _, _)| tr.translation)
                        })
                    }).flatten();
                    enemy.unwrap_or_else(|| player_position + pt.rotation * offset)
                }
            };
            let delta = Vec3::new(pack_goal.x - position.x, 0.0, pack_goal.z - position.z);
            let dist = delta.length();
            if dist > 0.5 {
                let step = delta / dist * (s.speed * 1.2 * dt).min(dist - 0.5);
                t.translation = land.step(t.translation, t.translation + step);
                face_toward(&mut t, pack_goal);
            } else {
                let look_target = t.translation + player_forward;
                face_toward(&mut t, look_target);
            }
            continue;
        }
        let to_player = dist_xz(player_position, position);
        let player_gap = (to_player - s.body_radius).max(0.0);
        let player_from_lair = dist_xz(player_position, actor.lair);
        let charmed = encounter.target == Some(id.0)
            && encounter.mode == EncounterMode::Social
            && mind.response_time > 0.0
            && !actor.aggressive;
        if mind.allied {
            actor.aggressive = false;
        } else if !charmed {
            if player_alive && player_gap <= s.aggro && player_from_lair <= s.leash {
                actor.aggressive = true;
            }
            if actor.aggressive && (!player_alive || player_from_lair > s.leash + 2.0) {
                actor.aggressive = false;
                actor.goal = actor.lair;
                actor.wait = 0.0;
            }
        }
        // Movement. A stunned giant (charge hit) stands still.
        let mut goal = None;
        if mind.stunned > 0.0 {
            // rooted
        } else if charmed {
            face_toward(&mut t, player_position);
        } else if actor.aggressive && player_alive {
            if player_gap > 1.2 {
                goal = Some((player_position, s.speed * 1.35, s.body_radius + 1.0));
            } else {
                face_toward(&mut t, player_position);
            }
        } else if actor.wait > 0.0 {
            actor.wait -= dt;
        } else if dist_xz(position, actor.goal) <= 0.7 {
            actor.step = actor.step.wrapping_add(1);
            let goal = wander_goal(actor.lair, s.territory, actor.id, actor.step);
            let goal = land.land_toward(actor.lair.xz(), goal.xz());
            actor.goal = Vec3::new(goal.x, 0.0, goal.y);
            actor.wait = 2.0 + (actor.step % 4) as f32;
        } else {
            goal = Some((actor.goal, s.speed * 0.55, 0.5));
        }
        if let Some((target, speed, stop)) = goal {
            let delta = Vec3::new(target.x - position.x, 0.0, target.z - position.z);
            let distance = delta.length();
            if distance > stop {
                let step = delta / distance * (speed * dt).min(distance - stop);
                let next = land.step(t.translation, t.translation + step);
                if next.xz() == t.translation.xz() && !actor.aggressive {
                    // The shore blocks the way: pick the next goal.
                    actor.goal = t.translation;
                }
                t.translation = next;
                face_toward(&mut t, target);
            }
        }
        // Attacks: angry (or wild-and-crowded) roamers strike whoever is in
        // reach, player first on ties. Allies and charmed giants never do.
        if mind.allied
            || charmed
            || actor.attack_timer > 0.0
            || !actor.aggressive
            || mind.stunned > 0.0
        {
            continue;
        }
        let Some(caps) = abilities(&run, &originals, id.0) else {
            continue;
        };
        let ranks = creature_actions::levels(caps, EncounterMode::Combat);
        let mut victim: Option<(Option<u32>, f32)> = player_alive.then_some((None, player_gap));
        for (member, at) in &pack {
            let gap = (dist_xz(*at, t.translation) - s.body_radius).max(0.0);
            if victim.is_none_or(|(_, best)| gap < best) {
                victim = Some((Some(*member), gap));
            }
        }
        let Some((who, gap)) = victim else {
            continue;
        };
        // Epics stomp (CreatureAbilities!epicstomp: 6 damage, 3 s refresh);
        // rogues use the abilities of their parts at the retail per-level values.
        let (label, damage, recharge, effect, clip) = if actor.kind == RoamerKind::Epic {
            if gap > EPIC_STOMP_REACH {
                continue;
            }
            (
                "Stomp",
                // The epic predator's DamageMultiplier (100) on the stomp (C2D).
                crate::creature_rules::EPIC_STOMP_DAMAGE * s.damage_scale,
                crate::creature_rules::EPIC_STOMP_REFRESH,
                crate::creature_rules::CombatEffect::None,
                // `epicstomp` creatureAbilityAnimationIDs (0x04BC04AB).
                Some(originals.creature_abilities.clip(41, 0).map(|c| c.animation).unwrap_or(EPIC_STOMP_CLIP)),
            )
        } else {
            let Some((choice, ability)) =
                creature_actions::pack_attack(&originals.creature_abilities, ranks, gap)
            else {
                continue;
            };
            (
                creature_actions::COMBAT[choice],
                ability.damage * s.damage_scale,
                ability.recharge,
                ability.effect,
                creature_actions::ability_clip(&originals.creature_abilities, EncounterMode::Combat, choice, ability.level),
            )
        };
        actor.attack_timer = recharge.max(1.2);
        if let Some(clip) = clip {
            mind.attack_clip = clip;
            mind.attack_serial = mind.attack_serial.wrapping_add(1);
        }
        let name = run
            .species_world
            .roamer(id.0)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        match who {
            None => {
                player_health.hurt(damage);
                creature_actions::hurt_avatar_with_effect(&mut encounter, effect);
                encounter.message = format!("The {} uses {} on you!", name.to_lowercase(), label);
            }
            Some(member) => pack_hits.push((member, damage)),
        }
    }
    for (member, damage) in pack_hits {
        if let Some((_, _, mut npc, mut mind, mut visibility)) =
            npcs.iter_mut().find(|(id, _, _, _, _)| id.0 == member)
        {
            npc.hp = (npc.hp - damage).max(0.0);
            if npc.hp <= 0.0 {
                crate::creature_social::knock_out(&mut npc, &mut mind, &mut visibility);
                encounter.message = "A pack member was knocked out and fled to its nest".into();
            }
        } else if let Some((_, _, _, mut mind, mut visibility)) =
            roamers.iter_mut().find(|(id, _, _, _, _)| id.0 == member)
        {
            mind.defeated = true;
            *visibility = Visibility::Hidden;
            encounter.message = "A rogue pack member was knocked out in battle".into();
        }
    }
}

fn face_toward(t: &mut Transform, target: Vec3) {
    let flat = Vec3::new(target.x - t.translation.x, 0.0, target.z - t.translation.z);
    if flat.length_squared() > 1e-4 {
        let flat = flat.normalize();
        let wanted = Quat::from_rotation_y((-flat.x).atan2(-flat.z));
        t.rotation = t.rotation.slerp(wanted, 0.2);
    }
}

/// Dev hook for captures: `OPENSPORE_EPIC_TEST=<gap>` keeps the avatar `gap`
/// units south of the nearest epic roamer, facing it, from frame 20 on (the
/// giant stays in the follow camera's frame; world state only, no saves).
pub fn epic_test(
    mut frames: Local<u32>,
    mut player: Query<&mut Transform, (With<crate::game::Player>, Without<RoamerActor>)>,
    roamers: Query<(&Transform, &RoamerActor)>,
) {
    static GAP: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let Some(gap) = *GAP.get_or_init(|| std::env::var("OPENSPORE_EPIC_TEST").ok().and_then(|v| v.parse().ok())) else {
        return;
    };
    *frames += 1;
    if *frames < 20 {
        return;
    }
    let Ok(mut avatar) = player.single_mut() else { return };
    let at = avatar.translation;
    let Some((t, _)) = roamers
        .iter()
        .filter(|(_, r)| r.kind == RoamerKind::Epic)
        .min_by(|a, b| a.0.translation.xz().distance(at.xz()).total_cmp(&b.0.translation.xz().distance(at.xz())))
    else {
        return;
    };
    if *frames == 20 {
        info!("epic test: giant at {:?}, avatar {gap} units south", t.translation);
    }
    avatar.translation = Vec3::new(t.translation.x, avatar.translation.y, t.translation.z + gap);
    avatar.rotation = Quat::IDENTITY;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epic_ranks_are_raised_but_capped_and_always_perform() {
        let epic = stats(RoamerKind::Epic);
        let boosted = boost(Capabilities::default(), epic);
        assert_eq!(boosted.bite, 3);
        assert_eq!((boosted.sing, boosted.pose), (5, 5));
        let strong = boost(
            Capabilities {
                bite: 5,
                dance: 2,
                ..Capabilities::default()
            },
            epic,
        );
        assert_eq!(strong.bite, 5);
        assert_eq!(strong.dance, 5);
        assert_eq!(
            strong.sing, 0,
            "a native performer keeps its own repertoire"
        );
        assert!(epic.required_social_rank >= 4, "epics want very high ranks");
        assert!(stats(RoamerKind::Rogue).max_hp < epic.max_hp);
    }

    #[test]
    fn wander_goals_stay_inside_the_territory() {
        let lair = Vec3::new(-19.0, 0.0, -19.0);
        for step in 0..64 {
            let goal = wander_goal(lair, 150.0, 1300, step);
            assert!(dist_xz(goal, lair) <= 150.0 + 1e-3);
            assert!(dist_xz(goal, lair) >= 0.35 * 150.0 - 1e-3);
        }
        assert_ne!(
            wander_goal(lair, 4.5, 1300, 1),
            wander_goal(lair, 4.5, 1300, 2)
        );
    }
}
