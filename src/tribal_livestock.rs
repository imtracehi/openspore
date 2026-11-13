//! Tribal domestication and food gathering. Broad behavior follows the manual;
//! prices, work time, production and capacity are explicit prototype tuning.
use super::*;
use serde::{Deserialize, Serialize};

/// The price of one feeding without a herd record: the species property 0xEB69140E's
/// loader default (VERIFIED, `tribal_rules::FEED_COST_DEFAULT`). An animal of a tribal
/// herd pays its archetype's price (`tribal_rules::feed_cost_of`: 5 / 10 / 15).
///
/// Taming itself costs nothing: the tame test and conversion 0xDB5E10 (called by
/// FeedWild's last stage, 0xD9E12B, and by the command handler for an animal that is
/// already a friend, 0xCD13F2) pay no food and the Tame action (25, 0xCD1405) only
/// celebrates; the one price is what each feeding pays at FeedWild stage 1 (VERIFIED,
/// notes T15).
pub const TAME_FOOD: f32 = crate::tribal_rules::FEED_COST_DEFAULT;
/// Seconds one feeding takes at the animal: the member's `tame_give_apple` clip
/// (FeedWild stage 3, 0xD9DC40; the group 0x05654DF5, ANIM 0xE2B8019F: 152 frames at
/// 30 fps, VERIFIED from the installed data). ADAPTED: the animal's own answer
/// (the status 0x200 it consumes, stage 4) takes no time here; the earlier port
/// used a flat 4 s.
pub const TAME_SECONDS: f32 = 152.0 / 30.0;
/// `maxNumDomesticatedAnimals` (tuning).
pub const MAX_LIVESTOCK: usize = crate::tribal_rules::MAX_DOMESTICATED_ANIMALS;
pub const EGG_SECONDS: f32 = 15.0;
pub const EGG_FOOD: f32 = 2.0;
pub const EGG_CAPACITY: f32 = 40.0;
pub const ID_BASE: u32 = 1_100;
// Adapted combat tuning; no recovered retail formula is implied.
pub const GUARD_RADIUS: f32 = 8.0;
pub const GUARD_LEASH: f32 = 12.0;
pub const PET_REACH: f32 = 1.8;
pub const PET_DPS: f32 = 8.0;

fn pet_health() -> f32 {
    40.0
}
fn pet_scale() -> f32 {
    0.6
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LivestockRecord {
    pub slot: u32,
    pub species: u32,
    pub position: [f32; 3],
    pub health: f32,
    pub egg_seconds: f32,
    #[serde(default)]
    pub pack_member: Option<u32>,
    #[serde(default = "pet_health")]
    pub max_health: f32,
    #[serde(default = "pet_scale")]
    pub scale: f32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct DomesticAnimal(pub u32);

/// Feedings an animal has taken so far: the interaction counter the retail
/// manager keeps (0xBA43E0, `tribal_rules::interaction_state`).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Fed(pub u32);

/// The price of one feeding of `animal` and the interaction counts that
/// befriend it: its herd archetype's (VERIFIED), else the default price and
/// one feeding (ADAPTED: an animal without a herd record, the synthetic worlds
/// of the tests).
pub fn feeding_of(world: &World, animal: Entity) -> (f32, [u32; 3]) {
    match world.get::<HerdMember>(animal) {
        Some(h) => {
            let name = crate::tribal_rules::herd_archetype(h.herd).name;
            (crate::tribal_rules::feed_cost_of(name), crate::tribal_rules::impress_counts(name))
        }
        None => (TAME_FOOD, [1, 1, 1]),
    }
}

/// Runtime target only: never serialize ECS entity IDs into the campaign.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PetGuard {
    pub target: Option<Entity>,
}

/// Capture the recruited, living foreign species before Creature checkpoints
/// are cleared. The transfer consumes no food and is committed only once.
pub fn inherit_pack(run: &mut RunProgress) {
    if run.tribal_campaign.pack_inherited {
        return;
    }
    run.tribal_campaign.pack_inherited = true;
    normalize(
        &mut run.tribal_campaign.livestock,
        &mut run.tribal_campaign.livestock_eggs,
    );
    let mut seen = Vec::new();
    for &member in &run.creature_pack.members {
        if seen.contains(&member) || run.tribal_campaign.livestock.len() >= MAX_LIVESTOCK {
            continue;
        }
        seen.push(member);
        if run
            .checkpoint
            .as_ref()
            .filter(|c| c.stage == 1)
            .is_some_and(|c| {
                c.objects.iter().any(|o| {
                    o.id == member
                        && o.creature
                            .as_ref()
                            .map(|c| c.hp)
                            .or(o.hp)
                            .is_some_and(|hp| !hp.is_finite() || hp <= 0.0)
                })
            })
        {
            continue;
        }
        let identity = run
            .species_world
            .species_for_member(member)
            .filter(|_| {
                run.species_world.relationship(member) == Some(crate::species::Relationship::Allied)
            })
            .map(|species| (species.id, 40.0))
            .or_else(|| {
                run.species_world
                    .roamer(member)
                    .filter(|r| {
                        r.kind == crate::species::RoamerKind::Rogue
                            && r.relationship == crate::species::Relationship::Allied
                    })
                    .map(|r| (r.visual, 80.0))
            });
        let Some((species, max_health)) = identity else {
            continue;
        };
        let slot = (0..MAX_LIVESTOCK as u32)
            .find(|slot| {
                !run.tribal_campaign
                    .livestock
                    .iter()
                    .any(|p| p.slot == *slot)
            })
            .unwrap();
        run.tribal_campaign.livestock.push(LivestockRecord {
            slot,
            species,
            position: (pen_position() + Vec3::new(slot as f32 - 1.0, 0.0, 1.7)).to_array(),
            health: max_health,
            egg_seconds: 0.0,
            pack_member: Some(member),
            max_health,
            scale: 0.45,
        });
    }
}

/// Species identity accompanies the actual original wildlife body, so taming
/// and reloading cannot silently substitute a different creature.
#[derive(Component, Debug, Clone, Copy)]
pub struct WildlifeSpecies(pub u32);

/// A logical food source at the existing original egg pen. It shares the
/// normal gather/carry/deliver path; eggs never credit the stockpile directly.
#[derive(Component)]
pub struct EggPile;

pub fn pen_position() -> Vec3 {
    tribal::egg_pen_position()
}

pub fn normalize(records: &mut Vec<LivestockRecord>, eggs: &mut f32) {
    let mut seen = Vec::new();
    records.retain_mut(|record| {
        if record.slot >= MAX_LIVESTOCK as u32 || seen.contains(&record.slot) {
            return false;
        }
        seen.push(record.slot);
        if !Vec3::from_array(record.position).is_finite() {
            record.position = pen_position().to_array();
        }
        record.position[1] = 0.0;
        record.max_health = if record.max_health.is_finite() {
            record.max_health.clamp(1.0, 80.0)
        } else {
            pet_health()
        };
        record.scale = if record.scale.is_finite() {
            record.scale.clamp(0.3, 0.8)
        } else {
            pet_scale()
        };
        record.health = if record.health.is_finite() {
            record.health.clamp(0.0, record.max_health)
        } else {
            record.max_health
        };
        record.egg_seconds = if record.egg_seconds.is_finite() {
            record.egg_seconds.clamp(0.0, EGG_SECONDS)
        } else {
            0.0
        };
        record.health > 0.0
    });
    *eggs = if eggs.is_finite() {
        eggs.clamp(0.0, EGG_CAPACITY)
    } else {
        0.0
    };
}

#[allow(clippy::too_many_arguments)]
pub(super) fn restore(
    commands: &mut Commands,
    originals: &mut crate::Originals,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    campaign: &tribal::TribalCampaign,
) {
    for record in &campaign.livestock {
        let entity = spawn_creature(
            commands,
            originals,
            meshes,
            images,
            materials,
            record.species,
            Vec3::from_array(record.position),
            record.scale,
            Color::srgb(0.70, 0.62, 0.40),
            "domesticated creature".into(),
        );
        commands.entity(entity).insert((
            DomesticAnimal(record.slot),
            PetGuard::default(),
            WorldId(ID_BASE + record.slot),
            Health {
                hp: record.health,
                max: record.max_health,
            },
        ));
    }
}

/// Exclusive only within the serial Tribal simulation chain. Commands from
/// unit movement have been applied; other stages and their resources are idle.
pub fn update(world: &mut World) {
    let _layout = crate::tribal_planet::scope_world(world);
    let dt = world.resource::<Time>().delta_secs().clamp(0.0, 0.25);
    let mut run = world
        .remove_resource::<RunProgress>()
        .expect("run progress");
    normalize(
        &mut run.tribal_campaign.livestock,
        &mut run.tribal_campaign.livestock_eggs,
    );

    // Preserve food already removed by real gatherers before adding new eggs.
    let pool = world
        .query_filtered::<(Entity, &Bush), With<EggPile>>()
        .iter(world)
        .next()
        .map(|(e, b)| (e, b.food_left));
    if let Some((_, food)) = pool {
        run.tribal_campaign.livestock_eggs = food.clamp(0.0, EGG_CAPACITY);
    }
    let orders: Vec<_> = world
        .query::<(Entity, &Transform, &UnitTask, &Health)>()
        .iter(world)
        .filter_map(|(e, t, task, health)| match task {
            UnitTask::Domesticate(target) if !health.dead() => Some((e, t.translation, *target)),
            _ => None,
        })
        .collect();
    for (member, position, target) in orders {
        let animal = world
            .get::<WildAnimal>(target)
            .zip(world.get::<WildlifeSpecies>(target))
            .zip(world.get::<Transform>(target))
            .zip(world.get::<Health>(target))
            .map(|(((animal, species), pose), health)| {
                (
                    !animal.hostile && !health.dead(),
                    species.0,
                    pose.translation,
                    health.hp,
                )
            });
        let Some((true, species, at, health)) = animal else {
            if let Some(mut task) = world.get_mut::<UnitTask>(member) {
                *task = UnitTask::Follow;
            }
            continue;
        };
        // Installed text 5FCCA927:051CCA6A rejects a second animal of a
        // species already owned; evaluate again at completion for races.
        if run
            .tribal_campaign
            .livestock
            .iter()
            .any(|pet| pet.species == species)
        {
            run.tribal_campaign
                .say("An animal of this species is already domesticated.");
            if let Some(mut task) = world.get_mut::<UnitTask>(member) {
                *task = UnitTask::Follow;
            }
            continue;
        }
        let (cost, counts) = feeding_of(world, target);
        if run.tribal_campaign.livestock.len() >= MAX_LIVESTOCK || run.dna_this_stage < cost {
            let reason = if run.tribal_campaign.livestock.len() >= MAX_LIVESTOCK {
                "The animal pen is full (3 creatures).".to_string()
            } else {
                format!("Need {cost:.0} food to feed a wild grazer.")
            };
            run.tribal_campaign.say(reason);
            if let Some(mut task) = world.get_mut::<UnitTask>(member) {
                *task = UnitTask::Follow;
            }
            continue;
        }
        if dist_xz(position, at) > WORK_REACH + 0.1 {
            continue;
        }
        let ready = if let Some(mut unit) = world.get_mut::<TribeUnit>(member) {
            unit.work += dt;
            unit.work >= TAME_SECONDS
        } else {
            false
        };
        if !ready {
            continue;
        }
        let slot = (0..MAX_LIVESTOCK as u32)
            .find(|slot| {
                !run.tribal_campaign
                    .livestock
                    .iter()
                    .any(|pet| pet.slot == *slot)
            })
            .unwrap();
        // One feeding (FeedWild stage 1: the stock pays `cost`, 0xC94960) counts as
        // one interaction of the animal; it befriends it at the best friends
        // count (state 6, 0xBA43E0), which ends the feeding and starts Tame.
        run.dna_this_stage -= cost;
        let fed = world.get::<Fed>(target).map_or(0, |f| f.0) + 1;
        if crate::tribal_rules::interaction_state(fed, counts, 3) != 6 {
            world.entity_mut(target).insert(Fed(fed));
            if let Some(mut unit) = world.get_mut::<TribeUnit>(member) {
                unit.work = 0.0;
            }
            run.tribal_campaign.say(format!("The animal takes the food ({fed}/{}). Keep feeding it.", counts[2]));
            run.request_save();
            continue;
        }
        run.tribal_campaign.livestock.push(LivestockRecord {
            slot,
            species,
            position: [at.x, 0.0, at.z],
            health,
            egg_seconds: 0.0,
            pack_member: None,
            max_health: 40.0,
            scale: 0.6,
        });
        world.entity_mut(target).remove::<WildAnimal>().insert((
            DomesticAnimal(slot),
            PetGuard::default(),
            WorldId(ID_BASE + slot),
            Name::new("domesticated creature"),
        ));
        if let Some(mut task) = world.get_mut::<UnitTask>(member) {
            *task = UnitTask::Follow;
        }
        if let Some(mut unit) = world.get_mut::<TribeUnit>(member) {
            unit.work = 0.0;
        }
        run.tribal_campaign.say(
            "Creature domesticated! It returns to the pen and produces eggs. Gather them with J.",
        );
        run.request_save();
    }

    let threats: Vec<_> = world
        .query::<(
            Entity,
            &Transform,
            &Health,
            Option<&WildAnimal>,
            Option<&RivalMember>,
        )>()
        .iter(world)
        .filter(|(_, t, h, wild, rival)| {
            !h.dead()
                && dist_xz(t.translation, pen_position()) <= GUARD_LEASH
                && (wild.is_some_and(|a| a.hostile && a.chase.is_some())
                    || rival.is_some_and(|r| {
                        run.tribal_campaign
                            .village(r.village)
                            .is_some_and(|v| v.status == RivalStatus::Hostile)
                            && matches!(r.mode, RivalMode::Raid | RivalMode::Defend(_))
                    }))
        })
        .map(|(e, t, ..)| (e, t.translation))
        .collect();
    let pets: Vec<_> = world
        .query::<(Entity, &DomesticAnimal, &Transform, &Health)>()
        .iter(world)
        .map(|(e, pet, t, h)| (e, pet.0, t.translation, h.hp))
        .collect();
    for (entity, slot, position, health) in &pets {
        let Some(record) = run
            .tribal_campaign
            .livestock
            .iter_mut()
            .find(|pet| pet.slot == *slot)
        else {
            let _ = world.despawn(*entity);
            continue;
        };
        if *health <= 0.0 {
            continue;
        }
        let offset = Vec3::new((*slot as f32 - 1.0) * 1.2, 0.0, 1.7);
        let threat = threats
            .iter()
            .filter(|(_, at)| dist_xz(*position, *at) <= GUARD_RADIUS)
            .min_by(|a, b| {
                dist_xz(*position, a.1)
                    .total_cmp(&dist_xz(*position, b.1))
                    .then_with(|| a.0.to_bits().cmp(&b.0.to_bits()))
            });
        world.entity_mut(*entity).insert(PetGuard {
            target: threat.map(|(e, _)| *e),
        });
        let destination = threat.map(|(_, p)| *p).unwrap_or(pen_position() + offset);
        let stopping = if threat.is_some() {
            PET_REACH * 0.85
        } else {
            0.3
        };
        let (next, reached) = step_toward(*position, destination, 3.6, dt);
        if let Some(mut transform) = world.get_mut::<Transform>(*entity) {
            if dist_xz(*position, destination) > stopping {
                face(&mut transform, next - *position);
                transform.translation.x = next.x;
                transform.translation.z = next.z;
            }
            record.position = [transform.translation.x, 0.0, transform.translation.z];
        }
        record.health = health.clamp(0.0, record.max_health);
        if threat.is_none() && (reached < 0.3 || dist_xz(*position, destination) < 0.5) {
            record.egg_seconds += dt;
            if record.egg_seconds + 0.0001 >= EGG_SECONDS {
                record.egg_seconds = 0.0;
                let before = run.tribal_campaign.livestock_eggs;
                run.tribal_campaign.livestock_eggs = (before + EGG_FOOD).min(EGG_CAPACITY);
                if run.tribal_campaign.livestock_eggs > before {
                    run.request_save();
                }
            }
        }
    }
    let missing: Vec<_> = run
        .tribal_campaign
        .livestock
        .iter()
        .filter(|record| {
            !pets
                .iter()
                .any(|(_, slot, _, hp)| *slot == record.slot && *hp > 0.0)
        })
        .map(|record| record.slot)
        .collect();
    if !missing.is_empty() {
        run.tribal_campaign
            .livestock
            .retain(|pet| !missing.contains(&pet.slot));
        for (entity, slot, _, _) in &pets {
            if missing.contains(slot) {
                let _ = world.despawn(*entity);
            }
        }
        run.request_save();
    }
    let eggs = run.tribal_campaign.livestock_eggs;
    if let Some((entity, _)) = pool {
        world.get_mut::<Bush>(entity).unwrap().food_left = eggs;
    } else if !run.tribal_campaign.livestock.is_empty() || eggs > 0.0 {
        world.spawn((
            StageOwned,
            EggPile,
            Bush { food_left: eggs },
            Name::new("livestock eggs: gather and carry to the food mat"),
            Transform::from_translation(pen_position()),
        ));
    }
    world.insert_resource(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_lineage() -> RunProgress {
        use crate::species::{Relationship, RoamerKind, SpeciesWorld};
        let mut run = RunProgress::default();
        run.reset_to(GameStage::Creature);
        run.species_world = SpeciesWorld::generate(75, &Default::default(), 24.0);
        let ids: Vec<_> = run.species_world.nests[0]
            .members
            .iter()
            .take(3)
            .map(|m| m.id)
            .collect();
        for &id in &ids {
            run.species_world.record_befriend(id);
        }
        let rogue = run
            .species_world
            .roamers
            .iter_mut()
            .find(|r| r.kind == RoamerKind::Rogue)
            .unwrap();
        rogue.relationship = Relationship::Allied;
        run.creature_pack.members = vec![ids[0], ids[0], 999_999, rogue.id, ids[1]];
        run
    }

    #[test]
    fn tribal_transition_transfers_living_pack_identity_once_without_food_charge() {
        let mut run = pack_lineage();
        let member = run.creature_pack.members[0];
        let species = run.species_world.species_for_member(member).unwrap().id;
        let rogue = run
            .species_world
            .roamers
            .iter()
            .find(|r| Some(&r.id) == run.creature_pack.members.get(3))
            .unwrap()
            .clone();
        assert_eq!(run.advance(), Some(GameStage::Tribal));
        // The tribe starts with `newTribeFood`; the pets cost nothing on top.
        assert_eq!(run.dna_this_stage, crate::tribal_rules::NEW_TRIBE_FOOD);
        assert!(run.tribal_campaign.pack_inherited);
        let pets = &run.tribal_campaign.livestock;
        assert_eq!(pets.len(), 3);
        assert_eq!(
            (pets[0].pack_member, pets[0].species),
            (Some(member), species)
        );
        assert_eq!(
            (pets[1].pack_member, pets[1].species, pets[1].max_health),
            (Some(rogue.id), rogue.visual, 80.0)
        );
        assert!(pets.iter().all(|p| p.scale == 0.45 && p.egg_seconds == 0.0));
        let saved: RunProgress =
            serde_json::from_slice(&serde_json::to_vec(&run).unwrap()).unwrap();
        let mut app = app(saved);
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<DomesticAnimal>>()
                .iter(world)
                .count(),
            3
        );
        assert_eq!(
            world.resource::<RunProgress>().dna_this_stage,
            crate::tribal_rules::NEW_TRIBE_FOOD
        );
        // A later loss stays lost even if a debug stage selector revisits this boundary.
        let mut run = world.resource::<RunProgress>().clone();
        run.tribal_campaign.livestock.clear();
        run.reset_to(GameStage::Creature);
        run.reset_to(GameStage::Tribal);
        assert!(run.tribal_campaign.livestock.is_empty());
    }

    #[test]
    fn tribal_transition_rejects_dead_unknown_and_nonallied_pack_entries() {
        use crate::species::Relationship;
        let mut run = pack_lineage();
        let ids: Vec<_> = run.species_world.nests[0]
            .members
            .iter()
            .take(3)
            .map(|m| m.id)
            .collect();
        run.species_world.nests[0].members[0].relationship = Relationship::Defeated;
        run.species_world.nests[0].members[1].relationship = Relationship::Neutral;
        run.creature_pack.members = vec![ids[0], ids[1], 999_999];
        run.reset_to(GameStage::Tribal);
        assert!(run.tribal_campaign.livestock.is_empty());
        assert!(run.tribal_campaign.pack_inherited);
    }

    fn isolate_pet(app: &mut App) -> Entity {
        let world = app.world_mut();
        let pet = world
            .query_filtered::<Entity, With<DomesticAnimal>>()
            .iter(world)
            .next()
            .unwrap();
        let at = pen_position() + Vec3::Z * 1.7;
        world.get_mut::<Transform>(pet).unwrap().translation = at;
        for (mut t, mut task) in world
            .query_filtered::<(&mut Transform, &mut UnitTask), With<TribeUnit>>()
            .iter_mut(world)
        {
            t.translation = Vec3::new(28.0, 0.0, -28.0);
            *task = UnitTask::Idle(t.translation);
        }
        // Unrelated roaming populations are outside this bounded combat scenario.
        for (mut t, mut a) in world
            .query::<(&mut Transform, &mut WildAnimal)>()
            .iter_mut(world)
        {
            t.translation = Vec3::new(28.0, 0.0, 28.0);
            a.home = t.translation;
            a.target = t.translation;
            a.wait = 1_000.0;
        }
        pet
    }

    fn predator(app: &mut App, at: Vec3) -> Entity {
        app.world_mut()
            .spawn((
                StageOwned,
                Transform::from_translation(at),
                Health::new(30.0),
                WildAnimal {
                    hostile: true,
                    speed: 4.6,
                    home: at,
                    target: at,
                    wait: 1000.0,
                    flee: 0.0,
                    chase: None,
                },
            ))
            .id()
    }

    #[test]
    fn pet_defends_against_predator_takes_damage_then_returns_and_lays_eggs() {
        let mut app = paid_animal();
        let pet = isolate_pet(&mut app);
        let at = app.world().get::<Transform>(pet).unwrap().translation;
        let egg_seconds = app
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .livestock[0]
            .egg_seconds;
        let foe = predator(&mut app, at + Vec3::X * 1.3);
        step(&mut app, 45);
        let world = app.world_mut();
        assert_eq!(world.get::<PetGuard>(pet).unwrap().target, Some(foe));
        assert!(world.get::<Health>(foe).unwrap().hp < 30.0);
        let (health, max) = {
            let h = world.get::<Health>(pet).unwrap();
            (h.hp, h.max)
        };
        assert!(
            health < max && health > 0.0,
            "predator must retaliate against the pet"
        );
        assert!(
            world.resource::<RunProgress>().tribal_campaign.livestock[0].egg_seconds
                <= egg_seconds + 0.02,
            "combat cannot advance egg production"
        );
        crate::persistence::capture(world);
        let saved: RunProgress =
            serde_json::from_slice(&serde_json::to_vec(world.resource::<RunProgress>()).unwrap())
                .unwrap();
        let mut restored = super::tests::app(saved);
        let world = restored.world_mut();
        let restored_pet = world
            .query_filtered::<Entity, With<DomesticAnimal>>()
            .single(world)
            .unwrap();
        assert!(
            (world.get::<Health>(restored_pet).unwrap().hp - health).abs() < 0.01,
            "checkpoint health must survive a cold reload"
        );
        // The source predator's death is processed by normal combat cleanup.
        app.world_mut().get_mut::<Health>(foe).unwrap().hp = 0.0;
        step(&mut app, 1_050);
        let world = app.world_mut();
        assert!(world.get::<PetGuard>(pet).unwrap().target.is_none());
        assert!(
            world
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock_eggs
                >= EGG_FOOD
        );
        assert_eq!(world.resource::<RunProgress>().dna_this_stage, 120.0 - FIRST_GRAZER_PRICE);
    }

    #[test]
    fn pet_defense_ignores_allies_and_distant_raiders_but_fights_close_raids() {
        let mut app = paid_animal();
        // NPC tribes appear over time (`tribal_rules`): bring V1 into view.
        app.world_mut().resource_mut::<RunProgress>().tribal_campaign.villages[0].appeared = true;
        let pet = isolate_pet(&mut app);
        let at = app.world().get::<Transform>(pet).unwrap().translation;
        let raider = app
            .world_mut()
            .spawn((
                StageOwned,
                Transform::from_translation(at + Vec3::X),
                Health::new(40.0),
                RivalMember {
                    village: 0,
                    home: at,
                    target: at,
                    wait: 1000.0,
                    mode: RivalMode::Raid,
                    stolen: 0.0,
                    speed: 0.0,
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .villages[0]
            .status = RivalStatus::Allied;
        step(&mut app, 30);
        // The pet keeps the hit points of its herd (a medium prey has 25).
        let pet_max = app.world().get::<Health>(pet).unwrap().max;
        assert_eq!(app.world().get::<Health>(raider).unwrap().hp, 40.0);
        assert_eq!(app.world().get::<Health>(pet).unwrap().hp, pet_max);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .tribal_campaign
            .villages[0]
            .status = RivalStatus::Hostile;
        app.world_mut()
            .get_mut::<Transform>(raider)
            .unwrap()
            .translation = Vec3::new(25.0, 0.0, 25.0);
        app.world_mut().entity_mut(raider).insert(tribal::Raider {
            hp: 40.0,
            phase: tribal::RaidPhase::Approach,
            timer: 6.0,
            ..Default::default()
        });
        app.world_mut().get_mut::<RivalMember>(raider).unwrap().home = tribal::village_position(0);
        app.world_mut().get_mut::<RivalMember>(raider).unwrap().mode = RivalMode::Raid;
        step(&mut app, 30);
        assert!(app.world().get::<PetGuard>(pet).unwrap().target.is_none());
        app.world_mut()
            .get_mut::<Transform>(raider)
            .unwrap()
            .translation = at + Vec3::X;
        app.world_mut().get_mut::<RivalMember>(raider).unwrap().mode = RivalMode::Raid;
        step(&mut app, 30);
        assert!(app.world().get::<Health>(raider).unwrap().hp < 40.0);
        assert!(app.world().get::<Health>(pet).unwrap().hp < pet_max);
    }

    #[test]
    fn pet_death_removes_saved_identity_without_erasing_already_laid_eggs() {
        let mut app = paid_animal();
        let pet = isolate_pet(&mut app);
        let world = app.world_mut();
        let pool = world
            .query_filtered::<Entity, With<EggPile>>()
            .single(world)
            .unwrap();
        world.get_mut::<Bush>(pool).unwrap().food_left = 6.0;
        world.get_mut::<Health>(pet).unwrap().hp = 0.0;
        step(&mut app, 2);
        let world = app.world_mut();
        assert!(world.get_entity(pet).is_err());
        assert!(world
            .resource::<RunProgress>()
            .tribal_campaign
            .livestock
            .is_empty());
        assert_eq!(
            world
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock_eggs,
            6.0
        );
        crate::persistence::capture(world);
        let run: RunProgress =
            serde_json::from_slice(&serde_json::to_vec(world.resource::<RunProgress>()).unwrap())
                .unwrap();
        let mut restored = super::tests::app(run);
        let world = restored.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<DomesticAnimal>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(
            world
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock_eggs,
            6.0
        );
    }

    fn step(app: &mut App, frames: usize) {
        for _ in 0..frames {
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        }
    }

    fn key(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        step(app, 1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }

    fn app(run: RunProgress) -> App {
        let mut app = crate::runtime::headless_app(run);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .enter_stage(GameStage::Tribal);
        app.world_mut()
            .resource_mut::<NextState<GameStage>>()
            .set(GameStage::Tribal);
        step(&mut app, 1);
        app
    }

    /// What the first grazer of the test world (a `TRG_NPC_Medium_Prey` herd animal)
    /// costs to feed: its herd's property 0xEB69140E = 10 (the earlier port charged an
    /// ADAPTED 15 for every animal).
    const FIRST_GRAZER_PRICE: f32 = 10.0;

    fn order_animal(food: f32) -> App {
        let mut app = app(RunProgress::default());
        let at = pen_position();
        let world = app.world_mut();
        let mut animals = world.query::<(Entity, &mut Transform, &mut WildAnimal)>();
        let mut prey = None;
        for (entity, mut pose, mut animal) in animals.iter_mut(world) {
            if !animal.hostile && prey.is_none() {
                prey = Some(entity);
                pose.translation = at;
                animal.home = at;
                animal.target = at;
                animal.wait = 1_000.0;
            } else {
                pose.translation = Vec3::new(29.0, 0.0, 29.0);
                animal.home = pose.translation;
                animal.target = pose.translation;
            }
        }
        assert!(prey.is_some());
        // Two real members race for the same animal: payment and identity must
        // still be committed only once.
        let members: Vec<_> = world
            .query_filtered::<Entity, With<TribeMate>>()
            .iter(world)
            .take(2)
            .collect();
        for member in members {
            world.entity_mut(member).insert(Selected);
            world.get_mut::<Transform>(member).unwrap().translation = at;
        }
        world.resource_mut::<RunProgress>().dna_this_stage = food;
        key(&mut app, KeyCode::KeyK);
        app
    }

    fn paid_animal() -> App {
        let mut app = order_animal(120.0);
        step(&mut app, 310);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(
            run.tribal_campaign.livestock.len(),
            1,
            "one animal cannot be tamed twice"
        );
        // The feeding costs what the animal's herd asks (0xEB69140E: 10 for a
        // medium, 15 for a large prey herd); the animal is a friend after one
        // feeding (`NumToImpressForBestFriends` 1), so it is paid once.
        let paid = {
            let world = app.world_mut();
            let animal = world.query_filtered::<Entity, With<DomesticAnimal>>().single(world).unwrap();
            let (price, counts) = feeding_of(world, animal);
            assert_eq!(counts, [1, 1, 1], "a prey herd befriends at the first feeding");
            assert!(world.get::<HerdMember>(animal).is_none() || price >= 10.0, "a herd animal pays its herd's price");
            price
        };
        let run = app.world().resource::<RunProgress>();
        assert_eq!(
            run.dna_this_stage,
            120.0 - paid,
            "only the winner pays"
        );
        assert_eq!(
            run.tribal_campaign.livestock_eggs, 0.0,
            "no instant egg or stockpile reward"
        );
        app
    }

    #[test]
    fn domestication_uses_real_order_pays_once_and_keeps_source_species() {
        let mut app = paid_animal();
        let world = app.world_mut();
        let pet = world
            .query::<(&DomesticAnimal, &WildlifeSpecies, &WorldId)>()
            .single(world)
            .unwrap();
        let record = &world.resource::<RunProgress>().tribal_campaign.livestock[0];
        assert_eq!(record.slot, pet.0 .0);
        assert_eq!(record.species, pet.1 .0);
        assert_eq!(pet.2 .0, ID_BASE + record.slot);
        let entity = world
            .query_filtered::<Entity, With<DomesticAnimal>>()
            .single(world)
            .unwrap();
        assert!(
            world.get::<WildAnimal>(entity).is_none(),
            "livestock must leave wild wandering/combat"
        );
    }

    #[test]
    fn eggs_require_collection_and_conserve_food_through_delivery() {
        let mut app = paid_animal();
        step(&mut app, 950);
        let world = app.world_mut();
        assert!(
            world
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock_eggs
                >= EGG_FOOD
        );
        assert_eq!(
            world.resource::<RunProgress>().dna_this_stage,
            120.0 - FIRST_GRAZER_PRICE,
            "eggs are not passive stockpile income"
        );
        // Isolate collection from unrelated forest gathering and later eggs.
        let sources: Vec<_> = world
            .query_filtered::<Entity, (With<Bush>, Without<EggPile>)>()
            .iter(world)
            .collect();
        for source in sources {
            let _ = world.despawn(source);
        }
        let pool = world
            .query_filtered::<Entity, With<EggPile>>()
            .single(world)
            .unwrap();
        world.get_mut::<Bush>(pool).unwrap().food_left = 10.0;
        {
            let mut run = world.resource_mut::<RunProgress>();
            run.tribal_campaign.livestock_eggs = 10.0;
            run.tribal_campaign.livestock[0].egg_seconds = 0.0;
        }
        key(&mut app, KeyCode::KeyJ);
        step(&mut app, 600);
        let world = app.world_mut();
        let carried: f32 = world
            .query::<&TribeUnit>()
            .iter(world)
            .map(|unit| unit.carrying)
            .sum();
        let run = world.resource::<RunProgress>();
        assert!(
            run.dna_this_stage > 110.0,
            "members must deliver collected eggs"
        );
        assert!(
            (run.dna_this_stage + run.tribal_campaign.livestock_eggs + carried - (130.0 - FIRST_GRAZER_PRICE)).abs()
                < 0.01,
            "collection cannot mint or lose food: stock {}, eggs {}, carried {carried}",
            run.dna_this_stage,
            run.tribal_campaign.livestock_eggs
        );
    }

    #[test]
    fn livestock_progress_and_uncollected_eggs_survive_reload_and_pause() {
        let mut first = paid_animal();
        step(&mut first, 360);
        crate::persistence::capture(first.world_mut());
        let before = first
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .livestock
            .clone();
        let saved: RunProgress = serde_json::from_slice(
            &serde_json::to_vec(first.world().resource::<RunProgress>()).unwrap(),
        )
        .unwrap();
        let mut restored = app(saved);
        let record = &restored
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .livestock[0];
        assert_eq!(record.slot, before[0].slot);
        assert_eq!(record.species, before[0].species);
        assert!(
            record.egg_seconds >= before[0].egg_seconds
                && record.egg_seconds < before[0].egg_seconds + 0.1
        );
        key(&mut restored, KeyCode::Escape);
        let frozen = restored
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .livestock
            .clone();
        step(&mut restored, 180);
        assert_eq!(
            restored
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock,
            frozen
        );
        key(&mut restored, KeyCode::Escape);
        step(&mut restored, 950);
        assert_eq!(
            restored
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock
                .len(),
            1
        );
        assert!(
            restored
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .livestock_eggs
                >= EGG_FOOD
        );
    }

    #[test]
    fn empty_egg_assignment_waits_at_the_pen_instead_of_gathering_fruit() {
        let mut app = paid_animal();
        let world = app.world_mut();
        let pool = world
            .query_filtered::<Entity, With<EggPile>>()
            .single(world)
            .unwrap();
        assert!(world
            .query_filtered::<Entity, (With<Bush>, Without<EggPile>)>()
            .iter(world)
            .next()
            .is_some());
        key(&mut app, KeyCode::KeyJ);
        step(&mut app, 120);
        let world = app.world_mut();
        for task in world
            .query_filtered::<&UnitTask, (With<TribeUnit>, With<Selected>)>()
            .iter(world)
        {
            assert_eq!(
                *task,
                UnitTask::Gather(pool),
                "an empty pen must not redirect its workers into the forest"
            );
        }
        assert_eq!(world.resource::<RunProgress>().dna_this_stage, 120.0 - FIRST_GRAZER_PRICE);
    }

    #[test]
    fn unaffordable_taming_does_not_grant_animal_or_eggs() {
        let mut app = order_animal(TAME_FOOD - 1.0);
        step(&mut app, 320);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.dna_this_stage, TAME_FOOD - 1.0);
        assert!(run.tribal_campaign.livestock.is_empty());
        assert_eq!(run.tribal_campaign.livestock_eggs, 0.0);
    }

    #[test]
    fn original_species_limit_refuses_another_animal_without_a_second_payment() {
        let mut app = paid_animal();
        let species = app
            .world()
            .resource::<RunProgress>()
            .tribal_campaign
            .livestock[0]
            .species;
        let at = pen_position();
        app.world_mut().spawn((
            StageOwned,
            Transform::from_translation(at),
            Health::new(40.0),
            WildlifeSpecies(species),
            WildAnimal {
                hostile: false,
                speed: 3.6,
                home: at,
                target: at,
                wait: 1000.0,
                flee: 0.0,
                chase: None,
            },
        ));
        key(&mut app, KeyCode::KeyK);
        step(&mut app, 320);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.tribal_campaign.livestock.len(), 1);
        assert_eq!(run.dna_this_stage, 120.0 - FIRST_GRAZER_PRICE);
        assert!(run.tribal_campaign.message.contains("already domesticated"));
    }

    #[test]
    fn friendly_stance_toggle_uses_normal_input_and_survives_reload() {
        let mut app = app(RunProgress::default());
        assert!(
            !app.world()
                .resource::<RunProgress>()
                .tribal_campaign
                .friendly_stance
        );
        key(&mut app, KeyCode::KeyC);
        let run = app.world().resource::<RunProgress>();
        assert!(run.tribal_campaign.friendly_stance);
        let saved: RunProgress = serde_json::from_slice(&serde_json::to_vec(run).unwrap()).unwrap();
        let mut restored = super::tests::app(saved);
        assert!(
            restored
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .friendly_stance
        );
        key(&mut restored, KeyCode::KeyC);
        assert!(
            !restored
                .world()
                .resource::<RunProgress>()
                .tribal_campaign
                .friendly_stance
        );
    }

    #[test]
    fn legacy_and_invalid_livestock_data_is_bounded() {
        let legacy: tribal::TribalCampaign = serde_json::from_str("{}").unwrap();
        assert!(legacy.livestock.is_empty());
        assert_eq!(legacy.livestock_eggs, 0.0);
        let old_pet: LivestockRecord = serde_json::from_str(
            r#"{"slot":0,"species":470,"position":[0,0,0],"health":23,"egg_seconds":8}"#,
        )
        .unwrap();
        assert_eq!(
            (old_pet.max_health, old_pet.scale, old_pet.pack_member),
            (40.0, 0.6, None)
        );
        let mut records = vec![LivestockRecord {
            slot: 0,
            species: 470,
            position: [f32::NAN; 3],
            health: f32::INFINITY,
            egg_seconds: -100.0,
            pack_member: None,
            max_health: 40.0,
            scale: 0.6,
        }];
        records.push(records[0].clone());
        let mut eggs = f32::NAN;
        normalize(&mut records, &mut eggs);
        assert_eq!(records.len(), 1);
        assert!(Vec3::from_array(records[0].position).is_finite());
        assert_eq!(
            (records[0].health, records[0].egg_seconds, eggs),
            (40.0, 0.0, 0.0)
        );
    }
}
