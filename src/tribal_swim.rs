//! Tribe members wading and swimming (tribal-swim, notes `tribal-re-notes.md`
//! T9J).
//!
//! Retail: `cCreatureBase::WalkTo` 0xC1C720 sets a straight locomotion goal (no
//! path finding), and a creature that enters water starts swimming
//! (`OnStartSwimming` 0xC14F70 sets locomotion state 3 and, unless the species
//! profile flag +0x61C is set, restarts the default locomotion clip with 0xC148B0;
//! it is reached from 0xC04E30. It does not drop the held item: nothing in the
//! code reads drops or hides it, T9K-3). So tribe members walk straight to their target and
//! swim where that line crosses water. The Creature stage already does this for
//! its avatar (`creature_support::swim_float`, the `gen_swim` clips); this is
//! the Tribal side:
//!
//! * movement: `tribal_rts::step_toward_swim` over the swimmers' grid
//!   (`tribal_path::NavGrid::build_swim`, water open, cliffs ADAPTED);
//! * this module: [`Swim`] marks a member (or raider) in water, [`swim_units`]
//!   eases its body down to the swimming height after grounding (the water
//!   surface, retail `ToSurface`; ADAPTED sink, the same fraction as the
//!   avatar's), `outfit_crowd` plays the swim clips for it;
//! * the dev hook `OPENSPORE_TRIBE_SWIM_TEST=1` ([`swim_test`]).
//!
//! Not applied (no retail evidence): the sea monster (`cSeaMonsterEvent`
//! 0xD4D450 needs the Creature avatar, its cinematic is `CRG_`), a swim speed
//! (no property is read: the walking speed is kept, ADAPTED), a wading state
//! (no depth tuning exists in the traced code).
use bevy::prelude::*;

use crate::tribal_planet::{self, TribalLayout};
use crate::tribal_rts::{RivalMember, TribeUnit, UnitTask};

/// ADAPTED: seconds the body takes to sink to the swimming height on entering
/// water (and to rise on leaving it); retail's swim transition is the clip's.
pub const SINK_SECONDS: f32 = 0.4;

/// A member (or raider) over water. Inserted on entering water, removed once
/// it is out and risen; `outfit_crowd` plays the swim clips while [`Self::wet`].
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Swim {
    wet: bool,
    /// 0 (standing) .. 1 (swimming height).
    depth: f32,
}

impl Swim {
    /// Over water now.
    pub fn wet(&self) -> bool {
        self.wet
    }
}

/// How far a body of `scale` rides below its standing height at `depth`
/// (0..1): the Creature avatar's ADAPTED sink (`creature_support`).
pub fn sink(depth: f32, scale: f32) -> f32 {
    let d = depth.clamp(0.0, 1.0);
    crate::creature_support::SWIM_SINK * 0.7 * scale * d * d * (3.0 - 2.0 * d)
}

/// Mark members and raiders over water and let them float at the surface.
/// Runs after grounding (`terrain::ground` stands them on `ToSurface`, the
/// water surface at sea, every frame) with the same run conditions.
#[allow(clippy::type_complexity)]
pub fn swim_units(
    mut commands: Commands,
    time: Res<Time>,
    stage_layout: Option<Res<tribal_planet::StageLayout>>,
    mut units: Query<(Entity, &mut Transform, Option<&mut Swim>), Or<(With<TribeUnit>, With<RivalMember>)>>,
) {
    let _layout = tribal_planet::scope(stage_layout.as_deref());
    let layout = tribal_planet::current();
    let dt = time.delta_secs().clamp(0.0, 0.1);
    for (entity, mut t, swim) in &mut units {
        let wet = layout.in_water(t.translation.xz());
        match swim {
            Some(mut s) => {
                s.wet = wet;
                let target = if wet { 1.0 } else { 0.0 };
                let step = dt / SINK_SECONDS;
                s.depth = if s.depth < target { (s.depth + step).min(target) } else { (s.depth - step).max(target) };
                if !wet && s.depth <= 0.0 {
                    commands.entity(entity).remove::<Swim>();
                    continue;
                }
                t.translation.y -= sink(s.depth, t.scale.x);
            }
            None if wet => {
                commands.entity(entity).insert(Swim { wet: true, depth: 0.0 });
            }
            None => {}
        }
    }
}

/// A water crossing from the hut: the last land point `a` before the water and
/// the first land point `b` after it on one bearing, with the water between
/// them at least `min_width` wide (the narrowest such crossing). Used by the
/// capture hook and the tests.
pub fn find_crossing(layout: &TribalLayout, min_width: f32) -> Option<(Vec2, Vec2)> {
    let land = &layout.land;
    if !layout.on_planet() {
        return None;
    }
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    for k in 0..72 {
        let angle = k as f32 / 72.0 * std::f32::consts::TAU;
        let dir = Vec2::new(angle.cos(), angle.sin());
        let mut shore: Option<f32> = None;
        let mut r = 4.0f32;
        while r < tribal_planet::CHART_LIMIT - 20.0 {
            let on_land = land.is_land(dir * r);
            match (shore, on_land) {
                (None, false) => shore = Some(r - 1.5),
                (Some(_), true) if !(1..=4).all(|k| land.is_land(dir * (r + 1.5 * k as f32))) => {
                    // A sliver of land in the water, not the far shore.
                }
                (Some(a), true) => {
                    let width = r - a;
                    if width >= min_width && best.is_none_or(|(w, ..)| width < w) {
                        best = Some((width, dir * a, dir * (r + 2.0)));
                    }
                    break;
                }
                _ => {}
            }
            r += 1.5;
        }
    }
    best.map(|(_, a, b)| (a, b))
}

/// Dev hook for captures: `OPENSPORE_TRIBE_SWIM_TEST=1` puts every member on
/// the shore before the narrowest lake or bay near the hut at frame 20 of the
/// stage and orders it to the far shore, the camera following the chieftain.
/// (`=shore` leaves the members at the hut, so the order walks them out;
/// `=low` also lowers the camera to a shallow pitch.)
pub fn swim_test(
    mut frames: Local<u32>,
    stage_layout: Option<Res<tribal_planet::StageLayout>>,
    controls: Option<ResMut<crate::tribal_controls::TribalControls>>,
    camera: Option<ResMut<crate::tribal_controls::TribalCamera>>,
    mut units: Query<(Entity, &mut Transform, &mut UnitTask, &TribeUnit, Option<&Swim>), Without<RivalMember>>,
) {
    static MODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let Some(mode) = MODE.get_or_init(|| std::env::var("OPENSPORE_TRIBE_SWIM_TEST").ok()) else {
        return;
    };
    *frames += 1;
    if *frames > 20 && *frames % 10 == 0 && *frames <= 400 {
        // Progress log: the chieftain's chart position, water and swim state.
        let layout = tribal_planet::current();
        if let Some((_, t, task, _, swim)) = units.iter().find(|u| u.3.chieftain) {
            info!(
                "tribal swim test: frame {} chieftain at ({:.1}, {:.1}) y {:.2} water {} swim {:?} task {}",
                *frames,
                t.translation.x,
                t.translation.z,
                t.translation.y,
                layout.in_water(t.translation.xz()),
                swim.map(|s| (s.wet, s.depth)),
                task.label()
            );
        }
    }
    if *frames != 20 {
        return;
    }
    let _layout = tribal_planet::scope(stage_layout.as_deref());
    let layout = tribal_planet::current();
    let Some((a, b)) = find_crossing(&layout, 14.0) else {
        info!("tribal swim test: no water crossing near the hut");
        return;
    };
    info!("tribal swim test: {a} -> {b} ({:.0} units, water between)", a.distance(b));
    let toward = (b - a).normalize_or(Vec2::X);
    let side = Vec2::new(-toward.y, toward.x);
    let mut chieftain = None;
    for (i, (entity, mut t, mut task, unit, _)) in units.iter_mut().enumerate() {
        if mode != "shore" {
            let p = a - toward * (4.0 + 1.2 * (i / 3) as f32) + side * (1.6 * ((i % 3) as f32 - 1.0));
            t.translation.x = p.x;
            t.translation.z = p.y;
        }
        *task = UnitTask::MoveTo(Vec3::new(b.x, 0.0, b.y));
        if unit.chieftain {
            chieftain = Some(entity);
        }
    }
    if let (Some(mut controls), Some(entity)) = (controls, chieftain) {
        controls.follow = Some(entity);
    }
    if let Some(mut camera) = camera.filter(|_| mode == "low") {
        // PageUp for 0.6 s: the elevation falls 30 degrees (retail PageUp lowers
        // the camera; the earlier rig raised it, so this was `tilt_keys(-1, 1)`).
        camera.rig.tilt_keys(1.0, 0.6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_sink_eases_in_and_scales_with_the_body() {
        assert_eq!(sink(0.0, 1.0), 0.0);
        assert!((sink(1.0, 1.0) - crate::creature_support::SWIM_SINK * 0.7).abs() < 1e-6);
        assert!(sink(0.5, 1.0) > 0.0 && sink(0.5, 1.0) < sink(1.0, 1.0));
        assert!((sink(1.0, 2.0) - 2.0 * sink(1.0, 1.0)).abs() < 1e-6);
        assert_eq!(sink(2.0, 1.0), sink(1.0, 1.0));
    }

    /// Without a planet (headless, flat layout) there is no water: nobody swims.
    #[test]
    fn nobody_swims_on_the_flat_layout() {
        let layout = tribal_planet::flat_layout(1);
        assert!(!layout.in_water(Vec2::new(500.0, 500.0)));
        assert!(find_crossing(&layout, 1.0).is_none());
    }

    #[test]
    fn a_member_in_water_is_marked_sinks_and_surfaces_again() {
        let layout = std::sync::Arc::new(crate::tribal_planet::tests::lake_layout());
        let (a, b) = find_crossing(&layout, 14.0).expect("the test planet has a lake near the hut");
        let mid = a.lerp(b, 0.5);
        assert!(layout.in_water(mid) && !layout.in_water(a - (b - a).normalize() * 3.0));
        let mut app = App::new();
        app.init_resource::<Time>()
            .insert_resource(tribal_planet::StageLayout(layout.clone()))
            .add_systems(Update, swim_units);
        let unit = app.world_mut().spawn((TribeUnit::new(false, 5.0), Transform::from_xyz(mid.x, 0.0, mid.y))).id();
        let dry = app.world_mut().spawn((TribeUnit::new(false, 5.0), Transform::from_xyz(a.x * 0.2, 0.0, a.y * 0.2))).id();
        let step = |app: &mut App| {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(100));
            app.update();
        };
        for _ in 0..8 {
            // Grounding stands the member on the surface (y = 0 here) each frame.
            app.world_mut().get_mut::<Transform>(unit).unwrap().translation.y = 0.0;
            step(&mut app);
        }
        let swim = app.world().get::<Swim>(unit).expect("a member in water swims");
        assert!(swim.wet());
        let y = app.world().get::<Transform>(unit).unwrap().translation.y;
        assert!((y + sink(1.0, 1.0)).abs() < 1e-4, "afloat at the swimming height: {y}");
        assert!(app.world().get::<Swim>(dry).is_none(), "a member on land does not swim");
        assert_eq!(app.world().get::<Transform>(dry).unwrap().translation.y, 0.0);
        // Out of the water it rises and the marker goes.
        let out = b;
        assert!(!layout.in_water(out), "the far shore {out} is land (crossing {a} -> {b})");
        {
            let mut t = app.world_mut().get_mut::<Transform>(unit).unwrap();
            t.translation = Vec3::new(out.x, 0.0, out.y);
        }
        for _ in 0..8 {
            app.world_mut().get_mut::<Transform>(unit).unwrap().translation.y = 0.0;
            step(&mut app);
        }
        assert!(app.world().get::<Swim>(unit).is_none(), "on the far shore it stands again");
    }
}
