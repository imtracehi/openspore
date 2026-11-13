//! 3D scene of the Cell Creator: the editor camera, the editor light, the
//! installed pedestal model (`cl_editor_base_level0..4`: the concentric ring
//! platform and the microscope backdrop in one model), the cell body and its
//! parts, plus the picking geometry the editor places parts with.
//!
//! The scene lives on its own render layer and is drawn by its own camera
//! into the game camera's target; the game camera keeps drawing only the UI
//! (the SPUI composite) on top, so the Cell world is untouched while editing.

use bevy::{
    camera::{visibility::RenderLayers, ClearColorConfig, RenderTarget},
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
};

use crate::{
    cell_editor_data::{CellEditorData, CellPartInfo, Side},
    game::PartInstance,
};

/// Render layer of the editor scene (30 = mechanical preview, 31 = thumbnails).
pub const EDITOR_LAYER: usize = 29;
/// Layer the game camera is moved to while the editor is open: nothing lives
/// on it, so the game camera only draws its UI over the editor image.
pub const EMPTY_LAYER: usize = 28;

/// Retail `EditorCameraCellUI`: straight down (pitch -90), heading -90, zoom
/// 50 (min 25, max 50), FOV 0.09 rad, near clip 1. The FOV is the horizontal
/// one: the retail frames show the ring platform 46% of the view width (and
/// 60% of its height) at both 4:3 captures, which `fov` as Bevy's vertical
/// angle would not (35% / 46%).
pub const CAMERA_FOV: f32 = 0.09;

/// Vertical field of view of a perspective camera whose horizontal one is
/// `horizontal` (radians) in a view of `aspect` (width / height).
pub fn vertical_fov(horizontal: f32, aspect: f32) -> f32 {
    2.0 * ((horizontal * 0.5).tan() / aspect.max(0.1)).atan()
}

/// Vertical field of view the retail projection gives a camera FOV
/// (cViewer 0x7C5400, VERIFIED): the vertical half-angle tangent is
/// tan(fov / 2), times 0.75 whenever the view is not square; the horizontal
/// angle follows the view's shape. At 4:3 this equals treating `fov` as the
/// horizontal angle; wider views keep the vertical angle (and so the
/// platform's height on screen) and show more at the sides.
pub fn retail_vertical_fov(fov: f32, aspect: f32) -> f32 {
    let k = if (aspect - 1.0).abs() > 1e-3 { 0.75 } else { 1.0 };
    2.0 * ((fov * 0.5).tan() * k).atan()
}

/// ADAPTED: the retail Creator draws the cell about half the size of its
/// Cell-stage body next to the ring platform (frames: body 26% of the view
/// height against the ring's 60%). The subject root carries this scale; the
/// part placement maths stays in the cell's own (game) space.
pub const SUBJECT_SCALE: f32 = 0.5;
pub const CAMERA_ZOOM: f32 = 50.0;
pub const CAMERA_MIN_ZOOM: f32 = 25.0;
pub const CAMERA_MAX_ZOOM: f32 = 50.0;
pub const CAMERA_NEAR: f32 = 1.0;

/// Body radius the cell visuals are fitted to (`cell_visuals::centered_fit`).
pub const BODY_RADIUS: f32 = crate::cell::CELL_BODY_RADIUS;

#[derive(Component)]
pub struct CellEditorOwned;
#[derive(Component)]
pub struct CellEditorCamera;
/// The root every cell-model entity hangs from (body + parts + preview).
#[derive(Component)]
pub struct CellEditorSubject;
/// The editor's sun (dimmed in Paint mode).
#[derive(Component)]
pub struct CellEditorSun;
/// A mounted part mesh; the index is into `run.cell_parts`.
#[derive(Component)]
pub struct CellEditorPartMesh(#[allow(dead_code)] pub usize);
/// Meshes of a part that is being dragged (not yet in `run.cell_parts`).
#[derive(Component)]
pub struct CellEditorPreview;

/// Triangle soup of the body in its local space, for placement rays.
#[derive(Clone, Debug, Default)]
pub struct BodyShape {
    pub triangles: Vec<[Vec3; 3]>,
    pub min: Vec3,
    pub max: Vec3,
}

impl BodyShape {
    pub fn from_mesh(mesh: &Mesh) -> Option<Self> {
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            return None;
        };
        let p: Vec<Vec3> = positions.iter().map(|v| Vec3::from_array(*v)).collect();
        let indices: Vec<usize> = match mesh.indices() {
            Some(Indices::U32(i)) => i.iter().map(|v| *v as usize).collect(),
            Some(Indices::U16(i)) => i.iter().map(|v| *v as usize).collect(),
            None => (0..p.len()).collect(),
        };
        let triangles: Vec<[Vec3; 3]> = indices
            .chunks_exact(3)
            .filter(|t| t.iter().all(|i| *i < p.len()))
            .map(|t| [p[t[0]], p[t[1]], p[t[2]]])
            .collect();
        if triangles.is_empty() {
            return None;
        }
        let (min, max) = p.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), v| (a.min(*v), b.max(*v)));
        Some(Self { triangles, min, max })
    }

    /// An ellipsoid stand-in (tests, or a missing installed body).
    pub fn ellipsoid(radii: Vec3) -> Self {
        let mut mesh = Sphere::new(1.0).mesh().uv(32, 16);
        if let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            for p in v {
                *p = (Vec3::from_array(*p) * radii).to_array();
            }
        }
        Self::from_mesh(&mesh).unwrap_or_default()
    }

    /// Nearest hit of a ray (Moller-Trumbore, both faces).
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<(f32, Vec3, Vec3)> {
        let mut best: Option<(f32, Vec3, Vec3)> = None;
        for [a, b, c] in &self.triangles {
            let (e1, e2) = (*b - *a, *c - *a);
            let p = dir.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-9 {
                continue;
            }
            let inv = 1.0 / det;
            let s = origin - *a;
            let u = s.dot(p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = dir.dot(q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let t = e2.dot(q) * inv;
            if t > 1e-5 && best.is_none_or(|(bt, ..)| t < bt) {
                let mut n = e1.cross(e2).normalize_or_zero();
                if n.dot(dir) > 0.0 {
                    n = -n;
                }
                best = Some((t, origin + dir * t, n));
            }
        }
        best
    }

    /// Membrane point in a horizontal direction (on the body's equator).
    pub fn rim(&self, dir: Vec2) -> Vec3 {
        let d = Vec3::new(dir.x, 0.0, dir.y).normalize_or(Vec3::NEG_Z);
        let y = (self.min.y + self.max.y) * 0.5;
        let far = (self.max - self.min).length() + 1.0;
        let origin = d * far + Vec3::Y * y;
        self.raycast(origin, -d)
            .map(|(_, p, _)| p)
            .unwrap_or(d * BODY_RADIUS + Vec3::Y * y)
    }

    /// Top surface point above (x, z), if the body is there.
    pub fn top(&self, x: f32, z: f32) -> Option<Vec3> {
        let origin = Vec3::new(x, self.max.y + 1.0, z);
        self.raycast(origin, Vec3::NEG_Y).map(|(_, p, _)| p)
    }

    /// Distance from the centre to the rim along a horizontal direction.
    pub fn rim_radius(&self, dir: Vec2) -> f32 {
        let p = self.rim(dir);
        Vec2::new(p.x, p.z).length()
    }
}

/// Where a part goes on the body: the attachment point (body space), the
/// outward horizontal direction the part faces and the symmetry side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub point: Vec3,
    pub dir: Vec3,
    pub on_top: bool,
    pub side: Side,
}

/// ADAPTED: how far outside the membrane (in body radii) a drop still attaches.
/// Retail highlights the part red where it cannot be placed; the exact reach
/// is not decoded.
pub const ATTACH_REACH: f32 = 0.85;

/// Placement of a part under a body-space ray. Parts snap to the membrane in
/// the direction of the cursor; eyes (`modelCellAllowOnTopOfBody`) may sit on
/// top of the body. Positions within the part's `modelSymmetrySnapDelta` of
/// the centreline snap onto it. `None` when the cursor is too far away.
pub fn place(shape: &BodyShape, info: &CellPartInfo, origin: Vec3, dir: Vec3) -> Option<Placement> {
    let snap = info.symmetry_snap.max(0.03);
    if info.allow_on_top {
        if let Some((_, hit, _)) = shape.raycast(origin, dir) {
            let mut point = hit;
            if point.x.abs() < snap {
                point.x = 0.0;
                point = shape.top(0.0, point.z).unwrap_or(point);
            }
            let flat = Vec2::new(point.x, point.z);
            let dir = if flat.length() > 1e-3 {
                Vec3::new(flat.x, 0.0, flat.y).normalize()
            } else {
                Vec3::NEG_Z
            };
            return Some(Placement {
                point,
                dir,
                on_top: true,
                side: side_of(point.x),
            });
        }
    }
    // The equator plane under the cursor.
    let y = (shape.min.y + shape.max.y) * 0.5;
    if dir.y.abs() < 1e-6 {
        return None;
    }
    let t = (y - origin.y) / dir.y;
    if t <= 0.0 {
        return None;
    }
    let q = origin + dir * t;
    let flat = Vec2::new(q.x, q.z);
    let mut d = if flat.length() > 1e-4 { flat.normalize() } else { Vec2::NEG_Y };
    let radius = shape.rim_radius(d);
    if flat.length() > radius * (1.0 + ATTACH_REACH) {
        return None;
    }
    let mut point = shape.rim(d);
    if point.x.abs() < snap {
        // Onto the centreline: the front or back of the body.
        d = Vec2::new(0.0, d.y.signum());
        point = shape.rim(d);
        point.x = 0.0;
    }
    Some(Placement {
        point,
        dir: Vec3::new(d.x, 0.0, d.y).normalize_or(Vec3::NEG_Z),
        on_top: false,
        side: side_of(point.x),
    })
}

pub fn side_of(x: f32) -> Side {
    if x.abs() < 1e-4 {
        Side::Center
    } else if x < 0.0 {
        Side::Left
    } else {
        Side::Right
    }
}

/// Yaw that turns a part's outward axis (model -Y in Spore space, +Z after
/// the importer's Z-up to Y-up turn) to face `dir`.
pub fn yaw_for(dir: Vec3) -> f32 {
    dir.x.atan2(dir.z)
}

/// Saved offset for a body-space attachment point. Every editor part uses
/// slot 0, so `cell_slot(0) + offset` (the in-game formula of
/// `editor::refresh_parts`) is exactly the point.
pub fn offset_for(point: Vec3) -> [f32; 3] {
    (point - crate::editor::cell_slot(0)).to_array()
}

pub fn point_of(part: &PartInstance) -> Vec3 {
    crate::editor::cell_slot(part.bone) + Vec3::from_array(part.offset)
}

/// Local transform of a mounted part, identical to the in-game one
/// (`editor::refresh_parts`).
pub fn part_transform(part: &PartInstance, model_scale: f32) -> Transform {
    Transform::from_translation(point_of(part))
        .with_rotation(Quat::from_rotation_y(part.yaw))
        .with_scale(part.mesh_scale(model_scale * 0.35 * part.scale))
}

/// Editor copy of a model material whose colour texture samples as sRGB.
///
/// Spore's editors shade in gamma space on D3D9 (no sRGB sampling, no sRGB
/// back buffer), so an unlit texel reaches the screen unchanged: the level-0
/// backdrop raster averages (54, 60, 131) and the retail frames show
/// (57, 64, 127). The shared importer uploads these rasters as linear
/// `Rgba8Unorm`, which an sRGB target brightens (to ~(130, 138, 190)); the
/// editor draws with copies that decode the same bytes as sRGB instead,
/// leaving the shared materials of the Cell stage untouched.
pub fn gamma_material(
    cache: &mut std::collections::HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    source: &Handle<StandardMaterial>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Handle<StandardMaterial> {
    use bevy::render::render_resource::TextureFormat;
    if let Some(copy) = cache.get(&source.id()) {
        return copy.clone();
    }
    let Some(mut material) = materials.get(source).cloned() else {
        return source.clone();
    };
    let texture = material.base_color_texture.as_ref().and_then(|t| images.get(t)).and_then(|image| {
        (image.texture_descriptor.format == TextureFormat::Rgba8Unorm && image.data.is_some()).then(|| {
            let mut image = image.clone();
            image.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
            if let Some(view) = image.texture_view_descriptor.as_mut() {
                view.format = Some(TextureFormat::Rgba8UnormSrgb);
            }
            image
        })
    });
    let Some(texture) = texture else {
        cache.insert(source.id(), source.clone());
        return source.clone();
    };
    material.base_color_texture = Some(images.add(texture));
    let copy = materials.add(material);
    // The copy keeps the source's `materialParams` (`cell_sh`).
    crate::cell_sh::copy_params(source.id(), copy.id());
    cache.insert(source.id(), copy.clone());
    copy
}

/// Fallback Bevy light levels (overridable through `OS_CELL_EDITOR_AMB` /
/// `OS_CELL_EDITOR_SUN`) of a material that is not SH lit; the editor's models
/// are lit by the retail SH light (`cell_sh`), which replaces these.
pub const AMBIENT_BRIGHTNESS: f32 = 350.0;
pub const SUN_ILLUMINANCE: f32 = 2000.0;

/// Saved state of the game camera while the editor draws.
#[derive(Clone, Default)]
pub struct SavedGameCamera {
    pub entity: Option<Entity>,
    pub layers: Option<RenderLayers>,
    pub clear: Option<ClearColorConfig>,
}

/// Turn the game camera into a UI-only overlay and spawn the editor camera on
/// the same render target. Returns the saved game camera state.
pub fn spawn_camera(
    commands: &mut Commands,
    game: Option<(Entity, Mut<Camera>, Option<&RenderLayers>, Option<&RenderTarget>)>,
    data: &CellEditorData,
    clear: Color,
) -> SavedGameCamera {
    let mut saved = SavedGameCamera::default();
    let mut target = RenderTarget::default();
    if let Some((entity, mut camera, layers, game_target)) = game {
        target = game_target.cloned().unwrap_or_default();
        saved = SavedGameCamera {
            entity: Some(entity),
            layers: layers.cloned(),
            clear: Some(camera.clear_color.clone()),
        };
        // The game camera keeps the editor image and adds its UI on top.
        camera.clear_color = ClearColorConfig::None;
        commands
            .entity(entity)
            .insert(RenderLayers::layer(EMPTY_LAYER));
    }
    // Fallback only (see `cell_sh`): the first SH coefficient's hue as Bevy
    // ambient light for a material that is not SH lit.
    let ambient = data
        .light
        .as_ref()
        .map(|l| {
            let m = l.ambient.iter().cloned().fold(1e-3, f32::max);
            Color::srgb(l.ambient[0] / m, l.ambient[1] / m, l.ambient[2] / m)
        })
        .unwrap_or(Color::WHITE);
    commands.spawn((
        CellEditorOwned,
        CellEditorCamera,
        Name::new("cell-editor-camera"),
        Camera3d::default(),
        // Spore draws its editors as plain LDR (like the Cell stage camera in
        // `cell_fx::ldr_camera`): the unlit backdrop must keep its authored
        // colours instead of the default filmic curve's washed-out ones.
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Camera {
            // Before the game camera, which then adds only its UI.
            order: -1,
            // ADAPTED: beyond the backdrop plane (under the palette) the view
            // shows the backdrop's edge colour.
            clear_color: ClearColorConfig::Custom(clear),
            ..default()
        },
        target,
        Projection::Perspective(PerspectiveProjection {
            fov: CAMERA_FOV,
            near: CAMERA_NEAR,
            far: 400.0,
            ..default()
        }),
        RenderLayers::layer(EDITOR_LAYER),
        AmbientLight {
            color: ambient,
            brightness: crate::world_presentation::tune("CELL_EDITOR_AMB", AMBIENT_BRIGHTNESS),
            ..default()
        },
        camera_transform(0.0, CAMERA_ZOOM, CAMERA_OFFSET),
    ));
    // `lightSunDir` points at the sun in Spore space (Z up, front -Y); the
    // scene is Spore (x, y, z) -> Bevy (-x, z, y) (front -Z, see `cell_sh`).
    let sun = data
        .light
        .as_ref()
        .map(|l| Vec3::new(-l.sun_dir[0], l.sun_dir[2], l.sun_dir[1]))
        .unwrap_or(Vec3::new(0.75, 1.0, -0.1))
        .normalize_or(Vec3::Y);
    let color = data
        .light
        .as_ref()
        .map(|l| Color::srgb(l.sun_color[0], l.sun_color[1], l.sun_color[2]))
        .unwrap_or(Color::WHITE);
    commands.spawn((
        CellEditorOwned,
        CellEditorSun,
        Name::new("cell-editor-sun"),
        DirectionalLight {
            color,
            illuminance: crate::world_presentation::tune("CELL_EDITOR_SUN", SUN_ILLUMINANCE),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(sun * 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(EDITOR_LAYER),
    ));
    saved
}

/// Put the game camera back the way it was.
pub fn restore_camera(commands: &mut Commands, saved: &SavedGameCamera, camera: Option<Mut<Camera>>) {
    let Some(entity) = saved.entity else {
        return;
    };
    match &saved.layers {
        Some(layers) => {
            commands.entity(entity).insert(layers.clone());
        }
        None => {
            commands.entity(entity).remove::<RenderLayers>();
        }
    }
    if let (Some(mut camera), Some(clear)) = (camera, saved.clear.clone()) {
        camera.clear_color = clear;
    }
}

/// Retail `EditorCameraCellUI` `cameraInitialOffsetY`: the view centre sits
/// this far in front of the cell (screen left), so the cell appears right of
/// the palette.
pub const CAMERA_OFFSET: f32 = 0.8;

/// Editor camera placement for a turntable heading and zoom: straight down,
/// the cell's front (-Z) to the left of the screen like the retail frames
/// (heading -90), looking at a point `offset` in front of the cell. The offset
/// turns with the heading, so the cell keeps its place on screen.
pub fn camera_transform(heading: f32, distance: f32, offset: f32) -> Transform {
    let turn = Quat::from_rotation_y(heading);
    let up = turn * Vec3::X;
    let target = turn * Vec3::new(0.0, 0.0, -offset);
    Transform::from_translation(target + Vec3::Y * distance).looking_at(target, up)
}

/// Mean colour of the outer ring of texels of an image (the backdrop's edge),
/// used to fill the view beyond the pedestal's backdrop plane.
pub fn border_color(image: &Image) -> Option<Color> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data.as_ref()?;
    if w < 2 || h < 2 || data.len() < w * h * 4 {
        return None;
    }
    let mut sum = [0f32; 3];
    let mut n = 0.0;
    let mut total = 0.0;
    let mut add = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        total += 1.0;
        if data[i + 3] > 200 {
            for c in 0..3 {
                sum[c] += data[i + c] as f32;
            }
            n += 1.0;
        }
    };
    for x in 0..w {
        add(x, 0);
        add(x, h - 1);
    }
    for y in 0..h {
        add(0, y);
        add(w - 1, y);
    }
    // Only a texture that fills its whole square (the backdrop, not the ring
    // disc) has an edge colour.
    (n > 0.0 && n * 2.0 >= total)
        .then(|| Color::srgb_u8((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_editor_data::PartClass;

    fn info(on_top: bool) -> CellPartInfo {
        CellPartInfo {
            key: crate::cell_editor_data::prop_key(crate::asset_catalog::CELL_PARTS, 1),
            name: "Test".into(),
            price: 10,
            complexity: 6,
            class: if on_top { PartClass::Eye } else { PartClass::Mouth },
            subtype: 0,
            unlock: None,
            left: None,
            right: None,
            center: None,
            game_speed: 0,
            allow_on_top: on_top,
            remain_upright: false,
            point_forward: false,
            symmetry_snap: 0.1,
            replace_snap: 0.0,
            snap_replace: vec![],
            min_scale: 0.5,
            max_scale: 1.5,
            sell_back: 1.0,
            icon: crate::cell_editor_data::prop_key(0, 0),
            model: 1,
        }
    }

    #[test]
    fn retail_projection_keeps_the_vertical_angle_on_wide_views() {
        let v43 = retail_vertical_fov(CAMERA_FOV, 4.0 / 3.0);
        assert!((v43 - vertical_fov(CAMERA_FOV, 4.0 / 3.0)).abs() < 1e-6, "same as horizontal at 4:3");
        assert_eq!(retail_vertical_fov(CAMERA_FOV, 16.0 / 9.0), v43);
        assert!((retail_vertical_fov(CAMERA_FOV, 1.0) - CAMERA_FOV).abs() < 1e-6, "square views keep the angle");
        // 1 rad: 44.6 degrees vertical on any non-square view.
        assert!((retail_vertical_fov(1.0, 1.6).to_degrees() - 44.6).abs() < 0.05);
    }

    #[test]
    fn horizontal_fov_becomes_a_narrower_vertical_one() {
        // A square view keeps the angle, a 4:3 view narrows the vertical one.
        assert!((vertical_fov(0.09, 1.0) - 0.09).abs() < 1e-6);
        let v = vertical_fov(0.09, 4.0 / 3.0);
        assert!((v - 0.0675).abs() < 5e-4, "{v}");
        // The visible width at distance 50 is 4.5 units whatever the aspect.
        let width = 2.0 * 50.0 * (v * 0.5).tan() * (4.0 / 3.0);
        assert!((width - 4.5).abs() < 0.01, "{width}");
    }

    #[test]
    fn rim_and_top_follow_the_body_surface() {
        let shape = BodyShape::ellipsoid(Vec3::new(0.5, 0.3, 0.9));
        let side = shape.rim(Vec2::new(1.0, 0.0));
        assert!((side.x - 0.5).abs() < 0.02, "{side:?}");
        let front = shape.rim(Vec2::new(0.0, -1.0));
        assert!((front.z + 0.9).abs() < 0.02, "{front:?}");
        let top = shape.top(0.0, 0.0).unwrap();
        assert!((top.y - 0.3).abs() < 0.02, "{top:?}");
        assert!(shape.top(2.0, 0.0).is_none());
    }

    #[test]
    fn drops_snap_to_the_membrane_the_centreline_and_refuse_far_away() {
        let shape = BodyShape::ellipsoid(Vec3::new(0.5, 0.3, 0.9));
        let down = Vec3::NEG_Y;
        // Over the front, slightly off the centreline: snapped onto it.
        let p = place(&shape, &info(false), Vec3::new(0.03, 10.0, -1.2), down).unwrap();
        assert_eq!(p.side, Side::Center);
        assert_eq!(p.point.x, 0.0);
        assert!((p.point.z + 0.9).abs() < 0.03);
        assert!((yaw_for(p.dir).abs() - std::f32::consts::PI).abs() < 1e-3, "faces forward");
        // Over the right flank: a right-side placement facing +X.
        let p = place(&shape, &info(false), Vec3::new(0.8, 10.0, 0.0), down).unwrap();
        assert_eq!(p.side, Side::Right);
        assert!((p.point.x - 0.5).abs() < 0.03);
        assert!((yaw_for(p.dir) - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
        // Far away: cannot be placed (shown red, refunded on release).
        assert!(place(&shape, &info(false), Vec3::new(3.0, 10.0, 0.0), down).is_none());
        // Eyes may sit on top of the body.
        let p = place(&shape, &info(true), Vec3::new(-0.2, 10.0, -0.3), down).unwrap();
        assert!(p.on_top && p.side == Side::Left && p.point.y > 0.1);
    }

    #[test]
    fn saved_offsets_reproduce_the_in_game_mount() {
        let point = Vec3::new(0.3, 0.1, -0.5);
        let mut part = PartInstance::new(1, 0);
        part.offset = offset_for(point);
        part.yaw = yaw_for(Vec3::X);
        assert!((point_of(&part) - point).length() < 1e-6);
        let t = part_transform(&part, 2.0);
        assert!((t.translation - point).length() < 1e-6);
        assert!((t.scale.x - 0.7).abs() < 1e-6);
    }
}
