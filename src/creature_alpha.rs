//! Alpha creatures' skin colour (notes C9A).
//!
//! VERIFIED (0xC03750, called from cCreatureAnimal::Create 0xC09B40, SetAge
//! 0xC04F30 and the growth step 0xC04FB0): an adult (mAge 1) NPC with the
//! alpha flag (mGeneralFlags bit 0) takes its species' skin colour number
//! `alphaCreatureSkinPaintColorIndex` (2: the third colour, species profile
//! +0x4E0 + 12 x index), converts it to HSV (0x67FEF0), adds
//! `alphaCreatureHSVColorMultipliers.x` x 360 to the hue (wrapped), multiplies
//! the saturation by `.y` and the value by `.z` (each clamped to 0..1,
//! 0x5A6DF0), converts back (0x67FDF0) and sets the result as the creature's
//! identity colour (SetIdentityColor 0xC03370 -> 0xC0DAD0: the animated
//! creature's colour +0x74, alpha 1) with "colour is identity" on (0xC0DAB0:
//! creature +0xB65, animated creature +0x84). Every other creature gets white
//! (1, 1, 1) and the flag off. With the installed tuning (0, 1.5, 1) the alpha
//! is its species' third colour, half again as saturated.
//!
//! Where the identity colour shows is decided by the creature's skin: the
//! skin-paint scripts paint `diffuseColor identity` splotches into a layer of
//! their own (`skinpaint_coat_blank` "puts a splotch into identity color"),
//! which the baked creature atlas carries in its alpha channel (10-20 % of
//! the texels of the installed creatures, the rest 0). VERIFIED (the shader
//! fragment `showIdentityPS`, Spore_Graphics 0x40212002 HLSL source):
//! `color.rgb = lerp(color.rgb, identityColor.rgb, color.a * identityColor.a)`.
//! `paint_alphas` gives alpha NPCs that look: their surfaces get a copy of the
//! atlas with the identity colour blended in by its alpha (texel bytes, the
//! game's gamma space). ADAPTED: the blend is baked into a texture copy per
//! species instead of a shader constant.

use bevy::prelude::*;
use std::collections::HashMap;

/// An NPC actor whose surfaces were given their identity colour (or that
/// needs none).
#[derive(Component)]
pub struct AlphaPainted;

/// Alpha surface materials and identity colours, per source material and
/// installed model.
#[derive(Resource, Default)]
pub struct AlphaPaintCache {
    materials: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    colors: HashMap<u32, Option<[f32; 3]>>,
}

/// The source material an alpha surface material was painted from, if any.
pub fn painted_from(world: &World, material: &Handle<StandardMaterial>) -> Option<AssetId<StandardMaterial>> {
    let cache = world.get_resource::<AlphaPaintCache>()?;
    cache.materials.iter().find(|(_, h)| h.id() == material.id()).map(|(source, _)| *source)
}

/// The skin colours of an installed creature model (its `.creaturedata`).
pub fn model_skin_colors(originals: &crate::Originals, instance: u32) -> Option<[[f32; 3]; 3]> {
    let key = crate::dbpf::ResourceKey {
        type_id: crate::creature_data::TYPE,
        group_id: crate::original_species::CREATURE_ANATOMY_GROUP,
        instance_id: instance,
    };
    let bytes = originals.resource_index.read(key).ok()?;
    crate::creature_data::parse(&bytes).ok()?.skin_colors().ok()
}

/// Blend `color` into RGBA8 texels by their alpha (`showIdentityPS`).
pub fn blend_identity(rgba: &mut [u8], color: [f32; 3]) {
    let target = color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round());
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        for k in 0..3 {
            let v = px[k] as f32;
            px[k] = (v + (target[k] - v) * a).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Give every alpha NPC of a retail herd its identity colour (0xC03750) on its
/// installed model's surfaces. Other creatures keep white and no identity.
#[allow(clippy::too_many_arguments)]
pub fn paint_alphas(
    mut commands: Commands,
    originals: Option<Res<crate::Originals>>,
    run: Option<Res<crate::game::RunProgress>>,
    actors: Query<
        (Entity, &crate::creature_ecology::HerdMember, Option<&Children>),
        (With<crate::baked_model::NativeCreature>, Without<AlphaPainted>),
    >,
    mut surfaces: Query<&mut MeshMaterial3d<StandardMaterial>, With<crate::material_binding::OriginalSurfaceChild>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<AlphaPaintCache>,
) {
    let (Some(originals), Some(run)) = (originals, run) else { return };
    for (entity, member, children) in &actors {
        if !member.role.is_alpha() {
            commands.entity(entity).insert(AlphaPainted);
            continue;
        }
        // The model's surfaces arrive with the spawn's commands: wait for them.
        let Some(children) = children else { continue };
        commands.entity(entity).insert(AlphaPainted);
        let Some(species) = run.species_world.species(member.species) else { continue };
        let Some(key) = crate::baked_model::species_key(&originals, species.visual()) else { continue };
        let color = *cache
            .colors
            .entry(key.instance_id)
            .or_insert_with(|| model_skin_colors(&originals, key.instance_id).map(alpha_identity_color));
        let Some(mut color) = color else { continue };
        if std::env::var_os("OPENSPORE_ALPHA_DEBUG").is_some() {
            // Dev switch: a loud identity colour shows where the layer is.
            color = [1.0, 0.0, 0.0];
        }
        info!(
            "alpha paint: creature {} ({}) model {:08X} identity {:?}",
            member.species, species.name, key.instance_id, color
        );
        for child in children.iter() {
            let Ok(mut surface) = surfaces.get_mut(child) else { continue };
            let source = surface.0.id();
            let handle = match cache.materials.get(&source) {
                Some(h) => h.clone(),
                None => {
                    let Some(mut material) = materials.get(source).cloned() else { continue };
                    if let Some(texture) = material.base_color_texture.as_ref().and_then(|t| images.get(t)).cloned() {
                        let mut texture = texture;
                        if let Some(data) = texture.data.as_mut() {
                            blend_identity(data, color);
                        }
                        material.base_color_texture = Some(images.add(texture));
                    }
                    let h = materials.add(material);
                    cache.materials.insert(source, h.clone());
                    h
                }
            };
            surface.0 = handle;
        }
    }
}

/// Dev hook for captures (`OPENSPORE_RULES4_TEST`), from frame 20 of the
/// stage until a creature qualifies (world state only, no saves):
/// `alpha` puts the avatar 7 units south of the nearest alpha NPC;
/// `attack` makes the nearest NPC with spit (else strike / charge) hostile and
/// puts the avatar 6 units from it, so it attacks with that ability;
/// `befriend` opens a social round with the nearest friendly NPC whose
/// request the avatar can answer with key 1, its side of the bar one answer
/// short of full (press 1 to befriend it and see the reward).
#[allow(clippy::type_complexity)]
pub fn rules4_test(
    mut frames: Local<u32>,
    mut done: Local<bool>,
    originals: Option<Res<crate::Originals>>,
    run: Option<Res<crate::game::RunProgress>>,
    mut encounter: Option<ResMut<crate::creature_actions::Encounter>>,
    mut player: Query<&mut Transform, With<crate::game::Player>>,
    mut npcs: Query<
        (
            &crate::persistence::WorldId,
            &Transform,
            &mut crate::game::Npc,
            &mut crate::creature_actions::CreatureMind,
            &crate::creature_ecology::HerdMember,
        ),
        Without<crate::game::Player>,
    >,
) {
    use crate::creature_actions::{levels, member_abilities, player_abilities, EncounterMode};
    static MODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let Some(mode) = MODE.get_or_init(|| std::env::var("OPENSPORE_RULES4_TEST").ok()) else {
        return;
    };
    *frames += 1;
    if *done || *frames < 20 || *frames > 2000 {
        return;
    }
    let (Some(originals), Some(run)) = (originals, run) else { return };
    let Ok(mut avatar) = player.single_mut() else { return };
    let at = avatar.translation;
    let mut best: Option<(f32, u32)> = None;
    for (id, t, npc, mind, member) in &npcs {
        if mind.defeated || npc.hp <= 0.0 {
            continue;
        }
        let combat = levels(member_abilities(&run, &originals, id.0), EncounterMode::Combat);
        let ok = match mode.as_str() {
            "alpha" => member.role.is_alpha(),
            "attack" => combat[3] > 0 || (combat[0] == 0 && (combat[2] > 0 || combat[1] > 0)),
            "befriend" => {
                let social = levels(member_abilities(&run, &originals, id.0), EncounterMode::Social);
                let mine = levels(player_abilities(&run, &originals), EncounterMode::Social);
                !npc.hostile && !mind.allied && social[0] > 0 && mine[0] > 0
            }
            _ => false,
        };
        let d = t.translation.xz().distance(at.xz());
        if ok && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, id.0));
        }
    }
    let Some((_, target)) = best else { return };
    *done = true;
    for (id, t, mut npc, mut mind, _) in &mut npcs {
        if id.0 != target {
            continue;
        }
        let gap = match mode.as_str() {
            "alpha" => 7.0,
            "attack" => 6.0,
            _ => 3.2,
        };
        avatar.translation = Vec3::new(t.translation.x, avatar.translation.y, t.translation.z + gap);
        avatar.rotation = Quat::IDENTITY;
        match mode.as_str() {
            "attack" => {
                npc.hostile = true;
                mind.allied = false;
                mind.provoked = true;
                mind.attack_timer = 0.0;
            }
            "befriend" => {
                let social = levels(member_abilities(&run, &originals, id.0), EncounterMode::Social);
                let mine = levels(player_abilities(&run, &originals), EncounterMode::Social);
                mind.rounds = 1;
                mind.requests = vec![0];
                crate::creature_actions::propose(&mut mind, social);
                mind.response_time = crate::creature_rules::SOCIAL_RESPONSE_TIMEOUT;
                mind.npc_fill = (mind.bar_max - crate::creature_rules::social_power(mine[0])).max(0.0);
                // Patient until the scripted key press (no drain in the capture).
                mind.drain_armed = true;
                mind.drain_timer = 1.0e6;
                if let Some(e) = encounter.as_mut() {
                    e.target = Some(id.0);
                    e.mode = EncounterMode::Social;
                    e.cooldowns = [0.0; 4];
                }
            }
            _ => {}
        }
        info!("rules4 test {mode}: creature {} at {:?}", id.0, t.translation);
    }
}

/// RGB (0..1) to HSV (hue in degrees), retail 0x67FEF0.
pub fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta == 0.0 {
        return [0.0, 0.0, max];
    }
    let mut h = if r == max {
        (g - b) / delta * 60.0
    } else if g == max {
        (b - r) / delta * 60.0 + 120.0
    } else {
        (r - g) / delta * 60.0 + 240.0
    };
    if h < 0.0 {
        h += 360.0;
    }
    [h, delta / max, max]
}

/// HSV to RGB, retail 0x67FDF0 (sectors of 120 degrees centred on the
/// primaries).
pub fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let x = (h + 60.0) - ((h + 60.0) / 360.0).floor() * 360.0;
    let mut i = (x / 120.0).floor() as i32;
    let f = (x - i as f32 * 120.0) / 60.0;
    if i >= 3 {
        i = 2;
    }
    let i = i.max(0) as usize;
    let (near, far) = if f <= 1.0 { (s, f * s) } else { ((2.0 - f) * s, s) };
    let mut out = [0.0; 3];
    out[i] = v;
    out[(i + 1) % 3] = (1.0 - near) * v;
    out[(i + 2) % 3] = (1.0 - far) * v;
    out
}

/// The identity colour of an alpha from its species' skin colours (0xC03750).
pub fn alpha_identity_color(skin_colors: [[f32; 3]; 3]) -> [f32; 3] {
    use crate::creature_ecology::{ALPHA_HSV_COLOR_MULTIPLIERS as M, ALPHA_SKIN_PAINT_COLOR_INDEX};
    let [h, s, v] = rgb_to_hsv(skin_colors[ALPHA_SKIN_PAINT_COLOR_INDEX.min(2)]);
    let mut h = M[0] * 360.0 + h;
    if h > 360.0 {
        h -= 360.0;
    }
    if h < 0.0 {
        h += 360.0;
    }
    hsv_to_rgb([h, (M[1] * s).clamp(0.0, 1.0), (M[2] * v).clamp(0.0, 1.0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn hsv_round_trips_the_primaries_and_greys() {
        for c in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0, 0.0], [0.2, 0.5, 0.8], [0.5, 0.5, 0.5]] {
            assert!(close(hsv_to_rgb(rgb_to_hsv(c)), c), "{c:?} -> {:?}", hsv_to_rgb(rgb_to_hsv(c)));
        }
        assert_eq!(rgb_to_hsv([1.0, 0.0, 0.0]), [0.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv([0.0, 0.0, 1.0])[0], 240.0);
    }

    #[test]
    fn an_alpha_wears_its_third_skin_colour_half_again_as_saturated() {
        // Starter palette: pink, pink, white -> white stays white (no saturation).
        let starter = [[0.576, 0.341, 0.431], [0.576, 0.341, 0.431], [1.0, 1.0, 1.0]];
        assert!(close(alpha_identity_color(starter), [1.0, 1.0, 1.0]));
        // A muted green third colour: same hue and value, saturation x1.5.
        let green = [0.4, 0.6, 0.4];
        let out = alpha_identity_color([[0.0; 3], [0.0; 3], green]);
        let (a, b) = (rgb_to_hsv(green), rgb_to_hsv(out));
        assert!((a[0] - b[0]).abs() < 1e-3 && (a[2] - b[2]).abs() < 1e-4);
        assert!((b[1] - (a[1] * 1.5)).abs() < 1e-4, "{a:?} {b:?}");
        // Saturation is clamped to 1.
        let vivid = alpha_identity_color([[0.0; 3], [0.0; 3], [0.9, 0.1, 0.1]]);
        assert!((rgb_to_hsv(vivid)[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn identity_blends_by_the_atlas_alpha() {
        let mut px = vec![100, 100, 100, 0, 100, 100, 100, 255, 0, 0, 0, 128];
        blend_identity(&mut px, [1.0, 0.0, 0.0]);
        assert_eq!(&px[0..4], &[100, 100, 100, 0], "no identity layer: unchanged");
        assert_eq!(&px[4..8], &[255, 0, 0, 255], "full layer: the identity colour");
        assert_eq!(&px[8..12], &[128, 0, 0, 128], "half layer: half way");
    }

    #[test]
    #[ignore = "requires installed original Spore assets; prints the creature atlas channels"]
    fn installed_creature_atlas_alpha_census() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let keys = crate::baked_model::candidates(&index, crate::baked_model::CREATURE_GROUP);
        for key in keys.iter().step_by((keys.len() / 10).max(1)).take(10) {
            let Ok(model) = crate::baked_model::parse(&index.read(*key).unwrap()) else { continue };
            for (i, m) in model.materials.iter().enumerate() {
                for b in &m.textures {
                    let Ok(mips) = crate::baked_model::texture_mips(&index, b.key) else { continue };
                    let Some(p) = mips.first() else { continue };
                    let mut hist = [0usize; 5];
                    for px in p.rgba.chunks_exact(4) {
                        hist[px[3] as usize * 5 / 256] += 1;
                    }
                    println!(
                        "{:08X} mat {i} sampler {} tex {:08X}!{:08X} {}x{} alpha hist {hist:?} shader data {:X?}",
                        key.instance_id, b.sampler, b.key.group_id, b.key.instance_id, p.width, p.height,
                        m.shader_data.keys().collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}
