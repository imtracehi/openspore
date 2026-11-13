//! Part guarantee (RE 10A, `AddFood` 0xE7CE10 -> 0xE79720, the "part
//! cinematic"). A player who has not found a part yet is shown one between 120
//! and 150 food: the retail routine marks the cinematic as played, removes the
//! current unlock pickup (`cCellGame+0x516C`), clears the space on one side of
//! the view, creates the special
//! cells #1 `cell_GoldySpecial` (the stationary "Pokey" whose loot table
//! `loot_spikey` yields the spike part) and #2 `Cell_ChomperSpecial` (a chomper
//! whose chase target is the Pokey), points the camera at the Pokey (zoom 10)
//! and runs a 15 s event with the `0x1485A30` cinematic.
//!
//! ORIGINAL: the trigger (not played, 119 < food < 151 on the food before the
//! meal, at most four unlocked part slots, difficulty not hard), the two special
//! cells and their data, and (VERIFIED this round, 0xE79720 read in full) the
//! geometry: the side is +x when the avatar's facing has a positive x
//! (0xE5EB20 gives the facing, 0xE577A0 the side), the Pokey stands 15 and the
//! chomper 25 world units from the camera LOOK point along that side, and every
//! foreground cell within 5 units of the Pokey's spot and within 15 of the
//! chomper's (0xE78B20: FindCellsInRadius 0xE87210 = bounding sphere against the
//! query sphere, then 0xE780A0 = destroy) is removed except the avatar.
//! The chomper's preset chase (0xE79720 sets `+0x22C` = the Pokey and `+0x230` =
//! 1000 s on the new chomper): the spawn request has no target hook, so
//! [`part_guide_chase`] links the two cells once the spawn queue has made them and
//! starts the chase through the AI's protect reaction (0xE70BC0, `start_chase`).
//!
//! The event (VERIFIED, helper `cell-final4`; the type-0x24 record 0xE79920 creates,
//! the per-frame update 0xE7E6C0 -> 0xE7BA30 -> 0xE5F160, the end 0xE7E130 ->
//! 0xE794F0, the cinematic's continue message 0x445F729 -> 0xE7F960 and the skip
//! 0xE7F5A0): see [`PartGuideScene`]. The `ui+0xF4 = 10` the creation writes next
//! to `ui+0xE0` is the follow speed of the camera, not a zoom (`cell-final3`).
//!
//! The avatar's `+0x170 = Pokey id` (VERIFIED, helper `cell-final5`; nothing to port): the cell object's `+0x170` is a
//! "face this object" id. Its readers are `MoveToTarget` 0xE56F80 (target orientation `+0x14` = look-at of the object,
//! then the move target), the orientation step 0xE68170 (the avatar's own update 0xE6F5B0 calls it every frame and
//! the mating start 0xE75D0C / 0xE75D2F for both mates; the AI's command-target pick 0xE6B2B0 reads it as well) and the
//! idle state of a cell with the sequence flag `+0x17E` (0xE71386: look-at, then `0xE61E20` turns toward it at 5 rad/s).
//! The target orientation is applied only by `0xE61E20`, whose callers are the propulsion update 0xE6EEC0 (returns at
//! once while `+4` = idle, 0xE6EF18) and that idle state. The scene's lock sets `+4` = 1 and `+0x16C / +0x178 /
//! +0x17B / +0x17C` but not `+0x17E` (0xE5F32C..0xE5F34A), so the avatar does not turn toward the Pokey: the id has no
//! visible consumer in this scene (the mating start does set `+0x17E`; `cell_flow` already turns both mates to face each other).
//!
//! ADAPTED: the scene ends when the `goldies_demise` cinematic stops (the retail
//! event ends on the cinematic's continue message and never on its 15 s timer,
//! 0xE7E750); with no cinematic (disabled, or none ever started) the timer
//! `evt+0x1C` ends it, and Escape / Space stand for the retail skip message
//! (its binding is not mapped).

use bevy::prelude::*;

use crate::{
    cell_data::CellDatabase,
    cell_engine::{heading_dir, heading_of, CellGame, CellObject, CellSpawnQueue, SpawnRequest, UNIT},
    cell_rules::{CellCombat, DeathKind},
    cell_flow::CellFlow,
    cell_rules::CellSave,
    game::{Player, RunProgress},
    world_objects::name_id,
};

/// Special cell #1 / #2 of the special resource table (RE 11.3).
const GOLDY_SPECIAL: &str = "cell_GoldySpecial";
const CHOMPER_SPECIAL: &str = "Cell_ChomperSpecial";
/// Distances of the Pokey and the chomper from the camera look point along the
/// chosen side, retail world units (0xE577A0: 15 and 25).
pub const GOLDY_DISTANCE: f32 = 15.0;
pub const CHOMPER_DISTANCE: f32 = 25.0;
/// Clear radii around the Pokey's and the chomper's spots (0xE79720 calls
/// 0xE78B20 with 5.0 and 15.0).
pub const GOLDY_CLEAR_RADIUS: f32 = 5.0;
pub const CHOMPER_CLEAR_RADIUS: f32 = 15.0;

/// The two spots of the scene in world units: the side is +x when the avatar's
/// facing x is positive, else -x (0xE577A0); the Pokey at `look + side * 15`,
/// the chomper at `look + side * 25`.
pub fn scene_spots(look: Vec2, facing: Vec2) -> (Vec2, Vec2) {
    let side = if facing.x > 0.0 { Vec2::X } else { Vec2::NEG_X };
    (look + side * GOLDY_DISTANCE, look + side * CHOMPER_DISTANCE)
}

/// Whether the guarantee is due at a meal (`food` = progression before it).
pub fn due(save: &CellSave, food: i32) -> bool {
    !save.part_cinematic_played
        && food > 119
        && food < 151
        && save.unlocked_parts.iter().filter(|v| **v != 0).count() <= 4
        && save.difficulty != 2
}

/// The scene's two cells were queued and the chomper still has to be pointed at
/// the Pokey (set by [`part_guide`], cleared by [`part_guide_chase`]).
#[derive(Resource, Default)]
pub struct PartGuideChase {
    pub pending: bool,
    /// Frames the link has waited (the spawn queue runs later in the same frame).
    pub waited: u32,
}

/// Point the scene's chomper at the Pokey (0xE79720: `+0x22C` / `+0x230` = 1000 s).
pub fn part_guide_chase(
    mut link: ResMut<PartGuideChase>,
    cells: Query<(Entity, &CellObject), Without<Player>>,
    mut chompers: Query<(&mut crate::cell_ai::CellAiState, Option<&mut crate::cell_contract::CellAnimState>)>,
) {
    if !link.pending {
        return;
    }
    link.waited += 1;
    let find = |name: &str| {
        let cell = name_id(name);
        cells.iter().find(|(_, o)| o.cell == cell && !o.dead && !o.background).map(|(e, _)| e)
    };
    let (Some(goldy), Some(chomper)) = (find(GOLDY_SPECIAL), find(CHOMPER_SPECIAL)) else {
        // The queue has not made them yet; give up when a cell is not in the data.
        if link.waited > 600 {
            link.pending = false;
        }
        return;
    };
    if let Ok((mut ai, anim)) = chompers.get_mut(chomper) {
        crate::cell_ai::start_chase(&mut ai, anim.map(|a| a.into_inner()), goldy, CHOMPER_CHASE_SECONDS);
    }
    link.pending = false;
}

/// `+0x230` of the scene's chomper: 1000 s of chase (0xE79720).
pub const CHOMPER_CHASE_SECONDS: f32 = 1000.0;

/// The event record of the scene (type 0x24, pool `sCellGame+0x54`): duration `+0x1C` /
/// `+0x20` 15 s (0xE79920), the Pokey `+0x28`, the chomper `+0x2C`; `sCellGame+0x5194` /
/// `+0x5198` hold its id (the `+0x5198` event is exempt from the timer, 0xE7E750).
pub const EVENT_SECONDS: f32 = 15.0;
/// The camera's follow speed while it looks at the Pokey (`ui+0xF4`, 0xE798CF).
pub const POKEY_FOLLOW_SPEED: f32 = 10.0;
/// ... while it looks at the part pickup (0xE5F1DA) and when it returns to the avatar (0xE796C6).
pub const PICKUP_FOLLOW_SPEED: f32 = 5.0;
/// `ui+0x80`: the camera offset's target height once the pickup exists (0xE5F1E2).
pub const PICKUP_OFFSET_Z: f32 = 10.0;
/// With the Pokey gone the remaining time is cut to this (0xE5F184).
pub const GONE_SECONDS: f32 = 8.0;
/// The chomper drifts away from the pickup at this many units per second (0xE5F2A9) and
/// its target is a point this far beyond it (0xE5F287).
pub const CHOMPER_DRIFT: f32 = 2.0;
pub const CHOMPER_TARGET_DISTANCE: f32 = 10.0;
/// 0xE5F010's step and 0xE794F0's / 0xE79460's radius (retail units).
pub const END_NUDGE: f32 = 4.5;
pub const END_CLEAR_RADIUS: f32 = 10.0;
/// The cinematic of the scene (string 0x1485A30, started at 0xE799DD).
pub const CINEMATIC: &str = "goldies_demise";

/// The running event of the part scene (0xE79720 .. 0xE794F0), `active` from the
/// creation until the end action. VERIFIED per frame (0xE5F160): the avatar is locked
/// (the same four flags the other sequence events set, `+0x16C` / `+0x178` / `+0x17B`
/// / `+0x17C`); while the Pokey is gone the time left is cut to 8 s; once the part
/// pickup exists (`cCellGame+0x516C`) the camera looks at it (speed 5, offset target
/// 10) and the chomper drifts away from it at 2 units per second. Ambient spawns are
/// suppressed while it runs (`cCellGame+0x518C`, 0xE771D0).
#[derive(Resource, Default)]
pub struct PartGuideScene {
    pub active: bool,
    pub goldy: Option<Entity>,
    pub chomper: Option<Entity>,
    /// `evt+0x1C`: seconds left.
    pub time: f32,
    pub age: f32,
    /// The cinematic has been seen playing.
    pub saw_cinematic: bool,
}

impl PartGuideScene {
    /// The record created by 0xE79720.
    pub fn start(&mut self) {
        *self = Self { active: true, time: EVENT_SECONDS, ..default() };
    }

    /// 0xE5F160's time rule: with the Pokey gone, no more than 8 s remain.
    pub fn tick(&mut self, dt: f32, goldy_gone: bool) {
        self.time -= dt;
        self.age += dt;
        if goldy_gone && self.time > GONE_SECONDS {
            self.time = GONE_SECONDS;
        }
    }

    /// Whether the event is over: the cinematic ran and stopped (message 0x445F729 ->
    /// 0xE7F960), or - ADAPTED - no cinematic ever ran and the timer is out.
    pub fn over(&self, cinematic_playing: bool) -> bool {
        if self.saw_cinematic {
            !cinematic_playing
        } else {
            !cinematic_playing && self.time <= 0.0
        }
    }
}

/// 0xE5F010 (`A`, `B`, step): when `B` is farther than `step` from `A`, `A` moves `step`
/// toward it.
pub fn nudge(a: Vec2, b: Vec2, step: f32) -> Vec2 {
    let d = b - a;
    if d.length() > step {
        a + d / (d.length_squared() + 1e-8).sqrt() * step
    } else {
        a
    }
}

fn plane(t: &Transform) -> Vec2 {
    Vec2::new(t.translation.x, t.translation.z)
}

/// The scene's per-frame event update (0xE7E6C0 -> 0xE5F160) and its end (0xE794F0).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn part_guide_scene(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    cinematics: Option<ResMut<crate::cinematics::Cinematics>>,
    mut scene: ResMut<PartGuideScene>,
    mut game: ResMut<CellGame>,
    mut player: Query<(&mut Transform, Option<&mut CellCombat>), With<Player>>,
    mut cells: Query<(Entity, &mut Transform, &mut CellObject, Option<&mut CellCombat>), Without<Player>>,
) {
    if !scene.active {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let Ok((mut pt, mut pcombat)) = player.single_mut() else {
        return;
    };
    let here = plane(&pt);
    // The two special cells exist a frame or two after the request (the spawn queue).
    if scene.goldy.is_none() || scene.chomper.is_none() {
        let find = |name: &str| {
            let cell = name_id(name);
            cells.iter().find(|(_, _, o, _)| o.cell == cell && !o.dead && !o.background).map(|(e, ..)| e)
        };
        if scene.goldy.is_none() {
            scene.goldy = find(GOLDY_SPECIAL);
        }
        if scene.chomper.is_none() {
            scene.chomper = find(CHOMPER_SPECIAL);
        }
        for e in [scene.goldy, scene.chomper].into_iter().flatten() {
            if !game.protected.contains(&e) {
                // +0x354: kept out of the cull (0xE79881 / 0xE798D7).
                game.protected.push(e);
            }
        }
    }
    let goldy_at = scene.goldy.and_then(|e| cells.get(e).ok()).filter(|(_, _, o, _)| !o.dead).map(|(_, t, ..)| plane(t));
    let gone = scene.goldy.is_some() && goldy_at.is_none();
    scene.tick(dt, gone);
    // 0xE5F160: the part pickup exists -> the camera looks at it, the chomper leaves it.
    let pickup_at = game.unlock_pickup.and_then(|e| cells.get(e).ok()).map(|(_, t, ..)| plane(t));
    if let Some(pickup_at) = pickup_at {
        game.camera_object = Some(pickup_at);
        game.camera_speed = PICKUP_FOLLOW_SPEED;
        game.offset_z_target = PICKUP_OFFSET_Z;
        if let Some((_, mut t, mut o, _)) = scene.chomper.and_then(|e| cells.get_mut(e).ok()) {
            let at = plane(&t);
            let away = at - pickup_at;
            let n = away / (away.length_squared() + 1e-8).sqrt();
            o.target = Some(at + n * CHOMPER_TARGET_DISTANCE * UNIT);
            let moved = at + n * CHOMPER_DRIFT * dt * UNIT;
            (t.translation.x, t.translation.z) = (moved.x, moved.y);
        }
    } else {
        // 0xE798E4: the camera looks at the Pokey (a focus id that no longer resolves
        // leaves the follow rules to the avatar).
        game.camera_object = goldy_at;
    }
    // The avatar is locked like in the other sequence events.
    game.locked = true;
    if let Some(c) = pcombat.as_mut() {
        c.hit_cooldown = c.hit_cooldown.max(0.1);
        c.busy = c.busy.max(0.1);
    }
    // The end: the cinematic's continue / end, or Escape / Space (the skip).
    let playing = cinematics
        .as_ref()
        .is_some_and(|c| c.is_playing() && c.current().is_some_and(|(_, name, _)| name == CINEMATIC));
    scene.saw_cinematic |= playing;
    let skipped = keys.just_pressed(KeyCode::Escape) || keys.just_pressed(crate::cell_flow::SKIP_KEY);
    if !scene.over(playing) && !skipped {
        return;
    }
    // 0xE794F0.
    if let Some(mut cinematics) = cinematics {
        cinematics.stop_named(CINEMATIC);
    }
    game.camera_object = None;
    game.camera_speed = crate::cell_engine::CAMERA_SPEED_DEFAULT;
    let look = game.camera_look / UNIT;
    // The chomper, when off screen (0xE5CDE0), steps 4.5 toward the avatar.
    if let Some((_, mut t, o, _)) = scene.chomper.and_then(|e| cells.get_mut(e).ok()) {
        let at = plane(&t);
        if !crate::cell_population::pin_visible(look, at / UNIT, o.radius / UNIT) {
            let moved = nudge(at / UNIT, here / UNIT, END_NUDGE) * UNIT;
            (t.translation.x, t.translation.z) = (moved.x, moved.y);
        }
    }
    // The Pokey goes (0xE780A0 with the effect flag): killed, its loot drops.
    if let Some((_, _, mut o, combat)) = scene.goldy.and_then(|e| cells.get_mut(e).ok()) {
        if !o.dead {
            o.dead = true;
            if let Some(mut combat) = combat {
                combat.death = DeathKind::Killed;
            }
        }
    }
    // 0xE79460: every cell within 10 units of the avatar that is off screen goes.
    for (e, t, mut o, _) in &mut cells {
        let at = plane(&t);
        if Some(e) != scene.goldy
            && !o.background
            && !o.dead
            && at.distance(here) < END_CLEAR_RADIUS * UNIT
            && !crate::cell_population::pin_visible(look, at / UNIT, o.radius / UNIT)
        {
            o.dead = true;
        }
    }
    // The part pickup and the avatar: skipped, the pickup comes 4.5 toward the avatar and the
    // camera cuts back at once; at the natural end the avatar steps 4.5 toward the pickup and
    // the camera glides back at speed 5 with the avatar as its focus object.
    game.offset_z_target = crate::cell_engine::CAMERA_OFFSET_Z;
    let pickup = game.unlock_pickup;
    if skipped {
        if let Some((_, mut t, ..)) = pickup.and_then(|e| cells.get_mut(e).ok()) {
            let moved = nudge(plane(&t) / UNIT, here / UNIT, END_NUDGE) * UNIT;
            (t.translation.x, t.translation.z) = (moved.x, moved.y);
        }
        (game.camera_look, game.camera_lead, game.camera_focus) = (here, here, here);
    } else {
        if let Some(pickup_at) = pickup.and_then(|e| cells.get(e).ok()).map(|(_, t, ..)| plane(t)) {
            let moved = nudge(here / UNIT, pickup_at / UNIT, END_NUDGE) * UNIT;
            (pt.translation.x, pt.translation.z) = (moved.x, moved.y);
        }
        game.camera_focus_avatar = true;
        game.camera_speed = PICKUP_FOLLOW_SPEED;
    }
    game.protected.clear();
    game.locked = false;
    *scene = PartGuideScene::default();
}

/// Spawn the scene once the guarantee is due and the avatar is free.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn part_guide(
    mut commands: Commands,
    mut run: ResMut<RunProgress>,
    mut game: ResMut<CellGame>,
    flow: Res<CellFlow>,
    db: Res<CellDatabase>,
    mut queue: ResMut<CellSpawnQueue>,
    mut link: ResMut<PartGuideChase>,
    mut scene: ResMut<PartGuideScene>,
    player: Query<(&Transform, &CellObject), With<Player>>,
    others: Query<(Entity, &Transform, &CellObject), Without<Player>>,
) {
    if !run.cell.part_guide_pending || !flow.player_free() || game.player.is_none() {
        return;
    }
    let Ok((transform, object)) = player.single() else {
        return;
    };
    run.cell.part_guide_pending = false;
    run.cell.part_cinematic_played = true;
    if std::env::var_os("OPENSPORE_CELL_DEBUG").is_some() {
        eprintln!("cell part guide: scene at food {}", run.cell.food_progression);
    }
    let here = Vec2::new(transform.translation.x, transform.translation.z);
    let facing = heading_dir(object.heading);
    // Retail world units -> Bevy plane units (`UNIT`); the camera look point is
    // already in Bevy plane coordinates.
    // 0xE79765: the unlock pickup object of `cCellGame+0x516C` is destroyed first.
    if let Some(pickup) = game.unlock_pickup.take() {
        if let Ok(mut e) = commands.get_entity(pickup) {
            e.despawn();
        }
    }
    let (goldy_side, chomper_side) = scene_spots(Vec2::ZERO, facing);
    let (goldy_at, chomper_at) = (game.camera_look + goldy_side * UNIT, game.camera_look + chomper_side * UNIT);
    // 0xE78B20 twice: every foreground cell whose sphere meets the query sphere
    // goes (0xE780A0), the avatar stays. ADAPTED: the sphere radius of a cell is
    // its current world radius.
    for (entity, t, o) in &others {
        if o.background {
            continue;
        }
        let at = Vec2::new(t.translation.x, t.translation.z);
        let hit = |spot: Vec2, radius: f32| at.distance(spot) < o.radius + radius * UNIT;
        if hit(goldy_at, GOLDY_CLEAR_RADIUS) || hit(chomper_at, CHOMPER_CLEAR_RADIUS) {
            commands.entity(entity).despawn();
        }
    }
    for (name, position) in [(GOLDY_SPECIAL, goldy_at), (CHOMPER_SPECIAL, chomper_at)] {
        let cell = name_id(name);
        if db.cell(cell).is_none() {
            continue;
        }
        queue.0.push(SpawnRequest {
            cell,
            scale: object.scale,
            position,
            heading: heading_of(here - position),
            ..default()
        });
    }
    *link = PartGuideChase { pending: true, waited: 0 };
    // 0xE798E4: `ui+0xE0` = the Pokey, `ui+0xF4` = 10; the event record (0xE79920);
    // `cCellGame+0x518C` = 1 (spawns off). The cinematic starts with the flag
    // `part_cinematic_played` (`cinematics::moments`, string 0x1485A30).
    scene.start();
    game.camera_object = Some(goldy_at);
    game.camera_speed = POKEY_FOLLOW_SPEED;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarantee_follows_the_addfood_rule() {
        let mut save = CellSave::default();
        assert!(!due(&save, 119), "119 < food");
        assert!(due(&save, 120));
        assert!(due(&save, 150));
        assert!(!due(&save, 151), "food < 151");
        save.unlocked_parts[2] = 1;
        save.unlocked_parts[3] = 2;
        save.unlocked_parts[4] = 1;
        save.unlocked_parts[5] = 1;
        assert!(due(&save, 130), "four non-zero slots still count");
        save.unlocked_parts[6] = 1;
        assert!(!due(&save, 130), "more than four slots");
        save.unlocked_parts = [0; 13];
        save.difficulty = 2;
        assert!(!due(&save, 130), "never on hard");
        save.difficulty = 0;
        assert!(due(&save, 130));
        save.part_cinematic_played = true;
        assert!(!due(&save, 130), "once");
    }

    #[test]
    fn the_chomper_is_pointed_at_the_pokey_once_both_exist() {
        use crate::cell_ai::CellAiState;
        use crate::cell_data::{CellAi, CellScale};
        use crate::cell_engine::CellKind;
        use bevy::ecs::system::RunSystemOnce;
        let object = |cell: u32| CellObject {
            cell,
            kind: CellKind::Creature,
            scale: CellScale(1),
            abs_size: 1.0,
            radius: 1.0,
            size_vel: 0.0,
            hp: 3,
            density: 50.0,
            mass: 50.0,
            velocity: Vec2::ZERO,
            heading: 0.0,
            target: None,
            depth: 0.0,
            background: false,
            tile: None,
            age: 0.0,
            invulnerable: 0.0,
            passive: false,
            dead: false,
        };
        let mut world = World::new();
        world.insert_resource(PartGuideChase { pending: true, waited: 0 });
        let goldy = world.spawn(object(name_id(GOLDY_SPECIAL))).id();
        world.run_system_once(part_guide_chase).unwrap();
        assert!(world.resource::<PartGuideChase>().pending, "the chomper is not there yet");
        let chomper_cell = name_id(CHOMPER_SPECIAL);
        let chomper = world.spawn((object(chomper_cell), CellAiState::new(chomper_cell, &CellAi::default()))).id();
        world.run_system_once(part_guide_chase).unwrap();
        let ai = world.get::<CellAiState>(chomper).unwrap();
        assert_eq!(ai.chase_target, Some(goldy));
        assert_eq!(ai.chase_time, CHOMPER_CHASE_SECONDS);
        assert!(!world.resource::<PartGuideChase>().pending);
    }

    #[test]
    fn scene_spots_follow_the_retail_geometry() {
        // 0xE577A0: +x side when the facing x is positive, else -x; 15 and 25 out.
        let look = Vec2::new(2.0, -3.0);
        let (goldy, chomper) = scene_spots(look, Vec2::new(0.3, -0.9));
        assert_eq!(goldy, Vec2::new(17.0, -3.0));
        assert_eq!(chomper, Vec2::new(27.0, -3.0));
        let (goldy, chomper) = scene_spots(look, Vec2::new(-0.3, -0.9));
        assert_eq!(goldy, Vec2::new(-13.0, -3.0));
        assert_eq!(chomper, Vec2::new(-23.0, -3.0));
        // Facing exactly along y takes the -x side (the comparison is `> 0`).
        assert_eq!(scene_spots(look, Vec2::new(0.0, -1.0)).0, Vec2::new(-13.0, -3.0));
        assert!(GOLDY_CLEAR_RADIUS < CHOMPER_CLEAR_RADIUS);
    }

    #[test]
    fn add_food_marks_the_guarantee_before_the_meal() {
        let mut run = RunProgress::default();
        run.cell.food_progression = 118;
        crate::cell_rules::add_food(&mut run, 10, true);
        assert!(!run.cell.part_guide_pending, "118 is before the window");
        assert_eq!(run.cell.food_progression, 128);
        crate::cell_rules::add_food(&mut run, 10, true);
        assert!(run.cell.part_guide_pending, "128 is inside the window");
        // Past the window the flag is only set by earlier meals.
        run.cell.part_guide_pending = false;
        run.cell.food_progression = 151;
        crate::cell_rules::add_food(&mut run, 10, true);
        assert!(!run.cell.part_guide_pending);
    }

    fn plain_cell(cell: u32) -> CellObject {
        use crate::cell_data::CellScale;
        use crate::cell_engine::CellKind;
        CellObject {
            cell,
            kind: CellKind::Creature,
            scale: CellScale(1),
            abs_size: 1.0,
            radius: 1.0,
            size_vel: 0.0,
            hp: 3,
            density: 50.0,
            mass: 50.0,
            velocity: Vec2::ZERO,
            heading: 0.0,
            target: None,
            depth: 0.0,
            background: false,
            tile: None,
            age: 0.0,
            invulnerable: 0.0,
            passive: false,
            dead: false,
        }
    }

    #[test]
    fn the_event_record_follows_0xe5f160_and_0xe7e6c0() {
        let mut scene = PartGuideScene::default();
        assert!(!scene.active);
        scene.start();
        assert!(scene.active && scene.time == 15.0);
        // The 15 s run down by dt; with the Pokey gone no more than 8 s remain.
        scene.tick(1.0, false);
        assert_eq!(scene.time, 14.0);
        scene.tick(0.5, true);
        assert_eq!(scene.time, 8.0, "0xE5F184 caps the time left");
        scene.tick(3.0, true);
        assert_eq!(scene.time, 5.0, "below the cap it just runs down");
        // The retail event is not ended by its timer; the cinematic's end ends it. With no
        // cinematic ever seen the timer is the way out (the fallback of `over`).
        assert!(!scene.over(true));
        assert!(!scene.over(false), "timer left, no cinematic seen");
        scene.time = 0.0;
        assert!(scene.over(false));
        scene.saw_cinematic = true;
        assert!(!scene.over(true), "a running cinematic holds the event past its timer");
        assert!(scene.over(false), "and its end ends the event");
        // 0xE5F010: a step of 4.5 toward B, only when B is farther than that.
        let a = Vec2::new(1.0, 1.0);
        assert_eq!(nudge(a, Vec2::new(4.0, 1.0), END_NUDGE), a, "3 units away: stays");
        let moved = nudge(a, Vec2::new(11.0, 1.0), END_NUDGE);
        assert!((moved - Vec2::new(5.5, 1.0)).length() < 1e-4, "{moved:?}");
        assert!(CHOMPER_DRIFT < END_NUDGE && POKEY_FOLLOW_SPEED > PICKUP_FOLLOW_SPEED);
    }

    #[test]
    fn the_scene_runs_the_camera_the_chomper_and_its_end() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.insert_resource(ButtonInput::<KeyCode>::default());
        let mut game = CellGame::default();
        game.camera_look = Vec2::new(2.0, 0.0);
        world.insert_resource(game);
        let mut scene = PartGuideScene::default();
        scene.start();
        world.insert_resource(scene);
        let avatar = world.spawn((Transform::from_xyz(0.0, 0.0, 0.0), Player)).id();
        let goldy_at = Vec2::new(17.0, 0.0) * UNIT;
        let chomper_at = Vec2::new(27.0, 0.0) * UNIT;
        let goldy = world.spawn((Transform::from_xyz(goldy_at.x, 0.0, goldy_at.y), plain_cell(name_id(GOLDY_SPECIAL)))).id();
        let chomper = world.spawn((Transform::from_xyz(chomper_at.x, 0.0, chomper_at.y), plain_cell(name_id(CHOMPER_SPECIAL)))).id();
        let step = |world: &mut World| {
            world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
            world.run_system_once(part_guide_scene).unwrap();
        };
        // 0xE798E4: the two cells are found and protected (+0x354); the camera looks at the Pokey.
        step(&mut world);
        let game = world.resource::<CellGame>();
        assert_eq!(game.protected, vec![goldy, chomper]);
        assert!(game.locked, "the avatar is locked");
        assert_eq!(game.camera_object, Some(goldy_at));
        // The part pickup exists (the Pokey died): the camera looks at it, speed 5, offset target 10.
        let pickup_at = Vec2::new(20.0, 3.0) * UNIT;
        let pickup = world.spawn((Transform::from_xyz(pickup_at.x, 0.0, pickup_at.y), plain_cell(1))).id();
        world.resource_mut::<CellGame>().unlock_pickup = Some(pickup);
        world.entity_mut(goldy).despawn();
        step(&mut world);
        let game = world.resource::<CellGame>();
        assert_eq!(game.camera_object, Some(pickup_at));
        assert_eq!((game.camera_speed, game.offset_z_target), (PICKUP_FOLLOW_SPEED, PICKUP_OFFSET_Z));
        assert_eq!(world.resource::<PartGuideScene>().time, GONE_SECONDS, "the Pokey is gone: 8 s at most");
        // The chomper drifts away from the pickup at 2 units per second (0.1 s here) and is aimed beyond it.
        let t = world.get::<Transform>(chomper).unwrap();
        let moved = Vec2::new(t.translation.x, t.translation.z).distance(chomper_at);
        assert!((moved - CHOMPER_DRIFT * 0.1 * UNIT).abs() < 1e-3, "{moved}");
        assert!(world.get::<CellObject>(chomper).unwrap().target.is_some());
        // The natural end (the timer, with no cinematic in this world): the pickup stays, the avatar steps 4.5
        // toward it, the camera returns to the avatar at speed 5, the spawns are free and the lock is off.
        world.resource_mut::<PartGuideScene>().time = 0.0;
        step(&mut world);
        let game = world.resource::<CellGame>();
        assert!(!world.resource::<PartGuideScene>().active);
        assert!(game.camera_focus_avatar && game.camera_object.is_none());
        assert_eq!((game.camera_speed, game.offset_z_target), (PICKUP_FOLLOW_SPEED, crate::cell_engine::CAMERA_OFFSET_Z));
        assert!(game.protected.is_empty() && !game.locked);
        let at = world.get::<Transform>(avatar).unwrap();
        let expect = nudge(Vec2::ZERO, pickup_at / UNIT, END_NUDGE) * UNIT;
        assert!((Vec2::new(at.translation.x, at.translation.z) - expect).length() < 1e-3);
    }

    #[test]
    fn skipping_cuts_the_camera_back_and_brings_the_pickup() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Escape);
        world.insert_resource(keys);
        let mut game = CellGame::default();
        game.camera_look = Vec2::new(14.0, 0.0) * UNIT;
        game.camera_lead = game.camera_look;
        game.camera_focus = game.camera_look;
        game.camera_object = Some(game.camera_look);
        game.offset_z_target = PICKUP_OFFSET_Z;
        let pickup_at = Vec2::new(14.0, 0.0) * UNIT;
        world.insert_resource(game);
        let mut scene = PartGuideScene::default();
        scene.start();
        world.insert_resource(scene);
        world.spawn((Transform::from_xyz(0.0, 0.0, 0.0), Player));
        let pickup = world.spawn((Transform::from_xyz(pickup_at.x, 0.0, pickup_at.y), plain_cell(1))).id();
        world.resource_mut::<CellGame>().unlock_pickup = Some(pickup);
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        world.run_system_once(part_guide_scene).unwrap();
        let game = world.resource::<CellGame>();
        // 0xE79611: the offset target back to 20, the look / lead / focus points on the avatar.
        assert_eq!(game.offset_z_target, crate::cell_engine::CAMERA_OFFSET_Z);
        assert_eq!((game.camera_look, game.camera_lead, game.camera_focus), (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO));
        assert!(!game.camera_focus_avatar, "the skip leaves the follow rules to the avatar");
        assert_eq!(game.camera_speed, crate::cell_engine::CAMERA_SPEED_DEFAULT);
        // The pickup came 4.5 toward the avatar (0xE5F010(pickup, avatar, 4.5)).
        let t = world.get::<Transform>(pickup).unwrap();
        assert!((t.translation.x - 9.5 * UNIT).abs() < 1e-3, "{}", t.translation.x);
    }
}
