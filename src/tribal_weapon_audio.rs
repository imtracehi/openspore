//! Thrown-spear impact sounds of the Tribal stage (audit gap G6).
//!
//! RETAIL (code image, march2017): the update of the thrown spear object
//! (vtable 0x145EAB4, function 0xC91E30) moves it toward its target and, when
//! the flight ends, plays one of two procedural sounds at the spear's position
//! (`Audio::PlayProceduralAudio` 0x571F80, 3D, at x / y / z):
//! * it struck a creature: `sfx_trg_com_sp_hi` (0xAA6D7524, pushed at 0xC92A9F),
//! * it landed without a hit: `sfx_trg_com_sp_throwmiss` (0x3516E5D7, 0xC922C4).
//! The two names belong to the thrown spear, not to a tool table: they are the
//! impact of the Throwing Spears ability (`tribeabilities` ability 43, whose
//! attack clip is `spear_throw`). The other weapons' sounds (swing, hit) are
//! events of their attack clips (`torch_poke`, `axe_attack`, `spear_stab`, ...),
//! which `anim_audio` already plays; the audit's reading that these two names
//! pick a weapon by the attacker's tool was an inference that the code does not
//! bear out.
//!
//! OpenSpore has no spear object: the Throwing Spears attack is a clip and the
//! damage a rate (`tribal_rules`). ADAPTED: when a member (the player's or a
//! rival's) starts the `spear_throw` clip, the landing is [`FLIGHT`] seconds
//! later, [`attack_range`](crate::tribal_tools::attack_range) in front of the
//! thrower; it is a hit when a living creature stands within [`HIT_RADIUS`] of
//! that spot at that time, a miss otherwise. The sound is positional, heard
//! from the camera.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    cell_audio::{NamedSounds, Play},
    game::Health,
    outfit_crowd::{tribe_clip, CrowdActor},
    stages::GameStage,
    tribal_tools::{attack_range, ToolKind},
};

/// The spear struck a creature (0xC92A9F).
pub const SPEAR_HIT: &str = "sfx_trg_com_sp_hi";
/// The spear landed without a hit (0xC922C4).
pub const SPEAR_MISS: &str = "sfx_trg_com_sp_throwmiss";
/// Seconds from the start of the `spear_throw` clip to the landing: the spear
/// ability's (`tribe_hunt3`) range 25 over its projectile speed 35 (VERIFIED
/// props `creatureAbilityRange` / `creatureAbilitySpeed`, `tribe_abilities_table.txt`),
/// the landing being the end of that range. ADAPTED: the launch is taken at the
/// start of the clip (the retail projectile leaves at a clip event whose time was
/// not read).
pub const FLIGHT: f32 = 25.0 / 35.0;
/// ADAPTED: how close to the landing spot a creature counts as struck.
pub const HIT_RADIUS: f32 = 1.6;

/// Pure: the impact sound of a landed spear.
pub fn impact_sound(hit: bool) -> &'static str {
    if hit {
        SPEAR_HIT
    } else {
        SPEAR_MISS
    }
}

/// Pure: where a spear thrown from `from` toward `forward` lands.
pub fn landing(from: Vec3, forward: Vec3) -> Vec3 {
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    let mut at = from + flat * attack_range(Some(ToolKind::ThrowingSpears));
    at.y = from.y;
    at
}

struct Flight {
    due: f64,
    at: Vec3,
    thrower: Entity,
}

#[derive(Default)]
struct Throws {
    /// The clip each thrower played last frame.
    last: HashMap<Entity, u32>,
    flying: Vec<Flight>,
}

/// Start a flight when a member begins the `spear_throw` clip; resolve the
/// landings that are due.
fn throws(
    time: Res<Time<Real>>,
    mut state: Local<Throws>,
    crowd: Query<(Entity, &GlobalTransform, &CrowdActor)>,
    creatures: Query<(Entity, &GlobalTransform, &Health)>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut sounds: ResMut<NamedSounds>,
) {
    let now = time.elapsed_secs_f64();
    let mut seen = Vec::new();
    for (entity, tf, actor) in &crowd {
        seen.push(entity);
        let clip = actor.playing;
        let before = state.last.insert(entity, clip);
        if clip == tribe_clip::SPEAR_THROW && before != Some(clip) {
            state.flying.push(Flight {
                due: now + FLIGHT as f64,
                at: landing(tf.translation(), tf.forward().as_vec3()),
                thrower: entity,
            });
        }
    }
    state.last.retain(|e, _| seen.contains(e));
    let listener = camera.single().ok().map(|c| c.translation());
    let flying = std::mem::take(&mut state.flying);
    for flight in flying {
        if now < flight.due {
            state.flying.push(flight);
            continue;
        }
        let struck = creatures.iter().any(|(e, t, h)| {
            e != flight.thrower && !h.dead() && t.translation().distance(flight.at) <= HIT_RADIUS
        });
        let mut play = Play::named(impact_sound(struck), "tribal spear");
        play.distance = listener.map(|l| l.distance(flight.at));
        sounds.play(play, now, 0.0);
    }
}

pub struct TribalWeaponAudioPlugin;

impl Plugin for TribalWeaponAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            throws.run_if(in_state(GameStage::Tribal)).run_if(resource_exists::<NamedSounds>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_impact_names_are_the_retail_ids() {
        assert_eq!(crate::world_objects::name_id(SPEAR_HIT), 0xAA6D_7524, "pushed at 0xC92A9F");
        assert_eq!(crate::world_objects::name_id(SPEAR_MISS), 0x3516_E5D7, "pushed at 0xC922C4");
        assert_eq!(impact_sound(true), SPEAR_HIT);
        assert_eq!(impact_sound(false), SPEAR_MISS);
    }

    #[test]
    fn the_spear_lands_one_range_ahead_on_the_ground_plane() {
        let from = Vec3::new(2.0, 1.5, 3.0);
        let at = landing(from, Vec3::new(0.0, -0.5, -1.0));
        let range = attack_range(Some(ToolKind::ThrowingSpears));
        assert!((at - Vec3::new(2.0, 1.5, 3.0 - range)).length() < 1e-4, "{at}");
        assert_eq!(landing(from, Vec3::ZERO), from, "no facing: it drops at the thrower");
    }

    /// The attack clips of the tool abilities and what their events play
    /// (printed; the thrown spear's impact is code, the rest is clip data).
    #[test]
    #[ignore = "needs the installed Spore animation packages"]
    fn installed_tribal_attack_clips_events() {
        use crate::outfit_crowd::clip_id;
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let clips = [
            ("spear_throw", tribe_clip::SPEAR_THROW),
            ("spear_get", tribe_clip::SPEAR_GET),
            ("spear_stab", clip_id::SPEAR),
            ("axe_attack", clip_id::AXE),
            ("torch_poke", clip_id::TORCH),
            ("chief_attack_01", clip_id::CHIEF_ATTACK),
            ("shaman_attack_01", clip_id::SHAMAN_ATTACK),
        ];
        for (name, id) in clips {
            let Some(clip) = crate::creature_clips::clip(&originals, id) else {
                println!("{name} {id:08X}: no clip");
                continue;
            };
            let all = crate::anim_audio::clip_cues(&clip.anim);
            let cues: Vec<String> = all.iter().map(|c| format!("{:.2}s {:?} {:?}", c.time, c.cue, c.pick)).collect();
            let picked: Vec<&str> = all
                .iter()
                .filter(|c| c.pick.is_some())
                .filter_map(|c| match &c.cue {
                    crate::anim_audio::Cue::Sound { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            let expect: &[&str] = match name {
                "spear_stab" => &[
                    "sfx_trg_com_sp_lo",
                    "sfx_trg_com_sp_med",
                    "sfx_trg_com_sp_hi",
                    "sfx_trg_com_sp_pull",
                    "sfx_trg_com_sp_pull",
                ],
                "axe_attack" => &[
                    "sfx_trg_com_axe_lo",
                    "sfx_trg_com_axe_med",
                    "sfx_trg_com_axe_hi",
                    "sfx_trg_com_axe_med",
                    "sfx_trg_com_axe_lo",
                    "sfx_trg_com_axe_med",
                    "sfx_trg_com_axe_hi",
                    "sfx_trg_com_axe_med",
                ],
                _ => &[],
            };
            if !expect.is_empty() {
                assert_eq!(picked, expect, "{name}: the disabled hit sounds (no `_miss`)");
            }
            println!("{name} {id:08X}: {cues:?}");
        }
    }
}
