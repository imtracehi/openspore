//! The retail Creature Creator (part A): session, modes, editor camera, scene
//! (lighting, dais/background effects) and Test Drive.
//!
//! One `cEditor` engine runs every creator; what differs is the editor setup
//! PROP (`creature_editor_data`). The Creature stage opens
//! `CreatureEditorExtraLarge` after a mating (`EditorState::show`); a caller
//! may ask for another setup first with [`request_setup`] (the "Early
//! Creature Creator" `CellToCreatureEditor` after the Cell stage).
//!
//! The creature itself stays the in-world player entity with its parts (the
//! data model, part import, purchases, undo history and saving of
//! `editor.rs`, which keeps working for the stages this module does not
//! route). This module replaces, for the routed stages, the creator's UI
//! (`creature_editor_ui`, the retail `GlobalEditorUI` through SPUI), its
//! camera (the retail `EditorCamera` formulas, section 1.3 of
//! `notes/creator-re-notes.md`), its keyboard table (section 1.2), the scene
//! light (`editorLight`) and effects (`editorPlayModeExitEffectID` = the Build
//! dais look, `editorSkyBoxEffect`), and adds Test Drive (section 4).
//!
//! Edits go through `editor.rs`'s own operations: palette presses queue
//! `editor::Action::Buy` (the held press then carries the part onto the body
//! in `editor::drag`), Undo/Redo/Delete queue the matching actions. The
//! editing manipulators proper (placement, wheel scaling, spine, handles,
//! limbs) are `creator-b`'s; see `CHANGES.md` for the API.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
};
use serde::{Deserialize, Serialize};

use crate::{
    creature_editor_data::{
        CreatureEditorData, EditorCameraProps, PlayAnimButton, PlayAnimKind, SetupKind, ENVIRONMENT_REACTIONS,
        IDLE_ANIM, PLAY_ANIM_BUTTONS,
    },
    editor::{Action, EditorState},
    game::{Player, RunProgress},
    stages::GameStage,
    Originals,
};

/// Routing hook used by `editor.rs`: the stages whose creator this module
/// draws and drives. (Structure: the main-menu free creator without a creator
/// wheel session keeps the older `editor.rs` UI, same setup; no player-visible
/// difference.)
pub fn routes(stage: &GameStage) -> bool {
    *stage == GameStage::Creature
        || crate::outfitter::routes(stage)
        // An editor opened from the galaxy screen's creator wheel.
        || (*stage == GameStage::MainMenu && crate::front_end::creators::menu_session())
}

/// The creature as a creation: what the Creator saves besides its parts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CreatureCreation {
    /// Species name typed into the name field (empty = not named yet).
    pub name: String,
    /// Creator sessions accepted so far.
    pub visits: u32,
    /// Found parts already shown in a creator session (the others are
    /// highlighted as new; `creature_editor_edit::unlocks`).
    pub seen_parts: Vec<u32>,
    /// Paint mode result (`creature_editor_paint`).
    pub paint: crate::creature_editor_paint::CreaturePaint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Build,
    Paint,
    /// Test Drive.
    Play,
}

/// State restored by Cancel (exit without saving).
#[derive(Clone, Debug, PartialEq)]
pub struct EntrySnapshot {
    pub parts: Vec<crate::game::PartInstance>,
    pub body: crate::body::BodyProfile,
    pub dna: f32,
    pub name: String,
    pub paint: crate::creature_editor_paint::CreaturePaint,
}

impl EntrySnapshot {
    pub fn of(run: &RunProgress) -> Self {
        Self {
            parts: run.parts.clone(),
            body: run.body.clone(),
            dna: run.editor_dna,
            name: run.creature_creation.name.clone(),
            paint: run.creature_creation.paint.clone(),
        }
    }
    pub fn apply(&self, run: &mut RunProgress) {
        run.parts = self.parts.clone();
        run.body = self.body.clone();
        run.editor_dna = self.dna;
        run.creature_creation.name = self.name.clone();
        run.creature_creation.paint = self.paint.clone();
        run.sync_legacy_accessory();
    }
}

// ---------------------------------------------------------------------------
// Editor camera (EditorCamera, VERIFIED formulas)
// ---------------------------------------------------------------------------

/// `DAT_0150F2C8` = pi: drag rotation is pi * 0.0025 rad per pixel (times
/// `cameraRotateScale`).
pub const DRAG_RADIANS_PER_PIXEL: f32 = PI * 0.0025;
/// ADAPTED: `EditorCamera +0x8C`, the framing size of the offsets (not traced);
/// the setup's `editorBoundSize` (3) puts the creature right of the palette
/// as in the retail frames.
pub const DEFAULT_FRAMING: f32 = 3.0;

/// `EditorCamera::Update` 0x5A3E00: the look-at point moves toward its target by `s x 0.37` a frame.
const FOLLOW_RATE: f32 = 0.37;
/// ADAPTED: how far the cursor may move between the two clicks of a Test Drive double click.
const DOUBLE_CLICK_PIXELS: f32 = 8.0;

/// Whether a ground click at `now` on `cursor` is the second click of a double click, given the
/// previous click (time, cursor).
pub fn is_double_click(last: Option<(f32, Vec2)>, now: f32, cursor: Vec2) -> bool {
    last.is_some_and(|(at, spot)| (now - at) as f64 <= crate::creature_controls::DOUBLE_CLICK_SECS && spot.distance(cursor) <= DOUBLE_CLICK_PIXELS)
}

/// The editor camera: target values moved by input, smoothed values drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorRig {
    pub props: EditorCameraProps,
    /// Radians.
    pub heading: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub s_heading: f32,
    pub s_pitch: f32,
    pub s_zoom: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub s_offset_x: f32,
    pub s_offset_y: f32,
    /// The player turned or zoomed (retail `field_9C`).
    pub moved: bool,
}

impl Default for EditorRig {
    fn default() -> Self {
        Self::new(EditorCameraProps::default())
    }
}

fn angle_lerp(from: f32, to: f32, s: f32) -> f32 {
    let mut d = (to - from) % TAU;
    if d > PI {
        d -= TAU;
    } else if d < -PI {
        d += TAU;
    }
    from + d * s
}

impl EditorRig {
    pub fn new(props: EditorCameraProps) -> Self {
        let heading = props.heading.to_radians();
        let pitch = props.pitch.to_radians();
        Self {
            props,
            heading,
            pitch,
            zoom: props.zoom,
            s_heading: heading,
            s_pitch: pitch,
            s_zoom: props.zoom,
            offset_x: props.offset_x,
            offset_y: props.offset_y,
            s_offset_x: props.offset_x,
            s_offset_y: props.offset_y,
            moved: false,
        }
    }
    fn clamp_zoom(&self, z: f32) -> f32 {
        z.clamp(self.props.min_zoom, self.props.max_zoom.max(self.props.min_zoom))
    }
    /// Mouse drag (mode 0, `FUN_005A2BE0`): heading += K*dx, pitch -= K*dy.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        let k = DRAG_RADIANS_PER_PIXEL * self.props.rotate_scale;
        self.heading += k * dx;
        self.pitch = (self.pitch - k * dy)
            .clamp(self.props.min_pitch.to_radians(), self.props.max_pitch.to_radians());
        self.moved = true;
    }
    /// Drag with a modifier (mode 3): `zoom *= 1 - 4 * dy`.
    pub fn drag_zoom(&mut self, dy: f32) {
        let d = dy * 0.0025 * self.props.zoom_scale;
        self.zoom = self.clamp_zoom(self.zoom * (1.0 - 4.0 * d));
        self.moved = true;
    }
    /// Mouse wheel (`OnMouseWheel` 0x5A2560): `zoom *= 1 - delta * scale /
    /// 3000` with 120 per notch (4 % a notch).
    pub fn wheel(&mut self, delta: f32) {
        self.zoom = self.clamp_zoom(self.zoom * (1.0 - delta * self.props.zoom_scale / 3000.0));
        self.moved = true;
    }
    /// Keys per frame (`Update` 0x5A3E00): ',' / '.' heading -/+ 0.2 s,
    /// '-' / '=' zoom *= 1 +/- 0.05 s.
    pub fn keys(&mut self, heading_dir: f32, zoom_dir: f32, s: f32) {
        if heading_dir != 0.0 {
            self.heading += heading_dir * 0.2 * s;
            self.moved = true;
        }
        if zoom_dir != 0.0 {
            self.zoom = self.clamp_zoom(self.zoom * (1.0 + zoom_dir * 0.05 * s));
            self.moved = true;
        }
    }
    /// Smoothing factor of a frame: `clamp(dt_ms * 0.008, 0.01, 1)`.
    pub fn step(dt_secs: f32) -> f32 {
        (dt_secs * 1000.0 * 0.008).clamp(0.01, 1.0)
    }
    /// Smoothed values approach the targets; the vertical offset rises as the
    /// pitch flattens (`offsetY + (0.1 - |pitch| / (pi/2) * 0.5) * framing`).
    pub fn update(&mut self, s: f32, framing: f32) {
        let pitch_fraction = self.s_pitch.abs() / FRAC_PI_2;
        self.s_zoom += (self.zoom - self.s_zoom) * s;
        self.s_pitch = angle_lerp(self.s_pitch, self.pitch, s);
        self.s_heading = angle_lerp(self.s_heading, self.heading, s);
        self.s_offset_x += (self.offset_x - self.s_offset_x) * s;
        let y = self.offset_y + (0.1 - pitch_fraction * 0.5) * framing;
        self.s_offset_y += (y - self.s_offset_y) * s;
    }
    /// View in the creature's Spore frame (Z up, front -Y): the eye offset from
    /// the target and the camera basis (columns right, forward, up).
    /// `FUN_005A3070`: R = RotZ(-heading) * RotX(pitch); eye = target +
    /// R * (-(offX d k), -d, offY d k), k = 0.52 / framing.
    pub fn view(&self, framing: f32) -> (Vec3, Mat3) {
        let d = self.clamp_zoom(self.s_zoom);
        let k = 0.52 / framing.max(1e-3);
        let r = Mat3::from_rotation_z(-self.s_heading) * Mat3::from_rotation_x(self.s_pitch);
        (r * Vec3::new(-(self.s_offset_x * d * k), -d, self.s_offset_y * d * k), r)
    }
}

/// Spore model space (Z up, front -Y) -> the player's Bevy basis (the basis of
/// `baked_model::mesh`: (-x, z, y); front -Z).
pub fn spore_to_bevy(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, v.z, v.y)
}

/// The editor viewer's vertical field of view (radians) for the camera FOV
/// `fov` (radians) and the viewport aspect: `FUN_007C5400` (the projection
/// `EditorCamera` sets each frame from `mfInitialFOV` = FOV x 57.29578)
/// keeps the view window's aspect and sets its height to `0.75 x tan(fov / 2)`
/// unless the viewport is square. So the FOV is horizontal on a 4:3 screen and
/// the vertical angle stays the same on any other (VERIFIED code).
pub fn retail_vertical_fov(fov: f32, aspect: f32) -> f32 {
    let half = (fov * 0.5).tan();
    let h = if (aspect - 1.0).abs() > f32::EPSILON { half * 0.75 } else { half };
    2.0 * h.atan()
}

/// Player units per editor unit. The editor camera's zoom and near clip and
/// the scene effects are in the editor model's units = creature units, the
/// units of the skin (`creature_limbs::skin_units`); the rigid setups keep
/// player units. `OPENSPORE_EDITOR_PLAYER_UNITS=1` restores the earlier
/// player-unit camera (a debugging switch for before / after captures).
pub fn editor_units(originals: &Originals, kind: SetupKind) -> f32 {
    static PLAYER_UNITS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if kind.is_rigid() || *PLAYER_UNITS.get_or_init(|| std::env::var("OPENSPORE_EDITOR_PLAYER_UNITS").is_ok_and(|v| v == "1")) {
        return 1.0;
    }
    originals
        .baked_body
        .as_ref()
        .and_then(|t| t.skin.as_ref())
        .filter(|s| !s.native.is_empty())
        .map_or_else(crate::creature_skin_view::unit, |s| s.unit)
        .max(1e-3)
}

/// One frame of the editor's framing rule (`FUN_005A33B0`, from
/// `cEditor::Update` in Build / Paint while the player has not turned or
/// zoomed, VERIFIED): when some block's box is not fully inside the view the
/// target zoom grows by `framing x s x 0.2`; when every box is inside the view
/// at 0.9 x the FOV it shrinks by as much, not below the initial zoom; always
/// within [min, max] zoom. `s` = the camera's smoothing factor.
pub fn framing_zoom(zoom: f32, props: &EditorCameraProps, framing: f32, s: f32, outside: bool, inside_tight: bool) -> f32 {
    let step = framing * s * 0.2;
    let z = if outside {
        zoom + step
    } else if inside_tight {
        (zoom - step).max(props.zoom)
    } else {
        return zoom;
    };
    z.clamp(props.min_zoom, props.max_zoom.max(props.min_zoom))
}

/// `cFrustumCull::FrustumTestAABB` < 0 (fully inside) for a world box seen
/// from `camera` (Bevy camera transform, looking down -Z) with the vertical
/// half-angle tangent `tan_v`, viewport `aspect` and near distance.
pub fn box_inside(camera: &Transform, min: Vec3, max: Vec3, tan_v: f32, aspect: f32, near: f32) -> bool {
    let inv = camera.rotation.conjugate();
    (0..8).all(|i| {
        let p = Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        );
        let l = inv * (p - camera.translation);
        let depth = -l.z;
        depth >= near && l.y.abs() <= depth * tan_v && l.x.abs() <= depth * tan_v * aspect
    })
}

/// Camera transform for a rig around a creature at `target` with rotation
/// `frame` (the player's), `unit` world units per editor unit.
pub fn camera_transform(rig: &EditorRig, target: Vec3, frame: Quat, unit: f32, framing: f32) -> Transform {
    let (eye, r) = rig.view(framing);
    let right = frame * spore_to_bevy(r.x_axis);
    let forward = frame * spore_to_bevy(r.y_axis);
    let up = frame * spore_to_bevy(r.z_axis);
    Transform {
        translation: target + frame * spore_to_bevy(eye) * unit,
        rotation: Quat::from_mat3(&Mat3::from_cols(right, up, -forward)).normalize(),
        scale: Vec3::ONE,
    }
}

// ---------------------------------------------------------------------------
// Test Drive
// ---------------------------------------------------------------------------

/// An animation the creature is playing in Test Drive.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayAnim {
    /// TLSA request id.
    pub id: u32,
    pub kind: PlayAnimKind,
    pub elapsed: f32,
    pub duration: f32,
}

#[derive(Clone, Debug, Default)]
pub struct PlayState {
    pub anim: Option<PlayAnim>,
    pub sitting: bool,
    /// Index into the setup's backgrounds.
    pub background: usize,
    pub last_reaction: Option<u32>,
    /// Player transform when Test Drive started (restored on leaving).
    pub rest: Option<Transform>,
    /// ADAPTED walk: body-local target on the ground and run flag.
    pub walk_to: Option<(Vec3, bool)>,
    /// Test Drive camera: how far the look-at point has followed the creature (world frame).
    pub follow: Vec3,
    /// Time and cursor of the last ground click (a second click soon after, on the
    /// same spot, is a double click: the creature runs).
    pub last_click: Option<(f32, Vec2)>,
    pub walk_angle: f32,
    /// ADAPTED walk: body-local ground position reached so far (x, z).
    pub walk_angle_pos: [f32; 2],
    /// Panels: animations (true) or backgrounds.
    pub anims_panel: bool,
    pub seed: u32,
}

impl PlayState {
    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Seconds of an animation request: the first action list's TLSA choice
/// (sum of its clips), else the ANIM clip of that instance (the rule of
/// `cinematics::host::animation_seconds`); 2 s when neither resolves.
pub fn anim_seconds(originals: &Originals, id: u32) -> f32 {
    let clip = |id: u32| -> Option<f32> {
        let key = originals.animation_catalog.resolve(id, None).ok()?;
        let bytes = originals.animation_catalog.read(&originals.resource_index, key).ok()?;
        let secs = crate::animation::parse(&bytes).ok()?.duration_seconds();
        (secs.is_finite() && secs > 0.0).then_some(secs)
    };
    let group = originals.action_lists.lists.iter().flat_map(|(_, l)| &l.groups).find(|g| g.id == id);
    let total = group
        .and_then(|g| g.choices.first())
        .map(|c| {
            c.animations
                .iter()
                .map(|a| if a.duration > 0.0 { a.duration * a.duration_scale.max(0.01) } else { clip(a.id).unwrap_or(0.0) })
                .sum::<f32>()
        })
        .filter(|t| *t > 0.0)
        .or_else(|| clip(id));
    total.filter(|s| s.is_finite() && *s > 0.05).unwrap_or(2.0)
}

/// The request a button plays now: Roar picks one of three, Sit toggles.
pub fn button_request(play: &mut PlayState, button: &PlayAnimButton) -> u32 {
    match button.kind {
        PlayAnimKind::Toggle => {
            play.sitting = !play.sitting;
            if play.sitting {
                button.anims[0]
            } else {
                button.anims[button.anims.len() - 1]
            }
        }
        _ if button.anims.len() > 1 => {
            let i = (play.random() * button.anims.len() as f32) as usize;
            button.anims[i.min(button.anims.len() - 1)]
        }
        _ => button.anims[0],
    }
}

/// A random environment reaction other than the previous one
/// (`GetNextEnvironmentReactionAnimID`).
pub fn next_reaction(play: &mut PlayState) -> u32 {
    loop {
        let i = (play.random() * ENVIRONMENT_REACTIONS.len() as f32) as usize;
        let id = ENVIRONMENT_REACTIONS[i.min(ENVIRONMENT_REACTIONS.len() - 1)];
        if Some(id) != play.last_reaction {
            play.last_reaction = Some(id);
            return id;
        }
    }
}

pub fn start_anim(play: &mut PlayState, originals: &Originals, id: u32, kind: PlayAnimKind) {
    play.anim = Some(PlayAnim {
        id,
        kind,
        elapsed: 0.0,
        duration: anim_seconds(originals, id),
    });
}

/// Advance the playing animation; a finished action queues the editor idle.
pub fn advance_anim(play: &mut PlayState, originals: &Originals, dt: f32) {
    let done = match play.anim.as_mut() {
        Some(a) => {
            a.elapsed += dt;
            a.kind == PlayAnimKind::Action && a.elapsed >= a.duration
        }
        None => true,
    };
    if done {
        start_anim(play, originals, IDLE_ANIM, PlayAnimKind::Loop);
    }
}

/// Fallback body motion of a Test Drive request for a creature no clip rig
/// drives (`creature_clips::ClipDrive::drives_body` is false; with the
/// installed data the clip player poses the body and this is not used): the
/// whole creature moves procedurally for the request's retail duration: hops,
/// spins, flips, bobs and sways shaped after the named animations. The
/// request (id, time) is in [`PlayState::anim`]. Returns the offset (Bevy,
/// creature frame: translation in editor units, rotation) from the rest pose.
pub fn procedural_pose(anim: &PlayAnim, sitting: bool) -> (Vec3, Quat) {
    let t = anim.elapsed;
    let u = (t / anim.duration.max(0.05)).clamp(0.0, 1.0);
    let hop = |height: f32| Vec3::Y * height * (u * PI).sin().max(0.0);
    let id = anim.id;
    let sit = if sitting { Vec3::Y * -0.12 } else { Vec3::ZERO };
    let named = |name: &str| PLAY_ANIM_BUTTONS.iter().any(|b| b.name == name && b.anims.contains(&id));
    if named("csa_actn_jumphit") {
        return (hop(0.45), Quat::from_rotation_y(TAU * u));
    }
    if named("csa_actn_backflip") {
        return (hop(0.6), Quat::from_rotation_x(TAU * u));
    }
    if named("csa_sit_start") {
        let k = if sitting { u } else { 1.0 - u };
        return (Vec3::Y * -0.12 * k, Quat::from_rotation_x(0.12 * k));
    }
    if named("csa_actn_battleroar") {
        let rear = (u * PI).sin();
        return (sit + Vec3::Y * 0.05 * rear, Quat::from_rotation_x(-0.25 * rear));
    }
    if named("ed_matecall") {
        return (sit + Vec3::Y * 0.06 * (t * 9.0).sin().abs(), Quat::from_rotation_x(-0.15 * (u * PI).sin()));
    }
    if named("csa_actn_punch") {
        let lunge = (u * PI).sin();
        return (sit + Vec3::NEG_Z * 0.25 * lunge, Quat::from_rotation_x(0.2 * lunge));
    }
    if named("csa_emo_happy") || named("csa_emo_laugh") {
        let rate = if named("csa_emo_laugh") { 14.0 } else { 9.0 };
        return (sit + Vec3::Y * 0.12 * (t * rate).sin().abs(), Quat::IDENTITY);
    }
    if named("csa_emo_angry") {
        return (sit, Quat::from_rotation_y(0.25 * (t * 18.0).sin() * (u * PI).sin()));
    }
    if named("csa_emo_sad") {
        return (sit + Vec3::Y * -0.06 * (u * PI).sin(), Quat::from_rotation_x(0.3 * (u * PI).sin()));
    }
    if named("csa_emo_scared") {
        return (sit + Vec3::Z * 0.15 * (u * PI).sin(), Quat::from_rotation_y(0.05 * (t * 40.0).sin()));
    }
    if named("csa_emo_swoon") {
        return (sit, Quat::from_rotation_z(0.6 * (u * PI).sin()));
    }
    if let Some(b) = PLAY_ANIM_BUTTONS.iter().position(|b| b.name.starts_with("csa_dance") && b.anims.contains(&id)) {
        let phase = b as f32 * 0.7;
        let beat = t * TAU * 2.0;
        return (
            Vec3::new(0.08 * (beat * 0.5 + phase).sin(), 0.1 * (beat + phase).sin().abs(), 0.0),
            Quat::from_rotation_y(0.35 * (beat * 0.5 + phase).sin()) * Quat::from_rotation_z(0.12 * (beat + phase).cos()),
        );
    }
    // The editor idle and anything else: breathing.
    (sit + Vec3::Y * 0.01 * (t * 2.0).sin(), Quat::IDENTITY)
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Marker of every entity this module spawns (despawned on exit).
#[derive(Component)]
pub struct CreatureEditorOwned;

/// What the scene changed on the game camera, sun and ambient light (put
/// back when the session ends).
#[derive(Clone, Debug, Default)]
pub struct SavedView {
    pub projection: Option<Projection>,
    pub clear: Option<ClearColorConfig>,
    pub sun: Option<(Color, f32, Quat)>,
    pub ambient: Option<(Color, f32)>,
}

#[derive(Resource)]
pub struct CreatureEditor {
    /// A session is running (UI and scene set up).
    pub active: bool,
    pub kind: SetupKind,
    /// Setup of the next session (taken when it opens).
    pub requested: Option<SetupKind>,
    pub mode: Mode,
    /// Palette tab and page within it; first visible tab of the strip.
    pub category: usize,
    pub page: usize,
    pub tab_scroll: usize,
    /// Palette item under the pointer (part PROP instance).
    pub hover_item: Option<u32>,
    pub rig: EditorRig,
    /// Framing size of the camera offsets ([`DEFAULT_FRAMING`]).
    pub framing: f32,
    /// Centre of the creature's bounds in the player's frame (editor units).
    pub center_local: Vec3,
    /// Esc options panel (Resume / Spore Guide / Settings / Exit / Quit).
    pub options_open: bool,
    /// Ctrl+H hides the UI.
    pub ui_hidden: bool,
    pub naming: bool,
    pub message: Option<(String, f32)>,
    pub entry: Option<EntrySnapshot>,
    pub play: PlayState,
    pub saved: SavedView,
    /// Light currently applied (`editorLight`, paint light or background).
    pub light: Option<u32>,
    /// Effects to (re)start: the scene system plays them.
    pub effects_dirty: bool,
    /// Rotating the camera with a held button.
    pub turning: Option<MouseButton>,
    /// Creator-b sets this when an editing manipulator used the wheel this
    /// frame (the retail editor only zooms when no part took it).
    pub wheel_consumed: bool,
    /// Creator-b sets this while a manipulator (spine, vertebra) holds the
    /// left button, so the press does not turn the camera.
    pub manipulating: bool,
    /// Parts found since the previous session (palette "new" highlight).
    pub new_parts: Vec<u32>,
    /// The session ended with Cancel (the outfitters keep their record then).
    pub cancelled: bool,
    /// Accept is refused with this message (`rigid_stats::check_playable`).
    pub accept_refusal: Option<String>,
    pub clock: f32,
    /// Session options from the command line (captures): mode, category,
    /// animation button, background.
    pub startup: Vec<String>,
}

impl Default for CreatureEditor {
    fn default() -> Self {
        Self {
            active: false,
            kind: SetupKind::Creature,
            requested: None,
            mode: Mode::Build,
            category: 0,
            page: 0,
            tab_scroll: 0,
            hover_item: None,
            rig: EditorRig::default(),
            framing: DEFAULT_FRAMING,
            center_local: Vec3::ZERO,
            options_open: false,
            ui_hidden: false,
            naming: false,
            message: None,
            entry: None,
            play: PlayState { anims_panel: true, seed: 0x5EED_1234, ..default() },
            saved: SavedView::default(),
            light: None,
            effects_dirty: false,
            turning: None,
            wheel_consumed: false,
            manipulating: false,
            new_parts: vec![],
            cancelled: false,
            accept_refusal: None,
            clock: 0.0,
            startup: vec![],
        }
    }
}

impl CreatureEditor {
    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), 3.0));
    }
}

/// Ask the next creator session for a setup (e.g. `CellToCreature` after the
/// Cell stage), then open it.
pub fn request_setup(editor: &mut CreatureEditor, state: &mut EditorState, kind: SetupKind) {
    editor.requested = Some(kind);
    state.show();
}

// ---------------------------------------------------------------------------
// Rules (pure)
// ---------------------------------------------------------------------------

/// Why a palette part cannot be taken (rollover reason line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not found yet (bone piles / other species, `creature_parts.rs`).
    Locked,
    NoDna,
    TooComplex,
    /// No model to place yet (limb assemblies are creator-b's).
    Unavailable,
}

/// Palette availability (`cCollectableItems`, `creature_editor_edit::unlocks`):
/// a part is available when it starts unlocked (base rows of the first
/// column, the Cell-to-Creature palette), was found (bone piles, other
/// species: the next column of a row) or is already on the creature.
pub fn part_unlocked(run: &RunProgress, info: &crate::creature_editor_data::CreaturePartInfo) -> bool {
    crate::creature_editor_edit::unlocks::available(run, info)
}

pub fn refusal(data: &CreatureEditorData, kind: SetupKind, run: &RunProgress, originals: &Originals, part: u32) -> Option<Refusal> {
    let info = data.parts.get(&part)?;
    if !part_unlocked(run, info) {
        return Some(Refusal::Locked);
    }
    // Limb assemblies have no mesh of their own (`creature_limbs`).
    if originals.part_catalog.by_definition_in(info.key.group_id, part).is_none()
        && !crate::creature_limbs::is_assembly(&originals.resource_index, part)
    {
        return Some(Refusal::Unavailable);
    }
    // Retail order (FUN_0059E580): the complexity limits, then the budget.
    let c = data.complexity(kind, &run.parts, run.body.spine.len());
    if !c.has_room_for(info) {
        return Some(Refusal::TooComplex);
    }
    if run.editor_dna < info.price as f32 {
        return Some(Refusal::NoDna);
    }
    None
}

/// The creature has a mouth (needed to accept the creation).
pub fn has_mouth(data: &CreatureEditorData, run: &RunProgress) -> bool {
    run.parts.iter().any(|p| data.info_of(p).is_some_and(|i| i.mouth || i.carnivore > 0 || i.herbivore > 0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diet {
    None,
    Herbivore,
    Carnivore,
    Omnivore,
}

pub fn diet(data: &CreatureEditorData, run: &RunProgress) -> Diet {
    let (mut c, mut h) = (false, false);
    for p in &run.parts {
        if let Some(i) = data.info_of(p) {
            c |= i.carnivore > 0;
            h |= i.herbivore > 0;
        }
    }
    match (c, h) {
        (true, true) => Diet::Omnivore,
        (true, false) => Diet::Carnivore,
        (false, true) => Diet::Herbivore,
        _ => Diet::None,
    }
}

/// Highest level per ability (verb category) over the mounted parts (retail:
/// ability level = highest part value per capability).
pub fn ability_levels(data: &CreatureEditorData, run: &RunProgress) -> Vec<(&'static str, i32)> {
    let mut out: Vec<(&'static str, i32)> = Vec::new();
    for p in &run.parts {
        if let Some(i) = data.info_of(p) {
            for (verb, level) in &i.abilities {
                match out.iter_mut().find(|(v, _)| v == verb) {
                    Some(e) => e.1 = e.1.max(*level),
                    None => out.push((verb, *level)),
                }
            }
        }
    }
    out
}

/// Scale a mounted part (and its symmetric twin) by `notches` wheel notches:
/// `size *= 2^(0.075 * notch)` within `[modelMinScale, modelMaxScale]`
/// (`cEditor::sub_581F70`; Up/Down = one notch on the selected part).
pub fn scale_part(data: &CreatureEditorData, run: &mut RunProgress, index: usize, notches: f32) -> bool {
    let Some(part) = run.parts.get(index).cloned() else {
        return false;
    };
    let (min, max) = data.info_of(&part).map(|i| (i.min_scale, i.max_scale.max(i.min_scale))).unwrap_or((0.5, 2.0));
    let scale = (part.scale * 2f32.powf(0.075 * notches)).clamp(min, max);
    if (scale - part.scale).abs() < 1e-6 {
        return false;
    }
    for (i, p) in run.parts.iter_mut().enumerate() {
        if i == index || (part.pair_id != 0 && p.pair_id == part.pair_id) {
            p.scale = scale;
        }
    }
    run.request_save();
    true
}

/// Switch Build / Paint / Test Drive (`EditorUI::SetActiveMode` 0x5DDA70):
/// closes the options panel; entering Test Drive starts the idle and the
/// play camera; leaving it restores the creature.
pub fn set_mode(editor: &mut CreatureEditor, state: &EditorState, data: &CreatureEditorData, originals: &Originals, mode: Mode) {
    // Test Drive: the vehicles only among the rigid editors (`rigid_stats`).
    if editor.mode == mode || state.dragging() || (editor.kind.is_rigid() && mode == Mode::Play && !editor.kind.is_vehicle()) {
        return;
    }
    let was_play = editor.mode == Mode::Play;
    editor.mode = mode;
    editor.options_open = false;
    editor.hover_item = None;
    editor.effects_dirty = true;
    if editor.kind.is_rigid() {
        // A vehicle's Test Drive keeps the editor camera (`rigid_stats::test_drive`).
    } else if mode == Mode::Play {
        let t = data.play_camera;
        let mut props = editor.rig.props;
        props.zoom = t.initial_distance;
        props.min_zoom = t.min_distance;
        props.max_zoom = t.max_distance;
        props.min_pitch = -t.max_pitch;
        props.max_pitch = -t.min_pitch;
        props.offset_x = 0.0;
        props.offset_y = 0.0;
        // ADAPTED: the play camera (`CreatureTestMode~` +
        // `PlayModeCameraTuning`) is a creature-game orbit; this keeps the
        // editor rig with its distance/pitch limits, the initial distance and
        // PHI as pitch, and the current heading.
        let heading = editor.rig.heading;
        editor.rig = EditorRig { heading, s_heading: editor.rig.s_heading, ..EditorRig::new(props) };
        editor.rig.pitch = -t.initial_phi.to_radians();
        editor.rig.s_zoom = editor.rig.s_zoom.max(props.min_zoom);
        start_anim(&mut editor.play, originals, IDLE_ANIM, PlayAnimKind::Loop);
        editor.play.walk_to = None;
        editor.play.sitting = false;
    } else if was_play {
        let heading = editor.rig.heading;
        let props = data.camera_of(editor.kind);
        editor.rig = EditorRig { heading, s_heading: editor.rig.s_heading, ..EditorRig::new(props) };
        editor.play.anim = None;
        editor.play.walk_to = None;
    }
    // ADAPTED (as in the Cell Creator): the paint palette is on the right, so
    // Paint mode mirrors the horizontal offset and the creature glides left.
    if mode != Mode::Play {
        let x = editor.rig.props.offset_x.abs();
        editor.rig.offset_x = if mode == Mode::Paint { -x } else { x };
    }
}

/// A Test Drive animation button.
pub fn press_play_button(editor: &mut CreatureEditor, originals: &Originals, control: u32) -> bool {
    let Some(button) = PLAY_ANIM_BUTTONS.iter().find(|b| b.control == control) else {
        return false;
    };
    let id = button_request(&mut editor.play, button);
    let kind = if button.kind == PlayAnimKind::Toggle { PlayAnimKind::Action } else { button.kind };
    start_anim(&mut editor.play, originals, id, kind);
    let walking = button.name.starts_with("BP1_cycle");
    editor.play.walk_to = None;
    if walking {
        editor.play.walk_angle = 0.0;
    }
    true
}

/// A Test Drive background slot: its entry effect and light, then a random
/// environment reaction.
pub fn choose_background(editor: &mut CreatureEditor, data: &CreatureEditorData, originals: &Originals, slot: usize) -> bool {
    let count = data.backgrounds_of(editor.kind).len();
    if slot >= count || (slot == editor.play.background && editor.play.anim.is_some()) {
        return false;
    }
    editor.play.background = slot;
    editor.effects_dirty = true;
    let id = next_reaction(&mut editor.play);
    start_anim(&mut editor.play, originals, id, PlayAnimKind::Action);
    true
}

/// Leave the editor. `keep` = Save and Exit (needs a mouth), else Cancel
/// (restores the creature as it was on entry).
pub fn finish(editor: &mut CreatureEditor, state: &mut EditorState, data: &CreatureEditorData, run: &mut RunProgress, keep: bool) -> bool {
    if !keep {
        editor.cancelled = true;
    }
    if let Some(refusal) = editor.accept_refusal.clone().filter(|_| keep) {
        // The rigid editors' minimum playable size (`rigid_stats`).
        editor.say(refusal);
        return false;
    }
    if keep && !editor.kind.is_outfitter() && !editor.kind.is_rigid() && !has_mouth(data, run) {
        // `HintsEDI` 0x1F83D13C.
        editor.say(data.hint_needs_mouth.clone().unwrap_or_else(|| "Your creation needs a mouth".into()));
        return false;
    }
    if !keep {
        if let Some(entry) = editor.entry.take() {
            entry.apply(run);
        }
    } else if !editor.kind.is_outfitter() && !editor.kind.is_rigid() {
        run.creature_creation.visits += 1;
    }
    run.request_save();
    state.queued.clear();
    state.open = false;
    state.visuals_dirty = true;
    true
}

// ---------------------------------------------------------------------------
// Plugin and systems
// ---------------------------------------------------------------------------

pub struct CreatureEditorPlugin;

impl Plugin for CreatureEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatureEditor>()
            .init_resource::<CreatureEditorData>()
            .init_resource::<crate::creature_editor_ui::CreatureEditorUiLayout>()
            .add_message::<MouseMotion>()
            .add_message::<MouseWheel>()
            .add_systems(
                Update,
                (
                    session,
                    keyboard.run_if(active),
                    crate::creature_editor_edit::manipulate.run_if(active),
                    crate::creature_editor_edit::symmetry_sounds.run_if(active),
                    crate::creature_handles::manipulate.run_if(active).run_if(crate::creature_limbs::ready),
                    crate::creature_limbs::manipulate.run_if(active).run_if(crate::creature_limbs::ready),
                    mouse_camera.run_if(active),
                    test_drive.run_if(active),
                    crate::creature_editor_ui::manage,
                    crate::creature_editor_ui::bind.run_if(active),
                    crate::creature_editor_ui::actions.run_if(active),
                    crate::creature_editor_paint::paint_ui.run_if(active),
                )
                    .chain()
                    .after(crate::editor::drag)
                    .after(crate::spui::view::input)
                    .before(crate::spui::view::sync),
            )
            .add_systems(
                PostUpdate,
                (place_camera, scene_light_and_effects)
                    .chain()
                    .run_if(active)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                editor_app_flag.run_if(active).before(crate::swarm_fx::SwarmSet::Simulate),
            )
            // Creator-b: the creature keeps its paint in every stage.
            .init_resource::<crate::creature_editor_paint::CreaturePaintData>()
            .add_systems(Update, crate::creature_editor_paint::apply.after(crate::editor::refresh_parts));
    }
}

pub fn active(editor: Res<CreatureEditor>) -> bool {
    editor.active
}

#[allow(clippy::too_many_arguments)]
fn session(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    mut state: ResMut<EditorState>,
    mut editor: ResMut<CreatureEditor>,
    mut data: ResMut<CreatureEditorData>,
    run: Res<RunProgress>,
    originals: Res<Originals>,
    time: Res<Time>,
    mut players: Query<&mut Transform, With<Player>>,
    mut cameras: Query<(&mut Projection, &mut Camera, Option<&mut AmbientLight>), With<crate::GameCamera>>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform), (With<crate::GameSun>, Without<Player>)>,
    owned: Query<Entity, With<CreatureEditorOwned>>,
) {
    let want = state.open && routes(stage.get());
    if want && !editor.active {
        if !data.loaded && !originals.resource_index.records.is_empty() {
            *data = CreatureEditorData::load(&originals.resource_index);
            for e in &data.errors {
                warn!("creature editor data: {e}");
            }
        }
        // The outfitters of the later stages are this editor with another
        // setup PROP (`outfitter`).
        let kind = editor.requested.take().or_else(|| crate::outfitter::setup_of(stage.get())).unwrap_or_default();
        let startup = std::mem::take(&mut editor.startup);
        let seed = editor.play.seed;
        *editor = CreatureEditor {
            active: true,
            kind,
            rig: EditorRig::new(data.camera_of(kind)),
            framing: data.setup(kind).and_then(|s| s.bound_size).unwrap_or(DEFAULT_FRAMING),
            entry: Some(EntrySnapshot::of(&run)),
            effects_dirty: true,
            play: PlayState { anims_panel: true, seed: seed.wrapping_add(run.parts.len() as u32), ..default() },
            startup,
            ..default()
        };
        editor.light = data.setup(kind).and_then(|s| s.light);
        // Save what the scene changes.
        if let Some((projection, camera, ambient)) = cameras.iter_mut().next() {
            editor.saved.projection = Some(projection.clone());
            editor.saved.clear = Some(camera.clear_color.clone());
            editor.saved.ambient = ambient.map(|a| (a.color, a.brightness));
        }
        if let Some((sun, transform)) = suns.iter_mut().next() {
            editor.saved.sun = Some((sun.color, sun.illuminance, transform.rotation));
        }
        // Capture options: `--creator-mode paint|play`, `--creator-category N`,
        // `--creator-anim <control hex>`, `--creator-bg N`.
        let args: Vec<String> = std::env::args().collect();
        let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
        if let Some(c) = flag("--creator-category").and_then(|c| c.parse().ok()) {
            editor.category = c;
        }
        let mode = flag("--creator-mode");
        if mode.as_deref() == Some("paint") {
            set_mode(&mut editor, &state, &data, &originals, Mode::Paint);
        } else if mode.as_deref() == Some("play") {
            set_mode(&mut editor, &state, &data, &originals, Mode::Play);
            if let Some(bg) = flag("--creator-bg").and_then(|b| b.parse().ok()) {
                choose_background(&mut editor, &data, &originals, bg);
            }
            if let Some(control) = flag("--creator-anim").and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok()) {
                press_play_button(&mut editor, &originals, control);
            }
        }
    } else if !want && editor.active {
        // Restore the creature (Test Drive moved it), camera and lights.
        if let Some(rest) = editor.play.rest.take() {
            if let Ok(mut t) = players.single_mut() {
                *t = rest;
            }
        }
        if let Some((mut projection, mut camera, ambient)) = cameras.iter_mut().next() {
            if let Some(p) = editor.saved.projection.take() {
                *projection = p;
            }
            camera.sub_camera_view = None;
            if let Some(clear) = editor.saved.clear.take() {
                camera.clear_color = clear;
            }
            if let (Some(mut a), Some((color, brightness))) = (ambient, editor.saved.ambient.take()) {
                a.color = color;
                a.brightness = brightness;
            }
        }
        if let Some((mut sun, mut transform)) = suns.iter_mut().next() {
            if let Some((color, illuminance, rotation)) = editor.saved.sun.take() {
                sun.color = color;
                sun.illuminance = illuminance;
                transform.rotation = rotation;
            }
        }
        for e in &owned {
            commands.entity(e).despawn();
        }
        crate::swarm_fx::game_model::set_editor_backdrop(false);
        editor.active = false;
        editor.options_open = false;
        editor.mode = Mode::Build;
    }
    if editor.active {
        editor.clock += time.delta_secs();
        if let Some((_, t)) = editor.message.as_mut() {
            *t -= time.delta_secs();
        }
        if editor.message.as_ref().is_some_and(|(_, t)| *t <= 0.0) {
            editor.message = None;
        }
    }
}

/// The keyboard table (`cEditor::OnKeyDown` 0x58AC10 and
/// `EditorCamera::OnKeyDown` 0x5A2060).
#[allow(clippy::too_many_arguments)]
fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut editor: ResMut<CreatureEditor>,
    mut state: ResMut<EditorState>,
    data: Res<CreatureEditorData>,
    originals: Res<Originals>,
    mut run: ResMut<RunProgress>,
) {
    if editor.naming {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let play = editor.mode == Mode::Play;
    if keys.just_pressed(KeyCode::Escape) {
        // No selection concept beyond the focused part: Esc toggles the
        // options panel (cmd 0x10D).
        editor.options_open = !editor.options_open;
    }
    if editor.options_open {
        return;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyH) {
        editor.ui_hidden = !editor.ui_hidden;
    }
    if !play {
        if ctrl && keys.just_pressed(KeyCode::KeyZ) {
            state.queued.push(Action::Undo);
        }
        if ctrl && keys.just_pressed(KeyCode::KeyY) {
            state.queued.push(Action::Redo);
        }
        if editor.mode == Mode::Build && keys.any_just_pressed([KeyCode::Backspace, KeyCode::Delete]) {
            state.queued.push(Action::Remove);
        }
        // Up / Down: one wheel notch on the selected part.
        let notch = if keys.just_pressed(KeyCode::ArrowUp) {
            1.0
        } else if keys.just_pressed(KeyCode::ArrowDown) {
            -1.0
        } else {
            0.0
        };
        if notch != 0.0 && editor.mode == Mode::Build && run.focused_part as usize >= state.locked_parts {
            let focus = run.focused_part as usize;
            let before = run.clone();
            if scale_part(&data, &mut run, focus, notch) {
                state.record_external(before);
                state.visuals_dirty = true;
            }
        }
    }
    if keys.just_pressed(KeyCode::Enter) {
        finish(&mut editor, &mut state, &data, &mut run, true);
        return;
    }
    let _ = &originals;
    // Camera keys, per frame while held.
    let s = EditorRig::step(time.delta_secs());
    let heading = if keys.pressed(KeyCode::Comma) {
        -1.0
    } else if keys.pressed(KeyCode::Period) {
        1.0
    } else {
        0.0
    };
    let zoom = if keys.any_pressed([KeyCode::NumpadAdd, KeyCode::Equal]) {
        -1.0
    } else if keys.any_pressed([KeyCode::NumpadSubtract, KeyCode::Minus]) {
        1.0
    } else {
        0.0
    };
    editor.rig.keys(heading, zoom, s);
}

/// Mouse: drag the background to turn the camera (right button anywhere off
/// the UI, left button where no part was grabbed), Ctrl/Shift + drag zooms,
/// the wheel zooms when no editing manipulator used it. Test Drive: a click
/// on the ground walks there, a double click runs there (manual p. 22: "To move your
/// creature around in the Test Drive circle, click a location ... To make your creature
/// run, double-click a location"; ADAPTED: the double click interval is the creature
/// game's `creatureMouseDoubleClickThreshold`, the editor's own is not traced).
#[allow(clippy::too_many_arguments)]
pub(crate) fn mouse_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    pointer: Res<crate::spui::view::SpuiPointer>,
    state: Res<EditorState>,
    mut editor: ResMut<CreatureEditor>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::GameCamera>>,
    players: Query<&Transform, With<Player>>,
) {
    let over_ui = pointer.over_ui;
    let blocked = editor.options_open || editor.naming;
    for button in [MouseButton::Right, MouseButton::Left] {
        if mouse.just_pressed(button) && !over_ui && !blocked && editor.turning.is_none() {
            editor.turning = Some(button);
        }
    }
    if let Some(b) = editor.turning {
        if !mouse.pressed(b) {
            editor.turning = None;
        }
    }
    // A left press that grabbed a part (or carries one) is an edit.
    if editor.turning == Some(MouseButton::Left) && (state.dragging() || editor.manipulating) {
        editor.turning = None;
    }
    let zoom_mod = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::ControlLeft, KeyCode::ControlRight]);
    let mut delta = Vec2::ZERO;
    for m in motion.read() {
        delta += m.delta;
    }
    if editor.turning.is_some() && delta != Vec2::ZERO {
        if zoom_mod {
            editor.rig.drag_zoom(delta.y);
        } else {
            editor.rig.drag(delta.x, delta.y);
        }
    }
    let consumed = std::mem::take(&mut editor.wheel_consumed);
    for w in wheel.read() {
        if over_ui || consumed || blocked {
            continue;
        }
        let notches = match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 40.0,
        };
        editor.rig.wheel(notches * 120.0);
    }
    // Test Drive: click the ground to walk there.
    if editor.mode == Mode::Play && mouse.just_released(MouseButton::Left) && !over_ui && delta.length() < 2.0 {
        let (Ok(window), Ok((camera, cam)), Ok(pt)) = (windows.single(), cameras.single(), players.single()) else {
            return;
        };
        let rest = editor.play.rest.unwrap_or(*pt);
        if let Some(cursor) = window.cursor_position() {
            if let Ok(ray) = camera.viewport_to_world(cam, cursor) {
                let up = rest.rotation * Vec3::Y;
                let floor = rest.translation - up * 1.3 * rest.scale.y;
                let denom = ray.direction.dot(up);
                if denom.abs() > 1e-4 {
                    let t = (floor - ray.origin).dot(up) / denom;
                    if t > 0.0 {
                        let hit = ray.origin + *ray.direction * t;
                        let local = rest.rotation.conjugate() * (hit - floor) / rest.scale.y.max(1e-3);
                        // ADAPTED: walks stay on the test ground near the dais.
                        let local = Vec3::new(local.x, 0.0, local.z).clamp_length_max(4.0);
                        let now = editor.clock;
                        let double = is_double_click(editor.play.last_click, now, cursor);
                        editor.play.last_click = if double { None } else { Some((now, cursor)) };
                        editor.play.walk_to = Some((local, double));
                        editor.play.anim = Some(PlayAnim {
                            // `BP1_cycle_run` / `BP1_cycle_walk`.
                            id: if double { 0x06F8_A61B } else { 0x06F8_A609 },
                            kind: PlayAnimKind::Loop,
                            elapsed: 0.0,
                            duration: 1.0,
                        });
                    }
                }
            }
        }
    }
}

/// Test Drive: animation clock and the creature's procedural motion.
fn test_drive(
    time: Res<Time>,
    mut editor: ResMut<CreatureEditor>,
    originals: Res<Originals>,
    mut players: Query<&mut Transform, With<Player>>,
    clip_drive: Option<Res<crate::creature_clips::ClipDrive>>,
) {
    let clips = crate::creature_clips::drives_body(clip_drive.as_deref());
    let Ok(mut t) = players.single_mut() else {
        return;
    };
    if editor.mode != Mode::Play {
        if let Some(rest) = editor.play.rest.take() {
            *t = rest;
        }
        return;
    }
    let rest = *editor.play.rest.get_or_insert(*t);
    let dt = time.delta_secs();
    advance_anim(&mut editor.play, &originals, dt);
    let Some(anim) = editor.play.anim.clone() else {
        return;
    };
    let unit = rest.scale.y.max(1e-3);
    let walking = PLAY_ANIM_BUTTONS
        .iter()
        .find(|b| b.anims.contains(&anim.id) && b.name.starts_with("BP1_cycle"))
        .map(|b| b.name == "BP1_cycle_run");
    let mut base = rest;
    if let Some(run_cycle) = walking {
        // ADAPTED walk/run: a circle around the dais (or to the clicked
        // point), facing the way it goes.
        let speed = if run_cycle { 2.2 } else { 0.9 };
        let (pos, heading) = match editor.play.walk_to {
            Some((target, _)) => {
                let here = editor.play.walk_pos();
                let to = target - here;
                let step = to.clamp_length_max(speed * dt);
                let next = here + step;
                editor.play.set_walk_pos(next);
                if to.length() < 0.05 {
                    editor.play.walk_to = None;
                    start_anim(&mut editor.play, &originals, IDLE_ANIM, PlayAnimKind::Loop);
                }
                (next, (-step.x).atan2(-step.z))
            }
            None => {
                let r = 1.4;
                editor.play.walk_angle += speed / r * dt;
                let a = editor.play.walk_angle;
                let p = Vec3::new(r * a.sin(), 0.0, r * (1.0 - a.cos()));
                editor.play.set_walk_pos(p);
                // Tangent of the circle.
                let tangent = Vec3::new(a.cos(), 0.0, a.sin());
                (p, (-tangent.x).atan2(-tangent.z))
            }
        };
        base.translation = rest.translation + rest.rotation * (pos * unit);
        base.rotation = rest.rotation * Quat::from_rotation_y(heading);
        // The walk clip moves the body itself when clips drive it.
        let bob = if clips { 0.0 } else { (editor.clock * if run_cycle { 14.0 } else { 8.0 }).sin().abs() * 0.04 };
        base.translation += rest.rotation * Vec3::Y * bob * unit;
        *t = base;
        return;
    }
    let walk_pos = editor.play.walk_pos();
    base.translation = rest.translation + rest.rotation * (walk_pos * unit);
    let (offset, turn) = if clips { (Vec3::ZERO, Quat::IDENTITY) } else { procedural_pose(&anim, editor.play.sitting) };
    t.translation = base.translation + base.rotation * (offset * unit);
    t.rotation = (base.rotation * turn).normalize();
    t.scale = rest.scale;
}

impl PlayState {
    fn walk_pos(&self) -> Vec3 {
        Vec3::new(self.walk_angle_pos[0], 0.0, self.walk_angle_pos[1])
    }
    fn set_walk_pos(&mut self, p: Vec3) {
        self.walk_angle_pos = [p.x, p.z];
    }
}

/// Scene: the editor camera on the game camera (retail FOV, near clip, the
/// rig's view of the creature, the framing rule).
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_camera(
    time: Res<Time>,
    mut editor: ResMut<CreatureEditor>,
    originals: Res<Originals>,
    players: Query<(Entity, &Transform), (With<Player>, Without<crate::GameCamera>)>,
    children: Query<&Children>,
    meshes: Query<(&GlobalTransform, &bevy::camera::primitives::Aabb), With<Mesh3d>>,
    parts: Query<(&GlobalTransform, &bevy::camera::primitives::Aabb), (With<Mesh3d>, With<crate::creature::Accessory>)>,
    mut cameras: Query<(&mut Transform, &mut Projection, &mut Camera), With<crate::GameCamera>>,
) {
    let Ok((mut transform, mut projection, mut camera)) = cameras.single_mut() else {
        return;
    };
    // World boxes of the creature's blocks: the skin and its child meshes
    // and every part (feet and hands on limbs are root entities).
    let mut world_boxes: Vec<(Vec3, Vec3)> = Vec::new();
    // ADAPTED target: the centre of the creature's bounds (retail aims at the model
    // origin, `FUN_005A1FF0((0, 0, 0))`, whose frame OpenSpore does not share);
    // measured in Build/Paint, kept in Test Drive while the creature moves.
    if editor.mode != Mode::Play {
        if let Ok((player, pose)) = players.single() {
            let mut stack = vec![player];
            let mut boxes = Vec::new();
            while let Some(e) = stack.pop() {
                if let Ok(kids) = children.get(e) {
                    stack.extend(kids.iter());
                }
                if let Ok((t, aabb)) = meshes.get(e) {
                    boxes.push((Vec3::from(aabb.center), Vec3::from(aabb.half_extents), t.to_matrix()));
                }
            }
            for (t, aabb) in &parts {
                world_boxes.extend(crate::creature_hud::portrait::union_box([(Vec3::from(aabb.center), Vec3::from(aabb.half_extents), t.to_matrix())]));
            }
            for b in &boxes {
                world_boxes.extend(crate::creature_hud::portrait::union_box([*b]));
            }
            if let Some((min, max)) = crate::creature_hud::portrait::union_box(boxes) {
                let unit = pose.scale.y.max(1e-3);
                let local = pose.rotation.conjugate() * ((min + max) * 0.5 - pose.translation) / unit;
                if local.is_finite() && local.length() < 10.0 {
                    editor.center_local = local;
                }
            }
        }
    }
    let players = players.iter().map(|(_, t)| *t).collect::<Vec<_>>();
    let s = EditorRig::step(time.delta_secs());
    let framing = editor.framing;
    editor.rig.update(s, framing);
    let pose = editor.play.rest.or_else(|| players.first().copied()).unwrap_or_default();
    let unit = pose.scale.y.max(1e-3);
    let up = pose.rotation * Vec3::Y;
    let mut target = pose.translation + pose.rotation * (editor.center_local * unit);
    if editor.mode == Mode::Play {
        // The camera follows the walking creature: Test Drive aims the camera at the
        // creature's position and `EditorCamera::Update` 0x5A3E00 moves the look-at point
        // toward it by `s x 0.37` a frame (`cEditor::Update` 0x58BE50; notes
        // `editors-final2-notes.md` section 1), so the view trails the creature.
        let live = players.first().copied().unwrap_or(pose);
        let mut want = live.translation - pose.translation;
        want -= up * want.dot(up);
        let follow = editor.play.follow;
        editor.play.follow = follow + (want - follow) * (s * FOLLOW_RATE);
        target += editor.play.follow;
    } else {
        editor.play.follow = Vec3::ZERO;
    }
    let rig = editor.rig;
    // Zoom and near clip are in editor (creature) units.
    let scale = unit * editor_units(&originals, editor.kind);
    *transform = camera_transform(&rig, target, pose.rotation, scale, framing);
    // The retail FOV keeps its vertical angle (`retail_vertical_fov`); the
    // near clip scales with the creature.
    if let (Some(size), Projection::Perspective(p)) = (camera.logical_target_size(), projection.as_mut()) {
        if size.y > 0.0 {
            let aspect = size.x / size.y;
            let fov = retail_vertical_fov(rig.props.fov, aspect);
            if (p.fov - fov).abs() > 1e-6 {
                p.fov = fov;
            }
            let near = (rig.props.near * scale).max(0.01);
            if (p.near - near).abs() > 1e-6 {
                p.near = near;
            }
            // The framing rule (creature setups; ADAPTED: the blocks are the
            // skin and part meshes, retail tests every rigblock's models).
            if editor.mode != Mode::Play && !editor.rig.moved && !editor.manipulating && !editor.kind.is_rigid() && !world_boxes.is_empty() {
                let tan_v = (fov * 0.5).tan();
                let tight = (retail_vertical_fov(rig.props.fov * 0.9, aspect) * 0.5).tan();
                let outside = world_boxes.iter().any(|(lo, hi)| !box_inside(&transform, *lo, *hi, tan_v, aspect, near));
                let inside_tight = !outside && world_boxes.iter().all(|(lo, hi)| box_inside(&transform, *lo, *hi, tight, aspect, near));
                let zoom = framing_zoom(editor.rig.zoom, &rig.props, framing, s, outside, inside_tight);
                if zoom != editor.rig.zoom {
                    editor.rig.zoom = zoom;
                }
            }
        }
    }
    if camera.sub_camera_view.is_some() {
        camera.sub_camera_view = None;
    }
}

/// The editor's effects test the editor application flag (`EDITOR_MODE`,
/// bit 16) besides the stage's game mode.
fn editor_app_flag(mut flags: ResMut<crate::swarm_fx::SwarmAppFlags>) {
    let want = flags.0 | crate::swarm_fx::app_flag::EDITOR_MODE;
    if flags.0 != want {
        flags.0 = want;
    }
}

/// Tag of the editor's effects.
pub const EFFECT_TAG: u64 = 0xC4EA_7012;

/// Scene light (`editorLight`, the paint-mode light in Paint mode, the
/// background's light in Test Drive) and the effects: the Build dais look
/// (`editorPlayModeExitEffectID`), the skybox, Test Drive's entry effect and
/// background entry effect.
#[allow(clippy::too_many_arguments)]
fn scene_light_and_effects(
    mut commands: Commands,
    mut editor: ResMut<CreatureEditor>,
    data: Res<CreatureEditorData>,
    players: Query<&Transform, (With<Player>, Without<crate::GameSun>, Without<crate::GameCamera>)>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform), (With<crate::GameSun>, Without<Player>)>,
    mut ambients: Query<&mut AmbientLight, With<crate::GameCamera>>,
    effects: Query<(Entity, &crate::swarm_fx::SwarmEffect), With<CreatureEditorOwned>>,
    land: Option<Res<crate::terrain::LandSurface>>,
    mut room: Query<&mut Visibility, With<crate::editor_environment::EditorEnvironmentSurface>>,
    mut cameras: Query<&mut Camera, With<crate::GameCamera>>,
    originals: Res<Originals>,
) {
    // Test Drive backgrounds are effects + lighting + environment models: the
    // `editor_*_mode_entry` effects hold two gameModel blocks, a ground plane
    // (`ce_beach_ground_plane-<bg>`, world 0) and a backdrop of the editor
    // model world 0x021B37D6 (`..._background`, model group 0x35907808):
    // `swarm_fx::game_model::drawn` draws both while a backdrop is shown
    // ([`set_editor_backdrop`](crate::swarm_fx::game_model::set_editor_backdrop)).
    // ADAPTED: the default background (order 0, `mountainbg`) keeps the creator
    // room (a mountain landscape, as the retail default); the others replace it
    // with their own models on the play camera's black background
    // (`CreatureTestMode~` `cameraBackgroundColor` (0,0,0,1)), with their
    // particles, screen effects and light.
    let backgrounds = data.backgrounds_of(editor.kind);
    let room_on = editor.mode != Mode::Play
        || backgrounds.get(editor.play.background).is_none_or(|b| b.order == 0);
    let want = if room_on { Visibility::Inherited } else { Visibility::Hidden };
    // The backdrop's own models (the `..._background` gameModel of the editor
    // background world) draw instead of the room.
    crate::swarm_fx::game_model::set_editor_backdrop(!room_on);
    for mut v in &mut room {
        if *v != want {
            *v = want;
        }
    }
    if let Some(mut camera) = cameras.iter_mut().next() {
        let clear = if room_on {
            editor.saved.clear.clone().unwrap_or_default()
        } else {
            ClearColorConfig::Custom(Color::BLACK)
        };
        if format!("{:?}", camera.clear_color) != format!("{clear:?}") {
            camera.clear_color = clear;
        }
    }
    let Some(setup) = data.setup(editor.kind).cloned() else {
        return;
    };
    let pose = editor.play.rest.or_else(|| players.iter().next().copied()).unwrap_or_default();
    let unit = pose.scale.y.max(1e-3);
    // The effects are authored in editor (creature) units.
    let fx_scale = unit * editor_units(&originals, editor.kind);
    let want_light = match editor.mode {
        Mode::Build => setup.light,
        Mode::Paint => setup.paint_light.or(setup.light),
        Mode::Play => data
            .backgrounds_of(editor.kind)
            .get(editor.play.background)
            .and_then(|b| b.lighting)
            .or(data.play_light)
            .or(setup.light),
    };
    if let Some(light) = data.light_of(want_light).cloned() {
        let saved_sun = editor.saved.sun;
        if let Some((mut sun, mut transform)) = suns.iter_mut().next() {
            // `lightSunDir` points at the sun in the creature's frame.
            let dir = (pose.rotation * spore_to_bevy(Vec3::from_array(light.sun_dir))).normalize_or(Vec3::Y);
            let up = if dir.cross(Vec3::Y).length_squared() < 1e-4 { Vec3::Z } else { Vec3::Y };
            let rotation = Transform::IDENTITY.looking_to(-dir, up).rotation;
            if transform.rotation != rotation {
                transform.rotation = rotation;
            }
            let color = Color::srgb(light.sun_color[0], light.sun_color[1], light.sun_color[2]);
            // ADAPTED: intensities stay on the stage's calibrated scale (the
            // saved sun) times the authored strength.
            // (A stage that switched its sun off, as Space does in the
            // galaxy views, uses the default level.)
            let base = saved_sun.map_or(3000.0, |s| s.1).max(if editor.kind.is_outfitter() || editor.kind.is_rigid() { 2600.0 } else { 0.0 });
            let lux = base * light.sun_strength.max(0.0);
            if sun.color != color {
                sun.color = color;
            }
            if (sun.illuminance - lux).abs() > 0.01 {
                sun.illuminance = lux;
            }
        }
        if let Some(mut ambient) = ambients.iter_mut().next() {
            let m = light.ambient.iter().cloned().fold(1e-3, f32::max);
            let color = Color::srgb(light.ambient[0] / m, light.ambient[1] / m, light.ambient[2] / m);
            if ambient.color != color {
                ambient.color = color;
            }
            // The Space stage keeps a night-sky fill (`space_view::lighting`):
            // the rigid-block editors need the studio's own (ADAPTED level).
            if editor.kind.is_rigid() && ambient.brightness < 300.0 {
                ambient.brightness = 300.0;
            }
        }
    }
    editor.light = want_light;
    if !editor.effects_dirty {
        return;
    }
    editor.effects_dirty = false;
    for (e, fx) in &effects {
        if fx.tag == EFFECT_TAG {
            commands.entity(e).despawn();
        }
    }
    let up = pose.rotation * Vec3::Y;
    // (An outfitter's preview creature stands above the stage's land.)
    let floor = match land.as_deref().filter(|_| !editor.kind.is_outfitter()) {
        Some(l) => {
            let h = l.height(pose.translation.x, pose.translation.z);
            Vec3::new(pose.translation.x, h, pose.translation.z)
        }
        None => pose.translation - up * 1.3 * unit,
    };
    let mut play = |effect: Option<u32>, at: Vec3| {
        let Some(id) = effect else { return };
        let mut fx = crate::swarm_fx::SwarmEffect::new(id);
        fx.position = at;
        fx.rotation = pose.rotation;
        fx.scale = fx_scale;
        fx.tag = EFFECT_TAG;
        fx.despawn_when_done = false;
        commands.spawn((fx, CreatureEditorOwned, Name::new(format!("creature editor effect {id:08X}"))));
    };
    play(setup.skybox_effect, pose.translation);
    match editor.mode {
        Mode::Build | Mode::Paint => play(setup.play_exit_effect, floor),
        Mode::Play => {
            // The generic entry effect carries the default background's scene (the
            // mountains and the growing trees of the creator room look, drawn here
            // only with a backdrop); the other backgrounds bring their own entry
            // effect and replace it (INFERRED from the models each effect holds).
            if room_on {
                play(setup.play_entry_effect, floor);
            }
            let bg = data.backgrounds_of(editor.kind).get(editor.play.background).and_then(|b| b.entry_effect);
            play(bg, floor);
        }
    }
}

// ---------------------------------------------------------------------------
// Render checks (GPU test)
// ---------------------------------------------------------------------------

/// Drive the Creature Creator through its real chrome, keyboard and modes in
/// the GPU render test (`runtime::render_original_install`).
pub(crate) fn render_smoke_checks(app: &mut App, window: Entity, step: &dyn Fn(&mut App)) {
    use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState};
    let press = |app: &mut App, keys: &[KeyCode]| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            for k in keys {
                app.world_mut().resource_mut::<Messages<KeyboardInput>>().write(KeyboardInput {
                    key_code: *k,
                    logical_key: Key::Character("".into()),
                    state,
                    text: None,
                    repeat: false,
                    window,
                });
            }
            step(app);
        }
        step(app);
    };
    for _ in 0..6 {
        step(app);
    }
    assert!(app.world().resource::<CreatureEditor>().active, "the Creature Creator session runs");
    let data = app.world().resource::<CreatureEditorData>().clone();
    assert!(data.loaded && data.errors.is_empty(), "{:?}", data.errors);
    let palette = data.parts_palette(SetupKind::Creature).cloned().expect("palette !0C");
    assert_eq!(palette.categories.len(), 7);
    {
        let world = app.world_mut();
        let roots = world.query::<&crate::creature_editor_ui::EditorRoot>().iter(world).count();
        assert!(roots >= 8, "the retail editor layouts are spawned ({roots})");
    }
    // Buy an eye pair through the palette press path (queued `Buy`).
    let (eye_page, eye) = {
        let ci = palette
            .categories
            .iter()
            .position(|c| c.name.contains("Eye"))
            .expect("Eyes & Senses tab");
        let run = app.world().resource::<RunProgress>();
        let originals = app.world().resource::<Originals>();
        let page = palette.page(ci, 0).unwrap().clone();
        let eye = page
            .items
            .iter()
            .find(|i| refusal(&data, SetupKind::Creature, run, originals, i.part).is_none_or(|r| r == Refusal::NoDna))
            .map(|i| i.part)
            .expect("a buyable eye");
        (ci, eye)
    };
    {
        let mut editor = app.world_mut().resource_mut::<CreatureEditor>();
        editor.category = eye_page;
        editor.page = 0;
    }
    app.world_mut().resource_mut::<RunProgress>().editor_dna = 500.0;
    step(app);
    step(app);
    let before = app.world().resource::<RunProgress>().parts.clone();
    app.world_mut().resource_mut::<EditorState>().queued.push(Action::Buy(eye));
    step(app);
    step(app);
    let after = app.world().resource::<RunProgress>().parts.clone();
    assert!(after.len() > before.len(), "a palette part is bought");
    let price = data.parts[&eye].price as f32;
    assert_eq!(app.world().resource::<RunProgress>().editor_dna, 500.0 - price, "one price for the pair");
    // Undo / redo through the retail keyboard table.
    press(app, &[KeyCode::ControlLeft, KeyCode::KeyZ]);
    assert_eq!(app.world().resource::<RunProgress>().parts, before);
    press(app, &[KeyCode::ControlLeft, KeyCode::KeyY]);
    assert_eq!(app.world().resource::<RunProgress>().parts, after);
    // Up arrow scales the selected part by one wheel notch.
    let focus = app.world().resource::<RunProgress>().focused_part as usize;
    let scale = app.world().resource::<RunProgress>().parts[focus].scale;
    press(app, &[KeyCode::ArrowUp]);
    let grown = app.world().resource::<RunProgress>().parts[focus].scale;
    assert!(grown > scale || grown >= data.parts[&eye].max_scale - 1e-4, "{scale} -> {grown}");
    // Delete sells the selection back.
    press(app, &[KeyCode::Delete]);
    assert_eq!(app.world().resource::<RunProgress>().parts, before);
    assert_eq!(app.world().resource::<RunProgress>().editor_dna, 500.0, "full refund");
    // Camera keys zoom and turn.
    let rig = app.world().resource::<CreatureEditor>().rig;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Period);
    step(app);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Period);
    step(app);
    assert!(app.world().resource::<CreatureEditor>().rig.heading > rig.heading);
    // Esc opens and closes the options panel.
    press(app, &[KeyCode::Escape]);
    assert!(app.world().resource::<CreatureEditor>().options_open);
    press(app, &[KeyCode::Escape]);
    assert!(!app.world().resource::<CreatureEditor>().options_open);
    println!("PASS Creature Creator: retail palette, purchase, undo/redo, arrow scale, delete refund, camera keys, options");
}

/// Switch the running session's mode (captures and checks).
pub(crate) fn force_mode(app: &mut App, mode: Mode) {
    let world = app.world_mut();
    world.resource_scope(|world, mut editor: Mut<CreatureEditor>| {
        let state = world.resource::<EditorState>();
        let data = world.resource::<CreatureEditorData>();
        let originals = world.resource::<Originals>();
        set_mode(&mut editor, state, data, originals, mode);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_click_soon_after_on_the_same_spot_is_a_double_click() {
        let at = Vec2::new(400.0, 300.0);
        assert!(!is_double_click(None, 1.0, at));
        assert!(is_double_click(Some((1.0, at)), 1.2, at + Vec2::new(3.0, 2.0)));
        // Too late, or too far away.
        assert!(!is_double_click(Some((1.0, at)), 1.9, at));
        assert!(!is_double_click(Some((1.0, at)), 1.2, at + Vec2::new(40.0, 0.0)));
    }

    /// Prints the block structure of the editor's effects (diagnostic).
    #[test]
    #[ignore = "requires the installed original Spore data"]
    fn installed_editor_effects_structure() {
        use crate::swarm_fx::{format::kind, SwarmLibrary};
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        fn walk(lib: &SwarmLibrary, r: crate::swarm_fx::EffectRef, depth: usize) {
            let dir = lib.dir(r.dir);
            let Some(e) = dir.effects.get(r.index as usize) else { return };
            for b in &e.blocks {
                let extra = match b.kind {
                    kind::MODEL => dir.models.get(b.index as usize).map(|m| format!("model {:08X}!{:08X} size {}", m.resource.group, m.resource.instance, m.size)),
                    kind::PARTICLES => dir.particles.get(b.index as usize).map(|_| "particles".to_string()),
                    kind::SCREEN => Some("screen".into()),
                    kind::LIGHT => Some("light".into()),
                    _ => None,
                };
                println!("{}kind {:02X} index {:08X} app {:08X}/{:08X} {}", "  ".repeat(depth), b.kind, b.index, b.app_flags, b.app_flags_mask, extra.unwrap_or_default());
                if b.kind == kind::EFFECT && depth < 3 {
                    if let Some(sub) = lib.resolve_effect(r.dir, b.index) {
                        walk(lib, sub, depth + 1);
                    }
                }
            }
        }
        for name in [
            "editor_part_mode_entry_extra_large",
            "editor_test_mode_entry_extra_large",
            "editor_test_mode_return",
            "editor_sci_fi_mode_entry",
            "editor_doom_mode_entry",
            "editor_black_mode_entry",
            "CR_direction_particles",
        ] {
            let id = crate::world_objects::name_id(name);
            match lib.find(id) {
                Some(r) => {
                    println!("== {name} ({id:08X})");
                    walk(&lib, r, 1);
                }
                None => println!("== {name} ({id:08X}) not installed"),
            }
        }
    }

    #[test]
    fn drag_turns_by_pi_times_0_0025_per_pixel_and_clamps_pitch() {
        let mut rig = EditorRig::default();
        let h = rig.heading;
        rig.drag(100.0, 0.0);
        assert!((rig.heading - h - PI * 0.25).abs() < 1e-5);
        rig.drag(0.0, 10_000.0);
        assert!((rig.pitch - (-55f32).to_radians()).abs() < 1e-5);
        rig.drag(0.0, -10_000.0);
        assert!((rig.pitch - 5f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn wheel_zooms_four_percent_a_notch_within_limits() {
        let mut rig = EditorRig::default();
        rig.wheel(120.0);
        assert!((rig.zoom - 4.8).abs() < 1e-5);
        for _ in 0..100 {
            rig.wheel(120.0);
        }
        assert_eq!(rig.zoom, 2.5);
        for _ in 0..100 {
            rig.wheel(-120.0);
        }
        assert_eq!(rig.zoom, 7.5);
    }

    #[test]
    fn smoothing_follows_the_retail_frame_factor() {
        assert_eq!(EditorRig::step(0.0), 0.01);
        assert!((EditorRig::step(1.0 / 60.0) - 0.1333).abs() < 1e-3);
        assert_eq!(EditorRig::step(1.0), 1.0);
        let mut rig = EditorRig::default();
        rig.zoom = 7.0;
        rig.update(0.5, 3.0);
        assert!((rig.s_zoom - 6.0).abs() < 1e-5);
    }

    #[test]
    fn default_view_shows_the_creature_left_front_and_right_of_centre() {
        let mut rig = EditorRig::default();
        for _ in 0..200 {
            rig.update(1.0, DEFAULT_FRAMING);
        }
        let t = camera_transform(&rig, Vec3::ZERO, Quat::IDENTITY, 1.0, DEFAULT_FRAMING);
        // The camera stands in front of the creature (its front is -Z) on its
        // left (-X) side, a little above it, about 5 units away.
        assert!(t.translation.z < 0.0 && t.translation.x < 0.0 && t.translation.y > 0.0, "{:?}", t.translation);
        assert!((t.translation.length() - 5.0).abs() < 1.0);
        // The creature appears right of the view centre (the palette is left).
        let local = t.rotation.conjugate() * (Vec3::ZERO - t.translation);
        assert!(local.z < 0.0, "in front of the camera");
        let x = local.x / -local.z;
        assert!(x > 0.15 && x < (0.5f32).tan(), "right of centre, inside the view: {x}");
        // The head (-Z) points to the screen's left.
        let head = t.rotation.conjugate() * Vec3::NEG_Z;
        assert!(head.x < 0.0);
    }

    #[test]
    fn editor_fov_keeps_its_vertical_angle() {
        // FUN_007C5400: horizontal 1 rad on 4:3, the same vertical angle on
        // every other screen; a square viewport takes the FOV itself.
        let v43 = retail_vertical_fov(1.0, 4.0 / 3.0);
        let h43 = 2.0 * ((v43 * 0.5).tan() * 4.0 / 3.0).atan();
        assert!((h43 - 1.0).abs() < 1e-5, "{h43}");
        assert!((retail_vertical_fov(1.0, 16.0 / 9.0) - v43).abs() < 1e-6);
        assert!((retail_vertical_fov(1.0, 1280.0 / 900.0) - v43).abs() < 1e-6);
        assert!((v43.to_degrees() - 44.6).abs() < 0.1, "{}", v43.to_degrees());
        assert!((retail_vertical_fov(1.0, 1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn framing_rule_zooms_out_until_every_block_shows() {
        // CellToCreature camera: initial 4.5, range 2.5..4.6.
        let props = EditorCameraProps { zoom: 4.5, min_zoom: 2.5, max_zoom: 4.6, ..default() };
        let z = framing_zoom(4.5, &props, 1.0, 0.1, true, false);
        assert!((z - 4.52).abs() < 1e-5, "out by framing x s x 0.2: {z}");
        assert_eq!(framing_zoom(4.59, &props, 1.0, 0.1, true, false), 4.6, "never past max");
        assert!((framing_zoom(4.55, &props, 1.0, 0.1, false, true) - 4.53).abs() < 1e-5);
        assert_eq!(framing_zoom(4.51, &props, 1.0, 0.1, false, true), 4.5, "never below the initial zoom");
        assert_eq!(framing_zoom(4.55, &props, 1.0, 0.1, false, false), 4.55, "inside, not inside the tighter view");
        // The frustum test: a box straight ahead is inside, one off to the side is not.
        let cam = Transform::from_xyz(0.0, 0.0, 5.0);
        let tan_v = (retail_vertical_fov(1.0, 1.4) * 0.5).tan();
        assert!(box_inside(&cam, Vec3::splat(-0.5), Vec3::splat(0.5), tan_v, 1.4, 0.2));
        assert!(!box_inside(&cam, Vec3::new(2.5, -0.5, -0.5), Vec3::new(3.5, 0.5, 0.5), tan_v, 1.4, 0.2));
        assert!(!box_inside(&cam, Vec3::new(-0.5, -0.5, 4.9), Vec3::new(0.5, 0.5, 6.0), tan_v, 1.4, 0.2), "behind the near plane");
    }

    #[test]
    fn roar_and_reactions_pick_randomly_sit_toggles() {
        let mut play = PlayState { seed: 7, ..default() };
        let roar = PLAY_ANIM_BUTTONS.iter().find(|b| b.control == 0x03E9_9D8C).unwrap();
        let picks: std::collections::HashSet<u32> = (0..40).map(|_| button_request(&mut play, roar)).collect();
        assert!(picks.len() > 1 && picks.iter().all(|p| roar.anims.contains(p)));
        let sit = PLAY_ANIM_BUTTONS.iter().find(|b| b.kind == PlayAnimKind::Toggle).unwrap();
        assert_eq!(button_request(&mut play, sit), 0x0407_9846);
        assert_eq!(button_request(&mut play, sit), 0x0407_9859);
        let mut last = None;
        for _ in 0..20 {
            let r = next_reaction(&mut play);
            assert_ne!(Some(r), last, "never the previous reaction");
            last = Some(r);
        }
    }

    #[test]
    fn actions_end_in_the_editor_idle() {
        let originals = Originals::default();
        let mut play = PlayState::default();
        start_anim(&mut play, &originals, 0x042D_D0E8, PlayAnimKind::Action);
        let d = play.anim.as_ref().unwrap().duration;
        advance_anim(&mut play, &originals, d + 0.01);
        assert_eq!(play.anim.as_ref().unwrap().id, IDLE_ANIM);
        assert_eq!(play.anim.as_ref().unwrap().kind, PlayAnimKind::Loop);
        let spin = PlayAnim { id: 0x042D_D0E8, kind: PlayAnimKind::Action, elapsed: 0.5, duration: 1.0 };
        let (offset, turn) = procedural_pose(&spin, false);
        assert!(offset.y > 0.3 && turn.angle_between(Quat::IDENTITY) > 3.0);
    }
}
