//! Full-key retrieval of original animation resources. Cataloging is index-only;
//! clips are read and decoded on demand, preserving package override precedence.
use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey, decode::TYPE_ANIM};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub const TYPE_TLSA: u32 = 0x4aeb6bc6;

#[derive(Debug, Clone, Default)]
pub struct ActionLists {
    pub lists: Vec<(ResourceKey, crate::tlsa::Tlsa)>,
    pub errors: Vec<String>,
}
impl ActionLists {
    pub fn from_index(index: &ResourceIndex, catalog: &AnimationCatalog) -> Self {
        let mut out = Self::default();
        for &key in &catalog.action_lists {
            match index.read(key).and_then(|b| crate::tlsa::parse(&b)) {
                Ok(list) => out.lists.push((key, list)),
                Err(e) => out
                    .errors
                    .push(format!("{:08X}!{:08X}: {e}", key.group_id, key.instance_id)),
            }
        }
        out
    }

    pub fn group(&self, id: u32, list: Option<ResourceKey>) -> Result<&crate::tlsa::Group, String> {
        let mut matches = self
            .lists
            .iter()
            .filter(|(key, _)| list.is_none_or(|requested| requested == *key))
            .flat_map(|(_, list)| &list.groups)
            .filter(|group| group.id == id);
        let group = matches
            .next()
            .ok_or_else(|| format!("original action {id:08X} is missing"))?;
        if matches.next().is_some() {
            return Err(format!(
                "original action {id:08X} is ambiguous; specify its action list"
            ));
        }
        Ok(group)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnimationCatalog {
    pub clips: Vec<ResourceKey>,
    pub action_lists: Vec<ResourceKey>,
    by_instance: HashMap<u32, Vec<ResourceKey>>,
}

impl AnimationCatalog {
    pub fn from_index(index: &ResourceIndex) -> Self {
        let mut out = Self::default();
        for &key in index.records.keys() {
            match key.type_id {
                TYPE_ANIM => {
                    out.clips.push(key);
                    out.by_instance
                        .entry(key.instance_id)
                        .or_default()
                        .push(key);
                }
                TYPE_TLSA => out.action_lists.push(key),
                _ => {}
            }
        }
        let order = |key: &ResourceKey| (key.group_id, key.instance_id, key.type_id);
        out.clips.sort_by_key(order);
        out.action_lists.sort_by_key(order);
        for keys in out.by_instance.values_mut() {
            keys.sort_by_key(order);
        }
        out
    }

    /// TLSA stores instance IDs. Use the caller's authored group when available;
    /// an ambiguous ID never resolves to an arbitrary HashMap iteration result.
    pub fn resolve(&self, instance: u32, group: Option<u32>) -> Result<ResourceKey, String> {
        let keys = self
            .by_instance
            .get(&instance)
            .ok_or_else(|| format!("animation instance {instance:08X} is missing"))?;
        if let Some(group) = group {
            return keys
                .iter()
                .find(|k| k.group_id == group)
                .copied()
                .ok_or_else(|| format!("animation {group:08X}!{instance:08X} is missing"));
        }
        match keys.as_slice() {
            [key] => Ok(*key),
            _ => Err(format!(
                "animation instance {instance:08X} occurs in {} groups",
                keys.len()
            )),
        }
    }

    pub fn read(&self, index: &ResourceIndex, key: ResourceKey) -> Result<Vec<u8>, String> {
        if key.type_id != TYPE_ANIM
            || self
                .clips
                .binary_search_by_key(&(key.group_id, key.instance_id, key.type_id), |k| {
                    (k.group_id, k.instance_id, k.type_id)
                })
                .is_err()
        {
            return Err("resource is not in this animation catalog".into());
        }
        index.read(key)
    }
}

/// Decoded clip cache bounded by caller policy. A retained Arc remains valid
/// while playing even after eviction. Decode failures never poison the cache.
#[derive(Debug)]
pub struct ClipCache<T> {
    capacity: usize,
    entries: HashMap<ResourceKey, Arc<T>>,
    recency: VecDeque<ResourceKey>,
}

impl<T> ClipCache<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            recency: VecDeque::new(),
        }
    }

    pub fn load(
        &mut self,
        key: ResourceKey,
        loader: impl FnOnce() -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        if let Some(value) = self.entries.get(&key).cloned() {
            self.recency.retain(|k| *k != key);
            self.recency.push_back(key);
            return Ok(value);
        }
        let value = Arc::new(loader()?);
        if self.capacity != 0 {
            while self.entries.len() >= self.capacity {
                if let Some(oldest) = self.recency.pop_front() {
                    self.entries.remove(&oldest);
                } else {
                    break;
                }
            }
            self.entries.insert(key, value.clone());
            self.recency.push_back(key);
        }
        Ok(value)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl<T> Default for ClipCache<T> {
    fn default() -> Self {
        Self::new(16)
    }
}

pub fn load_clip(
    catalog: &AnimationCatalog,
    cache: &mut ClipCache<crate::animation::Animation>,
    index: &ResourceIndex,
    key: ResourceKey,
) -> Result<Arc<crate::animation::Animation>, String> {
    cache.load(key, || crate::animation::parse(&catalog.read(index, key)?))
}

/// Audit every effective original clip, without preview scan limits. Each
/// resource retains its identity and a decode result, including empty clips.
pub fn audit(index: &ResourceIndex) -> serde_json::Value {
    let catalog = AnimationCatalog::from_index(index);
    let mut rows = vec![];
    let mut versions = std::collections::BTreeMap::<u32, usize>::new();
    let mut decoded = 0;
    let mut channel_total = 0;
    let mut key_total = 0;
    let mut event_total = 0;
    for &key in &catalog.clips {
        let result = index.read(key).and_then(|bytes| {
            if let Some(version) = bytes.get(8..12) {
                *versions
                    .entry(u32::from_le_bytes(version.try_into().unwrap()))
                    .or_default() += 1;
            }
            crate::animation::parse(&bytes)
        });
        let package = index.records[&key]
            .0
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let row = match result {
            Ok(clip) => {
                decoded += 1;
                channel_total += clip.channels.len();
                event_total += clip.events.len();
                let keys = clip
                    .channels
                    .iter()
                    .flat_map(|c| &c.components)
                    .map(|c| match &c.keys {
                        crate::animation::Keys::Info(k) => k.len(),
                        crate::animation::Keys::Position(k) => k.len(),
                        crate::animation::Keys::Rotation(k) => k.len(),
                        crate::animation::Keys::Deform(k) => k.len(),
                        crate::animation::Keys::Unknown(k) => k.len(),
                    })
                    .sum::<usize>();
                key_total += keys;
                serde_json::json!({"key":crate::resource_audit::key_json(key),"package":package,"version":clip.version,"flags":format!("{:08X}",clip.flags),"cyclic_neighbors":clip.flags&8!=0,"path":clip.source_path,"length_frames":clip.length_frames,"frame_length_seconds":clip.frame_length,"duration_seconds":clip.duration_seconds(),"channels":clip.channels.len(),"component_keys":keys,"events":clip.events.len()})
            }
            Err(e) => {
                serde_json::json!({"key":crate::resource_audit::key_json(key),"package":package,"error":e})
            }
        };
        rows.push(row);
    }
    let actions = ActionLists::from_index(index, &catalog);
    let mut action_rows = vec![];
    for (key, list) in &actions.lists {
        let mut references = 0;
        let mut resolved = 0;
        let mut failures = vec![];
        for group in &list.groups {
            for animation in group.choices.iter().flat_map(|c| &c.animations) {
                references += 1;
                match catalog.resolve(animation.id,None) {
                    Ok(_)=>resolved+=1,
                    Err(e)=>failures.push(serde_json::json!({"action":format!("{:08X}",group.id),"animation":format!("{:08X}",animation.id),"description":animation.description,"error":e})),
                }
            }
        }
        action_rows.push(serde_json::json!({"key":crate::resource_audit::key_json(*key),"version":list.version,"groups":list.groups.len(),"animation_references":references,"resolved_references":resolved,"trailing_bytes":list.trailing.len(),"reference_errors":failures}));
    }
    serde_json::json!({"effective_clips":catalog.clips.len(),"decoded_clips":decoded,"channels":channel_total,"component_keys":key_total,"events":event_total,"versions":versions,"clips":rows,"action_lists":action_rows,"action_list_errors":actions.errors,"index_errors":index.errors})
}

pub fn run_audit() {
    let result = crate::spore_install::discover()
        .map(|install| audit(&ResourceIndex::load(&install.packages)));
    match result {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn all_installed_clips_and_action_references_are_retrievable() {
        let install = crate::spore_install::discover().expect("original install");
        let index = ResourceIndex::load(&install.packages);
        assert!(index.errors.is_empty(), "{:?}", index.errors);
        let catalog = AnimationCatalog::from_index(&index);
        assert!(!catalog.clips.is_empty());
        let mut cache = ClipCache::new(16);
        for &key in &catalog.clips {
            let clip = load_clip(&catalog, &mut cache, &index, key)
                .unwrap_or_else(|e| panic!("clip {key:?}: {e}"));
            assert!(clip.duration_seconds().is_finite());
            assert!(cache.len() <= 16);
        }
        let actions = ActionLists::from_index(&index, &catalog);
        assert!(actions.errors.is_empty(), "{:?}", actions.errors);
        assert!(!actions.lists.is_empty());
        for (_, list) in &actions.lists {
            for animation in list
                .groups
                .iter()
                .flat_map(|g| &g.choices)
                .flat_map(|c| &c.animations)
            {
                catalog
                    .resolve(animation.id, None)
                    .unwrap_or_else(|e| panic!("action animation {}: {e}", animation.description));
            }
        }
    }
    fn key(group: u32, instance: u32) -> ResourceKey {
        ResourceKey {
            type_id: TYPE_ANIM,
            group_id: group,
            instance_id: instance,
        }
    }
    fn index(keys: &[ResourceKey]) -> ResourceIndex {
        ResourceIndex {
            records: keys
                .iter()
                .map(|&key| {
                    (
                        key,
                        (
                            "unused".into(),
                            crate::dbpf::DbpfEntry {
                                key,
                                offset: 0,
                                disk_size: 0,
                                mem_size: 0,
                                is_compressed: false,
                            },
                        ),
                    )
                })
                .collect(),
            errors: vec![],
        }
    }
    #[test]
    fn full_catalog_and_group_ambiguity() {
        let a = key(1, 7);
        let b = key(2, 7);
        let c = key(1, 9);
        let action = ResourceKey {
            type_id: TYPE_TLSA,
            ..a
        };
        let catalog = AnimationCatalog::from_index(&index(&[c, b, a, action]));
        assert_eq!(catalog.clips, vec![a, c, b]);
        assert_eq!(catalog.action_lists, vec![action]);
        assert!(catalog.resolve(7, None).is_err());
        assert_eq!(catalog.resolve(7, Some(2)).unwrap(), b);
        assert_eq!(catalog.resolve(9, None).unwrap(), c);
        assert!(catalog.resolve(7, Some(3)).is_err());
        assert!(catalog.resolve(10, None).is_err());
    }
    #[test]
    fn eviction_keeps_playing_clip_alive_and_hit_refreshes_recency() {
        let mut cache = ClipCache::new(2);
        let first = cache.load(key(1, 1), || Ok(11)).unwrap();
        cache.load(key(1, 2), || Ok(22)).unwrap();
        assert_eq!(
            *cache
                .load(key(1, 1), || panic!("cache hit decoded again"))
                .unwrap(),
            11
        );
        cache.load(key(1, 3), || Ok(33)).unwrap();
        assert_eq!(*first, 11);
        assert_eq!(cache.len(), 2);
        assert_eq!(*cache.load(key(1, 2), || Ok(222)).unwrap(), 222);
        assert_eq!(*first, 11);
    }
    #[test]
    fn failed_decode_can_be_retried_and_zero_capacity_is_uncached() {
        let mut cache = ClipCache::new(1);
        assert!(cache
            .load(key(1, 1), || Err::<u32, _>("bad clip".into()))
            .is_err());
        assert_eq!(cache.len(), 0);
        assert_eq!(*cache.load(key(1, 1), || Ok(7)).unwrap(), 7);
        let mut uncached = ClipCache::new(0);
        assert_eq!(*uncached.load(key(1, 1), || Ok(1)).unwrap(), 1);
        assert_eq!(*uncached.load(key(1, 1), || Ok(2)).unwrap(), 2);
        assert_eq!(uncached.len(), 0);
    }
}
