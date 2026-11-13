//! `UTFWin::OutlineFormat` and the outline / halo kernel of the retail window manager.
//!
//! Three kinds of windows carry an outline format (property `0x039A69E6` "Text Outline" /
//! "Image Outline" of the Button, Text, ComboBox, TextEdit, Grid, ImageDrawable and StdDrawable
//! classes) and the `SporeStdDrawableImageInfo` of a `SporeStdDrawable` carries two of them
//! (`Stroke Shadow` 0xA and `Halo Shadow` 0xB) switched by `Stroke Mode` (8) / `Halo Mode` (9).
//!
//! VERIFIED (march2017 code image): `SporeStdDrawable::Paint` 0x8318F0 draws six layers per image
//! info: background halo, background stroke, background, icon halo, icon stroke, icon. A halo or a
//! stroke layer re-draws the layer's image `(2 sizeX + 1) (2 sizeY + 1)` times, one copy per
//! offset `(x, y)` of a unit grid over `[-sizeX, sizeX] x [-sizeY, sizeY]` (x and y run in steps of
//! 1.0 from `-size` while `<= size`), shifted by `(floor(x + offsetX), floor(y + offsetY))` pixels,
//! the copy at a zero shift left out (the layer itself is drawn plain afterwards). The copy is
//! tinted with `(alpha << 24) + (color & 0xFFFFFF)`; the image's own colour is not used:
//!
//! ```text
//! r      = sqrt(sizeX^2 + sizeY^2)
//! d2     = x^2 + y^2                      (the grid is centred on 0: (-s + s) / 2)
//! alpha  = clamp(saturation / (r + 1) * (1 - d2 / (sizeX^2 + sizeY^2 + 1)) + smoothness, 0, 1) * 255
//! ```
//!
//! (rounded to the nearest integer by the x87 `fistp`). The tint colour alpha of the format is
//! ignored. Mode 1 draws the stroke / halo of background and icon, mode 2 of the background only,
//! mode 3 of the icon only (`ShadowModes`). `OutlineFormat::SetStrength` 0x830230 maps the strength
//! preset 1..9 to `smoothness` / `saturation` (see [`strength_preset`]), the size preset 1..9 is
//! `sizeX = sizeY = size` (ModAPI `OutlineFormat::SetSize`).

use super::format::{Prop, PropList};

/// Property ids of the `Outline Format` struct (SporeModder-FX designer definition).
pub mod prop {
    pub const SIZE: u32 = 1;
    pub const STRENGTH: u32 = 2;
    pub const QUALITY: u32 = 3;
    pub const OFFSET_X: u32 = 4;
    pub const OFFSET_Y: u32 = 5;
    pub const SIZE_X: u32 = 6;
    pub const SIZE_Y: u32 = 7;
    pub const SMOOTHNESS: u32 = 8;
    pub const SATURATION: u32 = 9;
    pub const COLOR: u32 = 10;
}

/// Property id of the outline struct on text and image windows / drawables.
pub const PROP_OUTLINE: u32 = 0x039A_69E6;

/// `ShadowModes` of `SporeStdDrawableImageInfo`.
pub const SHADOW_NONE: u32 = 0;
pub const SHADOW_FULL: u32 = 1;
pub const SHADOW_BACKGROUND: u32 = 2;
pub const SHADOW_ICON: u32 = 3;

/// `UTFWin::OutlineFormat` (0x28 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub size: u32,
    pub strength: u32,
    pub quality: u32,
    pub offset: [f32; 2],
    pub size_x: f32,
    pub size_y: f32,
    pub smoothness: f32,
    pub saturation: f32,
    /// `0xAARRGGBB`; the alpha is ignored by the stroke / halo kernel.
    pub color: u32,
}

/// `OutlineFormat::SetStrength` 0x830230: the `(smoothness, saturation)` of a strength preset.
pub fn strength_preset(strength: u32) -> Option<(f32, f32)> {
    Some(match strength {
        1 => (0.0004, 0.25),
        2 => (0.0003, 0.5),
        3 => (0.0002, 0.75),
        4 => (0.0001, 1.0),
        5 => (0.0, 1.25),
        6 => (0.0, 1.5),
        7 => (0.0, 1.85),
        8 => (0.0, 2.35),
        9 => (0.0, 3.15),
        _ => return None,
    })
}

impl Default for Outline {
    /// The `OutlineFormat` of a freshly constructed drawable / image info (constructor
    /// 0x8308F0: size 0, strength 2, quality 3, no offset, `sizeX = sizeY = 0`,
    /// `SetStrength(2)`, colour 0): it draws nothing until a file gives it a size.
    fn default() -> Self {
        let (smoothness, saturation) = strength_preset(2).unwrap_or((0.0003, 0.5));
        Self {
            size: 0,
            strength: 2,
            quality: 3,
            offset: [0.0, 0.0],
            size_x: 0.0,
            size_y: 0.0,
            smoothness,
            saturation,
            color: 0,
        }
    }
}

/// `OS_SPUI_NO_OUTLINE=1` draws no stroke, halo or outline at all (frame time comparisons of
/// the copies, see `view::SyncStats`; not a game setting).
pub fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("OS_SPUI_NO_OUTLINE").is_some())
}

/// One copy of an image or of a text run in a stroke / halo / outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tap {
    /// Pixel shift of the copy.
    pub dx: i32,
    pub dy: i32,
    /// Alpha of the copy's tint (0..=255).
    pub alpha: u8,
}

impl Outline {
    /// Reads the properties of one `Outline Format` struct over the constructor defaults (a
    /// property a file does not store keeps the constructor value), then applies the size and
    /// strength presets like the post-load fix-up 0x830780 (`SetSize`, `SetStrength`).
    pub fn from_props(props: &[Prop]) -> Outline {
        let d = Outline::default();
        let mut o = Outline {
            size: props.u32_at(prop::SIZE, 0).unwrap_or(d.size),
            strength: props.u32_at(prop::STRENGTH, 0).unwrap_or(d.strength),
            quality: props.u32_at(prop::QUALITY, 0).unwrap_or(d.quality),
            offset: [
                props.f32_at(prop::OFFSET_X, 0).unwrap_or(d.offset[0]),
                props.f32_at(prop::OFFSET_Y, 0).unwrap_or(d.offset[1]),
            ],
            size_x: props.f32_at(prop::SIZE_X, 0).unwrap_or(d.size_x),
            size_y: props.f32_at(prop::SIZE_Y, 0).unwrap_or(d.size_y),
            smoothness: props.f32_at(prop::SMOOTHNESS, 0).unwrap_or(d.smoothness),
            saturation: props.f32_at(prop::SATURATION, 0).unwrap_or(d.saturation),
            color: props.u32_at(prop::COLOR, 0).unwrap_or(d.color),
        };
        o.apply_presets();
        o
    }

    /// `SetSize(size)` (0x830360: preset 1..9 sets `sizeX = sizeY = size`, other values leave
    /// the stored sizes) and `SetStrength(strength)` (0x830230, [`strength_preset`]).
    pub fn apply_presets(&mut self) {
        if (1..=9).contains(&self.size) {
            self.size_x = self.size as f32;
            self.size_y = self.size as f32;
        }
        if let Some((smoothness, saturation)) = strength_preset(self.strength) {
            self.smoothness = smoothness;
            self.saturation = saturation;
        }
    }

    /// The outline struct stored under `id` of a property list, when there is one.
    pub fn find(props: &[Prop], id: u32) -> Option<Outline> {
        props.structs(id).first().map(|s| Outline::from_props(s))
    }

    /// Whether the kernel has any copy to draw (the copy at a zero shift is never drawn).
    pub fn is_visible(&self) -> bool {
        !self.taps().is_empty()
    }

    /// The copies of the kernel, in the retail loop order (x outer, y inner, both from
    /// `-size` in steps of 1.0 while `<= size`; `UTFWin` 0x8318F0 / 0x97D1E0 / 0x987C70 /
    /// 0x96E690). ADAPTED: the graphics-quality level of 0x96E2F0 (`SetOutlineLevel` 0x96E450,
    /// clamped to 0..3, both slots 3 in the code image) is taken as 3, the highest: the format is
    /// used unchanged (levels 0 / 1 / 2 draw nothing / a thinner / a halved kernel).
    pub fn taps(&self) -> Vec<Tap> {
        let (sx, sy) = (self.size_x, self.size_y);
        let mut out = Vec::new();
        if disabled() {
            return out;
        }
        // `comiss sizeX, -sizeX` fails for a negative size: nothing is drawn.
        if sx < 0.0 || sy < 0.0 || !sx.is_finite() || !sy.is_finite() || sx > 64.0 || sy > 64.0 {
            return out;
        }
        let radius = (sx * sx + sy * sy).sqrt();
        let falloff_scale = self.saturation / (radius + 1.0);
        let denominator = sx * sx + sy * sy + 1.0;
        let mut x = -sx;
        while x <= sx {
            let mut y = -sy;
            while y <= sy {
                let shift_x = (x + self.offset[0]).floor();
                let shift_y = (y + self.offset[1]).floor();
                if shift_x != 0.0 || shift_y != 0.0 {
                    let d2 = x * x + y * y;
                    let weight = (falloff_scale * (1.0 - d2 / denominator) + self.smoothness).clamp(0.0, 1.0);
                    // x87 `fistp` rounds to nearest even.
                    let alpha = (weight * 255.0).round_ties_even() as u8;
                    // A copy with a zero tint alpha changes nothing: it is not made.
                    if alpha != 0 {
                        out.push(Tap { dx: shift_x as i32, dy: shift_y as i32, alpha });
                    }
                }
                y += 1.0;
            }
            x += 1.0;
        }
        out
    }

    /// The tint of a copy: `(alpha << 24) + (color & 0xFFFFFF)`; the colour's own alpha is not
    /// used by the retail kernel.
    pub fn tap_color(&self, tap: &Tap) -> u32 {
        ((tap.alpha as u32) << 24) | (self.color & 0x00FF_FFFF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spui::format::{parse_raw, Value};

    fn preset(size: u32, strength: u32) -> Outline {
        let mut o = Outline { size, strength, ..Default::default() };
        o.apply_presets();
        o
    }

    #[test]
    fn strength_presets_are_the_table_of_set_strength() {
        // 0x830230: 1 -> (0.0004, 0.25) ... 9 -> (0, 3.15); 0 and above 9 leave the values.
        assert_eq!(strength_preset(1), Some((0.0004, 0.25)));
        assert_eq!(strength_preset(5), Some((0.0, 1.25)));
        assert_eq!(strength_preset(9), Some((0.0, 3.15)));
        assert_eq!(strength_preset(0), None);
        assert_eq!(strength_preset(10), None);
        let mut o = Outline { size: 0, strength: 0, size_x: 2.5, smoothness: 0.01, saturation: 0.7, ..Default::default() };
        o.apply_presets();
        assert_eq!((o.size_x, o.smoothness, o.saturation), (2.5, 0.01, 0.7), "user defined values stay");
    }

    #[test]
    fn a_constructed_outline_draws_nothing_and_a_size_preset_sets_both_sizes() {
        assert!(Outline::default().taps().is_empty());
        let o = preset(3, 2);
        assert_eq!((o.size_x, o.size_y), (3.0, 3.0));
        assert_eq!(o.taps().len(), 7 * 7 - 1);
        assert_eq!(preset(1, 2).taps().len(), 8);
        // Copies whose alpha rounds to 0 are not made: the four far corners of a size 9, strength 1
        // kernel (weight 0.0005) drop out.
        assert_eq!(preset(9, 1).taps().len(), 19 * 19 - 1 - 4);
    }

    #[test]
    fn tap_alpha_follows_the_retail_falloff() {
        // size 1, strength 2: r = sqrt 2; saturation / (r + 1) = 0.2071; the denominator is
        // sizeX^2 + sizeY^2 + 1 = 3. Edge copies (d2 = 1): 0.2071 * 2/3 + 0.0003 -> 35;
        // corner copies (d2 = 2): 0.2071 / 3 + 0.0003 -> 18.
        let taps = preset(1, 2).taps();
        let at = |dx: i32, dy: i32| taps.iter().find(|t| t.dx == dx && t.dy == dy).unwrap().alpha;
        assert_eq!(at(1, 0), 35);
        assert_eq!(at(0, -1), 35);
        assert_eq!(at(1, 1), 18);
        assert!(taps.iter().all(|t| (t.dx, t.dy) != (0, 0)));
        // x is the outer loop, both ascending from -size.
        assert_eq!((taps[0].dx, taps[0].dy), (-1, -1));
        assert_eq!((taps[1].dx, taps[1].dy), (-1, 0));
        assert_eq!((taps[3].dx, taps[3].dy), (0, -1));
        // Strength 9 (saturation 3.15): 3.15 / 2.4142 = 1.3048; edge 1.3048 * 2/3 -> 222,
        // corner 1.3048 / 3 -> 111.
        let strong = preset(1, 9).taps();
        assert_eq!(strong.iter().find(|t| (t.dx, t.dy) == (1, 0)).unwrap().alpha, 222);
        assert_eq!(strong.iter().find(|t| (t.dx, t.dy) == (1, 1)).unwrap().alpha, 111);
        // A weight above 1 clamps at 255: user defined sizes 0.5 x 0 with strength 9 give
        // 3.15 / 1.5 * (1 - 0.25 / 1.25) = 1.68 -> the copy at x = -0.5 (shift -1) is opaque
        // (the copy at x = 0.5 has a zero shift).
        let mut thin = Outline { strength: 9, size_x: 0.5, size_y: 0.0, ..Default::default() };
        thin.apply_presets();
        assert_eq!(thin.taps(), vec![Tap { dx: -1, dy: 0, alpha: 255 }]);
    }

    #[test]
    fn offsets_shift_the_copies_and_the_colour_alpha_is_ignored() {
        let mut o = preset(1, 2);
        o.offset = [1.0, 0.0];
        o.color = 0x7F11_2233;
        let taps = o.taps();
        // floor(x + 1): the copy of x = -1 sits at 0 (kept, y != 0), the centre copy of x = 0
        // moves to dx = 1; the copy with a zero shift (x = -1, y = 0) is left out.
        assert_eq!(taps.len(), 8);
        assert!(taps.iter().any(|t| (t.dx, t.dy) == (0, 1)));
        assert!(taps.iter().all(|t| (t.dx, t.dy) != (0, 0)));
        assert_eq!(o.tap_color(&Tap { dx: 1, dy: 0, alpha: 0x40 }), 0x4011_2233);
        let mut neg = preset(2, 2);
        neg.size_x = -1.0;
        assert!(neg.taps().is_empty());
    }


    use crate::spui::schema;
    use std::collections::BTreeMap;

    fn describe(props: &[Prop]) -> String {
        let mut v: Vec<String> = props
            .iter()
            .map(|p| match &p.value {
                Value::U32(x) => format!("{}={}", p.id, x[0]),
                Value::F32(x) => format!("{}={}", p.id, x[0]),
                other => format!("{}:{:?}", p.id, other.len()),
            })
            .collect();
        v.sort();
        v.join(",")
    }

    /// The typed model reads the installed layouts' strokes, halos and outlines (the census
    /// below shows the raw numbers): the icon strokes of the HUD buttons, the dark / light
    /// outlines of captions and the image outlines all come through with a visible kernel.
    /// `claude-cargo.ps1 test --offline --bin openspore spui::outline::tests::installed -- --ignored --nocapture`
    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn installed_layouts_have_visible_strokes_halos_and_outlines() {
        use crate::spui::model::{Drawable, Extra, Layout};
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let (mut strokes, mut halos, mut captions, mut images, mut biggest) = (0usize, 0usize, 0usize, 0usize, 0usize);
        for key in index.records.keys().filter(|k| k.type_id == crate::spui::format::SPUI_TYPE) {
            let Ok(bytes) = index.read(*key) else { continue };
            let Ok(layout) = Layout::parse(&bytes) else { continue };
            for w in &layout.windows {
                match &w.drawable {
                    Some(Drawable::SporeStd { infos, .. }) => {
                        for info in infos.iter().flatten() {
                            if info.stroke_mode != SHADOW_NONE && info.stroke.is_visible() {
                                strokes += 1;
                            }
                            if info.halo_mode != SHADOW_NONE && info.halo.is_visible() {
                                halos += 1;
                            }
                            biggest = biggest.max(info.stroke.taps().len()).max(info.halo.taps().len());
                        }
                    }
                    Some(Drawable::Std(d)) if d.outline.is_some_and(|o| o.is_visible()) => images += 1,
                    Some(Drawable::Image(d)) if d.outline.is_some_and(|o| o.is_visible()) => images += 1,
                    _ => {}
                }
                let o = match &w.extra {
                    Extra::Button(b) => b.outline,
                    Extra::Text(t) => t.outline,
                    _ => None,
                };
                if o.is_some_and(|o| o.is_visible()) {
                    captions += 1;
                    biggest = biggest.max(o.unwrap().taps().len());
                }
            }
        }
        println!("visible: strokes {strokes} halos {halos} caption outlines {captions} image outlines {images}; biggest kernel {biggest} copies");
        assert!(strokes > 500 && halos > 20 && captions > 500 && images > 20, "{strokes} {halos} {captions} {images}");
    }

    /// Census of the stroke / halo / outline properties of every installed layout.
    /// `claude-cargo.ps1 test --offline --bin openspore spui::outline::tests::census -- --ignored --nocapture`
    #[test]
    #[ignore = "requires the installed Spore resource packages"]
    fn census() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut keys: Vec<_> = index
            .records
            .keys()
            .copied()
            .filter(|k| k.type_id == crate::spui::format::SPUI_TYPE)
            .collect();
        keys.sort_by_key(|k| (k.group_id, k.instance_id));
        // (class, description) -> (count, layouts)
        let mut formats: BTreeMap<(String, String), (usize, Vec<String>)> = BTreeMap::new();
        let mut infos: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
        let mut per_layout: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for key in &keys {
            let Ok(bytes) = index.read(*key) else { continue };
            let Ok(raw) = parse_raw(&bytes) else { continue };
            let name = format!("{:08X}!{:08X}", key.group_id, key.instance_id);
            for e in &raw.elements {
                let class = schema::class_by_proxy(e.class).map(|c| c.name).unwrap_or("?");
                if e.class == 0x0540_037E {
                    let sm = e.u32_at(8, 0);
                    let hm = e.u32_at(9, 0);
                    let ss = Outline::find(&e.props, 0xA);
                    let hs = Outline::find(&e.props, 0xB);
                    if sm.is_some() || hm.is_some() || ss.is_some() || hs.is_some() {
                        let d = format!(
                            "stroke_mode={:?} halo_mode={:?} stroke={} halo={}",
                            sm,
                            hm,
                            e.structs(0xA).first().map(|s| describe(s)).unwrap_or_default(),
                            e.structs(0xB).first().map(|s| describe(s)).unwrap_or_default()
                        );
                        let ent = infos.entry(d).or_default();
                        ent.0 += 1;
                        if !ent.1.contains(&name) && ent.1.len() < 4 {
                            ent.1.push(name.clone());
                        }
                        let c = per_layout.entry(name.clone()).or_default();
                        c[0] += 1;
                    }
                    continue;
                }
                if let Some(s) = e.structs(PROP_OUTLINE).first() {
                    let ent = formats.entry((class.to_string(), describe(s))).or_default();
                    ent.0 += 1;
                    if !ent.1.contains(&name) && ent.1.len() < 4 {
                        ent.1.push(name.clone());
                    }
                    let c = per_layout.entry(name.clone()).or_default();
                    c[1] += 1;
                }
            }
        }
        println!("== image infos with stroke / halo properties: {} kinds", infos.len());
        for (d, (n, l)) in &infos {
            println!("INFO x{n}: {d}   e.g. {l:?}");
        }
        println!("== outline formats on windows / drawables: {} kinds", formats.len());
        for ((c, d), (n, l)) in &formats {
            println!("OUTLINE x{n} [{c}]: {d}   e.g. {l:?}");
        }
        println!("== layouts: {}", per_layout.len());
        for (l, c) in &per_layout {
            println!("LAYOUT {l} info={} outline={}", c[0], c[1]);
        }
    }
}
