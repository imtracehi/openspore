//! A generation record's own skin paint for the History thumbnails.
//!
//! The Cell Creator paints the shared body and part textures in place from the live paint (`cell_paint::sync`
//! keeps one body target and one texture per mounted part instance). A past generation's thumbnail needs the paint
//! its creature had, so this module runs the same retail painter ([`cell_skinpaint`]: the style's brush script on the
//! body surface and on every part instance's chart, the installed `skpSplatTintShader` for the parts) into material
//! copies of its own. Everything it calls is public in `cell_paint` / `cell_skinpaint`; the three mesh-to-surface
//! helpers (`body_surface`, `body_bones`, `part_chart`) are private there and are mirrored here (same maths, same
//! arguments; keep them in step).
//!
//! The rules follow `cell_paint::sync` (VERIFIED there, see its comments): a painted cell is painted with its
//! style and colour; an unpainted one keeps the installed body, its parts use the default style's colour (the
//! Creator always paints with it); an edited body (a vertebra chain) with no paint is painted with the neutral
//! cell's own paint (`CellBody::neutral_paint`). ADAPTED: the neutral cell's baked body, when a thumbnail has to show
//! it while the live cell is painted, is painted with that same neutral paint instead of being read back from the
//! installed atlas (the live material no longer holds it); the starter is that paint's output.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    asset_catalog::CELL_PARTS,
    cell_editor_scene as scene,
    cell_paint::{self, CellPaint, CellPaintData},
    cell_skinpaint,
    creature_skin_paint::{Brush, Script},
    dbpf::ResourceKey,
    game::PartInstance,
    texture::Pixels,
    Originals,
};

/// The painter state of the thumbnails: the palette data, the decoded brush scripts and brushes (the Creator's own
/// copies are private to `cell_paint`), and the decoded definition textures of the parts.
#[derive(Default)]
pub struct ThumbPainter {
    data: Option<CellPaintData>,
    library: Option<Result<cell_skinpaint::Library, String>>,
    scripts: HashMap<u32, Option<(Script, u32)>>,
    brushes: HashMap<String, Brush>,
    /// Authored diffuse and tint mask of a part definition (by definition instance).
    parts: HashMap<u32, Option<(Pixels, Option<Pixels>)>>,
}

/// The paint a record's thumbnail shows: the body's and the parts'.
#[derive(Clone, Debug, PartialEq)]
pub struct Effective {
    pub body: CellPaint,
    pub parts: CellPaint,
}

impl ThumbPainter {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn data(&mut self, originals: &Originals) -> &CellPaintData {
        self.data.get_or_insert_with(|| CellPaintData::load(&originals.resource_index))
    }

    /// The paints a record with `paint` shows (the rules of `cell_paint::sync`). An unpainted record has the neutral
    /// cell's own paint on its body: that is what an edited chain gets, and what stands in for the installed atlas
    /// of the baked body (ADAPTED, see the module doc).
    pub fn effective(&mut self, originals: &Originals, paint: &CellPaint, neutral: Option<(u32, [f32; 3])>) -> Effective {
        let data = self.data(originals);
        let default = CellPaint { theme: data.default_theme, color: data.default_color() };
        let parts = if paint.is_painted() { paint.clone() } else { default.clone() };
        let body = if paint.is_painted() {
            paint.clone()
        } else {
            neutral
                .and_then(|(script, color)| {
                    data.themes
                        .values()
                        .find(|t| t.paints.first() == Some(&script))
                        .map(|t| CellPaint { theme: t.instance, color })
                })
                .unwrap_or(default)
        };
        Effective { body, parts }
    }

    /// Make sure the brush script of `theme` and its brushes are loaded; `false` when it is not available.
    fn ensure_script(&mut self, originals: &Originals, theme: u32) -> bool {
        let index = &originals.resource_index;
        let Some(info) = self.data(originals).themes.get(&theme).cloned() else {
            return false;
        };
        let library = self.library.get_or_insert_with(|| cell_skinpaint::Library::load(index));
        let Ok(library) = library else {
            return false;
        };
        let entry = self.scripts.entry(theme).or_insert_with(|| {
            let id = *info.paints.first()?;
            library.script(id).map(|s| (s, id))
        });
        let Some((script, _)) = entry else {
            return false;
        };
        for name in script.brushes() {
            if !self.brushes.contains_key(&name) {
                let brush = cell_skinpaint::load_brush(index, &name);
                self.brushes.insert(name, brush);
            }
        }
        true
    }

    /// A copy of `live`, the body material, painted with `paint` on the body mesh `mesh`. `bones`: the dominant
    /// vertebra of every vertex of an edited body (`None`: the baked body's own partition). `None` when the style's
    /// script is not available.
    #[allow(clippy::too_many_arguments)]
    pub fn body_material(
        &mut self,
        originals: &Originals,
        paint: &CellPaint,
        mesh: &Handle<Mesh>,
        bones: Option<&[u8]>,
        scale: f32,
        live: &Handle<StandardMaterial>,
        meshes: &Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<StandardMaterial>> {
        if !paint.is_painted() || !self.ensure_script(originals, paint.theme) {
            return None;
        }
        let surface = body_surface(meshes, mesh, scale, originals, bones)?;
        let (script, id) = self.scripts.get(&paint.theme)?.as_ref()?;
        let mut textures = cell_skinpaint::paint_body(&surface, script, *id, paint.color, &self.brushes);
        if let Some(native) = originals.native_painter.as_ref() {
            textures = textures.with_native_normals(native);
        }
        let mut material = materials.get(live)?.clone();
        let size = cell_skinpaint::SIZE;
        let mut repaint = |texture: &Option<Handle<Image>>, rgba: &[u8]| -> Option<Handle<Image>> {
            let mut image = images.get(texture.as_ref()?)?.clone();
            let (w, h) = (image.width(), image.height());
            image.data = Some(cell_paint::with_mips(
                &cell_skinpaint::resample(rgba, (size, size), (w, h)),
                w,
                h,
                image.texture_descriptor.mip_level_count,
            ));
            Some(images.add(image))
        };
        if let Some(diffuse) = repaint(&material.base_color_texture, &textures.diffuse) {
            material.base_color_texture = Some(diffuse);
        }
        if let Some(normal) = repaint(&material.normal_map_texture, &textures.normal_spec) {
            material.normal_map_texture = Some(normal);
        }
        let painted = materials.add(material);
        crate::cell_sh::set_params(painted.id(), textures.material_params());
        Some(painted)
    }

    /// A copy of a part model's material painted for the instance `placed` (`parts[index]` of its creature) with
    /// `paint`, as `cell_paint::sync` paints a mounted instance: the style's stamps on the instance's own chart
    /// (its mount scale and draw salt) under the part's authored diffuse, tinted by the paint's layer colours.
    /// `None` keeps the model's own material.
    #[allow(clippy::too_many_arguments)]
    pub fn part_material(
        &mut self,
        originals: &Originals,
        paint: &CellPaint,
        model: &crate::OriginalModel,
        parts: &[PartInstance],
        index: usize,
        body_scale: f32,
        meshes: &Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<StandardMaterial>> {
        let placed = parts.get(index)?;
        let source = model.material.as_ref()?;
        let key = model.source_definition.filter(|k| k.group_id == CELL_PARTS)?;
        let (diffuse, tint) = self
            .parts
            .entry(key.instance_id)
            .or_insert_with(|| {
                let definition = originals.part_catalog.by_definition_in(CELL_PARTS, key.instance_id)?;
                let read = |key: Option<ResourceKey>| {
                    key.and_then(|k| originals.resource_index.read(k).ok()).and_then(|b| crate::texture::decode_rw4(&b))
                };
                Some((read(definition.diffuse)?, read(definition.tint)))
            })
            .clone()?;
        let have_script = self.ensure_script(originals, paint.theme);
        let colors = cell_paint::part_colors(paint.color);
        let layers = cell_skinpaint::layer_colors(paint.color);
        let (under, base) = match self.scripts.get(&paint.theme).and_then(Option::as_ref).filter(|_| have_script) {
            Some((script, id)) => {
                let mount = scene::part_transform(placed, model.scale).scale.y.abs();
                let salt = parts[..index].iter().filter(|q| q.instance == placed.instance).count() as u32;
                let under = (body_scale > 0.0 && mount > 0.0)
                    .then(|| part_chart(meshes, &model.handle, body_scale / mount))
                    .flatten()
                    .map(|chart| {
                        let stamps = cell_skinpaint::paint_chart(&chart, script, *id, paint.color, &self.brushes, salt);
                        Pixels {
                            width: diffuse.width,
                            height: diffuse.height,
                            rgba: cell_skinpaint::resample(
                                &stamps.rgba,
                                (cell_skinpaint::SIZE, cell_skinpaint::SIZE),
                                (diffuse.width, diffuse.height),
                            ),
                        }
                    });
                (under, cell_skinpaint::script_base_color(script, layers))
            }
            None => (None, paint.color),
        };
        let painted = originals
            .native_painter
            .as_ref()
            .and_then(|n| cell_skinpaint::paint_part(n, &diffuse, tint.as_ref(), base, under.as_ref(), colors).ok())
            .unwrap_or_else(|| crate::texture::skinpaint(&diffuse, tint.as_ref(), colors));
        let mut material = materials.get(source)?.clone();
        let mut image = images.get(material.base_color_texture.as_ref()?)?.clone();
        if image.width() != painted.width || image.height() != painted.height || image.texture_descriptor.mip_level_count != 1 {
            return None;
        }
        image.data = Some(painted.rgba);
        material.base_color_texture = Some(images.add(image));
        let copy = materials.add(material);
        // The copy keeps the source's `materialParams` (`cell_sh`).
        crate::cell_sh::copy_params(source.id(), copy.id());
        Some(copy)
    }
}

/// `cell_paint::body_bones` (private there): the dominant body bone of every vertex of the baked body mesh, read
/// from the baked GMDL's own partition (`u8::MAX` = none).
pub fn baked_bones(originals: &Originals, uvs: &[[f32; 2]]) -> Option<Vec<u8>> {
    const CELL_GLOBALS: u32 = 0x2a3c_e5b7;
    let creature = originals.world_models.keys().find(|k| k.type_id == CELL_GLOBALS)?.instance_id;
    let key = ResourceKey {
        type_id: crate::decode::TYPE_GMDL,
        group_id: crate::cell_visuals::CELL_MODEL_GROUP,
        instance_id: creature,
    };
    let raw = crate::baked_model::parse(&originals.resource_index.read(key).ok()?).ok()?;
    if raw.root_bones == 0 || raw.root_bones == u32::MAX || raw.bone_ranges.len() != raw.pieces.len() {
        return None;
    }
    let mut by_uv: HashMap<(u32, u32), u8> = HashMap::new();
    for (i, piece) in raw.pieces.iter().enumerate() {
        let base = raw.bone_ranges[i][0];
        for v in 0..piece.positions.len().min(piece.bones.len()).min(piece.weights.len()).min(piece.uvs.len()) {
            let w = piece.weights[v];
            let j = (0..4).filter(|j| w[*j] > 0.0).max_by(|a, b| w[*a].total_cmp(&w[*b]));
            let bone = j.map(|j| base + piece.bones[v][j] as u32 / 3).filter(|b| *b < raw.root_bones && *b < 255);
            if let Some(bone) = bone {
                by_uv.entry((piece.uvs[v][0].to_bits(), piece.uvs[v][1].to_bits())).or_insert(bone as u8);
            }
        }
    }
    let bones: Vec<u8> = uvs.iter().map(|uv| by_uv.get(&(uv[0].to_bits(), uv[1].to_bits())).copied().unwrap_or(u8::MAX)).collect();
    bones.iter().any(|b| *b != u8::MAX).then_some(bones)
}

/// `cell_paint::body_surface` (private there): a body mesh as the paintable skin.
fn body_surface(
    meshes: &Assets<Mesh>,
    handle: &Handle<Mesh>,
    scale: f32,
    originals: &Originals,
    edited: Option<&[u8]>,
) -> Option<cell_skinpaint::BodySurface> {
    use bevy::mesh::{Indices, VertexAttributeValues as Values};
    let mesh = meshes.get(handle)?;
    let (Some(Values::Float32x3(positions)), Some(Values::Float32x3(normals)), Some(Values::Float32x2(uvs))) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
    ) else {
        return None;
    };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|v| *v as u32).collect(),
    };
    let bones = match edited {
        Some(b) if b.len() == uvs.len() => Some(b.to_vec()),
        _ => baked_bones(originals, uvs),
    };
    cell_skinpaint::BodySurface::build(positions, normals, uvs, &indices, scale, bones.as_deref())
}

/// `cell_paint::part_chart` (private there): a part model's mesh as its chart (`scale` Bevy units per skin unit).
fn part_chart(meshes: &Assets<Mesh>, handle: &Handle<Mesh>, scale: f32) -> Option<cell_skinpaint::PartChart> {
    use bevy::mesh::{Indices, VertexAttributeValues as Values};
    let mesh = meshes.get(handle)?;
    let (Some(Values::Float32x3(positions)), Some(Values::Float32x3(normals)), Some(Values::Float32x2(uvs))) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
    ) else {
        return None;
    };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|v| *v as u32).collect(),
    };
    cell_skinpaint::PartChart::build(positions, normals, uvs, &indices, scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_paint::PaintTheme;

    fn painter_with_data() -> (ThumbPainter, Originals) {
        let mut painter = ThumbPainter::default();
        let originals = Originals::default();
        let icon = ResourceKey { type_id: 0, group_id: 0, instance_id: 0 };
        let mut data = CellPaintData::default();
        data.default_theme = 7;
        data.swatches = vec![[0.25, 0.5, 0.75]];
        data.themes.insert(
            7,
            PaintTheme { instance: 7, name: "default".into(), paints: vec![0xAA], colors: vec![[0.25, 0.5, 0.75]], icon },
        );
        data.themes.insert(
            9,
            PaintTheme { instance: 9, name: "neutral".into(), paints: vec![0xBB], colors: vec![[1.0, 0.0, 0.0]], icon },
        );
        painter.data = Some(data);
        (painter, originals)
    }

    /// An unpainted record: the body takes the neutral cell's own paint, the parts the default style (the Creator
    /// always paints with it), as `cell_paint::sync` does.
    #[test]
    fn unpainted_records_follow_the_creator_rules() {
        let (mut painter, originals) = painter_with_data();
        let none = CellPaint::default();
        let unpainted = painter.effective(&originals, &none, Some((0xBB, [0.1, 0.2, 0.3])));
        assert_eq!(unpainted.body, CellPaint { theme: 9, color: [0.1, 0.2, 0.3] });
        assert_eq!(unpainted.parts.theme, 7, "the default style paints the parts");
        // Without the neutral paint the default style stands in.
        assert_eq!(painter.effective(&originals, &none, None).body.theme, 7);
        // A painted record paints body and parts with its own style and colour.
        let painted = CellPaint { theme: 9, color: [0.9, 0.8, 0.7] };
        let both = painter.effective(&originals, &painted, None);
        assert_eq!((&both.body, &both.parts), (&painted, &painted));
    }
}
