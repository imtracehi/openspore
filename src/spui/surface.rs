//! Gamma-space presentation of SPUI layouts.
//!
//! The retail renderer blends UI layers on gamma-encoded values (no sRGB
//! conversion on read or write). Bevy blends linearly, which makes translucent
//! overlays such as the white gloss over the progress bar look far too strong.
//! This module reproduces the retail blending:
//!
//! * a dedicated UI camera renders every SPUI layout into an `Rgba8Unorm`
//!   image, so the render pass stores and blends the raw values;
//! * colours and textures of the layouts are handed over unconverted (see
//!   [`gamma_surface`] and `view::argb_color`);
//! * a full-screen UI material shows that image over the scene, decoding the
//!   (premultiplied) result to linear light at the very end.
//!
//! The surface follows the size of the game camera's render target in physical
//! pixels, so layouts are laid out at the native resolution like the original
//! (no DPI scaling). A new image is allocated whenever the size changes (a
//! material keeps its bind group when only the texture behind a handle grows).

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::{
    asset::embedded_asset,
    camera::{ClearColorConfig, ImageRenderTarget, RenderTarget},
    picking::Pickable,
    prelude::*,
    math::Vec4,
    render::render_resource::{AsBindGroup, BlendState, RenderPipelineDescriptor, ShaderType, TextureFormat},
    shader::ShaderRef,
    ui_render::{
        ui_material::{UiMaterial, UiMaterialKey},
        UiMaterialPlugin,
    },
};

use super::view::SpuiState;

static GAMMA_SURFACE: AtomicBool = AtomicBool::new(false);

/// True once the surface plugin is installed: SPUI colours and textures are
/// then raw gamma values for the surface instead of sRGB-converted ones.
pub fn gamma_surface() -> bool {
    GAMMA_SURFACE.load(Ordering::Relaxed)
}

/// `GlobalZIndex` of the composite over the rest of the game UI. The legacy
/// editor and social panels sit below it, the legacy pause menu above.
pub const COMPOSITE_Z: i32 = 100;

/// Camera that renders the SPUI layouts into the surface image.
#[derive(Component)]
pub struct SpuiHudCamera;

/// Full-screen node that shows the surface over the game camera's view.
#[derive(Component)]
pub struct SpuiComposite;

#[derive(Resource, Default)]
pub struct SpuiSurface {
    /// The camera layouts draw through (once the game camera's size is known).
    pub camera: Option<Entity>,
    pub image: Handle<Image>,
    pub size: UVec2,
    material: Handle<SpuiCompositeMaterial>,
    composite: Option<Entity>,
}

/// Material that decodes the premultiplied gamma surface to linear light.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SpuiCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
}

impl UiMaterial for SpuiCompositeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/spui/shaders/composite.wesl".into()
    }

    fn specialize(descriptor: &mut RenderPipelineDescriptor, _key: UiMaterialKey<Self>) {
        // The shader returns colour premultiplied by coverage.
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if let Some(Some(target)) = fragment.targets.get_mut(0) {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
    }
}

/// Uniforms of [`SpuiCopiesMaterial`] (`shaders/copies.wesl`).
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct CopiesParams {
    /// Source region of the image in the texture (uv): min x, min y, max x, max y.
    pub src: Vec4,
    /// x: margin around the layer in the node (pixels), y: number of copies.
    pub info: Vec4,
    /// Tint colour of the copies (rgb, raw 0..1).
    pub color: Vec4,
    /// `(dx, dy, alpha 0..1, unused)` of each copy.
    pub taps: [Vec4; super::paint::MAX_COPIES],
}

impl Default for CopiesParams {
    fn default() -> Self {
        Self {
            src: Vec4::new(0.0, 0.0, 1.0, 1.0),
            info: Vec4::ZERO,
            color: Vec4::ZERO,
            taps: [Vec4::ZERO; super::paint::MAX_COPIES],
        }
    }
}

/// All copies of one image layer in a stroke / halo / outline kernel, drawn by one UI node (see
/// `paint::Cmd::Copies`): the shader sums the shifted, tinted samples exactly like the retail
/// sequence of single draws.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct SpuiCopiesMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
    #[uniform(2)]
    pub params: CopiesParams,
}

impl UiMaterial for SpuiCopiesMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/spui/shaders/copies.wesl".into()
    }
}

pub struct SpuiSurfacePlugin;

impl Plugin for SpuiSurfacePlugin {
    fn build(&self, app: &mut App) {
        GAMMA_SURFACE.store(true, Ordering::Relaxed);
        embedded_asset!(app, "shaders/composite.wesl");
        embedded_asset!(app, "shaders/copies.wesl");
        // Development aid shared with the other shaders: `OS_SHADER_DIR` holds
        // an edited `spui_composite.wesl`.
        if let Ok(dir) = std::env::var("OS_SHADER_DIR") {
            if let Ok(bytes) = std::fs::read(std::path::Path::new(&dir).join("spui_composite.wesl")) {
                app.world_mut()
                    .resource_mut::<bevy::asset::io::embedded::EmbeddedAssetRegistry>()
                    .insert_asset(
                        std::path::PathBuf::new(),
                        std::path::Path::new("openspore/spui/shaders/composite.wesl"),
                        bytes,
                    );
            }
        }
        app.add_plugins(UiMaterialPlugin::<SpuiCompositeMaterial>::default())
            .add_plugins(UiMaterialPlugin::<SpuiCopiesMaterial>::default())
            .init_resource::<SpuiSurface>()
            .add_systems(Update, (update_surface, retarget).chain());
    }
}

fn target_image(size: UVec2) -> Image {
    Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8Unorm, None)
}

fn image_target(handle: &Handle<Image>) -> RenderTarget {
    RenderTarget::Image(ImageRenderTarget {
        handle: handle.clone(),
        scale_factor: 1.0,
    })
}

/// Create the surface once the game camera's target size is known, keep it the
/// size of that target, run the HUD camera only while a layout exists and show
/// the composite node only then.
#[allow(clippy::too_many_arguments)]
fn update_surface(
    mut commands: Commands,
    mut surface: ResMut<SpuiSurface>,
    images: Option<ResMut<Assets<Image>>>,
    materials: Option<ResMut<Assets<SpuiCompositeMaterial>>>,
    game: Query<(Entity, &Camera), (With<crate::GameCamera>, Without<SpuiHudCamera>)>,
    layouts: Query<(), With<SpuiState>>,
    mut hud: Query<&mut Camera, With<SpuiHudCamera>>,
    mut composites: Query<&mut Visibility, With<SpuiComposite>>,
) {
    let (Some(mut images), Some(mut materials)) = (images, materials) else {
        return;
    };
    let Some((game_entity, game_camera)) = game.iter().next() else {
        return;
    };
    let Some(size) = game_camera.physical_target_size().filter(|s| s.x > 0 && s.y > 0) else {
        return;
    };
    let any = !layouts.is_empty();
    match surface.camera {
        None => {
            let image = images.add(target_image(size));
            let camera = commands
                .spawn((
                    Name::new("spui-hud-camera"),
                    Camera2d,
                    Camera {
                        // Renders before the game camera, whose UI pass samples the image.
                        order: -50,
                        is_active: false,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        ..default()
                    },
                    image_target(&image),
                    Msaa::Off,
                    SpuiHudCamera,
                ))
                .id();
            let material = materials.add(SpuiCompositeMaterial { image: image.clone() });
            let node = commands
                .spawn((
                    Name::new("spui-composite"),
                    SpuiComposite,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    MaterialNode(material.clone()),
                    GlobalZIndex(COMPOSITE_Z),
                    Pickable::IGNORE,
                    Visibility::Hidden,
                    UiTargetCamera(game_entity),
                ))
                .id();
            surface.camera = Some(camera);
            surface.image = image;
            surface.size = size;
            surface.material = material;
            surface.composite = Some(node);
        }
        Some(camera) if size != surface.size => {
            let image = images.add(target_image(size));
            commands.entity(camera).insert(image_target(&image));
            if let Some(mut material) = materials.get_mut(&surface.material) {
                material.image = image.clone();
            }
            surface.image = image;
            surface.size = size;
        }
        Some(_) => {}
    }
    if let Some(camera) = surface.camera {
        if let Ok(mut cam) = hud.get_mut(camera) {
            if cam.is_active != any {
                cam.is_active = any;
            }
        }
    }
    for mut visibility in &mut composites {
        let want = if any { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want {
            *visibility = want;
        }
    }
}

/// Every layout draws into the surface, whichever camera it was spawned for.
fn retarget(
    mut commands: Commands,
    surface: Res<SpuiSurface>,
    layouts: Query<(Entity, Option<&UiTargetCamera>), With<SpuiState>>,
) {
    let Some(camera) = surface.camera else {
        return;
    };
    for (entity, target) in &layouts {
        if target.map(|t| t.0) != Some(camera) {
            commands.entity(entity).insert(UiTargetCamera(camera));
        }
    }
}
