//! Installed TrueType fonts; no font data is bundled with the rewrite.
use bevy::prelude::*;
#[derive(Resource, Default)]
pub struct OriginalFont {
    pub handle: Option<Handle<Font>>,
    pub source: Option<crate::dbpf::ResourceKey>,
}
pub fn load(
    originals: Res<crate::Originals>,
    mut fonts: ResMut<Assets<Font>>,
    mut selected: ResMut<OriginalFont>,
) {
    let mut keys = originals
        .resource_index
        .records
        .keys()
        .copied()
        .filter(|k| [0x276CA4B9, 0x027C5CEF].contains(&k.type_id))
        .collect::<Vec<_>>();
    keys.sort_by_key(|k| {
        (
            k.instance_id != crate::world_objects::name_id("Arial"),
            k.group_id,
            k.instance_id,
        )
    });
    for key in keys {
        let Ok(bytes) = originals.resource_index.read(key) else {
            continue;
        };
        if !bytes.starts_with(&[0, 1, 0, 0]) && !bytes.starts_with(b"OTTO") {
            continue;
        }
        selected.handle = Some(fonts.add(Font::from_bytes(bytes)));
        selected.source = Some(key);
        info!("original UI font {key:?}");
        return;
    }
}
pub fn apply(selected: Res<OriginalFont>, mut texts: Query<&mut TextFont, Added<TextFont>>) {
    for mut text in &mut texts {
        text.font = selected
            .handle
            .as_ref()
            .map(|font| bevy::text::FontSource::Handle(font.clone()))
            .unwrap_or_else(bevy::text::FontSource::sans_serif);
    }
}
