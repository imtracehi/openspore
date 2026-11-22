//! The stats readout (`stats` cheat / debug menu) and the toasts that show a
//! cheat's result while the console is closed.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::entity::Entities;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::console::line_color;
use super::{CheatLog, CheatState};
use crate::stages::GameStage;

/// Stage values for the stats readout, filled by `apply.rs` every frame.
#[derive(Resource, Default, Debug, Clone)]
pub struct Readout {
    pub player: Option<Vec3>,
    pub lines: Vec<String>,
}

#[derive(Component)]
struct StatsRoot;

#[derive(Component)]
struct StatsText;

#[derive(Component)]
struct ToastRoot {
    signature: String,
}

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        app.init_resource::<Readout>().add_systems(PostUpdate, (stats, toasts));
    }
}

fn scale_of(windows: &Query<&Window, With<PrimaryWindow>>) -> f32 {
    windows.iter().next().map(|w| (w.height() / 1080.0).clamp(0.6, 2.0)).unwrap_or(1.0)
}

/// The readout text.
#[allow(clippy::too_many_arguments)]
fn stats_text(
    state: &CheatState,
    diagnostics: Option<&DiagnosticsStore>,
    entities: usize,
    stage: Option<GameStage>,
    camera: Option<Vec3>,
    readout: &Readout,
) -> String {
    let fps = diagnostics
        .and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FPS))
        .and_then(|d| d.smoothed());
    let ms = diagnostics
        .and_then(|d| d.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME))
        .and_then(|d| d.smoothed());
    let mut out = Vec::new();
    out.push(match (fps, ms) {
        (Some(f), Some(m)) => format!("FPS {f:.0}  ({m:.1} ms)"),
        _ => "FPS -".to_string(),
    });
    out.push(format!("Entities {entities}"));
    if let Some(stage) = stage {
        out.push(format!("Stage {stage:?}"));
    }
    if let Some(p) = readout.player {
        out.push(format!("Player {:.1}, {:.1}, {:.1}", p.x, p.y, p.z));
    }
    if let Some(c) = camera {
        out.push(format!("Camera {:.1}, {:.1}, {:.1}", c.x, c.y, c.z));
    }
    out.extend(readout.lines.iter().cloned());
    let mut switches = Vec::new();
    if (state.time_scale - 1.0).abs() > 1e-3 {
        switches.push(format!("speed x{:.2}", state.time_scale));
    }
    if state.god_mode {
        switches.push("god mode".to_string());
    }
    if state.infinite_energy {
        switches.push("infinite energy".to_string());
    }
    if state.freedom {
        switches.push("freedom".to_string());
    }
    if !switches.is_empty() {
        out.push(format!("Cheats: {}", switches.join(", ")));
    }
    out.join("\n")
}

#[allow(clippy::too_many_arguments)]
fn stats(
    mut commands: Commands,
    state: Res<CheatState>,
    diagnostics: Option<Res<DiagnosticsStore>>,
    entities: &Entities,
    stage: Option<Res<State<GameStage>>>,
    readout: Res<Readout>,
    cameras: Query<(Entity, &GlobalTransform), With<crate::GameCamera>>,
    roots: Query<Entity, With<StatsRoot>>,
    mut texts: Query<&mut Text, With<StatsText>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut since: Local<f32>,
) {
    if !state.stats {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    *since += time.delta_secs();
    let fresh = roots.is_empty();
    if !fresh && *since < 0.25 {
        return;
    }
    *since = 0.0;
    let camera = cameras.iter().next();
    let text = stats_text(
        &state,
        diagnostics.as_deref(),
        entities.len() as usize,
        stage.map(|s| s.get().clone()),
        camera.map(|c| c.1.translation()),
        &readout,
    );
    if !fresh {
        for mut t in &mut texts {
            t.0.clone_from(&text);
        }
        return;
    }
    let scale = scale_of(&windows);
    let mut root = commands.spawn((
        StatsRoot,
        super::CheatUi,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0 * scale),
            top: Val::Percent(26.0),
            padding: UiRect::axes(Val::Px(10.0 * scale), Val::Px(6.0 * scale)),
            border_radius: BorderRadius::all(Val::Px(8.0 * scale)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.62)),
        GlobalZIndex(i32::MAX - 12),
        Pickable::IGNORE,
    ));
    if let Some((camera, _)) = camera {
        root.insert(UiTargetCamera(camera));
    }
    root.with_children(|r| {
        r.spawn((
            StatsText,
            Text::new(text),
            TextFont {
                font_size: bevy::text::FontSize::Px(15.0 * scale),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.95, 1.0)),
            TextLayout::no_wrap(),
        ));
    });
}

fn toasts(
    mut commands: Commands,
    log: Res<CheatLog>,
    state: Res<CheatState>,
    roots: Query<(Entity, &ToastRoot)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
) {
    let show = !state.console_open && !log.toasts.is_empty();
    let scale = scale_of(&windows);
    let signature = format!("{:?}|{scale:.2}", log.toasts.iter().map(|t| &t.1).collect::<Vec<_>>());
    let mut current = false;
    for (entity, root) in &roots {
        if show && root.signature == signature && !current {
            current = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !show || current {
        return;
    }
    let mut root = commands.spawn((
        ToastRoot { signature },
        super::CheatUi,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(14.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(4.0 * scale),
            ..default()
        },
        GlobalZIndex(i32::MAX - 11),
        Pickable::IGNORE,
    ));
    if let Some(camera) = cameras.iter().next() {
        root.insert(UiTargetCamera(camera));
    }
    root.with_children(|root| {
        for (kind, text, _) in &log.toasts {
            root.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(12.0 * scale), Val::Px(5.0 * scale)),
                    border_radius: BorderRadius::all(Val::Px(8.0 * scale)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.7)),
                Pickable::IGNORE,
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(text.clone()),
                    TextFont {
                        font_size: bevy::text::FontSize::Px(18.0 * scale),
                        ..default()
                    },
                    TextColor(line_color(*kind)),
                    TextShadow::default(),
                    TextLayout::no_wrap(),
                ));
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readout_lists_stage_player_and_switches() {
        let state = CheatState {
            god_mode: true,
            time_scale: 2.0,
            ..default()
        };
        let readout = Readout {
            player: Some(Vec3::new(1.0, 2.0, 3.0)),
            lines: vec!["DNA 150".into()],
        };
        let text = stats_text(&state, None, 42, Some(GameStage::Creature), None, &readout);
        assert!(text.contains("Entities 42"));
        assert!(text.contains("Stage Creature"));
        assert!(text.contains("Player 1.0, 2.0, 3.0"));
        assert!(text.contains("DNA 150"));
        assert!(text.contains("Cheats: speed x2.00, god mode"));
    }
}
