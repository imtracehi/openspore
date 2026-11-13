//! The verb tray's recharge overlay: a clock-style wipe over the icon of an
//! ability that is still recharging.
//!
//! What the original does (VERIFIED, tray update 0xD4A560): for each slot it asks
//! `0xD49510(slot)` for the slot's recharge progress, a float at `+4` of the
//! 16-byte entries of the verb tray's vector (`[object + 0xC28]`); progress
//! `1.0` is "ready" (`0xD1E8D0`, the check the button handler uses, tests it for
//! equality with 1.0), and while it is below 1 the tray calls `0xD49F30(window,
//! progress)`. That routine takes the window's `SporeStdDrawable`
//! (`0x053EB526`, through the window component `0x8ED27E7A`), the window's
//! rectangle and its image, and draws the image as a triangle fan with
//! `0x805CA0` (an image drawn between two angles around the rectangle's centre):
//! the start angle is `pi / 2` (twelve o'clock) and the angle of the other edge
//! is `pi / 2 - angle_scale x progress` (the float at `0x169E434`, a runtime
//! global), so the drawn wedge shrinks as the progress grows and the whole icon
//! is plain once the progress reaches 1; a window flag (bit 2 of the window
//! component's flags, 0xD49F3B..0xD49F69) swaps which side of the wipe is drawn.
//!
//! ADAPTED: the angle scale is taken as a full turn (2 pi), the flag's side is
//! not drawn, and the wedge is a translucent dark pie over the icon instead of
//! the icon's own image drawn as a fan (the fan's colour and the `0.05` constant
//! passed with it were not decoded). The progress is `1 - seconds left / refresh`
//! (the ability's refresh, `creature_actions::combat_ability` / `social_refresh`).

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{id, CreatureHudRoot, CreatureHudState, Stance};
use crate::spui::view::SpuiState;

/// Side of the generated wedge picture in pixels (the picture is stretched
/// over the button).
const MASK_SIZE: u32 = 48;
/// The wipe moves in this many steps over a full turn (the picture is only
/// redrawn when the step changes).
const STEPS: i32 = 96;
/// The icon's share of the button window's width (the window is a little larger
/// than the button art it holds).
const ICON_SHARE: f32 = 0.74;
/// Colour of the covered part: translucent black.
const COVER_ALPHA: u8 = 0x99;

/// Pure: the progress of a slot's recharge, `0..=1` (1 = ready), from the
/// seconds still to wait and the ability's refresh.
pub fn progress(cooldown: f32, refresh: f32) -> f32 {
    if !(cooldown > 0.0) || !(refresh > 0.0) {
        return 1.0;
    }
    (1.0 - cooldown / refresh).clamp(0.0, 1.0)
}

/// Pure: the wipe step for a progress (the wedge that stays covered shrinks
/// as the step grows; `STEPS` is ready).
pub fn step_of(progress: f32) -> i32 {
    (progress.clamp(0.0, 1.0) * STEPS as f32).floor() as i32
}

/// Pure: the pixels (RGBA, row-major, `size` x `size`) of the wedge at `step`:
/// a pixel is covered while its clockwise angle from twelve o'clock is past the
/// share `step / STEPS` of the turn, so the clear part grows clockwise from the
/// top (0xD49F30: the fan starts at pi / 2 and its far edge turns by
/// `2 pi x progress`; the covered share is `1 - progress`).
pub fn wedge(size: u32, step: i32) -> Vec<u8> {
    let share = (step.clamp(0, STEPS) as f32) / STEPS as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    let c = (size as f32 - 1.0) * 0.5;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            // Clockwise angle from twelve o'clock, 0..1 of a turn.
            let turn = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
            if turn >= share {
                let at = ((y * size + x) * 4) as usize;
                out[at + 3] = COVER_ALPHA;
            }
        }
    }
    out
}

/// One slot's overlay: the node over the button and its wedge picture.
struct Entry {
    button: Entity,
    node: Entity,
    image: Handle<Image>,
    step: i32,
}

/// The overlays of the eight tray buttons (the social and the combat tray).
#[derive(Resource, Default)]
pub struct RechargeOverlays {
    entries: Vec<Option<Entry>>,
}

/// The seconds a slot waits in all: the ability's refresh.
fn refresh(originals: &crate::Originals, stance: Stance, slot: usize, rank: u8) -> f32 {
    let catalog = &originals.creature_abilities;
    match stance {
        Stance::Social => crate::creature_actions::social_refresh(catalog, slot, rank),
        Stance::Combat => crate::creature_actions::combat_ability(catalog, slot, rank).recharge,
    }
}

/// Keep a wedge over every tray button whose ability recharges.
pub fn draw(
    mut commands: Commands,
    hud: Res<CreatureHudState>,
    originals: Res<crate::Originals>,
    mut images: ResMut<Assets<Image>>,
    mut overlays: ResMut<RechargeOverlays>,
    roots: Query<(&SpuiState, &CreatureHudRoot)>,
    mut nodes: Query<&mut Visibility>,
) {
    let Some(ui) = roots.iter().find(|(ui, kind)| **kind == CreatureHudRoot::Main && ui.ready).map(|(ui, _)| ui) else {
        return;
    };
    if overlays.entries.is_empty() {
        overlays.entries.resize_with(8, || None);
    }
    for (stance, controls, slots) in [
        (Stance::Social, id::SOCIAL_VERBS, &hud.social),
        (Stance::Combat, id::COMBAT_VERBS, &hud.combat),
    ] {
        for (slot, control) in controls.into_iter().enumerate() {
            let index = slot + if stance == Stance::Combat { 4 } else { 0 };
            let Some(window) = ui.find(control) else {
                continue;
            };
            let button = ui.entities[window];
            let VerbSlotView { rank, cooldown } = VerbSlotView::of(&slots[slot]);
            let total = if rank > 0 { refresh(&originals, stance, slot, rank) } else { 0.0 };
            let step = step_of(progress(cooldown, total));
            let visible = rank > 0 && step < STEPS && hud.stance == stance && !hud.paused;
            // A new button entity (the HUD was rebuilt) gets a new overlay.
            if overlays.entries[index].as_ref().is_some_and(|e| e.button != button) {
                if let Some(old) = overlays.entries[index].take() {
                    if let Ok(mut e) = commands.get_entity(old.node) {
                        e.despawn();
                    }
                }
            }
            if overlays.entries[index].is_none() {
                if !visible {
                    continue;
                }
                let image = images.add(Image::new(
                    Extent3d { width: MASK_SIZE, height: MASK_SIZE, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    wedge(MASK_SIZE, step),
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                ));
                // The button window is taller than its icon (the hot key digit
                // sits below it): the wedge is a square of the window's width
                // in the middle of it, where the icon is drawn.
                let node = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        // Above the window's own icon (the SPUI paint children
                        // are z 0), under its text (z 2: the hot key digit).
                        ZIndex(1),
                        ChildOf(button),
                    ))
                    .id();
                commands.spawn((
                    Node { width: Val::Percent(ICON_SHARE * 100.0), aspect_ratio: Some(1.0), ..default() },
                    ImageNode::new(image.clone()),
                    ChildOf(node),
                ));
                overlays.entries[index] = Some(Entry { button, node, image, step });
                continue;
            }
            let Some(entry) = overlays.entries[index].as_mut() else {
                continue;
            };
            if let Ok(mut shown) = nodes.get_mut(entry.node) {
                let want = if visible { Visibility::Inherited } else { Visibility::Hidden };
                if *shown != want {
                    *shown = want;
                }
            }
            if visible && entry.step != step {
                entry.step = step;
                if let Some(mut image) = images.get_mut(&entry.image) {
                    image.data = Some(wedge(MASK_SIZE, step));
                }
            }
        }
    }
}

/// The two fields of a tray slot this module reads.
struct VerbSlotView {
    rank: u8,
    cooldown: f32,
}

impl VerbSlotView {
    fn of(slot: &super::VerbSlot) -> Self {
        Self { rank: slot.rank, cooldown: slot.cooldown }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_the_share_of_the_refresh_that_has_passed() {
        assert_eq!(progress(0.0, 10.0), 1.0, "ready");
        assert_eq!(progress(10.0, 10.0), 0.0, "just used");
        assert!((progress(2.5, 10.0) - 0.75).abs() < 1e-6);
        assert_eq!(progress(1.0, 0.0), 1.0, "no refresh, nothing to wait for");
        assert_eq!(progress(f32::NAN, 10.0), 1.0);
        assert_eq!(step_of(0.0), 0);
        assert_eq!(step_of(1.0), STEPS);
        assert!(step_of(0.5) == STEPS / 2);
    }

    #[test]
    fn the_covered_wedge_shrinks_clockwise_from_the_top() {
        let size = 48u32;
        let alpha = |pixels: &[u8], x: u32, y: u32| pixels[((y * size + x) * 4 + 3) as usize];
        // Just used: every pixel is covered.
        let full = wedge(size, 0);
        assert!(full.chunks(4).all(|p| p[3] == COVER_ALPHA));
        // Half way: the right half (clockwise from the top) is clear, the left covered.
        let half = wedge(size, STEPS / 2);
        assert_eq!(alpha(&half, 40, 24), 0, "three o'clock is clear");
        assert_eq!(alpha(&half, 7, 24), COVER_ALPHA, "nine o'clock is covered");
        // A quarter of the turn is clear: the upper right quadrant.
        let quarter = wedge(size, STEPS / 4);
        assert_eq!(alpha(&quarter, 36, 10), 0, "one o'clock is clear");
        assert_eq!(alpha(&quarter, 36, 38), COVER_ALPHA, "five o'clock is covered");
        // Ready: nothing is covered.
        assert!(wedge(size, STEPS).chunks(4).all(|p| p[3] == 0));
    }
}
