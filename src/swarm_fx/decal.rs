//! Swarm decal components (`decal`, component type 0x03): the marks projected
//! onto the ground - tribal tool pads, scorched huts, footprints, crop
//! circles, colony pads, rare crash sites, explosion scars, volcano glows.
//!
//! NATIVE (effect data `format::DecalDef`; code image, ModAPI march2017
//! addresses; notes `notes\swarm-decal-notes.md`):
//! * Component layout (SporeModder-FX `DecalEffect`): flags, `type`
//!   (`Swarm::DecalType`: 0 terrain, 1 water, 2 terrainAndWater, 3 paint,
//!   4..11 user1..8), `lifeType` (0 loop, 1 single, 2 sustain), a texture slot
//!   (`layer` 0..3 = the terrain's four decal lists, drawn in that order),
//!   life, curves for rotation (turns), size, alpha, colour, aspect, their
//!   `vary` values, repeat / offset (unused by the installed data) and an
//!   effect map for the colour (unused here).
//! * The terrain (`Terrain::cTerrainSphere`, decal lists 0x218 x 4, creation
//!   0xFA2860 -> `cTerrainSphereDecal` ctor 0xFAD6E0 / `Initialize` 0xFAEC20)
//!   owns the decals; `cTerrainSphereDecal::UpdateDecal` 0xFAE7B0 turns the
//!   decal transform into the texture mapping: the orientation is made
//!   tangent to the sphere at the decal centre, rotated by `turns * 2 pi`, the
//!   aspect (forced to 1 for type 9) stretches V, and the shader data
//!   0x304 `decalState` carries `DecalColorTint` (colour, alpha; alpha 1 for
//!   type 9) and `dirScaleU` / `dirScaleV` (direction x scale, plus the
//!   offset that puts UV 0.5 at the centre; V is negated). The terrain vertex
//!   shaders (`TerrainDecalAboveVS*`, pack 0x40212002) compute `decalUV =
//!   (dot(dirScaleU, position), dot(dirScaleV, position))` per terrain
//!   vertex: a planar projection onto the terrain mesh. The pixel shaders
//!   (`TerrainDecalAbovePS*`) draw `tex(decalUV) * DecalColorTint`, lit and
//!   fogged like the terrain; `cTerrainSphereQuad::RenderDecals` 0xFB3010
//!   picks the material per decal: terrain 0x01C87A08, water 0x328F9A63,
//!   terrain and water 0xEAFC74F4, emissive 0x22A17AD9 (sphere-decal flag 2 set by
//!   `SetDecalEmissive`; not traced which decals use it, not drawn here).
//!
//! ADAPTED (not traced):
//! * Size: the effect `size` (x the block scale, x the parent's size
//!   modifiers) is the full edge length of the decal in world units (retail:
//!   the transform scale feeding `0.5 / scale`; whether the Swarm instance
//!   passes the full edge or half of it was not traced: the installed sizes
//!   - tool pads 6 m, destroyed huts 5.5 m, totem pad 2.2 m - fit a full
//!   edge).
//! * Projection: a grid of the decal's footprint draped over the stage
//!   ground along the world vertical (`LandSurface::ground_height`, the
//!   water surface for water types), lifted a little above it (grows with
//!   the camera distance) instead of the terrain shader pass.
//! * Space planet view (`space_planets::PlanetViewGround`, the height
//!   function the planet view draws): the tangent plane at the decal centre
//!   is the sphere's (UpdateDecal 0xFAE7B0 / 0xFAD180: normal = normalised
//!   offset from the planet centre), the texture right is the effect's
//!   right projected onto it, and the footprint grid is draped radially
//!   over `Planet::height` (the water radius for water types). The UVs are
//!   the planar projection of the draped vertices onto that tangent plane,
//!   as the terrain vertex shader computes them (`dirScaleU` / `dirScaleV`).
//!   The cell size, the lift and the size all follow the planet view's
//!   scene scale (`PlanetViewGround::unit`: scene units per retail unit);
//!   effects played there carry that scale as their effect scale.
//! * Light: the mark is drawn unlit; blended colour is clamped to 1 (retail
//!   tints above 1 compensate the terrain lighting).
//! * Life: see `sim::DecalState`; paint decals (type 3) stay on the ground
//!   after their effect ended (the newest [`MAX_MARKS`]; retail paints
//!   permanently into the terrain).
//! * `vary` values are symmetric relative variations like the particles';
//!   user types 4..8 (other decal managers: lights, ...) are not drawn; type
//!   9 (`crg_decal_nest_*`) draws like a terrain decal with the retail tint
//!   alpha 1 and aspect 1 (UpdateDecal 0xFAE7B0, RenderDecals 0xFB3010).

use std::collections::{HashSet, VecDeque};

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::format::{decal_flag, decal_type, DecalDef};
use super::render::{srgb_to_linear, Blend, SwarmDraw, SwarmMaterial, SwarmRenderCache};
use super::sim::{curve, curve3, DecalState, Frame};
use super::{SwarmEffect, SwarmLibrary, SwarmSet};

/// Most grid cells along one edge of a decal.
const GRID_MAX: usize = 24;
/// Paint marks kept on the ground after their effects ended.
pub const MAX_MARKS: usize = 48;

/// Whether this runtime draws a decal type (see the module notes).
pub fn drawn_type(kind: u8) -> bool {
    matches!(kind, 0..=3 | 9)
}

/// What a decal looks like at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecalNow {
    /// Edge lengths (U, V) in world units.
    pub size: Vec2,
    /// Rotation in radians (turns x 2 pi).
    pub angle: f32,
    /// Tint (rgb) and alpha, gamma-space values as authored.
    pub color: Vec3,
    pub alpha: f32,
}

/// Evaluate the curves of `def` for a decal of the given state.
pub fn evaluate(def: &DecalDef, state: &DecalState, scale: f32) -> DecalNow {
    let t = state.time(def);
    let size = curve(&def.size, t, 1.0) * (1.0 + def.size_vary * state.rolls[1]) * state.mods.size * scale;
    // UpdateDecal 0xFAE7B0: the aspect of type 9 is forced to 1.
    let aspect = if def.kind == 9 { 1.0 } else { curve(&def.aspect, t, 1.0) };
    let mut alpha = (curve(&def.alpha, t, 1.0) * (1.0 + def.alpha_vary * state.rolls[2])).clamp(0.0, 1.0) * state.mods.alpha;
    if def.kind == 9 {
        // RenderDecals 0xFB3010: the tint alpha of type 9 is 1.
        alpha = 1.0;
    }
    let turns = curve(&def.rotation, t, 0.0) + def.rotation_vary * state.rolls[0];
    DecalNow {
        size: Vec2::new(size, size * aspect),
        angle: turns * std::f32::consts::TAU,
        color: (curve3(&def.color, t) * state.mods.color).max(Vec3::ZERO),
        alpha,
    }
}

/// Vertex buffers of one decal (positions relative to `origin`).
#[derive(Default)]
pub struct DecalMesh {
    pub positions: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub uv_b: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl DecalMesh {
    fn write(self, mesh: &mut Mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv_b);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
    }
    fn placeholder() -> Self {
        DecalMesh {
            positions: vec![[0.0; 3]; 3],
            uv: vec![[0.0; 2]; 3],
            uv_b: vec![[0.0; 2]; 3],
            colors: vec![[0.0; 4]; 3],
            indices: vec![0, 1, 2],
        }
    }
}

/// What a decal lies on.
#[derive(Clone, Copy)]
pub enum Ground<'a> {
    /// No surface known: a flat plane at the decal origin, along its normal.
    Flat,
    /// A flat stage: the surface height under a world (x, z), draped along
    /// the world vertical.
    Height(&'a dyn Fn(f32, f32) -> f32),
    /// A planet: the surface point under a world point and the outward
    /// normal there (radial); `None` when the point has no surface.
    Planet(&'a dyn Fn(Vec3) -> Option<(Vec3, Vec3)>),
}

/// Pure: the footprint of a decal draped over the ground. `frame` is the
/// block frame (Spore X = texture right, Y = texture up, Z = the surface
/// normal), `ground` the surface under it, `lift` how far the mesh floats
/// above it and `cell` the grid cell edge in world units. UV (0.5, 0.5) is
/// the centre; V is negated as in the shader data (`dirScaleV`), so texture
/// up points along Spore +Y.
pub fn decal_mesh(
    now: &DecalNow,
    def: &DecalDef,
    frame: &Frame,
    ground: Ground,
    lift: f32,
    cell: f32,
    rgba: [f32; 4],
) -> DecalMesh {
    let mut out = DecalMesh::default();
    if !(now.size.x > 1e-4 && now.size.y > 1e-4) || rgba[3] <= 0.002 {
        return out;
    }
    let mut right = (frame.rot * Vec3::X).normalize_or_zero();
    let mut up_tex = (frame.rot * Vec3::Y).normalize_or_zero();
    let mut normal = (frame.rot * Vec3::Z).normalize_or_zero();
    // The surface point under the centre (planets): the decal's own frame is
    // re-oriented to the sphere's tangent plane there (0xFAD180).
    let mut centre = frame.origin;
    if let Ground::Planet(project) = ground {
        if let Some((point, outward)) = project(frame.origin) {
            let tangent = right - outward * right.dot(outward);
            let tangent = if tangent.length_squared() > 1e-6 {
                tangent.normalize()
            } else {
                // The effect's right is along the normal: any tangent.
                outward.any_orthonormal_vector()
            };
            centre = point;
            normal = outward;
            right = tangent;
            up_tex = outward.cross(tangent);
        }
    }
    let (s, c) = now.angle.sin_cos();
    // The texture basis turned by the decal's rotation.
    let dir_u = right * c + up_tex * s;
    let dir_v = up_tex * c - right * s;
    let longest = now.size.x.max(now.size.y);
    let cells = ((longest / cell.max(1e-6)).ceil() as usize).clamp(4, GRID_MAX);
    let repeat = if def.flags & decal_flag::TEXTURE_REPEAT != 0 { def.texture_repeat } else { 1.0 };
    for j in 0..=cells {
        for i in 0..=cells {
            let a = i as f32 / cells as f32 - 0.5;
            let b = j as f32 / cells as f32 - 0.5;
            let mut p = centre + dir_u * (a * now.size.x) + dir_v * (b * now.size.y);
            let (mut ua, mut vb) = (a, b);
            match ground {
                Ground::Height(h) => p.y = h(p.x, p.z) + lift,
                Ground::Planet(project) => {
                    if let Some((surface, outward)) = project(p) {
                        p = surface + outward * lift;
                        // The terrain shader's planar projection of the
                        // draped vertex (dirScaleU / dirScaleV).
                        let d = surface - centre;
                        ua = d.dot(dir_u) / now.size.x;
                        vb = d.dot(dir_v) / now.size.y;
                    }
                }
                Ground::Flat => p += normal * lift,
            }
            out.positions.push((p - frame.origin).to_array());
            out.uv.push([
                (0.5 + ua) * repeat + def.texture_offset[0],
                (0.5 - vb) * repeat + def.texture_offset[1],
            ]);
            out.uv_b.push([0.0, 0.0]);
            // A soft edge guard: the texture border alpha is authored zero in
            // the installed decals; clamp addressing keeps the rest clean.
            out.colors.push(rgba);
        }
    }
    let row = cells as u32 + 1;
    for j in 0..cells as u32 {
        for i in 0..cells as u32 {
            let k = j * row + i;
            // Counter-clockwise seen from above (+Y), either way drawn (no culling).
            out.indices.extend([k, k + row, k + 1, k + 1, k + row, k + row + 1]);
        }
    }
    out
}

/// One decal's draw entity.
struct DecalDraw {
    entity: Entity,
    layer: usize,
    paint: bool,
}

#[derive(Resource, Default)]
pub struct SwarmDecalCache {
    draws: std::collections::HashMap<u64, DecalDraw>,
    /// Paint marks that outlived their effects, oldest first.
    marks: VecDeque<Entity>,
    reported: HashSet<(u16, u32)>,
}

/// Marker for decal draw entities.
#[derive(Component)]
pub struct SwarmDecalDraw;

/// Registers the decal draw system (called by `SwarmFxRenderPlugin`).
pub fn build(app: &mut App) {
    app.init_resource::<SwarmDecalCache>().add_systems(
        PostUpdate,
        draw_decals
            .in_set(SwarmSet::Draw)
            .after(SwarmSet::Simulate)
            .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_decals(
    mut commands: Commands,
    lib: Res<SwarmLibrary>,
    originals: Option<Res<crate::Originals>>,
    mut render_cache: ResMut<SwarmRenderCache>,
    mut cache: ResMut<SwarmDecalCache>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SwarmMaterial>>,
    editor: Option<Res<crate::editor::EditorState>>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    planet: Option<Res<crate::space_planets::PlanetViewGround>>,
    stage: Option<Res<State<crate::stages::GameStage>>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    effects: Query<&SwarmEffect>,
    mut draws: Query<(&mut Transform, &Mesh3d, &mut MeshMaterial3d<SwarmMaterial>, &mut Visibility), With<SwarmDecalDraw>>,
) {
    let Some(originals) = originals else { return };
    let Some(camera) = camera.iter().next() else { return };
    // Escape hatch: OPENSPORE_DECALS=0 draws no decals.
    if std::env::var_os("OPENSPORE_DECALS").is_some_and(|v| v == "0") {
        return;
    }
    let camera_at = camera.translation();
    let hidden = editor.is_some_and(|e| e.open);
    // The stage ground, when it belongs to the running stage.
    let surface = surface.filter(|s| stage.as_ref().is_none_or(|g| *g.get() == s.stage));
    // The Space planet view's world (it only exists while that view runs).
    let planet = planet.filter(|_| stage.as_ref().is_none_or(|g| *g.get() == crate::stages::GameStage::Space));
    // Escape hatch: OPENSPORE_DECAL_PLANET=0 puts decals in the Space planet
    // view on a flat plane at the effect origin again (the earlier look).
    let planet = planet.filter(|_| !std::env::var_os("OPENSPORE_DECAL_PLANET").is_some_and(|v| v == "0"));
    let planet = planet.as_deref();
    // Scene units per retail unit: 1 on the flat stages.
    let unit = planet.map_or(1.0, |g| g.unit());
    let mut seen = HashSet::new();
    let cache = &mut *cache;
    for effect in &effects {
        let Some(instance) = &effect.instance else { continue };
        instance.for_each_decal(&lib, &mut |state: &DecalState, def: &DecalDef| {
            if !drawn_type(def.kind) {
                return;
            }
            let Some(frame) = state.frame else { return };
            let look = render_cache.slot_look(&lib, &originals, &mut images, &mut materials, state.dir, &def.texture, false, false);
            let Some(look) = look else {
                if cache.reported.insert((state.dir, state.index)) {
                    warn!(
                        "Swarm decal {} (dir {}) has no drawable texture {:08x}!{:08x}",
                        state.index, state.dir, def.texture.resource.group, def.texture.resource.instance
                    );
                }
                return;
            };
            seen.insert(state.uid);
            let now = evaluate(def, state, frame.scale);
            // The texture slot's own draw mode (mostly alpha; the volcano
            // glows are additive).
            let additive = look.blend == Blend::Additive;
            let color = if additive { now.color } else { now.color.min(Vec3::ONE) };
            let rgba = [
                srgb_to_linear(color.x),
                srgb_to_linear(color.y),
                srgb_to_linear(color.z),
                now.alpha,
            ];
            let distance = (frame.origin - camera_at).length();
            let layer_lift = def.texture.layer.clamp(0, 3) as f32 * 0.012;
            // The lift is in retail units over a planet (scene units x 1 / unit).
            let lift = (0.04 + layer_lift + distance / unit * 0.0015).min(0.8) * unit;
            let water = matches!(def.kind, decal_type::WATER | decal_type::TERRAIN_AND_WATER);
            let height = surface
                .as_deref()
                .map(|s| move |x: f32, z: f32| if water { s.height(x, z) } else { s.ground_height(x, z) });
            let project = planet.map(|g| move |p: Vec3| g.surface_under(p, water));
            let ground = match (&project, &height) {
                (Some(f), _) => Ground::Planet(f),
                (None, Some(f)) => Ground::Height(f),
                _ => Ground::Flat,
            };
            let mesh = if hidden || state.stopped && def.kind != decal_type::PAINT {
                DecalMesh::default()
            } else {
                decal_mesh(&now, def, &frame, ground, lift, unit, rgba)
            };
            let visible = !mesh.indices.is_empty();
            if std::env::var_os("OPENSPORE_FX_DEBUG").is_some() {
                let lo = mesh.positions.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(Vec3::from_array(*p)));
                let hi = mesh.positions.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(Vec3::from_array(*p)));
                info!(
                    "decal {} (type {} layer {} texture {:08x} {:?}) of fx {:08x}: origin {:?} size {:?} alpha {:.2} colour {:?} lift {:.4} ground {} vertices {} local box {lo:?}..{hi:?} visible {visible}",
                    state.uid,
                    def.kind,
                    def.texture.layer,
                    def.texture.resource.instance,
                    look.blend,
                    effect.effect,
                    frame.origin,
                    now.size,
                    now.alpha,
                    color,
                    lift,
                    if planet.is_some() { "planet" } else if surface.is_some() { "stage" } else { "none" },
                    mesh.positions.len()
                );
            }
            let paint = def.kind == decal_type::PAINT;
            let layer = effect.layer;
            let existing = cache
                .draws
                .get(&state.uid)
                .map(|d| (d.entity, d.layer))
                .and_then(|(e, l)| draws.get_mut(e).ok().map(|d| (e, l, d)));
            match existing {
                Some((entity, drawn_layer, (mut transform, handle, mut material, mut visibility))) => {
                    if drawn_layer != layer {
                        commands.entity(entity).try_insert(RenderLayers::layer(layer));
                        if let Some(d) = cache.draws.get_mut(&state.uid) {
                            d.layer = layer;
                        }
                    }
                    transform.translation = frame.origin;
                    if material.0 != look.material {
                        material.0 = look.material.clone();
                    }
                    *visibility = if visible { Visibility::Visible } else { Visibility::Hidden };
                    if visible {
                        if let Some(mut m) = meshes.get_mut(&handle.0) {
                            mesh.write(&mut m);
                        }
                    }
                }
                None => {
                    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                    if visible { mesh } else { DecalMesh::placeholder() }.write(&mut m);
                    let entity = commands
                        .spawn((
                            SwarmDecalDraw,
                            // A Swarm draw: `terrain::ground` leaves it alone
                            // (the vertices are already in world space).
                            SwarmDraw,
                            crate::stages::StageOwned,
                            Mesh3d(meshes.add(m)),
                            MeshMaterial3d(look.material.clone()),
                            Transform::from_translation(frame.origin),
                            GlobalTransform::from_translation(frame.origin),
                            if visible { Visibility::Visible } else { Visibility::Hidden },
                            NoFrustumCulling,
                            NotShadowCaster,
                            NotShadowReceiver,
                            RenderLayers::layer(layer),
                        ))
                        .id();
                    cache.draws.insert(state.uid, DecalDraw { entity, layer, paint });
                }
            }
        });
    }
    // Decals whose state is gone: paint marks stay (oldest dropped past the
    // cap), the rest despawn.
    let gone: Vec<u64> = cache.draws.keys().filter(|uid| !seen.contains(uid)).copied().collect();
    for uid in gone {
        let Some(d) = cache.draws.remove(&uid) else { continue };
        if d.paint {
            cache.marks.push_back(d.entity);
        } else if let Ok(mut e) = commands.get_entity(d.entity) {
            e.despawn();
        }
    }
    while cache.marks.len() > MAX_MARKS {
        if let Some(old) = cache.marks.pop_front() {
            if let Ok(mut e) = commands.get_entity(old) {
                e.despawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::super::format::{kind, IMPORT_MASK};
    use super::super::sim::Mods;
    use super::*;

    fn state(def_rolls: [f32; 3], age: f32) -> DecalState {
        DecalState {
            dir: 0,
            index: 0,
            uid: 1,
            age,
            updates: 5,
            mods: Mods::default(),
            frame: None,
            stopped: false,
            rolls: def_rolls,
        }
    }

    fn pad() -> DecalDef {
        DecalDef {
            size: vec![6.0],
            alpha: vec![0.7],
            life: 0.5,
            life_type: 2,
            ..DecalDef::default()
        }
    }

    #[test]
    fn evaluate_reads_the_curves_and_modifiers() {
        let mut def = pad();
        def.rotation = vec![0.25];
        def.aspect = vec![0.5];
        def.size_vary = 0.5;
        let now = evaluate(&def, &state([0.0, 1.0, 0.0], 0.1), 2.0);
        // 6 x (1 + 0.5) x scale 2 = 18 along U, half of it along V.
        assert!((now.size.x - 18.0).abs() < 1e-5 && (now.size.y - 9.0).abs() < 1e-5);
        assert!((now.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((now.alpha - 0.7).abs() < 1e-6);
    }

    #[test]
    fn type_nine_is_square_and_opaque() {
        let mut def = pad();
        def.kind = 9;
        def.aspect = vec![0.6];
        def.alpha = vec![0.2];
        let now = evaluate(&def, &state([0.0; 3], 0.0), 1.0);
        assert_eq!(now.size.x, now.size.y);
        assert_eq!(now.alpha, 1.0);
    }

    /// Life rules: loop wraps, single / sustain clamp.
    #[test]
    fn life_types_pick_the_curve_position() {
        let mut def = pad();
        def.size = vec![0.0, 10.0];
        def.life = 2.0;
        def.life_type = 0;
        let looped = evaluate(&def, &state([0.0; 3], 3.0), 1.0);
        assert!((looped.size.x - 5.0).abs() < 1e-4);
        def.life_type = 2;
        let held = evaluate(&def, &state([0.0; 3], 3.0), 1.0);
        assert!((held.size.x - 10.0).abs() < 1e-4);
    }

    /// A flat decal is a square of the requested edge around the origin,
    /// UV (0.5, 0.5) at the centre and texture up toward Spore +Y (Bevy -Z).
    #[test]
    fn flat_footprint_is_the_decal_square() {
        let def = pad();
        let now = evaluate(&def, &state([0.0; 3], 0.0), 1.0);
        let frame = Frame::new(Vec3::new(5.0, 1.0, 5.0), Quat::IDENTITY, 1.0);
        let mesh = decal_mesh(&now, &def, &frame, Ground::Flat, 0.0, 1.0, [1.0; 4]);
        let lo = mesh.positions.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(Vec3::from_array(*p)));
        let hi = mesh.positions.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(Vec3::from_array(*p)));
        assert!((hi - lo - Vec3::new(6.0, 0.0, 6.0)).length() < 1e-4);
        // The far (-Z) edge carries the top of the image.
        let top = mesh.positions.iter().zip(&mesh.uv).filter(|(p, _)| p[2] < -2.99).all(|(_, uv)| uv[1] < 1e-5);
        assert!(top);
        let centre = mesh.positions.iter().zip(&mesh.uv).any(|(p, uv)| {
            p[0].abs() < 1e-5 && p[2].abs() < 1e-5 && (uv[0] - 0.5).abs() < 1e-5 && (uv[1] - 0.5).abs() < 1e-5
        });
        assert!(centre);
    }

    /// Ground sampling drapes the grid; the lift floats it above.
    #[test]
    fn footprint_follows_the_ground() {
        let def = pad();
        let now = evaluate(&def, &state([0.0; 3], 0.0), 1.0);
        let frame = Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0);
        let slope = |x: f32, _z: f32| x * 0.5;
        let mesh = decal_mesh(&now, &def, &frame, Ground::Height(&slope), 0.05, 1.0, [1.0; 4]);
        for p in &mesh.positions {
            assert!((p[1] - (p[0] * 0.5 + 0.05)).abs() < 1e-4);
        }
    }

    /// Over a planet the footprint is draped radially (lift along the
    /// outward normal), the centre keeps UV (0.5, 0.5) and the image spans
    /// the decal edge in the tangent plane, even when the effect frame is
    /// not tangent (Space planet view).
    #[test]
    fn footprint_follows_a_planet() {
        let mut def = pad();
        def.size = vec![2.0];
        let now = evaluate(&def, &state([0.0; 3], 0.0), 1.0);
        let planet_centre = Vec3::new(0.0, -500.0, 0.0);
        // A sphere with a 3 unit hill toward +X.
        let surface = |p: Vec3| -> Option<(Vec3, Vec3)> {
            let d = (p - planet_centre).try_normalize()?;
            Some((planet_centre + d * (500.0 + 3.0 * d.x.max(0.0)), d))
        };
        // A point 11 degrees round the sphere; the effect frame is upright
        // (+Y) and floats 5 above the surface.
        let on = planet_centre + Quat::from_rotation_z(-0.2) * Vec3::Y * 502.0;
        let frame = Frame::new(on + Vec3::Y * 5.0, Quat::IDENTITY, 1.0);
        let mesh = decal_mesh(&now, &def, &frame, Ground::Planet(&surface), 0.1, 0.25, [1.0; 4]);
        assert!(!mesh.indices.is_empty());
        for p in &mesh.positions {
            let w = Vec3::from_array(*p) + frame.origin;
            let (ground, outward) = surface(w).unwrap();
            assert!(((w - ground).dot(outward) - 0.1).abs() < 1e-3);
            assert!((w - ground).cross(outward).length() < 1e-3);
        }
        let (u0, u1) = mesh.uv.iter().fold((f32::MAX, f32::MIN), |(a, b), uv| (a.min(uv[0]), b.max(uv[0])));
        assert!(u0.abs() < 0.05 && (u1 - 1.0).abs() < 0.05, "u {u0}..{u1}");
        let centre = mesh.uv.iter().filter(|uv| (uv[0] - 0.5).abs() < 1e-4 && (uv[1] - 0.5).abs() < 1e-4).count();
        assert!(centre >= 1);
    }

    /// Rotation turns the texture basis about the normal.
    #[test]
    fn rotation_turns_the_texture() {
        let mut def = pad();
        def.rotation = vec![0.25];
        let now = evaluate(&def, &state([0.0; 3], 0.0), 1.0);
        let frame = Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0);
        let mesh = decal_mesh(&now, &def, &frame, Ground::Flat, 0.0, 1.0, [1.0; 4]);
        // A quarter turn: the image's right edge now faces Spore +Y (Bevy -Z).
        let right = mesh.positions.iter().zip(&mesh.uv).filter(|(_, uv)| uv[0] > 0.999).all(|(p, _)| p[2] < -2.99);
        assert!(right);
    }

    #[test]
    fn nothing_for_invisible_or_empty_decals() {
        let def = pad();
        let now = evaluate(&def, &state([0.0; 3], 0.0), 0.0);
        let frame = Frame::new(Vec3::ZERO, Quat::IDENTITY, 0.0);
        assert!(decal_mesh(&now, &def, &frame, Ground::Flat, 0.0, 1.0, [1.0; 4]).indices.is_empty());
        let now = evaluate(&def, &state([0.0; 3], 0.0), 1.0);
        let frame = Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0);
        assert!(decal_mesh(&now, &def, &frame, Ground::Flat, 0.0, 1.0, [1.0, 1.0, 1.0, 0.0]).indices.is_empty());
    }

    #[test]
    fn paint_decals_do_not_keep_effects_alive_and_single_ones_end() {
        let mut def = pad();
        // Sustain: runs until stopped.
        assert!(state([0.0; 3], 99.0).running(&def));
        let mut stopped = state([0.0; 3], 0.0);
        stopped.stopped = true;
        assert!(!stopped.running(&def));
        // Single: ends at its life.
        def.life_type = 1;
        assert!(state([0.0; 3], 0.1).running(&def));
        assert!(!state([0.0; 3], 0.5).running(&def));
        // Paint: a stamp, shown for the first two updates only.
        def.kind = decal_type::PAINT;
        def.life_type = 0;
        let mut fresh = state([0.0; 3], 0.0);
        fresh.updates = 1;
        assert!(fresh.running(&def));
        assert!(!state([0.0; 3], 0.0).running(&def));
    }

    /// `cargo test survey_decals -- --ignored --nocapture`: every installed
    /// decal component, which exported effects carry it and the spread of its
    /// parameters.
    #[test]
    #[ignore = "requires the installed game"]
    fn survey_decals() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let lib = SwarmLibrary::load(&index);
        let reg = std::fs::read_to_string(
            "D:/SteamLibrary/steamapps/common/Spore/OpenSpore/target/format-reference/reg_file.txt",
        )
        .unwrap_or_default();
        let names: HashMap<u32, String> = reg
            .lines()
            .filter_map(|l| l.split('\t').next())
            .map(|n| (crate::world_objects::name_id(n.trim()), n.trim().to_string()))
            .collect();
        let mut hist: BTreeMap<String, u32> = BTreeMap::new();
        for (d, dir) in lib.dirs.iter().enumerate() {
            println!(
                "dir {d} {:08x}!{:08x}: {} decals (section count {:?})",
                dir.key_group,
                dir.key_instance,
                dir.decals.len(),
                dir.section_counts.get(&3)
            );
            assert_eq!(dir.section_counts.get(&3).copied().unwrap_or(0) as usize, dir.decals.len());
            let mut owners: HashMap<usize, Vec<String>> = HashMap::new();
            let by_index: HashMap<i32, u32> = dir.exports.iter().map(|(n, i)| (*i, *n)).collect();
            for (e, effect) in dir.effects.iter().enumerate() {
                for b in &effect.blocks {
                    if b.kind == kind::DECAL && b.index >= 0 && b.index & IMPORT_MASK == 0 {
                        let name = by_index
                            .get(&(e as i32))
                            .map(|n| names.get(n).cloned().unwrap_or(format!("{n:08x}")))
                            .unwrap_or(format!("#{e}"));
                        owners.entry(b.index as usize).or_default().push(name);
                    }
                }
            }
            for (i, c) in dir.decals.iter().enumerate() {
                let t = &c.texture;
                let key = format!(
                    "type {} life_type {} flags {:#x} draw_mode {:#x} draw_flags {:#x} layer {}",
                    c.kind, c.life_type, c.flags, t.draw_mode, t.draw_flags, t.layer
                );
                *hist.entry(key).or_default() += 1;
                println!(
                    "  decal {i}: type {} lifeType {} life {} tex {:08x}!{:08x} mode {:#x} layer {} size {:?} alpha {:?} | {:?}",
                    c.kind, c.life_type, c.life, t.resource.group, t.resource.instance, t.draw_mode, t.layer,
                    c.size, c.alpha,
                    owners.get(&i).map(|v| v.iter().take(3).collect::<Vec<_>>())
                );
            }
        }
        println!("--- histogram");
        for (k, n) in hist {
            println!("{n:>5}  {k}");
        }
    }
}
