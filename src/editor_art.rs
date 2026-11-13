//! Native creature editor category button and panel art.
//!
//! Every source key here is verified against `reg_file.txt` group names and
//! the installed package index:
//! - Group `creature_editor_palette_categories` (0x406B6E0C) holds one PROP
//!   per native category. Each PROP's property `0x2e1942a8` is the localized
//!   category label (read from the installed locale table) and property
//!   `0x044f6c09` is an array of PNG keys (type 0x2F7D0004) in group
//!   `editors_creature` (0x100D977C). All image keys were located as installed
//!   PNG records in `Spore_Graphics.package`; each is a single whole-frame
//!   texture (no strip/atlas), so `frame` is None.
//! - `clg-editor-tray` (GLOBAL_UI_GROUP 0x011C0BF5, instance 0x36383B84) is
//!   the native cell-editor tray (154x75). It is retained in the catalog for
//!   cell-specific use and must not be stretched behind the creature palette.
//!
//! The native category labels (Mouths, Eyes & Senses, Feet, Graspers, Arms &
//! Legs, Weapons, Details) are proven from the installed locale table and are
//! NOT assumed to align with `editor.rs` `CATEGORIES` by index.
//!
//! Part icons: the palette items group holds PROP part definitions, not
//! images, but each editor's `palette_definition` names a thumbnail group
//! (`palettePaletteThumbnailGroupID`, property 0x02233661) whose PNGs are
//! keyed by the part PROP instance. See [`part_icon_key`].
//!
//! Creator chrome (palette panel, undo/redo, mode buttons, DNA, ability
//! glyphs and level pips) is listed in [`CHROME`] by registered file name;
//! each instance is the lowercase FNV-1 hash of that name and every key was
//! located as an installed PNG record in `Spore_Graphics.package`.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    asset_catalog::{self, ResourceIndex},
    dbpf::ResourceKey,
    ui_art::{self, GLOBAL_UI_GROUP, UI_PNG_TYPE},
};

/// Group of the native creature editor palette categories (`creature_editor_palette_categories`
/// per `reg_file.txt`). Each entry is a PROP keyed by its instance ID.
pub const CATEGORY_GROUP: u32 = 0x406B_6E0C;

/// Group of the native editor palette button textures (`editors_creature`
/// per `reg_file.txt`).
pub const EDITOR_TEXTURE_GROUP: u32 = 0x100D_977C;

/// Native editor tray panel background (`clg-editor-tray` per `reg_file.txt`).
pub const EDITOR_PANEL_GROUP: u32 = GLOBAL_UI_GROUP; // 0x011C0BF5
pub const EDITOR_PANEL_INSTANCE: u32 = 0x3638_3B84;

/// Creature palette part icons (`CreaturePartIcons`). Proven chain:
/// `CreatureEditorSetup` (0x40600100!0xCF2DC8D0) `editorPartsPalette` names
/// palette_definition 0x406B6A00!0x0000000C, whose thumbnail group is this.
pub const CREATURE_PART_ICON_GROUP: u32 = 0x0223_1C8B;
/// Cell palette part icons (`cellparticons`), named as the thumbnail group by
/// every Cell editor palette_definition (0x406B6A00!0x01, 0x02 and 0x05).
pub const CELL_PART_ICON_GROUP: u32 = 0xB1B0_F42E;

/// Shared editor UI group (`editors` per `reg_file.txt`): DNA currency icon
/// (named by `editorCurrencyIcon` in the Cell and Creature setups).
pub const EDITORS_GROUP: u32 = 0x100D_976E;
/// Creature editor stat bar pieces (`editors_statBar`).
pub const STAT_BAR_GROUP: u32 = 0xC9B9_97F6;
/// PNG group sharing the `CreatureEditorVerbIcons` stats PROP instance; holds
/// the Speed and Health editor verb icons named by their ability templates.
pub const CREATURE_EDITOR_VERB_GROUP: u32 = 0x9CC3_EB58;
/// Diet tray icons (`verbicons_creature2`).
pub const DIET_ICON_GROUP: u32 = 0x9396_E398;
/// Shared common UI group (`common`).
pub const COMMON_UI_GROUP: u32 = 0x011C_0BDE;

/// Original palette icon for a part definition. The icon instance is the part
/// PROP instance inside the editor's thumbnail group; Cell and Creature parts
/// are the only editors with a proven thumbnail group here.
pub fn part_icon_key(definition: ResourceKey) -> Option<ResourceKey> {
    let group_id = match definition.group_id {
        asset_catalog::CELL_PARTS => CELL_PART_ICON_GROUP,
        asset_catalog::CREATURE_PARTS => CREATURE_PART_ICON_GROUP,
        _ => return None,
    };
    Some(ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id,
        instance_id: definition.instance_id,
    })
}

/// Registered PNG file names used by the creator chrome, with their group.
/// The instance is `world_objects::name_id(name)`; the ability glyph names
/// are the `verbIconImageID` values of the installed ability templates
/// (`creatureabilityTemplates`), e.g. BiteTemplate → `atk_bite`.
pub const CHROME: &[(&str, u32)] = &[
    ("edi_dna_icon", EDITORS_GROUP),
    ("locked-item", COMMON_UI_GROUP),
    ("palette-bckgrnd", EDITOR_TEXTURE_GROUP),
    ("palette-curve-top", EDITOR_TEXTURE_GROUP),
    ("palette-curve-bottom", EDITOR_TEXTURE_GROUP),
    ("panel-text", EDITOR_TEXTURE_GROUP),
    ("CE_evo_points_bg", EDITOR_TEXTURE_GROUP),
    ("dice", EDITOR_TEXTURE_GROUP),
    ("CE_button-undo-norm", EDITOR_TEXTURE_GROUP),
    ("CE_button-undo-off", EDITOR_TEXTURE_GROUP),
    ("CE_button-redo-norm", EDITOR_TEXTURE_GROUP),
    ("CE_button-redo-off", EDITOR_TEXTURE_GROUP),
    ("CE_button-back-norm", EDITOR_TEXTURE_GROUP),
    ("CE_button-forward-norm", EDITOR_TEXTURE_GROUP),
    ("button_mode-build-norm", EDITOR_TEXTURE_GROUP),
    ("button_mode-build-on", EDITOR_TEXTURE_GROUP),
    ("button_mode-play-norm", EDITOR_TEXTURE_GROUP),
    ("button-layer-norm", EDITOR_TEXTURE_GROUP),
    ("button-layer-on-norm", EDITOR_TEXTURE_GROUP),
    ("CE-property-dots-empty", STAT_BAR_GROUP),
    ("CE-property-dots-full", STAT_BAR_GROUP),
    ("atk_bite", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("atk_charge", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("atk_strike", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("atk_spit", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("social_vocalize", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("social_dance", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("social_gesture", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("social_poses", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("mv_energy", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("mv_sneak", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("mv_jump", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("mv_glide", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("mv_sight", ui_art::CREATURE_ACTION_ICON_GROUP),
    ("stats_speedPartInfo_icon", CREATURE_EDITOR_VERB_GROUP),
    ("stats_healthPartInfo_icon", CREATURE_EDITOR_VERB_GROUP),
    ("stats_veg_icon", DIET_ICON_GROUP),
    ("stats_meat_icon", DIET_ICON_GROUP),
    ("stats_omni_icon", DIET_ICON_GROUP),
];

/// Category button state file names, in [`NATIVE_CATEGORIES`] order. The
/// category PROP image arrays list `button-<part>-norm` at index 0 and
/// `button-<part>-on-norm` (the selected tab) at index 4.
pub const CATEGORY_BUTTON_PARTS: &[&str] =
    &["mouth", "eye", "feet", "hands", "limbs", "weapon", "detail"];
pub const NORMAL_STATE: usize = 0;
pub const SELECTED_STATE: usize = 4;
pub const OFF_STATE: usize = 1;

/// Original-source marker on the actual ImageNode art spawned for a native
/// editor category button. Retains the verified source key so the UI art can
/// be audited against the installed package index.
#[derive(Component)]
pub struct EditorArtSource {
    pub key: ResourceKey,
    pub role: &'static str,
}

/// One native editor category button-state image: verified source key,
/// imported image, dimensions, and optional frame/rect metadata
/// (None = whole texture, single frame).
#[derive(Debug, Clone)]
pub struct EditorButtonImage {
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
    /// Frame rect within the source texture; None means the whole texture.
    pub frame: Option<Rect>,
}

/// One native editor category: verified source PROP key, native localized
/// label, and its verified button-state images.
#[derive(Debug, Clone)]
pub struct EditorCategory {
    pub source: ResourceKey, // category PROP source key (type PROP, group CATEGORY_GROUP)
    pub label: &'static str, // native localized category label
    pub images: Vec<EditorButtonImage>,
}

/// Native editor panel / tray background texture.
#[derive(Debug, Clone)]
pub struct EditorPanel {
    pub name: &'static str,
    pub source: ResourceKey,
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
}

/// One imported creator chrome PNG from [`CHROME`].
#[derive(Debug, Clone)]
pub struct EditorChromeImage {
    pub name: &'static str,
    pub key: ResourceKey,
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
}

/// Imported native editor art catalog. Missing sources are recorded as
/// errors rather than silently substituted.
#[derive(Debug, Clone, Default)]
pub struct NativeEditorArt {
    pub categories: Vec<EditorCategory>,
    pub panel: Option<EditorPanel>,
    pub chrome: Vec<EditorChromeImage>,
    /// Localized `editorDefaultModelName` and `editorNamePrompt` of the Cell
    /// and Creature setups, when they are real text.
    pub cell_default_name: Option<String>,
    pub creature_default_name: Option<String>,
    pub cell_name_prompt: Option<String>,
    pub creature_name_prompt: Option<String>,
    pub errors: Vec<String>,
}

/// Canonical native category order, keyed by source PROP instance. This is
/// the proven native label order; it is intentionally NOT forced to match
/// `editor.rs` `CATEGORIES` by index.
pub const NATIVE_CATEGORIES: &[(u32, &'static str)] = &[
    (0x887C_91BB, "Mouths"),
    (0x7F46_6450, "Eyes & Senses"),
    (0x9D62_E363, "Feet"),
    (0x91CA_5E8C, "Graspers"),
    (0xA032_5A4F, "Arms & Legs"),
    (0x228D_8F36, "Weapons"),
    (0xDE8F_240B, "Details"),
];

/// Verified button-state PNG instance keys per native category source, in the
/// order referenced by the category PROP property `0x044f6c09`. Duplicates
/// represent distinct button states and are preserved; identical keys share
/// one imported handle via the loader cache. All keys verified installed in
/// `Spore_Graphics.package`.
pub const CATEGORY_IMAGES: &[&[u32]] = &[
    &[
        0x3A3E_AB26,
        0xBB27_BC1B,
        0x9D4C_5530,
        0xAE41_3F98,
        0x36AA_B892,
        0xBB27_BC1B,
        0x4C9C_2A7C,
        0x1CEC_B2FC,
        0x3A3E_AB26,
    ], // Mouths
    &[
        0x20F3_D498,
        0x5F81_8321,
        0xF18D_5F16,
        0x527B_5ED6,
        0x706D_6648,
        0x5F81_8321,
        0xF797_CE26,
        0xE4E8_34A6,
        0x20F3_D498,
    ], // Eyes & Senses
    &[
        0x2659_3059,
        0xE37B_7F8A,
        0x5491_1BCF,
        0xF080_11E5,
        0xBBE5_FF0B,
        0xE37B_7F8A,
        0xFE07_C5B5,
        0xA84D_99F3,
        0x2659_3059,
    ], // Feet
    &[
        0x82CF_40EF,
        0x90D4_7C04,
        0x3E69_6259,
        0x9DF7_236F,
        0x68B1_5E65,
        0x90D4_7C04,
        0x9C10_0BCB,
        0xA12B_77B9,
        0x82CF_40EF,
    ], // Graspers
    &[
        0x4BC8_A7F0,
        0xECB1_9609,
        0x271F_995E,
        0xFFD4_46EE,
        0xF3E6_C7E0,
        0xECB1_9609,
        0xDEF2_7AAE,
        0xB0CC_F7FE,
        0x4BC8_A7F0,
    ], // Arms & Legs
    &[
        0x8CD1_BCC1,
        0x74E8_8602,
        0x206B_E767,
        0x61C5_D64D,
        0x3D08_F533,
        0x74E8_8602,
        0xF731_762D,
        0x6CC7_705B,
        0x8CD1_BCC1,
    ], // Weapons
    &[
        0x54B0_7940,
        0x2878_E619,
        0x63D1_520E,
        0x3B9B_96DE,
        0xE79E_FB50,
        0x2878_E619,
        0xDFF9_27BE,
        0x5E4F_67CE,
        0x54B0_7940,
    ], // Details
];

/// Proven semantic mapping from the application's category tab name
/// (`editor.rs` `CATEGORIES`) to the native localized category label. Only
/// pairings with proven native semantics are mapped; unmapped names resolve
/// to `None` and never receive a fabricated role.
pub const APPLICATION_TO_NATIVE: &[(&str, &str)] = &[
    ("Mouths", "Mouths"),
    ("Eyes", "Eyes & Senses"),
    ("Feet", "Feet"),
    ("Hands", "Graspers"),
    ("Movement", "Arms & Legs"),
    ("Weapons", "Weapons"),
    ("Details", "Details"),
];

impl NativeEditorArt {
    /// Resolve the native category proven to back an application category
    /// tab name. This is stable against failed imports: it matches by proven
    /// native label, never by vector position.
    pub fn category_by_application(&self, app_category: &str) -> Option<&EditorCategory> {
        let native_label = APPLICATION_TO_NATIVE
            .iter()
            .find(|(app, _)| *app == app_category)
            .map(|(_, native)| *native)?;
        self.categories
            .iter()
            .find(|category| category.label == native_label)
    }

    /// Neutral (first verified) button-state image for a native category.
    /// Button-state semantics are not proven for every array index, so the
    /// first verified image is used unless a specific state is proven.
    pub fn neutral_image<'a>(
        &'a self,
        category: &'a EditorCategory,
    ) -> Option<&'a EditorButtonImage> {
        category.images.first()
    }

    /// Button image for a proven state index ([`NORMAL_STATE`],
    /// [`SELECTED_STATE`] or [`OFF_STATE`]). The image is returned only when
    /// its key is the registered `button-<part>-<state>` name, so a reordered
    /// or failed import never shows the wrong state.
    pub fn state_image<'a>(
        &'a self,
        category: &'a EditorCategory,
        state: usize,
    ) -> Option<&'a EditorButtonImage> {
        let index = NATIVE_CATEGORIES
            .iter()
            .position(|(instance, _)| *instance == category.source.instance_id)?;
        let suffix = match state {
            NORMAL_STATE => "norm",
            SELECTED_STATE => "on-norm",
            OFF_STATE => "off",
            _ => return None,
        };
        let expected = crate::world_objects::name_id(&format!(
            "button-{}-{suffix}",
            CATEGORY_BUTTON_PARTS[index]
        ));
        category
            .images
            .iter()
            .find(|image| image.key.instance_id == expected)
    }

    /// Imported creator chrome PNG by registered file name.
    pub fn chrome(&self, name: &str) -> Option<&EditorChromeImage> {
        self.chrome.iter().find(|image| image.name == name)
    }

    /// Native category at `index`, resolved by the expected source key at
    /// that index (NOT by vector position). This is stable against failed
    /// imports: if the import for `index` failed, `category(index)` returns
    /// None rather than shifting to a later category.
    pub fn category(&self, index: usize) -> Option<&EditorCategory> {
        let expected = NATIVE_CATEGORIES.get(index)?;
        let source = ResourceKey {
            type_id: asset_catalog::PROP,
            group_id: CATEGORY_GROUP,
            instance_id: expected.0,
        };
        self.categories.iter().find(|c| c.source == source)
    }

    /// Native panel / tray background art.
    pub fn panel(&self) -> Option<&EditorPanel> {
        self.panel.as_ref()
    }
}

/// Load verified original editor PNG resources into Bevy's image store.
///
/// The image cache is deduplicated by source key: the 63 button-state
/// references resolve to 49 original PNGs, so only 49 `Handle<Image>`
/// values are created instead of 63.
pub fn load(index: &ResourceIndex, images: &mut Assets<Image>) -> NativeEditorArt {
    let mut catalog = NativeEditorArt::default();
    // Source key -> (handle, width, height), so identical keys share one handle.
    let mut cache: HashMap<ResourceKey, (Handle<Image>, u32, u32)> = HashMap::new();
    for ((source_instance, label), image_instances) in
        NATIVE_CATEGORIES.iter().zip(CATEGORY_IMAGES.iter())
    {
        let source = ResourceKey {
            type_id: asset_catalog::PROP,
            group_id: CATEGORY_GROUP,
            instance_id: *source_instance,
        };
        let mut images_vec = Vec::with_capacity(image_instances.len());
        for &inst in image_instances.iter() {
            let key = ResourceKey {
                type_id: UI_PNG_TYPE,
                group_id: EDITOR_TEXTURE_GROUP,
                instance_id: inst,
            };
            if let Some((handle, width, height)) = cache.get(&key) {
                images_vec.push(EditorButtonImage {
                    key,
                    image: handle.clone(),
                    width: *width,
                    height: *height,
                    frame: None,
                });
                continue;
            }
            match ui_art::import_png(index, key, images) {
                Ok((image, width, height)) => {
                    cache.insert(key, (image.clone(), width, height));
                    images_vec.push(EditorButtonImage {
                        key,
                        image,
                        width,
                        height,
                        frame: None,
                    });
                }
                Err(error) => catalog.errors.push(format!(
                    "editor category {} image {:08X}!{:08X}.{:08X}: {error}",
                    label, EDITOR_TEXTURE_GROUP, inst, UI_PNG_TYPE
                )),
            }
        }
        catalog.categories.push(EditorCategory {
            source,
            label: *label,
            images: images_vec,
        });
    }
    let panel_source = ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id: EDITOR_PANEL_GROUP,
        instance_id: EDITOR_PANEL_INSTANCE,
    };
    match ui_art::import_png(index, panel_source, images) {
        Ok((image, width, height)) => {
            catalog.panel = Some(EditorPanel {
                name: "clg-editor-tray",
                source: panel_source,
                image,
                width,
                height,
            })
        }
        Err(error) => catalog.errors.push(format!(
            "editor panel clg-editor-tray ({:08X}!{:08X}.{:08X}): {error}",
            EDITOR_PANEL_GROUP, EDITOR_PANEL_INSTANCE, UI_PNG_TYPE
        )),
    }
    for (name, group_id) in CHROME {
        let key = chrome_key(name, *group_id);
        match ui_art::import_png(index, key, images) {
            Ok((image, width, height)) => catalog.chrome.push(EditorChromeImage {
                name,
                key,
                image,
                width,
                height,
            }),
            Err(error) => catalog.errors.push(format!(
                "editor chrome {name} ({:08X}!{:08X}.{:08X}): {error}",
                key.group_id, key.instance_id, key.type_id
            )),
        }
    }
    for (setup, name, prompt) in [
        (
            CELL_EDITOR_TEMPLATE,
            &mut catalog.cell_default_name,
            &mut catalog.cell_name_prompt,
        ),
        (
            CREATURE_EDITOR_SETUP,
            &mut catalog.creature_default_name,
            &mut catalog.creature_name_prompt,
        ),
    ] {
        *name = setup_text(index, setup, DEFAULT_MODEL_NAME);
        *prompt = setup_text(index, setup, NAME_PROMPT);
    }
    catalog
}

/// Group of the editor setup PROPs (`editor_config`).
pub const EDITOR_CONFIG_GROUP: u32 = 0x4060_0100;
/// `CellTemplate`, the parent of every Cell editor level setup.
pub const CELL_EDITOR_TEMPLATE: u32 = 0x3615_A30B;
/// `CreatureEditorSetup`.
pub const CREATURE_EDITOR_SETUP: u32 = 0xCF2D_C8D0;
/// `editorDefaultModelName` property.
pub const DEFAULT_MODEL_NAME: u32 = 0x04D9_2F82;
/// `editorNamePrompt` property: the name field's empty-state prompt.
pub const NAME_PROMPT: u32 = 0x0537_1343;
/// Locale table resource type (instance = table ID).
const LOCALE_TABLE: u32 = 0x02FA_C0B6;

/// Localized text property of an editor setup PROP (for example
/// [`DEFAULT_MODEL_NAME`]), falling back to the PROP's own embedded text
/// when its locale table has no entry. Development placeholders such as
/// `~blank text~` are treated as absent.
pub fn setup_text(index: &ResourceIndex, setup: u32, property: u32) -> Option<String> {
    let properties = index
        .properties(ResourceKey {
            type_id: asset_catalog::PROP,
            group_id: EDITOR_CONFIG_GROUP,
            instance_id: setup,
        })
        .ok()?;
    let (table, id, fallback) = properties.0.get(&property)?.text()?;
    let mut tables: Vec<_> = index
        .records
        .keys()
        .filter(|k| k.type_id == LOCALE_TABLE && k.instance_id == table)
        .copied()
        .collect();
    tables.sort_by_key(|k| k.group_id);
    tables
        .into_iter()
        .filter_map(|k| index.read(k).ok())
        .flat_map(|bytes| crate::decode::parse_locale_table(&bytes))
        .find(|(text_id, _)| *text_id == id)
        .map(|(_, text)| text)
        .or(Some(fallback))
        .map(|text| text.trim().to_string())
        .filter(|text| usable_text(text))
}

fn usable_text(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('~') && !text.contains("PLACEHOLDER")
}

/// Exact PNG key of a registered creator chrome file name.
pub fn chrome_key(name: &str, group_id: u32) -> ResourceKey {
    ResourceKey {
        type_id: UI_PNG_TYPE,
        group_id,
        instance_id: crate::world_objects::name_id(name),
    }
}

/// Import one exact installed PNG key (public for callers resolving
/// additional editor art from PROP/UI data).
pub fn import_png(
    index: &ResourceIndex,
    source: ResourceKey,
    images: &mut Assets<Image>,
) -> Result<(Handle<Image>, u32, u32), String> {
    ui_art::import_png(index, source, images)
}

/// Decode common native PNG color types to RGBA8.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    ui_art::decode_png(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires original Spore installation"]
    fn installed_editor_categories_bind_exact_native_pixels_and_share_states() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut images = Assets::<Image>::default();
        let art = load(&index, &mut images);
        assert!(art.errors.is_empty(), "{:?}", art.errors);
        let mut handles = std::collections::HashMap::new();
        for (i, category) in art.categories.iter().enumerate() {
            let props = index.properties(category.source).unwrap();
            assert!(props.0.contains_key(&0x044F6C09));
            assert_eq!(category.images.len(), CATEGORY_IMAGES[i].len());
            for original in &category.images {
                let (width, height, rgba) = decode_png(&index.read(original.key).unwrap()).unwrap();
                let live = images.get(&original.image).unwrap();
                assert_eq!((live.width(), live.height()), (width, height));
                assert_eq!(live.data.as_ref().unwrap(), &rgba);
                if let Some(previous) = handles.insert(original.key, original.image.clone()) {
                    assert_eq!(previous, original.image);
                }
            }
        }
        assert_eq!(handles.len(), 49);
    }

    fn rgba_png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let mut row = vec![0u8; (width * 4) as usize];
            for chunk in row.chunks_exact_mut(4) {
                chunk.copy_from_slice(&pixel);
            }
            writer
                .write_image_data(&row.repeat(height as usize))
                .unwrap();
        }
        bytes
    }

    #[test]
    fn decode_png_preserves_live_image_dimensions_and_bytes() {
        let bytes = rgba_png(2, 3, [10, 20, 30, 200]);
        let (width, height, rgba) = decode_png(&bytes).unwrap();
        assert_eq!((width, height), (2, 3));
        let mut expected = Vec::new();
        for _ in 0..(2 * 3) {
            expected.extend_from_slice(&[10, 20, 30, 200]);
        }
        assert_eq!(rgba, expected);
    }

    fn test_catalog() -> NativeEditorArt {
        let mut categories = Vec::new();
        for ((source_instance, label), image_sources) in
            NATIVE_CATEGORIES.iter().zip(CATEGORY_IMAGES)
        {
            let key = ResourceKey {
                type_id: UI_PNG_TYPE,
                group_id: EDITOR_TEXTURE_GROUP,
                instance_id: image_sources[0],
            };
            categories.push(EditorCategory {
                source: ResourceKey {
                    type_id: asset_catalog::PROP,
                    group_id: CATEGORY_GROUP,
                    instance_id: *source_instance,
                },
                label: *label,
                images: vec![EditorButtonImage {
                    key,
                    image: Handle::<Image>::default(),
                    width: 50,
                    height: 52,
                    frame: None,
                }],
            });
        }
        NativeEditorArt {
            categories,
            ..default()
        }
    }

    #[test]
    fn category_button_states_are_proven_by_registered_names() {
        for (i, part) in CATEGORY_BUTTON_PARTS.iter().enumerate() {
            let name =
                |state: &str| crate::world_objects::name_id(&format!("button-{part}-{state}"));
            assert_eq!(CATEGORY_IMAGES[i][NORMAL_STATE], name("norm"));
            assert_eq!(CATEGORY_IMAGES[i][OFF_STATE], name("off"));
            assert_eq!(CATEGORY_IMAGES[i][SELECTED_STATE], name("on-norm"));
        }
        // A catalog holding only the neutral image never claims a selected
        // state it did not import.
        let art = test_catalog();
        let mouths = art.category_by_application("Mouths").unwrap();
        assert!(art.state_image(mouths, NORMAL_STATE).is_some());
        assert!(art.state_image(mouths, SELECTED_STATE).is_none());
    }

    #[test]
    fn chrome_and_part_icons_use_exact_registered_png_keys() {
        let mut keys = std::collections::HashSet::new();
        for (name, group) in CHROME {
            let key = chrome_key(name, *group);
            assert_eq!(key.type_id, UI_PNG_TYPE);
            assert_eq!(key.instance_id, crate::world_objects::name_id(name));
            assert!(keys.insert(key), "{name} is listed once");
        }
        assert_eq!(
            chrome_key("atk_bite", ui_art::CREATURE_ACTION_ICON_GROUP).instance_id,
            0xED0F_470A
        );
        assert_eq!(
            chrome_key("edi_dna_icon", EDITORS_GROUP).instance_id,
            0xAA77_F867
        );
        let jaw = ResourceKey {
            type_id: asset_catalog::PROP,
            group_id: asset_catalog::CELL_PARTS,
            instance_id: 0x2494_A720,
        };
        assert_eq!(
            part_icon_key(jaw),
            Some(ResourceKey {
                type_id: UI_PNG_TYPE,
                group_id: CELL_PART_ICON_GROUP,
                instance_id: 0x2494_A720,
            })
        );
        let mouth = ResourceKey {
            group_id: asset_catalog::CREATURE_PARTS,
            instance_id: 0x56E7_7082,
            ..jaw
        };
        assert_eq!(
            part_icon_key(mouth).unwrap().group_id,
            CREATURE_PART_ICON_GROUP
        );
        assert!(part_icon_key(ResourceKey { group_id: 7, ..jaw }).is_none());
    }

    #[test]
    fn application_category_resolves_to_proven_native_role() {
        let art = test_catalog();
        let hands = art.category_by_application("Hands").unwrap();
        assert_eq!(hands.label, "Graspers");
        assert_eq!(hands.source.instance_id, 0x91CA_5E8C);
        assert_eq!(
            art.neutral_image(hands).unwrap().key.instance_id,
            0x82CF_40EF
        );
        let movement = art.category_by_application("Movement").unwrap();
        assert_eq!(movement.label, "Arms & Legs");
        assert_eq!(
            art.neutral_image(movement).unwrap().key.instance_id,
            0x4BC8_A7F0
        );
        let eyes = art.category_by_application("Eyes").unwrap();
        assert_eq!(eyes.label, "Eyes & Senses");
    }

    #[test]
    fn simulated_missing_category_resolves_to_none_without_position_shift() {
        let full = test_catalog();
        // Simulate a failed import by dropping the Graspers category entirely.
        let mut missing = full.clone();
        missing.categories.retain(|c| c.label != "Graspers");
        // Label-based lookup reports the role as absent.
        assert!(missing.category_by_application("Hands").is_none());
        // Key-based index lookup stays stable: index 3 is Graspers and returns
        // None instead of shifting to whatever category moved into slot 3.
        assert!(missing.category(3).is_none());
        assert!(missing.category_by_application("Mouths").is_some());
        assert!(missing.category_by_application("Details").is_some());
    }
}
