//! Bevy side of the retail creature skin (`creature_skin`) and skin paint
//! (`creature_skin_paint`): the player's body mesh in the creators and the
//! stages, re-meshed while editing and painted from the creation's paint.
//!
//! The editable body stays `editor.rs`'s `BodyProfile` (spine control points
//! with radii in the player's frame) over the installed starter creature. The
//! skin's vertebrae are the starter's own vertebra records (its
//! `.creaturedata`, the exact retail skin inputs: positions, scales, ball
//! offsets), fitted into the player's frame like its GMDL and moved / scaled
//! by the edited spine exactly as the previous body's vertices were
//! (`BodyTemplate::deform`, `baked_model::radius_ratio`). So an untouched
//! spine gives the starter's retail torso skin, and parts, handles and slots
//! keep their places. Without the installed starter, each spine point becomes
//! a vertebra centred on it (head first). ADAPTED: the unit between the
//! player's frame and creature space is chosen so a scale-1 vertebra's visible
//! radius is `creature_editor_edit::VERTEBRA_UNIT_RADIUS` (only the
//! resolution and the paint's brush sizes depend on it).
//!
//! Switch: `OPENSPORE_CREATURE_SKIN=0` keeps the previous body (the installed
//! starter's GMDL torso deformed along the spine, `baked_model`).
//!
//! Performance (retail FUN_004CC5B0 behaviour, simplified): while a spine
//! manipulator is held, or edits follow each other within [`IDLE_SECONDS`],
//! the skin is meshed at `editorSkinResolutionRealtime` 0.042; once idle it is
//! re-meshed at the high-quality 0.032 and painted (paint only regenerates when
//! the high-quality mesh or the paint changes).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{
    body::BodyProfile,
    creature_editor::spore_to_bevy,
    creature_editor_edit::VERTEBRA_UNIT_RADIUS,
    creature_skin::{self as skin, BlockKind, SkinRecord},
    creature_skin_paint as paint,
    dbpf::ResourceKey,
    game::RunProgress,
    OriginalModel, OriginalSurface, Originals,
};

/// ADAPTED: edits closer together than this count as one drag (coarse mesh).
pub const IDLE_SECONDS: f32 = 0.25;

/// The skin replaces the deformed starter torso unless
/// `OPENSPORE_CREATURE_SKIN=0`.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    !SUPPRESSED.with(|s| s.get()) && *ON.get_or_init(|| std::env::var("OPENSPORE_CREATURE_SKIN").map_or(true, |v| v != "0"))
}

thread_local! {
    static SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `f` with the skin switched off on this thread (tests of the previous
/// body path).
#[cfg_attr(not(test), allow(dead_code))]
pub fn without_skin<R>(f: impl FnOnce() -> R) -> R {
    SUPPRESSED.with(|s| s.set(true));
    let r = f();
    SUPPRESSED.with(|s| s.set(false));
    r
}

/// Player-frame units per creature unit (see the module comment).
pub fn unit() -> f32 {
    VERTEBRA_UNIT_RADIUS / (skin::VERTEBRA_RADIUS_FACTOR * skin::iso_factor(1.0))
}

/// Player frame -> creature space (the basis change is its own inverse).
pub fn to_creature(p: Vec3) -> Vec3 {
    spore_to_bevy(p) / unit()
}

/// Creature space -> player frame.
pub fn to_player(v: Vec3) -> Vec3 {
    spore_to_bevy(v) * unit()
}

/// Vertebra records of a body profile (head first).
pub fn records_from_profile(profile: &BodyProfile) -> Vec<SkinRecord> {
    let body = profile.validated();
    let pts: Vec<(Vec3, f32)> = body
        .spine
        .iter()
        .rev()
        .map(|p| (to_creature(Vec3::from_array(p.position)), (p.radius / VERTEBRA_UNIT_RADIUS).max(0.01)))
        .collect();
    let n = pts.len();
    let mut blocks = Vec::with_capacity(n);
    for i in 0..n {
        let ahead = pts[i.saturating_sub(1)].0;
        let behind = pts[(i + 1).min(n - 1)].0;
        let along = (behind - ahead).normalize_or(Vec3::Y);
        let side = (Vec3::X - along * along.x).normalize_or(Vec3::X);
        let up = side.cross(along);
        let (centre, scale) = pts[i];
        blocks.push(SkinRecord {
            kind: BlockKind::Vertebra,
            flags: skin::flag::USES_SKIN,
            scale,
            orientation: [side, along, up],
            position: centre - up * (skin::VERTEBRA_BALL_OFFSET * scale),
            parent: i as i16 - 1,
            ..Default::default()
        });
    }
    skin::order_records(blocks)
}

/// A starter vertebra ball in the fitted player frame (rest pose).
#[derive(Clone, Debug, PartialEq)]
pub struct NativeBall {
    pub centre: Vec3,
    /// Support radius `R` (player units).
    pub radius: f32,
    /// Position along the native torso (0 tail .. 1 head), as `BodyTemplate`.
    pub t: f32,
}

/// The vertebra balls the skin builds for a profile of `points` points (head
/// first): the starter's own balls, or, for a longer spine (a Maxis creation,
/// vertebrae added past the starter's), one ball per profile point resampled
/// along them (centre and radius interpolated by `t`), so the skin keeps the
/// spine's ball count (retail: one ball per vertebra). ADAPTED: the resampled
/// balls follow the starter's rest shape before the profile deforms them.
pub fn native_samples(native: &[NativeBall], points: usize) -> Vec<NativeBall> {
    if points <= native.len() || native.len() < 2 {
        return native.to_vec();
    }
    let mut sorted = native.to_vec();
    sorted.sort_by(|a, b| a.t.total_cmp(&b.t));
    (0..points)
        .map(|k| {
            let t = 1.0 - k as f32 / (points - 1) as f32;
            let i = sorted.iter().position(|b| b.t >= t).unwrap_or(sorted.len() - 1).max(1);
            let (a, b) = (&sorted[i - 1], &sorted[i]);
            let u = if b.t - a.t > 1e-6 { ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0) } else { 0.0 };
            NativeBall { centre: a.centre.lerp(b.centre, u), radius: a.radius + (b.radius - a.radius) * u, t }
        })
        .collect()
}

/// The starter's vertebra balls (`.creaturedata` of the baseline creature)
/// fitted like its GMDL: the fit (uniform scale + offset) is solved from the
/// template's fitted vertebra positions.
fn native_balls(template: &crate::baked_model::BodyTemplate, index: &crate::asset_catalog::ResourceIndex) -> (Vec<NativeBall>, f32) {
    let none = (Vec::new(), unit());
    let Ok(bytes) = index.read(ResourceKey {
        group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
        instance_id: crate::baked_model::PLAYER_BASELINE_INSTANCE,
        type_id: crate::creature_data::TYPE,
    }) else {
        return none;
    };
    let Ok(data) = crate::creature_data::parse(&bytes) else { return none };
    let records = skin::records_from_creature_data(&data);
    let vertebrae: Vec<&SkinRecord> = records.iter().filter(|r| r.kind == BlockKind::Vertebra).collect();
    let rest = &template.rest_spine().spine;
    let n = vertebrae.len();
    if n < 2 || rest.len() != n {
        return none;
    }
    // rest[n - 1 - i] = fit(spore_to_bevy(position of native vertebra i)).
    let (pa, pb) = (spore_to_bevy(vertebrae[0].position), spore_to_bevy(vertebrae[n - 1].position));
    let (ra, rb) = (Vec3::from_array(rest[n - 1].position), Vec3::from_array(rest[0].position));
    let native_len = pa.distance(pb);
    if native_len < 1e-6 {
        return none;
    }
    let scale = ra.distance(rb) / native_len;
    let offset = ra - pa * scale;
    let balls = vertebrae
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let (c, r) = v.vertebra_ball();
            NativeBall { centre: spore_to_bevy(c) * scale + offset, radius: r * scale, t: 1.0 - i as f32 / (n - 1) as f32 }
        })
        .collect();
    (balls, scale)
}

/// The skin's vertebra records for `profile` and the player units per
/// creature unit they use: the starter's balls moved with the spine when the
/// template carries them (in the starter's own units, so the retail
/// resolutions and brush sizes apply unchanged), else the profile's points.
pub fn skin_records(template: Option<&crate::baked_model::BodyTemplate>, profile: &BodyProfile) -> (Vec<SkinRecord>, f32) {
    skin_records_with(template, profile, false)
}

/// [`skin_records`] plus, when `limbs`, the player's limb segments
/// (`creature_limbs`; the visible skin has them, slots and picking use the
/// torso alone).
pub fn skin_records_with(template: Option<&crate::baked_model::BodyTemplate>, profile: &BodyProfile, limbs: bool) -> (Vec<SkinRecord>, f32) {
    let with_limbs = |mut blocks: Vec<SkinRecord>, units: f32| {
        if limbs {
            let extra = crate::creature_limbs::skin_limb_records(&blocks, units);
            blocks.extend(extra);
        }
        blocks
    };
    let native = template.and_then(|t| t.skin.as_ref().filter(|s| !s.native.is_empty()).map(|s| (t, &s.native, s.unit)));
    let Some((template, balls, units)) = native else {
        let records = with_limbs(records_from_profile(profile), unit());
        return (if limbs { skin::order_records(records) } else { records }, unit());
    };
    let body = profile.validated();
    let balls = native_samples(balls, body.spine.len());
    let blocks = balls
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let centre = spore_to_bevy(template.deform(b.centre, b.t, &body)) / units;
            let radius = b.radius * crate::baked_model::radius_ratio(&body, b.t) / units;
            let scale = radius / skin::VERTEBRA_RADIUS_FACTOR;
            SkinRecord {
                kind: BlockKind::Vertebra,
                flags: skin::flag::USES_SKIN,
                scale,
                position: centre - Vec3::Z * (skin::VERTEBRA_BALL_OFFSET * scale),
                parent: i as i16 - 1,
                ..Default::default()
            }
        })
        .collect();
    (skin::order_records(with_limbs(blocks, units)), units)
}

fn template_of(originals: &Originals) -> Option<&crate::baked_model::BodyTemplate> {
    originals.baked_body.as_ref()
}

/// One built skin (pure data, shared by every app in the process).
#[allow(dead_code)]
pub struct Built {
    pub profile: BodyProfile,
    /// Player units per creature unit of this skin.
    pub unit: f32,
    pub resolution: f32,
    pub generation: u64,
    pub records: Vec<SkinRecord>,
    pub prims: skin::Primitives,
    pub mesh: skin::SkinMesh,
    pub unwrapped: skin::Unwrapped,
    texels: OnceLock<paint::TexelMap>,
}

impl Built {
    pub fn texel_map(&self) -> &paint::TexelMap {
        self.texels.get_or_init(|| paint::texel_map(&self.prims, &self.unwrapped))
    }
}

#[derive(Default)]
struct Cache {
    /// Most recent first.
    built: Vec<Arc<Built>>,
    generation: u64,
    last_change: Option<Instant>,
    last_profile: Option<BodyProfile>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
static DRAGGING: AtomicBool = AtomicBool::new(false);

fn with_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Cache::default))
}

/// The skin of `profile` at `res` (cached by its records; the last four
/// builds are kept).
pub fn build(template: Option<&crate::baked_model::BodyTemplate>, profile: &BodyProfile, res: f32) -> Option<Arc<Built>> {
    build_records(skin_records_with(template, profile, true), profile, res)
}

fn build_records((records, units): (Vec<SkinRecord>, f32), profile: &BodyProfile, res: f32) -> Option<Arc<Built>> {
    if let Some(b) = with_cache(|c| c.built.iter().find(|b| b.resolution == res && b.unit == units && b.records == records).cloned()) {
        return Some(b);
    }
    let started = Instant::now();
    let (prims, mesh) = match skin::build(&records, res) {
        Ok(v) => v,
        Err(e) => {
            warn!("creature skin: {e:?}");
            return None;
        }
    };
    let unwrapped = skin::unwrap(&prims, &mesh);
    let ms = started.elapsed().as_secs_f32() * 1000.0;
    debug!("creature skin: {} vertices, {} triangles at {:.3} in {ms:.1} ms", mesh.positions.len(), mesh.triangles.len(), mesh.resolution);
    Some(with_cache(|c| {
        c.generation += 1;
        let b = Arc::new(Built {
            profile: profile.clone(),
            unit: units,
            resolution: res,
            generation: c.generation,
            records,
            prims,
            mesh,
            unwrapped,
            texels: OnceLock::new(),
        });
        c.built.insert(0, b.clone());
        c.built.truncate(4);
        b
    }))
}

/// Any cached skin of `profile`, else a new high-quality one.
pub fn any_for(template: Option<&crate::baked_model::BodyTemplate>, profile: &BodyProfile) -> Option<Arc<Built>> {
    let (records, units) = skin_records(template, profile);
    with_cache(|c| c.built.iter().find(|b| b.unit == units && b.records == records).cloned())
        .or_else(|| build_records((records, units), profile, skin::RES_HIGH_QUALITY))
}

fn idle() -> bool {
    !DRAGGING.load(Ordering::Relaxed) && with_cache(|c| c.last_change.is_none_or(|t| t.elapsed().as_secs_f32() >= IDLE_SECONDS))
}

/// The resolution for an edit to `profile`: coarse while dragging or when it
/// follows the previous edit closely, fine otherwise. Records the edit time.
fn edit_resolution(profile: &BodyProfile) -> f32 {
    let dragging = DRAGGING.load(Ordering::Relaxed);
    with_cache(|c| {
        let changed = c.last_profile.as_ref() != Some(profile);
        let recent = c.last_change.is_some_and(|t| t.elapsed().as_secs_f32() < IDLE_SECONDS);
        if changed {
            if c.last_profile.is_some() {
                c.last_change = Some(Instant::now());
            }
            c.last_profile = Some(profile.clone());
        }
        if dragging || (changed && recent) {
            skin::RES_REALTIME
        } else {
            // Settings > Graphics > Player Model Quality (front-end-4).
            crate::front_end::graphics::skin_resolution()
        }
    })
}

/// The Bevy mesh of a built skin in the player's frame (UV0 = paint texture).
pub fn render_mesh(b: &Built) -> Mesh {
    let un = &b.unwrapped;
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, un.positions.iter().map(|p| (spore_to_bevy(*p) * b.unit).to_array()).collect::<Vec<_>>());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        un.normals.iter().map(|n| spore_to_bevy(*n).normalize_or(Vec3::Y).to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, un.uvs.clone());
    // The basis change (-x, z, y) is a rotation: the winding stays
    // counter-clockwise seen from outside.
    mesh.insert_indices(Indices::U32(un.triangles.iter().flat_map(|t| *t).collect()));
    if mesh.generate_tangents().is_err() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; un.positions.len()]);
    }
    mesh
}

/// The skin's render handles, kept with the body template (per app).
#[derive(Clone, Debug)]
pub struct SkinTarget {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub diffuse: Handle<Image>,
    pub normal: Handle<Image>,
    /// The starter's vertebra balls (empty: build from the profile's points).
    pub native: Vec<NativeBall>,
    /// Player units per starter unit (the GMDL fit's scale).
    pub unit: f32,
}

fn paint_image(fill: [u8; 4], format: TextureFormat) -> Image {
    let size = skin::PAINT_SIZE;
    Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        fill.iter().copied().cycle().take((size * size * 4) as usize).collect(),
        format,
        RenderAssetUsages::default(),
    )
}

/// Hook for `baked_model::prepare_body`: give the body template the skin's
/// own mesh, paint textures and material (one surface).
pub fn adopt_body(
    template: &mut crate::baked_model::BodyTemplate,
    index: &crate::asset_catalog::ResourceIndex,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    if template.skin.is_some() {
        return;
    }
    let mesh = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()));
    let diffuse = images.add(paint_image([150, 170, 120, 255], TextureFormat::Rgba8UnormSrgb));
    let normal = images.add(paint_image([128, 128, 255, 255], TextureFormat::Rgba8Unorm));
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(diffuse.clone()),
        normal_map_texture: Some(normal.clone()),
        perceptual_roughness: 0.75,
        reflectance: 0.25,
        ..default()
    });
    let old = &template.model;
    template.model = OriginalModel {
        source_definition: old.source_definition,
        material: Some(material.clone()),
        handle: mesh.clone(),
        surfaces: vec![OriginalSurface { handle: mesh.clone(), material: material.clone(), source_section: 0, state_sections: Vec::new() }],
        scale: old.scale,
        instance: old.instance,
        root: None,
        rig_bones: 0,
    };
    let (native, unit) = native_balls(template, index);
    template.skin = Some(SkinTarget { mesh, material, diffuse, normal, native, unit });
}

/// Hook for `baked_model::update_body`: re-mesh the skin for `profile`.
pub fn update_body(template: &crate::baked_model::BodyTemplate, profile: &BodyProfile, meshes: &mut Assets<Mesh>) {
    let Some(target) = &template.skin else { return };
    let res = edit_resolution(profile);
    if let Some(b) = build(Some(template), profile, res) {
        if let Some(mut m) = meshes.get_mut(&target.mesh) {
            *m = render_mesh(&b);
        }
    }
}

/// The closest skin point to `point` (player frame): Newton projection on the
/// field when the point is inside a primitive's support, else the nearest
/// vertex.
pub fn nearest_surface(originals: &Originals, profile: &BodyProfile, point: Vec3) -> Vec3 {
    let Some(b) = any_for(template_of(originals), profile) else {
        return crate::body::nearest_surface(profile, point);
    };
    let q = spore_to_bevy(point) / b.unit;
    if b.prims.field.gradient(q).length_squared() > 1e-10 {
        let (p, _) = b.prims.field.project(q);
        if p.is_finite() && (p - q).length() < 1.0 {
            return spore_to_bevy(p) * b.unit;
        }
    }
    b.mesh
        .positions
        .iter()
        .min_by(|a, c| a.distance_squared(q).total_cmp(&c.distance_squared(q)))
        .map_or(point, |p| spore_to_bevy(*p) * b.unit)
}

/// Part attachment slot `bone` on the skin (the previous body's anchor, or the
/// profile's slot, moved onto the surface).
pub fn attachment_point(originals: &Originals, profile: &BodyProfile, bone: u32) -> Vec3 {
    let anchor = template_of(originals).map_or_else(|| profile.slot(bone), |t| t.attachment(profile, bone));
    nearest_surface(originals, profile, anchor)
}

/// The skin's triangles in the player's frame (counter-clockwise), for picking.
pub fn body_triangles(originals: &Originals, profile: &BodyProfile) -> Vec<[Vec3; 3]> {
    let Some(b) = any_for(template_of(originals), profile) else { return Vec::new() };
    b.mesh
        .triangles
        .iter()
        .map(|t| t.map(|i| spore_to_bevy(b.mesh.positions[i as usize]) * b.unit))
        .collect()
}

/// Whether `material` is the skin's own (painted by this module, not by the
/// part tint of `creature_editor_paint`).
pub fn is_skin_material(originals: &Originals, material: &Handle<StandardMaterial>) -> bool {
    originals.baked_body.as_ref().and_then(|b| b.skin.as_ref()).is_some_and(|s| s.material.id() == material.id())
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

pub struct CreatureSkinPlugin;

impl Plugin for CreatureSkinPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.add_systems(Update, (capture_demo, track_drag).chain().before(crate::editor::refresh_parts))
            .add_systems(Update, (refine, paint_skin).chain().after(crate::editor::refresh_parts));
    }
}

/// Capture option `--skin-demo`: in a land stage, fatten the middle vertebra,
/// add one at the tail and paint the creature with the zebroid base style, so
/// a quick capture shows a custom, painted skin.
fn capture_demo(state: Option<Res<State<crate::stages::GameStage>>>, run: Option<ResMut<RunProgress>>, mut done: Local<bool>) {
    use crate::stages::GameStage;
    if *done || !std::env::args().any(|a| a == "--skin-demo") {
        return;
    }
    let (Some(state), Some(mut run)) = (state, run) else { return };
    if !matches!(state.get(), GameStage::Creature | GameStage::Tribal) {
        return;
    }
    *done = true;
    let n = run.body.spine.len();
    crate::creature_editor_edit::scale_vertebra(&mut run.body, n / 2, 5.0);
    run.body.edit_spine(|spine| {
        crate::creature_editor_edit::drag_end(spine, false, 10.0, None, crate::creature_editor_edit::MAX_VERTEBRAE)
            == crate::creature_editor_edit::EndEdit::Added
    });
    let paint = &mut run.creature_creation.paint;
    paint.painted = true;
    paint.brushes = [
        crate::world_objects::name_id("skinpaint_base_zebroid"),
        crate::world_objects::name_id("skinpaint_coat_blank"),
        crate::world_objects::name_id("skinpaint_detail_blank"),
    ];
    paint.colors = [[0.85, 0.62, 0.3], [0.35, 0.22, 0.12], [1.0, 1.0, 1.0]];
}

/// A spine manipulator holds the body: mesh coarse until it lets go.
fn track_drag(editor: Option<Res<crate::creature_editor::CreatureEditor>>, state: Option<Res<crate::editor::EditorState>>) {
    let held = editor.is_some_and(|e| e.manipulating) || state.is_some_and(|s| s.body_editing() && s.dragging());
    DRAGGING.store(held, Ordering::Relaxed);
}

/// Once idle, replace a coarse mesh by the high-quality one.
fn refine(run: Option<Res<RunProgress>>, originals: Option<Res<Originals>>, mut meshes: ResMut<Assets<Mesh>>, mut written: Local<Option<(AssetId<Mesh>, u64)>>) {
    let (Some(run), Some(originals)) = (run, originals) else { return };
    let Some(target) = originals.baked_body.as_ref().and_then(|b| b.skin.clone()) else { return };
    if !idle() {
        return;
    }
    let Some(b) = build(template_of(&originals), &run.body, crate::front_end::graphics::skin_resolution()) else { return };
    if *written == Some((target.mesh.id(), b.generation)) {
        return;
    }
    if let Some(mut m) = meshes.get_mut(&target.mesh) {
        *m = render_mesh(&b);
        *written = Some((target.mesh.id(), b.generation));
    }
}

/// The paint a creature shows: the creation's layers (defaults for unpainted
/// layers: `editorDefaultSkinPaintScripts`, else blank matte / blank coat /
/// blank detail) with its colours (the installed starter's while unpainted).
pub fn effective_paint(
    creation: &crate::creature_editor_paint::CreaturePaint,
    defaults: [u32; 3],
    native: [[u8; 3]; 3],
) -> ([u32; 3], [[f32; 3]; 3]) {
    let fallback = [
        crate::world_objects::name_id("skinpaint_base_blankMatte"),
        crate::world_objects::name_id("skinpaint_coat_blank"),
        crate::world_objects::name_id("skinpaint_detail_blank"),
    ];
    let brushes = std::array::from_fn(|i| {
        if creation.painted && creation.brushes[i] != 0 {
            creation.brushes[i]
        } else if defaults[i] != 0 {
            defaults[i]
        } else {
            fallback[i]
        }
    });
    let colors = if creation.painted { creation.colors } else { native.map(|c| c.map(|v| v as f32 / 255.0)) };
    (brushes, colors)
}

/// Paint a built skin with a creation's paint, as `paint_skin` paints the
/// player's skin (for offscreen renders such as the Sporepedia thumbnails,
/// `front_end::thumbs`): the diffuse and normal maps (PAINT_SIZE^2 RGBA8).
pub fn paint_built(
    originals: &Originals,
    built: &Built,
    creation: &crate::creature_editor_paint::CreaturePaint,
    defaults: [u32; 3],
) -> (Vec<u8>, Vec<u8>) {
    let native = crate::creature_editor_paint::native_skin_colors(originals);
    let (ids, colors) = effective_paint(creation, defaults, native);
    let scripts: [Option<Arc<paint::Script>>; 3] = std::array::from_fn(|i| load_script(&originals.resource_index, ids[i]));
    let mut brushes = HashMap::new();
    for s in scripts.iter().flatten() {
        for name in s.brushes() {
            if !brushes.contains_key(&name) {
                let brush = load_brush(&originals.resource_index, &name);
                brushes.insert(name, brush);
            }
        }
    }
    let job = paint::PaintJob {
        scripts: std::array::from_fn(|i| scripts[i].as_deref().map(|s| (s, ids[i]))),
        colors,
        seeds: creation.effective_seeds(),
        brushes: &brushes,
    };
    let painted = paint::paint(&built.prims, built.texel_map(), &job);
    (painted.diffuse, painted.normal)
}

#[derive(Default)]
struct PaintState {
    scripts: HashMap<u32, Option<Arc<paint::Script>>>,
    brushes: HashMap<String, paint::Brush>,
    native: Option<[[u8; 3]; 3]>,
    painted: Option<(u64, [u32; 3], [[u32; 3]; 3], [u32; 3], AssetId<StandardMaterial>)>,
}

fn load_script(index: &crate::asset_catalog::ResourceIndex, id: u32) -> Option<Arc<paint::Script>> {
    let bytes = index.read(ResourceKey { type_id: paint::PFX_TYPE, group_id: paint::PFX_GROUP, instance_id: id }).ok()?;
    Some(Arc::new(paint::parse(&String::from_utf8_lossy(&bytes))))
}

fn load_brush(index: &crate::asset_catalog::ResourceIndex, name: &str) -> paint::Brush {
    let tex = |suffix: &str| {
        let key = ResourceKey {
            type_id: paint::RASTER_TYPE,
            group_id: paint::BRUSH_GROUP,
            instance_id: crate::world_objects::name_id(&format!("{name}{suffix}")),
        };
        index
            .read(key)
            .ok()
            .and_then(|b| crate::texture::decode_rw4(&b))
            .map(|p| paint::BrushTex { width: p.width, height: p.height, rgba: p.rgba })
    };
    paint::Brush { diffuse: tex("__diffuse"), spec_bump: tex("__specBump") }
}

/// Paint the high-quality skin when it or the creature's paint changed.
fn paint_skin(
    run: Option<Res<RunProgress>>,
    originals: Option<Res<Originals>>,
    paint_data: Option<Res<crate::creature_editor_paint::CreaturePaintData>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<PaintState>,
) {
    let (Some(run), Some(originals)) = (run, originals) else { return };
    let Some(target) = originals.baked_body.as_ref().and_then(|b| b.skin.clone()) else { return };
    if !idle() {
        return;
    }
    let Some(b) = build(template_of(&originals), &run.body, crate::front_end::graphics::skin_resolution()) else { return };
    let native = *state.native.get_or_insert_with(|| crate::creature_editor_paint::native_skin_colors(&originals));
    let defaults = paint_data.map_or([0; 3], |d| d.defaults);
    let shown = crate::outfitter::shown_paint(&run);
    let (brushes, colors) = effective_paint(shown, defaults, native);
    let seeds = shown.effective_seeds();
    let key = (b.generation, brushes, colors.map(|c| c.map(f32::to_bits)), seeds, target.material.id());
    if state.painted == Some(key) {
        return;
    }
    let started = Instant::now();
    let mut scripts: [Option<Arc<paint::Script>>; 3] = [None, None, None];
    for (i, id) in brushes.iter().enumerate() {
        let s = state.scripts.entry(*id).or_insert_with(|| load_script(&originals.resource_index, *id)).clone();
        if let Some(s) = &s {
            for name in s.brushes() {
                if !state.brushes.contains_key(&name) {
                    let brush = load_brush(&originals.resource_index, &name);
                    state.brushes.insert(name, brush);
                }
            }
        }
        scripts[i] = s;
    }
    let job = paint::PaintJob {
        scripts: std::array::from_fn(|i| scripts[i].as_deref().map(|s| (s, brushes[i]))),
        colors,
        seeds,
        brushes: &state.brushes,
    };
    let painted = paint::paint(&b.prims, b.texel_map(), &job);
    if let Some(mut img) = images.get_mut(&target.diffuse) {
        img.data = Some(painted.diffuse);
    }
    if let Some(mut img) = images.get_mut(&target.normal) {
        img.data = Some(painted.normal);
    }
    if let Some(mut m) = materials.get_mut(&target.material) {
        // ADAPTED material mapping of the Skinner settings: gloss
        // (x `skinpaintGlossMultiplier` 0.7) -> smoothness, specular level ->
        // reflectance.
        m.perceptual_roughness = (1.0 - painted.settings.gloss * 0.7).clamp(0.3, 1.0);
        m.reflectance = (painted.spec * painted.settings.phong * 0.95 * 4.0).clamp(0.05, 0.6);
    }
    info!(
        "creature skin painted: {} stamps, {} texels, {:.0} ms",
        painted.stamps,
        b.texel_map().samples.len(),
        started.elapsed().as_secs_f32() * 1000.0
    );
    state.painted = Some(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_becomes_a_head_first_vertebra_chain() {
        let profile = BodyProfile::default();
        let records = records_from_profile(&profile);
        assert_eq!(records.len(), profile.spine.len());
        assert!(records.iter().all(|r| r.kind == BlockKind::Vertebra && r.uses_skin()));
        // Ball centres sit on the spine points (player frame), head first.
        for (r, p) in records.iter().zip(profile.spine.iter().rev()) {
            let (c, radius) = r.vertebra_ball();
            assert!(to_player(c).distance(Vec3::from_array(p.position)) < 1e-4);
            let visible = radius * skin::iso_factor(1.0) * unit();
            assert!((visible - p.radius).abs() < 1e-3 || radius == skin::VERTEBRA_MIN_RADIUS);
            // Local up is the creature's up for a straight horizontal spine.
            assert!(r.orientation[2].z > 0.8);
        }
        assert!(to_creature(Vec3::from_array(profile.spine.last().unwrap().position)).y < 0.0);
    }

    #[test]
    fn default_body_skin_mesh_and_render_mesh() {
        let profile = BodyProfile::default();
        let b = build(None, &profile, skin::RES_HIGH_QUALITY).unwrap();
        assert!(b.mesh.positions.len() > 300);
        let mesh = render_mesh(&b);
        assert_eq!(mesh.count_vertices(), b.unwrapped.positions.len());
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some());
        // Player-frame bounds follow the profile's spine and radii.
        let (min, max) = b.mesh.positions.iter().map(|p| to_player(*p)).fold((Vec3::MAX, Vec3::MIN), |(a, c), p| (a.min(p), c.max(p)));
        assert!(max.y > 0.8 && min.y < -0.2, "{min} {max}");
        assert!(min.z < -1.0 && max.z > 0.85, "{min} {max}");
        assert!((min.x + max.x).abs() < 0.1);
        // Picking triangles face outward in the player frame.
        let originals = Originals::default();
        let tris = body_triangles(&originals, &profile);
        let centre = Vec3::new(0.0, 0.3, 0.0);
        let outward = tris.iter().filter(|t| (t[1] - t[0]).cross(t[2] - t[0]).dot((t[0] + t[1] + t[2]) / 3.0 - centre) > 0.0).count();
        assert!(outward * 10 > tris.len() * 9, "{outward} of {}", tris.len());
        // Slots sit on the skin.
        for bone in 0..8 {
            let p = to_creature(attachment_point(&originals, &profile, bone));
            assert!(b.prims.field.value(p).abs() < 0.01, "slot {bone}");
        }
        // A coarse build has fewer vertices.
        let coarse = build(None, &profile, skin::RES_REALTIME).unwrap();
        assert!(coarse.mesh.positions.len() < b.mesh.positions.len());
    }

    /// Paint the default body with installed brush scripts; timings, and PNGs
    /// of the paint texture when `OPENSPORE_SKIN_DUMP` names a folder.
    #[test]
    fn installed_scripts_paint_the_default_body() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let profile = BodyProfile::default();
        let t = Instant::now();
        let b = build(None, &profile, skin::RES_HIGH_QUALITY).unwrap();
        let mesh_ms = t.elapsed().as_secs_f32() * 1000.0;
        let t = Instant::now();
        let texels = b.texel_map().samples.len();
        let map_ms = t.elapsed().as_secs_f32() * 1000.0;
        for (base, coat) in [("skinpaint_base_blankMatte", "skinpaint_coat_blank"), ("skinpaint_base_zebroid", "skinpaint_coat_blank")] {
            let ids = [crate::world_objects::name_id(base), crate::world_objects::name_id(coat)];
            let scripts: Vec<_> = ids.iter().map(|id| load_script(&index, *id).expect("installed script")).collect();
            let mut brushes = HashMap::new();
            for s in &scripts {
                for name in s.brushes() {
                    let brush = load_brush(&index, &name);
                    assert!(brush.diffuse.is_some(), "brush {name}");
                    brushes.insert(name, brush);
                }
            }
            let job = paint::PaintJob {
                scripts: [Some((&*scripts[0], ids[0])), Some((&*scripts[1], ids[1])), None],
                colors: [[0.75, 0.55, 0.3], [0.3, 0.25, 0.2], [1.0; 3]],
                seeds: paint::DEFAULT_SEEDS,
                brushes: &brushes,
            };
            let t = Instant::now();
            let painted = paint::paint(&b.prims, b.texel_map(), &job);
            let paint_ms = t.elapsed().as_secs_f32() * 1000.0;
            println!(
                "{base}: mesh {} verts {mesh_ms:.0} ms, texel map {texels} {map_ms:.0} ms, paint {} stamps {paint_ms:.0} ms",
                b.mesh.positions.len(),
                painted.stamps
            );
            assert!(painted.stamps > 100);
            let distinct: std::collections::HashSet<_> = painted.diffuse.chunks(4).map(|c| [c[0], c[1], c[2]]).collect();
            assert!(distinct.len() > 50, "{base}: {} colours", distinct.len());
            if let Ok(dir) = std::env::var("OPENSPORE_SKIN_DUMP") {
                let path = std::path::Path::new(&dir).join(format!("{base}.png"));
                let file = std::fs::File::create(path).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 512, 512);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&painted.diffuse).unwrap();
            }
        }
    }

    /// With the installed starter, the default spine gives the starter's own
    /// vertebra balls, and the skin covers the previous torso's extent.
    #[test]
    fn starter_vertebrae_drive_the_skin() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let profile = BodyProfile::default();
        let bounds = |tris: Vec<[Vec3; 3]>| tris.iter().flat_map(|t| t.iter().copied()).fold((Vec3::MAX, Vec3::MIN), |(a, b), q| (a.min(q), b.max(q)));
        let mut old = Originals::default();
        old.resource_index = index.clone();
        let (mut meshes, mut images, mut materials) = (Assets::<Mesh>::default(), Assets::<Image>::default(), Assets::<StandardMaterial>::default());
        if without_skin(|| crate::baked_model::prepare_body(&mut old, &profile, &mut meshes, &mut images, &mut materials)).is_err() {
            return;
        }
        let (old_min, old_max) = bounds(crate::baked_model::body_triangles(&old, &profile));
        let mut new = Originals::default();
        new.resource_index = index;
        let model = crate::baked_model::prepare_body(&mut new, &profile, &mut meshes, &mut images, &mut materials).unwrap();
        let target = new.baked_body.as_ref().unwrap().skin.clone().unwrap();
        assert_eq!(target.native.len(), 9);
        assert_eq!(model.surfaces.len(), 1);
        assert_eq!(model.material, Some(target.material.clone()));
        assert!(meshes.get(&model.draw_mesh()).unwrap().count_vertices() > 300);
        let (min, max) = bounds(crate::baked_model::body_triangles(&new, &profile));
        println!("previous torso {old_min} .. {old_max}; skin {min} .. {max}");
        assert!(min.cmpge(old_min - Vec3::splat(0.35)).all() && max.cmple(old_max + Vec3::splat(0.35)).all());
        assert!((max.y - min.y) > 0.6 * (old_max.y - old_min.y));
        // Slots stay on the skin; an edited spine moves it.
        let b = any_for(new.baked_body.as_ref(), &profile).unwrap();
        let slot = spore_to_bevy(attachment_point(&new, &profile, 3)) / b.unit;
        println!("starter unit {} (profile unit {}), {} vertices", b.unit, unit(), b.mesh.positions.len());
        assert!(b.prims.field.value(slot).abs() < 0.01);
        let mut fat = profile.clone();
        fat.spine[2].radius *= 1.4;
        let wider = any_for(new.baked_body.as_ref(), &fat).unwrap();
        let width = |b: &Built| b.mesh.positions.iter().map(|p| p.x.abs()).fold(0.0f32, f32::max);
        assert!(width(&wider) > width(&b));
    }

    #[test]
    fn effective_paint_uses_defaults_until_painted() {
        let mut p = crate::creature_editor_paint::CreaturePaint::default();
        let native = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];
        let (brushes, colors) = effective_paint(&p, [0; 3], native);
        assert_eq!(brushes[0], crate::world_objects::name_id("skinpaint_base_blankMatte"));
        assert_eq!(colors[0], [1.0, 0.0, 0.0]);
        p.painted = true;
        p.brushes = [7, 0, 9];
        p.colors = [[0.5; 3]; 3];
        let (brushes, colors) = effective_paint(&p, [1, 2, 3], native);
        assert_eq!(brushes, [7, 2, 9]);
        assert_eq!(colors[1], [0.5; 3]);
    }
}
