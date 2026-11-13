//! Checkpoints capture the playable world after all simulation systems run.
use bevy::app::AppExit;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    io,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{game::*, stages::GameStage};

/// Stable within a stage, independent of Bevy entity allocation.
#[derive(Component, Clone, Copy)]
pub struct WorldId(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerCheckpoint {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub hp: f32,
    pub food: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectCheckpoint {
    #[serde(default)]
    pub rotation: Option<[f32; 4]>,
    #[serde(default)]
    pub wander: Option<[f32; 3]>,
    #[serde(default)]
    pub creature: Option<CreatureCheckpoint>,
    pub id: u32,
    pub position: [f32; 3],
    pub food_left: Option<f32>,
    pub spice_left: Option<f32>,
    pub planet: Option<(bool, bool)>,
    pub hp: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatureCheckpoint {
    pub hp: f32,
    pub hostile: bool,
    pub mind: crate::creature_actions::CreatureMind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    #[serde(default)]
    pub encounter: Option<crate::creature_actions::Encounter>,
    pub stage: usize,
    pub player: PlayerCheckpoint,
    pub objects: Vec<ObjectCheckpoint>,
}

#[derive(Resource)]
pub struct SaveState {
    pub path: PathBuf,
    pub enabled: bool,
    pub status: String,
    timer: Timer,
}

impl Default for SaveState {
    fn default() -> Self {
        Self {
            path: RunProgress::save_path(),
            enabled: true,
            status: "F5 saves | checkpoint every 10s".into(),
            timer: Timer::from_seconds(10.0, TimerMode::Repeating),
        }
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

pub fn load_from(path: &Path) -> io::Result<RunProgress> {
    let mut error = None;
    for candidate in [path.to_path_buf(), backup_path(path)] {
        match std::fs::read(&candidate) {
            Ok(bytes) => match serde_json::from_slice::<RunProgress>(&bytes) {
                Ok(mut run) => {
                    run.migrate_parts();
                    run.resume_legacy_space();
                    return Ok(run);
                }
                Err(problem) => error = Some(io::Error::new(io::ErrorKind::InvalidData, problem)),
            },
            Err(problem) if problem.kind() != io::ErrorKind::NotFound => error = Some(problem),
            Err(_) => {}
        }
    }
    Err(error.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no save or backup found")))
}

/// Write and flush a sibling temporary file, retain the last valid save, then
/// replace the main file atomically. A serialization/write error preserves it.
pub fn save_to(path: &Path, run: &RunProgress) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(run).map_err(io::Error::other)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp_name = path.as_os_str().to_os_string();
    temp_name.push(".tmp");
    let temp = PathBuf::from(temp_name);
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if let Ok(old) = std::fs::read(path) {
        // Never replace a usable backup with a corrupt main file.
        if serde_json::from_slice::<RunProgress>(&old).is_ok() {
            std::fs::write(backup_path(path), old)?;
        }
    }
    std::fs::rename(temp, path)
}

pub fn capture(world: &mut World) {
    let stage = world.resource::<State<GameStage>>().get().clone();
    let run = world.resource::<RunProgress>();
    // Transition systems already selected the new objective. Do not save the
    // outgoing avatar as the next stage's player.
    if stage == GameStage::MainMenu || stage != run.current_stage() {
        return;
    }
    let index = run.stage_order_index;
    // Not while an outfitter's preview creature stands in for the player.
    let mut players = world.query_filtered::<
        (Entity, &Transform, &Health, Option<&Hunger>),
        (With<Player>, Without<crate::outfitter::OutfitterPreview>),
    >();
    let Ok((entity, t, hp, hunger)) = players.single(world) else {
        return;
    };
    let player = PlayerCheckpoint {
        position: crate::terrain::checkpoint_position(world, entity, t.translation),
        rotation: t.rotation.to_array(),
        hp: hp.hp,
        food: hunger.map(|h| h.food),
    };
    let mut query = world.query::<(
        Entity,
        &WorldId,
        &Transform,
        Option<&Bush>,
        Option<&SpiceNode>,
        Option<&Planet>,
        Option<&Health>,
        Option<&crate::game::Npc>,
        Option<&crate::creature_actions::CreatureMind>,
        Option<&crate::creature::Wander>,
    )>();
    let mut objects: Vec<_> = query
        .iter(world)
        .map(
            |(entity, id, t, bush, spice, planet, hp, npc, mind, wander)| ObjectCheckpoint {
                rotation: Some(t.rotation.to_array()),
                wander: wander.map(|w| [w.tx, w.tz, w.wait]),
                creature: npc.zip(mind).map(|(n, m)| CreatureCheckpoint {
                    hp: n.hp,
                    hostile: n.hostile,
                    mind: m.clone(),
                }),
                id: id.0,
                position: crate::terrain::checkpoint_position(world, entity, t.translation),
                food_left: bush.map(|b| b.food_left),
                spice_left: spice.map(|s| s.spice_left),
                planet: planet.map(|p| (p.colonized, p.surveyed)),
                hp: hp.map(|h| h.hp),
            },
        )
        .collect();
    objects.sort_by_key(|o| o.id);
    let encounter = (stage == GameStage::Creature).then(|| {
        world
            .resource::<crate::creature_actions::Encounter>()
            .clone()
    });
    world.resource_mut::<RunProgress>().checkpoint = Some(Checkpoint {
        encounter,
        stage: index,
        player,
        objects,
    });
}

pub fn restore(world: &mut World) {
    let run = world.resource::<RunProgress>();
    let Some(checkpoint) = run
        .checkpoint
        .clone()
        .filter(|c| c.stage == run.stage_order_index)
    else {
        return;
    };
    // The saved species build defines the same health bound used on spawn.
    // A fixed cap would weaken higher-health species every time a run loads.
    let npc_max_hp: std::collections::HashMap<u32, f32> = {
        let originals = world.resource::<crate::Originals>();
        checkpoint
            .objects
            .iter()
            .filter(|o| o.creature.is_some())
            .map(|o| {
                let max = crate::creature::member_max_hp(&run.species_world, o.id, originals)
                    .unwrap_or(50.);
                (o.id, max)
            })
            .collect()
    };
    let mut players =
        world.query_filtered::<(&mut Transform, &mut Health, Option<&mut Hunger>), With<Player>>();
    if let Ok((mut t, mut hp, hunger)) = players.single_mut(world) {
        let position = Vec3::from_array(checkpoint.player.position);
        let rotation = Quat::from_array(checkpoint.player.rotation);
        if position.is_finite() {
            t.translation = position;
        }
        if rotation.is_finite() && rotation.length_squared() > 0.001 {
            t.rotation = rotation.normalize();
        }
        hp.hp = checkpoint.player.hp.clamp(0.0, hp.max);
        if let (Some(mut hunger), Some(food)) = (hunger, checkpoint.player.food) {
            hunger.food = food.clamp(0.0, hunger.max);
        }
    }
    let mut objects = world.query::<(
        Entity,
        &WorldId,
        &mut Transform,
        Option<&mut Bush>,
        Option<&mut SpiceNode>,
        Option<&mut Planet>,
        Option<&mut Health>,
        Option<&mut crate::game::Npc>,
        Option<&mut crate::creature_actions::CreatureMind>,
        Option<&mut Visibility>,
        Option<&mut crate::creature::Wander>,
    )>();
    let mut absent_mates = Vec::new();
    for (entity, id, mut t, bush, spice, planet, hp, npc, mind, visibility, wander) in
        objects.iter_mut(world)
    {
        let Some(saved) = checkpoint.objects.iter().find(|o| o.id == id.0) else {
            if (200..200 + crate::tribal::MAX_MATES as u32).contains(&id.0) {
                absent_mates.push((entity, id.0));
            }
            continue;
        };
        let position = Vec3::from_array(saved.position);
        if position.is_finite() {
            t.translation = position;
        }
        if let Some(rotation) = saved
            .rotation
            .map(Quat::from_array)
            .filter(|r| r.is_finite() && r.length_squared() > 0.001)
        {
            t.rotation = rotation.normalize();
        }
        if let (Some(mut w), Some(v)) = (wander, saved.wander) {
            if v.iter().all(|n| n.is_finite()) {
                w.tx = v[0].clamp(-24., 24.);
                w.tz = v[1].clamp(-24., 24.);
                w.wait = v[2].clamp(0., 60.);
            }
        }
        if let (Some(mut b), Some(food)) = (bush, saved.food_left) {
            b.food_left = food.max(0.0);
        }
        if let (Some(mut s), Some(spice)) = (spice, saved.spice_left) {
            s.spice_left = spice.max(0.0);
        }
        if let (Some(mut p), Some((colonized, surveyed))) = (planet, saved.planet) {
            p.colonized = colonized;
            p.surveyed = surveyed;
        }
        if let (Some(mut hp), Some(value)) = (hp, saved.hp) {
            hp.hp = value.clamp(0.0, hp.max);
        }
        if let (Some(mut npc), Some(mut mind), Some(saved)) = (npc, mind, saved.creature.as_ref()) {
            npc.hp = saved.hp.clamp(0., npc_max_hp[&id.0]);
            npc.hostile = saved.hostile;
            *mind = saved.mind.clone();
            if let Some(mut visibility) = visibility {
                *visibility = if mind.defeated {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
            }
        }
    }
    for (entity, id) in absent_mates {
        world.despawn(entity);
        let mut run = world.resource_mut::<RunProgress>();
        if let Some(index) = run.tribal_campaign.release_slot(id) {
            if index < run.tribe.len() {
                run.tribe.swap_remove(index);
            }
        }
    }
    if let Some(mut encounter) = checkpoint.encounter {
        encounter.cooldowns =
            encounter
                .cooldowns
                .map(|v| if v.is_finite() { v.clamp(0., 60.) } else { 0. });
        if let Some(target) = encounter.target {
            let exists = world
                .query::<(&WorldId, &crate::creature_actions::CreatureMind)>()
                .iter(world)
                .any(|(id, mind)| id.0 == target && !mind.defeated);
            if !exists {
                encounter.target = None;
            }
        }
        world.insert_resource(encounter);
    }
}

pub fn checkpoint_and_save(world: &mut World) {
    let playing = *world.resource::<State<GameStage>>().get() != GameStage::MainMenu;
    let dt = world.resource::<Time>().delta();
    let manual = world
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::F5);
    let exiting = world
        .get_resource::<Messages<AppExit>>()
        .is_some_and(|messages| !messages.is_empty());
    let periodic = {
        let mut saves = world.resource_mut::<SaveState>();
        playing && saves.timer.tick(dt).just_finished()
    };
    if !manual && !periodic && !exiting && !world.resource::<RunProgress>().save_requested {
        return;
    }
    capture(world);
    world.resource_mut::<RunProgress>().save_requested = false;
    if !world.resource::<SaveState>().enabled {
        return;
    }
    let path = world.resource::<SaveState>().path.clone();
    let result = save_to(&path, world.resource::<RunProgress>());
    let mut saves = world.resource_mut::<SaveState>();
    match result {
        Ok(()) => saves.status = "Checkpoint saved | F5 saves again".into(),
        Err(error) => {
            saves.status = format!("SAVE FAILED: {error} | F5 retries");
            error!("{}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_finished_empire_reopens_without_losing_saved_state() {
        let path = std::env::temp_dir().join(format!(
            "openspore-space-migration-{}.json",
            std::process::id()
        ));
        let mut run = RunProgress::default();
        run.stage_order_index = 4;
        run.finished = true;
        run.victories = 2;
        run.spice = 317.0;
        run.space_campaign.colonies = vec![0, 2, 4, 5, 6, 8];
        run.space_campaign.surveyed = vec![0, 1, 2, 4, 5, 6, 8];
        run.space_campaign.terraformed = vec![2, 4, 5, 6, 8];
        run.space_campaign.claimed = crate::space::MISSION_ORDER.len();
        run.space_campaign.mission = run.space_campaign.claimed;
        let original = serde_json::to_vec(&run).unwrap();
        std::fs::write(&path, &original).unwrap();
        let mut resumed = load_from(&path).unwrap();
        assert!(!resumed.finished);
        assert_eq!(resumed.current_stage(), GameStage::Space);
        assert_eq!(resumed.victories, 2);
        assert_eq!(resumed.spice, run.spice);
        assert_eq!(
            serde_json::to_value(&resumed.space_campaign).unwrap(),
            serde_json::to_value(&run.space_campaign).unwrap()
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "loading must not overwrite a save"
        );
        resumed.dna_this_stage = 42.0;
        resumed.checkpoint = Some(Checkpoint {
            encounter: None,
            stage: 4,
            player: PlayerCheckpoint {
                position: [3.0, 8.0, 12.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                hp: 93.0,
                food: None,
            },
            objects: vec![],
        });
        let before = serde_json::to_value(&resumed).unwrap();
        assert_eq!(resumed.advance(), None);
        assert_eq!(
            serde_json::to_value(&resumed).unwrap(),
            before,
            "advancing in Space must preserve the checkpoint and empire"
        );
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn atomic_save_retains_backup_and_reports_errors() {
        let dir = std::env::temp_dir().join(format!("openspore-save-test-{}", std::process::id()));
        let path = dir.join("run.json");
        let mut run = RunProgress::default();
        run.dna_this_stage = 31.0;
        save_to(&path, &run).unwrap();
        run.dna_this_stage = 48.0;
        save_to(&path, &run).unwrap();
        let latest: RunProgress = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let backup: RunProgress =
            serde_json::from_slice(&std::fs::read(backup_path(&path)).unwrap()).unwrap();
        assert_eq!(latest.dna_this_stage, 48.0);
        assert_eq!(backup.dna_this_stage, 31.0);
        std::fs::write(&path, b"interrupted/corrupt save").unwrap();
        assert_eq!(load_from(&path).unwrap().dna_this_stage, 31.0);
        save_to(&path, &run).unwrap();
        assert_eq!(load_from(&backup_path(&path)).unwrap().dna_this_stage, 31.0);
        assert!(save_to(&path.join("cannot-save.json"), &run).is_err());
        for file in [path.clone(), backup_path(&path)] {
            std::fs::remove_file(file).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
