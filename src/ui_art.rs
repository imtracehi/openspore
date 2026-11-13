//! Source-key backed pieces of original Spore UI art.
//!
//! This module imports only textures whose PNG resource key and registered
//! file name have both been verified in the installed package index. Native
//! action hint `hintIcon` keys use type zero; their exact group+instance pairs
//! resolve to individual indexed PNGs. Named normal-state HUD icons provide
//! the action mapping, including Strike, whose hint PROP points at Bite art.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::io::Cursor;

use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey};

pub const UI_PNG_TYPE: u32 = 0x2F7D_0004;
pub const GLOBAL_UI_GROUP: u32 = 0x011C_0BF5;
pub const CREATURE_HINT_GROUP: u32 = 0x0522_DE06;
pub const CREATURE_ACTION_ICON_GROUP: u32 = 0x604B_35E7;
pub const CREATURE_HUD_GROUP: u32 = 0x106C_7116;
pub const SHARED_CREATURE_UI_GROUP: u32 = 0x31A4_4893;
/// Installed image types used when resolving the native `hintIcon` key's
/// exact group and instance. No group or instance substitutions are tried.
pub const NATIVE_IMAGE_TYPES: &[u32] = &[
    UI_PNG_TYPE,
    0x2F7D_0002, // JPEG
    0x2F4E_681C, // RW4 raster
    0x2F4E_681B, // RW4 model/texture container
    0x2F7D_0005, // BMP
    0x2F7D_0006, // TGA
    0x2F7D_0007, // GIF
    0x1795_2E6C, // DDS
    0x03E4_21E9, // bit image
    0x03E4_21EC,
    0x03E4_21ED,
    0x03E4_21EF,
];

/// Installed game PNGs, verified by their registered `reg_file.txt` name,
/// native instance hash, resource type, and group in Spore_Graphics.package.
pub const NATIVE_UI_TEXTURES: &[NativeUiTexture] = &[
    NativeUiTexture {
        name: "title-logo",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x02E9A2CA,
            instance_id: 0xC0F8FE6B,
        },
    },
    NativeUiTexture {
        name: "verb-tray-buttons-hm",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: GLOBAL_UI_GROUP,
            instance_id: 0xCA79_0169,
        },
    },
    NativeUiTexture {
        name: "clg-editor-tray",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: GLOBAL_UI_GROUP,
            instance_id: 0x3638_3B84,
        },
    },
    NativeUiTexture {
        name: "atk_tray_norm",
        key: action_icon_key(0x6733_E6B1),
    },
    NativeUiTexture {
        name: "atk_tray_sel_norm",
        key: action_icon_key(0x7642_AE2A),
    },
    NativeUiTexture {
        name: "social_tray_norm",
        key: action_icon_key(0x2266_EB18),
    },
    NativeUiTexture {
        name: "social_tray_sel_norm",
        key: action_icon_key(0xBC55_1B9B),
    },
    // Gameplay HUD horizontal health, hunger, and energy bars. Each native
    // texture is a 16-state strip; this importer intentionally keeps it whole.
    NativeUiTexture {
        name: "crg-ingame_hp-hrz",
        key: creature_hud_key(0x4365_3213),
    },
    NativeUiTexture {
        name: "crg-ingame_hunger-hrz",
        key: creature_hud_key(0x7634_C43A),
    },
    NativeUiTexture {
        name: "crg-ingame_energy-hrz",
        key: creature_hud_key(0xE509_E42F),
    },
    NativeUiTexture {
        name: "crg_ingame_hp",
        key: creature_hud_key(0x1666_DCC2),
    },
    NativeUiTexture {
        name: "crg_ingame_hp_selected",
        key: creature_hud_key(0xED30_881E),
    },
    // Creature progression/DNA meter background, end caps, level marker,
    // shadow, and native brain-level markers. No fill image was found under
    // the installed crg_progress-dna-full/empty registry names.
    NativeUiTexture {
        name: "crg-p-bar-bkgrnd",
        key: creature_hud_key(0xE1BC_270F),
    },
    NativeUiTexture {
        name: "crg-p-bar-shadow",
        key: creature_hud_key(0x139A_1AC3),
    },
    NativeUiTexture {
        name: "crg-p-bar-rounding-left",
        key: creature_hud_key(0x9368_9E23),
    },
    NativeUiTexture {
        name: "crg-p-bar-rounding-right",
        key: creature_hud_key(0x3D2B_4E5E),
    },
    NativeUiTexture {
        name: "crg-p-bar-level-notch",
        key: creature_hud_key(0xE9D0_F486),
    },
    NativeUiTexture {
        name: "crg-p-bar-brain-1",
        key: creature_hud_key(0xCABF_2733),
    },
    NativeUiTexture {
        name: "crg-p-bar-brain-2",
        key: creature_hud_key(0xCABF_2730),
    },
    NativeUiTexture {
        name: "crg-p-bar-brain-3",
        key: creature_hud_key(0xCABF_2731),
    },
    NativeUiTexture {
        name: "crg-p-bar-brain-4",
        key: creature_hud_key(0xCABF_2736),
    },
    // Registered Creature-game tray tabs/backgrounds and bottom HUD panel
    // frame pieces. These are original panels, not editor art.
    NativeUiTexture {
        name: "crg-vrbtray-fight",
        key: creature_hud_key(0x4404_B931),
    },
    NativeUiTexture {
        name: "crg-vrbtray-social",
        key: creature_hud_key(0x9D27_2274),
    },
    NativeUiTexture {
        name: "crg-GUI-bkgrnd",
        key: creature_hud_key(0x50AC_F890),
    },
    NativeUiTexture {
        name: "crg-gui-btmPanel-leftIndent",
        key: creature_hud_key(0xA89F_EA05),
    },
    NativeUiTexture {
        name: "crg-gui-btmPanel-mid",
        key: creature_hud_key(0x2DE6_448E),
    },
    NativeUiTexture {
        name: "crg-gui-btmPanel-sideTile",
        key: creature_hud_key(0x1D0D_DEB5),
    },
    NativeUiTexture {
        name: "crg-gui-btmPanel-lftDip",
        key: creature_hud_key(0xBAE0_BC4D),
    },
    NativeUiTexture {
        name: "crg-gui-btmPanel-rgtDip",
        key: creature_hud_key(0x1BBA_DFDE),
    },
    // Civilization stage action header glyphs. These are three separately
    // registered, source-key-backed images from the same native UI family.
    NativeUiTexture {
        name: "icon_headerMilitary",
        key: civilization_ui_key(0x8C0A_0D3B),
    },
    NativeUiTexture {
        name: "icon_headerEconomic",
        key: civilization_ui_key(0xD839_BAFD),
    },
    NativeUiTexture {
        name: "icon_headerReligious",
        key: civilization_ui_key(0x8055_187F),
    },
    // Native health-meter state strips are kept intact; their atlas regions
    // have not been reverse-mapped to UV coordinates.
    NativeUiTexture {
        name: "NoGroup_CreatureInGameHP-01",
        key: shared_creature_ui_key(0xC132_7980),
    },
    NativeUiTexture {
        name: "NoGroup_CreatureInGameHP-02",
        key: shared_creature_ui_key(0xC132_7983),
    },
];

/// Additional standalone stage HUD PNGs verified by exact name and key in the
/// installed resource scan. Only unambiguous PNG records are listed here;
/// similarly named RW4 resources and unresolved expansion names are omitted.
pub const STAGE_HUD_UI_TEXTURES: &[NativeUiTexture] = &[
    NativeUiTexture {
        name: "level-icon-cell",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x02E9_A2CA,
            instance_id: 0xAABD_531E,
        },
    },
    NativeUiTexture {
        name: "cvg_levelicon",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x011C_0BF5,
            instance_id: 0x8A3E_402D,
        },
    },
    NativeUiTexture {
        name: "spg-icon-mission",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x011C_0BDE,
            instance_id: 0x29F4_5B82,
        },
    },
    NativeUiTexture {
        name: "spg-icon-health",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x011C_0BDE,
            instance_id: 0x843E_A01A,
        },
    },
    NativeUiTexture {
        name: "spg-gui-btn-attack-nml",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x0354_663B,
            instance_id: 0x5D24_FDE9,
        },
    },
    NativeUiTexture {
        name: "spg-gui-btn-terra-nml",
        key: ResourceKey {
            type_id: UI_PNG_TYPE,
            group_id: 0x0354_663B,
            instance_id: 0x987F_64D1,
        },
    },
];

/// Original normal-state Creature action button icons. Names are registered
/// in `reg_file.txt` and each exact PNG key was found in Spore_Graphics.package.
pub const ACTION_ICON_TEXTURES: &[ActionIconTexture] = &[
    ActionIconTexture {
        action: "Bite",
        name: "atk_bite_norm",
        key: action_icon_key(0x61D8_0033),
    },
    ActionIconTexture {
        action: "Charge",
        name: "atk_charge_norm",
        key: action_icon_key(0x447E_4ACD),
    },
    ActionIconTexture {
        action: "Strike",
        name: "atk_strike_norm",
        key: action_icon_key(0x5D5E_4A83),
    },
    ActionIconTexture {
        action: "Spit",
        name: "atk_spit_norm",
        key: action_icon_key(0x889D_BB77),
    },
    ActionIconTexture {
        action: "Sing",
        name: "social_vocal_norm",
        key: action_icon_key(0x4378_B0F3),
    },
    ActionIconTexture {
        action: "Dance",
        name: "social_dance_norm",
        key: action_icon_key(0x20AB_8AF9),
    },
    ActionIconTexture {
        action: "Charm",
        name: "social_gest_norm",
        key: action_icon_key(0xBE68_0A93),
    },
    ActionIconTexture {
        action: "Pose",
        name: "social_pose_norm",
        key: action_icon_key(0x9485_2301),
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeUiTexture {
    pub name: &'static str,
    pub key: ResourceKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionIconTexture {
    pub action: &'static str,
    pub name: &'static str,
    pub key: ResourceKey,
}

const fn action_icon_key(instance_id: u32) -> ResourceKey {
    ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id: CREATURE_ACTION_ICON_GROUP,
        instance_id,
    }
}

const fn creature_hud_key(instance_id: u32) -> ResourceKey {
    ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id: CREATURE_HUD_GROUP,
        instance_id,
    }
}

const fn shared_creature_ui_key(instance_id: u32) -> ResourceKey {
    ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id: SHARED_CREATURE_UI_GROUP,
        instance_id,
    }
}

const fn civilization_ui_key(instance_id: u32) -> ResourceKey {
    ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id: 0xCA14_DE92,
        instance_id,
    }
}

/// Native creature hint PROP that corresponds to one creature action.
/// `key` identifies the source PROP; it is not an image key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionHintSource {
    pub action: &'static str,
    pub key: ResourceKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionHintIconRef {
    pub action: &'static str,
    pub hint_source: ResourceKey,
    /// Native PROP `hintIcon` value, retained verbatim as a key. It is not
    /// assumed to be a DBPF PNG until its type and record are verified.
    pub native_reference: Option<ResourceKey>,
    /// Exact indexed image key found by retaining the native reference's
    /// group and instance and checking known image types.
    pub texture_source: Option<ResourceKey>,
    pub error: Option<String>,
}

/// Original creature hint records that cover the eight HUD actions. Charm is
/// named `gesture` in the installed hint file; its native hint text says
/// “learn to charm”. Pose uses the installed `posture` hint record.
pub const ACTION_HINT_SOURCES: &[ActionHintSource] = &[
    ActionHintSource {
        action: "Bite",
        key: hint_key(0xEFF9_F865),
    },
    ActionHintSource {
        action: "Charge",
        key: hint_key(0x256D_59FB),
    },
    ActionHintSource {
        action: "Strike",
        key: hint_key(0x45AA_9FE5),
    },
    ActionHintSource {
        action: "Spit",
        key: hint_key(0xB6AC_D709),
    },
    ActionHintSource {
        action: "Sing",
        key: hint_key(0xB1EB_383A),
    },
    ActionHintSource {
        action: "Dance",
        key: hint_key(0x5D09_0C04),
    },
    ActionHintSource {
        action: "Charm",
        key: hint_key(0x9E80_5E6C),
    },
    ActionHintSource {
        action: "Pose",
        key: hint_key(0xF946_11D9),
    },
];

const fn hint_key(instance_id: u32) -> ResourceKey {
    ResourceKey {
        type_id: crate::asset_catalog::PROP,
        group_id: CREATURE_HINT_GROUP,
        instance_id,
    }
}

/// Read the exact installed native hint-icon references for the eight
/// creature actions. Missing or malformed PROP records are returned as
/// per-action errors so callers can show an explicit absence.
pub fn action_hint_icon_refs(index: &ResourceIndex) -> Vec<ActionHintIconRef> {
    ACTION_HINT_SOURCES
        .iter()
        .map(|source| match index.properties(source.key) {
            Ok(properties) => match properties
                .0
                .get(&0x0522_DE10)
                .and_then(crate::prop::Property::key)
            {
                Some(native_reference) => {
                    let matching_sources = NATIVE_IMAGE_TYPES
                        .iter()
                        .map(|type_id| ResourceKey {
                            type_id: *type_id,
                            group_id: native_reference.group_id,
                            instance_id: native_reference.instance_id,
                        })
                        .filter(|candidate| index.records.contains_key(candidate))
                        .collect::<Vec<_>>();
                    let (texture_source, error) = match matching_sources.as_slice() {
                        [source] => (Some(*source), None),
                        [] => (
                            None,
                            Some("no indexed image type matches the exact hintIcon group and instance".into()),
                        ),
                        _ => (
                            None,
                            Some(format!(
                                "ambiguous image types for exact hintIcon group and instance: {:?}",
                                matching_sources
                                    .iter()
                                    .map(|key| key.type_id)
                                    .collect::<Vec<_>>()
                            )),
                        ),
                    };
                    ActionHintIconRef {
                        action: source.action,
                        hint_source: source.key,
                        native_reference: Some(native_reference),
                        texture_source,
                        error,
                    }
                }
                None => ActionHintIconRef {
                    action: source.action,
                    hint_source: source.key,
                    native_reference: None,
                    texture_source: None,
                    error: Some("native hint PROP has no valid hintIcon key".into()),
                },
            },
            Err(error) => ActionHintIconRef {
                action: source.action,
                hint_source: source.key,
                native_reference: None,
                texture_source: None,
                error: Some(error),
            },
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ImportedUiTexture {
    pub name: &'static str,
    pub source: ResourceKey,
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct ImportedActionIcon {
    pub action: &'static str,
    pub hint_source: Option<ResourceKey>,
    pub source: ResourceKey,
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default)]
pub struct UiArtCatalog {
    pub textures: Vec<ImportedUiTexture>,
    pub action_icons: Vec<ImportedActionIcon>,
    pub errors: Vec<String>,
}

/// Load verified original UI PNG resources into Bevy's image store. Each
/// returned image contains the full native texture, with original alpha and
/// dimensions intact; callers can apply UV cropping if a native layout proves
/// that a texture is an atlas.
pub fn load(index: &ResourceIndex, images: &mut Assets<Image>) -> UiArtCatalog {
    let mut catalog = UiArtCatalog::default();
    for source in NATIVE_UI_TEXTURES {
        match import_png(index, source.key, images) {
            Ok((image, width, height)) => catalog.textures.push(ImportedUiTexture {
                name: source.name,
                source: source.key,
                image,
                width,
                height,
            }),
            Err(error) => catalog.errors.push(format!(
                "{} ({:08X}!{:08X}.{:08X}): {error}",
                source.name, source.key.group_id, source.key.instance_id, source.key.type_id
            )),
        }
    }
    for source in ACTION_ICON_TEXTURES {
        match import_png(index, source.key, images) {
            Ok((image, width, height)) => catalog.action_icons.push(ImportedActionIcon {
                action: source.action,
                hint_source: ACTION_HINT_SOURCES
                    .iter()
                    .find(|hint| hint.action == source.action)
                    .map(|hint| hint.key),
                source: source.key,
                image,
                width,
                height,
            }),
            Err(error) => catalog.errors.push(format!(
                "{} ({}) action icon {:08X}!{:08X}.{:08X}: {error}",
                source.action,
                source.name,
                source.key.group_id,
                source.key.instance_id,
                source.key.type_id
            )),
        }
    }
    for source in STAGE_HUD_UI_TEXTURES {
        let result = if source.key.instance_id != crate::world_objects::name_id(source.name) {
            Err(format!(
                "registered name hash is {:08X}, but the explicit key uses {:08X}",
                crate::world_objects::name_id(source.name),
                source.key.instance_id
            ))
        } else if index.records.contains_key(&source.key) {
            import_png(index, source.key, images).map(|(image, width, height)| ImportedUiTexture {
                name: source.name,
                source: source.key,
                image,
                width,
                height,
            })
        } else {
            Err(format!(
                "verified native key is absent from this install: {:?}",
                source.key
            ))
        };
        match result {
            Ok(texture) => catalog.textures.push(texture),
            Err(error) => catalog.errors.push(format!("{}: {error}", source.name)),
        }
    }
    catalog
}

/// Import one exact installed PNG key. This is public so callers can load
/// additional source-key-backed native UI textures after resolving them from
/// PROP/UI data.
pub fn import_png(
    index: &ResourceIndex,
    source: ResourceKey,
    images: &mut Assets<Image>,
) -> Result<(Handle<Image>, u32, u32), String> {
    if source.type_id != UI_PNG_TYPE {
        return Err(format!(
            "resource type {:08X} is not a native PNG",
            source.type_id
        ));
    }
    let bytes = index.read(source)?;
    let (width, height, rgba) = decode_png(&bytes)?;
    let image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    Ok((images.add(image), width, height))
}

/// Decode common native PNG color types to RGBA8. The PNG decoder expands
/// indexed and low-bit-depth inputs while preserving per-pixel alpha.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if !bytes.starts_with(crate::decode::PNG_MAGIC) {
        return Err("resource does not have PNG magic".into());
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    let header = decoder
        .read_header_info()
        .map_err(|e| format!("PNG header: {e}"))?;
    let (header_width, header_height) = (header.width, header.height);
    let pixels = (header_width as usize)
        .checked_mul(header_height as usize)
        .filter(|count| {
            header_width > 0
                && header_height > 0
                && header_width <= 8192
                && header_height <= 8192
                && *count <= 16_777_216
        })
        .ok_or("PNG dimensions exceed UI image limits")?;
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG header: {e}"))?;
    let output_size = reader
        .output_buffer_size()
        .ok_or("PNG decoded size overflow")?;
    if output_size > pixels.saturating_mul(8) {
        return Err("PNG decoded buffer exceeds UI image limits".into());
    }
    let mut decoded = vec![0; output_size];
    let frame = reader
        .next_frame(&mut decoded)
        .map_err(|e| format!("PNG pixels: {e}"))?;
    let (width, height) = (frame.width, frame.height);
    if (width, height) != (header_width, header_height) {
        return Err("PNG frame dimensions changed after header parse".into());
    }
    let data = &decoded[..frame.buffer_size()];
    let mut rgba = Vec::with_capacity(pixels * 4);
    match frame.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(data),
        png::ColorType::Rgb => {
            for pixel in data.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for value in data {
                rgba.extend_from_slice(&[*value, *value, *value, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in data.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Indexed => return Err("PNG palette was not expanded by the decoder".into()),
    }
    if rgba.len() != pixels * 4 {
        return Err("PNG decoded byte count does not match dimensions".into());
    }
    Ok((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_png(pixel: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&pixel).unwrap();
        }
        bytes
    }

    #[test]
    fn rgba_png_keeps_native_alpha_and_dimensions() {
        let (width, height, rgba) = decode_png(&rgba_png([19, 71, 203, 64])).unwrap();
        assert_eq!((width, height), (1, 1));
        assert_eq!(rgba, [19, 71, 203, 64]);
    }

    #[test]
    fn png_decoder_rejects_wrong_magic_and_truncation() {
        assert!(decode_png(b"not a png").is_err());
        let mut bytes = rgba_png([1, 2, 3, 4]);
        bytes.truncate(20);
        assert!(decode_png(&bytes).is_err());
    }

    #[test]
    fn native_action_hints_are_explicit_prop_sources() {
        assert_eq!(ACTION_HINT_SOURCES.len(), 8);
        assert_eq!(ACTION_HINT_SOURCES[0].action, "Bite");
        assert_eq!(ACTION_HINT_SOURCES[6].action, "Charm");
        assert_eq!(ACTION_HINT_SOURCES[6].key.instance_id, 0x9E80_5E6C);
        assert!(ACTION_HINT_SOURCES
            .iter()
            .all(|hint| hint.key.type_id == crate::asset_catalog::PROP));
        assert!(NATIVE_UI_TEXTURES
            .iter()
            .all(|texture| texture.key.type_id == UI_PNG_TYPE));
    }

    #[test]
    fn action_button_textures_are_named_native_png_sources() {
        assert_eq!(ACTION_ICON_TEXTURES.len(), 8);
        assert_eq!(ACTION_ICON_TEXTURES[2].action, "Strike");
        assert_eq!(ACTION_ICON_TEXTURES[2].name, "atk_strike_norm");
        assert_eq!(ACTION_ICON_TEXTURES[2].key.instance_id, 0x5D5E_4A83);
        assert!(ACTION_ICON_TEXTURES.iter().all(|texture| {
            texture.key.type_id == UI_PNG_TYPE && texture.key.group_id == CREATURE_ACTION_ICON_GROUP
        }));
    }

    #[test]
    fn original_action_tray_states_are_native_png_sources() {
        let names = NATIVE_UI_TEXTURES
            .iter()
            .map(|texture| texture.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"atk_tray_norm"));
        assert!(names.contains(&"atk_tray_sel_norm"));
        assert!(names.contains(&"social_tray_norm"));
        assert!(names.contains(&"social_tray_sel_norm"));
        assert!(NATIVE_UI_TEXTURES.iter().all(|texture| {
            texture.key.type_id == UI_PNG_TYPE
                && (texture.key.group_id == GLOBAL_UI_GROUP
                    || texture.key.group_id == CREATURE_ACTION_ICON_GROUP
                    || texture.key.group_id == CREATURE_HUD_GROUP
                    || texture.key.group_id == SHARED_CREATURE_UI_GROUP
                    || texture.key.group_id == 0x02E9_A2CA
                    || texture.key.group_id == 0xCA14_DE92)
        }));
    }

    #[test]
    fn original_creature_hud_bars_and_panels_keep_registered_source_keys() {
        let source = |name| {
            NATIVE_UI_TEXTURES
                .iter()
                .find(|texture| texture.name == name)
        };
        let hp = source("crg-ingame_hp-hrz").unwrap();
        let hunger = source("crg-ingame_hunger-hrz").unwrap();
        let energy = source("crg-ingame_energy-hrz").unwrap();
        assert_eq!(hp.key, creature_hud_key(0x4365_3213));
        assert_eq!(hunger.key, creature_hud_key(0x7634_C43A));
        assert_eq!(energy.key, creature_hud_key(0xE509_E42F));
        assert_eq!(
            source("crg_ingame_hp").unwrap().key,
            creature_hud_key(0x1666_DCC2)
        );
        assert_eq!(
            source("crg_ingame_hp_selected").unwrap().key,
            creature_hud_key(0xED30_881E)
        );

        for name in [
            "crg-p-bar-bkgrnd",
            "crg-p-bar-shadow",
            "crg-p-bar-rounding-left",
            "crg-p-bar-rounding-right",
            "crg-p-bar-level-notch",
            "crg-p-bar-brain-1",
            "crg-p-bar-brain-2",
            "crg-p-bar-brain-3",
            "crg-p-bar-brain-4",
            "crg-vrbtray-fight",
            "crg-vrbtray-social",
        ] {
            assert_eq!(source(name).unwrap().key.group_id, CREATURE_HUD_GROUP);
        }
        assert_eq!(
            source("crg-vrbtray-fight").unwrap().key,
            creature_hud_key(0x4404_B931)
        );
        assert_eq!(
            source("crg-vrbtray-social").unwrap().key,
            creature_hud_key(0x9D27_2274)
        );
        assert_eq!(
            source("NoGroup_CreatureInGameHP-01").unwrap().key,
            shared_creature_ui_key(0xC132_7980)
        );
        assert_eq!(
            source("NoGroup_CreatureInGameHP-02").unwrap().key,
            shared_creature_ui_key(0xC132_7983)
        );
        assert!(!NATIVE_UI_TEXTURES
            .iter()
            .any(|texture| texture.name.starts_with("crg_progress-dna-")));
    }

    #[test]
    fn civilization_header_glyphs_are_exact_native_png_records() {
        let sources = [
            ("icon_headerMilitary", 0x8C0A_0D3B),
            ("icon_headerEconomic", 0xD839_BAFD),
            ("icon_headerReligious", 0x8055_187F),
        ];
        for (name, instance_id) in sources {
            let texture = NATIVE_UI_TEXTURES
                .iter()
                .find(|texture| texture.name == name)
                .unwrap();
            assert_eq!(texture.key, civilization_ui_key(instance_id));
        }
    }

    #[test]
    fn stage_hud_png_names_and_resource_keys_are_unique() {
        let unique_names = STAGE_HUD_UI_TEXTURES
            .iter()
            .map(|texture| texture.name)
            .collect::<std::collections::HashSet<_>>();
        let unique_keys = STAGE_HUD_UI_TEXTURES
            .iter()
            .map(|texture| texture.key)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique_names.len(), STAGE_HUD_UI_TEXTURES.len());
        assert_eq!(unique_keys.len(), STAGE_HUD_UI_TEXTURES.len());
        assert!(STAGE_HUD_UI_TEXTURES.iter().all(|texture| {
            texture.key.type_id == UI_PNG_TYPE
                && texture.key.instance_id == crate::world_objects::name_id(texture.name)
        }));
    }

    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn verified_stage_hud_png_keys_exist_in_installed_packages() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = ResourceIndex::load(&install.packages);
        let mut images = Assets::<Image>::default();
        let catalog = load(&index, &mut images);
        for source in STAGE_HUD_UI_TEXTURES {
            let texture = catalog
                .textures
                .iter()
                .find(|texture| texture.name == source.name)
                .unwrap_or_else(|| {
                    panic!(
                        "missing native HUD image {}: {:?}",
                        source.name, catalog.errors
                    )
                });
            assert_eq!(texture.source, source.key);
            assert!(texture.width > 0 && texture.height > 0);
            assert!(images.contains(&texture.image));
        }
    }
}
