//! Creature stage: herds answering an attack, the pack defending the avatar,
//! babies growing up and the sea monster (creature-rules3, notes C8).
//!
//! Retail sources (march2017 addresses, `notes\creature-re-notes.md` C8):
//! * 0xC075E0 (a creature was hit): up to the victim herd archetype's
//!   `attacked_MaxSpeciesToAlert` (+0x438) members of the SAME herd within
//!   `attacked_SpeciesHelpRadius` (+0x434, truncated to an integer) are flagged
//!   to help; every nearby creature also gets the attack event (0x200, 5 s) and
//!   an attacker memory (0x21, 15 s).
//! * 0xD8F1C0 (pick a target from that event): an unflagged responder only
//!   answers when the attacker is within `creatureHelpAllyInRadius` (10) of it
//!   and it is not too hurt; this is how the avatar's posse defends it.
//! * 0xC04F30 / 0xC04FB0 (`SetAge(1)` on a baby, then the growth state at
//!   +0x1688): effect 0x3A616FEE, the scale grows by `growthRate` (0.9) per
//!   second to the midpoint between baby and adult scale, the creature becomes
//!   an adult (age 1, HP / abilities refreshed), then grows on to the adult scale.
//! * cSeaMonsterEvent (factory 0xD4D450, condition 0xD4C5F0, strike 0xD4C580):
//!   checked every `CheckTime` (2 s); when no land lies within
//!   `creatureMaxDistanceFromShore` (40) of the avatar, it kills the avatar with
//!   cause 9 (`CRG_AvatarDied_SeaMonster`, 0xD38E60).

use bevy::prelude::*;

use crate::creature_ecology::{Archetype, HerdMember, MemberRole};

/// `creatureHelpAllyInRadius` (CreatureGameTuning, global 0x1582EBC, read in
/// 0xD8F1C0).
pub const HELP_ALLY_IN_RADIUS: f32 = 10.0;
/// Seconds an attack keeps the responders on the attacker: 0xC075E0 posts the
/// attacker memory event 0x21 with 15.0 (INFERRED: its duration).
pub const ALERT_MEMORY_SECONDS: f32 = 15.0;
/// `growthRate` (speciestuning, global 0x150C8D4): scale per second (0xC04FB0).
pub const GROWTH_RATE: f32 = 0.9;
/// `CheckTime` of `creature_events~!SeaMonster` (event template, name only).
pub const SEA_MONSTER_CHECK_TIME: f32 = 2.0;
/// `creatureMaxDistanceFromShore` of `creature_events~!SeaMonster` (0xD4C5F0).
pub const MAX_DISTANCE_FROM_SHORE: f32 = 40.0;

/// Pure: the members of the victim's herd that answer an attack on it
/// (0xC075E0). `candidates` are the other living members of the same herd
/// (id, chart position); at most `attacked_MaxSpeciesToAlert` of them within
/// the integer part of `attacked_SpeciesHelpRadius`. ADAPTED: nearest first
/// (retail walks the victim's perception list in its own order).
pub fn herd_helpers(archetype: Archetype, victim: Vec2, candidates: &[(u32, Vec2)]) -> Vec<u32> {
    let record = archetype.record();
    let radius = record.help_radius.trunc();
    let cap = record.max_species_to_alert as usize;
    if cap == 0 {
        return Vec::new();
    }
    let mut near: Vec<(u32, f32)> = candidates
        .iter()
        .map(|(id, p)| (*id, p.distance(victim)))
        .filter(|(_, d)| d.is_finite() && *d <= radius)
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    near.into_iter().take(cap).map(|(id, _)| id).collect()
}

/// Pure: the pack members that defend the avatar against `attacker`
/// (0xD8F1C0): those within `creatureHelpAllyInRadius` of the attacker.
pub fn defenders(attacker: Vec2, pack: &[(u32, Vec2)]) -> Vec<u32> {
    pack.iter()
        .filter(|(_, p)| p.distance(attacker) < HELP_ALLY_IN_RADIUS)
        .map(|(id, _)| *id)
        .collect()
}

/// A baby growing up (0xC04FB0): `adult_at` is the midpoint scale where it
/// becomes an adult, `target` the adult scale.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BabyGrowth {
    pub adult_at: f32,
    pub target: f32,
    /// Phase 2 (growing to the midpoint, still a baby) or 3 (adult, growing on).
    pub phase: u8,
}

impl BabyGrowth {
    /// State 1: from the baby scale toward the adult one.
    pub fn start(baby: f32, adult: f32) -> Self {
        Self {
            adult_at: (adult - baby) * 0.5 + baby,
            target: adult,
            phase: 2,
        }
    }
}

/// Pure: one growth step (states 2 and 3): `scale + rate * dt`, capped at
/// `goal`. `rate` is `growthRate` in the actor's units (see `grow_babies`).
pub fn grow_step(scale: f32, goal: f32, rate: f32, dt: f32) -> f32 {
    (scale + rate * dt.max(0.0)).min(goal)
}

/// Pure: the baby timer (seconds lived as a baby) has passed
/// `creatureNPCMinBabyTime` (180 s). INFERRED trigger: the per-frame update
/// 0xC05970 runs a clock (+0x1628) against that tuning; the `SetAge(1)` call
/// that follows was not traced.
pub fn ready_to_grow(baby_age: f32) -> bool {
    baby_age >= crate::creature_ecology::NPC_MIN_BABY_TIME
}

/// Babies of retail herds age while their herd is active; after
/// `creatureNPCMinBabyTime` they grow up (0xC04FB0): the scale grows at
/// `growthRate`, the creature becomes an adult at the midpoint (role, max HP,
/// saved) and grows on to the adult scale.
pub fn grow_babies(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<crate::game::RunProgress>,
    mut babies: Query<(
        Entity,
        &crate::persistence::WorldId,
        &mut Transform,
        &mut crate::game::Npc,
        &mut HerdMember,
        Option<&mut BabyGrowth>,
    )>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let mut changed = false;
    for (entity, id, mut t, mut npc, mut herd, growth) in &mut babies {
        if npc.hp <= 0.0 {
            continue;
        }
        let Some(mut growth) = growth else {
            if herd.role != MemberRole::Baby {
                continue;
            }
            let Some(member) = run.species_world.member_mut(id.0) else {
                continue;
            };
            member.baby_age += dt;
            if !ready_to_grow(member.baby_age) {
                continue;
            }
            let roll = crate::creature_ecology::unit(id.0 as u64 ^ 0x5CA1E);
            let size = run.species_world.species(herd.species).map_or(1.0, |s| s.size);
            let adult = crate::creature_ecology::member_scale(size, MemberRole::Adult, roll);
            commands.entity(entity).insert(BabyGrowth::start(t.scale.x, adult));
            continue;
        };
        // ADAPTED: retail scales are near 1, so `growthRate` is applied as a
        // fraction of the adult scale per second (0.6 -> 1.0 in about 0.45 s).
        let rate = GROWTH_RATE * growth.target;
        let goal = if growth.phase == 2 { growth.adult_at } else { growth.target };
        let scale = grow_step(t.scale.x, goal, rate, dt);
        t.scale = Vec3::splat(scale);
        if scale < goal {
            continue;
        }
        if growth.phase == 2 {
            // The creature is an adult now (age 1): max HP refreshed, the HP
            // fraction kept (ADAPTED: 0xC0D500's exact HP rule not traced).
            let baby_max = crate::creature_ecology::member_max_hp(herd.archetype, MemberRole::Baby);
            let adult_max = crate::creature_ecology::member_max_hp(herd.archetype, MemberRole::Adult);
            if baby_max > 0.0 {
                npc.hp = (npc.hp / baby_max * adult_max).min(adult_max);
            }
            herd.role = MemberRole::Adult;
            if let Some(member) = run.species_world.member_mut(id.0) {
                member.role = MemberRole::Adult;
                member.baby_age = 0.0;
            }
            growth.phase = 3;
            changed = true;
        } else {
            commands.entity(entity).remove::<BabyGrowth>();
        }
    }
    if changed {
        run.request_save();
    }
}

/// Pure: the sea monster takes the avatar at chart `p` (0xD4C5F0): no land
/// within `creatureMaxDistanceFromShore`. ADAPTED: land is sampled on three
/// rings of twelve points (`Land::water_within`'s pattern) instead of
/// retail's planet query 0xB888C0.
pub fn sea_monster_strikes(land: &crate::creature_planet::Land, p: Vec2) -> bool {
    if !land.on_planet() || land.is_land(p) {
        return false;
    }
    for ring in [0.25, 0.5, 0.75, 1.0] {
        for k in 0..12 {
            let a = k as f32 / 12.0 * std::f32::consts::TAU + ring;
            if land.is_land(p + Vec2::new(a.cos(), a.sin()) * MAX_DISTANCE_FROM_SHORE * ring) {
                return false;
            }
        }
    }
    true
}

/// TLSA groups `gen_swim` (looping stroke) and `gen_swim_inplace` (treading
/// water), from `notes/creator-re/tlsa_groups.txt`.
pub const SWIM: u32 = 0x0293_BB70;
pub const SWIM_IN_PLACE: u32 = 0x02E6_EA7D;
/// ADAPTED: how far the swimming body sits below its walking height, as a
/// fraction of its body clearance (retail swim height not traced).
pub const SWIM_SINK: f32 = 0.6;

static SWIMMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Smoothed standing height of the avatar's drawn body (f32 bits; 0 = not
/// measured yet), from `swim_float`. The Creature camera frames the avatar by
/// it (0xD27300: the locomotive's bounding box height).
static BODY_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The avatar's measured body height (world units), when it is drawn.
pub fn body_height() -> Option<f32> {
    let h = f32::from_bits(BODY_HEIGHT.load(std::sync::atomic::Ordering::Relaxed));
    (h.is_finite() && h > 0.05).then_some(h)
}

/// The avatar is swimming (its ground is under water: `IsInWater`). VERIFIED
/// (0xB445C0 mode 0, creature-audit-a): in water = |position|^2 <= waterR^2
/// with the feet at max(ground, waterR - 0.55 x body height), i.e. the ground
/// is below the water surface, which is what `!Land::is_land` tests;
/// OnStartSwimming 0xC14F70 sets locomotion state 3.
pub fn swimming() -> bool {
    SWIMMING.load(std::sync::atomic::Ordering::Relaxed)
}

/// How deep a swimming creature floats (VERIFIED, planet correction 0xB445C0
/// case 0, notes C10B): its origin (feet) stays at least at the water surface
/// minus 0.55 x its bounding-box height (+0x14 - +0x8 of the locomotive's
/// box), never below the sea floor. The in-water flag (0xB422B0, flags
/// 0x1000) is set while the origin is at or below the water surface; it picks
/// `gen_swim` / `gen_swim_inplace` (0xC0C790).
pub const SWIM_DEPTH_OF_BODY: f32 = 0.55;

/// The feet height of a creature in water (0xB445C0 case 0): the deeper of
/// the sea floor and `water - 0.55 x body height` wins the max.
pub fn swim_feet_height(water: f32, floor: f32, body_height: f32) -> f32 {
    floor.max(water - SWIM_DEPTH_OF_BODY * body_height.max(0.0))
}

/// The avatar's drawn body: (height, origin above its lowest point), from the
/// world boxes of its meshes.
fn body_extent(
    root: Entity,
    origin_y: f32,
    children: &Query<&Children>,
    boxes: &Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
) -> Option<(f32, f32)> {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for e in children.iter_descendants(root) {
        let Ok((aabb, gt)) = boxes.get(e) else { continue };
        let m = gt.affine();
        let c = m.transform_point3a(aabb.center);
        let h = aabb.half_extents;
        let ey = m.matrix3.x_axis.y.abs() * h.x + m.matrix3.y_axis.y.abs() * h.y + m.matrix3.z_axis.y.abs() * h.z;
        low = low.min(c.y - ey);
        high = high.max(c.y + ey);
    }
    (high > low && high.is_finite()).then(|| (high - low, origin_y - low))
}

/// After grounding (the avatar stands on `ToSurface`, the water surface at
/// sea): mark swimming and float the body the retail way
/// (`swim_feet_height`), measured from its drawn box on land. Without a drawn
/// body (headless) it keeps the earlier ADAPTED sink.
#[allow(clippy::too_many_arguments)]
pub fn swim_float(
    surface: Option<Res<crate::terrain::LandSurface>>,
    mobility: Option<Res<crate::creature::mobility::CreatureMobility>>,
    mut player: Query<(Entity, &mut Transform, &GlobalTransform), With<crate::game::Player>>,
    children: Query<&Children>,
    boxes: Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
    mut body: Local<Option<(f32, f32)>>,
) {
    let Ok((entity, mut t, drawn)) = player.single_mut() else {
        SWIMMING.store(false, std::sync::atomic::Ordering::Relaxed);
        return;
    };
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let airborne = mobility.is_some_and(|m| m.height > 0.0);
    let swim = land.on_planet() && !land.is_land(t.translation.xz()) && !airborne;
    // The drawn boxes are last frame's: measure against last frame's origin.
    let drawn_y = drawn.translation().y;
    SWIMMING.store(swim, std::sync::atomic::Ordering::Relaxed);
    if !swim {
        // On land: remember the standing body (its bind box with the walk;
        // swim strokes would distort it).
        if !airborne {
            if let Some((height, above)) = body_extent(entity, drawn_y, &children, &boxes) {
                *body = Some(match *body {
                    Some((h, a)) => (h + (height - h) * 0.1, a + (above - a) * 0.1),
                    None => (height, above),
                });
                if let Some((h, _)) = *body {
                    BODY_HEIGHT.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        return;
    }
    let (Some(surface), Some((height, above))) = (surface.as_deref(), *body) else {
        t.translation.y -= SWIM_SINK * 0.7 * t.scale.x;
        return;
    };
    let (x, z) = (t.translation.x, t.translation.z);
    let water = surface.height(x, z);
    let floor = surface.ground_height(x, z);
    // Grounding stood the feet on the water surface; lower them to the
    // retail float height.
    let feet = t.translation.y - above;
    let target = swim_feet_height(water, floor, height);
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        info!("swim: body height {height:.2}, origin {above:.2} above the feet; water {water:.2}, floor {floor:.2}, feet {feet:.2} -> {target:.2}");
    }
    t.translation.y -= (feet - target).max(0.0);
}

/// Dev hook for captures: `OPENSPORE_SWIM_TEST=shore` puts the avatar a few
/// units into the nearest sea at frame 20 of the stage, `=far` beyond
/// `creatureMaxDistanceFromShore` (the sea monster takes it 2 s later).
pub fn swim_test(
    mut frames: Local<u32>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut player: Query<&mut Transform, With<crate::game::Player>>,
) {
    static MODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let Some(mode) = MODE.get_or_init(|| std::env::var("OPENSPORE_SWIM_TEST").ok()) else {
        return;
    };
    *frames += 1;
    if *frames != 20 {
        return;
    }
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    let Ok(mut t) = player.single_mut() else {
        return;
    };
    if let Some(p) = sea_point(&land, t.translation.xz(), mode == "far") {
        info!("swim test: avatar to {p} ({mode})");
        t.translation.x = p.x;
        t.translation.z = p.y;
    }
}

/// The nearest sea point on 16 rays from `from` (5-unit steps): 4 units past
/// the shore, or with `far` the first point the sea monster takes.
pub fn sea_point(land: &crate::creature_planet::Land, from: Vec2, far: bool) -> Option<Vec2> {
    let mut best: Option<(f32, Vec2)> = None;
    for k in 0..16 {
        let a = k as f32 / 16.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        let Some(shore) = (1..=120).map(|i| i as f32 * 5.0).find(|d| !land.is_land(from + dir * *d)) else {
            continue;
        };
        let point = if far {
            match (0..=60)
                .map(|i| shore + i as f32 * 5.0)
                .find(|d| sea_monster_strikes(land, from + dir * *d))
            {
                Some(d) => d,
                None => continue,
            }
        } else {
            shore + 4.0
        };
        if best.is_none_or(|(d, _)| point < d) {
            best = Some((point, from + dir * point));
        }
    }
    best.map(|(_, p)| p)
}

/// Pure: a guard's next post and how long it holds it: one of the herd's
/// `NumGuardLocations` evenly spaced around the nest at
/// `guardLocationRadiusRange`, held for `guardPositionDurationRange`.
/// ADAPTED: a random post each time (retail post assignment, the per-post cap
/// and `guardLocationAreaBuffer` are not traced).
pub fn guard_post(archetype: Archetype, home: Vec2, nest: usize, rolls: [f32; 3]) -> (Vec2, f32) {
    let record = archetype.record();
    let locations = record.guard_locations.max(1);
    let post = ((rolls[0].clamp(0.0, 0.999) * locations as f32) as u32).min(locations - 1);
    let phase = crate::creature_ecology::unit(nest as u64 ^ 0x6A4D) * std::f32::consts::TAU;
    let angle = phase + post as f32 / locations as f32 * std::f32::consts::TAU;
    let (lo, hi) = record.guard_radius;
    let radius = (lo + (hi - lo) * rolls[1].clamp(0.0, 1.0)) * crate::creature_ecology::CREATURE_WORLD_SCALE;
    let (wlo, whi) = record.guard_duration;
    let wait = wlo + (whi - wlo) * rolls[2].clamp(0.0, 1.0);
    (home + Vec2::new(angle.cos(), angle.sin()) * radius, wait)
}

static SEA_MONSTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The avatar's death was the sea monster's (cause 9); clears the mark.
pub fn take_sea_monster() -> bool {
    SEA_MONSTER.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// cSeaMonsterEvent: every `CheckTime` the avatar far from any shore is taken
/// (death cause 9 -> `CRG_AvatarDied_SeaMonster`). Only an avatar standing in
/// water can be taken; see CHANGES.md (swimming is not implemented yet).
pub fn sea_monster(
    time: Res<Time>,
    mut clock: Local<f32>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut player: Query<(&Transform, &mut crate::game::Health), With<crate::game::Player>>,
) {
    *clock += time.delta_secs();
    if *clock < SEA_MONSTER_CHECK_TIME {
        return;
    }
    *clock = 0.0;
    let Ok((t, mut health)) = player.single_mut() else {
        return;
    };
    let land = crate::creature_planet::Land::from_surface(surface.as_deref());
    if !health.dead() && sea_monster_strikes(&land, t.translation.xz()) {
        health.hp = 0.0;
        SEA_MONSTER.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature_ecology::Archetype;

    #[test]
    fn herd_helpers_follow_the_archetype_cap_and_radius() {
        // npc_medium_combat: 3 helpers within 25.
        let victim = Vec2::ZERO;
        let others = [
            (1, Vec2::new(5.0, 0.0)),
            (2, Vec2::new(24.9, 0.0)),
            (3, Vec2::new(25.5, 0.0)),
            (4, Vec2::new(1.0, 1.0)),
            (5, Vec2::new(10.0, 0.0)),
        ];
        assert_eq!(herd_helpers(Archetype::MediumCombat, victim, &others), vec![4, 1, 5]);
        // npc_large_combat: 4 within 30 picks the far one too.
        assert_eq!(herd_helpers(Archetype::LargeCombat, victim, &others), vec![4, 1, 5, 2]);
        // npc_social: nobody answers (attacked_MaxSpeciesToAlert 0).
        assert!(herd_helpers(Archetype::Social, victim, &others).is_empty());
        // npc_small_combat: one helper within 15.
        assert_eq!(herd_helpers(Archetype::SmallCombat, victim, &others), vec![4]);
    }

    #[test]
    fn pack_defends_within_the_help_ally_radius() {
        let pack = [(7, Vec2::new(3.0, 4.0)), (8, Vec2::new(10.0, 0.0)), (9, Vec2::new(0.0, 9.9))];
        assert_eq!(defenders(Vec2::ZERO, &pack), vec![7, 9]);
    }

    #[test]
    fn babies_grow_in_two_phases_at_the_growth_rate() {
        assert!(!ready_to_grow(179.9));
        assert!(ready_to_grow(180.0));
        let g = BabyGrowth::start(0.6, 1.0);
        assert!((g.adult_at - 0.8).abs() < 1e-6);
        assert_eq!(g.phase, 2);
        // 0.9 per second, capped at the goal.
        assert!((grow_step(0.6, 0.8, 0.9, 0.1) - 0.69).abs() < 1e-6);
        assert_eq!(grow_step(0.79, 0.8, 0.9, 0.1), 0.8);
        let mut scale = 0.6;
        let mut seconds = 0.0;
        while scale < 1.0 {
            scale = grow_step(scale, 1.0, 0.9, 0.05);
            seconds += 0.05;
        }
        assert!(seconds < 0.5, "{seconds}");
    }

    #[test]
    fn guard_posts_ring_the_nest_and_hold_for_the_duration() {
        // npc_social: 4 posts at 5..10, held 10..30 s.
        let home = Vec2::new(3.0, 4.0);
        for k in 0..16 {
            let r = k as f32 / 16.0;
            let (p, wait) = guard_post(Archetype::Social, home, 2, [r, r, r]);
            let d = p.distance(home);
            assert!((5.0 - 1e-4..=10.0 + 1e-4).contains(&d), "{d}");
            assert!((10.0..=30.0).contains(&wait), "{wait}");
        }
        let posts: std::collections::HashSet<_> = (0..40)
            .map(|k| {
                let (p, _) = guard_post(Archetype::Social, home, 2, [k as f32 / 40.0, 0.0, 0.0]);
                ((p.x * 100.0).round() as i32, (p.y * 100.0).round() as i32)
            })
            .collect();
        assert_eq!(posts.len(), 4);
    }

    #[test]
    fn no_sea_monster_without_a_planet() {
        let land = crate::creature_planet::Land::flat();
        assert!(!sea_monster_strikes(&land, Vec2::new(1000.0, 1000.0)));
    }

    #[test]
    fn swimmers_float_with_most_of_the_body_under_water() {
        // 0xB445C0: feet at max(floor, water - 0.55 x height).
        assert!((swim_feet_height(10.0, 0.0, 2.0) - 8.9).abs() < 1e-5);
        // Wading: the floor is higher than the float height, so it walks on it.
        assert_eq!(swim_feet_height(10.0, 9.5, 2.0), 9.5);
        assert_eq!(swim_feet_height(10.0, 0.0, -1.0), 10.0);
    }
}
