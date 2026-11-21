//! Button glyphs on the HUD itself: while the controller is in use, the
//! Creature ability tray's key captions ("1" to "4", literal text of the
//! retail layout) are hidden and the pad buttons that use those abilities
//! (X, Y, B, RB; see `contexts`) are drawn in their place. The captions come
//! back as soon as the mouse or keyboard is used.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::glyphs::{self, Glyph};
use super::{InputMode, Pad};
use crate::creature_hud::{id, CreatureHudRoot};
use crate::spui::view::SpuiState;

/// The pad buttons of ability slots 1 to 4 (`contexts`, Creature).
pub const ABILITY_PADS: [Pad; 4] = [Pad::X, Pad::Y, Pad::B, Pad::RB];

#[derive(Component)]
struct Badge(usize);

/// Captions this module hid (layout window ids), to show them again.
#[derive(Resource, Default)]
struct Hidden(Vec<usize>);

pub struct BadgesPlugin;

impl Plugin for BadgesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hidden>().add_systems(PostUpdate, ability_badges);
    }
}

/// The caption window (a child with literal text) of each ability button.
fn captions(ui: &SpuiState, verbs: &[u32; 4]) -> [Option<usize>; 4] {
    verbs.map(|control| {
        let button = ui.find(control)?;
        ui.layout
            .windows
            .iter()
            .position(|w| w.parent == Some(button) && matches!(w.caption, Some(crate::spui::format::Text::Literal(_))))
    })
}

#[allow(clippy::too_many_arguments)]
fn ability_badges(
    mut commands: Commands,
    mode: Res<InputMode>,
    mut hidden: ResMut<Hidden>,
    mut roots: Query<(&mut SpuiState, &CreatureHudRoot)>,
    mut badges: Query<(Entity, &Badge, &mut Node)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
) {
    let main = roots
        .iter_mut()
        .find(|(ui, root)| **root == CreatureHudRoot::Main && ui.ready)
        .map(|(ui, _)| ui);
    let Some(mut ui) = main.filter(|_| mode.gamepad()) else {
        // Keyboard and mouse (or no Creature HUD): captions back, no badges.
        if let Some((mut ui, _)) = roots.iter_mut().find(|(_, root)| **root == CreatureHudRoot::Main) {
            for caption in hidden.0.drain(..) {
                if caption < ui.layout.windows.len() {
                    ui.set_visible(caption, true);
                }
            }
        } else {
            hidden.0.clear();
        }
        for (entity, _, _) in &badges {
            commands.entity(entity).despawn();
        }
        return;
    };
    // The tray that is showing (social or combat).
    let tray = [(id::SOCIAL_TRAY, id::SOCIAL_VERBS), (id::COMBAT_TRAY, id::COMBAT_VERBS)]
        .into_iter()
        .find(|(tray, _)| ui.find(*tray).is_some_and(|w| ui.visible(w)))
        .map(|(_, verbs)| verbs);
    // SPUI rectangles are physical pixels on the gamma surface.
    let scale = if crate::spui::surface::gamma_surface() {
        windows.iter().next().map(|w| w.scale_factor()).unwrap_or(1.0)
    } else {
        1.0
    };
    let mut places: [Option<(Vec2, f32)>; 4] = [None; 4];
    for verbs in [id::SOCIAL_VERBS, id::COMBAT_VERBS] {
        for (slot, caption) in captions(&ui, &verbs).into_iter().enumerate() {
            let Some(caption) = caption else { continue };
            let button_shown = ui.find(verbs[slot]).is_some_and(|b| ui.visible(b));
            if ui.visible(caption) {
                ui.set_visible(caption, false);
                if !hidden.0.contains(&caption) {
                    hidden.0.push(caption);
                }
            }
            if tray == Some(verbs) && button_shown {
                let r = ui.rect(caption);
                let centre = Vec2::new(r.x1 + r.x2, r.y1 + r.y2) * 0.5 / scale;
                let size = ((r.y2 - r.y1) / scale * 1.25).clamp(16.0, 30.0);
                places[slot] = Some((centre, size));
            }
        }
    }
    let camera = cameras.iter().next();
    for (slot, place) in places.into_iter().enumerate() {
        let existing = badges.iter_mut().find(|(_, badge, _)| badge.0 == slot);
        match (place, existing) {
            (Some((centre, size)), Some((_, _, mut node))) => {
                node.left = Val::Px(centre.x - size * 0.5);
                node.top = Val::Px(centre.y - size * 0.5);
                node.display = Display::Flex;
            }
            (Some((centre, size)), None) => {
                let mut badge = commands.spawn((
                    Badge(slot),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(centre.x - size * 0.5),
                        top: Val::Px(centre.y - size * 0.5),
                        ..default()
                    },
                    GlobalZIndex(crate::spui::surface::COMPOSITE_Z + 620),
                    Pickable::IGNORE,
                ));
                if let Some(camera) = camera {
                    badge.insert(UiTargetCamera(camera));
                }
                badge.with_children(|p| glyphs::spawn(p, Glyph::Button(ABILITY_PADS[slot]), size));
            }
            (None, Some((_, _, mut node))) => node.display = Display::None,
            (None, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_match_the_creature_ability_bindings() {
        let c = super::super::ControllerContext::for_kind(super::super::ContextKind::Creature);
        let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];
        for (slot, pad) in ABILITY_PADS.iter().enumerate() {
            let bound = c.bindings.iter().find(|b| b.pad == *pad).map(|b| b.out);
            assert_eq!(bound, Some(super::super::Out::Key(digits[slot])), "slot {}", slot + 1);
        }
    }
}
