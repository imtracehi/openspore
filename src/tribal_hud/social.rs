//! The social game's bar: the `SocialBar` layout (0xEB1B600F) the Tribal UI
//! (0xCCDEF0 -> 0xE31630) keeps loaded and the performance object drives.
//!
//! VERIFIED (code image, `notes\tribal-social2-notes.md`):
//! * The layout is loaded once with the UI, hidden (0xCCE139) and shown only
//!   while a song runs: 0xDA3040..0xDA309B tests every frame whether the camera
//!   (global 0x167EA30) is closer than `TribeTuning` 0xFA555876 = 200 to the bar's
//!   world point (the troupe's centre lifted by the height of its bounding box
//!   along the planet's up) and shows or hides the layout with the answer
//!   (0xE31120). The position setter the code calls with the projected screen
//!   point (0xB267D0) is an empty stub (`ret 8`), so the bar keeps the place its
//!   layout gives it: the top centre of the screen (the root window's group is
//!   `P[0.5, 0, 0.5, 0]`).
//! * At the start of a song of level 1 or 2 (Cautious / Pleased tribe) the bar
//!   is reset (0xE31510): the left knob's art (75481AD7) is the only one shown,
//!   the full-bar art (B5481AE1, 55481AF3, D5481B15), the fill (B5481B0B), the
//!   right knob's own art (B5481AEA) and the star burst (D5481B72) are hidden,
//!   the fill is 0; the two faces (B5481B68 left, D5481B5F right) show the
//!   attitude's face and the next one (0xE31170: Cautious 2 -> 3, Pleased 3 -> 4).
//!   A song of level 0 never creates the bar.
//! * Every frame (0xE31380): the fill window is shown while the fill is above 0
//!   and its width is the authored width times the fill (`SetSize(width x fill,
//!   height)`); a full bar (fill >= 1) shows B5481AE1, 55481AF3 and D5481B15;
//!   a success shows the star burst (0xE31150, after the bar object said
//!   full, 0xDA34CC); a failed song drains the fill at 0.4 per second.
//!
//! The bar's own numbers live in [`crate::tribal_rules::SocialBarMeter`]; this
//! module turns the campaign's performance (or the bar of the song that just
//! ended) into a [`SocialBarView`] and pushes it into the layout.

use bevy::prelude::*;

use super::TribalHudState;
use crate::{
    game::RunProgress,
    locomotion_preview::PoseClip,
    spui::view::SpuiState,
    stages::GameStage,
    swarm_fx::{SwarmPlay, SwarmStop},
    tribal,
    tribal_rts::RivalMember,
    tribal_rules::{self, SocialBarMeter},
};

pub const LAYOUT: &str = "SocialBar";

/// Control ids of `SocialBar` (the names are the roles the code gives them).
#[allow(dead_code)]
pub mod id {
    pub const ROOT: u32 = 0x9548_1ABB;
    /// The bar's frame (a 128 x 32 picture) with its two full-bar overlays.
    pub const FRAME: u32 = 0x0564_EEB8;
    /// The fill (+0x94) and the glow of a full bar (+0x98), children of the frame.
    pub const FILL: u32 = 0xB548_1B0B;
    pub const FRAME_FULL: u32 = 0xD548_1B15;
    /// Left knob art: the one shown at the reset (+0x84) and the full-bar one (+0x88).
    pub const KNOB_LEFT: u32 = 0x7548_1AD7;
    pub const KNOB_LEFT_FULL: u32 = 0xB548_1AE1;
    /// Right knob art: hidden by the reset (+0x8C) and the full-bar one (+0x90).
    pub const KNOB_RIGHT: u32 = 0xB548_1AEA;
    pub const KNOB_RIGHT_FULL: u32 = 0x5548_1AF3;
    /// The five-frame faces (+0x9C left, +0xA0 right) and the star burst (+0xA4).
    pub const FACE_LEFT: u32 = 0xB548_1B68;
    pub const FACE_RIGHT: u32 = 0xD548_1B5F;
    pub const STAR: u32 = 0xD548_1B72;
}

/// What the bar shows this frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SocialBarView {
    /// The bar's fill (+0x68).
    pub fill: f32,
    /// Frames of the left and the right face.
    pub faces: (u32, u32),
    /// The star burst of a success.
    pub star: bool,
}

/// The view of a bar, or `None` for a song without one.
pub fn view_of(meter: &SocialBarMeter, level: u8, star: bool) -> Option<SocialBarView> {
    let faces = tribal_rules::social_bar_faces(level)?;
    Some(SocialBarView { fill: meter.fill, faces, star })
}

/// 0xDA3040..0xDA309B: the layout is shown while the camera is within
/// [`tribal_rules::SOCIAL_BAR_VISIBLE_DISTANCE`] of the bar's world point (squared
/// distances, a strict test).
pub fn near_enough(camera: Vec3, point: Vec3) -> bool {
    let d = tribal_rules::SOCIAL_BAR_VISIBLE_DISTANCE;
    d * d > camera.distance_squared(point)
}

/// The bar of the running song (or of the one that just ended) for the HUD.
/// `point` is the troupe's world point and `camera` the camera position.
pub fn publish_view(run: &RunProgress, camera: Option<Vec3>) -> Option<SocialBarView> {
    let campaign = &run.tribal_campaign;
    let (view, point) = match (&campaign.performance, &campaign.social_end) {
        (Some(p), _) => (view_of(&p.bar, p.level, false)?, tribal::village_position(p.village)),
        // The ended song's bar stands at the village it was for.
        (None, Some(end)) => (view_of(&end.meter, end.level, end.success)?, tribal::village_position(end.village)),
        _ => return None,
    };
    match camera {
        Some(cam) if !near_enough(cam, point) => None,
        _ => Some(view),
    }
}

/// Fill [`TribalHudState::social_bar`].
pub fn publish_social(
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    cameras: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut hud: ResMut<TribalHudState>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let view = if *stage.get() == GameStage::Tribal {
        publish_view(&run, cameras.single().ok().map(|t| t.translation()))
    } else {
        None
    };
    if hud.social_bar != view {
        hud.social_bar = view;
    }
}

/// Push the view into the layout (the reset of 0xE31510 and the frame update of
/// 0xE31380).
pub fn bind_bar(ui: &mut SpuiState, view: Option<&SocialBarView>) {
    let Some(root) = ui.find(id::ROOT) else {
        return;
    };
    ui.set_visible(root, view.is_some());
    let Some(view) = view else {
        return;
    };
    let full = view.fill >= 1.0;
    for (control, on) in [
        (id::KNOB_LEFT, true),
        (id::KNOB_RIGHT, false),
        (id::KNOB_LEFT_FULL, full),
        (id::KNOB_RIGHT_FULL, full),
        (id::FRAME_FULL, full),
        (id::STAR, view.star),
    ] {
        super::set_visible(ui, control, on);
    }
    if let Some(w) = ui.find(id::FACE_LEFT) {
        ui.set_frame(w, Some(view.faces.0));
    }
    if let Some(w) = ui.find(id::FACE_RIGHT) {
        ui.set_frame(w, Some(view.faces.1));
    }
    // The fill: shown above 0, the authored width times the fill. ADAPTED: the
    // retail code lets the last step pass 1 (and the width with it); the bar
    // stops at the frame's end here.
    if let Some(w) = ui.find(id::FILL) {
        ui.set_visible(w, view.fill > 0.0);
        let mut area = ui.layout.window(w).area;
        let width = area[2] - area[0];
        area[2] = area[0] + width * view.fill.clamp(0.0, 1.0);
        ui.set_area(w, area);
    }
}

// ---------------------------------------------------------------------------
// The village's reactions: thought bubbles and answer clips
// ---------------------------------------------------------------------------

/// The animation of a member that shows a thought bubble (0xDA2244, `PlayAnimation`
/// 0x061E0675) and the bubble effects by instrument kind (0xDA21B8 / 0xDA21C9 /
/// 0xDA2190: the ability type 6 maracas -> `tribe_chat_icon_shaker`, 5 horn ->
/// `tribe_chat_icon_flute`, 7 didgeridoo -> `tribe_chat_icon_horn`; the effect is
/// created through the pool 0x950A2438, 0xDA225F). The array is by the troupe's
/// instrument index (0 horn, 1 maracas, 2 didgeridoo).
pub const BUBBLE_CLIP: u32 = 0x061E_0675;
pub const BUBBLE_EFFECTS: [u32; 3] = [0xC393_3F3F, 0x4EA1_9703, 0x9DE7_94A4];
/// What a village member plays when the round is evaluated (0xDA049D..0xDA04D1):
/// a bubble member by how well the bubble was answered (1 not at all, 2 partly, 3
/// whole), every other member of the audience the last clip.
pub const ANSWER_CLIPS: [u32; 3] = [0x9509_2408, 0xD509_23DB, 0xF509_23E7];
pub const IGNORED_CLIP: u32 = 0x7509_23CF;
/// Tag of the bubble effects (for the stop at the evaluation).
const BUBBLE_TAG: u64 = 0x534F_4342_5542;
/// `PoseClip` serials of these requests start here (the tribe clip driver counts
/// from 0, so they never meet).
const SERIAL_BASE: u32 = 0x4000_0000;
/// ADAPTED: height over a member's feet the bubble effect is attached at (the
/// retail pool effect rides the creature's own effect attachment).
const BUBBLE_HEIGHT: f32 = 1.6;

/// What the audience of the running song has been shown.
#[derive(Default)]
pub struct AudienceSeen {
    village: Option<usize>,
    bubbles: u32,
    reactions: u32,
    /// Members with a bubble up.
    bubbled: Vec<Entity>,
    serial: u32,
}

/// The members a bubble goes to: the first `required` of the village's members,
/// nearest to the troupe first (retail sorts them with 0xD9B3E0; ADAPTED: the
/// sort's key was not read, the nearest ask first).
pub fn bubble_members(audience: &[(Entity, f32)], required: usize) -> Vec<Entity> {
    let mut sorted = audience.to_vec();
    sorted.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.to_bits().cmp(&b.0.to_bits())));
    sorted.into_iter().take(required).map(|(e, _)| e).collect()
}

/// The clip of a member at an evaluation: a bubble member answers by the outcome
/// (`reaction` 1..=3), the others play the ignored clip.
pub fn evaluation_clip(bubbled: bool, reaction: u8) -> u32 {
    if bubbled {
        ANSWER_CLIPS[(reaction.clamp(1, 3) - 1) as usize]
    } else {
        IGNORED_CLIP
    }
}

/// Show the thought bubbles and the answers of the village the troupe performs
/// for (the sendgame handler 0xDA1F20 and the evaluation 0xDA0100).
#[allow(clippy::too_many_arguments)]
pub fn audience(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    members: Query<(Entity, &Transform, &RivalMember, Option<&PoseClip>)>,
    mut plays: MessageWriter<SwarmPlay>,
    mut stops: MessageWriter<SwarmStop>,
    mut seen: Local<AudienceSeen>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    let song = (*stage.get() == GameStage::Tribal).then_some(run.tribal_campaign.performance).flatten();
    let stop_bubbles = |seen: &mut AudienceSeen, stops: &mut MessageWriter<SwarmStop>| {
        for e in seen.bubbled.drain(..) {
            stops.write(SwarmStop { effect: 0, attach_to: Some(e), tag: BUBBLE_TAG, hard: false });
        }
    };
    let Some(p) = song else {
        stop_bubbles(&mut seen, &mut stops);
        seen.village = None;
        return;
    };
    if seen.village != Some(p.village) {
        // A new song: what already happened is not shown again.
        stop_bubbles(&mut seen, &mut stops);
        *seen = AudienceSeen { village: Some(p.village), bubbles: p.bubble_serial, reactions: p.reaction_serial, ..default() };
    }
    let centre = tribal::village_position(p.village);
    let mut audience: Vec<(Entity, f32, Option<PoseClip>)> = members
        .iter()
        .filter(|(_, _, m, _)| m.village == p.village)
        .map(|(e, t, _, clip)| (e, t.translation.distance(centre), clip.copied()))
        .collect();
    audience.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.to_bits().cmp(&b.0.to_bits())));
    let play = |commands: &mut Commands, seen: &mut AudienceSeen, e: Entity, action: u32, idle: Option<PoseClip>| {
        // A member that walks (no tribe clip running) keeps its own gait.
        let Some(idle) = idle else {
            return;
        };
        seen.serial = seen.serial.wrapping_add(1);
        commands.entity(e).insert(PoseClip { action: Some(action), idle: idle.idle, serial: SERIAL_BASE + (seen.serial & 0x3FFF_FFFF) });
    };
    if p.reaction_serial != seen.reactions {
        // The evaluation (0xDA049D): the bubbles go, every member of the audience
        // answers.
        seen.reactions = p.reaction_serial;
        let bubbled = std::mem::take(&mut seen.bubbled);
        for e in &bubbled {
            stops.write(SwarmStop { effect: 0, attach_to: Some(*e), tag: BUBBLE_TAG, hard: false });
        }
        for (e, _, clip) in &audience {
            play(&mut commands, &mut seen, *e, evaluation_clip(bubbled.contains(e), p.reaction), *clip);
        }
    }
    if p.bubble_serial != seen.bubbles {
        // The thought bubble (0xDA2244..0xDA225F): the nearest `required` members
        // play the bubble clip with the instrument's icon effect.
        seen.bubbles = p.bubble_serial;
        stop_bubbles(&mut seen, &mut stops);
        let kind = tribal::instrument_kind(p.prompt).unwrap_or(0);
        let list: Vec<(Entity, f32)> = audience.iter().map(|(e, d, _)| (*e, *d)).collect();
        for e in bubble_members(&list, p.required as usize) {
            let clip = audience.iter().find(|(a, ..)| *a == e).and_then(|(.., c)| *c);
            play(&mut commands, &mut seen, e, BUBBLE_CLIP, clip);
            plays.write(SwarmPlay {
                attach_to: Some(e),
                tag: BUBBLE_TAG,
                ..SwarmPlay::at(BUBBLE_EFFECTS[kind.min(2)], Vec3::Y * BUBBLE_HEIGHT)
            });
            seen.bubbled.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubbles_go_to_the_nearest_members_and_answers_follow_the_outcome() {
        let mut world = World::new();
        let a = world.spawn_empty().id();
        let b = world.spawn_empty().id();
        let c = world.spawn_empty().id();
        let audience = [(a, 9.0), (b, 2.0), (c, 5.0)];
        assert_eq!(bubble_members(&audience, 2), vec![b, c]);
        assert_eq!(bubble_members(&audience, 7), vec![b, c, a], "never more than the audience");
        assert_eq!(bubble_members(&audience, 0), Vec::<Entity>::new());
        // 0xDA049D: 3 whole / 2 partly / 1 none; the rest of the audience ignores.
        assert_eq!(evaluation_clip(true, 3), 0xF509_23E7);
        assert_eq!(evaluation_clip(true, 2), 0xD509_23DB);
        assert_eq!(evaluation_clip(true, 1), 0x9509_2408);
        assert_eq!(evaluation_clip(false, 3), IGNORED_CLIP);
        assert_eq!(BUBBLE_EFFECTS.len(), 3);
    }

    #[test]
    fn the_bar_shows_within_200_of_the_troupe() {
        let at = Vec3::new(10.0, 0.0, 10.0);
        assert!(near_enough(at + Vec3::Y * 199.0, at));
        assert!(!near_enough(at + Vec3::Y * 200.0, at), "the test is strict");
        assert!(!near_enough(at + Vec3::X * 250.0, at));
    }

    #[test]
    fn a_song_without_a_bar_has_no_view() {
        let meter = SocialBarMeter::default();
        assert_eq!(view_of(&meter, 0, false), None);
        let view = view_of(&meter, 1, false).unwrap();
        assert_eq!((view.fill, view.faces, view.star), (0.0, (2, 3), false));
        assert_eq!(view_of(&meter, 2, true).unwrap().faces, (3, 4));
    }
}
