//! Saved games as galaxy stars. The original keeps every game of the profile
//! as a star of the galaxy screen (`GalaxyMaxNumSavedGameWorlds` 200 in the
//! `gge` tuning). OpenSpore saves one JSON file per game: the `--save-path`
//! file is slot 0 and further games live beside it as `<stem>-slot<N>.json`.
//! Each slot's star is a real star of the generated galaxy ([`pick_stars`]).

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::SystemTime,
};

use bevy::prelude::{info, warn, Vec2};

use crate::{game::RunProgress, stages::GameStage};

/// Games the galaxy screen offers: `GalaxyMaxNumSavedGameWorlds` (VERIFIED:
/// the `gge` prop 0x4AAF12F1 = 200, read into the star manager's cap at
/// 0xBB724E and used by the starter picker 0xBB6295).
pub const MAX_SLOTS: usize = 200;
/// `GalaxyNumVisibleStarterWorlds`: free stars offered for a new game.
pub const VISIBLE_STARTER_WORLDS: usize = 6;

/// File of slot `index` for the base save path (slot 0 is the base itself).
pub fn slot_path(base: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return base.to_path_buf();
    }
    let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
    let ext = base.extension().and_then(|s| s.to_str()).unwrap_or("json");
    base.with_file_name(format!("{stem}-slot{index}.{ext}"))
}

/// What the star rollover shows about a saved game.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotSummary {
    pub stage: GameStage,
    pub finished: bool,
    pub planet: String,
    pub creature: String,
    pub difficulty: u8,
    pub modified: Option<SystemTime>,
    /// The homeworld seed (`creature::world_seed`): the planet impostor.
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlotState {
    Empty,
    Saved(SlotSummary),
    /// The file exists but neither it nor its backup loads ("File Missing or
    /// Corrupt" in the rollover).
    Corrupt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlotInfo {
    pub index: usize,
    pub path: PathBuf,
    pub state: SlotState,
}

impl SlotInfo {
    pub fn saved(&self) -> Option<&SlotSummary> {
        match &self.state {
            SlotState::Saved(s) => Some(s),
            _ => None,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.state == SlotState::Empty
    }
}

/// The name shown for a game's creature: the creator name, else the cell's.
pub fn creature_name(run: &RunProgress) -> String {
    [&run.creature_creation.name, &run.cell_creation.name]
        .into_iter()
        .find(|n| !n.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

pub fn summarize(run: &RunProgress, modified: Option<SystemTime>) -> SlotSummary {
    SlotSummary {
        stage: run.current_stage(),
        finished: run.finished,
        planet: run.game_setup.planet_name.clone(),
        creature: creature_name(run),
        difficulty: run.game_setup.difficulty,
        modified,
        seed: crate::creature::world_seed(run),
    }
}

/// Read one slot (its backup when the main file is damaged).
pub fn read_slot(base: &Path, index: usize) -> SlotInfo {
    let path = slot_path(base, index);
    let exists = path.exists() || crate::persistence::backup_path(&path).exists();
    let state = if !exists {
        SlotState::Empty
    } else {
        match crate::persistence::load_from(&path) {
            Ok(run) => {
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                SlotState::Saved(summarize(&run, modified))
            }
            Err(_) => SlotState::Corrupt,
        }
    };
    SlotInfo { index, path, state }
}

pub fn scan(base: &Path) -> Vec<SlotInfo> {
    (0..MAX_SLOTS).map(|i| read_slot(base, i)).collect()
}

/// Delete a game: its file and backup (the original deletes the saved
/// planet after the confirmation).
pub fn delete(path: &Path) -> io::Result<()> {
    for file in [path.to_path_buf(), crate::persistence::backup_path(path)] {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The stages the saved games have reached: Cell always, every later stage once
/// some saved game has got there. Retail's unlocks are achievements
/// (VERIFIED, `front-end-notes.md` 7b: ALG-UnlockCreature .. ALG-UnlockSpace
/// earned when a stage ends); OpenSpore keeps them in `profile.rs`, and this
/// only seeds a profile file that does not exist yet from the saves beside it
/// (ADAPTED: saves that predate the profile file).
pub fn unlocked_stages(slots: &[SlotInfo]) -> [bool; 5] {
    let mut reached = 0usize;
    for s in slots.iter().filter_map(SlotInfo::saved) {
        let index = RunProgress::stage_order().iter().position(|g| *g == s.stage).unwrap_or(0);
        reached = reached.max(index);
    }
    std::array::from_fn(|i| i <= reached)
}

/// Free slots offered as starter stars, first `VISIBLE_STARTER_WORLDS`.
pub fn starter_slots(slots: &[SlotInfo]) -> Vec<usize> {
    slots.iter().filter(|s| s.is_empty()).map(|s| s.index).take(VISIBLE_STARTER_WORLDS).collect()
}

/// A slot's star: the star of the generated galaxy and where it lies (galaxy
/// units, centred on the core; the galaxy radius is 700).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarPick {
    /// Index of the star in the galaxy layout (`u32::MAX`: not a real star).
    pub star: u32,
    pub position: [f32; 2],
}

/// The galaxy seed of `space_galaxy` (the stars the Space stage plays on).
const GALAXY_SEED: u64 = 0x5EED_6A1A;
/// Seed of the slot -> star choice. ADAPTED: retail picks the starter stars
/// with `Math::sMainRNG` and keeps the choice in the saved game; OpenSpore
/// saves have no star field, so the n-th pick of a fixed sequence is slot n's.
const PICK_SEED: u64 = 0x6C0F_57A2;
/// `GalaxyStarterWorldMinDistanceFromCore` (gge 0xD2125FBA = 250, read at
/// 0xBB728F into 0x156C50C and squared at 0xBB3E24).
const MIN_DISTANCE_FROM_CORE: f32 = crate::space_worldgen::STARTER_MIN_DISTANCE;
/// The random point is drawn in the 1400 x 1400 grid square and rejected
/// beyond radius 700 (0xBB3E6A, 0xBB3EF2: 490000 = 700 squared). The loaded
/// `GalaxyStarterWorldMaxDistanceFromCore` (450) is stored but this picker
/// never reads it.
const GRID_HALF: f32 = crate::space_worldgen::GALAXY_EXTENT;
/// A new point must lie at least 10 units from every other pick (0xBB3F66:
/// 100 = 10 squared; the loaded `GalaxyStarterWorldMinDistanceFromOtherStarter`
/// 50 is not read by the picker either).
const PICK_SEPARATION: f32 = 10.0;
/// Stars within 10 units of the point are the candidates (0xBB4046).
const PICK_CANDIDATE_RADIUS: f32 = 10.0;
/// A candidate needs 20 or more stars within 20 units (0xBB4151 radius,
/// 0xBB41F0 `cmp ecx, 0x14`).
const PICK_NEIGHBOUR_RADIUS: f32 = 20.0;
const PICK_NEIGHBOURS: usize = 20;
/// Attempts per pick (0xBB4013: 0x3E8).
const PICK_ATTEMPTS: u32 = 1000;

/// The retail starter-star choice (`cStarManager` 0xBB3E10), repeated for
/// `count` slots on the stars of `stars`: a random point of the galaxy square
/// at least 250 from the core and 10 from the earlier picks, then a star near
/// it whose surroundings hold at least 20 stars.
/// VERIFIED (`fe-final`, 0xBB4040 .. 0xBB41F0): the candidates come from a
/// `StarRequestFilter` (ModAPI `StarManager.h`) whose `starTypes` word is 0x10
/// (bit 4 = `StarType::StarG`, the yellow single stars; 0xBB4061) and `techLevels`
/// 0x3F (all), the neighbourhood count uses the filter's defaults (`starTypes`
/// 0x1FFF = every type, radius 20, 0xBB416C), and a candidate whose record byte
/// +0xAC is set is skipped. ADAPTED: a slot whose 1000 attempts all fail falls
/// back to [`fallback_position`] without a star.
pub fn pick_stars(stars: &[crate::space_worldgen::GalaxyStar], count: usize) -> Vec<StarPick> {
    use rand::{Rng, SeedableRng};
    let position = |i: u32| stars[i as usize].position;
    let grid = crate::space_worldgen::SectorGrid::build(stars.len(), position);
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(PICK_SEED);
    let mut picks: Vec<StarPick> = Vec::with_capacity(count);
    for slot in 0..count {
        let mut found = None;
        for _ in 0..PICK_ATTEMPTS {
            let point = Vec2::new(rng.random_range(-GRID_HALF..GRID_HALF), rng.random_range(-GRID_HALF..GRID_HALF));
            let r2 = point.length_squared();
            if r2 < MIN_DISTANCE_FROM_CORE * MIN_DISTANCE_FROM_CORE || r2 > GRID_HALF * GRID_HALF {
                continue;
            }
            if picks.iter().any(|p| Vec2::from(p.position).distance(point) < PICK_SEPARATION) {
                continue;
            }
            let mut near: Vec<(u32, f32)> = Vec::new();
            grid.for_each_within(&position, point, PICK_CANDIDATE_RADIUS, |i, d| near.push((i, d)));
            near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let star = near.into_iter().map(|(i, _)| i).find(|&i| {
                if stars[i as usize].kind != crate::space_worldgen::StarKind::Yellow || picks.iter().any(|p| p.star == i) {
                    return false;
                }
                let mut neighbours = 0usize;
                grid.for_each_within(&position, position(i), PICK_NEIGHBOUR_RADIUS, |_, _| neighbours += 1);
                neighbours >= PICK_NEIGHBOURS
            });
            if let Some(i) = star {
                found = Some(StarPick { star: i, position: position(i).into() });
                break;
            }
        }
        picks.push(found.unwrap_or(StarPick { star: u32::MAX, position: fallback_position(slot) }));
    }
    picks
}

/// The picks of the installed galaxy, made once by [`request_real_stars`].
static REAL_STARS: OnceLock<Vec<StarPick>> = OnceLock::new();
static REAL_STARS_STARTED: AtomicBool = AtomicBool::new(false);

/// Generate the galaxy (the installed emit maps, as the Space stage does) on a
/// worker thread and pick the slots' stars. Idempotent.
pub fn request_real_stars(index: &crate::asset_catalog::ResourceIndex) {
    if index.records.is_empty() || REAL_STARS_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let index = index.clone();
    let spawned = std::thread::Builder::new().name("gge-stars".into()).stack_size(32 << 20).spawn(move || {
        let start = std::time::Instant::now();
        let maps = crate::space_worldgen::StarMaps::from_install(|group_id, instance_id, type_id| {
            index.read(crate::dbpf::ResourceKey { group_id, instance_id, type_id }).ok()
        })
        .unwrap_or_else(crate::space_worldgen::StarMaps::analytic);
        let layout = crate::space_worldgen::generate_layout(&maps, GALAXY_SEED, crate::space_galaxy::SOL_STAR);
        let picks = pick_stars(&layout.stars, MAX_SLOTS);
        let real = picks.iter().filter(|p| p.star != u32::MAX).count();
        info!(
            "front end: {} galaxy stars, {real} of {MAX_SLOTS} slot stars picked in {} ms",
            layout.stars.len(),
            start.elapsed().as_millis()
        );
        let _ = REAL_STARS.set(picks);
    });
    if let Err(error) = spawned {
        warn!("front end: galaxy star worker: {error}");
    }
}

/// The real star picks are still being made (the galaxy screen waits for them
/// instead of showing stars that would move).
pub fn stars_pending(index: &crate::asset_catalog::ResourceIndex) -> bool {
    !index.records.is_empty() && REAL_STARS.get().is_none()
}

/// Where a slot's star lies in the galaxy (galaxy units): the picked real
/// star, else the fallback ring.
pub fn star_position(index: usize) -> [f32; 2] {
    REAL_STARS.get().and_then(|p| p.get(index)).map_or_else(|| fallback_position(index), |p| p.position)
}

/// The star's galaxy index of a slot, when the picks are known and real.
#[allow(dead_code)]
pub fn star_index(index: usize) -> Option<u32> {
    REAL_STARS.get().and_then(|p| p.get(index)).map(|p| p.star).filter(|s| *s != u32::MAX)
}

/// Positions used until / unless real stars are picked (headless runs without
/// the install): between `GalaxyStarterWorldMin/MaxDistanceFromCore`
/// (250..450) from the core, spread by the golden angle.
/// ADAPTED: a stand-in for the real stars.
pub fn fallback_position(index: usize) -> [f32; 2] {
    const GOLDEN: f32 = 2.399_963;
    let angle = 0.6 + index as f32 * GOLDEN;
    let radius = 250.0 + ((index * 37) % 11) as f32 / 10.0 * 200.0;
    [radius * angle.cos(), radius * angle.sin()]
}

/// "M/D/YY" like the layout's sample date (12/19/07), in UTC.
pub fn format_date(time: SystemTime) -> String {
    let secs = time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86_400) as i64;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{m}/{d}/{:02}", y.rem_euclid(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspore-fe-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn slot_files_sit_beside_the_base_save() {
        let base = Path::new("C:/x/qc_save.json");
        assert_eq!(slot_path(base, 0), base);
        assert_eq!(slot_path(base, 3), Path::new("C:/x/qc_save-slot3.json"));
    }

    #[test]
    fn scan_reads_games_unlocks_and_deletes() {
        let dir = temp_dir("slots");
        let base = dir.join("save.json");
        let mut run = RunProgress::default();
        run.game_setup.planet_name = "Gorgon".into();
        run.creature_creation.name = "Blib".into();
        run.reset_to(GameStage::Tribal);
        crate::persistence::save_to(&slot_path(&base, 2), &run).unwrap();
        std::fs::write(slot_path(&base, 4), b"not json").unwrap();
        let slots = scan(&base);
        assert_eq!(slots.len(), MAX_SLOTS);
        assert!(slots[0].is_empty());
        let s = slots[2].saved().expect("slot 2 saved");
        assert_eq!((s.stage.clone(), s.planet.as_str(), s.creature.as_str()), (GameStage::Tribal, "Gorgon", "Blib"));
        assert_eq!(slots[4].state, SlotState::Corrupt);
        assert_eq!(unlocked_stages(&slots), [true, true, true, false, false]);
        assert_eq!(starter_slots(&slots), vec![0, 1, 3, 5, 6, 7]);
        delete(&slots[2].path).unwrap();
        assert!(read_slot(&base, 2).is_empty());
        assert_eq!(unlocked_stages(&scan(&base))[1], false);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fallback_stars_keep_their_distance_from_the_core_and_each_other() {
        let stars: Vec<_> = (0..12).map(fallback_position).collect();
        for (i, a) in stars.iter().enumerate() {
            let r = (a[0] * a[0] + a[1] * a[1]).sqrt();
            assert!((249.0..=451.0).contains(&r), "slot {i} radius {r}");
            for b in &stars[i + 1..] {
                let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
                assert!(d >= 50.0, "slots too close: {d}");
            }
        }
    }

    #[test]
    fn slot_stars_are_real_yellow_stars_with_crowded_surroundings() {
        use crate::space_worldgen::{generate_layout, StarKind, StarMaps};
        let layout = generate_layout(&StarMaps::analytic(), GALAXY_SEED, crate::space_galaxy::SOL_STAR);
        let picks = pick_stars(&layout.stars, 24);
        assert_eq!(picks.len(), 24);
        let real: Vec<_> = picks.iter().filter(|p| p.star != u32::MAX).collect();
        assert!(real.len() >= 20, "only {} of 24 slots found a star", real.len());
        for (i, a) in real.iter().enumerate() {
            let star = &layout.stars[a.star as usize];
            assert_eq!(star.kind, StarKind::Yellow);
            assert_eq!(Vec2::from(a.position), star.position);
            assert!(star.position.length() > 240.0, "star {} too near the core", a.star);
            let crowd = layout.stars.iter().filter(|s| s.position.distance(star.position) <= PICK_NEIGHBOUR_RADIUS).count();
            assert!(crowd >= PICK_NEIGHBOURS, "star {} has {crowd} neighbours", a.star);
            for b in &real[i + 1..] {
                assert_ne!(a.star, b.star);
            }
        }
        // The choice is deterministic.
        assert_eq!(picks, pick_stars(&layout.stars, 24));
    }

    /// Capture fixtures: `FE_FIXTURES=<dir>` writes three saved games there
    /// (slot 0 Cell, slot 2 Creature, slot 5 Tribal) for `--save-path
    /// <dir>\qc_save.json` captures of the galaxy screen.
    #[test]
    #[ignore = "writes capture fixtures to FE_FIXTURES"]
    fn write_capture_fixtures() {
        let Some(dir) = std::env::var_os("FE_FIXTURES") else { return };
        let base = PathBuf::from(dir).join("qc_save.json");
        for (slot, stage, planet, creature) in [
            (0, GameStage::Cell, "Ixtapa", "Blibber"),
            (2, GameStage::Creature, "Gorgonia", "Snarfle"),
            (5, GameStage::Tribal, "Umbrax", "Tuskhorn"),
        ] {
            let mut run = RunProgress::default();
            run.reset_to(stage);
            run.game_setup.planet_name = planet.into();
            run.creature_creation.name = creature.into();
            crate::persistence::save_to(&slot_path(&base, slot), &run).unwrap();
        }
    }

    #[test]
    fn dates_read_like_the_layout_sample() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_198_022_400); // 2007-12-19
        assert_eq!(format_date(t), "12/19/07");
    }
}
