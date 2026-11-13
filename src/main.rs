//! OpenSpore — open-source Bevy engine for Spore (OpenRCT2 model).
//!
//! Requires a legal copy of Spore. All `Data/*.package` assets are loaded
//! read-only at runtime from the user's install; this repo ships no EA assets.
//!
//! Prototype stage arc: Cell -> Creature -> Tribal -> Civilization -> Space.
//! Native scene/game parity is tracked separately in PARITY_LEDGER.json.
//! Menu: Enter = new run, L = continue saved run. Stage keys 1-5/0 for debug jump.

mod anim_audio;
mod audio_audit;
mod loop_audio;
mod voice_banks;
mod ui_sounds;
mod civ_voices;
mod civ_target;
mod civ_cursor;
mod civ_orders;
mod space_voices;
mod space_tool_fx;
mod space_weapon_fx;
mod effect_audio;
mod tribal_weapon_audio;
mod animation;
mod animation_catalog;
mod animation_layout;
mod animation_sampling;
mod arithmetica;
mod asset_catalog;
mod asteroid_visuals;
mod audio;
mod baked_model;
mod bitmap;
mod body;
mod cell;
mod cell_ai;
mod cell_body;
mod cell_anim;
mod cell_acoustics;
mod cell_audio;
mod cell_spatial;
mod cell_voices;
mod cell_contract;
mod cell_data;
mod cell_editor;
mod cell_editor_data;
mod cell_editor_paint;
mod cell_editor_swatch;
mod cell_editor_scene;
mod cell_editor_ui;
mod cell_sh;
mod cell_paint;
mod cell_skinpaint;
mod cell_engine;
mod cell_hud;
mod cell_environment;
mod cell_flow;
mod cell_fluid;
mod cell_fluid_tex;
mod cell_softbody;
mod cell_motion;
mod cell_part_guide;
mod cell_population;
mod cell_rules;
mod cell_fx;
mod cell_parallax;
mod cell_visuals;
mod cinematics;
mod city_layout;
mod city_presentation;
mod civ;
mod civ_hud;
mod civ_audio;
mod civ_controls;
mod civ_planet;
mod civ_rts;
mod civ_rules;
mod civ_combat;
mod civ_comm;
mod civ_music;
mod civ_anthem;
mod civ_planner;
mod civ_decor;
mod civ_fx;
mod civ_vfx;
mod civ_pump;
mod civ_goals;
mod civ_view;
mod civ_path;
mod civ_reveal;
mod stage_restart;
mod civ_wildlife;
mod control_options;
mod creation_assets;
mod creation_records;
mod creature;
mod creature_actions;
mod creature_audio;
mod creature_cinematics;
mod creature_controls;
mod creature_data;
mod creature_data_catalog;
mod creature_editor;
mod creature_editor_data;
mod creature_editor_edit;
mod creature_editor_paint;
mod creature_editor_ui;
mod outfitter;
mod outfit_crowd;
mod creature_skin;
mod creature_skin_paint;
mod creature_skin_view;
mod creature_cell_convert;
mod creature_handles;
mod creature_limbs;
mod creature_limb_solver;
mod creature_bem;
mod creature_blocks;
mod creature_clips;
mod creature_look;
mod creature_morph;
mod creature_ecology;
mod creature_events;
mod creature_flow;
mod creature_home;
mod creature_hud;
mod creature_lifecycle;
mod creature_motion;
mod creature_music;
mod creature_pack;
mod creature_nestmates;
mod creature_eggs;
mod creature_parts;
mod creature_planet;
mod creature_progression;
mod land_minimap;
mod creature_rules;
mod creature_selection;
mod creature_social;
mod creature_support;
mod creature_alpha;
mod creature_group;
mod d3d9_shader;
mod dbpf;
mod decode;
mod editor;
mod editor_art;
mod editor_environment;
mod epic_creatures;
mod first_run;
mod foliage_visibility;
mod front_end;
mod game;
mod game_audio;
mod gmdl;
mod gmdl_bindings;
mod hint_manager;
mod locomotion_preview;
mod material_audit;
mod material_binding;
mod material_state;
mod mechanical_assets;
mod mechanical_creator;
mod mechanical_environment;
mod mechanical_preview;
mod menu_visuals;
mod morph;
#[cfg(windows)]
mod native_d3d9;
mod native_packet;
mod native_paint;
mod native_texture;
mod native_ui;
mod navigation;
mod original_species;
mod parity;
mod part_thumbnails;
mod pause_menu;
mod pd;
mod pd_audio;
mod persistence;
mod pirate_visuals;
mod planet_data;
mod planet_gen;
mod planet_meta;
mod planet_query;
mod planet_render;
mod planet_flora;
mod planet_grass;
mod planet_light;
mod planet_sky;
mod planet_time;
mod play_camera;
mod player_appearance;
mod player_hud;
mod prop;
mod resource_audit;
mod retarget;
mod rigid_editor;
mod rigid_editor_edit;
mod rigid_editor_stack;
mod rigid_paint;
mod rigid_stats;
mod maxis_creation;
mod runtime;
mod rw4;
mod shader_pack;
mod shader_vm;
mod skinpaint;
mod space;
mod space_comm;
mod space_galaxy;
mod space_hud;
mod space_planets;
mod space_rules;
mod space_rares;
mod space_tool_artifacts;
mod space_terraform;
mod space_tools;
mod space_tools_table;
mod space_tool_use;
mod space_scene;
mod space_universe;
mod space_endgame;
mod space_audio;
mod space_controls;
mod space_view;
mod space_visuals;
mod space_worldgen;
mod space_worldgen_data;
mod species;
mod spice_presentation;
mod spore_install;
mod spui;
mod stages;
mod starter_parts;
mod swarm_fx;
mod terrain;
mod texture;
mod tlsa;
mod tribal;
mod tribal_audio;
mod tribal_controls;
mod tribal_hud;
mod tribal_path;
mod tribal_plans;
mod tribal_planet;
mod tribal_rts;
mod tribal_rules;
mod tribal_swim;
mod tribal_tools;
mod tribal_village;
mod tribal_vignettes;
mod ui_art;
mod user_paths;
mod water;
mod world_catalog;
mod world_objects;
mod world_presentation;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use game::{Health, Hunger, Player, RunProgress};
use stages::{GameStage, StageOwned};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Asset-install resources (OpenRCT2 model)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct InstallStatus {
    root: Option<PathBuf>,
    detail: String,
    ok: bool,
}

#[derive(Resource, Default)]
struct Catalog {
    packages: Vec<(String, usize)>,
    total_entries: usize,
    scan_errors: Vec<String>,
}

/// Original game data loaded at startup (capped scans stay fast).
#[derive(Resource, Default)]
pub struct Originals {
    pub ui_art: ui_art::UiArtCatalog,
    pub editor_art: editor_art::NativeEditorArt,
    pub native_painter: Option<skinpaint::NativePainter>,
    pub animation_catalog: animation_catalog::AnimationCatalog,
    pub action_lists: animation_catalog::ActionLists,
    pub animation_cache: animation_catalog::ClipCache<animation::Animation>,
    pub part_catalog: asset_catalog::PartCatalog,
    pub creature_abilities: creature_actions::CreatureAbilityCatalog,
    pub resource_index: asset_catalog::ResourceIndex,
    pub creation_catalog: creation_records::CreationCatalog,
    pub creation_models:
        std::collections::HashMap<dbpf::ResourceKey, creation_assets::ImportedCreation>,
    pub creation_part_meshes: std::collections::HashMap<
        creation_assets::MorphMeshKey,
        (
            Handle<Mesh>,
            Vec<(usize, Handle<Mesh>)>,
            morph::MorphApplication,
        ),
    >,
    pub planet_names: Vec<String>,
    pub string_count: usize,
    pub png_count: usize,
    pub icon_file: Option<String>,
    pub locale_strings: usize,
    pub locale_sample: Vec<String>,
    pub textures: Vec<Handle<Image>>,
    pub models: Vec<OriginalModel>,
    /// Mechanical imports are keyed by the complete authored PROP identity.
    /// Distinct definitions may share model instance IDs and use different
    /// geometry groups, materials, offsets or rotations.
    pub mechanical_models: std::collections::HashMap<dbpf::ResourceKey, OriginalModel>,
    pub world_models: std::collections::HashMap<dbpf::ResourceKey, OriginalModel>,
    pub baked_models: std::collections::HashMap<dbpf::ResourceKey, OriginalModel>,
    pub starter_parts: std::collections::HashMap<dbpf::ResourceKey, OriginalModel>,
    /// Exact anatomy/part definitions of displayed original NPCs. Errors are
    /// cached too, so unsupported sources remain explicit without retrying.
    pub original_species: std::collections::HashMap<
        dbpf::ResourceKey,
        Result<original_species::OriginalSpecies, String>,
    >,
    pub baked_body: Option<baked_model::BodyTemplate>,
    pub model_verts: usize,
    pub anim_total: usize,
    pub anim_folders: Vec<(String, usize)>,
    pub skeleton: Option<SkeletonView>,
    pub morph_demo: Option<MorphDemo>,
    pub skinned_demo: Option<SkinnedDemo>,
    /// Full original character: body mesh in bind pose + rig driven by a real
    /// animation clip from the same file.
    pub rigged_demo: Option<RiggedDemo>,
    /// Small original prop rigs (skeleton + own clip) from gmdl references.
    pub prop_rigs: Vec<PropRig>,
    /// Decoded standalone animation clips for the clip-driven preview.
    pub clips: Vec<ClipPreview>,
    pub audio_file: Option<String>,
    pub music_file: Option<String>,
}

/// An original animation clip ready to preview (channels + source label).
#[derive(Clone, Default)]
pub struct ClipPreview {
    pub clip: decode::AnimClip,
    pub timed_clip: Option<std::sync::Arc<animation::Animation>>,
    /// Short label: the source file name.
    pub label: String,
}

/// Which cataloged clip the preview currently plays (cycles with `P`).
#[derive(Resource, Default)]
pub struct ClipPick(pub usize, pub f32);

/// Original character: a real RW4 body mesh (bind pose) plus its skeleton
/// animated by a real embedded clip.
#[derive(Clone)]
pub struct RiggedDemo {
    pub instance: u32,
    pub mesh: Handle<Mesh>,
    pub bones: usize,
    pub parents: Vec<Option<usize>>,
    pub locals: Vec<rw4::Mat34>,
    pub invs: Vec<rw4::Mat34>,
    pub clip: rw4::KeyAnim,
    pub chan_for_bone: Vec<Option<usize>>,
    pub bind_t: Vec<[f32; 3]>,
    pub key0_loc: Vec<[f32; 3]>,
    /// Bone name hashes (for readable slot labels).
    pub bone_names: Vec<u32>,
    /// Uniform scale mapping the body bbox to ~2.5 world units.
    pub scale: f32,
}

#[derive(Clone, Default)]
pub struct OriginalModel {
    pub source_definition: Option<dbpf::ResourceKey>,
    pub material: Option<Handle<StandardMaterial>>,
    pub handle: Handle<Mesh>,
    /// Individual source meshes keep their authored material boundaries.
    /// The combined handle remains available for full assembly bounds.
    pub surfaces: Vec<OriginalSurface>,
    /// Uniform scale mapping the model's bbox diagonal to ~2 world units.
    pub scale: f32,
    /// Source RW4 instance ID (0 = procedural placeholder).
    pub instance: u32,
    /// Authored slot alignment: the part's own root-bone bind matrix (how the
    /// part sits relative to the creature bone it mounts on).
    pub root: Option<rw4::Mat34>,
    /// Bone count of the part's own largest rig (0 = no rig): used for
    /// family-based slot defaults (limb vs detail).
    pub rig_bones: usize,
}

#[derive(Clone)]
pub struct OriginalSurface {
    pub handle: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub source_section: usize,
    pub state_sections: Vec<usize>,
}
impl OriginalModel {
    pub fn draw_mesh(&self) -> Handle<Mesh> {
        self.surfaces
            .first()
            .map(|s| s.handle.clone())
            .unwrap_or_else(|| self.handle.clone())
    }
}

/// Debug skeleton view: bind-space joints + parent links, normalized.
#[derive(Clone, Default)]
pub struct SkeletonView {
    pub joints: Vec<Vec3>,
    pub links: Vec<(usize, usize)>,
    pub bones: usize,
}

/// Live morph demo: base positions + per-shape deltas + a mesh handle.
#[derive(Clone, Default)]
pub struct MorphDemo {
    pub mesh: Handle<Mesh>,
    pub base: Vec<[f32; 3]>,
    pub deltas: Vec<Vec<[f32; 3]>>,
    pub scale: f32,
    pub shapes: usize,
}

/// Per-entity morph animation state (weights driven procedurally).
#[derive(Component)]
pub struct MorphAnim {
    pub base: Vec<[f32; 3]>,
    pub deltas: Vec<Vec<[f32; 3]>>,
    pub speeds: Vec<f32>,
}

/// Live skinned demo: CPU linear-blend skinning over file-local bones
/// (influences targeting unknown bones hold bind pose).
#[derive(Clone, Default)]
pub struct SkinnedDemo {
    pub mesh: Handle<Mesh>,
    pub base: Vec<[f32; 3]>,
    pub idx: Vec<[u16; 2]>,
    pub wts: Vec<[f32; 2]>,
    pub locals: Vec<rw4::Mat34>,
    pub invs: Vec<rw4::Mat34>,
    pub parents: Vec<Option<usize>>,
    pub wave_bone: usize,
    pub scale: f32,
    pub bones: usize,
    /// Real animation clip + per-bone channel mapping (None = sine fallback).
    pub clip: Option<rw4::KeyAnim>,
    pub chan_for_bone: Vec<Option<usize>>,
    pub bind_t: Vec<[f32; 3]>,
    pub key0_loc: Vec<[f32; 3]>,
}

/// Per-entity procedural skin animation state.
#[derive(Component)]
pub struct SkinAnim {
    pub base: Vec<[f32; 3]>,
    pub idx: Vec<[u16; 2]>,
    pub wts: Vec<[f32; 2]>,
    pub locals: Vec<rw4::Mat34>,
    pub invs: Vec<rw4::Mat34>,
    pub parents: Vec<Option<usize>>,
    pub wave_bone: usize,
    pub clip: Option<rw4::KeyAnim>,
    pub chan_for_bone: Vec<Option<usize>>,
    pub bind_t: Vec<[f32; 3]>,
    pub key0_loc: Vec<[f32; 3]>,
}

#[derive(Component)]
struct StatusText;
/// Camera rendering the playable world; creator thumbnails use separate cameras.
#[derive(Component)]
pub struct GameCamera;
#[derive(Component)]
pub struct GameSun;
#[derive(Component)]
pub(crate) struct MenuScene;

fn spawn_menu_scene(
    mut commands: Commands,
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if originals.resource_index.records.is_empty() {
        return;
    }
    for (instance_id, x) in [(0x067B1055, -2.0), (0x065CC046, 2.0)] {
        let key = dbpf::ResourceKey {
            type_id: decode::TYPE_GMDL,
            group_id: baked_model::CREATURE_GROUP,
            instance_id,
        };
        let result = (|| {
            let model = baked_model::ensure(
                &mut originals,
                key,
                &mut meshes,
                &mut images,
                &mut materials,
            )?;
            let mut transform =
                world_objects::fitted_transform(&model, &meshes, Vec3::new(x, 0., 0.), 2.8)?;
            transform.rotation = Quat::from_rotation_y(std::f32::consts::PI + x * 0.1);
            commands.spawn((
                StageOwned,
                MenuScene,
                baked_model::source(key),
                Mesh3d(model.draw_mesh()),
                MeshMaterial3d(model.material.clone().unwrap()),
                transform,
            ));
            Ok::<_, String>(())
        })();
        if let Err(e) = result {
            warn!("original menu creature: {e}")
        }
    }
    commands.spawn((
        StageOwned,
        MenuScene,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(30., 30.))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.52, 0.61),
            perceptual_roughness: 1.,
            ..default()
        })),
        Transform::from_xyz(0., -0.02, 0.),
    ));
}
fn menu_presentation(
    state: Res<State<GameStage>>,
    options: Res<HudOptions>,
    editor: Res<editor::EditorState>,
    mut roots: Query<
        (
            &mut Visibility,
            Option<&MenuScene>,
            Option<&menu_visuals::MenuVisual>,
        ),
        (With<StageOwned>, Without<ChildOf>, Without<Player>),
    >,
) {
    if *state.get() != GameStage::MainMenu || editor.open {
        return;
    }
    for (mut visibility, scene, galaxy) in &mut roots {
        let visible = if galaxy.is_some() {
            !options.diagnostics
        } else {
            options.diagnostics && scene.is_none()
        };
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired
        }
    }
}

#[derive(Resource, Default)]
struct HudOptions {
    diagnostics: bool,
}

fn hud_keys(keys: Res<ButtonInput<KeyCode>>, mut options: ResMut<HudOptions>, state: Res<State<GameStage>>) {
    // F3 opens the vehicle / building / UFO editor in the Civ and Space stages
    // (`rigid_editor`); Shift+F3 there is the older assembly modal and keeps
    // toggling the diagnostics.
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let editor_key = (matches!(state.get(), GameStage::Civilization | GameStage::Space)
        || front_end::creators::menu_session())
        && !shift;
    if keys.just_pressed(KeyCode::F3) && !editor_key {
        options.diagnostics = !options.diagnostics;
    }
}

#[derive(Component)]
struct Spin {
    speed: f32,
}

fn locate_and_scan(mut install_status: ResMut<InstallStatus>, mut catalog: ResMut<Catalog>) {
    match spore_install::discover() {
        Ok(install) => {
            install_status.root = Some(install.root.clone());
            install_status.ok = true;
            let mut total = 0usize;
            for pkg in &install.packages {
                let name = pkg
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| pkg.display().to_string());
                match dbpf::read_package_index(pkg) {
                    Ok(index) => {
                        total += index.entries.len();
                        catalog.packages.push((name, index.entries.len()));
                    }
                    Err(e) => {
                        warn!("{}: index read failed: {e}", name);
                        catalog.scan_errors.push(format!("{name}: {e}"));
                        catalog.packages.push((name, 0));
                    }
                }
            }
            catalog.total_entries = total;
            install_status.detail = format!(
                "Spore install: {} | {} packages, {} records",
                install.describe(),
                catalog.packages.len(),
                total
            );
            info!("{}", install_status.detail);
        }
        Err(e) => {
            install_status.ok = false;
            install_status.detail = format!("NO VALID SPORE INSTALL: {e}");
            warn!("{}", install_status.detail);
        }
    }
}

// ---------------------------------------------------------------------------
// Original-data loading (capped, fast): icon PNG + game strings for names.
// ---------------------------------------------------------------------------

/// Pull original art/text from the user's install:
/// - first good PNG -> `assets/spore_icon.png` (menu shows real Spore art)
/// - PROP UTF-16 strings (capped scan) -> planet names + HUD counts
fn load_originals(mut originals: ResMut<Originals>) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    // Icon: first PNG >= 48px.
    for (meta, bytes) in decode::collect_pngs(&install, 12, 12) {
        originals.png_count += 1;
        if originals.icon_file.is_none() {
            if let Some(b) = bytes {
                if meta.width >= 48 && meta.height >= 48 && decode::parse_png_dims(&b).is_some() {
                    // Runtime file: `%LOCALAPPDATA%\OpenSpore\` (see user_paths), never
                    // the exe / Spore folders. Nothing loads it back; offscreen
                    // captures and tests skip the write.
                    let capture = cfg!(test)
                        || std::env::args()
                            .any(|a| a == "--quick-capture" || a == "--render-smoke");
                    let wrote = capture
                        || crate::user_paths::write_runtime_file("spore_icon.png", &b).is_ok();
                    if wrote {
                        originals.icon_file = Some(format!(
                            "{} ({}x{} from {})",
                            "spore_icon.png", meta.width, meta.height, meta.package
                        ));
                        info!("original icon: {}", originals.icon_file.as_ref().unwrap());
                    }
                }
            }
        }
    }
    // Strings: capped PROP scan (fast — skips the 1GB audio/graphic blobs).
    let mut seen = std::collections::HashSet::new();
    let mut names: Vec<String> = Vec::new();
    let mut budget = 900;
    for pkg in &install.packages {
        if budget == 0 || names.len() >= 300 {
            break;
        }
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_PROP)
        {
            if budget == 0 || names.len() >= 300 {
                break;
            }
            if e.mem_size > 8192 {
                continue; // skip giant props for startup speed
            }
            budget -= 1;
            let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            for s in decode::extract_utf16_strings(&bytes, 5) {
                let t = s.trim().to_string();
                if t.len() < 3 || t.len() > 28 || !seen.insert(t.clone()) {
                    continue;
                }
                // Planet-name-ish: starts uppercase, mostly letters/spaces/apostrophes.
                if t.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                    && t.chars()
                        .all(|c| c.is_alphabetic() || c == ' ' || c == '\'' || c == '-')
                {
                    names.push(t);
                }
                originals.string_count += 1;
            }
        }
    }
    names.sort();
    names.truncate(64);
    if !names.is_empty() {
        info!(
            "original strings: {} candidates, e.g. {:?}",
            names.len(),
            &names[..names.len().min(5)]
        );
    }
    // Locale text (en-us preferred): real UI/guide strings keyed by instance.
    // NameGen (en-us preferred): authentic planet/people word lists.
    let mut locale_pkgs: Vec<_> = install.packages.iter().collect();
    locale_pkgs.sort_by_key(|p| {
        let s = p.to_string_lossy().to_lowercase();
        (!s.contains("en-us")) as u8
    });
    let mut namegen_names: Vec<String> = Vec::new();
    let mut seen_loc = std::collections::HashSet::new();
    for pkg in locale_pkgs {
        let fname = pkg
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if fname != "Text.package" && fname != "NameGeneration.package" {
            continue;
        }
        // Only first locale found (en-us when present).
        if originals.locale_strings > 0 && namegen_names.len() > 200 {
            break;
        }
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in &index.entries {
            if e.key.type_id == decode::TYPE_LOCALE_TEXT {
                if originals.locale_strings >= 4000 {
                    continue;
                }
                let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                    continue;
                };
                for (id, text) in decode::parse_locale_table(&bytes) {
                    if seen_loc.insert((id, text.clone())) {
                        originals.locale_strings += 1;
                        if originals.locale_sample.len() < 8
                            && text.len() > 12
                            && text.len() < 120
                            && !text.starts_with("0x")
                        {
                            originals.locale_sample.push(text);
                        }
                    }
                }
            } else if e.key.type_id == decode::TYPE_NAMEGEN {
                let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                    continue;
                };
                for w in decode::parse_name_list(&bytes) {
                    if w.len() >= 3 && w.len() <= 24 {
                        namegen_names.push(w);
                    }
                }
            }
        }
        if originals.locale_strings > 0 && !namegen_names.is_empty() {
            break; // en-us (or first locale) done
        }
    }
    if !namegen_names.is_empty() {
        namegen_names.sort();
        namegen_names.dedup();
        namegen_names.truncate(512);
        info!(
            "original namegen: {} names, e.g. {:?}",
            namegen_names.len(),
            &namegen_names[..namegen_names.len().min(6)]
        );
        // Authentic planet names win over the PROP heuristic.
        let take = namegen_names.len().min(64);
        names = namegen_names[..take].to_vec();
    }
    originals.planet_names = names;
}

fn raster_to_image(tex: &decode::RasterTexture, images: &mut Assets<Image>) -> Handle<Image> {
    let format = match tex.format {
        decode::RasterFormat::Bc1 => TextureFormat::Bc1RgbaUnormSrgb,
        decode::RasterFormat::Bc2 => TextureFormat::Bc2RgbaUnormSrgb,
        decode::RasterFormat::Bc3 => TextureFormat::Bc3RgbaUnormSrgb,
        decode::RasterFormat::Rgba8 => TextureFormat::Rgba8UnormSrgb,
    };
    images.add(Image::new(
        Extent3d {
            width: tex.width,
            height: tex.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        tex.data.clone(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

/// Catalog original animations (headers only; totals are index-free).
fn load_original_anims(mut originals: ResMut<Originals>) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    let (total, sample) = decode::collect_anims(&install, 200);
    let mut folders = std::collections::HashMap::new();
    for a in &sample {
        // Folder = path segment under animations\_final\, e.g. "tribe".
        let key = a
            .path
            .split('\\')
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .nth(1)
            .unwrap_or(&"?")
            .to_string();
        *folders.entry(key).or_insert(0) += 1;
    }
    let mut folders: Vec<_> = folders.into_iter().collect();
    folders.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    folders.truncate(8);
    originals.anim_total = total;
    originals.anim_folders = folders;
    info!("original anims: {} total", total);
}

/// Upload a capped set of original raster textures to the GPU.
fn load_original_textures(mut originals: ResMut<Originals>, mut images: ResMut<Assets<Image>>) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    let mut scanned = 0;
    'outer: for pkg in &install.packages {
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_RASTER)
        {
            if originals.textures.len() >= 8 || scanned >= 800 {
                break 'outer;
            }
            // Skip giant textures for startup speed; 256px and below first.
            if e.mem_size > 400000 {
                continue;
            }
            scanned += 1;
            let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            if let Some(tex) = decode::decode_raster_base(&bytes) {
                if tex.width <= 512 && tex.height <= 512 {
                    let h = raster_to_image(&tex, &mut images);
                    originals.textures.push(h);
                    info!(
                        "original texture {}x{} {} from {}",
                        tex.width,
                        tex.height,
                        tex.format.label(),
                        pkg.file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                }
            }
        }
    }
}

/// Import a capped set of original RW4 meshes as Bevy assets.
/// A part's authored slot alignment root matrix + its rig size.
fn part_root_and_bones(blob: &[u8]) -> Option<(Option<rw4::Mat34>, usize)> {
    let model = rw4::extract_rigged_model(blob)?;
    let (locals, _invs) = rw4::bind_locals(&model.bones, &model.poses)?;
    Some((locals.first().copied(), model.bones.len()))
}

/// Pick the default slot bone for a newly added part of `rig_bones` joints:
/// limb parts go to a limb slot, details to the head, using bind-position
/// semantics. Falls back to the topmost bone.
fn default_slot_for_part(
    originals: &Originals,
    rig_bones: usize,
    want: game::SlotKind,
) -> Option<usize> {
    let demo = originals.rigged_demo.as_ref()?;
    let worlds = rw4::fk_worlds(&demo.parents, &demo.locals)?;
    let family = game::part_family(rig_bones);
    // Candidate bones by kind, in rig order (stable).
    let mut by_kind: Vec<usize> = Vec::new();
    let mut fallback = 0usize;
    let mut best_y = f32::MIN;
    for (bi, w) in worlds.iter().enumerate() {
        let p = Vec3::new(w[0][3], w[1][3], w[2][3]);
        if w[1][3] > best_y {
            best_y = w[1][3];
            fallback = bi;
        }
        let want_limb = family == "limb" || want == game::SlotKind::Limb;
        if want_limb && game::slot_kind(p) == game::SlotKind::Limb {
            by_kind.push(bi);
        } else if !want_limb && game::slot_kind(p) == game::SlotKind::Head {
            by_kind.push(bi);
        }
    }
    by_kind.first().copied().or(Some(fallback))
}

/// A part's authored slot alignment: the root bone's bind matrix from its own
/// largest rig (None when the part has no rig). Rigid RW4 parts carry this
/// rotation so they seat correctly on the creature bone they mount on.
fn part_root_matrix(blob: &[u8]) -> Option<rw4::Mat34> {
    let model = rw4::extract_rigged_model(blob)?;
    let (locals, _invs) = rw4::bind_locals(&model.bones, &model.poses)?;
    locals.first().copied()
}
fn load_original_models(
    mut originals: ResMut<Originals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    let mut stats = rw4::MeshStats::default();
    let mut scanned = 0;
    'outer: for pkg in &install.packages {
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
        {
            if (originals.models.len() >= 12 && originals.morph_demo.is_some()) || scanned >= 120 {
                break 'outer;
            }
            if e.mem_size > 1_500_000 || e.mem_size < 200 {
                continue;
            }
            scanned += 1;
            let record_bytes = dbpf::read_entry_bytes(pkg, e).ok();
            let part_info = record_bytes.as_deref().and_then(part_root_and_bones);
            let (part_root_mat, rig_bones) = part_info.unwrap_or((None, 0));
            for em in rw4::meshes_from_package_record(pkg, e, &mut stats) {
                if originals
                    .models
                    .iter()
                    .any(|m| m.source_definition == Some(e.key))
                {
                    continue;
                }
                if em.positions.len() > 20000 || em.positions.len() < 8 {
                    continue;
                }
                let mut m = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::default(),
                );
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION, em.positions.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, em.normals.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_UV_0, em.uvs.clone());
                m.insert_indices(Indices::U16(em.indices.clone()));
                originals.model_verts += em.positions.len();
                let scale = (2.0 / em.bbox_diagonal().max(1e-6)).clamp(0.001, 1000.0);
                let handle = meshes.add(m);
                // First morph-bearing mesh becomes the live morph demo.
                if originals.morph_demo.is_none() && !em.morphs.is_empty() {
                    originals.morph_demo = Some(MorphDemo {
                        mesh: handle.clone(),
                        base: em.positions.clone(),
                        deltas: em.morphs.clone(),
                        scale,
                        shapes: em.morphs.len(),
                    });
                    info!("original morph demo: {} shapes", em.morphs.len());
                }
                let fallback = OriginalModel {
                    source_definition: None,
                    material: None,
                    surfaces: vec![],
                    handle,
                    scale,
                    instance: e.key.instance_id,
                    root: part_root_mat,
                    rig_bones,
                };
                let definition = asset_catalog::PartDefinition::raw_model(e.key);
                let mut model = editor::import_definition(
                    &originals,
                    &definition,
                    &mut meshes,
                    &mut images,
                    &mut materials,
                )
                .unwrap_or_else(|error| {
                    warn!(
                        "original model materials {:08X}: {error}",
                        e.key.instance_id
                    );
                    fallback
                });
                if let Some(mesh) = meshes.get(&model.handle) {
                    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                    {
                        let (mut low, mut high) =
                            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
                        for p in positions {
                            let p = Vec3::from_array(*p);
                            low = low.min(p);
                            high = high.max(p);
                        }
                        model.scale = (2. / (high - low).length().max(1e-6)).clamp(0.001, 1000.);
                    }
                }
                originals.models.push(model);
                info!(
                    "original model: {} verts {} tris diag {:.2}",
                    em.positions.len(),
                    em.indices.len() / 3,
                    em.bbox_diagonal()
                );
                if originals.models.len() >= 12 {
                    break 'outer;
                }
            }
        }
    }
    info!(
        "rw4 scan: {} records, {} meshes ({} static), {} verts",
        scanned, stats.meshes, stats.static_ok, stats.verts
    );
}

/// Show imported original models on the menu stage.
fn spawn_menu_models(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.72, 0.68),
        perceptual_roughness: 0.6,
        ..default()
    });
    for (i, model) in originals.models.iter().take(3).enumerate() {
        commands.spawn((
            StageOwned,
            Spin { speed: 0.4 },
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(model.material.clone().unwrap_or_else(|| mat.clone())),
            Transform::from_xyz(-6.0 + i as f32 * 6.0, 1.5, -4.0)
                .with_scale(Vec3::splat(model.scale)),
        ));
    }
}

/// Load the first animation-ready skeleton as a debug bind-pose view.
fn load_original_skeleton(mut originals: ResMut<Originals>) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    let mut stats = rw4::MeshStats::default();
    let mut scanned = 0;
    for pkg in &install.packages {
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
        {
            if scanned >= 80 || e.mem_size > 1_000_000 {
                continue;
            }
            scanned += 1;
            let (skels, skins) = rw4::rig_from_package_record(pkg, e, &mut stats);
            // Pair the first skeleton whose bone count matches a skin.
            let mut pair: Option<(Vec<rw4::Bone>, Vec<rw4::BonePose>)> = None;
            'pair: for bones in &skels {
                for poses in &skins {
                    if bones.len() == poses.len() && !bones.is_empty() {
                        pair = Some((bones.clone(), poses.clone()));
                        break 'pair;
                    }
                }
            }
            if let Some((bones, poses)) = pair {
                let pts = rw4::bind_world_positions(&bones, &poses);
                if pts.len() < 2 {
                    continue;
                }
                // Normalize extent to ~2 units.
                let mut lo = [f32::INFINITY; 3];
                let mut hi = [f32::NEG_INFINITY; 3];
                for p in &pts {
                    for i in 0..3 {
                        lo[i] = lo[i].min(p[i]);
                        hi[i] = hi[i].max(p[i]);
                    }
                }
                let ext =
                    ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2))
                        .sqrt()
                        .max(1e-6);
                let s = 2.0 / ext;
                let joints: Vec<Vec3> = pts
                    .iter()
                    .map(|p| Vec3::new((p[0] - lo[0]) * s, (p[1] - lo[1]) * s, (p[2] - lo[2]) * s))
                    .collect();
                let links: Vec<(usize, usize)> = bones
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| b.parent.map(|par| (par, i)))
                    .collect();
                let n = bones.len();
                originals.skeleton = Some(SkeletonView {
                    joints,
                    links,
                    bones: n,
                });
                info!("original skeleton: {n} bones debug view ready");
                return;
            }
        }
    }
}

/// Show the debug skeleton on the menu stage (joints + parent links).
fn spawn_menu_skeleton(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
) {
    let Some(view) = originals.skeleton.as_ref() else {
        return;
    };
    let joint_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.3, 0.3),
        emissive: Color::srgb(0.6, 0.05, 0.05).into(),
        ..default()
    });
    let link_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.8, 0.3),
        emissive: Color::srgb(0.4, 0.25, 0.05).into(),
        ..default()
    });
    let joint_mesh = meshes.add(Sphere::new(0.09));
    let link_mesh = meshes.add(Cuboid::new(0.05, 0.05, 1.0));
    for j in &view.joints {
        commands.spawn((
            StageOwned,
            Mesh3d(joint_mesh.clone()),
            MeshMaterial3d(joint_mat.clone()),
            Transform::from_translation(*j + Vec3::new(6.0, 0.5, -4.0)),
        ));
    }
    for (a, b) in &view.links {
        if let (Some(pa), Some(pb)) = (view.joints.get(*a), view.joints.get(*b)) {
            let (pa, pb) = (
                *pa + Vec3::new(6.0, 0.5, -4.0),
                *pb + Vec3::new(6.0, 0.5, -4.0),
            );
            let d = pb - pa;
            let len = d.length().max(1e-4);
            let up = if d.normalize().cross(Vec3::Y).length() < 0.01 {
                Vec3::X
            } else {
                Vec3::Y
            };
            commands.spawn((
                StageOwned,
                Mesh3d(link_mesh.clone()),
                MeshMaterial3d(link_mat.clone()),
                Transform::from_translation(pa + d * 0.5)
                    .looking_at(pb, up)
                    .with_scale(Vec3::new(1.0, 1.0, len)),
            ));
        }
    }
}

/// Which loaded prop rig the preview shows (cycles with `O`).
#[derive(Resource, Default)]
pub struct PropRigPick(pub usize);

/// Spawn the prop-rig preview: joints + links of the selected original prop
/// rig, standing off to one side of the creature.
#[derive(Component)]
pub struct PropRigJoint {
    pub idx: usize,
}

#[derive(Component)]
pub struct PropRigLink {
    pub a: usize,
    pub b: usize,
}

fn spawn_prop_rig_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
    pick: Res<PropRigPick>,
) {
    let Some(rig) = originals.prop_rigs.get(pick.0) else {
        return;
    };
    let joint_mesh = meshes.add(Sphere::new(0.06));
    let link_mesh = meshes.add(Cuboid::new(0.035, 0.035, 1.0));
    let joint_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.6, 0.9),
        emissive: Color::srgb(0.4, 0.15, 0.4).into(),
        ..default()
    });
    let link_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.35, 0.6),
        ..default()
    });
    for idx in 0..rig.bones {
        commands.spawn((
            StageOwned,
            PropRigJoint { idx },
            Mesh3d(joint_mesh.clone()),
            MeshMaterial3d(joint_mat.clone()),
            Transform::from_xyz(0.0, 1.5, -5.0),
        ));
    }
    for (i, par) in rig.parents.iter().enumerate() {
        if let Some(p) = par {
            if *p != i {
                commands.spawn((
                    StageOwned,
                    PropRigLink { a: *p, b: i },
                    Mesh3d(link_mesh.clone()),
                    MeshMaterial3d(link_mat.clone()),
                    Transform::from_xyz(0.0, 1.5, -5.0),
                ));
            }
        }
    }
    info!(
        "prop rig preview: inst {:#X}, {} bones, clip {}",
        rig.instance,
        rig.bones,
        rig.clip.is_some()
    );
}

/// Drive the selected prop rig with its own embedded clip (FK over bind
/// locals); `O` cycles to another loaded rig.
fn animate_prop_rig(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    originals: Res<Originals>,
    mut pick: ResMut<PropRigPick>,
    state: Res<State<GameStage>>,
    mut joints: Query<(&PropRigJoint, &mut Transform)>,
    mut links: Query<(&PropRigLink, &mut Transform), Without<PropRigJoint>>,
) {
    if originals.prop_rigs.is_empty() {
        return;
    }
    if *state.get() == GameStage::MainMenu
        && keys.just_pressed(KeyCode::KeyO)
        && originals.prop_rigs.len() > 1
    {
        pick.0 = (pick.0 + 1) % originals.prop_rigs.len();
        info!(
            "prop rig preview: {:#X}",
            originals.prop_rigs[pick.0].instance
        );
    }
    let Some(rig) = originals.prop_rigs.get(pick.0) else {
        return;
    };
    let mut locals = rig.locals.clone();
    if let Some(clip) = rig.clip.as_ref() {
        if clip.length > 0.0 {
            let ct = time.elapsed_secs().rem_euclid(clip.length);
            for (bi, slot) in rig.chan_for_bone.iter().enumerate() {
                if bi >= locals.len() {
                    break;
                }
                if let Some(ci) = slot {
                    if let Some((q, loc)) = clip.sample(*ci, ct) {
                        let r = rw4::quat_to_mat33(q);
                        for rr in 0..3 {
                            for cc in 0..3 {
                                locals[bi][rr][cc] = r[rr][cc];
                            }
                        }
                        let bt = rig.bind_t.get(bi).copied().unwrap_or([0.0; 3]);
                        let k0 = rig.key0_loc.get(bi).copied().unwrap_or([0.0; 3]);
                        locals[bi][0][3] = bt[0] + (loc[0] - k0[0]);
                        locals[bi][1][3] = bt[1] + (loc[1] - k0[1]);
                        locals[bi][2][3] = bt[2] + (loc[2] - k0[2]);
                    }
                }
            }
        }
    }
    let Some(worlds) = rw4::fk_worlds(&rig.parents, &locals) else {
        return;
    };
    let base = Vec3::new(0.0, 1.5, -5.0);
    let s = rig.scale;
    let pts: Vec<Vec3> = worlds
        .iter()
        .map(|w| base + Vec3::new(w[0][3] * s, w[1][3] * s, w[2][3] * s))
        .collect();
    for (j, mut t) in &mut joints {
        if let Some(p) = pts.get(j.idx) {
            t.translation = *p;
        }
    }
    for (l, mut t) in &mut links {
        if let (Some(pa), Some(pb)) = (pts.get(l.a), pts.get(l.b)) {
            let mid = (*pa + *pb) * 0.5;
            let d = *pb - *pa;
            let len = d.length().max(1e-4);
            t.translation = mid;
            t.rotation = Quat::from_rotation_arc(Vec3::Z, d / len);
            t.scale = Vec3::new(1.0, 1.0, len);
        }
    }
}

/// Spawn the original rigged character: body mesh at bind pose plus a
/// skeleton view driven by the file's own animation clip.
#[derive(Component)]
pub struct RiggedJoint {
    pub idx: usize,
}

#[derive(Component)]
pub struct RiggedLink {
    pub a: usize,
    pub b: usize,
}

/// A part mounted on a creature slot bone: follows that bone's animated
/// world transform (part assembly), with the part's own root alignment.
/// `part` indexes into `RunProgress::parts`.
#[derive(Component)]
pub struct RiggedMount {
    pub part: usize,
    /// Authored slot alignment (part root bind matrix), None = identity.
    pub root: Option<rw4::Mat34>,
    /// Source part instance the mesh handle belongs to.
    pub instance: u32,
}

/// Spawn one mount entity per saved part (the assembled creature).
fn spawn_rigged_mount(
    commands: &mut Commands,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    originals: &Originals,
    run: &RunProgress,
) {
    for (i, p) in run.parts.iter().enumerate() {
        let Some(model) = originals.models.iter().find(|m| m.instance == p.instance) else {
            continue;
        };
        commands.spawn((
            StageOwned,
            RiggedMount {
                part: i,
                root: model.root,
                instance: model.instance,
            },
            Mesh3d(model.draw_mesh()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.75, 0.6),
                perceptual_roughness: 0.6,
                ..default()
            })),
            Transform::from_xyz(-8.0, 1.25, -5.0),
        ));
    }
}

fn spawn_rigged_demo(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
    run: Res<RunProgress>,
) {
    let Some(demo) = originals.rigged_demo.as_ref() else {
        return;
    };
    // Body at bind pose (RW4 bodies are rigid; the rig carries the motion).
    commands.spawn((
        StageOwned,
        Mesh3d(demo.mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.65, 0.55),
            perceptual_roughness: 0.7,
            ..default()
        })),
        Transform::from_xyz(-8.0, 1.25, -5.0).with_scale(Vec3::splat(demo.scale)),
    ));
    // Animated joints + links (real clip, FK).
    let joint_mesh = meshes.add(Sphere::new(0.07));
    let link_mesh = meshes.add(Cuboid::new(0.04, 0.04, 1.0));
    let joint_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.9, 1.0),
        emissive: Color::srgb(0.1, 0.4, 0.6).into(),
        ..default()
    });
    let link_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.6, 0.8),
        ..default()
    });
    for idx in 0..demo.bones {
        commands.spawn((
            StageOwned,
            RiggedJoint { idx },
            Mesh3d(joint_mesh.clone()),
            MeshMaterial3d(joint_mat.clone()),
            Transform::from_xyz(-8.0, 1.25, -5.0),
        ));
    }
    for (i, par) in demo.parents.iter().enumerate() {
        if let Some(p) = par {
            if *p != i {
                commands.spawn((
                    StageOwned,
                    RiggedLink { a: *p, b: i },
                    Mesh3d(link_mesh.clone()),
                    MeshMaterial3d(link_mat.clone()),
                    Transform::from_xyz(-8.0, 1.25, -5.0),
                ));
            }
        }
    }
    // Mount every saved part on its slot bone (user data — the saved creature
    // stores part + bone + transform per part, exactly like the original).
    spawn_rigged_mount(&mut commands, &mut materials, &originals, &run);
    info!(
        "rigged character spawned: {} bones, clip {:.1}s, {} part(s)",
        demo.bones,
        demo.clip.length,
        run.parts.len()
    );
}

/// Drive the rigged character's skeleton from its real clip (FK). Joints are
/// placed at bone world positions; links stretch between parent and child;
/// the mounted part follows its slot bone with the user's yaw/scale.
fn animate_rigged(
    time: Res<Time>,
    originals: Res<Originals>,
    run: Res<RunProgress>,
    mut joints: Query<(&RiggedJoint, &mut Transform)>,
    mut links: Query<(&RiggedLink, &mut Transform), Without<RiggedJoint>>,
    mut mounts: Query<(&RiggedMount, &mut Transform), (Without<RiggedJoint>, Without<RiggedLink>)>,
) {
    let Some(demo) = originals.rigged_demo.as_ref() else {
        return;
    };
    let mut locals = demo.locals.clone();
    if demo.clip.length > 0.0 {
        let ct = time.elapsed_secs().rem_euclid(demo.clip.length);
        for (bi, slot) in demo.chan_for_bone.iter().enumerate() {
            if bi >= locals.len() {
                break;
            }
            if let Some(ci) = slot {
                if let Some((q, loc)) = demo.clip.sample(*ci, ct) {
                    let r = rw4::quat_to_mat33(q);
                    for rr in 0..3 {
                        for cc in 0..3 {
                            locals[bi][rr][cc] = r[rr][cc];
                        }
                    }
                    let bt = demo.bind_t.get(bi).copied().unwrap_or([0.0; 3]);
                    let k0 = demo.key0_loc.get(bi).copied().unwrap_or([0.0; 3]);
                    locals[bi][0][3] = bt[0] + (loc[0] - k0[0]);
                    locals[bi][1][3] = bt[1] + (loc[1] - k0[1]);
                    locals[bi][2][3] = bt[2] + (loc[2] - k0[2]);
                }
            }
        }
    }
    let Some(worlds) = rw4::fk_worlds(&demo.parents, &locals) else {
        return;
    };
    let base = Vec3::new(-8.0, 1.25, -5.0);
    let s = demo.scale;
    let bone_pos: Vec<Vec3> = worlds
        .iter()
        .map(|w| base + Vec3::new(w[0][3] * s, w[1][3] * s, w[2][3] * s))
        .collect();
    for (j, mut t) in &mut joints {
        if let Some(p) = bone_pos.get(j.idx) {
            t.translation = *p;
        }
    }
    for (l, mut t) in &mut links {
        if let (Some(pa), Some(pb)) = (bone_pos.get(l.a), bone_pos.get(l.b)) {
            let mid = (*pa + *pb) * 0.5;
            let d = *pb - *pa;
            let len = d.length().max(1e-4);
            t.translation = mid;
            t.rotation = Quat::from_rotation_arc(Vec3::Z, d / len);
            t.scale = Vec3::new(1.0, 1.0, len);
        }
    }
    for (m, mut t) in &mut mounts {
        // Live slot choice + user tweaks from this part's save record.
        let Some(p) = run.parts.get(m.part) else {
            continue;
        };
        let Some(model) = originals.models.iter().find(|d| d.instance == p.instance) else {
            continue;
        };
        let bone = (p.bone as usize).min(worlds.len().saturating_sub(1));
        let Some(w) = worlds.get(bone) else {
            continue;
        };
        // Bone world × slide × part root alignment × user yaw = placement.
        let user = rw4::rot_y(p.yaw);
        let slide: rw4::Mat34 = [
            [1.0, 0.0, 0.0, p.offset[0]],
            [0.0, 1.0, 0.0, p.offset[1]],
            [0.0, 0.0, 1.0, p.offset[2]],
        ];
        let combined = match m.root {
            Some(r) => rw4::mat34_mul(&rw4::mat34_mul(&rw4::mat34_mul(w, &slide), &r), &user),
            None => rw4::mat34_mul(&rw4::mat34_mul(w, &slide), &user),
        };
        let pos = base + Vec3::new(combined[0][3] * s, combined[1][3] * s, combined[2][3] * s);
        let rot = Mat3::from_cols(
            Vec3::new(combined[0][0], combined[1][0], combined[2][0]),
            Vec3::new(combined[0][1], combined[1][1], combined[2][1]),
            Vec3::new(combined[0][2], combined[1][2], combined[2][2]),
        );
        t.translation = pos;
        t.rotation = Quat::from_mat3(&rot);
        t.scale = Vec3::splat(model.scale * 0.6 * p.scale);
    }
}

/// Keep mount entities in sync with the saved part list: add/remove on
/// count changes, hot-swap meshes when a part's instance changes.
fn rigged_mount_sync(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
    run: Res<RunProgress>,
    state: Res<State<GameStage>>,
    editor: Res<editor::EditorState>,
    mut mounts: Query<(Entity, &mut RiggedMount, &mut Mesh3d)>,
) {
    // These mounts belong to the imported menu animation preview. Gameplay
    // creatures and mechanical vehicles assemble their own children.
    if *state.get() != GameStage::MainMenu {
        for (entity, _, _) in &mounts {
            commands.entity(entity).despawn();
        }
        return;
    }
    if editor.open {
        return;
    }
    if originals.rigged_demo.is_none() {
        return;
    }
    let want = run.parts.len();
    let have = mounts.iter().len();
    if have != want {
        // Rebuild the mount set (part added/removed).
        for (e, _, _) in &mounts {
            commands.entity(e).despawn();
        }
        spawn_rigged_mount(&mut commands, &mut materials, &originals, &run);
        return;
    }
    for (_, mut m, mut mesh) in &mut mounts {
        let Some(p) = run.parts.get(m.part) else {
            continue;
        };
        let Some(model) = originals.models.iter().find(|d| d.instance == p.instance) else {
            continue;
        };
        if m.instance != model.instance {
            m.instance = model.instance;
            m.root = model.root;
            mesh.0 = model.draw_mesh();
        }
    }
}

/// Creature assembly editor (menu rig), operating on the saved part list:
/// `[`/`]` cycle the focused part's instance, `,`/`.` its slot bone,
/// `Tab` focus the next part, `N` add a part, `Backspace` remove it,
/// `R` rotate, `-`/`=` scale. All persisted, mirroring the original's
/// saved-creature part+bone+transform records.
fn rigged_assembly_keys(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameStage>>,
    originals: Res<Originals>,
    mut run: ResMut<RunProgress>,
) {
    if !originals.part_catalog.parts.is_empty() {
        return;
    }
    if *state.get() != GameStage::MainMenu {
        return;
    }
    let n = originals.models.len() as u32;
    let bones = originals
        .rigged_demo
        .as_ref()
        .map(|d| d.bones as u32)
        .unwrap_or(0);
    let mut changed = false;
    if keys.just_pressed(KeyCode::Tab) && !run.parts.is_empty() {
        run.focused_part = (run.focused_part + 1) % run.parts.len() as u32;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyN) && n > 0 {
        // Mount a part that isn't on the creature yet (families rotate).
        let mounted: Vec<u32> = run.parts.iter().map(|p| p.instance).collect();
        let pick = originals
            .models
            .iter()
            .find(|m| !mounted.contains(&m.instance))
            .or_else(|| originals.models.first())
            .expect("models non-empty");
        let bone = default_slot_for_part(&originals, pick.rig_bones, game::SlotKind::Limb)
            .unwrap_or(run.accessory_bone as usize) as u32;
        run.part_add(pick.instance, bone);
        changed = true;
    }
    if keys.just_pressed(KeyCode::Backspace) && !run.parts.is_empty() {
        run.part_remove_focused();
        changed = true;
    }
    let fi = game::focus_index(run.focused_part, run.parts.len()) as usize;
    if fi < run.parts.len() {
        let part = &mut run.parts[fi];
        if keys.just_pressed(KeyCode::BracketLeft) || keys.just_pressed(KeyCode::BracketRight) {
            if n > 0 {
                let dir = if keys.just_pressed(KeyCode::BracketRight) {
                    1
                } else {
                    -1
                };
                let cur = originals
                    .models
                    .iter()
                    .position(|m| m.instance == part.instance)
                    .map(|i| i as i32 + 1)
                    .unwrap_or(0);
                let next = game::cycle_index(cur, dir, n as i32);
                part.instance = if next == 0 {
                    0
                } else {
                    originals.models[next as usize - 1].instance
                };
                changed = true;
            }
        }
        if bones > 0 && (keys.just_pressed(KeyCode::Comma) || keys.just_pressed(KeyCode::Period)) {
            let dir = if keys.just_pressed(KeyCode::Period) {
                1
            } else {
                -1
            };
            part.bone = game::cycle_bone(part.bone, dir, bones);
            changed = true;
        }
        if keys.just_pressed(KeyCode::KeyR) {
            part.yaw = game::wrap_yaw(part.yaw + std::f32::consts::FRAC_PI_4);
            changed = true;
        }
        if keys.just_pressed(KeyCode::Minus) {
            part.scale = game::scaled(part.scale, 0.9);
            changed = true;
        }
        if keys.just_pressed(KeyCode::Equal) {
            part.scale = game::scaled(part.scale, 1.1);
            changed = true;
        }
        // Slide the part on its slot: I/K x, J/L z, U/O y.
        if game::apply_slide_keys(&keys, part) {
            changed = true;
        }
    }
    // Mirror the focused part onto its sagittal pair (M).
    if keys.just_pressed(KeyCode::KeyM) {
        let focus = run.focused_part;
        if let Some(demo) = originals.rigged_demo.as_ref() {
            if let Some(worlds) = rw4::fk_worlds(&demo.parents, &demo.locals) {
                let map = rw4::mirror_map(&worlds, 0.2);
                if game::part_add_mirrored(&mut run.parts, focus, &map) {
                    changed = true;
                }
            }
        }
    }
    if changed {
        run.sync_legacy_accessory();
        run.request_save();
    }
}

/// Spawn the live morph demo on the menu stage (shares its mesh asset
/// with the static gallery copy, so both breathe in sync).
fn spawn_morph_demo(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
) {
    let Some(demo) = originals.morph_demo.as_ref() else {
        return;
    };
    let speeds: Vec<f32> = demo
        .deltas
        .iter()
        .enumerate()
        .map(|(i, _)| 0.6 + i as f32 * 0.35)
        .collect();
    commands.spawn((
        StageOwned,
        Mesh3d(demo.mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.6, 0.4),
            perceptual_roughness: 0.5,
            ..default()
        })),
        Transform::from_xyz(0.0, 1.5, 4.0).with_scale(Vec3::splat(demo.scale)),
        MorphAnim {
            base: demo.base.clone(),
            deltas: demo.deltas.clone(),
            speeds,
        },
    ));
    info!("morph demo spawned with {} shapes", demo.shapes);
}

/// Procedurally drive morph weights (sine chorus) and rewrite positions.
/// Normals stay at base pose (documented approximation; deltas are small).
fn animate_morphs(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<(&Mesh3d, &MorphAnim)>,
) {
    let t = time.elapsed_secs();
    for (mesh3d, anim) in &q {
        if anim.deltas.is_empty() || anim.base.is_empty() {
            continue;
        }
        let mut out = anim.base.clone();
        for (s, shape) in anim.deltas.iter().enumerate() {
            let w = 0.5 + 0.5 * (t * anim.speeds[s % anim.speeds.len()] + s as f32 * 1.7).sin();
            for (p, d) in out.iter_mut().zip(shape.iter()) {
                p[0] += d[0] * w;
                p[1] += d[1] * w;
                p[2] += d[2] * w;
            }
        }
        if let Some(mut mesh) = meshes.get_mut(&mesh3d.0) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, out);
        }
    }
}

/// Find the best skinnable model: mesh skin influences resolved against a
/// file-local skeleton, scored by influenced verts. Unknown bones hold bind.
fn load_skinned_demo(mut originals: ResMut<Originals>, mut meshes: ResMut<Assets<Mesh>>) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    let mut best: Option<(
        rw4::ExtractedMesh,
        Vec<rw4::Mat34>,
        Vec<rw4::Mat34>,
        Vec<Option<usize>>,
        usize,
        Option<rw4::ClipPair>,
    )> = None;
    let mut best_score = 0usize;
    let mut scanned = 0;
    for pkg in &install.packages {
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
        {
            if e.mem_size > 1_000_000 || e.mem_size < 200 {
                continue;
            }
            scanned += 1;
            if scanned > 150 {
                break;
            }
            let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            if let Some((em, locals, invs, parents, wave, pair)) =
                rw4::score_skinned_candidate(&blob)
            {
                let n = locals.len();
                let score = em
                    .skin_idx
                    .iter()
                    .filter(|p| (p[0] as usize) < n || (p[1] as usize) < n)
                    .count();
                if score > best_score {
                    best_score = score;
                    best = Some((em, locals, invs, parents, wave, pair));
                }
            }
        }
    }
    if let Some((em, locals, invs, parents, wave, pair)) = best {
        let n = locals.len();
        let (clip, chan_for_bone, bind_t, key0_loc) = match pair {
            Some(p) => (Some(p.clip), p.chan_for_bone, p.bind_t, p.key0_loc),
            None => (
                None,
                vec![None; n],
                locals.iter().map(|m| [m[0][3], m[1][3], m[2][3]]).collect(),
                vec![[0.0; 3]; n],
            ),
        };
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, em.positions.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, em.normals.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, em.uvs.clone());
        m.insert_indices(Indices::U16(em.indices.clone()));
        let scale = (2.0 / em.bbox_diagonal().max(1e-6)).clamp(0.001, 1000.0);
        originals.skinned_demo = Some(SkinnedDemo {
            mesh: meshes.add(m),
            base: em.positions.clone(),
            idx: em.skin_idx.clone(),
            wts: em.skin_w.clone(),
            locals,
            invs,
            parents,
            wave_bone: wave,
            scale,
            bones: n,
            clip,
            chan_for_bone,
            bind_t,
            key0_loc,
        });
        info!("original skinned demo ready ({best_score} verts, wave bone {wave})");
    }
}

/// A small original rig (from gmdl-referenced records): skeleton + bind
/// locals + its embedded animation clips. Geometry-free by nature.
#[derive(Clone, Default)]
pub struct PropRig {
    pub instance: u32,
    pub bones: usize,
    pub parents: Vec<Option<usize>>,
    pub locals: Vec<rw4::Mat34>,
    pub invs: Vec<rw4::Mat34>,
    pub chan_for_bone: Vec<Option<usize>>,
    pub bind_t: Vec<[f32; 3]>,
    pub key0_loc: Vec<[f32; 3]>,
    pub clip: Option<rw4::KeyAnim>,
    pub scale: f32,
}

/// Load a few original prop rigs from the gmdl-referenced rig records
/// (RW4 variants tagged 0xCAFED00D: skeleton + anim, no meshes).
fn load_prop_rigs(mut originals: ResMut<Originals>) {
    let mut rigs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut records: Vec<_> = originals
        .resource_index
        .records
        .keys()
        .filter(|k| k.type_id == decode::TYPE_GMDL)
        .copied()
        .collect();
    records.sort_by_key(|k| (k.group_id, k.instance_id));
    'outer: for record in records.into_iter().take(512) {
        let Ok(blob) = originals.resource_index.read(record) else {
            continue;
        };
        let Some(g) = gmdl::parse_mesh_table(&blob) else {
            continue;
        };
        for key in g.table_meshes {
            if key.type_id != decode::TYPE_RW4_MODEL
                || !seen.insert((key.group_id, key.instance_id))
            {
                continue;
            }
            let dbkey = dbpf::ResourceKey {
                type_id: key.type_id,
                group_id: key.group_id,
                instance_id: key.instance_id,
            };
            let Ok(bytes) = originals.resource_index.read(dbkey) else {
                continue;
            };
            let mut stats = rw4::MeshStats::default();
            let (skels, skins) = rw4::extract_rig(&bytes, &mut stats);
            let Some(bones) = skels.into_iter().max_by_key(|s| s.len()) else {
                continue;
            };
            if bones.len() < 3 {
                continue;
            }
            let Some(poses) = skins.into_iter().find(|p| p.len() == bones.len()) else {
                continue;
            };
            let Some((locals, invs)) = rw4::bind_locals(&bones, &poses) else {
                continue;
            };
            let anims = rw4::extract_anims(&bytes, &mut stats);
            let (clip, chan_for_bone, bind_t, key0_loc) =
                match rw4::pair_clip(&bones, &locals, &anims) {
                    Some(p) => (Some(p.clip), p.chan_for_bone, p.bind_t, p.key0_loc),
                    None => (None, vec![None; bones.len()], Vec::new(), Vec::new()),
                };
            rigs.push(PropRig {
                instance: key.instance_id,
                bones: bones.len(),
                parents: bones.iter().map(|b| b.parent).collect(),
                locals,
                invs,
                clip,
                chan_for_bone,
                bind_t,
                key0_loc,
                scale: 1.5,
            });
            if rigs.len() >= 4 {
                break 'outer;
            }
        }
    }
    info!("prop rigs: {} original rigs loaded", rigs.len());
    originals.prop_rigs = rigs;
}

/// Load the biggest fully-rigged original character (body mesh + skeleton +
/// real animation clip). RW4 bodies are rigid morph models, so the body
/// renders at bind pose while its rig is driven by the file's own clip.
/// Pick the richest decodable original clips (shared by the loader and the
/// `--dump-anim-keys` verification mode).
fn pick_clip_previews(install: &spore_install::SporeInstall, budget: usize) -> Vec<ClipPreview> {
    let mut best: Vec<ClipPreview> = Vec::new();
    let mut scanned = 0usize;
    'outer: for pkg in &install.packages {
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_ANIM)
        {
            scanned += 1;
            if scanned > budget {
                break 'outer;
            }
            let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            let Some(mut clip) = decode::parse_anim_clip(&bytes) else {
                continue;
            };
            let Ok(timed_clip) = animation::parse(&bytes) else {
                continue;
            };
            clip.length = timed_clip.duration_seconds();
            let keys: usize = clip
                .channels
                .iter()
                .map(|c| c.rot.len() + c.pos.len())
                .sum();
            if keys < 8 {
                continue;
            }
            let path = decode::anim_path(&bytes);
            let label = path
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or("clip")
                .trim_end_matches(".animation")
                .to_string();
            best.push(ClipPreview {
                clip,
                timed_clip: Some(std::sync::Arc::new(timed_clip)),
                label,
            });
            best.sort_by_key(|c| std::cmp::Reverse(c.clip.channels.len()));
            best.truncate(8);
        }
    }
    best
}

fn select_clip_preview(originals: &mut Originals, index: usize) -> Result<(), String> {
    let key = *originals
        .animation_catalog
        .clips
        .get(index)
        .ok_or("clip index outside catalog")?;
    let clip = animation_catalog::load_clip(
        &originals.animation_catalog,
        &mut originals.animation_cache,
        &originals.resource_index,
        key,
    )?;
    let channels = clip
        .channels
        .iter()
        .map(|ch| decode::AnimChannel {
            name: ch.name.clone(),
            select_flags: ch.primary.flags,
            capability: ch.primary.capability.clone(),
            pos: ch
                .components
                .iter()
                .find_map(|c| {
                    if let animation::Keys::Position(keys) = &c.keys {
                        Some(keys.iter().map(|k| k.position).collect())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
            rot: ch
                .components
                .iter()
                .find_map(|c| {
                    if let animation::Keys::Rotation(keys) = &c.keys {
                        Some(keys.iter().map(|k| k.rotation).collect())
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
        })
        .collect();
    let legacy = decode::AnimClip {
        length: clip.duration_seconds(),
        channels,
    };
    let label = clip
        .source_path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("clip")
        .trim_end_matches(".animation")
        .to_string();
    originals.clips = vec![ClipPreview {
        clip: legacy,
        timed_clip: Some(clip),
        label,
    }];
    Ok(())
}

/// Read one initial clip on demand; P can browse the entire effective catalog.
fn load_clip_previews(mut originals: ResMut<Originals>) {
    if originals.animation_catalog.clips.is_empty() {
        return;
    }
    if let Err(e) = select_clip_preview(&mut originals, 0) {
        warn!("clip preview: {e}");
    }
    info!(
        "clip catalog: {} original animations available on demand",
        originals.animation_catalog.clips.len()
    );
}

/// Completely separate from the creature rig: a joint per animation channel,
/// driven by that clip's own position/rotation keyframe streams.
#[derive(Component)]
pub struct ClipJoint {
    pub channel: usize,
}

fn spawn_clip_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
) {
    let Some(preview) = originals.clips.first() else {
        return;
    };
    let joint_mesh = meshes.add(Sphere::new(0.09));
    let joint_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.3),
        emissive: Color::srgb(0.5, 0.35, 0.05).into(),
        ..default()
    });
    // Cap the joints for frame cost; the clip's channel order is stable.
    for i in 0..24 {
        let (cap, side) = preview
            .clip
            .channels
            .get(i)
            .map(decode::channel_selector)
            .unwrap_or_default();
        let label = match (cap.as_str(), side.as_str()) {
            ("", "") => format!("ch{i}"),
            (c, "") => c.to_string(),
            ("", s) => s.to_string(),
            (c, s) => format!("{c} {s}"),
        };
        commands.spawn((
            StageOwned,
            ClipJoint { channel: i },
            Name::new(label),
            Mesh3d(joint_mesh.clone()),
            MeshMaterial3d(joint_mat.clone()),
            Transform::from_xyz(-3.0, 1.5, -5.0),
        ));
    }
}

/// Drive the clip preview: each joint follows its channel's position stream,
/// oriented by its rotation stream. Pure sampling per channel.
fn animate_clip_preview(
    time: Res<Time>,
    originals: Res<Originals>,
    pick: Res<ClipPick>,
    options: Res<HudOptions>,
    editor: Res<editor::EditorState>,
    mut joints: Query<(&ClipJoint, &mut Transform, &mut Visibility)>,
) {
    if originals.clips.is_empty() {
        return;
    }
    let preview = &originals.clips[0];
    let duration = preview.clip.length.max(f32::EPSILON);
    let seconds = (time.elapsed_secs() - pick.1).max(0.).rem_euclid(duration);
    let t = seconds / duration;
    let base = Vec3::new(-3.0, 1.2, -5.0);
    let basis = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    for (j, mut tr, mut visibility) in &mut joints {
        let Some(ch) = preview.clip.channels.get(j.channel) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *visibility = if options.diagnostics && !editor.open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        tr.translation = base;
        tr.rotation = Quat::IDENTITY;
        let timed = preview.timed_clip.as_ref().and_then(|clip| {
            let frame = clip.frame_at_seconds(seconds).ok()?;
            animation_sampling::sample(
                clip.channels.get(j.channel)?,
                frame,
                clip.length_frames,
                clip.flags & 8 != 0,
                animation_sampling::RotationCurve::Spherical,
            )
            .ok()
        });
        let position = if preview.timed_clip.is_some() {
            timed.as_ref().and_then(|s| s.position.map(|p| p.0))
        } else {
            decode::sample_vec3(&ch.pos, t)
        };
        if let Some(p) = position {
            // Positions are in the creature's local frame; scale for display.
            tr.translation = base + basis * Vec3::from_array(p) * 0.5;
        }
        let rotation = if preview.timed_clip.is_some() {
            timed.as_ref().and_then(|s| s.rotation.map(|q| q.0))
        } else {
            decode::sample_quat(&ch.rot, t)
        };
        if let Some(q) = rotation {
            tr.rotation = basis * Quat::from_array(q) * basis.inverse();
        }
    }
}

/// `P` cycles the previewed original animation.
fn clip_pick_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut originals: ResMut<Originals>,
    mut pick: ResMut<ClipPick>,
    state: Res<State<GameStage>>,
    time: Res<Time>,
) {
    let count = originals.animation_catalog.clips.len();
    if *state.get() != GameStage::MainMenu || count == 0 {
        return;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        let next = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            (pick.0 + count - 1) % count
        } else {
            (pick.0 + 1) % count
        };
        match select_clip_preview(&mut originals, next) {
            Ok(()) => {
                pick.0 = next;
                pick.1 = time.elapsed_secs();
                info!(
                    "clip preview {}/{}: {}",
                    next + 1,
                    count,
                    originals.clips[0].label
                );
            }
            Err(e) => warn!("clip preview: {e}"),
        }
    }
}

fn load_original_rigged(mut originals: ResMut<Originals>, mut meshes: ResMut<Assets<Mesh>>) {
    let Ok(install) = spore_install::discover() else {
        return;
    };
    for pkg in &install.packages {
        let Ok(index) = dbpf::read_package_index(pkg) else {
            continue;
        };
        for e in index
            .entries
            .iter()
            .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL && e.key.instance_id == 0xC4F9_6704)
        {
            // The verified template is ~3.9 MB including its embedded clips.
            if e.mem_size > 16_000_000 {
                continue;
            }
            let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                continue;
            };
            let Some(model) = rw4::extract_rigged_model(&blob) else {
                continue;
            };
            // Want a big rig with at least one real clip and a renderable mesh.
            if model.bones.len() < 24 || model.anims.is_empty() {
                continue;
            }
            if let Some((locals, invs)) = rw4::bind_locals(&model.bones, &model.poses) {
                if let Some(pair) = rw4::pair_clip(&model.bones, &locals, &model.anims) {
                    let em = model
                        .meshes
                        .iter()
                        .filter(|m| m.positions.len() >= 2000 && m.positions.len() <= 30000)
                        .max_by_key(|m| m.positions.len());
                    let Some(em) = em else { continue };
                    let mut m = Mesh::new(
                        PrimitiveTopology::TriangleList,
                        RenderAssetUsages::RENDER_WORLD,
                    );
                    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, em.positions.clone());
                    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, em.normals.clone());
                    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, em.uvs.clone());
                    m.insert_indices(Indices::U16(em.indices.clone()));
                    let scale = (2.5 / em.bbox_diagonal().max(1e-6)).clamp(0.001, 100.0);
                    let demo = RiggedDemo {
                        instance: e.key.instance_id,
                        mesh: meshes.add(m),
                        bones: model.bones.len(),
                        parents: model.bones.iter().map(|b| b.parent).collect(),
                        locals,
                        invs,
                        clip: pair.clip,
                        chan_for_bone: pair.chan_for_bone,
                        bind_t: pair.bind_t,
                        key0_loc: pair.key0_loc,
                        bone_names: model.bones.iter().map(|b| b.name).collect(),
                        scale,
                    };
                    // This retail template has validated geometry, bind poses,
                    // and matching clips. Bone count alone also selects huge
                    // non-creature rigs, which cannot be used as an avatar.
                    originals.rigged_demo = Some(demo);
                    return;
                }
            }
        }
    }
    warn!(
        "Verified original creature template 0xC4F96704 unavailable; using the procedural avatar"
    );
}

/// Default the saved slot bone to the rig's topmost (head) bone once, so a
/// fresh save mounts parts on the head like the original's default creature.
fn default_slot_bone(originals: Res<Originals>, mut run: ResMut<RunProgress>) {
    if run.accessory_bone != 0 || run.accessory_picked {
        return;
    }
    let Some(demo) = originals.rigged_demo.as_ref() else {
        return;
    };
    let Some(worlds) = rw4::fk_worlds(&demo.parents, &demo.locals) else {
        return;
    };
    let mut best_bone = 0usize;
    let mut best_y = f32::MIN;
    for (bi, w) in worlds.iter().enumerate() {
        if w[1][3] > best_y {
            best_y = w[1][3];
            best_bone = bi;
        }
    }
    run.accessory_bone = best_bone as u32;
    run.accessory_picked = true;
    run.request_save();
}

fn spawn_skinned_demo(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    originals: Res<Originals>,
) {
    let Some(demo) = originals.skinned_demo.as_ref() else {
        return;
    };
    commands.spawn((
        StageOwned,
        Mesh3d(demo.mesh.clone()),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.5, 0.7, 0.9),
            perceptual_roughness: 0.5,
            ..default()
        })),
        Transform::from_xyz(0.0, 1.5, -1.0).with_scale(Vec3::splat(demo.scale)),
        SkinAnim {
            base: demo.base.clone(),
            idx: demo.idx.clone(),
            wts: demo.wts.clone(),
            locals: demo.locals.clone(),
            invs: demo.invs.clone(),
            parents: demo.parents.clone(),
            wave_bone: demo.wave_bone,
            clip: demo.clip.clone(),
            chan_for_bone: demo.chan_for_bone.clone(),
            bind_t: demo.bind_t.clone(),
            key0_loc: demo.key0_loc.clone(),
        },
    ));
}

/// Drive the skinned demo: real clip channels mapped by bone name when
/// present (rotation replaced, translation delta-applied over bind),
/// sine-wave fallback on the wave bone otherwise.
fn animate_skin(time: Res<Time>, mut meshes: ResMut<Assets<Mesh>>, q: Query<(&Mesh3d, &SkinAnim)>) {
    let t = time.elapsed_secs();
    for (mesh3d, anim) in &q {
        if anim.base.is_empty() || anim.locals.is_empty() {
            continue;
        }
        let mut locals = anim.locals.clone();
        if let Some(clip) = anim.clip.as_ref() {
            if clip.length > 0.0 {
                let ct = t.rem_euclid(clip.length);
                for (bi, slot) in anim.chan_for_bone.iter().enumerate() {
                    if bi >= locals.len() {
                        break;
                    }
                    if let Some(ci) = slot {
                        if let Some((q, loc)) = clip.sample(*ci, ct) {
                            let r = rw4::quat_to_mat33(q);
                            for rr in 0..3 {
                                for cc in 0..3 {
                                    locals[bi][rr][cc] = r[rr][cc];
                                }
                            }
                            let bt = anim.bind_t.get(bi).copied().unwrap_or([0.0; 3]);
                            let k0 = anim.key0_loc.get(bi).copied().unwrap_or([0.0; 3]);
                            locals[bi][0][3] = bt[0] + (loc[0] - k0[0]);
                            locals[bi][1][3] = bt[1] + (loc[1] - k0[1]);
                            locals[bi][2][3] = bt[2] + (loc[2] - k0[2]);
                        }
                    }
                }
            }
        } else if anim.wave_bone < locals.len() {
            locals[anim.wave_bone] =
                rw4::mat34_mul(&locals[anim.wave_bone], &rw4::rot_y((t * 1.4).sin() * 0.7));
        }
        let Some(worlds) = rw4::fk_worlds(&anim.parents, &locals) else {
            continue;
        };
        let Some(out) = rw4::skin_positions(&anim.base, &anim.idx, &anim.wts, &worlds, &anim.invs)
        else {
            continue;
        };
        if let Some(mut mesh) = meshes.get_mut(&mesh3d.0) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Base scene (camera/light/UI owned globally, not per-stage)
// ---------------------------------------------------------------------------

fn load_gameplay_originals(mut originals: ResMut<Originals>, mut images: ResMut<Assets<Image>>) {
    originals.ui_art = ui_art::load(&originals.resource_index, &mut images);
    originals.editor_art = editor_art::load(&originals.resource_index, &mut images);
    originals.creature_abilities =
        creature_actions::CreatureAbilityCatalog::from_index(&originals.resource_index);
    info!(
        "original Creature ability catalog: {}",
        originals.creature_abilities.audit()
    );
}

fn creature_lifecycle_controls(
    state: Res<State<GameStage>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<RunProgress>,
    mut editor: ResMut<editor::EditorState>,
    mut encounter: ResMut<creature_actions::Encounter>,
    mut awards: ResMut<creature_social::CreatureAwards>,
    player: Query<(&Transform, &Health), With<Player>>,
) {
    if *state.get() != GameStage::Creature {
        return;
    }
    if run.creature_lifecycle.mating && !editor.open && !cinematics::laying_egg() {
        if run.creature_lifecycle.finish_mating() {
            let gen = run.creature_lifecycle.generation;
            encounter.message = format!(
                "Generation {gen} hatched at your home nest! Species evolved."
            );
            awards.push(format!("Generation {gen} Egg Hatched! Species Evolved"));
            run.request_save();
        }
    }
    if editor.open {
        return;
    }
    let Ok((pose, health)) = player.single() else {
        return;
    };
    if health.dead() {
        return;
    }
    let threatened =
        encounter.mode == creature_actions::EncounterMode::Combat && encounter.target.is_some();
    if keys.just_pressed(KeyCode::KeyH) {
        encounter.message = match run
            .creature_lifecycle
            .start_rest(pose.translation.to_array(), threatened)
        {
            Ok(()) => "Resting at home; move to stand up".into(),
            Err(reason) => reason.into(),
        };
    }
    // Backspace is the retail "Call your mate" key (manual, Creature controls).
    if keys.just_pressed(KeyCode::KeyM) || keys.just_pressed(KeyCode::KeyC) || keys.just_pressed(KeyCode::Backspace) {
        if run.creature_lifecycle.at_home(pose.translation.to_array()) {
            encounter.message = match run.creature_lifecycle.begin_mating(
                pose.translation.to_array(),
                threatened,
                health.frac(),
            ) {
                Ok(()) => {
                    // Retail plays CRG_LaidEgg first; its kEnterCreatureEditor
                    // message (cinematics::moments) opens the editor.
                    if !cinematics::request_laid_egg() {
                        editor.show();
                        editor.visuals_dirty = true;
                    }
                    run.request_save();
                    awards.push("Mating courtship complete: evolve your species");
                    "Choose your next generation; F2 returns to the nest".into()
                }
                Err(reason) => reason.into(),
            };
        } else {
            encounter.message = match run.creature_lifecycle.emit_mating_call(threatened) {
                Ok(()) => {
                    awards.push("Mating call emitted! Mate answered: return to nest");
                    "Mate answered the call! Follow the call back to your nest to mate and evolve".into()
                }
                Err(reason) => reason.into(),
            };
        }
    }
}

fn setup(mut commands: Commands, run: Res<RunProgress>) {
    commands.spawn((
        Camera3d::default(),
        GameCamera,
        AmbientLight {
            color: Color::srgb(0.86, 0.93, 1.),
            brightness: 350.,
            ..default()
        },
        DistanceFog {
            color: Color::srgb(0.55, 0.72, 0.84),
            falloff: FogFalloff::Linear {
                start: 35.,
                end: 100.,
            },
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 14.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        GameSun,
        DirectionalLight {
            illuminance: 3000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(6.0, 12.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Text::new(format!(
            "OpenSpore booting… save: {:?}",
            RunProgress::save_path()
        )),
        TextFont {
            font_size: bevy::text::FontSize::Px(18.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.04, 0.07, 0.82)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            left: Val::Px(12.0),
            right: Val::Px(12.0),
            padding: UiRect::all(Val::Px(10.0)),
            ..default()
        },
        StatusText,
    ));
    let _ = run.current_stage();
    // Original music, ambience and effects play through game_audio.
}

fn despawn_stage(
    mut commands: Commands,
    owned: Query<Entity, (With<StageOwned>, Without<ChildOf>)>,
) {
    commands.remove_resource::<terrain::LandSurface>();
    for e in &owned {
        commands.entity(e).despawn();
    }
}

// Decorative stage plinth so every stage reads clearly even before gameplay spawns.
// Uses an original Spore texture when one has been uploaded.
fn build_plinth(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    originals: &Originals,
    stage: &GameStage,
) {
    let accent = stage.accent();
    let material = match originals.textures.first() {
        Some(handle) => StandardMaterial {
            base_color_texture: Some(handle.clone()),
            emissive: accent.into(),
            ..default()
        },
        None => StandardMaterial {
            base_color: Color::WHITE,
            emissive: accent.into(),
            ..default()
        },
    };
    commands.spawn((
        StageOwned,
        Spin { speed: 0.5 },
        Mesh3d(meshes.add(Torus::new(2.2, 0.15))),
        MeshMaterial3d(materials.add(material)),
        Transform::from_xyz(0.0, 3.2, -8.0),
    ));
}

macro_rules! menu_enter {
    ($fn_name:ident, $variant:expr) => {
        fn $fn_name(
            mut commands: Commands,
            mut meshes: ResMut<Assets<Mesh>>,
            mut materials: ResMut<Assets<StandardMaterial>>,
            mut clear: ResMut<ClearColor>,
            originals: Res<Originals>,
        ) {
            clear.0 = $variant.accent();
            build_plinth(
                &mut commands,
                &mut meshes,
                &mut materials,
                &originals,
                &$variant,
            );
            info!("entered stage: {}", $variant.title());
        }
    };
}
menu_enter!(enter_menu, GameStage::MainMenu);

// ---------------------------------------------------------------------------
// Global systems
// ---------------------------------------------------------------------------

fn menu_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameStage>>,
    mut run: ResMut<RunProgress>,
    state: Res<State<GameStage>>,
    (mut diet, originals, front): (ResMut<cell::CellIntro>, Res<Originals>, Option<Res<front_end::FrontEnd>>),
) {
    // The new-game planet name field has the keyboard.
    if front.is_some_and(|f| f.typing()) {
        return;
    }
    // Debug jump keys are always available. They are Ctrl+Alt chords, as the
    // original's debug keys are ("control alt o"): plain Ctrl + digit now
    // defines a vehicle group in the Civilization stage (the manual), and the
    // digit 0 is the superweapon key there.
    let debug_chord = (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
        && (keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight));
    let jump = if keys.just_pressed(KeyCode::Digit0) && debug_chord {
        Some(GameStage::MainMenu)
    } else if keys.just_pressed(KeyCode::Digit1) && debug_chord {
        Some(GameStage::Cell)
    } else if keys.just_pressed(KeyCode::Digit2) && debug_chord {
        Some(GameStage::Creature)
    } else if keys.just_pressed(KeyCode::Digit3) && debug_chord {
        Some(GameStage::Tribal)
    } else if keys.just_pressed(KeyCode::Digit4) && debug_chord {
        Some(GameStage::Civilization)
    } else if keys.just_pressed(KeyCode::Digit5) && debug_chord {
        Some(GameStage::Space)
    } else {
        None
    };
    if let Some(stage) = jump {
        if stage == GameStage::MainMenu {
            run.request_save();
        } else if stage != *state.get() {
            run.reset_to(stage.clone());
        }
        next.set(stage);
        return;
    }
    if *state.get() == GameStage::MainMenu {
        if diet.choosing {
            // The new-game diet step is on screen (cell::cell_intro).
        } else if keys.just_pressed(KeyCode::Enter) {
            *run = run.new_lineage();
            run.request_save();
            // Retail ChooseDiet is a new-game step before the Cell stage.
            if originals.part_catalog.parts.is_empty() {
                next.set(GameStage::Cell);
            } else {
                diet.choosing = true;
            }
        } else if keys.just_pressed(KeyCode::KeyL) {
            let loaded = RunProgress::load();
            let stage = if loaded.finished {
                GameStage::MainMenu
            } else {
                loaded.current_stage()
            };
            *run = loaded;
            next.set(stage);
        }
    }
}

fn camera_follow(
    state: Res<State<GameStage>>,
    player: Query<&Transform, (With<Player>, Without<Camera3d>)>,
    mut cam: Query<&mut Transform, (With<crate::GameCamera>, Without<Player>)>,
    time: Res<Time>,
    editor: Res<editor::EditorState>,
    run: Res<RunProgress>,
    mut orbit: ResMut<play_camera::PlayCamera>,
    surface: Option<Res<terrain::LandSurface>>,
    capture: Option<Res<runtime::RenderSmokeCamera>>,
) {
    let Ok(mut ct) = cam.single_mut() else { return };
    if let Some(capture) = capture {
        *ct = capture.0;
        return;
    }
    // Main menu: the galaxy screen's resting camera (`menu_visuals::rest_camera`).
    if !editor.open && *state.get() == GameStage::MainMenu {
        let rest = menu_visuals::rest_camera();
        ct.translation = ct.translation.lerp(rest.translation, (5.0 * time.delta_secs()).min(1.0));
        *ct = ct.looking_at(menu_visuals::GALAXY_CENTRE + menu_visuals::REST_LOOK, Vec3::Y);
        return;
    }
    // Creature: the retail camera rig (anchor, dead zone, orbit, terrain buffer).
    if !editor.open && *state.get() == GameStage::Creature {
        if let Ok(avatar) = player.single() {
            *ct = orbit.creature_pose(avatar, time.delta_secs(), |x, z| {
                surface.as_ref().map(|surface| surface.height(x, z))
            });
            return;
        }
    }
    let offset = if editor.open {
        editor.camera_offset()
    } else {
        match state.get() {
            GameStage::MainMenu => Vec3::new(0.0, 4.0, 10.0),
            // Straight down onto the pool, rising as the cell grows; the
            // slight offset keeps the view's up direction well defined.
            GameStage::Cell => {
                let height = cell::camera_height(run.dna_this_stage);
                Vec3::new(0.0, height, height * 0.06)
            }
            GameStage::Creature | GameStage::Tribal | GameStage::Civilization => orbit.offset(),
            GameStage::Space => Vec3::new(0.0, 20.0, 20.0),
        }
    };
    let target = if let Ok(pt) = player.single() {
        pt.translation + offset
    } else {
        offset
    };
    ct.translation = ct
        .translation
        .lerp(target, (5.0 * time.delta_secs()).min(1.0));
    if !editor.open {
        if let Some(surface) = surface {
            ct.translation.y = ct
                .translation
                .y
                .max(surface.height(ct.translation.x, ct.translation.z) + 0.8);
        }
    }
    let mut look = player.single().map(|p| p.translation).unwrap_or(Vec3::ZERO);
    if !editor.open && *state.get() == GameStage::Creature {
        // Follow the torso so the player sits below the horizon and the
        // original sky/landscape stay visible at the default zoom.
        look.y += 0.65;
    }
    if editor.open {
        let right = (-offset).cross(Vec3::Y).normalize_or(Vec3::X);
        look += right * 0.8;
    }
    *ct = ct.looking_at(look, Vec3::Y);
}

fn spin_props(time: Res<Time>, mut q: Query<(&mut Transform, &Spin)>) {
    for (mut t, s) in &mut q {
        t.rotate_y(s.speed * time.delta_secs());
    }
}

/// Creature-editor stub: press C to randomize your current avatar's color.
fn editor_stub(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameStage>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    player_mat: Query<(Entity, Option<&MeshMaterial3d<StandardMaterial>>), With<Player>>,
    original_parts: Query<
        (Entity, &MeshMaterial3d<StandardMaterial>),
        With<creation_assets::OriginalCreationPart>,
    >,
    parents: Query<&ChildOf>,
) {
    if *state.get() == GameStage::Space || !keys.just_pressed(KeyCode::KeyC) {
        return;
    }
    use rand::Rng;
    let mut rng = rand::rng();
    let Ok((player, handle)) = player_mat.single() else {
        return;
    };
    let color = Color::srgb(rng.random(), rng.random(), rng.random());
    if let Some(handle) = handle {
        if let Some(mut m) = mats.get_mut(&handle.0) {
            m.base_color = color;
        }
    }
    for (entity, handle) in &original_parts {
        let mut ancestor = entity;
        for _ in 0..64 {
            let Ok(parent) = parents.get(ancestor) else {
                break;
            };
            ancestor = parent.parent();
            if ancestor == player {
                if let Some(mut material) = mats.get_mut(&handle.0) {
                    material.base_color = color;
                }
                break;
            }
        }
    }
    info!("editor stub: recolored avatar (full editors roadmap in README)");
}

fn update_hud(
    state: Res<State<GameStage>>,
    encounter: Res<creature_actions::Encounter>,
    actors: Query<(
        &crate::persistence::WorldId,
        &creature_actions::CreatureMind,
    )>,
    install: Res<InstallStatus>,
    catalog: Res<Catalog>,
    (originals, editor, cell_weapon): (
        Res<Originals>,
        Res<editor::EditorState>,
        Res<cell::CellWeapon>,
    ),
    run: Res<RunProgress>,
    clip_pick: Res<ClipPick>,
    saves: Res<persistence::SaveState>,
    options: Res<HudOptions>,
    player: Query<(&Health, &Hunger), With<Player>>,
    player_hp_only: Query<&Health, (With<Player>, Without<Hunger>)>,
    mates: Query<&game::TribeMate>,
    raiders: Query<&tribal::Raider>,
    mut q: Query<(&mut Text, &mut Node), With<StatusText>>,
) {
    let stage = state.get();
    let mut s = String::with_capacity(1536);
    s.push_str(concat!(
        "OPENSPORE ",
        env!("CARGO_PKG_VERSION"),
        " | playable prototype with original assets\n"
    ));
    // Objective line per stage.
    let objective = match stage {
        GameStage::MainMenu => {
            if run.finished {
                format!(
                    "CAMPAIGN COMPLETE! Total DNA {:.0} | Enter=new run",
                    run.dna_banked_total
                )
            } else {
                format!(
                    "Enter=new run  L=continue (stage {:?}, DNA {:.0}) | F2 creature editor | Ctrl+Alt+1-5 debug jump",
                    run.current_stage(),
                    run.dna_banked_total
                )
            }
        }
        GameStage::Cell => format!(
            "CELL: eat food permitted by your mouth. DNA {:.0}/{:.0}  {} | growth {}/5 ({:.2}x) | electricity cooldown {:.1}s",
            run.dna_this_stage,
            game::tuning_for(stage).dna_to_advance,
            diet_line(&run),
            cell::growth_tier(run.dna_this_stage) + 1,
            cell::growth_scale(run.dna_this_stage),
            cell_weapon.electric_cooldown,
        ),
        GameStage::Creature => {
            let parts_line = if run.parts.is_empty() {
                "no parts".to_string()
            } else {
                let fi = game::focus_index(run.focused_part, run.parts.len()) as usize;
                format!(
                    "{} part(s) [{}] slot {}",
                    run.parts.len(),
                    fi,
                    run.parts[fi].bone
                )
            };
            let st = creature::stats_for(&run, &originals);
            let world = &run.species_world;
            let allied = world.species.iter().filter(|s| world.species_allied(s.id)).count();
            let extinct = world.species.iter().filter(|s| world.species_extinct(s.id)).count();
            format!(
                "CREATURE: E social / F combat, then 1-4 abilities | F2 editor. DNA {:.0}/{:.0}  {}\n{} | speed {:.1} health {:.0} social {:.2} size {:.2}\n{} species, {} nests | alliances {} | extinct {}",
                run.dna_this_stage,
                game::tuning_for(stage).dna_to_advance,
                diet_line(&run),
                parts_line,
                st.speed,
                st.max_hp,
                st.social,
                st.size,
                world.species.len(),
                world.nests.len(),
                allied,
                extinct,
            )
        }
        GameStage::Tribal => format!(
            "{}\nFood {:.0} | tribe: {} members | {} raiders\n{}",
            run.tribal_campaign.hud(), run.dna_this_stage, mates.iter().len(), raiders.iter().len(),
            tribal::tribal_editor_hud(&run, &originals)
        ),
        GameStage::Civilization => format!(
            "{}\nSpice {:.0} | engine {} (E: {:.0})\n{}",
            run.civ_campaign.hud(), run.spice, run.engine,
            game::engine_cost(run.engine), civ::civ_editor_hud(&run, &originals)
        ),
        GameStage::Space => format!(
            "{}\nSporebucks {:.0} | health {} | laser {}\n{}",
            run.space_campaign.hud(), run.spice,
            crate::space_rules::health_level(&run.space_campaign.tools),
            crate::space_rules::laser_level(&run.space_campaign.tools),
            space::ship_editor_hud(&run, &originals)
        ),
    };
    s.push_str(&objective);
    s.push('\n');
    if let Ok((hp, hunger)) = player.single() {
        s.push_str(&format!(
            "HP {:.0}%  Food {:.0}%   ",
            hp.frac() * 100.0,
            hunger.frac() * 100.0
        ));
    } else if let Ok(hp) = player_hp_only.single() {
        s.push_str(&format!("Hull {:.0}%   ", hp.frac() * 100.0));
    }
    s.push('\n');
    if *stage == GameStage::MainMenu {
        s.push_str(&format!(
            "Creature editor: {} parts | F2 open original part palette\n",
            run.parts.len()
        ));
    }
    if *stage == GameStage::Creature {
        s.push_str(&creature_actions::hud(
            &encounter,
            &run,
            &originals,
            actors.iter().map(|(id, m)| (id.0, m.clone())),
        ));
        s.push('\n');
        s.push_str(&creature_pack::hud(
            &run.creature_pack,
            run.earned_creature_dna(),
        ));
        s.push('\n');
    }
    if !install.ok {
        s.push_str("Spore install not found. Start OpenSpore with --choose-spore (or --spore-path) to select your installation.\n");
    }
    if options.diagnostics {
        s.push_str(&format!(
        "Assets: {} pkgs, {} records | {} | originals: {} PNGs, {} prop-str, {} locale-str, {} GPU tex, {} models ({} verts), {} anims{}",
        catalog.packages.len(),
        catalog.total_entries,
        install.detail,
        originals.png_count,
        originals.string_count,
        originals.locale_strings,
        originals.textures.len(),
        originals.models.len(),
        originals.model_verts,
        originals.anim_total,
        originals
            .skeleton
            .as_ref()
            .map(|v| format!(" | skeleton {} bones", v.bones))
            .unwrap_or_default(),
    ));
        if let Some(demo) = originals.rigged_demo.as_ref() {
            s.push_str(&format!(
                " | rigged character {} bones + {:.1}s original clip",
                demo.bones, demo.clip.length
            ));
        }
        if !originals.clips.is_empty() {
            s.push_str(&format!(
                " | clip preview {}/{} ({}, {} channels, P/Shift+P cycles)",
                clip_pick.0 + 1,
                originals.animation_catalog.clips.len(),
                originals.clips[0].label,
                originals.clips[0].clip.channels.len()
            ));
        }
        if let Some(demo) = originals.morph_demo.as_ref() {
            s.push_str(&format!(" | morphing {} shapes", demo.shapes));
        }
        if let Some(demo) = originals.skinned_demo.as_ref() {
            s.push_str(&format!(" | skinning {} bones", demo.bones));
        }
        if let Some(demo) = originals.rigged_demo.as_ref() {
            s.push_str(&format!(
                " | rigged character {} bones + {:.1}s original clip",
                demo.bones, demo.clip.length
            ));
            if *stage == GameStage::MainMenu && !run.parts.is_empty() {
                let fi = game::focus_index(run.focused_part, run.parts.len()) as usize;
                let p = &run.parts[fi];
                let part = originals
                    .models
                    .iter()
                    .find(|m| m.instance == p.instance)
                    .map(|m| format!("{:#X}", m.instance))
                    .unwrap_or_else(|| "none".to_string());
                let slot = originals
                    .rigged_demo
                    .as_ref()
                    .and_then(|d| d.bone_names.get(p.bone as usize).copied())
                    .and_then(game::known_bone_name)
                    .map(|n| format!("{n} (#{})", p.bone))
                    .unwrap_or_else(|| format!("#{}", p.bone));
                s.push_str(&format!(
                " | assembly: {} part(s), [{}] {} on {} yaw {:.0}° scale {:.1} slide ({:.1},{:.1},{:.1})",
                run.parts.len(),
                fi,
                part,
                slot,
                p.yaw.to_degrees(),
                p.scale,
                p.offset[0],
                p.offset[1],
                p.offset[2]
            ));
            }
        }
        if let Some(a) = originals.audio_file.as_ref() {
            s.push_str(&format!(" | audio {a}"));
        }
        if let Some(m) = originals.music_file.as_ref() {
            s.push_str(&format!(" | music {m}"));
        }
        if let Some(icon) = originals.icon_file.as_ref() {
            s.push_str(&format!(" | icon {icon}"));
        }
        s.push('\n');
    }
    if editor.open {
        s.push_str("CREATOR PAUSED | F2 return | right-drag orbit | wheel zoom/scroll\n");
    } else {
        s.push_str(match stage {
            GameStage::MainMenu => {
                "Enter new lineage | L continue | F2 creature creator | F3 diagnostics\n"
            }
            GameStage::Cell => {
                "WASD swim | F2 creator | Space electricity | Esc menu | F5 save | F3 diagnostics\n"
            }
            GameStage::Space => {
                "Click or right-click to fly | wheel + - zoom, End Home levels | A D rotate, W S tilt | V survey | T terraform | C colonize | M missions | F shoot | Esc menu | F5 save\n"
            }
            GameStage::Creature => {
                "WASD move | F2 creator | C recolor | Esc menu | F5 save | F3 diagnostics\n"
            }
            GameStage::Tribal => "WASD pan | Space follow | right-click orders | 1-5 respond | G gift | hold F attack | Esc deselect/menu | F5 save\n",
            GameStage::Civilization => "WASD drive | 1 military / 2 religious / 3 economic | hold F act | B build | E engine | V vehicle/building editor | Esc menu | F5 save\n",
        });
    }
    s.push_str(&saves.status);
    if !options.diagnostics {
        let health = player
            .single()
            .ok()
            .map(|(hp, hunger)| {
                format!(
                    "    Health {:.0}/{:.0}    Hunger {:.0}",
                    hp.hp, hp.max, hunger.food
                )
            })
            .unwrap_or_default();
        s = if editor.open {
            format!("Creature creator    DNA {:.0}\nF2 return    Right-drag orbit    Wheel zoom    Drag parts to place",run.editor_dna)
        } else {
            match stage {
            GameStage::Creature=>format!("Creature    DNA {:.0}/{:.0}{health}\nE socialize    F attack    1–4 abilities    F2 create    Esc menu",run.dna_this_stage,game::tuning_for(stage).dna_to_advance),
            GameStage::MainMenu=>format!("OpenSpore\nEnter — New game    L — Continue    F2 — Creature creator"),
            _=>format!("{}{health}\n{}",stage.title(),objective),
        }
        };
    }
    for (mut text, mut node) in &mut q {
        node.display = if options.diagnostics || !install.ok {
            Display::Flex
        } else {
            Display::None
        };
        if text.0 != s {
            text.0 = s.clone();
        }
    }
}

fn diet_line(run: &RunProgress) -> String {
    format!(
        "herb {:.0} carn {:.0}",
        run.diet_herbivore, run.diet_carnivore
    )
}

// ---------------------------------------------------------------------------
// Headless modes (CI / verification without opening a window)
// ---------------------------------------------------------------------------

fn scan_only_mode() -> bool {
    std::env::args().any(|a| a == "--scan-only")
}

fn stats_mode() -> bool {
    std::env::args().any(|a| a == "--stats")
}

fn run_scan_only() {
    match spore_install::discover() {
        Ok(install) => {
            println!("Spore install: {}", install.root.display());
            let mut total = 0usize;
            for pkg in &install.packages {
                let name = pkg
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| pkg.display().to_string());
                match dbpf::read_package_index(pkg) {
                    Ok(index) => {
                        println!(
                            "  {name}: v{}.{} entries={} index_size={}",
                            index.header.major,
                            index.header.minor,
                            index.entries.len(),
                            index.header.index_size
                        );
                        total += index.entries.len();
                        if let Some(e) = index.entries.iter().find(|e| e.is_compressed) {
                            match dbpf::read_entry_bytes(pkg, e) {
                                Ok(b) => {
                                    println!(
                                        "    sample decompress ok: type={:#X} bytes={}",
                                        e.key.type_id,
                                        b.len()
                                    );
                                    if e.key.type_id == decode::TYPE_RASTER {
                                        println!(
                                            "    raster: {:?}",
                                            decode::parse_raster_header(&b)
                                        );
                                    }
                                }
                                Err(err) => println!("    sample decompress FAILED: {err}"),
                            }
                        }
                    }
                    Err(e) => println!("  {name}: FAILED: {e}"),
                }
            }
            println!("TOTAL records: {total}");
            if stats_mode() {
                dump_type_histogram(&install);
            }
        }
        Err(e) => {
            println!("SCAN FAILED: {e}");
            std::process::exit(2);
        }
    }
}

fn probe_args() -> Option<(u32, usize)> {
    let args: Vec<String> = std::env::args().collect();
    let mut ty: Option<u32> = None;
    let mut n: usize = 5;
    for i in 0..args.len() {
        if args[i] == "--probe-type" && i + 1 < args.len() {
            let v = args[i + 1]
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            ty = u32::from_str_radix(v, 16).ok();
        } else if let Some(v) = args[i].strip_prefix("--probe-type=") {
            let v = v.trim().trim_start_matches("0x").trim_start_matches("0X");
            ty = u32::from_str_radix(v, 16).ok();
        } else if args[i] == "--probe-n" && i + 1 < args.len() {
            n = args[i + 1].parse().unwrap_or(5).min(50);
        }
    }
    ty.map(|t| (t, n))
}

fn guess_signature(b: &[u8]) -> &'static str {
    if b.len() >= 8 && &b[0..8] == b"\x89PNG\r\n\x1a\n" {
        return "PNG image";
    }
    if b.len() >= 4 && &b[0..4] == b"DDS " {
        return "DDS texture";
    }
    if b.len() >= 4 && &b[0..4] == b"OggS" {
        return "Ogg audio";
    }
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        return "WAV audio";
    }
    if b.len() >= 3 && &b[0..3] == b"ID3" {
        return "MP3 (ID3) audio";
    }
    if b.len() >= 2 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0 {
        return "MP3-frame(?) audio";
    }
    if b.len() >= 4 && &b[0..4] == b"\x00\x01\x00\x00" {
        return "ICO/CFX(?) 00010000";
    }
    // XML-ish: first non-ws byte is '<'
    let mut ws = 0;
    while ws < b.len().min(64)
        && (b[ws] == b' ' || b[ws] == b'\t' || b[ws] == b'\r' || b[ws] == b'\n' || b[ws] == 0)
    {
        ws += 1;
    }
    if ws < b.len() && b[ws] == b'<' {
        return "XML-ish text";
    }
    // UTF-16LE text heuristic: alternating zeros + printable ASCII.
    if b.len() >= 32 {
        let mut zeros = 0;
        let mut print = 0;
        for (i, &c) in b.iter().enumerate().take(128) {
            if i % 2 == 1 && c == 0 {
                zeros += 1;
            }
            if i % 2 == 0 && (c == b'\n' || c == b'\r' || c == b'\t' || (0x20..0x7F).contains(&c)) {
                print += 1;
            }
        }
        if zeros >= 40 && print >= 40 {
            return "UTF-16LE text(?)";
        }
    }
    // Plain ASCII text heuristic.
    if b.len() >= 16 {
        let sample = &b[..b.len().min(256)];
        let printable = sample
            .iter()
            .filter(|&&c| c == b'\n' || c == b'\r' || c == b'\t' || (0x20..0x7F).contains(&c))
            .count();
        if printable * 100 / sample.len() > 85 {
            return "ASCII text(?)";
        }
    }
    "binary/unknown"
}

fn shannon_entropy(b: &[u8]) -> f64 {
    let n = b.len().min(4096);
    if n == 0 {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &c in &b[..n] {
        counts[c as usize] += 1;
    }
    let mut h = 0.0;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / n as f64;
            h -= p * p.log2();
        }
    }
    h
}

fn ascii_preview(b: &[u8], len: usize) -> String {
    b.iter()
        .take(len)
        .map(|&c| {
            if c == b'\n' {
                '\n'
            } else if (0x20..0x7F).contains(&c) {
                c as char
            } else {
                '.'
            }
        })
        .collect()
}

fn hex_preview(b: &[u8], len: usize) -> String {
    b.iter()
        .take(len)
        .map(|c| format!("{c:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn run_probe(type_id: u32, n: usize) {
    match spore_install::discover() {
        Ok(install) => {
            println!("PROBE type={:#010X} (up to {n} samples)", type_id);
            let mut shown = 0;
            let mut sizes: Vec<usize> = Vec::new();
            let mut total_of_type = 0;
            for pkg in &install.packages {
                let pname = pkg
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                let matches: Vec<_> = index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == type_id)
                    .collect();
                if !matches.is_empty() {
                    println!("  {pname}: {} records of this type", matches.len());
                    total_of_type += matches.len();
                }
                for e in matches {
                    sizes.push(e.mem_size as usize);
                    if shown >= n {
                        continue;
                    }
                    shown += 1;
                    print!(
                        "  sample#{shown} {pname} g={:#X} i={:#X} disk={} mem={} comp={} : ",
                        e.key.group_id, e.key.instance_id, e.disk_size, e.mem_size, e.is_compressed
                    );
                    match dbpf::read_entry_bytes(pkg, e) {
                        Ok(b) => {
                            println!(
                                "OK bytes={} sig=[{}] entropy={:.2}",
                                b.len(),
                                guess_signature(&b),
                                shannon_entropy(&b)
                            );
                            if type_id == decode::TYPE_RASTER {
                                println!("    raster: {:?}", decode::parse_raster_header(&b));
                            }
                            println!("    hex: {}", hex_preview(&b, 48));
                            println!("    txt: {}", ascii_preview(&b, 120).replace('\n', "\\n"));
                        }
                        Err(err) => println!("READ FAIL: {err}"),
                    }
                }
            }
            println!("TOTAL of type {:#X}: {total_of_type}", type_id);
            if !sizes.is_empty() {
                sizes.sort_unstable();
                let sum: usize = sizes.iter().sum();
                println!(
                    "size min={} p50={} max={} mean={}",
                    sizes[0],
                    sizes[sizes.len() / 2],
                    sizes[sizes.len() - 1],
                    sum / sizes.len()
                );
            }
        }
        Err(e) => {
            println!("PROBE FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: dump original PNGs to disk (default `extracted_pngs/`).
fn run_extract_png() {
    let args: Vec<String> = std::env::args().collect();
    let mut outdir = PathBuf::from("extracted_pngs");
    let mut limit = 16;
    for i in 0..args.len() {
        if args[i] == "--out" && i + 1 < args.len() {
            outdir = PathBuf::from(&args[i + 1]);
        } else if args[i] == "--probe-n" && i + 1 < args.len() {
            limit = args[i + 1].parse().unwrap_or(16).min(200);
        }
    }
    match spore_install::discover() {
        Ok(install) => {
            std::fs::create_dir_all(&outdir).expect("mkdir outdir");
            let mut done = 0;
            for (meta, bytes) in decode::collect_pngs(&install, limit, limit) {
                if let Some(b) = bytes {
                    let fname = format!(
                        "{:08X}_{:08X}_{}x{}.png",
                        meta.group_id, meta.instance_id, meta.width, meta.height
                    );
                    if std::fs::write(outdir.join(&fname), &b).is_ok() {
                        println!("wrote {fname} ({} bytes, from {})", b.len(), meta.package);
                        done += 1;
                    }
                }
            }
            println!("extracted {done} PNGs -> {}", outdir.display());
        }
        Err(e) => {
            println!("EXTRACT FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: dump original UTF-16 strings from PROP records.
fn run_dump_strings() {
    match spore_install::discover() {
        Ok(install) => {
            let mut seen = std::collections::HashSet::new();
            let mut total = 0;
            let mut budget = 200_000;
            for pkg in &install.packages {
                if budget == 0 {
                    break;
                }
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_PROP)
                {
                    if budget == 0 {
                        break;
                    }
                    if e.mem_size > 4_000_000 {
                        continue;
                    }
                    budget -= 1;
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    for s in decode::extract_utf16_strings(&bytes, 5) {
                        let t = s.trim().to_string();
                        if t.len() >= 3 && seen.insert(t.clone()) {
                            total += 1;
                            println!("[{:#X}:{:#X}] {t}", e.key.group_id, e.key.instance_id);
                        }
                    }
                }
            }
            println!("TOTAL distinct strings: {total}");
        }
        Err(e) => {
            println!("DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: resolve gmdl mesh tables to their referenced rig records and
/// report each record's skeleton/anim anatomy — establishes what the game's
/// gmdl "models" actually reference.
fn run_dump_gmdl_rigs() {
    match spore_install::discover() {
        Ok(install) => {
            let mut files = 0usize;
            let mut rig_records = 0usize;
            let mut with_anims = 0usize;
            let mut best: Vec<(usize, usize, u32, String)> = Vec::new(); // bones, anims, inst, pkg
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_GMDL)
                {
                    let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let Some(g) = gmdl::parse_mesh_table(&blob) else {
                        continue;
                    };
                    files += 1;
                    if files > 60 {
                        break;
                    }
                    let pkg_name = pkg
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    for key in &g.table_meshes {
                        for pkg2 in &install.packages {
                            let Ok(idx2) = dbpf::read_package_index(pkg2) else {
                                continue;
                            };
                            let Some(e2) = idx2.entries.iter().find(|d| {
                                d.key.type_id == key.type_id
                                    && d.key.group_id == key.group_id
                                    && d.key.instance_id == key.instance_id
                            }) else {
                                continue;
                            };
                            let Ok(bytes) = dbpf::read_entry_bytes(pkg2, e2) else {
                                continue;
                            };
                            let mut stats = rw4::MeshStats::default();
                            let (skels, _skins) = rw4::extract_rig(&bytes, &mut stats);
                            let anims = rw4::anims_from_package_record(pkg2, e2, &mut stats);
                            if skels.is_empty() {
                                break;
                            }
                            rig_records += 1;
                            if !anims.is_empty() {
                                with_anims += 1;
                            }
                            let bones = skels.iter().map(|s| s.len()).max().unwrap_or(0);
                            best.push((bones, anims.len(), key.instance_id, pkg_name.clone()));
                            break;
                        }
                    }
                }
            }
            best.sort_by(|a, b| b.0.cmp(&a.0));
            println!(
                "gmdl rigs: {files} gmdl files, {rig_records} referenced rig records, {with_anims} with embedded anims"
            );
            for (bones, anims, inst, pkg_name) in best.iter().take(12) {
                println!("  {pkg_name} inst={inst:#X}: {bones} bones, {anims} embedded clips");
            }
        }
        Err(e) => {
            println!("GMDL RIG DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: resolve gmdl mesh tables to their RW4 parts and report each
/// part's mesh statistics — verifies that a gmdl is a composed multi-part
/// model (vehicle/building geometry) rather than a single mesh.
fn run_dump_gmdl_parts() {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            let mut files = 0usize;
            let mut parts = 0usize;
            let mut composed = 0usize;
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_GMDL)
                {
                    let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let Some(g) = gmdl::parse_mesh_table(&blob) else {
                        continue;
                    };
                    files += 1;
                    if files > 12 {
                        break;
                    }
                    let mut line: Vec<String> = Vec::new();
                    let mut all_min = [f32::MAX; 3];
                    let mut all_max = [f32::MIN; 3];
                    for key in &g.table_meshes {
                        // Resolve against every package (first match wins).
                        let mut handled = false;
                        for pkg2 in &install.packages {
                            let Ok(idx2) = dbpf::read_package_index(pkg2) else {
                                continue;
                            };
                            let Some(e2) = idx2.entries.iter().find(|d| {
                                d.key.type_id == key.type_id
                                    && d.key.group_id == key.group_id
                                    && d.key.instance_id == key.instance_id
                            }) else {
                                continue;
                            };
                            for em in rw4::meshes_from_package_record(pkg2, e2, &mut stats) {
                                parts += 1;
                                handled = true;
                                let (lo, hi) = em.bbox();
                                for k in 0..3 {
                                    all_min[k] = all_min[k].min(lo[k]);
                                    all_max[k] = all_max[k].max(hi[k]);
                                }
                                if line.len() < 8 {
                                    line.push(format!(
                                        "{:#X}:{}v/{:.1}",
                                        key.instance_id,
                                        em.positions.len(),
                                        em.bbox_diagonal()
                                    ));
                                }
                            }
                            if handled {
                                break;
                            }
                        }
                    }
                    if all_max[0] > all_min[0] && line.len() >= 2 {
                        composed += 1;
                        println!(
                            "  gmdl {:#X}: {} parts [{}] composed extent {:.1}x{:.1}x{:.1}",
                            e.key.instance_id,
                            line.len(),
                            line.join(" "),
                            all_max[0] - all_min[0],
                            all_max[1] - all_min[1],
                            all_max[2] - all_min[2],
                        );
                    }
                }
            }
            println!("gmdl parts: {files} files, {parts} meshes, {composed} composed models");
        }
        Err(e) => {
            println!("GMDL PARTS DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: print the original extension<->type registry table.
fn run_dump_registry() {
    match spore_install::discover() {
        Ok(install) => {
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_PROP)
                {
                    if e.mem_size > 32768 {
                        continue;
                    }
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let strings = decode::extract_utf16_strings(&bytes, 4);
                    if strings.iter().any(|s| s.contains("resource type mapping")) {
                        println!(
                            "registry record: {} g={:#X} i={:#X} ({} strings)",
                            pkg.file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            e.key.group_id,
                            e.key.instance_id,
                            strings.len()
                        );
                        for (id, name) in decode::parse_extension_map(&strings) {
                            println!("  {id:#010X} = {name}");
                        }
                        break 'outer;
                    }
                }
            }
        }
        Err(e) => {
            println!("REGISTRY DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate raster decoding across capped records.
fn run_dump_rasters() {
    match spore_install::discover() {
        Ok(install) => {
            let mut ok = 0;
            let mut bad = 0;
            let mut scanned = 0;
            let mut dims = std::collections::HashMap::new();
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RASTER)
                {
                    if scanned >= 600 {
                        break 'outer;
                    }
                    scanned += 1;
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        bad += 1;
                        continue;
                    };
                    match decode::decode_raster_base(&bytes) {
                        Some(r) => {
                            ok += 1;
                            *dims
                                .entry((r.width, r.height, r.format.label()))
                                .or_insert(0) += 1;
                            if ok <= 10 {
                                println!(
                                    "  {}x{} {} base={} ({} g={:#X} i={:#X})",
                                    r.width,
                                    r.height,
                                    r.format.label(),
                                    r.data.len(),
                                    pkg.file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_default(),
                                    e.key.group_id,
                                    e.key.instance_id
                                );
                            }
                        }
                        None => {
                            bad += 1;
                            if bad <= 5 {
                                println!(
                                    "  DECODE FAIL mem={} g={:#X} i={:#X}",
                                    e.mem_size, e.key.group_id, e.key.instance_id
                                );
                            }
                        }
                    }
                }
            }
            println!("rasters scanned={scanned} decoded={ok} failed={bad}");
            let mut dims: Vec<_> = dims.into_iter().collect();
            dims.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            for ((w, h, f), c) in dims.into_iter().take(15) {
                println!("  {w}x{h} {f}: {c}");
            }
        }
        Err(e) => {
            println!("RASTER DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate RW4 mesh import across capped records.
fn run_dump_meshes() {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            let mut scanned = 0;
            let mut with_mesh = 0;
            let mut demo_shapes: Option<(usize, usize)> = None;
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
                {
                    if scanned >= 150 {
                        break 'outer;
                    }
                    if e.mem_size > 2_000_000 {
                        continue;
                    }
                    scanned += 1;
                    let before = stats.static_ok + stats.blend_ok;
                    let meshes = rw4::meshes_from_package_record(pkg, e, &mut stats);
                    if stats.static_ok + stats.blend_ok > before {
                        with_mesh += 1;
                        for m in &meshes {
                            if demo_shapes.is_none() && !m.morphs.is_empty() {
                                demo_shapes = Some((m.positions.len(), m.morphs.len()));
                            }
                        }
                        if with_mesh <= 12 {
                            let desc: Vec<String> = meshes
                                .iter()
                                .map(|m| {
                                    format!(
                                        "{}v/{}t/d{:.1}",
                                        m.positions.len(),
                                        m.indices.len() / 3,
                                        m.bbox_diagonal()
                                    )
                                })
                                .collect();
                            println!(
                                "  {} g={:#X} i={:#X}: {}",
                                pkg.file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                e.key.group_id,
                                e.key.instance_id,
                                desc.join(" ")
                            );
                        }
                    }
                }
            }
            println!(
                "rw4 scanned={scanned} files_with_static_mesh={with_mesh} meshes={} static_ok={} blend_ok={} morph_shapes={} verts={} tris={} unresolved={} unsupported={} demo={:?}",
                stats.meshes, stats.static_ok, stats.blend_ok, stats.morph_shapes, stats.verts, stats.tris, stats.skipped_unresolved, stats.skipped_unsupported, demo_shapes
            );
        }
        Err(e) => {
            println!("MESH DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: dump one RW4 record's section/object layout (vertex declarations
/// and all) using the real parser. `--dump-vdecl <instance|name>`.
fn run_dump_vdecl(instance: u32) {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index.entries.iter().filter(|e| {
                    e.key.type_id == decode::TYPE_RW4_MODEL && e.key.instance_id == instance
                }) {
                    let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    println!(
                        "record {:#X} in {}",
                        instance,
                        pkg.file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                    for line in rw4::dump_record_layout(&blob) {
                        println!("  {line}");
                    }
                    let _ = stats;
                    return;
                }
            }
            println!("record {instance:#X} not found");
            std::process::exit(2);
        }
        Err(e) => {
            println!("VDECL DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

fn vdecl_arg() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--dump-vdecl" && i + 1 < args.len() {
            let v = args[i + 1]
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            return u32::from_str_radix(v, 16).ok();
        }
    }
    None
}

/// Headless: rig diagnostics for one record — bone world translations, mount
/// offsets, mesh bbox, and which bone the mesh plausibly mounts to.
fn run_rigcheck(instance: u32) {
    match spore_install::discover() {
        Ok(install) => {
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                let found = index.entries.iter().find(|e| {
                    e.key.type_id == decode::TYPE_RW4_MODEL && e.key.instance_id == instance
                });
                let Some(e) = found else { continue };
                let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                    continue;
                };
                let Some(model) = rw4::extract_rigged_model(&blob) else {
                    println!("record {instance:#X}: no rigged model (no skeleton+pose+mesh)");
                    return;
                };
                println!(
                    "record {:#X}: {} bones, {} poses, {} mounts, {} anims, {} meshes",
                    instance,
                    model.bones.len(),
                    model.poses.len(),
                    model.skin_matrices.len(),
                    model.anims.len(),
                    model.meshes.len()
                );
                let Some((locals, invs)) = rw4::bind_locals(&model.bones, &model.poses) else {
                    println!("  bind_locals failed");
                    return;
                };
                let Some(worlds) = rw4::fk_worlds(
                    &model.bones.iter().map(|b| b.parent).collect::<Vec<_>>(),
                    &locals,
                ) else {
                    println!("  fk failed");
                    return;
                };
                // Bone world translations (bind pose).
                let mut min = [f32::MAX; 3];
                let mut max = [f32::MIN; 3];
                for w in &worlds {
                    let t = [w[0][3], w[1][3], w[2][3]];
                    for k in 0..3 {
                        min[k] = min[k].min(t[k]);
                        max[k] = max[k].max(t[k]);
                    }
                }
                println!(
                    "  bone bind extent: x {:.3}..{:.3} y {:.3}..{:.3} z {:.3}..{:.3}",
                    min[0], max[0], min[1], max[1], min[2], max[2]
                );
                // Roots + bone 0 bind transform (tells us the mesh's frame).
                let roots: Vec<usize> = model
                    .bones
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.parent.is_none())
                    .map(|(i, _)| i)
                    .collect();
                println!("  roots: {roots:?}");
                for r in roots.iter().take(4) {
                    let w = &worlds[*r];
                    println!(
                        "   bone{r} bind: pos ({:.3},{:.3},{:.3}) row0 ({:.2},{:.2},{:.2})",
                        w[0][3], w[1][3], w[2][3], w[0][0], w[0][1], w[0][2]
                    );
                }
                // Resolved bone names (wordlist attack).
                let named: Vec<String> = model
                    .bones
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| {
                        crate::game::known_bone_name(b.name).map(|n| format!("{i}:{n}"))
                    })
                    .collect();
                if !named.is_empty() {
                    println!("  named bones ({}): {}", named.len(), named.join(" "));
                }
                // Mesh extents + centroid.
                for (mi, m) in model.meshes.iter().enumerate() {
                    let mut c = [0.0f64; 3];
                    let mut lo = [f32::MAX; 3];
                    let mut hi = [f32::MIN; 3];
                    for p in &m.positions {
                        for k in 0..3 {
                            c[k] += p[k] as f64;
                            lo[k] = lo[k].min(p[k]);
                            hi[k] = hi[k].max(p[k]);
                        }
                    }
                    let n = m.positions.len().max(1) as f64;
                    let centroid = [c[0] / n, c[1] / n, c[2] / n];
                    println!(
                        "  mesh{mi}: {} verts centroid ({:.3},{:.3},{:.3}) extent {:.3}..{:.3} / {:.3}..{:.3} / {:.3}..{:.3}",
                        m.positions.len(),
                        centroid[0], centroid[1], centroid[2],
                        lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]
                    );
                    // Candidate attach bones: bone_world + mount_translation ~= centroid.
                    let mut best: Vec<(f32, usize)> = Vec::new();
                    for (bi, w) in worlds.iter().enumerate() {
                        let mount = model.skin_matrices.get(bi);
                        let (mtx, mty, mtz) = mount
                            .map(|m| (m[0][3], m[1][3], m[2][3]))
                            .unwrap_or((0.0, 0.0, 0.0));
                        let x = w[0][3] + mtx;
                        let y = w[1][3] + mty;
                        let z = w[2][3] + mtz;
                        let d = ((x - centroid[0] as f32).powi(2)
                            + (y - centroid[1] as f32).powi(2)
                            + (z - centroid[2] as f32).powi(2))
                        .sqrt();
                        best.push((d, bi));
                    }
                    best.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    let top: Vec<String> = best
                        .iter()
                        .take(3)
                        .map(|(d, b)| format!("bone{b} (d={d:.3})"))
                        .collect();
                    println!("    attach candidates: {}", top.join(", "));
                    // How many verts lie within 0.5 of the best bone's world position.
                    if let Some((_, bi)) = best.first() {
                        let w = &worlds[*bi];
                        let t = [w[0][3], w[1][3], w[2][3]];
                        let near = m
                            .positions
                            .iter()
                            .filter(|p| {
                                ((p[0] - t[0]).powi(2)
                                    + (p[1] - t[1]).powi(2)
                                    + (p[2] - t[2]).powi(2))
                                .sqrt()
                                    < 0.5
                            })
                            .count();
                        println!("    verts within 0.5 of bone{bi}: {near}");
                    }
                }
                // Mount summary: how many are non-identity.
                let non_id = model
                    .skin_matrices
                    .iter()
                    .filter(|m| {
                        (m[0][0] - 1.0).abs() > 1e-4
                            || (m[1][1] - 1.0).abs() > 1e-4
                            || (m[2][2] - 1.0).abs() > 1e-4
                            || m[0][3].abs() > 1e-4
                            || m[1][3].abs() > 1e-4
                            || m[2][3].abs() > 1e-4
                    })
                    .count();
                println!(
                    "  skin matrix buffer: {} non-identity of {} (absolute offset={:#X})",
                    non_id,
                    model.skin_matrices.len(),
                    model.matrix_buffer_offset
                );
                // Probe trailing vertex elements as real skin data (weights + indices).
                if let Some((count, stride, w_off, i_off, weights, indices)) =
                    rw4::vertex_skin_probe(&blob, model.bones.len())
                {
                    let sum_ok = weights
                        .iter()
                        .filter(|w| w.iter().map(|v| *v as u32).sum::<u32>() >= 250)
                        .count();
                    let mut max_idx = 0usize;
                    for b in &indices {
                        for v in b {
                            max_idx = max_idx.max(*v as usize);
                        }
                    }
                    println!(
                        "  SKIN PROBE: {count} verts stride={stride} weights@{w_off} indices@{i_off} max_idx={max_idx} weights_sum~255: {sum_ok}/{count}"
                    );
                } else {
                    println!("  skin probe: no trailing 4-byte pair with in-bound indices");
                }
                let _ = invs;
                // Sagittal pairs derived from the bind pose (mirror editing).
                let map = rw4::mirror_map(&worlds, 0.2);
                let pairs = map.iter().filter(|m| m.is_some()).count();
                let swapped = map
                    .iter()
                    .enumerate()
                    .filter(|(i, m)| m.map_or(false, |j| j != *i))
                    .count();
                println!(
                    "  mirror map: {pairs}/{} bones mapped, {swapped} true L/R pairs",
                    model.bones.len()
                );
                // Slot semantics by bind position.
                let mut census = std::collections::BTreeMap::new();
                for w in &worlds {
                    let kind = game::slot_kind(Vec3::new(w[0][3], w[1][3], w[2][3]));
                    *census.entry(format!("{kind:?}")).or_insert(0usize) += 1;
                }
                let summary: Vec<String> = census.iter().map(|(k, v)| format!("{k}={v}")).collect();
                println!("  slot kinds: {}", summary.join(" "));
                return;
            }
            println!("record {instance:#X} not found");
            std::process::exit(2);
        }
        Err(e) => {
            println!("RIGCHECK FAILED: {e}");
            std::process::exit(2);
        }
    }
}

fn rigcheck_arg() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--rigcheck" && i + 1 < args.len() {
            let v = args[i + 1]
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            return u32::from_str_radix(v, 16).ok();
        }
    }
    None
}

/// Headless: dump one RW4 record's static meshes as Wavefront OBJ.
fn extract_obj_arg() -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--extract-obj" && i + 1 < args.len() {
            let v = args[i + 1]
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            if let Ok(id) = u32::from_str_radix(v, 16) {
                return Some(id);
            }
        }
    }
    None
}

fn run_extract_obj(instance: u32) {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index.entries.iter().filter(|e| {
                    e.key.type_id == decode::TYPE_RW4_MODEL && e.key.instance_id == instance
                }) {
                    let meshes = rw4::meshes_from_package_record(pkg, e, &mut stats);
                    if meshes.is_empty() {
                        println!("record found but no static meshes (skinned?)");
                        return;
                    }
                    let fname = format!("mesh_{instance:08X}.obj");
                    let mut out = String::new();
                    let mut voff = 1u32;
                    for m in &meshes {
                        for p in &m.positions {
                            out.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
                        }
                        for t in &m.uvs {
                            out.push_str(&format!("vt {} {}\n", t[0], t[1]));
                        }
                        for n in &m.normals {
                            out.push_str(&format!("vn {} {} {}\n", n[0], n[1], n[2]));
                        }
                        for t in m.indices.chunks(3) {
                            out.push_str(&format!(
                                "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}\n",
                                t[0] as u32 + voff,
                                t[1] as u32 + voff,
                                t[2] as u32 + voff
                            ));
                        }
                        voff += m.positions.len() as u32;
                    }
                    std::fs::write(&fname, &out).expect("write obj");
                    println!(
                        "wrote {fname}: {} meshes, {} verts, {} tris",
                        meshes.len(),
                        meshes.iter().map(|m| m.positions.len()).sum::<usize>(),
                        meshes.iter().map(|m| m.indices.len() / 3).sum::<usize>()
                    );
                    return;
                }
            }
            println!("instance {instance:#X} not found in any RW4 record");
        }
        Err(e) => {
            println!("OBJ EXTRACT FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate gmdl assembly parsing + cross-package resolution.
fn run_dump_gmdl() {
    match spore_install::discover() {
        Ok(install) => {
            let known = gmdl::build_index_set(&install);
            println!("index keys: {}", known.len());
            let mut files = 0;
            let mut other_variant = 0;
            let mut table_refs = 0;
            let mut table_hit = 0;
            let mut mat_refs = 0;
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_GMDL)
                {
                    if files >= 60 || e.mem_size > 2_000_000 {
                        if files >= 60 {
                            break 'outer;
                        }
                        continue;
                    }
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let mesh_table = match gmdl::classify_table(&bytes) {
                        Some(gmdl::TableKind::MeshTable(refs)) => refs,
                        _ => {
                            other_variant += 1;
                            continue;
                        }
                    };
                    files += 1;
                    let mats = gmdl::scan_material_mesh_refs(&bytes, &known);
                    let hits = mesh_table
                        .iter()
                        .filter(|k| known.contains(&(k.type_id, k.group_id, k.instance_id)))
                        .count();
                    table_refs += mesh_table.len();
                    table_hit += hits;
                    mat_refs += mats.len();
                    if files <= 10 {
                        println!(
                            "  {} g={:#X} i={:#X}: table {}/{} resolve, {} material refs",
                            pkg.file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            e.key.group_id,
                            e.key.instance_id,
                            hits,
                            mesh_table.len(),
                            mats.len()
                        );
                    }
                }
            }
            println!("gmdl mesh_table_files={files} other_variant={other_variant} table_refs={table_refs} resolved={table_hit} material_refs={mat_refs}");
        }
        Err(e) => {
            println!("GMDL DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: catalog original animations by creature folder.
/// Headless: name-harvest the standalone `.animation` catalog — prints the
/// channel names found inside each container (e.g. `root`, `L foot`,
/// `MouthMorphs`), plus the count of containers parsed.
fn run_dump_anim_channels() {
    match spore_install::discover() {
        Ok(install) => {
            let mut names: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            let mut parsed = 0usize;
            let mut total = 0usize;
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_ANIM)
                {
                    total += 1;
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let chans = decode::parse_anim_channels(&bytes);
                    if chans.is_empty() {
                        continue;
                    }
                    parsed += 1;
                    for c in chans {
                        *names.entry(c).or_insert(0) += 1;
                    }
                }
            }
            let mut all: Vec<_> = names.into_iter().collect();
            all.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            println!("anim channels: {parsed}/{total} containers parsed");
            for (n, c) in all.iter().take(40) {
                println!("  {n}: {c}");
            }
            println!("distinct channel names: {}", all.len());
        }
        Err(e) => {
            println!("ANIM CHANNEL DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: decode keyframes from the standalone `.animation` catalog —
/// per-channel rot/pos keyframe counts with a unit-quaternion sanity check.
/// Headless: harvest animation channel selectors (capability × side) across
/// the catalog — this is Spore's part-animation targeting vocabulary.
fn run_dump_anim_selectors() {
    match spore_install::discover() {
        Ok(install) => {
            let (caps, sides) = decode::anim_selector_census(&install, 900);
            let mut by_cap: Vec<_> = caps.iter().collect();
            by_cap.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            println!("anim selectors (capability x side) over the catalog:");
            for (c, n) in by_cap.iter().take(20) {
                let cap = if c.is_empty() { "(none)" } else { c.as_str() };
                let mut parts: Vec<_> = sides.iter().filter(|((cc, _), _)| cc == *c).collect();
                parts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
                let detail: Vec<String> = parts
                    .iter()
                    .take(6)
                    .map(|((_, s), n)| format!("{}={}", if s.is_empty() { "-" } else { s }, n))
                    .collect();
                println!("  {cap}: {n}  ({})", detail.join(" "));
            }
        }
        Err(e) => {
            println!("SELECTOR DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

fn run_dump_anim_keys() {
    match spore_install::discover() {
        Ok(install) => {
            let mut seen = 0usize;
            let mut parsed = 0usize;
            let mut total_keys = 0usize;
            let mut unit = 0usize;
            let mut quats = 0usize;
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_ANIM)
                {
                    seen += 1;
                    if seen > 40 {
                        break;
                    }
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let Some(clip) = decode::parse_anim_clip(&bytes) else {
                        continue;
                    };
                    parsed += 1;
                    let mut lines = Vec::new();
                    for c in &clip.channels {
                        for q in &c.rot {
                            quats += 1;
                            let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
                            if (l - 1.0).abs() < 0.02 {
                                unit += 1;
                            }
                        }
                        total_keys += c.rot.len() + c.pos.len();
                        if lines.len() < 6 && !c.rot.is_empty() {
                            lines.push(format!(
                                "  {:<14} rot={} pos={} q0=({:.2},{:.2},{:.2},{:.2})",
                                c.name,
                                c.rot.len(),
                                c.pos.len(),
                                c.rot.first().map(|q| q[0]).unwrap_or(0.0),
                                c.rot.first().map(|q| q[1]).unwrap_or(0.0),
                                c.rot.first().map(|q| q[2]).unwrap_or(0.0),
                                c.rot.first().map(|q| q[3]).unwrap_or(0.0),
                            ));
                        }
                        if lines.len() >= 6 {
                            break;
                        }
                    }
                    if parsed <= 4 && !lines.is_empty() {
                        println!(
                            "clip {:#X} (len {:.2}s, {} channels):",
                            e.key.instance_id,
                            clip.length,
                            clip.channels.len()
                        );
                        for l in &lines {
                            println!("{l}");
                        }
                    }
                    if seen > 40 {
                        break;
                    }
                }
            }
            println!(
                "anim keys: {parsed} clips decoded, {total_keys} keyframes, unit quats {unit}/{quats}"
            );
            // What the menu's clip preview would load (same selection logic).
            let picks = pick_clip_previews(&install, 600);
            println!("clip preview picks:");
            for p in &picks {
                let keys: usize = p
                    .clip
                    .channels
                    .iter()
                    .map(|c| c.rot.len() + c.pos.len())
                    .sum();
                println!(
                    "  {:<24} channels={} keyframes={} len={:.1}",
                    p.label,
                    p.clip.channels.len(),
                    keys,
                    p.clip.length
                );
            }
        }
        Err(e) => {
            println!("ANIM KEY DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

fn run_dump_anims() {
    match spore_install::discover() {
        Ok(install) => {
            let (total, sample) = decode::collect_anims(&install, 300);
            let mut folders = std::collections::HashMap::new();
            for a in &sample {
                let key = a
                    .path
                    .split('\\')
                    .collect::<Vec<_>>()
                    .iter()
                    .rev()
                    .nth(1)
                    .unwrap_or(&"?")
                    .to_string();
                *folders.entry(key).or_insert(0) += 1;
            }
            let mut folders: Vec<_> = folders.into_iter().collect();
            folders.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            println!("anims total={total} sampled={}", sample.len());
            for (f, c) in folders.into_iter().take(15) {
                println!("  {f}: {c}");
            }
            for a in sample.iter().take(8) {
                println!("  e.g. v{} {} ({} bytes)", a.version, a.path, a.bytes);
            }
        }
        Err(e) => {
            println!("ANIM DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate skeleton + bind-pose parsing at scale.
fn run_dump_skeletons() {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            let mut scanned = 0;
            let mut files = 0;
            let mut matched = 0;
            let mut max_bones = 0;
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
                {
                    if scanned >= 150 || e.mem_size > 2_000_000 {
                        if scanned >= 150 {
                            break 'outer;
                        }
                        continue;
                    }
                    scanned += 1;
                    let (skels, skins) = rw4::rig_from_package_record(pkg, e, &mut stats);
                    if skels.is_empty() && skins.is_empty() {
                        continue;
                    }
                    files += 1;
                    for s in &skels {
                        max_bones = max_bones.max(s.len());
                    }
                    // Animation-ready: some skeleton's bone count matches some skin's pose count.
                    if skels
                        .iter()
                        .any(|b| skins.iter().any(|p| p.len() == b.len()))
                    {
                        matched += 1;
                    }
                    if files <= 10 {
                        let b: usize = skels.iter().map(|s| s.len()).sum();
                        let p: usize = skins.iter().map(|s| s.len()).sum();
                        println!(
                            "  {} g={:#X} i={:#X}: {} skeletons ({} bones), {} skins ({} poses)",
                            pkg.file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            e.key.group_id,
                            e.key.instance_id,
                            skels.len(),
                            b,
                            skins.len(),
                            p
                        );
                    }
                }
            }
            println!(
                "rig scanned={scanned} files={files} skeletons={} bones={} valid_parents={} skins={} poses={} anim_ready={matched} max_bones={max_bones}",
                stats.skeletons, stats.bones, stats.bones_valid_parents, stats.skins, stats.poses
            );
        }
        Err(e) => {
            println!("SKELETON DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: full-corpus rig census (no record cap). Finds the biggest rigs
/// and records whose skin bone indices exceed every file-local rig — the
/// cases that need the shared master skeleton.
fn run_census_rigs() {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            let mut big: Vec<(usize, String, u32)> = Vec::new(); // bones, file, inst
            let mut orphans: Vec<(String, u32, usize, usize)> = Vec::new(); // file, inst, local, max_idx
            let mut records = 0usize;
            let mut rigged_files = 0usize;
            let mut skinned_files = 0usize;
            let mut histogram: std::collections::BTreeMap<usize, usize> =
                std::collections::BTreeMap::new();
            for pkg in &install.packages {
                let fname = pkg
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
                {
                    records += 1;
                    let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let meshes = rw4::extract_meshes(&blob, &mut stats);
                    let (skels, _skins) = rw4::extract_rig(&blob, &mut stats);
                    let local = skels.iter().map(|s| s.len()).max().unwrap_or(0);
                    if local > 25 {
                        big.push((local, fname.clone(), e.key.instance_id));
                    }
                    if local > 0 {
                        rigged_files += 1;
                    }
                    let mut max_idx = 0usize;
                    let mut has_skin = false;
                    for m in &meshes {
                        if m.skin_idx.is_empty() {
                            continue;
                        }
                        has_skin = true;
                        for p in &m.skin_idx {
                            max_idx = max_idx.max(p[0] as usize).max(p[1] as usize);
                        }
                    }
                    if has_skin {
                        skinned_files += 1;
                        *histogram.entry(max_idx + 1).or_insert(0) += 1;
                    }
                    if has_skin && max_idx >= local {
                        orphans.push((fname.clone(), e.key.instance_id, local, max_idx));
                    }
                }
            }
            big.sort_by(|a, b| b.0.cmp(&a.0));
            println!("rig census: {records} rw4 records, {rigged_files} with rigs, {skinned_files} with skins");
            println!("top rigs by bone count:");
            for (n, f, i) in big.iter().take(12) {
                println!("  bones={n} {f} inst={i:#X}");
            }
            println!("skin-index histogram (min bones implied = idx+1), top 15:");
            let mut hist: Vec<_> = histogram.into_iter().collect();
            hist.sort_by(|a, b| b.1.cmp(&a.1));
            for (n, c) in hist.iter().take(15) {
                println!("  implied_bones={n} records={c}");
            }
            println!(
                "orphans (skin indices exceed file-local rig): {}",
                orphans.len()
            );
            for (f, i, local, mx) in orphans.iter().take(12) {
                println!("  {f} inst={i:#X} local_bones={local} max_skin_idx={mx}");
            }
        }
        Err(e) => {
            println!("RIG CENSUS FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate skin pools (blend indices/weights) at scale.
fn run_dump_skin() {
    match spore_install::discover() {
        Ok(install) => {
            let mut scanned = 0;
            let mut pools = 0;
            let mut skinned = 0;
            let mut covered = 0;
            let mut max_idx_seen: u16 = 0;
            let mut demo: Option<(usize, usize, usize)> = None;
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
                {
                    if scanned >= 1500 || e.mem_size > 2_000_000 {
                        if scanned >= 1500 {
                            break 'outer;
                        }
                        continue;
                    }
                    scanned += 1;
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    if demo.is_none() {
                        if let Some((em, locals, _, _, wave, pair)) =
                            rw4::score_skinned_candidate(&bytes)
                        {
                            demo = Some((em.positions.len(), locals.len(), wave));
                            let mapped = pair
                                .as_ref()
                                .map(|p| p.chan_for_bone.iter().filter(|c| c.is_some()).count())
                                .unwrap_or(0);
                            println!(
                                "  demo candidate {} i={:#X}: {} verts {} bones wave={} clip_mapped={}/{}",
                                pkg.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                                e.key.instance_id,
                                em.positions.len(),
                                locals.len(),
                                wave,
                                mapped,
                                locals.len()
                            );
                        }
                    }
                    let rep = rw4::pool_report(&bytes);
                    if rep.is_empty() {
                        continue;
                    }
                    pools += rep.len();
                    // Bones + poses in the same file for index mapping analysis.
                    let mut stats = rw4::MeshStats::default();
                    let (skels, skins) = rw4::extract_rig(&bytes, &mut stats);
                    let bones: usize = skels.iter().map(|s| s.len()).sum();
                    let poses: usize = skins.iter().map(|s| s.len()).sum();
                    for p in &rep {
                        if p.skinned {
                            skinned += 1;
                            max_idx_seen = max_idx_seen.max(p.max_bone_idx);
                            if (p.max_bone_idx as usize) < bones {
                                covered += 1;
                            }
                            if skinned <= 12 {
                                println!(
                                    "  {} i={:#X}: {} verts {} shapes maxIdx={} fileBones={} filePoses={}",
                                    pkg.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                                    e.key.instance_id,
                                    p.vertex_count,
                                    p.shapes,
                                    p.max_bone_idx,
                                    bones,
                                    poses
                                );
                            }
                        }
                    }
                }
            }
            println!("skin scanned={scanned} pools={pools} skinned={skinned} idx_covered={covered} max_idx={max_idx_seen} demo={demo:?}");
        }
        Err(e) => {
            println!("SKIN DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate keyframe animation parsing + bone mapping at scale.
fn run_dump_keyframes() {
    match spore_install::discover() {
        Ok(install) => {
            let mut stats = rw4::MeshStats::default();
            let mut scanned = 0;
            let mut files = 0;
            let mut mapped = 0;
            let mut quat_bad = 0;
            let mut quat_n = 0;
            'outer: for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
                {
                    if scanned >= 150 || e.mem_size > 2_000_000 {
                        if scanned >= 150 {
                            break 'outer;
                        }
                        continue;
                    }
                    scanned += 1;
                    let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let anims = rw4::extract_anims(&blob, &mut stats);
                    if anims.is_empty() {
                        continue;
                    }
                    files += 1;
                    // Bone-name mapping in the same file.
                    let (skels, _) = rw4::extract_rig(&blob, &mut stats);
                    let names: std::collections::HashSet<u32> = skels
                        .iter()
                        .flat_map(|s| s.iter().map(|b| b.name))
                        .collect();
                    for a in &anims {
                        let m = a.channels.iter().filter(|c| names.contains(&c.id)).count();
                        mapped += m;
                        if files <= 8 {
                            println!(
                                "  {} i={:#X}: len={:.2}s {}ch {}/{} bone-mapped",
                                pkg.file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                e.key.instance_id,
                                a.length,
                                a.channels.len(),
                                m,
                                a.channels.len()
                            );
                        }
                        for c in &a.channels {
                            for k in &c.keys {
                                if !k.blend {
                                    let n = (k.rotation[0].powi(2)
                                        + k.rotation[1].powi(2)
                                        + k.rotation[2].powi(2)
                                        + k.rotation[3].powi(2))
                                    .sqrt();
                                    quat_n += 1;
                                    if (n - 1.0).abs() > 0.05 {
                                        quat_bad += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            println!(
                "keyanim scanned={scanned} files={files} anims={} channels={} keys={} mapped={mapped} quats_checked={quat_n} quat_bad={quat_bad}",
                stats.keyanims, stats.channels, stats.keys
            );
        }
        Err(e) => {
            println!("KEYFRAME DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: catalog EA SNR audio (codecs, rates, durations).
fn run_dump_audio() {
    match spore_install::discover() {
        Ok(install) => {
            use std::collections::HashMap;
            let mut codecs: HashMap<String, usize> = HashMap::new();
            let mut rates: HashMap<u32, usize> = HashMap::new();
            let mut total_samples: u64 = 0;
            let mut total_samples_known_rate: u64 = 0;
            let mut common_rate: u32 = 44100;
            let mut max_count = 0;
            let mut n = 0;
            let mut xas_ok = 0;
            let mut xas_fail = 0;
            let mut menu_clips = 0;
            // Codec x channels x storage census (`notes\audio-codec.md`); `undecodable` counts the
            // codecs `audio::decode_snr_sound` does not know (none since the codec 0 / 1 / 2 decoders).
            let mut layouts: std::collections::BTreeMap<(String, usize, &str), usize> = Default::default();
            let mut undecodable = 0;
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index.entries.iter().filter(|e| audio::is_audio_record(e)) {
                    // Header-only parse needs just 8 bytes: read raw start.
                    // (Records are usually uncompressed; fall back to full read.)
                    n += 1;
                    let head = read_head_bytes(pkg, e, 8);
                    let Some(h) = head.as_ref().and_then(|b| audio::parse_snr_header(b)) else {
                        continue;
                    };
                    let label = h.codec.label();
                    if !h.codec.supported() {
                        undecodable += 1;
                    }
                    let storage = if h.is_ram { "ram" } else { "stream" };
                    *layouts.entry((label.clone(), h.channels, storage)).or_insert(0) += 1;
                    *codecs.entry(label).or_insert(0) += 1;
                    *rates.entry(h.sample_rate).or_insert(0) += 1;
                    if rates[&h.sample_rate] > max_count {
                        max_count = rates[&h.sample_rate];
                        common_rate = h.sample_rate;
                    }
                    total_samples += h.num_samples as u64;
                    if h.sample_rate == common_rate || total_samples_known_rate == 0 {
                        total_samples_known_rate += h.num_samples as u64;
                    }
                    // Trial-decode capped XAS1 records for validation.
                    if h.codec == audio::SnrCodec::Xas1
                        && xas_ok + xas_fail < 40
                        && e.mem_size < 200000
                    {
                        match dbpf::read_entry_bytes(pkg, e)
                            .ok()
                            .and_then(|b| audio::decode_xas_ram(&b))
                        {
                            Some((pcm, _, _)) if pcm.len() / h.channels.max(1) == h.num_samples => {
                                xas_ok += 1
                            }
                            _ => xas_fail += 1,
                        }
                    }
                    // Menu-music candidates: short RAM clips like the loader wants.
                    if h.codec == audio::SnrCodec::Xas1 && h.channels <= 2 {
                        let secs = h.num_samples as f32 / h.sample_rate as f32;
                        if (0.4..=6.0).contains(&secs) {
                            menu_clips += 1;
                        }
                    }
                }
            }
            println!("audio records={n} codecs={codecs:?} rates={rates:?}");
            println!("audio layouts (codec, channels, storage) = {layouts:?} undecodable={undecodable}");
            println!(
                "total samples={total_samples} (~{:.1} min @ {common_rate}Hz)",
                total_samples as f64 / common_rate as f64 / 60.0
            );
            println!("xas trial decodes ok={xas_ok} fail={xas_fail} menu_clips={menu_clips}");
        }
        Err(e) => {
            println!("AUDIO DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Read just the first `n` bytes of a record (cheap header peek).
fn read_head_bytes(pkg: &std::path::Path, e: &dbpf::DbpfEntry, n: usize) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(pkg).ok()?;
    f.seek(SeekFrom::Start(e.offset as u64)).ok()?;
    let mut buf = vec![0u8; n.min(e.disk_size as usize)];
    f.read_exact(&mut buf).ok()?;
    // Compressed records need full decode; signal by returning None here
    // only when the bytes look like a QFS header.
    if buf.len() >= 2 && (buf[0] == 0x10 || buf[0] == 0x50) && buf[1] == 0xFB {
        return dbpf::read_entry_bytes(pkg, e)
            .ok()
            .map(|b| b[..n.min(b.len())].to_vec());
    }
    Some(buf)
}

fn extract_wav_arg() -> Option<u32> {
    extract_hex_arg("--extract-wav")
}

fn extract_mp3_arg() -> Option<u32> {
    extract_hex_arg("--extract-mp3")
}

fn extract_hex_arg(flag: &str) -> Option<u32> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == flag && i + 1 < args.len() {
            let v = args[i + 1]
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            if let Ok(id) = u32::from_str_radix(v, 16) {
                return Some(id);
            }
        }
    }
    None
}

/// Headless: rebuild one EALayer3 record into standard MPEG frames.
fn run_extract_mp3(instance: u32) {
    match spore_install::discover() {
        Ok(install) => {
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| audio::is_audio_record(e) && e.key.instance_id == instance)
                {
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let Some(h) = audio::parse_snr_header(&bytes) else {
                        continue;
                    };
                    if h.codec != audio::SnrCodec::EaLayer3V1 {
                        println!("record is not EALayer3");
                        return;
                    }
                    // Body starts after the 8B SNR header + 4B block header +
                    // 4B prefix (established by tiling validation).
                    let Some((mp3, ver)) = audio::record_to_mp3_cbr(&bytes, h.sample_rate) else {
                        // Diagnose the stage: break positions per start.
                        let mut diag = Vec::new();
                        for s in [12usize, 16, 17, 32, 33] {
                            if s < bytes.len() {
                                let (n, p, r) = audio::tiling_break(&bytes, s);
                                diag.push(format!("{s}:{n}f@{p}({r})"));
                            }
                        }
                        println!("mp3 rebuild failed: [{}]", diag.join(" "));
                        return;
                    };
                    let fname = format!("audio_{instance:08X}.mp3");
                    std::fs::write(&fname, &mp3).expect("write mp3");
                    // Validate: every rebuilt frame must start with a sync.
                    // Free-format has no lengths, so scan sync-to-sync.
                    let mut bad = 0;
                    let mut found = 0;
                    let mut pos = 0;
                    while pos + 4 <= mp3.len() {
                        if mp3[pos] != 0xFF || mp3[pos + 1] & 0xE0 != 0xE0 {
                            bad += 1;
                            break;
                        }
                        found += 1;
                        let mut next = pos + 4;
                        while next + 1 < mp3.len()
                            && !(mp3[next] == 0xFF && mp3[next + 1] & 0xE0 == 0xE0)
                        {
                            next += 1;
                        }
                        if next + 1 >= mp3.len() {
                            break;
                        }
                        pos = next;
                    }
                    println!(
                        "wrote {fname}: mpeg{ver} {} bytes, syncs_found={found} bad_syncs={bad}",
                        mp3.len()
                    );
                    // Full decode path (trim + stereo) for inspection + playback.
                    match audio::decode_eal3_record(&bytes) {
                        Some((pcm, ch, rate)) => {
                            let peak = pcm.iter().map(|v| (*v as i32).abs()).max().unwrap_or(0);
                            println!(
                                "eal3 decode: {} samples {ch}ch {rate}Hz peak={peak}",
                                pcm.len() / ch.max(1)
                            );
                            let wav = audio::write_wav(&pcm, ch, rate);
                            let wname = format!("audio_{instance:08X}_eal3.wav");
                            std::fs::write(&wname, &wav).expect("write wav");
                            println!("wrote {wname}");
                        }
                        None => println!("eal3 full decode failed (MPEG1 or corrupt?)"),
                    }
                    return;
                }
            }
            println!("audio instance {instance:#X} not found");
        }
        Err(e) => {
            println!("MP3 EXTRACT FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: decode one XAS1 record to a WAV file.
fn run_extract_wav(instance: u32) {
    match spore_install::discover() {
        Ok(install) => {
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| audio::is_audio_record(e) && e.key.instance_id == instance)
                {
                    match audio::wav_from_package_record(pkg, e) {
                        Some((wav, ch, rate, samples)) => {
                            let fname = format!("audio_{instance:08X}.wav");
                            std::fs::write(&fname, &wav).expect("write wav");
                            println!("wrote {fname}: {ch}ch {rate}Hz {samples} samples");
                            return;
                        }
                        None => {
                            println!("record found but not decodable XAS1 RAM");
                            return;
                        }
                    }
                }
            }
            println!("audio instance {instance:#X} not found");
        }
        Err(e) => {
            println!("WAV EXTRACT FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: validate EALayer3 frame tiling on real records.
fn run_dump_eal3() {
    match spore_install::discover() {
        Ok(install) => {
            let mut scanned = 0;
            let mut tiled = 0;
            let mut frames_total = 0;
            let mut main_bits_total: u64 = 0;
            let mut chan_modes = std::collections::HashMap::new();
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index.entries.iter().filter(|e| {
                    e.key.type_id == decode::TYPE_AUDIO && e.mem_size < 500000 && e.mem_size > 100
                }) {
                    if scanned >= 60 {
                        break;
                    }
                    let Ok(bytes) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let Some(h) = audio::parse_snr_header(&bytes) else {
                        continue;
                    };
                    if h.codec != audio::SnrCodec::EaLayer3V1 {
                        continue;
                    }
                    scanned += 1;
                    // Try body starts 12 and 16 (4-byte prefix variance).
                    let mut done = false;
                    for start in [12usize, 16usize] {
                        if start >= bytes.len() {
                            continue;
                        }
                        if let Some(frames) = audio::walk_eal_frames(&bytes, start) {
                            // Require exact tiling (called only on full success).
                            let end: usize = start + frames.iter().map(|f| f.size).sum::<usize>();
                            if end == bytes.len() {
                                tiled += 1;
                                frames_total += frames.len();
                                main_bits_total += frames
                                    .iter()
                                    .flat_map(|f| f.main_data_bits.iter())
                                    .sum::<u32>()
                                    as u64;
                                if tiled <= 6 {
                                    println!(
                                        "  {} i={:#X}: {} frames from {} ({} main-data kbits)",
                                        pkg.file_name()
                                            .map(|s| s.to_string_lossy().into_owned())
                                            .unwrap_or_default(),
                                        e.key.instance_id,
                                        frames.len(),
                                        start,
                                        frames
                                            .iter()
                                            .flat_map(|f| f.main_data_bits.iter())
                                            .sum::<u32>()
                                            / 1000
                                    );
                                }
                                for f in &frames {
                                    *chan_modes.entry(f.channels).or_insert(0) += 1;
                                }
                                done = true;
                                break;
                            }
                        }
                    }
                    if !done {
                        println!(
                            "  UNTiLED {} i={:#X} mem={}",
                            pkg.file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            e.key.instance_id,
                            e.mem_size
                        );
                    }
                }
            }
            println!("eal3 scanned={scanned} tiled={tiled} frames={frames_total} main_kbits={} channels={chan_modes:?}", main_bits_total / 1000);
        }
        Err(e) => {
            println!("EAL3 DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

/// Headless: census skeletons across RW4 files (bone counts + name hashes).
fn run_dump_rigs() {
    match spore_install::discover() {
        Ok(install) => {
            let mut scanned = 0;
            // (bones, instance, package, name hashes)
            let mut table: Vec<(usize, u32, String, Vec<u32>)> = Vec::new();
            for pkg in &install.packages {
                let Ok(index) = dbpf::read_package_index(pkg) else {
                    continue;
                };
                for e in index
                    .entries
                    .iter()
                    .filter(|e| e.key.type_id == decode::TYPE_RW4_MODEL)
                {
                    if scanned >= 3000 || e.mem_size > 1_500_000 {
                        continue;
                    }
                    scanned += 1;
                    let Ok(blob) = dbpf::read_entry_bytes(pkg, e) else {
                        continue;
                    };
                    let mut stats = rw4::MeshStats::default();
                    let (skels, _) = rw4::extract_rig(&blob, &mut stats);
                    if skels.is_empty() {
                        continue;
                    }
                    let bones: usize = skels.iter().map(|s| s.len()).sum();
                    if bones == 0 {
                        continue;
                    }
                    let mut names: Vec<u32> = skels
                        .iter()
                        .flat_map(|s| s.iter().map(|b| b.name))
                        .collect();
                    names.sort_unstable();
                    names.dedup();
                    table.push((
                        bones,
                        e.key.instance_id,
                        pkg.file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        names,
                    ));
                }
            }
            table.sort_by_key(|(b, _, _, _)| std::cmp::Reverse(*b));
            println!("rig scanned={scanned} files_with_skeletons={}", table.len());
            for (bones, inst, pkg, names) in table.iter().take(15) {
                println!("  {bones} bones {pkg} i={inst:#X}");
                let _ = names;
            }
            // Subset test: which rigs contain ALL demo-file bone names?
            // Demo names are printed by --dump-skin demo line context; instead,
            // report pairwise: rigs whose name set is a superset of another's.
            // (Full matrix is O(n^2); report only max-size supersets.)
            if let Some((big, _, _, bignames)) = table.first() {
                let mut contained = 0;
                for (_, _, _, names) in &table {
                    if names.iter().all(|n| bignames.contains(n)) {
                        contained += 1;
                    }
                }
                println!(
                    "largest rig ({big} bones) contains all names of {contained}/{} rigs",
                    table.len()
                );
            }
        }
        Err(e) => {
            println!("RIG DUMP FAILED: {e}");
            std::process::exit(2);
        }
    }
}

fn dump_type_histogram(install: &spore_install::SporeInstall) {
    use std::collections::HashMap;
    let mut hist: HashMap<u32, usize> = HashMap::new();
    for pkg in &install.packages {
        if let Ok(index) = dbpf::read_package_index(pkg) {
            for e in &index.entries {
                *hist.entry(e.key.type_id).or_default() += 1;
            }
        }
    }
    let mut top: Vec<(u32, usize)> = hist.into_iter().collect();
    top.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    println!("TOP resource types (type_id hex : count):");
    for (t, c) in top.into_iter().take(25) {
        println!("  {:#010X} : {c}", t);
    }
}

// ---------------------------------------------------------------------------

/// Inspect the same inherited, localized definitions used by the creators.
/// Writes metadata to stdout and leaves the installed packages untouched.
fn run_dump_part_catalog() {
    let install = spore_install::discover().expect("Spore install for --dump-part-catalog");
    let index = asset_catalog::ResourceIndex::load(&install.packages);
    let catalog = asset_catalog::PartCatalog::from_index(&index);
    let key = |k: dbpf::ResourceKey| {
        serde_json::json!({
            "group": k.group_id, "instance": k.instance_id, "type": k.type_id
        })
    };
    let parts: Vec<_> = catalog
        .parts
        .iter()
        .map(|p| {
            serde_json::json!({
                "definition": key(p.key), "model": key(p.model), "name": p.name,
                "internal": p.is_null, "price": p.price, "unlock_level": p.unlock_level,
                "scale": [p.min_scale, p.default_scale, p.max_scale],
                "model_scale": p.model_scale, "model_offset": p.model_offset,
                "model_rotation": p.model_rotation,
                "diffuse": p.diffuse.map(&key), "tint": p.tint.map(&key),
                "spec_bump": p.spec_bump.map(&key), "abilities": p.abilities.summary()
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "parts": parts, "catalog_errors": catalog.errors, "index_errors": index.errors
        }))
        .expect("part catalog JSON")
    );
}

/// Read original construction documents through the verified DBPF loader.
fn run_dump_creation_records() {
    let install = spore_install::discover().expect("Spore install for --dump-creation-records");
    let index = asset_catalog::ResourceIndex::load(&install.packages);
    let mut keys: Vec<_> = index
        .records
        .keys()
        .copied()
        .filter(|k| [0x2399be55, 0x24682294, 0x476a98c7].contains(&k.type_id))
        .collect();
    keys.sort_by_key(|k| (k.type_id, k.group_id, k.instance_id));
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for key in keys {
        match index
            .read(key)
            .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
        {
            Ok(xml) => records.push(serde_json::json!({
                "key": { "group": key.group_id, "instance": key.instance_id, "type": key.type_id },
                "xml": xml
            })),
            Err(error) => errors.push(format!("{key:?}: {error}")),
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "records": records, "errors": errors, "index_errors": index.errors
        }))
        .expect("creation record JSON")
    );
}

/// Audit each saved handle through the same readers and deformation code
/// used by the renderer, without allocating GPU assets or spawning a game.
fn run_dump_creation_morphs() {
    let install = spore_install::discover().expect("Spore install for --dump-creation-morphs");
    let index = asset_catalog::ResourceIndex::load(&install.packages);
    let catalog = asset_catalog::PartCatalog::from_index(&index);
    let creations = creation_records::CreationCatalog::from_index(&index);
    let mut rows = Vec::new();
    let mut errors = creations.errors.clone();
    for record in &creations.creations {
        for (block_index, block) in record.blocks.iter().enumerate() {
            let result = (|| {
                let definition = catalog
                    .parts
                    .iter()
                    .find(|d| d.key == block.definition)
                    .ok_or("missing definition")?;
                let bytes = index.read(definition.model)?;
                let data = rw4::extract_morph_data(&bytes)?;
                let mut meshes = rw4::extract_meshes(&bytes, &mut rw4::MeshStats::default());
                let applied = morph::apply_creation_handles(&mut meshes, &data, &block.handles);
                if let Err(e) = &applied {
                    errors.push(format!(
                        "{:08X} block {block_index}: {e}",
                        record.key.instance_id
                    ));
                }
                Ok::<_, String>(serde_json::json!({
                    "creation":record.name,"creation_instance":record.key.instance_id,"block":block_index,
                    "definition":block.definition.instance_id,"model":definition.model.instance_id,
                    "xml_handles":block.handles.len(),"model_handles":data.handles.len(),
                    "clips":data.handles.iter().map(|h|serde_json::json!({"id":h.id,"reference":h.animation_reference,
                        "length":h.animation.as_ref().map(|a|a.length),"default":h.default_time,
                        "channels":h.animation.as_ref().map(|a|a.channels.iter().map(|c|serde_json::json!({"id":c.id,
                            "blend":c.keys.first().map(|k|k.blend),"first":c.keys.first().map(|k|k.time),"last":c.keys.last().map(|k|k.time)})).collect::<Vec<_>>())})).collect::<Vec<_>>(),
                    "error":applied.as_ref().err(),
                    "shape_sets":data.shapes.iter().map(|s|&s.shape_ids).collect::<Vec<_>>(),
                    "rig_bones":data.rig.as_ref().map(|r|r.bones.len()),
                    "bone_vertices":meshes.iter().map(|m|m.bone_weights.len()).sum::<usize>(),
                    "shape_weights":applied.as_ref().ok().map(|a|&a.shape_weights),
                    "moved_vertices":applied.as_ref().ok().map(|a|a.moved_vertices),
                    "attachment_channels":applied.as_ref().ok().map(|a|a.attachment_channels),
                    "vertices":meshes.iter().map(|m|m.positions.len()).sum::<usize>()
                }))
            })();
            match result {
                Ok(row) => rows.push(row),
                Err(e) => errors.push(format!(
                    "{:08X} block {block_index}: {e}",
                    record.key.instance_id
                )),
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"blocks":rows,"errors":errors,"index_errors":index.errors})
        )
        .unwrap()
    );
}

fn run_export_native_model(group: u32, instance: u32, output: &std::path::Path) {
    std::fs::create_dir_all(output).expect("create native model output");
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .expect("ignored target");
    let output = output.canonicalize().expect("native model output path");
    assert!(
        output.starts_with(&target),
        "original model exports must stay in ignored target"
    );
    let install = spore_install::discover().expect("original install");
    let index = asset_catalog::ResourceIndex::load(&install.packages);
    let keys: Vec<_> = index
        .records
        .keys()
        .filter(|k| {
            k.type_id == decode::TYPE_GMDL && k.instance_id == instance && k.group_id == group
        })
        .copied()
        .collect();
    assert_eq!(
        keys.len(),
        1,
        "native model export requires one unambiguous full resource key"
    );
    let key = keys[0];
    let bytes = index.read(key).expect("original GMDL");
    let model = baked_model::parse(&bytes).expect("decode original GMDL");
    std::fs::write(output.join("model.gmdl"), &bytes).expect("write original model");
    let mut textures = vec![];
    for (piece, material) in model.materials.iter().enumerate() {
        for binding in &material.textures {
            let resolved_key =
                baked_model::texture_key(&index, binding.key).expect("indexed texture entry");
            let bytes = index.read(resolved_key).expect("original model texture");
            let native_texture = native_texture::decode(&bytes, None)
                .expect("preserve original texture mips/formats");
            let filename = format!(
                "{:08X}-{:08X}-{:08X}.raster",
                resolved_key.type_id, resolved_key.group_id, resolved_key.instance_id
            );
            std::fs::write(output.join(&filename), &bytes).expect("write original texture");
            let native_file = format!("{filename}.json");
            std::fs::write(
                output.join(&native_file),
                serde_json::to_vec_pretty(&native_texture).unwrap(),
            )
            .expect("native texture payload");
            textures.push(serde_json::json!({"piece":piece,"sampler":binding.sampler,"sampler_extra":binding.extra,"key":resource_audit::key_json(resolved_key),"native_entry_group":format!("{:08X}",binding.key.group_id),"native_entry_instance":format!("{:08X}",binding.key.instance_id),"file":filename,"sha256":parity::sha256(&bytes),"native_texture":native_file,"format":native_texture.format,"mips":native_texture.faces[0].len(),"faces":native_texture.faces.len()}));
        }
    }
    let report = serde_json::json!({"version":1,"source":resource_audit::key_json(key),"source_sha256":parity::sha256(&bytes),"gmdl_version":model.version,"native_geometry":model.native_geometry,
        "material_ids":model.pieces.iter().map(|p|p.material_id).collect::<Vec<_>>(),"material_parameters":model.materials.iter().map(|m|m.parameters).collect::<Vec<_>>(),"textures":textures,"bone_ranges":model.bone_ranges,"root_bones":model.root_bones,
        "limitation":"Native buffers and original texture payloads preserved. Shader selection, rig pose and scene constants require original capture; this export is not a native scene comparison."});
    std::fs::write(
        output.join("native-model.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .expect("native model metadata");
    println!(
        "Exported original model {:08X} into {}",
        instance,
        output.display()
    );
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|a| a == "--pd-trace") {
        std::process::exit(pd::trace::run_cli(&args));
    }
    if let Some(i) = args.iter().position(|a| a == "--renderer") {
        match args.get(i + 1).map(String::as_str) {
            Some("wgpu-preview") => {}
            Some("native-d3d9") => {
                eprintln!("Native scene presentation has not passed the original-capture gates. Use openspore-parity replay for native draw packets, or --native-skinpaint for the implemented original part passes.");
                std::process::exit(2);
            }
            _ => {
                eprintln!("--renderer requires wgpu-preview or native-d3d9. Native scene presentation is still under implementation.");
                std::process::exit(2);
            }
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--export-native-model") {
        let (group, instance) = args
            .get(i + 1)
            .expect("GROUP!INSTANCE resource key")
            .split_once('!')
            .expect("native model export requires GROUP!INSTANCE");
        let group = u32::from_str_radix(group.trim_start_matches("0x"), 16).expect("hex group ID");
        let instance =
            u32::from_str_radix(instance.trim_start_matches("0x"), 16).expect("hex instance ID");
        run_export_native_model(
            group,
            instance,
            std::path::Path::new(args.get(i + 2).expect("native export output directory")),
        );
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--dump-creature-data") {
        let result = args
            .get(i + 1)
            .ok_or("--dump-creature-data requires a target output file")
            .map_err(String::from)
            .and_then(|p| {
                let package = args
                    .iter()
                    .position(|a| a == "--creature-data-package")
                    .map(|j| {
                        args.get(j + 1)
                            .map(std::path::Path::new)
                            .ok_or("--creature-data-package requires a copied package path")
                    })
                    .transpose()
                    .map_err(String::from)?;
                creature_data_catalog::run(std::path::Path::new(p), package)
            });
        if let Err(error) = result {
            eprintln!("Native creature data: {error}");
            std::process::exit(2);
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--dump-planet") {
        let output = args.get(i + 1).expect("--dump-planet requires an output directory");
        let which = args.get(i + 2).filter(|a| !a.starts_with("--")).map(String::as_str);
        if let Err(error) = planet_gen::run_dump(std::path::Path::new(output), which) {
            eprintln!("Planet dump: {error}");
            std::process::exit(2);
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--dump-cell-data") {
        let output = args.get(i + 1).expect("--dump-cell-data requires an output directory");
        if let Err(error) = cell_data::run_dump(std::path::Path::new(output)) {
            eprintln!("Cell data: {error}");
            std::process::exit(2);
        }
        return;
    }
    if std::env::args().any(|a| a == "--dump-skinpaint-shaders") {
        shader_pack::run_dump_skinpaint();
        return;
    }
    if std::env::args().any(|a| a == "--audit-world-images") {
        resource_audit::run_world_images();
        return;
    }
    if std::env::args().any(|a| a == "--audit-shader-data") {
        shader_pack::run_audit();
        return;
    }
    if std::env::args().any(|a| a == "--audit-world-data") {
        world_catalog::run_audit();
        return;
    }
    if std::env::args().any(|a| a == "--audit-material-data") {
        material_audit::run();
        return;
    }
    if std::env::args().any(|a| a == "--audit-animation-data") {
        animation_catalog::run_audit();
        return;
    }
    if std::env::args().any(|a| a == "--dump-properties") {
        resource_audit::run_properties();
        return;
    }
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == "--quick-capture" {
            let output = args
                .next()
                .expect("--quick-capture requires an output directory");
            runtime::run_quick_capture(std::path::Path::new(&output));
            return;
        }
        if arg == "--render-smoke" {
            let output = args
                .next()
                .expect("--render-smoke requires an output directory");
            runtime::run_render_smoke(std::path::Path::new(&output));
            return;
        }
    }
    if std::env::args().any(|a| a == "--smoke-test") {
        runtime::run_smoke_test();
        return;
    }
    if std::env::args().any(|a| a == "--dump-part-catalog") {
        run_dump_part_catalog();
        return;
    }
    if std::env::args().any(|a| a == "--dump-creation-records") {
        run_dump_creation_records();
        return;
    }
    if std::env::args().any(|a| a == "--dump-creation-morphs") {
        run_dump_creation_morphs();
        return;
    }
    if let Some((t, n)) = probe_args() {
        run_probe(t, n);
        return;
    }
    if std::env::args().any(|a| a == "--extract-png") {
        run_extract_png();
        return;
    }
    if std::env::args().any(|a| a == "--dump-strings") {
        run_dump_strings();
        return;
    }
    if std::env::args().any(|a| a == "--dump-registry") {
        run_dump_registry();
        return;
    }
    if std::env::args().any(|a| a == "--dump-rasters") {
        run_dump_rasters();
        return;
    }
    if std::env::args().any(|a| a == "--dump-meshes") {
        run_dump_meshes();
        return;
    }
    if let Some(inst) = extract_obj_arg() {
        run_extract_obj(inst);
        return;
    }
    if std::env::args().any(|a| a == "--dump-gmdl") {
        run_dump_gmdl();
        return;
    }
    if std::env::args().any(|a| a == "--dump-gmdl-parts") {
        run_dump_gmdl_parts();
        return;
    }
    if std::env::args().any(|a| a == "--dump-gmdl-rigs") {
        run_dump_gmdl_rigs();
        return;
    }
    if std::env::args().any(|a| a == "--dump-anims") {
        run_dump_anims();
        return;
    }
    if std::env::args().any(|a| a == "--dump-anim-channels") {
        run_dump_anim_channels();
        return;
    }
    if std::env::args().any(|a| a == "--dump-anim-keys") {
        run_dump_anim_keys();
        return;
    }
    if std::env::args().any(|a| a == "--dump-anim-selectors") {
        run_dump_anim_selectors();
        return;
    }
    if std::env::args().any(|a| a == "--dump-skeletons") {
        run_dump_skeletons();
        return;
    }
    if std::env::args().any(|a| a == "--census-rigs") {
        run_census_rigs();
        return;
    }
    if let Some(id) = vdecl_arg() {
        run_dump_vdecl(id);
        return;
    }
    if let Some(id) = rigcheck_arg() {
        run_rigcheck(id);
        return;
    }
    if std::env::args().any(|a| a == "--dump-skin") {
        run_dump_skin();
        return;
    }
    if std::env::args().any(|a| a == "--dump-rigs") {
        run_dump_rigs();
        return;
    }
    if std::env::args().any(|a| a == "--dump-keyframes") {
        run_dump_keyframes();
        return;
    }
    if std::env::args().any(|a| a == "--dump-audio") {
        run_dump_audio();
        return;
    }
    if let Some(inst) = extract_wav_arg() {
        run_extract_wav(inst);
        return;
    }
    if let Some(inst) = extract_mp3_arg() {
        run_extract_mp3(inst);
        return;
    }
    if std::env::args().any(|a| a == "--dump-eal3") {
        run_dump_eal3();
        return;
    }
    if scan_only_mode() {
        run_scan_only();
        return;
    }
    if std::env::args().any(|a| a == "--detect-spore") {
        std::process::exit(first_run::detect());
    }
    // No Spore install found (or --choose-spore): ask for the folder first.
    if let first_run::Outcome::Quit = first_run::before_window() {
        return;
    }
    let saved = RunProgress::load();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::render::RenderPlugin {
                render_creation: renderer_settings().into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "OpenSpore — needs original Spore assets".into(),
                    ..default()
                }),
                ..default()
            }),
    );
    configure_game(&mut app, saved);
    app.add_plugins((
        water::WaterPlugin,
        terrain::TerrainPlugin,
        world_presentation::PreviewSkyPlugin,
        spui::surface::SpuiSurfacePlugin,
    ));
    add_startup(&mut app);
    app.run();
}

fn add_startup(app: &mut App) {
    app.add_plugins(game_audio::GameAudioPlugin);
    app.add_plugins(cell_audio::CellAudioPlugin);
    app.add_plugins(audio_audit::AuditAudioPlugin);
    app.add_plugins(loop_audio::LoopAudioPlugin);
    app.add_plugins(ui_sounds::UiSoundsPlugin);
    app.add_plugins(civ_voices::CivVoicesPlugin);
    app.add_plugins(civ_target::CivTargetPlugin);
    app.add_plugins(space_voices::SpaceVoicesPlugin);
    app.add_plugins(space_weapon_fx::SpaceWeaponFxPlugin);
    app.add_plugins(space_tool_fx::SpaceToolFxPlugin);
    app.add_plugins(tribal_audio::TribalAudioPlugin);
    app.add_plugins(creature_audio::CreatureAudioPlugin);
    app.add_plugins(civ_audio::CivAudioPlugin);
    app.add_plugins(pd_audio::PdAudioPlugin);
    app.add_plugins(anim_audio::AnimAudioPlugin);
    app.add_plugins(voice_banks::VoiceBanksPlugin);
    app.add_plugins(effect_audio::EffectAudioPlugin);
    app.add_plugins(swarm_fx::SwarmFxRenderPlugin);
    app.add_systems(
        Startup,
        (
            (
                locate_and_scan,
                editor::load_part_catalog,
                skinpaint::load_at_startup,
                load_originals,
                load_original_textures,
                load_original_models,
                native_ui::load,
                load_gameplay_originals,
                load_original_anims,
                load_original_skeleton,
                load_skinned_demo,
                load_original_rigged,
                load_prop_rigs,
                load_clip_previews,
                default_slot_bone,
                game_audio::start,
            )
                .chain(),
            (
                setup,
                enter_menu,
                spawn_menu_scene,
                menu_visuals::spawn_menu_galaxy_art,
                spawn_menu_models,
                spawn_menu_skeleton,
                spawn_morph_demo,
                spawn_skinned_demo,
                spawn_rigged_demo,
                spawn_clip_preview,
                mark_booted,
            )
                .chain(),
        )
            .chain(),
    );
}

fn renderer_settings() -> bevy::render::settings::WgpuSettings {
    let mut settings = bevy::render::settings::WgpuSettings::default();
    // Fresh offscreen checks on the local NVIDIA Windows driver exposed
    // intermittent DX12 queue timeouts; the same scenes pass with Vulkan.
    // Keep WGPU_BACKEND available for an explicit user/platform override.
    if cfg!(target_os = "windows") && std::env::var_os("WGPU_BACKEND").is_none() {
        settings.backends = Some(bevy::render::settings::Backends::VULKAN);
    }
    settings
}

#[derive(Resource)]
struct StartupComplete;

fn mark_booted(mut commands: Commands) {
    commands.insert_resource(StartupComplete);
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum GameSystems {
    Controls,
    Simulation,
    Grounding,
    Presentation,
}

/// Shared schedule: the desktop game and headless tests run the same systems.
fn configure_game(app: &mut App, saved: RunProgress) -> &mut App {
    app.init_state::<GameStage>()
        .init_resource::<InstallStatus>()
        .init_resource::<Catalog>()
        .init_resource::<HudOptions>()
        .init_resource::<Originals>()
        .init_resource::<native_ui::OriginalFont>()
        .init_resource::<Assets<water::WaterMaterial>>()
        .init_resource::<Assets<terrain::TerrainMaterial>>()
        .init_resource::<player_hud::InjectedKeys>()
        .init_resource::<editor::EditorState>()
        .init_resource::<mechanical_creator::MechanicalCreator>()
        .init_resource::<mechanical_environment::MechanicalEnvironmentCache>()
        .init_resource::<mechanical_preview::PreviewScene>()
        .init_resource::<pause_menu::PauseMenu>()
        .init_resource::<locomotion_preview::WalkTemplates>()
        .init_resource::<foliage_visibility::FloraBounds>()
        .init_resource::<part_thumbnails::PartThumbnailCache>()
        .init_resource::<menu_visuals::MenuVisualCache>()
        .init_resource::<editor_environment::EditorEnvironmentAssets>()
        .init_resource::<play_camera::PlayCamera>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<bevy::input::mouse::MouseMotion>()
        .init_resource::<persistence::SaveState>()
        .insert_resource(saved)
        .init_resource::<cell::CellRng>()
        .init_resource::<cell::CellSpawnTimer>()
        .init_resource::<cell::CellWeapon>()
        .init_resource::<cell::CellMating>()
        .init_resource::<cell::CellNotice>()
        .init_resource::<cell::CellIntro>()
        .add_plugins(cell_anim::CellAnimPlugin)
        .init_resource::<creature::CreatureSpawnTimer>()
        .init_resource::<creature::CreatureStatsRes>()
        .init_resource::<creature_actions::Encounter>()
        .add_plugins(creature_social::CreatureGameplayPlugin)
        .add_plugins(creature_controls::CreatureControlsPlugin)
        .add_plugins(tribal_controls::TribalControlsPlugin)
        .add_plugins(civ_controls::CivControlsPlugin)
        .add_plugins(civ_cursor::CivCursorPlugin)
        .add_plugins(civ_orders::CivOrdersPlugin)
        .add_plugins(space_controls::SpaceControlsPlugin)
        .add_plugins(space_audio::SpaceAudioPlugin)
        .add_plugins(creature_rules::CreatureRulesPlugin)
        .add_plugins((creature_flow::CreatureFlowPlugin, creature_music::CreatureMusicPlugin))
        .add_plugins(civ_wildlife::CivWildlifePlugin)
        .add_plugins(civ_comm::CivCommPlugin)
        .add_plugins(tribal_rules::TribalRulesPlugin)
        .add_plugins(space_rules::SpaceRulesPlugin)
        .add_plugins((swarm_fx::SwarmFxPlugin, cell_fx::CellFxPlugin))
        .add_plugins(spui::view::SpuiPlugin)
        .add_plugins(cell_hud::CellHudPlugin)
        .add_plugins(creature_hud::CreatureHudPlugin)
        .add_plugins(land_minimap::LandMinimapPlugin)
        .add_plugins(tribal_hud::TribalHudPlugin)
        .add_plugins(civ_hud::CivHudPlugin)
        .add_plugins(civ_planner::CivPlannerPlugin)
        .add_plugins(civ_anthem::CivAnthemPlugin)
        .add_plugins(civ_decor::CivDecorPlugin)
        .add_plugins(civ_fx::CivFxPlugin)
        .add_plugins(civ_view::CivViewPlugin)
        .add_plugins(space_hud::SpaceHudPlugin)
        .add_plugins(space_universe::SpaceUniversePlugin)
        .add_plugins(space_endgame::SpaceEndgamePlugin)
        .add_plugins(front_end::FrontEndPlugin)
        .add_plugins(cell_editor::CellEditorPlugin)
        .add_plugins(creature_editor::CreatureEditorPlugin)
        .add_plugins(outfitter::OutfitterPlugin)
        .add_plugins(outfit_crowd::OutfitCrowdPlugin)
        .add_plugins((tribal_village::TribalVillagePlugin, tribal_vignettes::TribalVignettesPlugin))
        .add_plugins(rigid_editor::RigidEditorPlugin)
        .add_plugins(creature_skin_view::CreatureSkinPlugin)
        .add_plugins(creature_limbs::CreatorLimbsPlugin)
        .add_plugins(creature_clips::ClipPlayerPlugin)
        .add_plugins(cinematics::CinematicsPlugin)
        .init_resource::<ClipPick>()
        .init_resource::<PropRigPick>()
        .init_resource::<tribal::TribalTimer>()
        .init_resource::<civ::CivTimer>()
        .init_resource::<civ::CivTints>()
        .init_resource::<tribal::TribalOutfitSync>()
        .init_resource::<space::SpaceTimer>()
        .insert_resource(ClearColor(GameStage::MainMenu.accent()))
        .add_systems(
            OnEnter(GameStage::MainMenu),
            (
                enter_menu,
                spawn_menu_scene,
                menu_visuals::spawn_menu_galaxy_art,
                spawn_menu_models,
                spawn_menu_skeleton,
                spawn_morph_demo,
                spawn_skinned_demo,
                spawn_rigged_demo,
                spawn_clip_preview,
                spawn_prop_rig_preview,
            )
                .chain()
                .run_if(resource_exists::<StartupComplete>),
        )
        .add_systems(
            OnEnter(GameStage::Cell),
            (
                editor::prepare_stage,
                cell::setup_cell,
                persistence::restore,
                editor::refresh_parts,
            )
                .chain(),
        )
        .add_systems(
            OnEnter(GameStage::Creature),
            (
                editor::prepare_stage,
                creature::prepare_species,
                creature::setup_creature,
                persistence::restore,
                creature::spawn_creature_parts,
                terrain::prepare,
                creature_home::setup,
                creature_parts::setup,
                cell::creator_after_landfall,
            )
                .chain(),
        )
        .add_systems(
            OnEnter(GameStage::Tribal),
            (
                editor::prepare_stage,
                tribal::setup_tribal,
                persistence::restore,
                terrain::prepare,
            )
                .chain(),
        )
        .add_systems(
            OnEnter(GameStage::Civilization),
            (
                editor::prepare_stage,
                civ::setup_civ,
                persistence::restore,
                terrain::prepare,
            )
                .chain(),
        )
        .add_systems(
            OnEnter(GameStage::Space),
            (
                editor::prepare_stage,
                space::setup_space,
                persistence::restore,
            )
                .chain(),
        )
        .add_systems(OnExit(GameStage::MainMenu), despawn_stage)
        .add_systems(OnExit(GameStage::Cell), despawn_stage)
        .add_systems(OnExit(GameStage::Creature), despawn_stage)
        .add_systems(
            OnExit(GameStage::Tribal),
            (tribal::tribal_stage_exit, despawn_stage),
        )
        .add_systems(OnExit(GameStage::Civilization), despawn_stage)
        .add_systems(
            OnExit(GameStage::Space),
            (despawn_stage, space_view::restore_camera),
        )
        // Global
        .configure_sets(
            Update,
            (
                GameSystems::Controls,
                GameSystems::Simulation,
                GameSystems::Grounding,
                GameSystems::Presentation,
            )
                .chain(),
        )
        .configure_sets(
            Update,
            GameSystems::Simulation
                .run_if(editor::simulation_active)
                .run_if(mechanical_creator::simulation_active)
                .run_if(pause_menu::simulation_active)
                .run_if(cinematics::simulation_active),
        )
        .add_systems(
            Update,
            terrain::ground
                .in_set(GameSystems::Grounding)
                .run_if(editor::simulation_active)
                .run_if(mechanical_creator::simulation_active)
                .run_if(pause_menu::simulation_active),
        )
        .add_systems(
            Update,
            (
                player_hud::actions,
                pause_menu::input,
                (mechanical_creator::input, mechanical_preview::orbit).chain(),
                menu_keys,
                creature_lifecycle_controls.run_if(pause_menu::simulation_active),
                hud_keys,
                editor::input.run_if(pause_menu::simulation_active),
                editor::scene,
                editor::drag,
                editor::scroll,
                editor::orbit,
                editor::refresh_parts,
                editor::ui,
                play_camera::update_play_camera_input.run_if(mechanical_creator::simulation_active),
                creature_home::update.run_if(pause_menu::simulation_active),
            )
                .chain()
                .in_set(GameSystems::Controls),
        )
        .add_systems(
            Update,
            (
                camera_follow,
                spin_props,
                editor_stub.run_if(mechanical_creator::simulation_active),
                animate_morphs,
                animate_skin,
                animate_rigged,
                animate_prop_rig,
                animate_clip_preview,
                clip_pick_keys,
                rigged_mount_sync,
                rigged_assembly_keys,
                update_hud,
                native_ui::apply,
                menu_presentation,
                menu_visuals::menu_visual_presentation.after(menu_presentation),
                navigation::update_map,
                water::animate,
            )
                .in_set(GameSystems::Presentation),
        )
        .add_systems(Last, persistence::checkpoint_and_save)
        .add_systems(Update, stage_restart::autosave)
        .add_systems(
            Last,
            player_hud::release.after(persistence::checkpoint_and_save),
        )
        .add_systems(
            Update,
            (
                player_hud::update,
                pause_menu::ui,
                mechanical_creator::ui,
                city_presentation::sync,
                city_presentation::ground_halls,
                spice_presentation::sync,
                (space_visuals::prepare, space_visuals::update).chain(),
                world_presentation::update,
                creature_selection::update,
                player_appearance::sync.after(editor::refresh_parts),
                locomotion_preview::update
                    .after(player_appearance::sync)
                    .run_if(pause_menu::simulation_active),
                editor_environment::sync.after(editor::scene),
                part_thumbnails::update_requests
                    .after(editor::ui)
                    .after(mechanical_creator::ui),
                foliage_visibility::update
                    .after(camera_follow)
                    .after(editor::scene),
            )
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            Update,
            cell_parallax::update
                .run_if(in_state(GameStage::Cell))
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            Update,
            (
                cell_anim::load_clip_times,
                cell_anim::attach_visuals,
                cell_anim::animate,
            )
                .chain()
                .after(cell_flow::animate)
                .run_if(in_state(GameStage::Cell))
                .run_if(pause_menu::simulation_active)
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            PostUpdate,
            (mechanical_preview::sync, mechanical_environment::sync)
                .chain()
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        )
        .add_systems(
            Update,
            asteroid_visuals::replace
                .run_if(in_state(GameStage::Space))
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            Update,
            pirate_visuals::mount_original_pirate_visuals
                .run_if(in_state(GameStage::Space))
                .before(material_binding::attach_remaining)
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            PostUpdate,
            asteroid_visuals::face_camera
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility)
                .run_if(in_state(GameStage::Space)),
        )
        .add_systems(
            Update,
            civ::sync_creator_assemblies
                .run_if(in_state(GameStage::Civilization))
                .run_if(mechanical_creator::is_open.or_else(rigid_editor::sync_pending))
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            Update,
            space::sync_creator_loadout
                .run_if(in_state(GameStage::Space))
                .run_if(mechanical_creator::is_open.or_else(rigid_editor::sync_pending))
                .in_set(GameSystems::Presentation),
        )
        .add_systems(
            Update,
            (
                material_binding::attach_remaining,
                material_binding::sync_surface_tints,
            )
                .chain()
                .in_set(GameSystems::Presentation),
        )
        // Cell: the retail engine registers its own systems (cell_engine.rs).
        .add_plugins(cell_engine::register)
        // Creature
        .add_systems(
            Update,
            (
                creature::creature_walk,
                creature::creature_wander,
                creature::creature_graze,
                creature_actions::update,
                creature::creature_respawn_nests,
                creature::accessory_follow,
                creature::part_editor,
                creature_parts::collect,
            )
                .chain()
                .in_set(GameSystems::Simulation)
                .run_if(in_state(GameStage::Creature)),
        )
        // Creature: the player's own species at the nest (creature_nestmates.rs).
        .add_plugins(creature_nestmates::NestMatesPlugin)
        // Tribal
        .add_systems(
            Update,
            (
                tribal_rts::tribal_palette_clicks,
                tribal_rts::tribal_input,
                tribal_rts::tribal_units_think,
                tribal_rts::livestock::update,
                tribal::tribal_editor,
                tribal_rts::tribal_wildlife,
                tribal_rts::tribal_rivals,
                tribal_rts::tribal_combat,
                tribal::tribal_cleanup,
                tribal::tribal_campaign_update,
                tribal::tribal_outfits_sync,
                tribal_tools::tribal_tools_sync,
            )
                .chain()
                .in_set(GameSystems::Simulation)
                .run_if(in_state(GameStage::Tribal)),
        )
        .add_systems(
            Update,
            tribal_rts::tribal_presentation
                .in_set(GameSystems::Presentation)
                .run_if(in_state(GameStage::Tribal)),
        )
        // Civ
        .add_systems(
            Update,
            (
                civ::civ_buy_engine,
                civ::civ_vehicle_editor,
                civ_rts::rts_input,
                civ::civ_city_commands,
                // A forced tutorial hint or a callout holds the game like the
                // retail pause (`civ_hud::guide::world_runs`); input stays live.
                civ_rts::rts_ai.run_if(civ_hud::guide::world_runs),
                civ_rts::rts_spawn.run_if(civ_hud::guide::world_runs),
                civ_rts::rts_move.run_if(civ_hud::guide::world_runs),
                civ_rts::rts_combat.run_if(civ_hud::guide::world_runs),
                civ_rts::rts_shots.run_if(civ_hud::guide::world_runs),
                civ_rts::rts_cleanup.run_if(civ_hud::guide::world_runs),
                civ::civ_campaign_update.run_if(civ_hud::guide::world_runs),
                civ::civ_city_assemblies,
            )
                .chain()
                .in_set(GameSystems::Simulation)
                .run_if(in_state(GameStage::Civilization)),
        )
        .add_systems(
            Update,
            (
                civ_rts::paint_flagship,
                civ_rts::present_units,
                civ_rts::present_fx,
                civ_rts::present_actions,
                civ_rts::present_labels,
                civ_reveal::sync_visibility,
                city_presentation::sync_plots,
                city_presentation::sync_turrets,
                spice_presentation::sync_derricks,
                spice_presentation::animate_plumes,
            )
                .chain()
                .in_set(GameSystems::Presentation)
                .run_if(in_state(GameStage::Civilization)),
        )
        // Space
        .add_systems(
            Update,
            (
                space_view::camera,
                space_view::lighting,
                space_scene::present,
                space_scene::dress_bodies,
                space_scene::planet_labels,
                space_galaxy::present_galaxy,
            )
                .chain()
                .after(camera_follow)
                .after(world_presentation::update)
                .in_set(GameSystems::Presentation)
                .run_if(in_state(GameStage::Space)),
        )
        .add_systems(
            Update,
            (
                space_view::zoom_input,
                space_galaxy::galaxy_controls,
                space_galaxy::arrive_in_system,
                space::space_orbits,
                space::space_fly,
                space::space_survey,
                space::space_tractor_beam,
                space::space_superweapon,
                space::space_fire,
                space::space_escort_combat,
                space::space_colony_turrets,
                space::space_projectiles,
                space::space_pirates,
                space::space_colonize,
                space::space_city_planning,
                space::space_ship_editor,
                space_tool_use::space_self_tools,
                space::space_campaign_update,
            )
                .chain()
                .in_set(GameSystems::Simulation)
                .run_if(in_state(GameStage::Space)),
        )
}
