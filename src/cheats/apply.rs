//! What the cheats do in each stage. Every handler reads the cheat requests,
//! acts on the cheats that apply to the screen it owns and records an outcome
//! in the log; `report` (cheats.rs) says "isn't available here" for a cheat
//! nobody took. The switches (god mode, infinite energy, free camera, hidden
//! HUD) are applied every frame here too.
//!
//! The changes go through the same state the game's own code changes (the
//! run's `RunProgress`, the stage campaigns, the stage components), using the
//! game's helpers where one exists (`award_dna`, `cell_rules::add_food`,
//! `cell_rules::unlock`, `CivCampaign::take_city`, `TribalCampaign::recruit`,
//! `SpaceCampaign::grant_tool` ...), so the HUDs, saves and stage flows pick
//! them up as if earned.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::{Cheat, CheatLog, CheatRequest, CheatState, CheatSystems, StageName};
use crate::game::{Health, Hunger, Player, RunProgress};
use crate::stages::GameStage;

/// DNA points `addDNA` adds, only while an editor is open (0x576BB0:
/// `mpEditorLimits->AddValue(budget, 150)`).
pub const ADD_DNA: f32 = 150.0;
/// Money `moreMoney` adds in Civilization (0xB60CF0: `AddMoney(2000)`).
pub const MORE_MONEY_CIV: f32 = 2_000.0;
/// ... and in Space (`spaceEconomyCheatMoneyAmount`).
pub const MORE_MONEY_SPACE: f32 = 1_000_000.0;
/// Space money clamp (empire money 0..99,999,999).
const SPACE_MONEY_CAP: f32 = 99_999_999.0;
/// Ammo `spaceCreate` adds to each owned tool that uses ammo (0xB5FF40).
const SPACE_CREATE_AMMO: u32 = 5;
/// ADAPTED: the per-tool ammo maximum `spaceCreate` stops at (retail reads
/// each tool's own maximum, which OpenSpore does not model yet).
const SPACE_CREATE_AMMO_MAX: u32 = 99;

/// `setConsequenceTrait` names (the table at 0x156ACD0) and menu labels.
pub const CONSEQUENCE_TRAITS: &[(&str, &str)] = &[
    ("cell_carnivore", "Cell carnivore"),
    ("cell_herbivore", "Cell herbivore"),
    ("cell_omnivore", "Cell omnivore"),
    ("creature_aggressive", "Creature aggressive"),
    ("creature_social", "Creature social"),
    ("creature_mixed", "Creature mixed"),
    ("tribe_aggressive", "Tribe aggressive"),
    ("tribe_social", "Tribe social"),
    ("tribe_mixed", "Tribe mixed"),
    ("civ_military", "Civ military"),
    ("civ_economic", "Civ economic"),
    ("civ_religious", "Civ religious"),
    ("space_bard", "Space bard"),
    ("space_ecologist", "Space ecologist"),
    ("space_zealot", "Space zealot"),
    ("space_diplomat", "Space diplomat"),
    ("space_scientist", "Space scientist"),
    ("space_trader", "Space trader"),
    ("space_shaman", "Space shaman"),
    ("space_warrior", "Space warrior"),
    ("space_wanderer", "Space wanderer"),
    ("space_knight", "Space knight"),
];

/// The stage prefix and trait of a `setConsequenceTrait` name; a bare name
/// (`military`, `bard`) gets the prefix of the stage it belongs to.
fn split_trait(name: &str) -> Option<(&'static str, &'static str)> {
    let name = name.to_ascii_lowercase();
    CONSEQUENCE_TRAITS.iter().find_map(|(full, _)| {
        let (stage, t) = full.split_once('_')?;
        (name == *full || name == t).then_some((stage, t))
    })
}

/// `setTime -speed`: the multiplier of the Creature and Tribal / Civ day
/// clocks (`creature_ecology::advance_day_clock`, `planet_sky`).
#[derive(Resource, Debug, Clone, Copy)]
pub struct DaySpeed(pub f32);

impl Default for DaySpeed {
    fn default() -> Self {
        DaySpeed(1.0)
    }
}

/// The free camera of `freeCam`.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct FreeCam {
    pub on: bool,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}

pub struct ApplyPlugin;

impl Plugin for ApplyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DaySpeed>()
            .init_resource::<FreeCam>()
            .add_systems(
                PreUpdate,
                (free_cam_input, teleport_click).in_set(CheatSystems::Input).after(super::console::input),
            )
            .add_systems(
                Update,
                (
                    global_cheats,
                    cell_cheats,
                    creature_cheats,
                    tribal_cheats,
                    civ_cheats,
                    space_cheats,
                    editor_cheats,
                )
                    .in_set(CheatSystems::Apply)
                    .after(super::core_cheats),
            )
            .add_systems(
                Update,
                (
                    keep_switches.before(crate::GameSystems::Simulation),
                    keep_switches_after.after(crate::GameSystems::Simulation),
                ),
            )
            .add_systems(
                PostUpdate,
                (hide_hud, readout, editor_freedom),
            )
            // Last, after every stage camera placed itself (cell_engine's
            // camera and others run in PostUpdate).
            .add_systems(Last, free_cam_apply);
    }
}

fn stage_of(state: &Option<Res<State<GameStage>>>) -> Option<GameStage> {
    state.as_ref().map(|s| s.get().clone())
}

/// The point on the horizontal plane at `height` under the cursor.
fn cursor_point(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    height: f32,
) -> Option<Vec3> {
    let cursor = windows.iter().next()?.cursor_position()?;
    let (camera, transform) = cameras.iter().next()?;
    let ray = camera.viewport_to_world(transform, cursor).ok()?;
    let d = ray.intersect_plane(Vec3::new(0.0, height, 0.0), InfinitePlane3d::new(Vec3::Y))?;
    Some(ray.get_point(d))
}

fn fmt_money(v: f32) -> String {
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Cheats that need no particular stage
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn global_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut log: ResMut<CheatLog>,
    mut state: ResMut<CheatState>,
    stage: Option<Res<State<GameStage>>>,
    mut next: Option<ResMut<NextState<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    mut speed: ResMut<DaySpeed>,
    mut free: ResMut<FreeCam>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut commands: Commands,
    mut clock: Option<ResMut<crate::planet_sky::StageClock>>,
) {
    let stage = stage_of(&stage);
    for r in requests.read() {
        match &r.cheat {
            Cheat::GotoStage(target) => {
                let (Some(run), Some(next)) = (run.as_deref_mut(), next.as_deref_mut()) else {
                    continue;
                };
                let to = match target {
                    StageName::Cell => GameStage::Cell,
                    StageName::Creature => GameStage::Creature,
                    StageName::Tribal => GameStage::Tribal,
                    StageName::Civilization => GameStage::Civilization,
                    StageName::Space => GameStage::Space,
                };
                // As the Ctrl+Alt+digit debug keys (`main::menu_keys`).
                if stage.as_ref() != Some(&to) {
                    run.reset_to(to.clone());
                }
                next.set(to);
                state.menu_open = false;
                log.done(r.id, format!("Starting the {} stage", target.label()));
            }
            Cheat::SetTimeSpeed(s) => {
                speed.0 = *s;
                log.done(r.id, format!("Day clock speed x{s}"));
            }
            Cheat::SetTime { hour, minute } => {
                let h = *hour as f32 + *minute as f32 / 60.0;
                match stage.clone() {
                    Some(GameStage::Creature) => {
                        if let Some(run) = run.as_deref_mut() {
                            run.species_world.time_of_day = h;
                            log.done(r.id, format!("Time of day {hour}:{minute:02}"));
                        }
                    }
                    Some(s @ (GameStage::Tribal | GameStage::Civilization)) => {
                        match clock.as_deref_mut() {
                            Some(c) if c.stage == s => c.hour = h,
                            _ => commands.insert_resource(crate::planet_sky::StageClock { stage: s, hour: h }),
                        }
                        log.done(r.id, format!("Time of day {hour}:{minute:02}"));
                    }
                    _ => log.refuse(r.id, "This stage has no time of day."),
                }
            }
            Cheat::FreeCam => {
                free.on = !free.on;
                if free.on {
                    if let Some(t) = cameras.iter().next() {
                        let (yaw, pitch, _) = t.compute_transform().rotation.to_euler(EulerRot::YXZ);
                        free.position = t.translation();
                        free.yaw = yaw;
                        free.pitch = pitch;
                    }
                    if free.speed <= 0.0 {
                        free.speed = 20.0;
                    }
                    // The original's Tribal / Civ text.
                    log.done(r.id, "Free Camera ON");
                    log.done(r.id, "WASD / arrows move, Q / E go down / up, Shift is faster, right mouse turns, the wheel sets the speed.");
                } else {
                    log.done(r.id, "Free Camera OFF");
                }
            }
            Cheat::Screenshot => {
                // Next to the settings (OPENSPORE_USER_DIR when it is set).
                let dir = crate::user_paths::UserPaths::from_process().config_dir().join("Screenshots");
                let _ = std::fs::create_dir_all(&dir);
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let path = dir.join(format!("openspore-{stamp}.png"));
                commands
                    .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                    .observe(bevy::render::view::screenshot::save_to_disk(path.clone()));
                log.done(r.id, format!("Screenshot saved to {}", path.display()));
            }
            Cheat::EvoAdvantage => {
                // Retail flags the Creature-start chooser so it offers the
                // "evolutionarily advantaged" Sporepedia creatures.
                log.refuse(
                    r.id,
                    "evoAdvantage changes the Sporepedia creature choice at a Creature stage start, which OpenSpore does not have yet.",
                );
            }
            Cheat::ToggleEditorBackground => {
                state.editor_background_off = !state.editor_background_off;
                log.done(
                    r.id,
                    if state.editor_background_off { "Editor background off" } else { "Editor background on" },
                );
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Cell
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn cell_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut log: ResMut<CheatLog>,
    stage: Option<Res<State<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    mut flow: Option<ResMut<crate::cell_flow::CellFlow>>,
    editor: Option<Res<crate::cell_editor::CellEditor>>,
    mut player: Query<(&mut crate::cell_engine::CellObject, &mut Transform), With<Player>>,
    mut others: Query<
        (&Transform, &mut crate::cell_engine::CellObject, &mut crate::cell_rules::CellCombat),
        Without<Player>,
    >,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
) {
    if stage_of(&stage) != Some(GameStage::Cell) {
        requests.clear();
        return;
    }
    let Some(run) = run.as_deref_mut() else {
        requests.clear();
        return;
    };
    let in_editor = editor.as_ref().is_some_and(|e| e.active);
    use crate::cell_rules::{self as rules, MAX_HP};
    for r in requests.read() {
        let dna = |run: &mut RunProgress, flow: &mut Option<ResMut<crate::cell_flow::CellFlow>>, n: i64| {
            let n = n.clamp(-100_000, 100_000) as i32;
            // The HUD shows earned - spent; the creator books what it spent
            // when it closes (cell_flow), so an editor top-up goes to both
            // budgets.
            run.cell.evolution_points_spent -= n;
            if in_editor {
                run.editor_dna = (run.editor_dna + n as f32).max(0.0);
                if let Some(flow) = flow.as_deref_mut() {
                    flow.editor_dna = (flow.editor_dna + n as f32).max(0.0);
                }
            }
            run.request_save();
        };
        match &r.cheat {
            Cheat::AddDna => {
                if in_editor {
                    dna(run, &mut flow, ADD_DNA as i64);
                    log.done(r.id, format!("+{ADD_DNA:.0} DNA"));
                } else {
                    log.refuse(r.id, "addDNA works in the cell creator (use dna <amount> here).");
                }
            }
            Cheat::Dna(n) => {
                dna(run, &mut flow, *n);
                log.done(r.id, format!("{n:+} DNA"));
            }
            Cheat::RefillMotives | Cheat::HealAll => {
                for (mut o, _) in &mut player {
                    o.hp = MAX_HP;
                }
                log.done(r.id, "Health refilled");
            }
            Cheat::Progress(p) => {
                let amount = (crate::cell_engine::STAGE_FOOD as f32 * p / 100.0).round() as i32;
                if amount > 0 {
                    rules::add_food(run, amount, run.diet_herbivore >= run.diet_carnivore);
                } else {
                    run.cell.food_progression = (run.cell.food_progression + amount).max(0);
                }
                run.request_save();
                log.done(r.id, format!("Evolution bar {}/{}", run.cell.food_progression, crate::cell_engine::STAGE_FOOD));
            }
            Cheat::CompleteStage => {
                let left = crate::cell_engine::STAGE_FOOD - run.cell.food_progression;
                if left > 0 {
                    rules::add_food(run, left, run.diet_herbivore >= run.diet_carnivore);
                    run.request_save();
                }
                log.done(r.id, "The evolution bar is full: the Cell stage ends.");
            }
            Cheat::UnlockParts => {
                let mut n = 0;
                for raw in 2..=10 {
                    if let Ok(kind) = crate::cell_data::UnlockType::from_raw(raw) {
                        if rules::unlock(run, kind) {
                            n += 1;
                        }
                    }
                }
                log.done(r.id, format!("Unlocked {n} cell part types"));
            }
            Cheat::Smite(_) => {
                let Some((me, at)) = player.iter().next().map(|(o, t)| (o.radius, Vec2::new(t.translation.x, t.translation.z))) else {
                    continue;
                };
                let reach = me * 24.0;
                let mut n = 0;
                for (t, mut o, mut c) in &mut others {
                    if o.kind != crate::cell_engine::CellKind::Creature || o.dead || o.hp <= 0 || o.passive {
                        continue;
                    }
                    if Vec2::new(t.translation.x, t.translation.z).distance(at) > reach {
                        continue;
                    }
                    o.hp = 0;
                    o.dead = true;
                    c.death = rules::DeathKind::Killed;
                    c.killed_by_player = true;
                    run.cell.count_kill();
                    n += 1;
                }
                log.done(r.id, format!("Smote {n} cells"));
            }
            Cheat::Teleport => {
                let Some((_, mut t)) = player.iter_mut().next() else { continue };
                match cursor_point(&windows, &cameras, t.translation.y) {
                    Some(p) => {
                        t.translation.x = p.x;
                        t.translation.z = p.z;
                        log.done(r.id, "Teleported");
                    }
                    None => log.refuse(r.id, "Point at the water to teleport."),
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Creature
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn creature_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut log: ResMut<CheatLog>,
    stage: Option<Res<State<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    (mut editor, editor_data): (
        Option<ResMut<crate::creature_editor::CreatureEditor>>,
        Option<Res<crate::creature_editor_data::CreatureEditorData>>,
    ),
    (mut encounter, mut mobility): (
        Option<ResMut<crate::creature_actions::Encounter>>,
        Option<ResMut<crate::creature::mobility::CreatureMobility>>,
    ),
    mut player: Query<(&mut Transform, &mut Health, Option<&mut Hunger>), With<Player>>,
    mut controls: Option<ResMut<crate::creature_controls::CreatureControls>>,
    mut minds: Query<
        (&Transform, &mut crate::creature_actions::CreatureMind, Option<&mut Health>, Option<&mut Hunger>),
        Without<Player>,
    >,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
) {
    if stage_of(&stage) != Some(GameStage::Creature) {
        requests.clear();
        return;
    }
    let Some(run) = run.as_deref_mut() else {
        requests.clear();
        return;
    };
    let in_editor = editor.as_ref().is_some_and(|e| e.active);
    for r in requests.read() {
        // Spendable DNA (capped at 9999 like the game's own counter).
        let mut spendable = |run: &mut RunProgress, n: f32| {
            if n >= 0.0 {
                run.editor_dna = crate::creature_rules::add_evolution_points(run.editor_dna, 0.0, n).0;
            } else {
                run.editor_dna = (run.editor_dna + n).max(0.0);
            }
            // Cancelling the editor restores its entry budget.
            if let Some(entry) = editor.as_deref_mut().and_then(|e| e.entry.as_mut()) {
                entry.dna = (entry.dna + n).max(0.0);
            }
            run.request_save();
        };
        match &r.cheat {
            Cheat::AddDna => {
                if in_editor {
                    spendable(run, ADD_DNA);
                    log.done(r.id, format!("+{ADD_DNA:.0} DNA"));
                } else {
                    log.refuse(r.id, "addDNA works in the creature editor (use dna <amount> here).");
                }
            }
            Cheat::Dna(n) => {
                spendable(run, *n as f32);
                log.done(r.id, format!("{n:+} DNA ({:.0} to spend)", run.editor_dna));
            }
            Cheat::RefillMotives => {
                for (_, mut h, hunger) in &mut player {
                    h.hp = h.max;
                    if let Some(mut hunger) = hunger {
                        hunger.food = hunger.max;
                    }
                }
                if let Some(e) = encounter.as_deref_mut() {
                    e.poison_seconds = 0.0;
                    e.stun = 0.0;
                }
                if let Some(m) = mobility.as_deref_mut() {
                    refill_mobility(m);
                }
                // The posse too (0xBFD2E0).
                for (_, mind, health, hunger) in &mut minds {
                    if !mind.allied {
                        continue;
                    }
                    if let Some(mut h) = health {
                        h.hp = h.max;
                    }
                    if let Some(mut h) = hunger {
                        h.food = h.max;
                    }
                }
                log.done(r.id, "Health, hunger and abilities refilled");
            }
            Cheat::HealAll => {
                for (_, mut h, _) in &mut player {
                    h.hp = h.max;
                }
                let mut n = 0;
                for (_, mind, health, _) in &mut minds {
                    if let (true, Some(mut h)) = (mind.allied, health) {
                        h.hp = h.max;
                        n += 1;
                    }
                }
                log.done(r.id, format!("Healed you and {n} allies"));
            }
            Cheat::Progress(p) => {
                let amount = 1000.0 * p / 100.0;
                if amount > 0.0 {
                    run.award_dna(amount, &GameStage::Creature);
                    run.request_save();
                }
                log.done(r.id, format!("Brain progress {:.0}/1000", run.dna_this_stage));
            }
            Cheat::CompleteStage => {
                let left = 1000.0 - run.dna_this_stage;
                if left > 0.0 {
                    run.award_dna(left, &GameStage::Creature);
                    run.request_save();
                }
                log.done(r.id, "Brain fully grown: press F4 (or the HUD button) to move on to the Tribal stage.");
            }
            Cheat::UnlockParts => {
                let Some(data) = editor_data.as_deref() else { continue };
                let before = run.creature_found_parts.len();
                let mut ids: Vec<u32> = data.parts.keys().copied().collect();
                ids.sort_unstable();
                for id in ids {
                    if !run.creature_found_parts.contains(&id) {
                        run.creature_found_parts.push(id);
                    }
                }
                run.request_save();
                log.done(r.id, format!("Unlocked {} creature parts", run.creature_found_parts.len() - before));
            }
            Cheat::Smite(radius) => {
                let Some(at) = player.iter().next().map(|p| p.0.translation) else { continue };
                let mut n = 0;
                for (t, mut mind, _, _) in &mut minds {
                    if mind.allied || mind.defeated || mind.downed > 0.0 || t.translation.distance(at) > *radius {
                        continue;
                    }
                    // The game's own kill path (poison) pays the rewards.
                    mind.poisoned = 0.01;
                    mind.poison_dps = 1e6;
                    n += 1;
                }
                log.done(r.id, format!("Smote {n} creatures"));
            }
            Cheat::Teleport => {
                let Some((mut t, _, _)) = player.iter_mut().next() else { continue };
                match cursor_point(&windows, &cameras, t.translation.y) {
                    Some(p) if p.distance(t.translation) < 400.0 => {
                        t.translation.x = p.x;
                        t.translation.z = p.z;
                        // A click-to-move goal would walk it straight back.
                        if let Some(c) = controls.as_deref_mut() {
                            c.reset_motion();
                        }
                        log.done(r.id, "Teleported");
                    }
                    _ => log.refuse(r.id, "Point at the ground nearby to teleport."),
                }
            }
            _ => {}
        }
    }
}

fn refill_mobility(m: &mut crate::creature::mobility::CreatureMobility) {
    m.sprint_cooldown = 0.0;
    m.stealth_cooldown = 0.0;
    m.jump_cooldown = 0.0;
}

// ---------------------------------------------------------------------------
// Tribal
// ---------------------------------------------------------------------------

/// Ally the next tribe that is still hostile; false when none is left.
fn ally_next_tribe(campaign: &mut crate::tribal::TribalCampaign) -> bool {
    use crate::tribal::{RivalStatus, TotemSegment};
    campaign.reveal_all();
    let Some(i) = campaign.villages.iter().position(|v| v.status == RivalStatus::Hostile) else {
        return false;
    };
    let v = &mut campaign.villages[i];
    v.relations = crate::tribal_rules::RELATION_MAX;
    v.status = RivalStatus::Allied;
    campaign.totem_carvings.push(TotemSegment::Allied);
    if campaign.performance.as_ref().is_some_and(|p| p.village == i) {
        campaign.performance = None;
    }
    true
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn tribal_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut log: ResMut<CheatLog>,
    stage: Option<Res<State<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    mut members: Query<
        (&GlobalTransform, &mut Health, Option<&mut Hunger>, Option<&mut crate::tribal_rts::TribeUnit>),
        With<crate::game::TribeMate>,
    >,
    mut foes: Query<
        (&GlobalTransform, &mut Health, Option<&crate::tribal_rts::WildAnimal>),
        (
            Or<(With<crate::tribal_rts::RivalMember>, With<crate::tribal_rts::WildAnimal>)>,
            Without<crate::game::TribeMate>,
        ),
    >,
) {
    if stage_of(&stage) != Some(GameStage::Tribal) {
        requests.clear();
        return;
    }
    let Some(run) = run.as_deref_mut() else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        match &r.cheat {
            Cheat::Food(n) => {
                let n = *n as f32;
                if n >= 0.0 {
                    run.award_dna(n, &GameStage::Tribal);
                } else {
                    run.dna_this_stage = (run.dna_this_stage + n).max(0.0);
                }
                run.request_save();
                log.done(r.id, format!("Food {:.0}", run.dna_this_stage));
            }
            Cheat::RefillMotives | Cheat::HealAll => {
                let mut n = 0;
                for (_, mut h, hunger, unit) in &mut members {
                    h.hp = h.max;
                    if let Some(mut hunger) = hunger {
                        hunger.food = hunger.max;
                    }
                    if let Some(mut u) = unit {
                        u.energy = crate::tribal::MEMBER_ENERGY_MAX;
                    }
                    n += 1;
                }
                if run.tribal_campaign.chieftain_respawn > 0.0 {
                    run.tribal_campaign.chieftain_respawn = 0.001;
                }
                log.done(r.id, format!("Healed and fed {n} tribe members"));
            }
            Cheat::AddMember => {
                let members = run.tribe.len();
                let c = &mut run.tribal_campaign;
                c.recruit_cooldown = 0.0;
                let births = c.births.len();
                if c.recruit(crate::tribal::RECRUIT_FOOD, members) && c.births.len() > births {
                    if let Some(b) = c.births.last_mut() {
                        b.remaining = 0.0;
                    }
                    log.done(r.id, "A new tribe member hatches");
                } else {
                    log.refuse(r.id, "The tribe is full: ally or conquer another tribe to grow the hut.");
                }
            }
            Cheat::Progress(p) => {
                let tribes = ((p / 20.0).ceil() as usize).max(1);
                let mut n = 0;
                for _ in 0..tribes {
                    if ally_next_tribe(&mut run.tribal_campaign) {
                        n += 1;
                    }
                }
                run.request_save();
                if n == 0 {
                    log.refuse(r.id, "Every tribe is already allied or conquered.");
                } else {
                    log.done(r.id, format!("{n} more tribe(s) allied ({}/5)", run.tribal_campaign.resolved_count()));
                }
            }
            Cheat::CompleteStage => {
                while ally_next_tribe(&mut run.tribal_campaign) {}
                run.request_save();
                log.done(r.id, "Every tribe is allied: press F4 (or the HUD button) to found your civilization.");
            }
            Cheat::Smite(radius) => {
                let Some(at) = members.iter().next().map(|m| m.0.translation()) else { continue };
                let mut n = 0;
                for (t, mut h, animal) in &mut foes {
                    if animal.is_some_and(|a| !a.hostile) || h.hp <= 0.0 || t.translation().distance(at) > *radius {
                        continue;
                    }
                    h.hp = 0.0;
                    n += 1;
                }
                log.done(r.id, format!("Smote {n} enemies"));
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Civilization
// ---------------------------------------------------------------------------

fn conquest_for(approach: crate::civ::Approach) -> crate::civ::Conquest {
    match approach {
        crate::civ::Approach::Military => crate::civ::Conquest::Military,
        crate::civ::Approach::Religious => crate::civ::Conquest::Religious,
        crate::civ::Approach::Economic => crate::civ::Conquest::Economic,
    }
}

/// Take the rival city nearest `from`; its name, or `None` when every city
/// is yours.
fn capture_nearest(run: &mut RunProgress, from: Vec3) -> Option<String> {
    let index = run.civ_campaign.nearest_rival(from, f32::MAX)?;
    let how = conquest_for(run.civ_campaign.approach);
    let mut spice = run.spice;
    let message = run.civ_campaign.take_city(index, how, &mut spice);
    run.spice = spice;
    run.civ_campaign.message = message.clone();
    run.request_save();
    Some(message)
}

fn add_money(run: &mut RunProgress, amount: f32, cap: f32) {
    run.spice = (run.spice + amount).clamp(0.0, cap);
    run.request_save();
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn civ_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut log: ResMut<CheatLog>,
    stage: Option<Res<State<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    commanders: Query<&GlobalTransform, With<crate::civ::CivCommander>>,
    mut units: Query<(&GlobalTransform, &mut crate::civ_rts::CivUnit)>,
) {
    if stage_of(&stage) != Some(GameStage::Civilization) {
        requests.clear();
        return;
    }
    let Some(run) = run.as_deref_mut() else {
        requests.clear();
        return;
    };
    let from = commanders.iter().next().map(|t| t.translation()).unwrap_or(Vec3::ZERO);
    let cap = crate::civ_rules::WEALTH_CAP as f32;
    for r in requests.read() {
        match &r.cheat {
            Cheat::MoreMoney => {
                add_money(run, MORE_MONEY_CIV, cap);
                log.done(r.id, format!("+{} Sporebucks", fmt_money(MORE_MONEY_CIV)));
            }
            Cheat::Money(n) => {
                add_money(run, *n as f32, cap);
                log.done(r.id, format!("Sporebucks {}", fmt_money(run.spice)));
            }
            Cheat::CaptureCity => match capture_nearest(run, from) {
                Some(m) => log.done(r.id, m),
                None => log.refuse(r.id, "Every city already answers to you."),
            },
            Cheat::Progress(p) => {
                let cities = ((p / 10.0).ceil() as usize).max(1);
                let mut n = 0;
                for _ in 0..cities {
                    if capture_nearest(run, from).is_some() {
                        n += 1;
                    }
                }
                if n == 0 {
                    log.refuse(r.id, "Every city already answers to you.");
                } else {
                    log.done(r.id, format!("Captured {n} cities ({} owned)", run.civ_campaign.owned_count()));
                }
            }
            Cheat::CompleteStage => {
                while capture_nearest(run, from).is_some() {}
                log.done(r.id, "The planet is united: press F4 (or the HUD button) to go to Space.");
            }
            Cheat::UnlockSuperWeapons => {
                // 0xB5F910: empty trait slots follow the civilization type,
                // then the city requirement, cost and recharge are waived.
                use crate::civ::{Approach, CivCellPower as Ce, CivCreaturePower as Cr, CivTribalPower as Tr};
                let c = &mut run.civ_campaign;
                let (cell, creature, tribal) = match c.approach {
                    Approach::Military => (Ce::Invulnerability, Cr::MightyBomb, Tr::GadgetBomb),
                    Approach::Economic => (Ce::StaticBomb, Cr::BribeBomb, Tr::AdBlitz),
                    Approach::Religious => (Ce::HealingAura, Cr::DiploDervish, Tr::BlackCloud),
                };
                c.cell_power.get_or_insert(cell);
                c.creature_power.get_or_insert(creature);
                c.tribal_power.get_or_insert(tribal);
                c.superweapons_cheat = true;
                run.request_save();
                log.done(r.id, "Every superweapon of your civilization type is unlocked.");
            }
            Cheat::SetConsequenceTrait(name) => {
                use crate::civ::{Approach, CivCellPower as Ce, CivCreaturePower as Cr, CivTribalPower as Tr, HOME_INDEX};
                let c = &mut run.civ_campaign;
                let text = match split_trait(name) {
                    Some(("civ", t)) => {
                        let approach = match t {
                            "military" => Approach::Military,
                            "religious" => Approach::Religious,
                            _ => Approach::Economic,
                        };
                        c.approach = approach;
                        c.cities[HOME_INDEX].kind = approach;
                        c.cities[HOME_INDEX].ensure_layout();
                        c.message = approach.hint();
                        format!("Civilization type: {approach:?}")
                    }
                    Some(("cell", t)) => {
                        let p = match t {
                            "carnivore" => Ce::Invulnerability,
                            "herbivore" => Ce::HealingAura,
                            _ => Ce::StaticBomb,
                        };
                        c.cell_power = Some(p);
                        format!("Cell power: {}", p.name())
                    }
                    Some(("creature", t)) => {
                        let p = match t {
                            "aggressive" => Cr::MightyBomb,
                            "social" => Cr::DiploDervish,
                            _ => Cr::BribeBomb,
                        };
                        c.creature_power = Some(p);
                        format!("Creature power: {}", p.name())
                    }
                    Some(("tribe", t)) => {
                        let p = match t {
                            "aggressive" => Tr::GadgetBomb,
                            "social" => Tr::BlackCloud,
                            _ => Tr::AdBlitz,
                        };
                        c.tribal_power = Some(p);
                        format!("Tribal power: {}", p.name())
                    }
                    _ => {
                        log.refuse(r.id, "In Civilization the traits are cell_*, creature_*, tribe_* and civ_* (help setConsequenceTrait).");
                        continue;
                    }
                };
                run.request_save();
                log.done(r.id, text);
            }
            Cheat::RefillMotives | Cheat::HealAll => {
                // 0xB5FBF0: the player's buildings, turrets and vehicles.
                let mut n = 0;
                for (_, mut u) in &mut units {
                    if u.nation == crate::civ::HOME_INDEX && u.hp > 0.0 {
                        u.hp = u.max_hp;
                        n += 1;
                    }
                }
                let mut cities = 0;
                for city in run.civ_campaign.cities.iter_mut().filter(|c| c.owned()) {
                    city.repair(1.0e9);
                    cities += 1;
                }
                run.request_save();
                log.done(r.id, format!("Repaired {n} vehicles and {cities} cities"));
            }
            Cheat::Smite(radius) => {
                let mut n = 0;
                for (t, mut u) in &mut units {
                    if u.nation != crate::civ::HOME_INDEX && u.hp > 0.0 && t.translation().distance(from) <= radius * 4.0 {
                        u.hp = 0.0;
                        n += 1;
                    }
                }
                log.done(r.id, format!("Destroyed {n} rival vehicles"));
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Space
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn space_cheats(
    mut requests: MessageReader<CheatRequest>,
    mut log: ResMut<CheatLog>,
    stage: Option<Res<State<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    mut view: Option<ResMut<crate::space_view::SpaceView>>,
    galaxy: Option<Res<crate::space_galaxy::Galaxy>>,
) {
    if stage_of(&stage) != Some(GameStage::Space) {
        requests.clear();
        return;
    }
    let Some(run) = run.as_deref_mut() else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        // The first notifying cheat in Space earns the Joker badge
        // (`cPlayer` 0xC77E30), whether or not the cheat did anything.
        if super::notifies(&r.cheat) && run.space_campaign.endgame.add_special("joker-1") {
            run.request_save();
        }
        match &r.cheat {
            Cheat::MoreMoney => {
                add_money(run, MORE_MONEY_SPACE, SPACE_MONEY_CAP);
                log.done(r.id, format!("+{} Sporebucks", fmt_money(MORE_MONEY_SPACE)));
            }
            Cheat::Money(n) => {
                add_money(run, *n as f32, SPACE_MONEY_CAP);
                log.done(r.id, format!("Sporebucks {}", fmt_money(run.spice)));
            }
            Cheat::RefillMotives | Cheat::HealAll => {
                // `apply_ship_rules` refills hull and energy on the next frame
                // (the same flag as a repair at a colony).
                run.space_campaign.pending_restore = true;
                run.space_campaign.pending_damage = 0.0;
                log.done(r.id, "Hull and energy refilled");
            }
            Cheat::SpaceCreate => {
                let c = &mut run.space_campaign;
                if !c.has_tool(crate::space_rules::tool::INTERPLANETARY_DRIVE) {
                    log.refuse(r.id, "Advance further in the game before using this tool!");
                    continue;
                }
                // ADAPTED: retail walks its `spaceCheatTools` list (106
                // tools, the colour and sculpt tools among them); OpenSpore
                // walks its store.
                let mut granted = 0;
                for item in crate::space_rules::STORE {
                    if item.consumable {
                        let have = c.charges(item.tool);
                        let add = SPACE_CREATE_AMMO.min(SPACE_CREATE_AMMO_MAX.saturating_sub(have));
                        c.add_charges(item.tool, add);
                    } else if c.grant_tool(item.tool) {
                        granted += 1;
                    }
                }
                c.fire_clock = Default::default();
                c.tool_clocks = Default::default();
                c.generic_clocks.clear();
                c.superweapon_cooldown = 0.0;
                run.request_save();
                log.done(r.id, format!("Tools unlocked ({granted} new) and recharged"));
            }
            Cheat::Spice(n) => {
                let c = &mut run.space_campaign;
                let color = crate::space::SpiceColor::default();
                let have = c.cargo_spice.get(&color).copied().unwrap_or(0) as i64;
                let room = c.cargo_room_for_spice(color) as i64;
                let new = (have + (*n).min(room)).clamp(0, 99) as u8;
                c.cargo_spice.insert(color, new);
                run.request_save();
                log.done(r.id, format!("{color:?} spice in the hold: {new}"));
            }
            Cheat::SetConsequenceTrait(name) => {
                let wanted = match split_trait(name) {
                    Some(("space", t)) => t,
                    _ => "",
                };
                match crate::space::SpaceArchetype::ALL.into_iter().find(|a| a.title().eq_ignore_ascii_case(wanted)) {
                    Some(a) => {
                        run.space_campaign.set_archetype(a);
                        run.request_save();
                        log.done(r.id, format!("Archetype: {} (superpower: {})", a.title(), a.superpower_name()));
                    }
                    None => log.refuse(
                        r.id,
                        "In Space the traits are space_bard, space_ecologist, space_zealot, space_diplomat, space_scientist, space_trader, space_shaman, space_warrior, space_wanderer and space_knight.",
                    ),
                }
            }
            Cheat::RevealGalaxy => {
                let Some(galaxy) = galaxy.as_deref() else { continue };
                let c = &mut run.space_campaign;
                let before = c.visited.len();
                for i in 0..galaxy.stars.len() as u32 {
                    if !c.visited.contains(&i) {
                        c.visited.push(i);
                    }
                }
                run.request_save();
                log.done(r.id, format!("{} more stars explored", run.space_campaign.visited.len() - before));
            }
            Cheat::WarpHome => {
                let home = crate::space_galaxy::HOME_STAR;
                let Some(view) = view.as_deref_mut() else { continue };
                if run.space_campaign.current_system == home {
                    log.refuse(r.id, "The ship is already in its home system.");
                } else if view.travel.is_some() {
                    log.refuse(r.id, "Wait for the jump to finish.");
                } else {
                    view.arrival = Some(home);
                    log.done(r.id, "Warping home");
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Editors: freedom
// ---------------------------------------------------------------------------

fn editor_cheats(mut requests: MessageReader<CheatRequest>, state: Res<CheatState>, mut log: ResMut<CheatLog>) {
    // `freedom` itself is a switch (cheats.rs); say where it applies.
    for r in requests.read() {
        if matches!(r.cheat, Cheat::Freedom(_)) && state.freedom {
            log.done(r.id, String::new());
        }
    }
}

/// Saved editor limits, put back when `freedom` is switched off.
#[derive(Default)]
struct SavedLimits {
    cell: Option<(Vec<(Option<i32>, Option<i32>)>, (Option<i32>, Option<i32>))>,
    creature: Option<Vec<(crate::creature_editor_data::SetupKind, LimitSet)>>,
}

type LimitSet = (Option<i32>, Option<i32>, Option<i32>, Option<i32>, Option<f32>);

fn editor_freedom(
    state: Res<CheatState>,
    cell: Option<ResMut<crate::cell_editor_data::CellEditorData>>,
    creature: Option<ResMut<crate::creature_editor_data::CreatureEditorData>>,
    mut saved: Local<SavedLimits>,
) {
    if let Some(mut cell) = cell {
        match (state.freedom, saved.cell.is_some()) {
            (true, false) => {
                let levels = cell.levels.iter().map(|l| (l.complexity_limit, l.max_baked_blocks)).collect();
                saved.cell = Some((levels, (cell.template.complexity_limit, cell.template.max_baked_blocks)));
                for l in cell.levels.iter_mut() {
                    l.complexity_limit = Some(i32::MAX);
                    l.max_baked_blocks = Some(i32::MAX);
                }
                cell.template.complexity_limit = Some(i32::MAX);
                cell.template.max_baked_blocks = Some(i32::MAX);
            }
            (false, true) => {
                let (levels, template) = saved.cell.take().unwrap_or_default();
                for (l, (c, b)) in cell.levels.iter_mut().zip(levels) {
                    l.complexity_limit = c;
                    l.max_baked_blocks = b;
                }
                (cell.template.complexity_limit, cell.template.max_baked_blocks) = template;
            }
            _ => {}
        }
    }
    if let Some(mut creature) = creature {
        match (state.freedom, saved.creature.is_some()) {
            (true, false) => {
                let mut list = Vec::new();
                for (kind, s) in creature.setups.iter_mut() {
                    list.push((
                        *kind,
                        (s.complexity_limit, s.bone_limit, s.max_baked_blocks, s.max_block_count, s.max_geom_score),
                    ));
                    // A limit of 0 is "no limit" (`Complexity::room`).
                    s.complexity_limit = Some(0);
                    s.bone_limit = Some(0);
                    s.max_baked_blocks = Some(0);
                    s.max_block_count = Some(0);
                    s.max_geom_score = Some(0.0);
                }
                saved.creature = Some(list);
            }
            (false, true) => {
                for (kind, l) in saved.creature.take().unwrap_or_default() {
                    if let Some(s) = creature.setups.get_mut(&kind) {
                        (s.complexity_limit, s.bone_limit, s.max_baked_blocks, s.max_block_count, s.max_geom_score) = l;
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Switches kept every frame
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
fn keep_switches(
    state: Res<CheatState>,
    stage: Option<Res<State<GameStage>>>,
    mut run: Option<ResMut<RunProgress>>,
    mut health: Query<&mut Health, Or<(With<Player>, With<crate::game::TribeMate>)>>,
    mut cells: Query<&mut crate::cell_engine::CellObject, With<Player>>,
    mut units: Query<&mut crate::civ_rts::CivUnit>,
    (mut encounter, mut mobility): (
        Option<ResMut<crate::creature_actions::Encounter>>,
        Option<ResMut<crate::creature::mobility::CreatureMobility>>,
    ),
) {
    if !state.god_mode && !state.infinite_energy {
        return;
    }
    let stage = stage_of(&stage);
    if state.god_mode {
        for mut h in &mut health {
            if h.hp < h.max {
                h.hp = h.max;
            }
        }
        match stage {
            Some(GameStage::Cell) => {
                for mut o in &mut cells {
                    o.hp = crate::cell_rules::MAX_HP;
                    o.invulnerable = o.invulnerable.max(0.5);
                }
            }
            Some(GameStage::Creature) => {
                if let Some(e) = encounter.as_deref_mut() {
                    e.poison_seconds = 0.0;
                    e.stun = 0.0;
                }
            }
            Some(GameStage::Tribal) => {
                if let Some(run) = run.as_deref_mut() {
                    if run.tribal_campaign.chieftain_respawn > 0.0 {
                        run.tribal_campaign.chieftain_respawn = 0.001;
                    }
                }
            }
            Some(GameStage::Civilization) => {
                for mut u in &mut units {
                    if u.nation == crate::civ::HOME_INDEX && u.hp > 0.0 {
                        u.hp = u.max_hp;
                    }
                }
            }
            Some(GameStage::Space) => {
                if let Some(run) = run.as_deref_mut() {
                    run.space_campaign.pending_damage = 0.0;
                }
            }
            _ => {}
        }
    }
    if state.infinite_energy {
        match stage {
            Some(GameStage::Creature) => {
                if let Some(e) = encounter.as_deref_mut() {
                    e.cooldowns = [0.0; 4];
                }
                if let Some(m) = mobility.as_deref_mut() {
                    refill_mobility(m);
                }
            }
            Some(GameStage::Space) => {
                if let Some(run) = run.as_deref_mut() {
                    let max = run.space_campaign.max_reactor_energy();
                    run.space_campaign.energy = max;
                }
            }
            _ => {}
        }
    }
}

/// God mode again after the simulation, so a blow taken this frame does not
/// stay on the HUD.
fn keep_switches_after(state: Res<CheatState>, mut health: Query<&mut Health, Or<(With<Player>, With<crate::game::TribeMate>)>>) {
    if !state.god_mode {
        return;
    }
    for mut h in &mut health {
        if h.hp > 0.0 && h.hp < h.max {
            h.hp = h.max;
        }
    }
}

// ---------------------------------------------------------------------------
// Free camera
// ---------------------------------------------------------------------------

/// Keys and the turning button held for the free camera. `ButtonInput` is
/// reset every frame so the game does not see them, which also forgets that
/// they are held, so the camera keeps its own record from the input messages.
#[derive(Default)]
struct Held {
    keys: std::collections::HashSet<KeyCode>,
    turning: bool,
}

#[allow(clippy::too_many_arguments)]
fn free_cam_input(
    mut free: ResMut<FreeCam>,
    state: Res<CheatState>,
    stage: Option<Res<State<GameStage>>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    (mut typed, mut clicks, mut focus_lost): (
        MessageReader<bevy::input::keyboard::KeyboardInput>,
        MessageReader<bevy::input::mouse::MouseButtonInput>,
        MessageReader<bevy::input::keyboard::KeyboardFocusLost>,
    ),
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    time: Res<Time<Real>>,
    mut held: Local<Held>,
) {
    use bevy::input::ButtonState;
    for k in typed.read() {
        match k.state {
            ButtonState::Pressed => held.keys.insert(k.key_code),
            ButtonState::Released => held.keys.remove(&k.key_code),
        };
    }
    for b in clicks.read().filter(|b| b.button == MouseButton::Right) {
        held.turning = b.state == ButtonState::Pressed;
    }
    if focus_lost.read().count() > 0 {
        *held = Held::default();
    }
    // A new stage brings its own camera back.
    if free.on && stage.is_some_and(|s| s.is_changed()) {
        free.on = false;
    }
    if !free.on || state.captures_input() {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    if held.turning {
        free.yaw -= motion.delta.x * 0.004;
        free.pitch = (free.pitch - motion.delta.y * 0.004).clamp(-1.5, 1.5);
    }
    if scroll.delta.y != 0.0 {
        free.speed = (free.speed * if scroll.delta.y > 0.0 { 1.25 } else { 0.8 }).clamp(1.0, 2000.0);
    }
    let rotation = Quat::from_euler(EulerRot::YXZ, free.yaw, free.pitch, 0.0);
    let forward = rotation * -Vec3::Z;
    let right = rotation * Vec3::X;
    let mut dir = Vec3::ZERO;
    let down = |k: &[KeyCode]| k.iter().any(|c| held.keys.contains(c));
    if down(&[KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir += forward;
    }
    if down(&[KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir -= forward;
    }
    if down(&[KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir += right;
    }
    if down(&[KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir -= right;
    }
    if down(&[KeyCode::KeyE, KeyCode::Space]) {
        dir += Vec3::Y;
    }
    if down(&[KeyCode::KeyQ]) {
        dir -= Vec3::Y;
    }
    let boost = if down(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]) { 4.0 } else { 1.0 };
    let step = dir.normalize_or_zero() * free.speed * boost * dt;
    free.position += step;
    // The flying keys and the turning button do not reach the game.
    for k in [
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyQ,
        KeyCode::KeyE,
        KeyCode::Space,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
    ] {
        keys.reset(k);
    }
    mouse.reset(MouseButton::Right);
}

/// Runs in `Last`, after the transforms propagated, so it sets the camera's
/// global transform too (the camera is a root entity).
#[allow(clippy::type_complexity)]
fn free_cam_apply(
    free: Res<FreeCam>,
    mut cameras: Query<(&mut Transform, &mut GlobalTransform), (With<crate::GameCamera>, Without<ChildOf>)>,
) {
    if !free.on {
        return;
    }
    for (mut t, mut global) in &mut cameras {
        t.translation = free.position;
        t.rotation = Quat::from_euler(EulerRot::YXZ, free.yaw, free.pitch, 0.0);
        *global = GlobalTransform::from(*t);
    }
}

/// The menu's "Teleport" waits for a click in the world.
fn teleport_click(
    mut state: ResMut<CheatState>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut sender: super::CheatSender,
) {
    if !state.teleport_armed || state.captures_input() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        state.teleport_armed = false;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        state.teleport_armed = false;
        mouse.reset(MouseButton::Left);
        sender.send(Cheat::Teleport);
    }
}

// ---------------------------------------------------------------------------
// Hidden HUD
// ---------------------------------------------------------------------------

/// Root UI nodes the hidden HUD hid (and only those come back).
#[allow(clippy::type_complexity)]
fn hide_hud(
    state: Res<CheatState>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    mut roots: Query<(Entity, &mut Visibility), (With<Node>, Without<ChildOf>, Without<super::CheatUi>)>,
    mut hidden: Local<Vec<Entity>>,
) {
    let paused = pause.is_some_and(|p| p.open) || state.holds_game();
    let hide = state.hud_hidden || (!state.pause_ui_visible && paused);
    if hide {
        for (entity, mut v) in &mut roots {
            if *v != Visibility::Hidden && !hidden.contains(&entity) {
                *v = Visibility::Hidden;
                hidden.push(entity);
            } else if hidden.contains(&entity) && *v != Visibility::Hidden {
                // Something showed it again this frame.
                *v = Visibility::Hidden;
            }
        }
    } else if !hidden.is_empty() {
        for entity in hidden.drain(..) {
            if let Ok((_, mut v)) = roots.get_mut(entity) {
                if *v == Visibility::Hidden {
                    *v = Visibility::Inherited;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stats readout
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn readout(
    state: Res<CheatState>,
    mut out: ResMut<super::overlay::Readout>,
    stage: Option<Res<State<GameStage>>>,
    run: Option<Res<RunProgress>>,
    players: Query<(&GlobalTransform, Option<&Health>, Option<&Hunger>), With<Player>>,
    clock: Option<Res<crate::planet_sky::StageClock>>,
    speed: Res<DaySpeed>,
    free: Res<FreeCam>,
) {
    if !state.stats {
        return;
    }
    let player = players.iter().next();
    out.player = player.map(|p| p.0.translation());
    let mut lines = Vec::new();
    if let Some(h) = player.and_then(|p| p.1) {
        lines.push(format!("Health {:.0} / {:.0}", h.hp, h.max));
    }
    if let Some(h) = player.and_then(|p| p.2) {
        lines.push(format!("Hunger {:.0} / {:.0}", h.food, h.max));
    }
    if let (Some(run), Some(stage)) = (run.as_deref(), stage_of(&stage)) {
        match stage {
            GameStage::Cell => {
                let earned = crate::cell_rules::dna_earned(run.cell.food_progression);
                lines.push(format!("DNA {}", (earned - run.cell.evolution_points_spent).max(0)));
                lines.push(format!("Evolution {}/{}", run.cell.food_progression, crate::cell_engine::STAGE_FOOD));
            }
            GameStage::Creature => {
                lines.push(format!("DNA {:.0} to spend, {:.0}/1000 brain", run.editor_dna, run.dna_this_stage));
                lines.push(format!("Brain level {}", run.creature_progression.brain_level));
                let t = run.species_world.time_of_day;
                lines.push(format!("Time {:02}:{:02}", t as u32 % 24, ((t.fract()) * 60.0) as u32));
            }
            GameStage::Tribal => {
                lines.push(format!("Food {:.0}", run.dna_this_stage));
                lines.push(format!("Members {}", run.tribe.len()));
                lines.push(format!("Tribes won {}/5", run.tribal_campaign.resolved_count()));
            }
            GameStage::Civilization => {
                lines.push(format!("Sporebucks {}", fmt_money(run.spice)));
                lines.push(format!("Cities {}/10", run.civ_campaign.owned_count()));
            }
            GameStage::Space => {
                let c = &run.space_campaign;
                lines.push(format!("Sporebucks {}", fmt_money(run.spice)));
                lines.push(format!("Energy {:.0} / {:.0}", c.energy, c.max_reactor_energy()));
                lines.push(format!("System {}  archetype {}", c.current_system, c.archetype.title()));
            }
            GameStage::MainMenu => {}
        }
        if matches!(stage, GameStage::Tribal | GameStage::Civilization) {
            if let Some(c) = clock.as_deref().filter(|c| c.stage == stage) {
                lines.push(format!("Time {:02}:{:02}", c.hour as u32 % 24, (c.hour.fract() * 60.0) as u32));
            }
        }
    }
    if (speed.0 - 1.0).abs() > 1e-3 {
        lines.push(format!("Day clock x{}", speed.0));
    }
    if free.on {
        let p = free.position;
        lines.push(format!("Free camera {:.1}, {:.1}, {:.1} (speed {:.0})", p.x, p.y, p.z, free.speed));
    }
    out.lines = lines;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_reads_with_thousands() {
        assert_eq!(fmt_money(150_000.0), "150,000");
        assert_eq!(fmt_money(999.0), "999");
        assert_eq!(fmt_money(-1234.0), "-1,234");
    }

    #[test]
    fn consequence_traits_are_the_original_names() {
        assert_eq!(CONSEQUENCE_TRAITS.len(), 22);
        assert_eq!(split_trait("CIV_MILITARY"), Some(("civ", "military")));
        assert_eq!(split_trait("bard"), Some(("space", "bard")));
        assert_eq!(split_trait("tribe_mixed"), Some(("tribe", "mixed")));
        assert_eq!(split_trait("pirate"), None);
        // Every Space trait has an archetype to switch to.
        for (full, _) in CONSEQUENCE_TRAITS.iter().filter(|(n, _)| n.starts_with("space_")) {
            let t = &full["space_".len()..];
            assert!(
                crate::space::SpaceArchetype::ALL.iter().any(|a| a.title().eq_ignore_ascii_case(t)),
                "{t} has no archetype"
            );
        }
    }

    fn stage_app(stage: GameStage) -> App {
        let mut app = crate::cheats::tests::cheat_app();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(stage);
        app.insert_resource(RunProgress::default());
        app.update();
        app
    }

    fn last(app: &App) -> String {
        crate::cheats::tests::last_lines(app, 1).pop().unwrap_or_default()
    }

    #[test]
    fn retail_amounts_and_where_cheats_work() {
        use crate::cheats::tests::send;
        let mut app = stage_app(GameStage::Civilization);
        let before = app.world().resource::<RunProgress>().spice;
        send(&mut app, Cheat::MoreMoney);
        assert_eq!(app.world().resource::<RunProgress>().spice, before + MORE_MONEY_CIV);
        // addDNA only works while an editor is open.
        send(&mut app, Cheat::AddDna);
        assert_eq!(last(&app), "addDNA isn't available here.");

        let mut app = stage_app(GameStage::Space);
        let before = app.world().resource::<RunProgress>().spice;
        assert!(!app.world().resource::<RunProgress>().space_campaign.endgame.has_special("joker-1"));
        send(&mut app, Cheat::MoreMoney);
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.spice, before + MORE_MONEY_SPACE);
        assert!(run.space_campaign.endgame.has_special("joker-1"), "the first cheat in Space earns the Joker badge");
        send(&mut app, Cheat::SpaceCreate);
        assert_eq!(last(&app), "Advance further in the game before using this tool!");

        let mut app = stage_app(GameStage::Cell);
        send(&mut app, Cheat::AddDna);
        assert!(last(&app).starts_with("addDNA works in the cell creator"));
        send(&mut app, Cheat::Dna(40));
        let run = app.world().resource::<RunProgress>();
        assert_eq!(crate::cell_rules::dna_earned(run.cell.food_progression) - run.cell.evolution_points_spent, 40);
    }

    #[test]
    fn the_free_camera_flies_on_held_keys_the_game_never_sees() {
        use bevy::input::{keyboard::Key, keyboard::KeyboardInput, ButtonState};
        let mut app = crate::cheats::tests::cheat_app();
        app.update();
        crate::cheats::tests::send(&mut app, Cheat::FreeCam);
        assert!(app.world().resource::<FreeCam>().on);
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state: ButtonState::Pressed,
            text: Some("w".into()),
            repeat: false,
            window,
        });
        app.update();
        let first = app.world().resource::<FreeCam>().position.z;
        // Held with no further messages (no key repeat): it keeps flying.
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.update();
            assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        }
        let later = app.world().resource::<FreeCam>().position.z;
        assert!(later < first && first <= 0.0, "forward is -Z: {first} -> {later}");
    }

    #[test]
    fn allying_fills_the_tribal_bar() {
        let mut c = crate::tribal::TribalCampaign::default();
        if c.villages.is_empty() {
            return;
        }
        let total = c.villages.len();
        let mut n = 0;
        while ally_next_tribe(&mut c) {
            n += 1;
        }
        assert_eq!(n, total);
        assert_eq!(c.resolved_count(), total);
        assert!(!ally_next_tribe(&mut c));
    }
}
