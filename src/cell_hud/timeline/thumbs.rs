//! The creature pictures of the History's generation cards.
//!
//! Retail: the card of a generation shows a thumbnail of that generation's creature, rendered from the creation record
//! (`ThumbnailManager`, like the portrait of the HUD, `cell_hud::portrait`). OpenSpore keeps one record per editor visit
//! ([`GenerationRecord`]: the mounted parts and the body chain at the visit, filled by [`record`] the frame after the
//! editor closed, saved in `CellSave.generations`) and renders them here: one small orthographic camera per generation
//! looking straight down at that creature on the Creator's pedestal, framed and lit like the HUD portrait, into its own
//! 128 x 128 image. The scenes sit side by side at [`SPACING`] world units on one render layer (an orthographic camera
//! only sees its own window), are built one per frame while the History is open and are dropped when it closes.
//!
//! Each record also keeps the skin paint of its visit (`GenerationRecord::paint`). A creature whose paint and body
//! chain are the live ones is drawn with the live materials; any other is painted again with the retail painter
//! into material copies of its own ([`thumb_paint`]: the style's brush script on the body surface and on the chart
//! of every part instance), so each generation shows its own colours.
//!
//! ADAPTED: a visit that predates the record (older saves) shows the creature of the newest such visit as the
//! live creature and leaves earlier ones empty, and a record written before the paint was kept shows the live
//! paint; the first card ("Generation 1") shows the neutral cell once the player has been in the editor, with the
//! neutral cell's own paint when the live body is not the baked one (see [`thumb_paint`]).

use std::collections::HashMap;

use bevy::{
    asset::AssetId,
    camera::{visibility::RenderLayers, RenderTarget, ScalingMode},
    prelude::*,
    render::render_resource::TextureFormat,
};

use super::events::{EventKind, GenerationRecord};
use super::thumb_paint::ThumbPainter;
use crate::{
    cell_body::{self, BodyFrame},
    cell_contract::CellHudState,
    cell_editor_scene as scene,
    cell_hud::portrait::{framing, mesh_box},
    cell_visuals::CellVisualAssets,
    game::RunProgress,
    stages::{GameStage, StageOwned},
    Originals,
};

/// Render layer of the generation scenes (21 .. 24 and 26 .. 31 belong to other pictures).
const LAYER: usize = 25;
const SIZE: u32 = 128;
/// Frames a camera renders after its scene was built (asynchronous pipeline, like the portrait).
const RENDER_FRAMES: u32 = 45;
/// World distance between two scenes.
const SPACING: f32 = 60.0;
const MIN_RADIUS: f32 = 1.05;

#[derive(Component)]
pub struct GenerationScene;

struct Slot {
    image: Handle<Image>,
    camera: Entity,
    root: Entity,
    pedestal: Option<Entity>,
    frames: u32,
}

/// The generation pictures: slot 0 is the first card (the neutral cell), slot `n + 1` the creature of editor visit `n`.
#[derive(Resource, Default)]
pub struct GenerationThumbs {
    slots: Vec<Option<Slot>>,
    light: Option<Entity>,
    gamma: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    /// The body revision the scenes were built for (a rewritten body mesh rebuilds them).
    revision: u32,
    /// The painter of the records that differ from the live creature.
    painter: ThumbPainter,
}

impl GenerationThumbs {
    /// The rendered picture of slot `n` (once its scene exists).
    pub fn image(&self, slot: usize) -> Option<Handle<Image>> {
        self.slots.get(slot)?.as_ref().map(|s| s.image.clone())
    }
}

/// The editor visits recorded so far (`clg_evolveineditor` events).
fn recorded_visits(run: &RunProgress) -> usize {
    run.cell.timeline.iter().filter(|e| e.kind == EventKind::EvolveInEditor).count()
}

/// The editor visits of the history the timeline shows (its generation cards after the first).
pub fn visits(cache: &super::TimelineCache) -> usize {
    cache.stacks.iter().filter(|s| matches!(s.kind, super::events::StackKind::EvolveInEditor(_))).count()
}

/// The creature as it is now.
fn live(run: &RunProgress) -> GenerationRecord {
    GenerationRecord {
        parts: run.cell_parts.clone(),
        spine: run.cell_creation.spine.clone(),
        paint: Some(run.cell_creation.paint.clone()),
    }
}

/// Capture only: `--cell-timeline-gen-paint STYLE R,G,B` gives the creature of the newest visit that has no
/// record (the demo history's) that style (a `cl_skinpaint_cell_*` name) and colour, so its card must replay a paint
/// other than the live one.
fn capture_paint() -> Option<crate::cell_paint::CellPaint> {
    static PAINT: std::sync::OnceLock<Option<crate::cell_paint::CellPaint>> = std::sync::OnceLock::new();
    PAINT
        .get_or_init(|| {
            let args: Vec<String> = std::env::args().collect();
            let at = args.iter().position(|a| a == "--cell-timeline-gen-paint")?;
            let theme = crate::world_objects::name_id(args.get(at + 1)?);
            let rgb: Vec<f32> = args.get(at + 2)?.split(',').filter_map(|v| v.parse().ok()).collect();
            let color = [*rgb.first()?, *rgb.get(1)?, *rgb.get(2)?];
            Some(crate::cell_paint::CellPaint { theme, color })
        })
        .clone()
}

/// What the generation cards show: `None` for a card without a picture.
pub fn record_of(run: &RunProgress, visits: usize, slot: usize) -> Option<GenerationRecord> {
    if slot == 0 {
        // The first card: the creature before the first visit, the neutral cell (no parts, the baked body).
        return Some(GenerationRecord::neutral());
    }
    let n = slot - 1;
    match run.cell.generations.get(n).cloned().flatten() {
        Some(record) => Some(record),
        None if n + 1 == visits => Some(GenerationRecord {
            paint: capture_paint().or_else(|| live(run).paint),
            ..live(run)
        }),
        None => None,
    }
}

/// Keep `CellSave.generations` in step with the editor visits: the frame after a visit was recorded
/// (`0xE809A0`, `cell_flow`), the creature is the one the editor left. Only the newest visit can be filled (a save
/// that predates the record gets `None` for the older ones).
pub fn record(mut run: ResMut<RunProgress>) {
    let visits = recorded_visits(&run);
    if run.cell.generations.len() >= visits {
        return;
    }
    let creature = live(&run);
    while run.cell.generations.len() + 1 < visits {
        run.cell.generations.push(None);
    }
    run.cell.generations.push(Some(creature));
    run.request_save();
}

/// Build, frame and render the generation scenes while the History is open.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    mut state: ResMut<GenerationThumbs>,
    stage: Res<State<GameStage>>,
    hud: Res<CellHudState>,
    run: Res<RunProgress>,
    cache: Res<super::TimelineCache>,
    mut originals: ResMut<Originals>,
    visuals: Option<Res<CellVisualAssets>>,
    body_data: Option<Res<cell_body::CellBody>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cameras: Query<&mut Camera, With<GenerationScene>>,
) {
    let body = visuals.as_deref().and_then(|v| v.body.as_ref());
    let open = *stage.get() == GameStage::Cell && hud.summary.is_some() && body.is_some() && visits(&cache) > 0;
    if !open {
        if state.slots.iter().any(Option::is_some) || state.light.is_some() {
            for slot in state.slots.drain(..).flatten() {
                for e in [Some(slot.camera), Some(slot.root), slot.pedestal].into_iter().flatten() {
                    if let Ok(mut entity) = commands.get_entity(e) {
                        entity.despawn();
                    }
                }
            }
            if let Some(light) = state.light.take() {
                if let Ok(mut entity) = commands.get_entity(light) {
                    entity.despawn();
                }
            }
            state.gamma.clear();
        }
        return;
    }
    let (Some(visuals), Some(body)) = (visuals.as_deref(), body) else {
        return;
    };
    let layer = RenderLayers::layer(LAYER);
    if state.light.is_none() {
        state.light = Some(
            commands
                .spawn((
                    GenerationScene,
                    DirectionalLight {
                        color: Color::WHITE,
                        illuminance: scene::SUN_ILLUMINANCE,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(Vec3::new(-0.75, 1.0, 0.1).normalize() * 20.0).looking_at(Vec3::ZERO, Vec3::Y),
                    layer.clone(),
                    StageOwned,
                ))
                .id(),
        );
    }
    // A rewritten body mesh (the Creator's edit) invalidates the baked-body scenes: rebuild everything.
    if state.revision != visuals.body_revision {
        state.revision = visuals.body_revision;
        for slot in state.slots.drain(..).flatten() {
            for e in [Some(slot.camera), Some(slot.root), slot.pedestal].into_iter().flatten() {
                if let Ok(mut entity) = commands.get_entity(e) {
                    entity.despawn();
                }
            }
        }
        state.gamma.clear();
    }
    let shown = visits(&cache);
    let wanted = shown + 1;
    if state.slots.len() < wanted {
        state.slots.resize_with(wanted, || None);
    }
    // One scene per frame (the skinning of an edited chain costs a few ms).
    let next = (0..wanted).find(|&n| state.slots[n].is_none() && record_of(&run, shown, n).is_some());
    if let Some(n) = next {
        let record = record_of(&run, shown, n).expect("checked above");
        let offset = Vec3::new(SPACING * (n as f32 + 1.0), 0.0, 0.0);
        let image = images.add(Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None));
        // The body: the baked one for the neutral chain, else the chain skinned now (`cell_body::build`) in the
        // body's own frame (`render_mesh`).
        let mut bones: Option<Vec<u8>> = None;
        let body_mesh = match (&record.spine, &body_data, visuals.body_fit) {
            (Some(chain), Some(data), Some(fit)) if !chain.is_empty() => cell_body::build(chain, data.resolution)
                .map(|skin| {
                    bones = Some(skin.bones.clone());
                    meshes.add(cell_body::render_mesh(&skin, &BodyFrame { fit }))
                })
                .unwrap_or_else(|| body.draw_mesh()),
            _ => visuals.baked_mesh.clone().map(|m| meshes.add(m)).unwrap_or_else(|| body.draw_mesh()),
        };
        // A creature that is not the live one (another paint or body chain) is painted with its own paint.
        let live_paint = &run.cell_creation.paint;
        let paint = record.paint.clone().unwrap_or_else(|| live_paint.clone());
        let replay = paint != *live_paint || record.spine != run.cell_creation.spine;
        let mut painter = std::mem::take(&mut state.painter);
        let effective = replay.then(|| {
            let neutral = body_data.as_ref().and_then(|d| d.neutral_paint);
            painter.effective(&originals, &paint, neutral)
        });
        let root = commands
            .spawn((
                GenerationScene,
                Transform::from_translation(offset).with_scale(Vec3::splat(scene::SUBJECT_SCALE)),
                Visibility::default(),
                layer.clone(),
                StageOwned,
            ))
            .id();
        let mut gamma = std::mem::take(&mut state.gamma);
        let mut boxes = Vec::new();
        commands.entity(root).with_children(|p| {
            if let Some(material) = body.material.as_ref() {
                boxes.extend(mesh_box(&meshes, &body_mesh, &Transform::default()));
                let own = effective.as_ref().and_then(|e| {
                    painter.body_material(
                        &originals,
                        &e.body,
                        &body_mesh,
                        bones.as_deref(),
                        visuals.body_scale,
                        material,
                        &meshes,
                        &mut materials,
                        &mut images,
                    )
                });
                p.spawn((
                    Mesh3d(body_mesh.clone()),
                    MeshMaterial3d(scene::gamma_material(
                        &mut gamma,
                        own.as_ref().unwrap_or(material),
                        &mut materials,
                        &mut images,
                    )),
                    Transform::default(),
                    layer.clone(),
                ));
            }
            for (index, part) in record.parts.iter().enumerate() {
                let Some(model) = crate::editor::model_for_part(&originals, part) else {
                    continue;
                };
                let Some(material) = model.material.as_ref() else {
                    continue;
                };
                let own = effective.as_ref().and_then(|e| {
                    painter.part_material(
                        &originals,
                        &e.parts,
                        model,
                        &record.parts,
                        index,
                        visuals.body_scale,
                        &meshes,
                        &mut materials,
                        &mut images,
                    )
                });
                let material = own.as_ref().unwrap_or(material);
                let transform = scene::part_transform(part, model.scale);
                boxes.extend(mesh_box(&meshes, &model.draw_mesh(), &transform));
                p.spawn((
                    Mesh3d(model.draw_mesh()),
                    MeshMaterial3d(scene::gamma_material(&mut gamma, material, &mut materials, &mut images)),
                    transform,
                    layer.clone(),
                ));
            }
        });
        state.painter = painter;
        // The Creator's pedestal of the cell's level behind the cell, as in the portrait.
        let mut pedestal = None;
        let level = crate::cell_editor::editor_level(run.cell.food_progression);
        if let Some(key) = crate::cell_editor_data::level_pedestal(&originals.resource_index, level) {
            if let Ok((model, _)) =
                crate::cell_editor::import_pedestal(&mut originals, key, &mut meshes, &mut images, &mut materials)
            {
                let parent = commands
                    .spawn((GenerationScene, Transform::from_translation(offset), Visibility::default(), layer.clone(), StageOwned))
                    .id();
                commands.entity(parent).with_children(|p| {
                    for surface in &model.surfaces {
                        p.spawn((
                            crate::material_binding::OriginalSurfaceChild::of(surface),
                            Mesh3d(surface.handle.clone()),
                            MeshMaterial3d(scene::gamma_material(&mut gamma, &surface.material, &mut materials, &mut images)),
                            Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                            layer.clone(),
                        ));
                    }
                });
                pedestal = Some(parent);
            }
        }
        state.gamma = gamma;
        let (centre, size) = if boxes.is_empty() { (Vec2::ZERO, 2.0 * MIN_RADIUS) } else { framing(&boxes) };
        let (centre, size) = (centre * scene::SUBJECT_SCALE, size * scene::SUBJECT_SCALE);
        let at = Vec3::new(offset.x + centre.x, 20.0, offset.z + centre.y);
        let camera = commands
            .spawn((
                GenerationScene,
                Camera3d::default(),
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                Camera {
                    order: -4,
                    clear_color: ClearColorConfig::Custom(Color::srgb_u8(57, 64, 127)),
                    ..default()
                },
                RenderTarget::from(image.clone()),
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed { width: size, height: size },
                    ..OrthographicProjection::default_3d()
                }),
                // Straight down; world +X is screen up, so the cell's front (-Z) is on the left like in the Creator.
                Transform::from_translation(at).looking_at(Vec3::new(at.x, 0.0, at.z), Vec3::X),
                layer.clone(),
                AmbientLight { color: Color::WHITE, brightness: scene::AMBIENT_BRIGHTNESS, ..default() },
                StageOwned,
            ))
            .id();
        state.slots[n] = Some(Slot { image, camera, root, pedestal, frames: RENDER_FRAMES });
    }
    // Render a few frames after the build, then rest.
    for slot in state.slots.iter_mut().flatten() {
        if let Ok(mut camera) = cameras.get_mut(slot.camera) {
            let render = slot.frames > 0;
            if camera.is_active != render {
                camera.is_active = render;
            }
        }
        slot.frames = slot.frames.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_hud::timeline::events::TimelineEvent;

    fn run_with(visits: usize, generations: Vec<Option<GenerationRecord>>) -> RunProgress {
        let mut run = RunProgress::default();
        for _ in 0..visits {
            run.cell.timeline.push(TimelineEvent { kind: EventKind::EvolveInEditor, seconds: 0, value: 0.0 });
        }
        run.cell.generations = generations;
        run
    }

    #[test]
    fn a_new_visit_records_the_creature_the_editor_left() {
        let mut app = App::new();
        app.insert_resource(run_with(1, vec![]));
        app.add_systems(Update, record);
        app.update();
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.cell.generations.len(), 1);
        assert!(run.cell.generations[0].is_some());
        // A second visit adds one more; nothing changes while the count matches.
        app.world_mut().resource_mut::<RunProgress>().cell.timeline.push(TimelineEvent {
            kind: EventKind::EvolveInEditor,
            seconds: 9,
            value: 0.2,
        });
        app.update();
        app.update();
        assert_eq!(app.world().resource::<RunProgress>().cell.generations.len(), 2);
    }

    /// A record saved before the paint was kept loads with `paint: None` (the thumbnail then shows the live paint);
    /// one written now keeps its paint through a save.
    #[test]
    fn records_keep_their_paint_and_old_ones_load_without() {
        let old: GenerationRecord = serde_json::from_str(r#"{"parts": [], "spine": null}"#).unwrap();
        assert_eq!(old.paint, None);
        let mut run = RunProgress::default();
        run.cell_creation.paint = crate::cell_paint::CellPaint { theme: 5, color: [0.1, 0.2, 0.3] };
        let json = serde_json::to_string(&live(&run)).unwrap();
        let back: GenerationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.paint, Some(run.cell_creation.paint.clone()));
        assert_eq!(GenerationRecord::neutral().paint, Some(crate::cell_paint::CellPaint::default()));
    }

    #[test]
    fn a_save_without_the_record_fills_only_the_newest_visit() {
        let mut app = App::new();
        app.insert_resource(run_with(3, vec![]));
        app.add_systems(Update, record);
        app.update();
        let run = app.world().resource::<RunProgress>();
        assert_eq!(run.cell.generations.len(), 3);
        assert!(run.cell.generations[0].is_none() && run.cell.generations[1].is_none() && run.cell.generations[2].is_some());
    }

    #[test]
    fn cards_show_the_recorded_creature_the_live_one_for_the_newest_gap_and_the_neutral_cell_first() {
        let rec = GenerationRecord { parts: vec![], spine: Some(vec![]), paint: None };
        let run = run_with(3, vec![Some(rec.clone()), None, None]);
        assert_eq!(record_of(&run, 3, 0), Some(GenerationRecord::neutral()), "the first card: the neutral cell");
        assert_eq!(record_of(&run, 3, 1), Some(rec), "a recorded visit");
        assert_eq!(record_of(&run, 3, 2), None, "an older visit without a record stays empty");
        assert!(record_of(&run, 3, 3).is_some(), "the newest visit without a record takes the live creature");
    }
}
