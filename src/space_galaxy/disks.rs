//! The galaxy map's disk and black-hole sprites (helper `swarm-galactic`, `notes/swarm-galactic-notes.md`).
//!
//! `sg_galaxy` (effect directory 0xC1195706) has three more distributes next to the star glows
//! (`stars.rs`), one particle per star on the same emit maps as the message distributes (start index
//! 300000 / 400000 = the protoplanetary disks / black holes of `SG_galaxy_messages~`), each drawn with a
//! galactic material (VERIFIED data, dumped with `scripts/all_distributes.py`):
//!
//! | distribute | material | program | raster | size (vary) | alpha |
//! | --- | --- | --- | --- | --- | --- |
//! | #54 (1,500 x 6) | 0xA4641C96 `galacticSpinTiltAdditive` | 0x70000120 | `protoplanetary_top_3x3` | 0.15 (0.5) | 0.8 |
//! | #56 (1,500 x 6) | 0x36A1D684 `galacticZpoleTiltAdditive` | 0x70000122 | `protoplanetary_side_3x3` | 0.1 (0.5) | 0.8 |
//! | #62 (400 x 6) | 0xB7744FB6 `galacticSpinTiltAdditive` | 0x70000120 | `blackhole_vortex_top_3x3` | 0.1 (0.2) | 1 |
//!
//! so every protoplanetary disk star carries two tilted plane sprites (face-on and edge-on views of the disk)
//! and every black hole one spinning vortex point. The programs (ported in `swarm_fx/galactic.rs`) tilt each
//! plane at a random orientation, spin it at a rate from c15, fade it by the angle between the view direction
//! and its normal (c7's three-entry table) and by the clip depth (c1 / c2: opaque inside 40 units, gone at
//! 100): the sprites only show close to the camera, which is why the zoomed-out map never showed them.
//!
//! The Swarm effect itself is not played (the distribute components are not simulated): this layer reads the
//! effect directory's material registers from the library and rebuilds the sprites near the camera every
//! frame with the ported vertex stage, drawn by the additive galactic material (`swarm_galactic.wesl`).
//!
//! ADAPTED: which star carries which random (a hash of the star index replaces the distribute's generator),
//! the distribute's `sizeVary` as `size * (1 + vary * (2 h - 1))`, the white colour (no colour list in the
//! distributes), and one sprite per star at the star's map position (the 3D emit offset of the cluster is not
//! modelled, like the glows).

use std::sync::Arc;

use bevy::{camera::visibility::NoFrustumCulling, light::NotShadowCaster, prelude::*};

use super::StarKind;
use crate::swarm_fx::galactic::{self, Output, Program, Registers};
use crate::swarm_fx::galactic_draw::{self, GalacticMaterial};
use crate::swarm_fx::{format::ResourceId, SwarmLibrary};
use crate::space_worldgen::GalaxyStar;

/// The sprite families of `sg_galaxy`: (material id of the effect directory, the star kind that carries it,
/// base size, size vary, alpha, name of the raster).
#[derive(Clone, Copy, Debug)]
pub struct Family {
    pub label: &'static str,
    pub material: u32,
    pub kind: StarKind,
    pub size: f32,
    pub size_vary: f32,
    pub alpha: f32,
    pub raster: &'static str,
    pub program: Program,
}

pub const FAMILIES: [Family; 3] = [
    Family {
        label: "protoplanetary disk, face-on (#54)",
        material: 0xA464_1C96,
        kind: StarKind::ProtoPlanetary,
        size: 0.15,
        size_vary: 0.5,
        alpha: 0.8,
        raster: "protoplanetary_top_3x3",
        program: Program::SpinTilt,
    },
    Family {
        label: "protoplanetary disk, edge-on (#56)",
        material: 0x36A1_D684,
        kind: StarKind::ProtoPlanetary,
        size: 0.1,
        size_vary: 0.5,
        alpha: 0.8,
        raster: "protoplanetary_side_3x3",
        program: Program::ZpoleTilt,
    },
    Family {
        label: "black hole vortex point (#62)",
        material: 0xB774_4FB6,
        kind: StarKind::BlackHole,
        size: 0.1,
        size_vary: 0.2,
        alpha: 1.0,
        raster: "blackhole_vortex_top_3x3",
        program: Program::SpinTilt,
    },
];

/// The effect directory of `sg_galaxy`.
const SG_GALAXY_DIR: u32 = 0xC119_5706;

/// One star that carries sprites.
#[derive(Clone, Copy, Debug)]
pub struct DiskStar {
    pub index: u32,
    pub kind: StarKind,
    /// Position in the map layer's space (x, `STAR_PLANE`, y).
    pub position: Vec3,
}

/// The sprites' per-star inputs: (half size, colour and alpha, the two vertex randoms). ADAPTED hashes.
pub fn sprite_inputs(family: &Family, index: u32) -> (f32, Vec4, Vec2) {
    let size = family.size * (1.0 + family.size_vary * (super::stars::hash01(index, 21 + family.material) * 2.0 - 1.0));
    let rand = Vec2::new(super::stars::hash01(index, 31 + family.material), super::stars::hash01(index, 41 + family.material));
    (size * 0.5, Vec4::new(1.0, 1.0, 1.0, family.alpha), rand)
}

/// The sprites of one family within the program's fade range of the camera.
pub fn family_sprites(
    family: &Family,
    regs: &Registers,
    cam: &galactic::Camera,
    eye: Vec3,
    forward: Vec3,
    stars: &[DiskStar],
) -> Vec<(Vec3, [Output; 4])> {
    // c1.w: the depth beyond which the distance fade is 0.
    let reach = regs.0[7].max(1.0);
    let mut out = Vec::new();
    for star in stars.iter().filter(|s| s.kind == family.kind) {
        let depth = (star.position - eye).dot(forward);
        if depth > reach + family.size || depth < -family.size {
            continue;
        }
        let (half, color, rand) = sprite_inputs(family, star.index);
        out.push((star.position, galactic::sprite(family.program, regs, cam, star.position, half, color, rand)));
    }
    out
}

/// The layer: the stars that carry sprites and, once the library is loaded, one dynamic mesh per family.
#[derive(Component)]
pub struct DiskLayer {
    stars: Arc<Vec<DiskStar>>,
    groups: Vec<DiskGroup>,
    failed: bool,
}

struct DiskGroup {
    family: Family,
    regs: Registers,
    mesh: Handle<Mesh>,
    drawn: bool,
}

/// Spawn the (empty) layer under the galaxy map root; the meshes follow when the effect library is loaded.
pub fn spawn_layer(world: &mut World, root: Entity, stars: &[GalaxyStar]) {
    let carriers: Vec<DiskStar> = stars
        .iter()
        .enumerate()
        .filter(|(_, s)| FAMILIES.iter().any(|f| f.kind == s.kind))
        .map(|(i, s)| DiskStar { index: i as u32, kind: s.kind, position: Vec3::new(s.position.x, super::STAR_PLANE, s.position.y) })
        .collect();
    if carriers.is_empty() {
        return;
    }
    let layer = world
        .spawn((
            DiskLayer { stars: Arc::new(carriers), groups: Vec::new(), failed: false },
            Name::new("galaxy disk and black hole sprites"),
            Transform::IDENTITY,
            Visibility::Inherited,
        ))
        .id();
    world.entity_mut(root).add_child(layer);
}

/// Find the family's material in the library (the `sg_galaxy` directory) and make its mesh entity.
fn make_group(
    family: Family,
    lib: &SwarmLibrary,
    originals: &crate::Originals,
    layer: Entity,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GalacticMaterial>,
) -> Option<DiskGroup> {
    let dir = lib.dirs.iter().find(|d| d.key_instance == SG_GALAXY_DIR)?;
    let material = dir.materials.get(&family.material)?;
    let regs = Registers::from_material(material);
    let id = ResourceId { group: 0, instance: crate::world_objects::name_id(family.raster) };
    let image = galactic_draw::load_raster(originals, id)?;
    let texture = images.add(image);
    let mesh = meshes.add(galactic_draw::new_mesh());
    let entity = commands
        .spawn((
            NotShadowCaster,
            NoFrustumCulling,
            Name::new(family.label),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(GalacticMaterial { texture })),
            Transform::IDENTITY,
            Visibility::Inherited,
        ))
        .id();
    commands.entity(layer).add_child(entity);
    Some(DiskGroup { family, regs, mesh, drawn: false })
}

/// Rebuild the sprites near the camera (runs when the galaxy map is visible).
#[allow(clippy::too_many_arguments)]
pub fn update_disks(
    mut commands: Commands,
    lib: Option<Res<SwarmLibrary>>,
    originals: Option<Res<crate::Originals>>,
    time: Res<Time>,
    camera: Query<(&GlobalTransform, &Projection, &bevy::camera::Camera), With<crate::GameCamera>>,
    mut layers: Query<(Entity, &mut DiskLayer, &GlobalTransform, &InheritedVisibility)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Option<ResMut<Assets<GalacticMaterial>>>,
    mut reported: Local<usize>,
    mut frame: Local<u32>,
) {
    // A/B switch for captures: OPENSPORE_GALAXY_DISKS=0 leaves the sprites out.
    if std::env::var("OPENSPORE_GALAXY_DISKS").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    let (Some(lib), Some(originals), Some(mut materials)) = (lib, originals, materials) else { return };
    let Some((transform, projection, cam)) = camera.iter().next() else { return };
    for (entity, mut layer, layer_transform, visible) in &mut layers {
        if !visible.get() || layer.failed || lib.dirs.is_empty() {
            continue;
        }
        if layer.groups.is_empty() {
            let made: Vec<DiskGroup> = FAMILIES
                .iter()
                .filter_map(|f| make_group(*f, &lib, &originals, entity, &mut commands, &mut images, &mut meshes, &mut materials))
                .collect();
            if made.len() != FAMILIES.len() {
                warn!("galaxy disk sprites: {} of {} families found in the effect library", made.len(), FAMILIES.len());
                layer.failed = true;
            }
            layer.groups = made;
            continue;
        }
        // The layer sits at the map root (a pure translation): sprites are built in its space.
        let origin = layer_transform.translation();
        let eye = transform.translation() - origin;
        let viewport = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 900.0));
        let galactic_camera = galactic_draw::camera_for(
            &GlobalTransform::from(Transform {
                translation: eye,
                rotation: transform.compute_transform().rotation,
                scale: Vec3::ONE,
            }),
            projection,
            viewport,
            time.elapsed_secs(),
        );
        let forward = transform.forward().as_vec3();
        let stars = layer.stars.clone();
        let mut shown = 0;
        for group in &mut layer.groups {
            let sprites = family_sprites(&group.family, &group.regs, &galactic_camera, eye, forward, &stars);
            shown += sprites.len();
            // Development aid: OPENSPORE_GALAXY_DISKS_DEBUG=1 logs the three nearest sprites of a family twice a second.
            if *frame % 30 == 0 && std::env::var_os("OPENSPORE_GALAXY_DISKS_DEBUG").is_some() {
                let mut near: Vec<&(Vec3, [Output; 4])> = sprites.iter().collect();
                near.sort_by(|a, b| (a.0 - eye).length_squared().total_cmp(&(b.0 - eye).length_squared()));
                for (center, corners) in near.into_iter().take(3) {
                    let ndc = corners[0].position.truncate().truncate() / corners[0].position.w;
                    let alpha = corners.iter().map(|c| c.color.w).fold(0.0f32, |m, a| if a.abs() > m.abs() { a } else { m });
                    info!(
                        "disk debug {}: eye {eye:?} forward {forward:?} star {center:?} distance {:.2} ndc {ndc:?} alpha {alpha:.3} blend {:.2}",
                        group.family.label,
                        (*center - eye).length(),
                        corners[0].blend
                    );
                }
            }
            if sprites.is_empty() && !group.drawn {
                continue;
            }
            group.drawn = !sprites.is_empty();
            if let Some(mut mesh) = meshes.get_mut(&group.mesh) {
                galactic_draw::write_sprites(&mut mesh, &sprites);
            }
        }
        *frame += 1;
        if (shown == 0) != (*reported == 0) {
            info!("galaxy disk sprites: {shown} within the fade range of the camera (was {})", *reported);
        }
        *reported = shown;
    }
}

/// Registration (render apps).
pub fn build(app: &mut App) {
    app.add_systems(
        PostUpdate,
        update_disks
            .run_if(in_state(crate::stages::GameStage::Space))
            .after(bevy::transform::TransformSystems::Propagate),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk_regs() -> Registers {
        let mut r = Registers::default();
        for (i, v) in [(4, 0.0), (5, 25.0), (6, 40.0), (7, 100.0), (8, 1.0), (9, 1.0), (10, 1.0), (11, 0.0)] {
            r.0[i] = v;
        }
        r.0[29] = 3.0;
        r.0[30] = -0.1;
        r.0[31] = 3.0;
        r.0[32] = 3.0;
        r.0[33] = 3.0;
        r.0[34] = 0.7;
        r.0[60] = -0.1;
        r.0[61] = 0.1;
        r.0[62] = 1.0;
        r
    }

    fn camera(eye: Vec3, forward: Vec3) -> galactic::Camera {
        let right = forward.cross(Vec3::Y).normalize();
        let up = right.cross(forward);
        galactic::Camera::from_frame(eye, right, up, forward, 1.0, 1.4, 0.05, 10_000.0, Vec2::new(1280.0, 900.0), 0.0)
    }

    #[test]
    fn families_match_the_dumped_distributes() {
        assert_eq!(FAMILIES.len(), 3);
        // The families use the programs the compiled states name.
        assert_eq!(FAMILIES[0].program, Program::SpinTilt);
        assert_eq!(FAMILIES[1].program, Program::ZpoleTilt);
        assert_eq!(FAMILIES[2].program, Program::SpinTilt);
        assert!(FAMILIES.iter().all(|f| f.size > 0.0 && f.alpha > 0.0));
        for f in &FAMILIES {
            let (half, color, rand) = sprite_inputs(f, 12345);
            assert!(half >= f.size * 0.5 * (1.0 - f.size_vary) - 1e-6 && half <= f.size * 0.5 * (1.0 + f.size_vary) + 1e-6);
            assert_eq!(color.w, f.alpha);
            assert!((0.0..=1.0).contains(&rand.x) && (0.0..=1.0).contains(&rand.y));
            assert_eq!(sprite_inputs(f, 12345), (half, color, rand), "deterministic");
        }
    }

    #[test]
    fn only_stars_inside_the_fade_range_get_sprites() {
        let family = FAMILIES[0];
        let eye = Vec3::new(0.0, 30.0, 0.0);
        let forward = Vec3::NEG_Y;
        let cam = camera(eye, forward);
        let stars = [
            DiskStar { index: 1, kind: StarKind::ProtoPlanetary, position: Vec3::new(2.0, 1.0, 1.0) }, // 29 units ahead
            DiskStar { index: 2, kind: StarKind::ProtoPlanetary, position: Vec3::new(2.0, -90.0, 1.0) }, // 120 ahead: beyond c1.w
            DiskStar { index: 3, kind: StarKind::BlackHole, position: Vec3::new(2.0, 1.0, 1.0) }, // wrong kind
            DiskStar { index: 4, kind: StarKind::ProtoPlanetary, position: Vec3::new(2.0, 60.0, 1.0) }, // behind the camera
        ];
        let sprites = family_sprites(&family, &disk_regs(), &cam, eye, forward, &stars);
        assert_eq!(sprites.len(), 1);
        // 29 units ahead: inside the opaque part of the fade, the alpha is the tilt alpha times 0.8.
        let (_, corners) = &sprites[0];
        assert!(corners.iter().all(|c| c.color.w.abs() <= 0.8 + 1e-5));
    }
}
