//! Paint mode of the vehicle, building and UFO editors, and the block paint
//! the stages and the Sporepedia thumbnails draw (helper `editors-paint`).
//!
//! Data (VERIFIED, installed PROPs, `notes/rigid-paint-notes.md`): the paint
//! palettes `palettes~!0000003C` (vehicles), `!00000029` (buildings) and
//! `!00000047` (UFO: the vehicle module and pages under its own thumbnail
//! group `ufopainticons~`). Their tabs: Complete Styles (block paint themes,
//! `ItemBlockPaintTheme`), Partial Styles with three region sub-tabs
//! (`paletteCategoryRegionFilter` -> `regionfilters~!RegionFilter_*`, e.g.
//! vehicles Chassis & Cockpits / Wheels, Wings, Sails / Details) and
//! Paintbrush Mode (`paletteCategoryPaintByNumber`, block paints
//! `ItemBlockPaint`). A theme lists one block paint and two colours per paint
//! region (`paintThemePaints` / `paintThemeColor1s` / `paintThemeColor2s`,
//! indexed by region: 51 vehicle entries, 18 building entries); a block paint
//! is a tiling texture (`paintMaterialDiffuse`: grey detail in RGB, the
//! colour-1 / colour-2 mask in alpha) with `paintMaterialCol1BlendFactor`,
//! `Col2BlendFactor`, glossiness and specular strength. The default paint is
//! the setup's `editorDefaultPaintTheme` (`ve_theme_default`,
//! `be_theme_default`).
//!
//! Regions (VERIFIED from the part meshes): every mesh of a rigid block's RW4
//! carries its paint region in the compiled state's shader data 0x20F (first
//! word; `ve_landcraft_08`: regions 10, 19, 14; its authored textures are
//! 64 x 64 debug checkers coloured by region, never shown in retail).
//! A block keeps its own paint per region (`RigidBlock::paints`, the retail
//! `EditorRigblock` paints: region, paint id, two colours; the Sporepedia
//! XML `<paintlist>`).
//!
//! ADAPTED (labelled below): the shading of a block paint (texture x colours,
//! `compose`) is read from the data, not from the decompiled rigid-block
//! shader; the colour row recolours colour 1 of the regions in scope
//! (Shift: colour 2); keys 1..3 switch the top tabs.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use serde::{Deserialize, Serialize};

use crate::{
    asset_catalog::{ResourceIndex, BUILDING_PARTS},
    cell_editor_data::{key_of, number, prop_key},
    cell_editor_paint::{id as page_id, swatch_layout, tile_layout},
    cell_editor_ui::{id, image},
    cell_paint::{colors as color_list, same_color, CellPaintData, PaintTheme, PAINT_THEME_GROUP},
    creature_editor::{CreatureEditor, CreatureEditorOwned, Mode},
    creature_editor_data::{palette, prop_id, Palette, PaletteCategory, SetupKind, CATEGORY_GROUP_BASE, PNG},
    creature_editor_ui::EditorRoot,
    dbpf::ResourceKey,
    editor::EditorState,
    game::RunProgress,
    rigid_editor::{holder_of, model_of, Hull, RigidEditor, RigidPreview},
    spui::{
        ids::state,
        solve::RootPolicy,
        view::{self, SpuiAssets, SpuiState},
    },
    world_objects::name_id,
    OriginalModel, Originals,
};

/// `regionfilters~`.
pub const REGION_FILTER_GROUP: u32 = 0x406A_6F00;
/// Shader data of a rigid block mesh's compiled state holding its paint region.
pub const PAINT_REGION_DATA: i16 = 0x20F;

pub mod paint_prop {
    pub const THEME_PAINTS: u32 = 0xB0E0_66A7;
    pub const THEME_COLOR1S: u32 = 0xB0E0_66A8;
    pub const THEME_COLOR2S: u32 = 0xB0E0_66A9;
    pub const MATERIAL_DIFFUSE: u32 = 0xB0E0_66A4;
    pub const COL1_BLEND: u32 = 0x025E_49BB;
    pub const COL2_BLEND: u32 = 0x0265_FB07;
    pub const GLOSSINESS: u32 = 0x055D_3D82;
    pub const SPEC_STRENGTH: u32 = 0x0562_A3B7;
    pub const DEFAULT_COLOR1: u32 = 0x921A_7B98;
    pub const DEFAULT_COLOR2: u32 = 0x921A_7B99;
    pub const REGION_FILTER: u32 = 0xF21A_7BDC;
    pub const CATEGORY_REGION_FILTER: u32 = 0xD20D_4636;
    pub const CATEGORY_PAINT_BY_NUMBER: u32 = 0x0337_BF31;
    pub const CATEGORY_ICON: u32 = 0xE6A3_1466;
}

// ---------------------------------------------------------------------------
// Records and data
// ---------------------------------------------------------------------------

/// The paint of one region of a block (retail `EditorRigblock` paint entry,
/// Sporepedia `<paint><paintregion><paintid><color1><color2>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegionPaint {
    pub region: u32,
    /// `paints~` block paint instance.
    pub paint: u32,
    /// Colours 0..1 (as the PROPs store them).
    pub color1: [f32; 3],
    pub color2: [f32; 3],
}

/// A block paint (`ItemBlockPaint`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockPaint {
    pub instance: u32,
    pub diffuse: Option<ResourceKey>,
    /// `paintMaterialCol1BlendFactor`, `paintMaterialCol2BlendFactor`.
    pub blend: [f32; 2],
    pub glossiness: f32,
    pub spec_strength: f32,
    /// `paintMaterialDefaultColor1/2`.
    pub colors: [[f32; 3]; 2],
}

/// A block paint theme (`ItemBlockPaintTheme`): per region a paint and two
/// colours.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockTheme {
    pub instance: u32,
    pub paints: Vec<u32>,
    pub color1s: Vec<[f32; 3]>,
    pub color2s: Vec<[f32; 3]>,
}

impl BlockTheme {
    /// The theme's paint of `region` (none when the theme leaves it).
    pub fn entry(&self, region: u32) -> Option<RegionPaint> {
        let r = region as usize;
        let paint = *self.paints.get(r)?;
        (paint != 0 && paint != u32::MAX).then(|| RegionPaint {
            region,
            paint,
            color1: self.color1s.get(r).copied().unwrap_or([1.0; 3]),
            color2: self.color2s.get(r).copied().unwrap_or([1.0; 3]),
        })
    }
}

pub fn load_theme(index: &ResourceIndex, instance: u32) -> Result<BlockTheme, String> {
    let p = index.properties(prop_key(PAINT_THEME_GROUP, instance))?;
    let paints: Vec<u32> = crate::cell_editor_data::key_list(&p, paint_prop::THEME_PAINTS).iter().map(|k| k.instance_id).collect();
    if paints.is_empty() {
        return Err(format!("paint theme {instance:08X} lists no paints"));
    }
    Ok(BlockTheme {
        instance,
        paints,
        color1s: p.0.get(&paint_prop::THEME_COLOR1S).map(color_list).unwrap_or_default(),
        color2s: p.0.get(&paint_prop::THEME_COLOR2S).map(color_list).unwrap_or_default(),
    })
}

pub fn load_paint(index: &ResourceIndex, instance: u32) -> Result<BlockPaint, String> {
    let p = index.properties(prop_key(PAINT_THEME_GROUP, instance))?;
    let color = |id: u32, fallback: [f32; 3]| p.0.get(&id).map(color_list).and_then(|c| c.first().copied()).unwrap_or(fallback);
    let diffuse = key_of(&p, paint_prop::MATERIAL_DIFFUSE).filter(|k| k.instance_id != 0);
    if diffuse.is_none() {
        return Err(format!("block paint {instance:08X} has no paintMaterialDiffuse"));
    }
    Ok(BlockPaint {
        instance,
        diffuse,
        blend: [number(&p, paint_prop::COL1_BLEND).unwrap_or(0.0), number(&p, paint_prop::COL2_BLEND).unwrap_or(1.0)],
        glossiness: number(&p, paint_prop::GLOSSINESS).unwrap_or(0.0),
        spec_strength: number(&p, paint_prop::SPEC_STRENGTH).unwrap_or(0.0),
        colors: [color(paint_prop::DEFAULT_COLOR1, [1.0; 3]), color(paint_prop::DEFAULT_COLOR2, [1.0; 3])],
    })
}

/// The setup's `editorDefaultPaintTheme` by the part group (the UFO's setup
/// inherits the vehicle template's).
pub fn default_theme_of_group(group: u32) -> u32 {
    if group == BUILDING_PARTS {
        name_id("be_theme_default")
    } else {
        name_id("ve_theme_default")
    }
}

/// Set (replace or add) one region's paint of a block.
pub fn set_region(paints: &mut Vec<RegionPaint>, entry: RegionPaint) {
    match paints.iter_mut().find(|p| p.region == entry.region) {
        Some(p) => *p = entry,
        None => {
            paints.push(entry);
            paints.sort_by_key(|p| p.region);
        }
    }
}

/// A theme on a block: every region of the block the filter admits takes the
/// theme's paint (a partial style: the sub-tab's region filter).
pub fn theme_block(paints: &mut Vec<RegionPaint>, regions: &[u32], theme: &BlockTheme, filter: Option<&[bool]>) {
    for &r in regions {
        if filter.is_some_and(|f| !f.get(r as usize).copied().unwrap_or(false)) {
            continue;
        }
        if let Some(e) = theme.entry(r) {
            set_region(paints, e);
        }
    }
}

/// The paint a region of a block shows: its own, else the default theme's.
pub fn resolve(paints: &[RegionPaint], region: u32, fallback: Option<&BlockTheme>) -> Option<RegionPaint> {
    paints.iter().find(|p| p.region == region).cloned().or_else(|| fallback.and_then(|t| t.entry(region)))
}

/// ADAPTED block paint shading (read from the data; the rigid-block shader is
/// not decompiled): the texture's alpha picks colour 1 (opaque) or colour 2,
/// each colour modulated by the grey detail (RGB, centred on mid grey) as
/// much as its blend factor says. A matte paint (white, opaque) gives flat
/// colour 1.
pub fn compose(rgba: &[u8], paint: &BlockPaint, color1: [f32; 3], color2: [f32; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for p in rgba.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            let g = p[c] as f32 / 255.0 * 2.0;
            let c1 = color1[c] * (1.0 + (g - 1.0) * paint.blend[0]);
            let c2 = color2[c] * (1.0 + (g - 1.0) * paint.blend[1]);
            out.push(((c2 + (c1 - c2) * a).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out.push(255);
    }
    out
}

fn quantize(c1: [f32; 3], c2: [f32; 3]) -> [u8; 6] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(c1[0]), q(c1[1]), q(c1[2]), q(c2[0]), q(c2[1]), q(c2[2])]
}

/// Paint regions of a model's surfaces (in `OriginalModel::surfaces` order).
pub fn surface_regions(bytes: &[u8], model: &OriginalModel) -> Vec<Option<u32>> {
    model
        .surfaces
        .iter()
        .map(|s| {
            crate::material_binding::common_state(bytes, &[s.source_section])
                .ok()
                .flatten()
                .and_then(|st| st.shader_data.iter().find(|d| d.index == PAINT_REGION_DATA).and_then(|d| d.words.first().copied()))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Materials (shared by the editor, the stages and the thumbnails)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct RigidPaintCache {
    regions: HashMap<ResourceKey, Arc<Vec<Option<u32>>>>,
    themes: HashMap<u32, Option<Arc<BlockTheme>>>,
    paints: HashMap<u32, Option<Arc<BlockPaint>>>,
    diffuse: HashMap<u32, Option<Arc<crate::texture::Pixels>>>,
    images: HashMap<(u32, [u8; 6]), Handle<Image>>,
    materials: HashMap<(AssetId<StandardMaterial>, u32, [u8; 6], bool), Handle<StandardMaterial>>,
    surface_hulls: HashMap<(ResourceKey, usize), Option<Arc<Hull>>>,
}

impl RigidPaintCache {
    /// Paint regions of a block model's surfaces (cached by definition).
    pub fn regions(&mut self, originals: &Originals, model: &OriginalModel) -> Arc<Vec<Option<u32>>> {
        let key = model.source_definition.unwrap_or(ResourceKey { type_id: 0, group_id: 0, instance_id: model.instance });
        if let Some(r) = self.regions.get(&key) {
            return r.clone();
        }
        let rw4 = model
            .source_definition
            .and_then(|d| originals.part_catalog.parts.iter().find(|p| p.key == d))
            .map(|d| d.model);
        let regions = rw4
            .and_then(|k| originals.resource_index.read(k).ok())
            .map(|bytes| surface_regions(&bytes, model))
            .unwrap_or_else(|| vec![None; model.surfaces.len()]);
        let regions = Arc::new(regions);
        self.regions.insert(key, regions.clone());
        regions
    }

    /// The distinct regions of a model.
    pub fn region_list(&mut self, originals: &Originals, model: &OriginalModel) -> Vec<u32> {
        let mut v: Vec<u32> = self.regions(originals, model).iter().flatten().copied().collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn theme(&mut self, index: &ResourceIndex, instance: u32) -> Option<Arc<BlockTheme>> {
        self.themes
            .entry(instance)
            .or_insert_with(|| match load_theme(index, instance) {
                Ok(t) => Some(Arc::new(t)),
                Err(e) => {
                    warn!("rigid paint: {e}");
                    None
                }
            })
            .clone()
    }

    pub fn paint(&mut self, index: &ResourceIndex, instance: u32) -> Option<Arc<BlockPaint>> {
        self.paints
            .entry(instance)
            .or_insert_with(|| match load_paint(index, instance) {
                Ok(p) => Some(Arc::new(p)),
                Err(e) => {
                    warn!("rigid paint: {e}");
                    None
                }
            })
            .clone()
    }

    fn image(&mut self, index: &ResourceIndex, images: &mut Assets<Image>, entry: &RegionPaint) -> Option<(Handle<Image>, Arc<BlockPaint>)> {
        let paint = self.paint(index, entry.paint)?;
        let key = (entry.paint, quantize(entry.color1, entry.color2));
        if let Some(h) = self.images.get(&key) {
            return Some((h.clone(), paint));
        }
        let diffuse = self
            .diffuse
            .entry(entry.paint)
            .or_insert_with(|| {
                paint.diffuse.and_then(|k| index.read(k).ok()).and_then(|b| crate::texture::decode_rw4(&b)).map(Arc::new)
            })
            .clone()?;
        let rgba = compose(&diffuse.rgba, &paint, entry.color1, entry.color2);
        let (w, h) = (diffuse.width, diffuse.height);
        let levels = 32 - w.max(h).max(1).leading_zeros();
        let mut image = Image::new(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            rgba.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        image.texture_descriptor.mip_level_count = levels;
        image.data = Some(crate::cell_paint::with_mips(&rgba, w, h, levels));
        // The paint textures tile over the block's UVs.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
        let handle = images.add(image);
        self.images.insert(key, handle.clone());
        Some((handle, paint))
    }

    /// The material of a surface painted with `entry` (`base` = the surface's
    /// imported material: culling, alpha, normal map stay).
    pub fn material(
        &mut self,
        index: &ResourceIndex,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        base: &Handle<StandardMaterial>,
        entry: &RegionPaint,
        lit: bool,
    ) -> Option<Handle<StandardMaterial>> {
        let key = (base.id(), entry.paint, quantize(entry.color1, entry.color2), lit);
        if let Some(m) = self.materials.get(&key) {
            return Some(m.clone());
        }
        let (texture, paint) = self.image(index, images, entry)?;
        let mut m = materials.get(base).cloned().unwrap_or_default();
        m.base_color = Color::WHITE;
        m.base_color_texture = Some(texture);
        // ADAPTED: the paint's glossiness / specular strength as PBR roughness
        // and reflectance.
        m.perceptual_roughness = (0.85 - 0.6 * paint.glossiness.clamp(0.0, 1.0) - 0.3 * paint.spec_strength.clamp(0.0, 1.0)).clamp(0.2, 0.9);
        m.reflectance = 0.2 + 0.4 * paint.spec_strength.clamp(0.0, 1.0);
        m.metallic = 0.0;
        // The paint's alpha is its colour mask, not coverage: a painted block
        // is opaque (several rigid block states are authored blended, which
        // drew the painted blocks see-through and out of order).
        m.alpha_mode = AlphaMode::Opaque;
        if lit {
            m.emissive = LinearRgba::new(0.16, 0.2, 0.32, 1.0);
        }
        let handle = materials.add(m);
        self.materials.insert(key, handle.clone());
        Some(handle)
    }

    /// The painted material of surface `surface` of a block model with the
    /// block's paints (the default theme of its group where it has none).
    #[allow(clippy::too_many_arguments)]
    pub fn surface_material(
        &mut self,
        originals: &Originals,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        model: &OriginalModel,
        surface: usize,
        paints: &[RegionPaint],
        lit: bool,
    ) -> Option<Handle<StandardMaterial>> {
        let base = model.surfaces.get(surface)?.material.clone();
        let region = (*self.regions(originals, model).get(surface)?)?;
        let group = model.source_definition.map_or(0, |d| d.group_id);
        let fallback = self.theme(&originals.resource_index, default_theme_of_group(group));
        let entry = resolve(paints, region, fallback.as_deref())?;
        self.material(&originals.resource_index, images, materials, &base, &entry, lit)
    }

    /// Triangles of one surface (paintbrush picking).
    pub fn surface_hull(&mut self, meshes: &Assets<Mesh>, model: &OriginalModel, surface: usize) -> Option<Arc<Hull>> {
        let key = model.source_definition.unwrap_or(ResourceKey { type_id: 0, group_id: 0, instance_id: model.instance });
        self.surface_hulls
            .entry((key, surface))
            .or_insert_with(|| model.surfaces.get(surface).and_then(|s| Hull::of(meshes, &s.handle)).map(Arc::new))
            .clone()
    }
}

/// A drawn surface of a rigid block: `apply_surface_paint` gives it its
/// painted material (`rigid_editor::spawn_surfaces` adds it).
#[derive(Component, Clone)]
pub struct RigidSurfacePaint {
    pub definition: Option<ResourceKey>,
    pub surface: usize,
    pub paints: Arc<[RegionPaint]>,
    /// The selected block of the editor (lit).
    pub lit: bool,
}

/// Swap new block surfaces to their painted material (every stage).
pub fn apply_surface_paint(
    mut commands: Commands,
    mut cache: ResMut<RigidPaintCache>,
    originals: Res<Originals>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    added: Query<(Entity, &RigidSurfacePaint), Added<RigidSurfacePaint>>,
    mut reported: Local<std::collections::HashSet<(Option<ResourceKey>, usize)>>,
) {
    for (entity, s) in &added {
        let Some(model) = s.definition.and_then(|d| originals.mechanical_models.get(&d)) else {
            if reported.insert((s.definition, s.surface)) {
                info!("rigid paint: no imported model for {:?}", s.definition);
            }
            continue;
        };
        match cache.surface_material(&originals, &mut images, &mut materials, model, s.surface, &s.paints, s.lit) {
            Some(m) => {
                commands.entity(entity).insert(MeshMaterial3d(m));
            }
            None => {
                if reported.insert((s.definition, s.surface)) {
                    let region = cache.regions(&originals, model).get(s.surface).copied().flatten();
                    info!("rigid paint: surface {} of {:?} keeps its imported material (region {region:?})", s.surface, s.definition);
                }
            }
        }
    }
}

/// A hash of a block's paints (rebuild keys, skyline signatures).
pub fn paints_signature(paints: &[RegionPaint]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let mut mix = |bits: u32| {
        hash ^= bits;
        hash = hash.wrapping_mul(16777619);
    };
    for p in paints {
        mix(p.region);
        mix(p.paint);
        for v in p.color1.iter().chain(p.color2.iter()) {
            mix(v.to_bits());
        }
    }
    hash
}

// ---------------------------------------------------------------------------
// The paint palettes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Vehicle,
    Building,
    Ufo,
}

impl Family {
    pub fn of(kind: SetupKind) -> Option<Family> {
        if kind.is_building() {
            Some(Family::Building)
        } else if kind == SetupKind::Ufo {
            Some(Family::Ufo)
        } else if kind.is_rigid() {
            Some(Family::Vehicle)
        } else {
            None
        }
    }

    /// `editorPaintPalette` of the setups.
    pub fn palette_id(self) -> u32 {
        match self {
            Family::Vehicle => 0x3C,
            Family::Building => 0x29,
            Family::Ufo => 0x47,
        }
    }

    /// Group (low byte) of the categories its pages name (the UFO palette
    /// uses the vehicle pages and categories).
    fn category_low(self) -> u32 {
        match self {
            Family::Building => 0x29,
            _ => 0x3C,
        }
    }

    fn partial_category(self) -> &'static str {
        match self {
            Family::Building => "be_category_partial_themes",
            _ => "ve_category_partial_themes",
        }
    }
}

/// One family's paint palette.
#[derive(Clone, Debug, Default)]
pub struct FamilyPaint {
    /// Categories in retail order: Complete Styles, the region sub-tabs,
    /// Paintbrush Mode.
    pub palette: Palette,
    /// The three top tabs: Complete, Partial (its sub-tabs below), Paintbrush.
    pub top_tabs: Palette,
    /// Region filter per palette category (sub-tabs only).
    pub filters: Vec<Option<Vec<bool>>>,
    /// Sub-tab icons per palette category.
    pub sub_icons: Vec<Option<ResourceKey>>,
    /// The Paintbrush Mode category.
    pub brush_tab: Option<usize>,
    /// Tile icons and colour row in the Cell Creator's shape.
    pub cell: CellPaintData,
    pub default_theme: u32,
}

impl FamilyPaint {
    pub fn load(index: &ResourceIndex, kind: SetupKind) -> Self {
        let mut out = FamilyPaint::default();
        let Some(family) = Family::of(kind) else { return out };
        if index.records.is_empty() {
            return out;
        }
        let mut locale = crate::cell_editor_data::Locale::default();
        out.palette = palette(index, &mut locale, family.palette_id());
        let cat_group = CATEGORY_GROUP_BASE | family.category_low();
        for c in &out.palette.categories {
            let p = index.properties(prop_key(cat_group, c.instance)).ok();
            let filter = p
                .as_ref()
                .and_then(|p| key_of(p, paint_prop::CATEGORY_REGION_FILTER))
                .and_then(|k| index.properties(prop_key(REGION_FILTER_GROUP, k.instance_id)).ok())
                .and_then(|f| f.0.get(&paint_prop::REGION_FILTER).map(|v| v.values.iter().map(|b| b.first().is_some_and(|x| *x != 0)).collect()));
            out.filters.push(filter);
            out.sub_icons.push(
                p.as_ref().and_then(|p| key_of(p, paint_prop::CATEGORY_ICON)).filter(|k| k.instance_id != 0).map(|k| ResourceKey { type_id: PNG, ..k }),
            );
            if p.as_ref().is_some_and(|p| p.0.get(&paint_prop::CATEGORY_PAINT_BY_NUMBER).and_then(|v| v.values.first()).is_some_and(|b| b.first().is_some_and(|x| *x != 0))) {
                out.brush_tab = Some(out.filters.len() - 1);
            }
        }
        // Top tabs: Complete Styles, Partial Styles, Paintbrush Mode.
        let mut partial = PaletteCategory { name: "Partial Styles".into(), ..Default::default() };
        if let Ok(p) = index.properties(prop_key(cat_group, name_id(family.partial_category()))) {
            partial.icons = crate::cell_editor_data::key_list(&p, prop_id::CATEGORY_ICONS).into_iter().map(|k| ResourceKey { type_id: PNG, ..k }).collect();
            if let Some(n) = locale.text(index, &p, prop_id::CATEGORY_NAME) {
                partial.name = n;
            }
        }
        let mut top = Vec::new();
        if let Some(c) = out.palette.categories.first() {
            top.push(c.clone());
        }
        if out.filters.iter().any(|f| f.is_some()) {
            top.push(partial);
        }
        if let Some(c) = out.brush_tab.and_then(|b| out.palette.categories.get(b)) {
            top.push(c.clone());
        }
        out.top_tabs = Palette { categories: top, ..Default::default() };
        // Tiles: every theme and block paint with its icon.
        let cell = CellPaintData::load(index);
        out.cell = CellPaintData { loaded: true, art: cell.art, swatches: cell.swatches, ..Default::default() };
        // Icons (VERIFIED): the setup's `palettePaletteThumbnailGroupID`
        // (e.g. `vehiclelandpainticons~`, styles drawn on the class's swatch
        // model) overrides the palette's (`vehiclepainticons~`); the block
        // paint pages ignore the override (`palettePageIgnoreOverrideThumbnailGroup`).
        let palette_group = out.palette.thumbnail_group.unwrap_or(0);
        let setup_group = index
            .properties(prop_key(crate::cell_editor_data::EDITOR_SETUP_GROUP, name_id(kind.setup_name())))
            .ok()
            .and_then(|p| key_of(&p, prop_id::PALETTE_THUMBNAIL_GROUP))
            .map(|k| k.group_id)
            .filter(|g| *g != 0 && *g != u32::MAX)
            .unwrap_or(palette_group);
        for (ci, c) in out.palette.categories.iter().enumerate() {
            let group = if out.brush_tab == Some(ci) { palette_group } else { setup_group };
            for item in c.pages.iter().filter_map(|pi| out.palette.pages.get(*pi)).flat_map(|p| p.items.iter()) {
                out.cell.themes.entry(item.part).or_insert_with(|| PaintTheme {
                    instance: item.part,
                    name: String::new(),
                    paints: vec![item.part],
                    colors: vec![],
                    icon: ResourceKey { type_id: PNG, group_id: group, instance_id: item.part },
                });
            }
        }
        out.default_theme = if family == Family::Building { name_id("be_theme_default") } else { name_id("ve_theme_default") };
        out
    }

    pub fn sub_tabs(&self) -> Vec<usize> {
        (0..self.filters.len()).filter(|i| self.filters[*i].is_some()).collect()
    }

    fn page(&self, tab: usize, page: usize) -> Option<crate::cell_editor_data::PalettePage> {
        let p = self.palette.page(tab, page)?;
        Some(crate::cell_editor_data::PalettePage {
            key: prop_key(0, p.instance),
            sequence: p.sequence,
            items: p.items.iter().map(|i| crate::cell_editor_data::PaletteItem { part: i.part, row: i.row, column: i.column }).collect(),
            rows: p.rows,
            columns: p.columns,
            item_width_fraction: p.item_width_fraction,
        })
    }
}

/// The paint palettes by setup (the tile icons follow the class).
#[derive(Resource, Default)]
pub struct RigidPaintData {
    pub families: HashMap<SetupKind, FamilyPaint>,
}

impl RigidPaintData {
    pub fn family(&mut self, index: &ResourceIndex, kind: SetupKind) -> &FamilyPaint {
        self.families.entry(kind).or_insert_with(|| FamilyPaint::load(index, kind))
    }
}

/// The brush of Paintbrush Mode and the last style of the session.
#[derive(Resource, Default)]
pub struct RigidPaintSession {
    pub active: bool,
    /// Region -> paint of every style applied in this session (a block bought
    /// afterwards takes it: INFERRED from the retail editor).
    pub scheme: BTreeMap<u32, RegionPaint>,
    pub theme: Option<u32>,
    pub brush: Option<(u32, [f32; 3], [f32; 3])>,
    /// Palette presses of the session, for `ui_sounds::editor_paint_events` (retail plays them in
    /// `cEditor::HandleMessage`): styles chosen (`ItemBlockPaintTheme`, `ui_ed_magicpaint`) and
    /// brush paints chosen (`ItemBlockPaint`, `ui_ed_patternselect`).
    pub style_presses: u32,
    pub brush_presses: u32,
}

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

/// Apply a theme to every block of the session (filtered by a sub-tab's
/// regions). Returns true when something changed.
pub fn apply_theme(
    run: &mut RunProgress,
    originals: &Originals,
    cache: &mut RigidPaintCache,
    session: &mut RigidPaintSession,
    group: u32,
    theme: &BlockTheme,
    filter: Option<&[bool]>,
) -> bool {
    let before = run.parts.clone();
    for p in run.parts.iter_mut() {
        let Some(model) = model_of(originals, group, p.definition_id, p.instance) else { continue };
        let regions = cache.region_list(originals, model);
        let block = p.rigid.get_or_insert_with(Default::default);
        theme_block(&mut block.paints, &regions, theme, filter);
    }
    for r in 0..theme.paints.len() as u32 {
        if filter.is_some_and(|f| !f.get(r as usize).copied().unwrap_or(false)) {
            continue;
        }
        if let Some(e) = theme.entry(r) {
            session.scheme.insert(r, e);
        }
    }
    run.parts != before
}

/// Recolour colour 1 (or 2) of the regions a tab covers on every block.
pub fn recolor(run: &mut RunProgress, session: &mut RigidPaintSession, filter: Option<&[bool]>, color: [f32; 3], second: bool) -> bool {
    let before = run.parts.clone();
    let admit = |r: u32| filter.is_none_or(|f| f.get(r as usize).copied().unwrap_or(false));
    for p in run.parts.iter_mut() {
        if let Some(b) = p.rigid.as_mut() {
            for e in b.paints.iter_mut().filter(|e| admit(e.region)) {
                if second {
                    e.color2 = color;
                } else {
                    e.color1 = color;
                }
            }
        }
    }
    for e in session.scheme.values_mut().filter(|e| admit(e.region)) {
        if second {
            e.color2 = color;
        } else {
            e.color1 = color;
        }
    }
    run.parts != before
}

/// Paintbrush: one region of one block (and its symmetric twin).
pub fn brush_region(run: &mut RunProgress, index: usize, entry: RegionPaint) -> bool {
    let Some(pair) = run.parts.get(index).map(|p| p.pair_id) else { return false };
    let before = run.parts.clone();
    for (i, p) in run.parts.iter_mut().enumerate() {
        if i == index || (pair != 0 && p.pair_id == pair) {
            let block = p.rigid.get_or_insert_with(Default::default);
            set_region(&mut block.paints, entry.clone());
        }
    }
    run.parts != before
}

/// Blocks bought after a style was applied take the session's scheme.
pub fn fill_new_blocks(
    rigid: Res<RigidEditor>,
    originals: Res<Originals>,
    mut cache: ResMut<RigidPaintCache>,
    mut session: ResMut<RigidPaintSession>,
    mut run: ResMut<RunProgress>,
) {
    if !rigid.active {
        session.active = false;
        return;
    }
    let group = rigid.kind.part_group();
    if !session.active {
        // A new session: the scheme of the loaded design (last entry wins).
        *session = RigidPaintSession { active: true, ..Default::default() };
        for p in &run.parts {
            for e in p.rigid.iter().flat_map(|b| b.paints.iter()) {
                session.scheme.insert(e.region, e.clone());
            }
        }
    }
    if session.scheme.is_empty() || !run.parts.iter().any(|p| p.rigid.as_ref().is_some_and(|b| b.paints.is_empty())) {
        return;
    }
    let scheme = session.scheme.clone();
    let mut fills = Vec::new();
    for (i, p) in run.parts.iter().enumerate() {
        if !p.rigid.as_ref().is_some_and(|b| b.paints.is_empty()) {
            continue;
        }
        let Some(model) = model_of(&originals, group, p.definition_id, p.instance) else { continue };
        let paints: Vec<RegionPaint> = cache.region_list(&originals, model).into_iter().filter_map(|r| scheme.get(&r).cloned()).collect();
        if !paints.is_empty() {
            fills.push((i, paints));
        }
    }
    for (i, paints) in fills {
        if let Some(b) = run.parts[i].rigid.as_mut() {
            b.paints = paints;
        }
    }
}

// ---------------------------------------------------------------------------
// Paint palette UI
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigidPaintRoot {
    Tabs,
    Subs,
    Page,
    Swatches,
    Pager,
}

/// Control id of the sub-tab row host in the paint category layout.
const SUB_ROW: u32 = 0x05D1_22C0;
/// `editors~` image group (sub-tab frames).
const EDITORS_GROUP: u32 = 0x100D_976E;

fn sub_frames() -> [ResourceKey; 3] {
    ["norm", "hl", "on-norm"].map(|s| ResourceKey { type_id: PNG, group_id: EDITORS_GROUP, instance_id: name_id(&format!("edt-button-partialTheme-{s}")) })
}

/// The region sub-tabs: frame + icon buttons (control = palette category + 1).
fn subs_layout(fam: &FamilyPaint, area: [f32; 4]) -> (crate::spui::model::Layout, Vec<(usize, usize, usize)>) {
    use crate::spui::{
        ids::{class, flag},
        model::{ButtonData, Drawable, Extra, ImageDrawable, Layout, Window, WindowKind},
    };
    let pic = |key: ResourceKey| Some(Drawable::Image(ImageDrawable { image: Some(image(key)), ..Default::default() }));
    let subs = fam.sub_tabs();
    let n = subs.len().max(1) as f32;
    let (w, h) = (area[2] - area[0], area[3] - area[1]);
    let mut windows = vec![crate::cell_editor_ui::plain(0, None, area, None, 0xFFFF_FFFF)];
    let mut ids = Vec::new();
    let (bw, bh) = (52.0f32.min(w / n), 27.0f32.min(h));
    let y = ((h - bh) * 0.5).round();
    for (slot, &ci) in subs.iter().enumerate() {
        let x = ((slot as f32 + 0.5) * w / n - bw * 0.5).round();
        let b = windows.len();
        windows.push(Window {
            class: class::BUTTON,
            kind: WindowKind::Button,
            flags: flag::VISIBLE | flag::ENABLED,
            control_id: ci as u32 + 1,
            extra: Extra::Button(ButtonData::default()),
            ..crate::cell_editor_ui::plain(b, Some(0), [x, y, x + bw, y + bh], None, 0xFFFF_FFFF)
        });
        windows[0].children.push(b);
        let icon = windows.len();
        let ix = ((bw - 29.0) * 0.5).round();
        windows.push(crate::cell_editor_ui::plain(icon, Some(b), [ix, 0.0, ix + 29.0, bh], fam.sub_icons[ci].and_then(pic), 0xFFFF_FFFF));
        let frame = windows.len();
        windows.push(crate::cell_editor_ui::plain(frame, Some(b), [0.0, 0.0, bw, bh], pic(sub_frames()[0]), 0xFFFF_FFFF));
        windows[b].children = vec![icon, frame];
        ids.push((b, frame, ci));
    }
    (Layout { version: 0, windows, roots: vec![0], hit_masks: vec![], warnings: vec![], unknown_classes: vec![] }, ids)
}

#[derive(Default)]
pub struct RigidPaintUi {
    tab: usize,
    page: usize,
    sub: usize,
    built: Option<(usize, usize, [f32; 4], SetupKind)>,
    tabs: Vec<(usize, usize, usize)>,
    subs: Vec<(usize, usize, usize)>,
    tiles: Vec<(usize, usize, u32)>,
    swatches: Vec<(usize, usize)>,
    demo_done: bool,
}

fn rect_of(ui: &SpuiState, control: u32) -> Option<[f32; 4]> {
    let w = ui.find(control)?;
    let r = ui.rect(w);
    Some([r.x1, r.y1, r.x2, r.y2])
}

fn to_local(root: &Transform, p: Vec3) -> Vec3 {
    root.rotation.conjugate() * (p - root.translation) / root.scale
}

/// The paint palette of the rigid editors and the Paintbrush clicks on the
/// blocks (one undo step each).
#[allow(clippy::too_many_arguments)]
pub fn paint_ui(
    mut commands: Commands,
    (keys, mouse): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    mut ui_state: Local<RigidPaintUi>,
    (mut data, mut cache, mut session): (ResMut<RigidPaintData>, ResMut<RigidPaintCache>, ResMut<RigidPaintSession>),
    (editor, rigid): (Res<CreatureEditor>, Res<RigidEditor>),
    mut state: ResMut<EditorState>,
    mut run: ResMut<RunProgress>,
    originals: Res<Originals>,
    (mut assets, mut images, meshes): (ResMut<SpuiAssets>, ResMut<Assets<Image>>, Res<Assets<Mesh>>),
    pointer: Res<crate::spui::view::SpuiPointer>,
    windows: Query<&Window>,
    cameras: Query<(Entity, &Camera, &GlobalTransform), With<crate::GameCamera>>,
    previews: Query<&Transform, With<RigidPreview>>,
    hosts: Query<(&SpuiState, &EditorRoot), Without<RigidPaintRoot>>,
    mut roots: Query<(Entity, &mut SpuiState, &RigidPaintRoot), Without<EditorRoot>>,
) {
    let index = &originals.resource_index;
    let family = Family::of(editor.kind).filter(|_| rigid.active);
    let on = editor.mode == Mode::Paint && !editor.ui_hidden && family.is_some();
    if !on {
        for (e, _, _) in &roots {
            commands.entity(e).despawn();
        }
        ui_state.built = None;
        return;
    }
    let fam = data.family(index, editor.kind).clone();
    let tabs = fam.palette.categories.len();
    if tabs == 0 {
        return;
    }
    let group = rigid.kind.part_group();
    let subs = fam.sub_tabs();
    let is_brush = |tab: usize| fam.brush_tab == Some(tab);
    // Clicks and keys.
    let mut clicks = Vec::new();
    for (_, mut ui, kind) in &mut roots {
        for w in ui.take_clicks() {
            clicks.push((*kind, ui.layout.window(w).control_id));
        }
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let before = run.clone();
    let mut changed = false;
    let top_click = |n: usize, ui_state: &mut RigidPaintUi| {
        ui_state.page = 0;
        ui_state.tab = match n {
            0 => 0,
            1 if !subs.is_empty() => subs[ui_state.sub.min(subs.len() - 1)],
            _ => fam.brush_tab.unwrap_or(0),
        };
    };
    for (kind, control) in clicks {
        match (kind, control) {
            (RigidPaintRoot::Tabs, c) if c >= 1 => {
                // `tab_layout` controls are top-tab index + 1.
                let name = fam.top_tabs.categories.get(c as usize - 1).map(|c| c.instance);
                let n = if name == fam.palette.categories.first().map(|c| c.instance) {
                    0
                } else if fam.brush_tab.is_some() && name == fam.brush_tab.and_then(|b| fam.palette.categories.get(b)).map(|c| c.instance) {
                    2
                } else {
                    1
                };
                top_click(n, &mut ui_state);
            }
            (RigidPaintRoot::Subs, c) if c >= 1 => {
                let ci = c as usize - 1;
                if let Some(s) = subs.iter().position(|x| *x == ci) {
                    ui_state.sub = s;
                    ui_state.tab = ci;
                    ui_state.page = 0;
                }
            }
            (RigidPaintRoot::Page, item) if is_brush(ui_state.tab) => {
                let colors = session.brush.map(|b| (b.1, b.2)).or_else(|| cache.paint(index, item).map(|p| (p.colors[0], p.colors[1])));
                if let Some((c1, c2)) = colors {
                    session.brush = Some((item, c1, c2));
                }
                session.brush_presses = session.brush_presses.wrapping_add(1);
            }
            (RigidPaintRoot::Page, item) => {
                session.style_presses = session.style_presses.wrapping_add(1);
                if let Some(theme) = cache.theme(index, item) {
                    let filter = fam.filters.get(ui_state.tab).cloned().flatten();
                    changed |= apply_theme(&mut run, &originals, &mut cache, &mut session, group, &theme, filter.as_deref());
                    session.theme = Some(item);
                }
            }
            (RigidPaintRoot::Swatches, c) if c >= 1 => {
                let Some(color) = fam.cell.swatches.get(c as usize - 1).copied() else { continue };
                if is_brush(ui_state.tab) {
                    if let Some(b) = session.brush.as_mut() {
                        if shift {
                            b.2 = color;
                        } else {
                            b.1 = color;
                        }
                    }
                } else {
                    let filter = fam.filters.get(ui_state.tab).cloned().flatten();
                    changed |= recolor(&mut run, &mut session, filter.as_deref(), color, shift);
                }
            }
            (RigidPaintRoot::Pager, page_id::PAGE_PREV) => ui_state.page = ui_state.page.saturating_sub(1),
            (RigidPaintRoot::Pager, page_id::PAGE_NEXT) => ui_state.page += 1,
            _ => {}
        }
    }
    if !editor.naming && !editor.options_open {
        for (n, k) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3].iter().enumerate() {
            if keys.just_pressed(*k) {
                top_click(n, &mut ui_state);
            }
        }
    }
    // Capture option `--rigid-paint-demo`: the second style of the first
    // Complete Styles page, then the first brush on the first block.
    if !ui_state.demo_done && !run.parts.is_empty() && std::env::args().any(|a| a == "--rigid-paint-demo") {
        ui_state.demo_done = true;
        let theme = fam.palette.page(0, 0).and_then(|p| p.items.get(1).or(p.items.first())).map(|i| i.part);
        if let Some(theme) = theme.and_then(|t| cache.theme(index, t)) {
            changed |= apply_theme(&mut run, &originals, &mut cache, &mut session, group, &theme, None);
            session.theme = Some(theme.instance);
        }
    }
    // Paintbrush: a click on a block paints the region under the cursor.
    if is_brush(ui_state.tab) && mouse.just_pressed(MouseButton::Left) && !pointer.over_ui {
        if let (Some(brush), Ok(window), Ok((_, camera, cam)), Ok(root)) = (session.brush, windows.single(), cameras.single(), previews.single()) {
            let root = holder_of(root);
            let cursor = window.cursor_position();
            let over_ui = cursor.is_some_and(|c| state.retail_layout.is_some_and(|l| l.over_ui(c)));
            if let (false, Some(ray)) = (over_ui, cursor.and_then(|c| camera.viewport_to_world(cam, c).ok())) {
                let o = to_local(&root, ray.origin);
                let d = (root.rotation.conjugate() * *ray.direction).normalize_or(Vec3::NEG_Z);
                let mut best: Option<(f32, usize, u32)> = None;
                for (i, p) in run.parts.iter().enumerate() {
                    let Some(model) = model_of(&originals, group, p.definition_id, p.instance) else { continue };
                    let pose = crate::rigid_editor_edit::pose_of(p, Some(model));
                    let regions = cache.regions(&originals, model);
                    for s in 0..model.surfaces.len() {
                        let (Some(region), Some(hull)) = (regions.get(s).copied().flatten(), cache.surface_hull(&meshes, model, s)) else {
                            continue;
                        };
                        if let Some((t, _)) = crate::rigid_editor_edit::ray_block(o, d, &pose, &hull) {
                            if best.is_none_or(|b| t < b.0) {
                                best = Some((t, i, region));
                            }
                        }
                    }
                }
                if let Some((_, i, region)) = best {
                    changed |= brush_region(&mut run, i, RegionPaint { region, paint: brush.0, color1: brush.1, color2: brush.2 });
                    session.scheme.entry(region).or_insert(RegionPaint { region, paint: brush.0, color1: brush.1, color2: brush.2 });
                }
            }
        }
    }
    if changed {
        state.record_external(before);
        run.request_save();
    }
    ui_state.tab = ui_state.tab.min(tabs - 1);
    let pages = fam.palette.page_count(ui_state.tab).max(1);
    ui_state.page = ui_state.page.min(pages - 1);
    // Host rectangles from the editor's paint layouts.
    let (mut strip, mut area, mut swatch, mut pager, mut row) = (None, None, None, None, None);
    for (ui, kind) in &hosts {
        if !ui.ready {
            continue;
        }
        match kind {
            EditorRoot::PaintPalette => strip = rect_of(ui, id::TABS),
            EditorRoot::PaintCategory => {
                area = rect_of(ui, id::PAGE_AREA);
                swatch = rect_of(ui, page_id::SWATCH_AREA);
                pager = rect_of(ui, page_id::PAGE_HOST);
                row = rect_of(ui, SUB_ROW);
            }
            _ => {}
        }
    }
    let (Some(strip), Some(area), Some(swatch), Ok((camera, _, _))) = (strip, area, swatch, cameras.single()) else {
        return;
    };
    let key = (ui_state.tab, ui_state.page, area, editor.kind);
    if ui_state.built != Some(key) {
        for (e, _, _) in &roots {
            commands.entity(e).despawn();
        }
        let (l, tab_ids, _) = crate::creature_editor_ui::tab_layout(&fam.top_tabs, strip, 0);
        let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
        commands.entity(root).insert((RigidPaintRoot::Tabs, CreatureEditorOwned));
        ui_state.tabs = tab_ids;
        ui_state.subs.clear();
        if let (true, Some(row)) = (subs.contains(&ui_state.tab), row) {
            let (l, ids) = subs_layout(&fam, row);
            let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 100);
            commands.entity(root).insert((RigidPaintRoot::Subs, CreatureEditorOwned));
            ui_state.subs = ids;
        }
        if let Some(page) = fam.page(ui_state.tab, ui_state.page) {
            let (l, tiles) = tile_layout(&fam.cell, &page, area, &fam.cell.art, vec![]);
            let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 99);
            commands.entity(root).insert((RigidPaintRoot::Page, CreatureEditorOwned));
            ui_state.tiles = tiles;
        }
        let (l, sw) = swatch_layout(&fam.cell.art, &fam.cell.swatches, swatch, vec![]);
        let root = view::spawn(&mut commands, Arc::new(l), RootPolicy::Authored, Some(camera), 100);
        commands.entity(root).insert((RigidPaintRoot::Swatches, CreatureEditorOwned));
        ui_state.swatches = sw;
        if pages > 1 {
            if let Some(l) = assets.layout(index, "EditorPageControls") {
                let root = view::spawn(&mut commands, l, RootPolicy::Authored, Some(camera), 99);
                commands.entity(root).insert((RigidPaintRoot::Pager, CreatureEditorOwned));
            }
        }
        ui_state.built = Some(key);
    }
    for c in &fam.top_tabs.categories {
        for icon in c.icons.iter().take(8) {
            assets.texture(index, &mut images, *icon);
        }
    }
    for key in fam.sub_icons.iter().flatten().chain(sub_frames().iter()) {
        assets.texture(index, &mut images, *key);
    }
    // States: tab icons, the chosen style / brush and colour glows, the counter.
    let top_now = if ui_state.tab == 0 {
        0
    } else if is_brush(ui_state.tab) {
        fam.top_tabs.categories.len().saturating_sub(1)
    } else {
        1
    };
    let brush_tab = is_brush(ui_state.tab);
    let shown_color = if brush_tab { session.brush.map(|b| b.1) } else { None };
    for (_, mut ui, kind) in &mut roots {
        if !ui.ready {
            continue;
        }
        match kind {
            RigidPaintRoot::Tabs => {
                for (b, icon, ci) in &ui_state.tabs {
                    let st = ui.states[*b];
                    let Some(c) = fam.top_tabs.categories.get(*ci) else { continue };
                    if let Some(file) = c.icon(*ci == top_now, st & state::HOVER != 0, st & state::CLICKED != 0, false) {
                        ui.set_image(*icon, Some(image(file)));
                    }
                    ui.set_tooltip(*b, Some(c.name.clone()));
                }
            }
            RigidPaintRoot::Subs => {
                for (b, frame, ci) in &ui_state.subs {
                    let hover = ui.states[*b] & state::HOVER != 0;
                    let f = if *ci == ui_state.tab { 2 } else if hover { 1 } else { 0 };
                    ui.set_image(*frame, Some(image(sub_frames()[f])));
                    ui.set_tooltip(*b, fam.palette.categories.get(*ci).map(|c| c.name.clone()));
                }
            }
            RigidPaintRoot::Page => {
                for (_, glow, item) in &ui_state.tiles {
                    let on = if brush_tab { session.brush.is_some_and(|b| b.0 == *item) } else { session.theme == Some(*item) };
                    if ui.visible(*glow) != on {
                        ui.set_visible(*glow, on);
                    }
                }
            }
            RigidPaintRoot::Swatches => {
                for (i, (_, glow)) in ui_state.swatches.iter().enumerate() {
                    let on = shown_color.is_some_and(|c| fam.cell.swatches.get(i).is_some_and(|s| same_color(*s, c)));
                    if ui.visible(*glow) != on {
                        ui.set_visible(*glow, on);
                    }
                }
            }
            RigidPaintRoot::Pager => {
                if let (Some(r), Some(host)) = (ui.layout.roots.first().copied(), pager) {
                    ui.set_area(r, host);
                }
                if let Some(w) = ui.find(page_id::PAGE_TEXT) {
                    ui.set_text(w, crate::cell_editor_paint::page_text(ui_state.page, pages));
                }
                if let Some(w) = ui.find(page_id::PAGE_PREV) {
                    ui.set_enabled(w, ui_state.page > 0);
                }
                if let Some(w) = ui.find(page_id::PAGE_NEXT) {
                    ui.set_enabled(w, ui_state.page + 1 < pages);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Captures and the GPU test
// ---------------------------------------------------------------------------

/// Switch the open editor's mode (captures, the GPU test).
pub(crate) fn enter_mode(world: &mut World, mode: Mode) {
    use bevy::ecs::system::RunSystemOnce;
    let _ = world.run_system_once(
        move |mut editor: ResMut<CreatureEditor>,
              state: Res<EditorState>,
              data: Res<crate::creature_editor_data::CreatureEditorData>,
              originals: Res<Originals>| {
            crate::creature_editor::set_mode(&mut editor, &state, &data, &originals, mode);
        },
    );
}

/// Apply style `nth` of the first Complete Styles page to the session (one
/// undo step, as a click on it). Returns the style.
pub(crate) fn demo_theme(world: &mut World, nth: usize) -> Option<u32> {
    use bevy::ecs::system::RunSystemOnce;
    world
        .run_system_once(
            move |rigid: Res<RigidEditor>,
                  originals: Res<Originals>,
                  (mut data, mut cache, mut session): (ResMut<RigidPaintData>, ResMut<RigidPaintCache>, ResMut<RigidPaintSession>),
                  mut run: ResMut<RunProgress>,
                  mut state: ResMut<EditorState>|
                  -> Option<u32> {
                Family::of(rigid.kind)?;
                let page = data.family(&originals.resource_index, rigid.kind).palette.page(0, 0)?.clone();
                let item = page.items.get(nth).or(page.items.first())?.part;
                let theme = cache.theme(&originals.resource_index, item)?;
                let before = run.clone();
                if apply_theme(&mut run, &originals, &mut cache, &mut session, rigid.kind.part_group(), &theme, None) {
                    state.record_external(before);
                    run.request_save();
                }
                session.theme = Some(item);
                for p in &run.parts {
                    let regions = model_of(&originals, rigid.kind.part_group(), p.definition_id, p.instance).map(|m| cache.regions(&originals, m));
                    let paints: Vec<(u32, u32)> = p.rigid.iter().flat_map(|b| b.paints.iter().map(|e| (e.region, e.paint))).collect();
                    info!("rigid paint demo: block {:08X} regions {regions:?} paints {paints:08X?}", p.definition_id.unwrap_or(p.instance));
                }
                Some(item)
            },
        )
        .ok()
        .flatten()
}

/// The first Paintbrush paint with `color` on the first region of block 0
/// (and its twin), as a brush click on it.
pub(crate) fn demo_brush(world: &mut World, color: [f32; 3]) -> Option<RegionPaint> {
    use bevy::ecs::system::RunSystemOnce;
    world
        .run_system_once(
            move |rigid: Res<RigidEditor>,
                  originals: Res<Originals>,
                  (mut data, mut cache): (ResMut<RigidPaintData>, ResMut<RigidPaintCache>),
                  mut run: ResMut<RunProgress>,
                  mut state: ResMut<EditorState>|
                  -> Option<RegionPaint> {
                Family::of(rigid.kind)?;
                let fam = data.family(&originals.resource_index, rigid.kind);
                let paint = fam.palette.page(fam.brush_tab?, 0)?.items.first()?.part;
                let p = run.parts.first()?;
                let model = model_of(&originals, rigid.kind.part_group(), p.definition_id, p.instance)?;
                let region = *cache.region_list(&originals, model).first()?;
                let colors = cache.paint(&originals.resource_index, paint)?.colors;
                let entry = RegionPaint { region, paint, color1: color, color2: colors[1] };
                let before = run.clone();
                if brush_region(&mut run, 0, entry.clone()) {
                    state.record_external(before);
                    run.request_save();
                }
                Some(entry)
            },
        )
        .ok()
        .flatten()
}

/// (painted, total) block surfaces drawn: painted = showing a block paint
/// material instead of the imported one.
pub(crate) fn painted_surfaces(world: &mut World) -> (usize, usize) {
    let mut q = world.query::<(&RigidSurfacePaint, &MeshMaterial3d<StandardMaterial>)>();
    let list: Vec<(Option<ResourceKey>, usize, AssetId<StandardMaterial>)> =
        q.iter(world).map(|(s, m)| (s.definition, s.surface, m.0.id())).collect();
    let originals = world.resource::<Originals>();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let mut painted = 0;
    for (definition, surface, material) in &list {
        let native = definition
            .and_then(|d| originals.mechanical_models.get(&d))
            .and_then(|m| m.surfaces.get(*surface))
            .map(|s| s.material.id());
        if native != Some(*material) && materials.get(*material).is_some_and(|m| m.base_color_texture.is_some()) {
            painted += 1;
        } else {
            debug!("rigid paint: unpainted surface {surface} of {definition:?}");
        }
    }
    (painted, list.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> BlockTheme {
        BlockTheme {
            instance: 1,
            paints: vec![10, 0, 12, 13],
            color1s: vec![[1.0, 0.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            color2s: vec![[0.5; 3]; 4],
        }
    }

    #[test]
    fn themes_paint_the_regions_a_block_has_and_filters_limit_them() {
        let t = theme();
        let mut paints = Vec::new();
        theme_block(&mut paints, &[0, 1, 3], &t, None);
        // Region 1 is left by the theme; region 2 is not on the block.
        assert_eq!(paints.iter().map(|p| (p.region, p.paint)).collect::<Vec<_>>(), vec![(0, 10), (3, 13)]);
        let mut filtered = Vec::new();
        theme_block(&mut filtered, &[0, 2, 3], &t, Some(&[false, false, true, false]));
        assert_eq!(filtered.iter().map(|p| p.region).collect::<Vec<_>>(), vec![2]);
        set_region(&mut paints, RegionPaint { region: 0, paint: 99, color1: [0.0; 3], color2: [0.0; 3] });
        assert_eq!(paints[0].paint, 99);
        assert_eq!(paints.len(), 2);
        // A region without its own paint shows the default theme's.
        assert_eq!(resolve(&paints, 2, Some(&t)).map(|p| p.paint), Some(12));
        assert_eq!(resolve(&paints, 0, Some(&t)).map(|p| p.paint), Some(99));
        assert_eq!(resolve(&paints, 1, Some(&t)), None);
    }

    #[test]
    fn block_paint_shading_uses_alpha_mask_and_blend_factors() {
        let paint = BlockPaint { blend: [0.0, 1.0], ..Default::default() };
        // A matte texel (white, opaque) is flat colour 1.
        let out = compose(&[255, 255, 255, 255], &paint, [0.2, 0.4, 0.6], [1.0; 3]);
        assert_eq!(&out[..3], &[51, 102, 153]);
        // A transparent mid-grey texel is colour 2 unchanged.
        let out = compose(&[128, 128, 128, 0], &paint, [1.0; 3], [0.5, 0.5, 0.5]);
        assert!((out[0] as i32 - 128).abs() <= 1, "{out:?}");
        // Dark detail darkens colour 2 (blend 1), not colour 1 (blend 0).
        let dark = compose(&[64, 64, 64, 0], &paint, [1.0; 3], [1.0; 3]);
        assert!(dark[0] < 140);
        let flat = compose(&[64, 64, 64, 255], &paint, [1.0; 3], [1.0; 3]);
        assert_eq!(flat[0], 255);
    }

    #[test]
    #[ignore = "installed data"]
    fn installed_paint_palettes_regions_and_themes() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        for family in [SetupKind::VehicleMilitaryLand, SetupKind::BuildingCityHall, SetupKind::Ufo] {
            let fam = FamilyPaint::load(&index, family);
            let names: Vec<_> = fam.palette.categories.iter().map(|c| c.name.clone()).collect();
            println!("{family:?}: tabs {names:?} subs {:?} brush {:?} top {}", fam.sub_tabs(), fam.brush_tab, fam.top_tabs.categories.len());
            assert_eq!(fam.sub_tabs().len(), 3, "{family:?}");
            assert!(fam.brush_tab.is_some(), "{family:?}");
            assert_eq!(fam.top_tabs.categories.len(), 3);
            let items: usize = fam.palette.pages.iter().map(|p| p.items.len()).sum();
            assert!(items > 50, "{family:?} {items}");
        }
        let ve = load_theme(&index, name_id("ve_theme_default")).unwrap();
        let be = load_theme(&index, name_id("be_theme_default")).unwrap();
        assert_eq!((ve.paints.len(), be.paints.len()), (51, 18));
        let matte = load_paint(&index, name_id("VE_Old_Car_Matte_01")).unwrap();
        assert_eq!(matte.blend, [0.0, 1.0]);
        // Region of every mesh of a part: `ve_landcraft_08` = 10, 19, 14.
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let mut originals = Originals { part_catalog: catalog, resource_index: index, ..Default::default() };
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let def = originals.part_catalog.by_definition_in(crate::asset_catalog::VEHICLE_PARTS, name_id("ve_landcraft_08")).cloned().unwrap();
        let model = crate::mechanical_assets::import(&mut originals, &def, &mut meshes, &mut images, &mut materials).unwrap().model;
        let mut cache = RigidPaintCache::default();
        assert_eq!(*cache.regions(&originals, &model), vec![Some(10), Some(19), Some(14)]);
        let m = cache.surface_material(&originals, &mut images, &mut materials, &model, 0, &[], false);
        assert!(m.is_some(), "default theme material");
        // Regions stay inside the filters of their family.
        // Vehicle regions run past the 34 filter entries (e.g. 50; themes list 51).
        for (group, limit) in [(crate::asset_catalog::VEHICLE_PARTS, 51), (crate::asset_catalog::BUILDING_PARTS, 51)] {
            let defs: Vec<_> = crate::mechanical_assets::eligible(&originals, group).into_iter().take(40).cloned().collect();
            let mut seen = std::collections::BTreeSet::new();
            for d in defs {
                let Ok(m) = crate::mechanical_assets::import(&mut originals, &d, &mut meshes, &mut images, &mut materials) else { continue };
                for r in cache.regions(&originals, &m.model).iter().flatten() {
                    seen.insert(*r);
                }
            }
            println!("group {group:08X}: regions {seen:?}");
            assert!(!seen.is_empty() && seen.iter().all(|r| *r < limit), "{group:08X} {seen:?}");
        }
    }
}
