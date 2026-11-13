//! Space names (cSpaceNames::GenerateRandomName 0x5ED000 -> 0x5EC3F0).
//!
//! Original code: a letter-trigram generator. For every pair of preceding
//! letters the model keeps the letters that followed it in the lexicon words
//! (built by 0x5EC1B0: words start after `^^` and end with `$`); a name is a
//! walk of that chain, accepted when its length lies in the model's
//! `min..=max` and it is neither an excluded word nor holds an excluded
//! substring. The lexicons are the `Lexicon*` resources of
//! `NameGeneration.package` (`SpaceNames` PROP 0x0461227D!0x0FD9C1E3).

use std::collections::{BTreeMap, HashSet};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Which generator a name comes from (`SpaceNamesType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    Planet,
    Star,
    BlackHole,
    Tribe,
}

/// A letter-trigram model trained on lexicon words.
#[derive(Clone, Debug, Default)]
pub struct NameModel {
    chain: BTreeMap<(char, char), Vec<(char, u32)>>,
    words: Vec<String>,
    min_len: usize,
    max_len: usize,
}

impl NameModel {
    /// Train on lexicon words. Lengths are clamped to `minNameLength` 5 /
    /// `maxNameLength` 10 of the `SpaceNames` PROP (0x5EC840: the model keeps
    /// the longer of the shortest word and 5 and the shorter of the longest
    /// word and 10).
    pub fn train(words: Vec<String>, min_prop: usize, max_prop: usize) -> Self {
        let mut chain: BTreeMap<(char, char), BTreeMap<char, u32>> = BTreeMap::new();
        let (mut shortest, mut longest) = (100usize, 0usize);
        for word in &words {
            let len = word.encode_utf16().count();
            if len == 0 {
                continue;
            }
            shortest = shortest.min(len);
            longest = longest.max(len);
            let (mut a, mut b) = ('^', '^');
            for c in word.chars() {
                *chain.entry((a, b)).or_default().entry(c).or_insert(0) += 1;
                a = b;
                b = c;
            }
            *chain.entry((a, b)).or_default().entry('$').or_insert(0) += 1;
        }
        let chain = chain
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        Self {
            chain,
            words,
            min_len: shortest.max(min_prop),
            max_len: longest.min(max_prop),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }

    pub fn len_range(&self) -> (usize, usize) {
        (self.min_len, self.max_len)
    }

    /// 0x5E8EF0: a weighted pick of the next letter.
    fn pick(&self, state: (char, char), rng: &mut impl Rng) -> Option<char> {
        let choices = self.chain.get(&state)?;
        let total: f32 = choices.iter().map(|(_, n)| *n as f32).sum();
        let mut roll = rng.random::<f32>() * total;
        for (c, n) in choices {
            if roll < *n as f32 {
                return Some(*c);
            }
            roll -= *n as f32;
        }
        choices.last().map(|(c, _)| *c)
    }
}

/// All name generators of the game plus the exclusion lists.
#[derive(Clone, Debug, Default)]
pub struct SpaceNames {
    planet: NameModel,
    star: NameModel,
    black_hole: NameModel,
    tribe: NameModel,
    excluded_words: HashSet<String>,
    excluded_substrings: Vec<String>,
}

fn lexicon_words(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes));
    text.split(['\r', '\n'])
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Group id and type id of the lexicon records of `NameGeneration.package`.
pub const NAMEGEN_GROUP: u32 = 0x0461_227D;
pub const LEXICON_TYPE: u32 = 0x024A_0E52;
/// The lists of the `SpaceNames` PROP: star names use the stars, creatures and
/// flowers lexicons; planets the planets, people and creatures lexicons.
const STAR_LEXICONS: [&str; 3] = ["LexiconStars", "LexiconCreatures", "LexiconFlowers"];
const PLANET_LEXICONS: [&str; 3] = ["LexiconPlanets", "LexiconPeople", "LexiconCreatures"];
const BLACK_HOLE_LEXICONS: [&str; 1] = ["LexiconBlackHoles"];
const TRIBE_LEXICONS: [&str; 1] = ["LexiconTribes"];

impl SpaceNames {
    /// Build from the installed `NameGeneration.package`; `read` looks up one
    /// resource by (group, instance, type).
    pub fn from_install(read: impl Fn(u32, u32, u32) -> Option<Vec<u8>>) -> Option<Self> {
        let lexicon = |name: &str| -> Vec<String> {
            let id = crate::world_objects::name_id(name);
            read(NAMEGEN_GROUP, id, LEXICON_TYPE)
                .map(|b| lexicon_words(&b))
                .unwrap_or_default()
        };
        let gather =
            |names: &[&str]| -> Vec<String> { names.iter().flat_map(|n| lexicon(n)).collect() };
        let planet = NameModel::train(gather(&PLANET_LEXICONS), 5, 10);
        if planet.is_empty() {
            return None;
        }
        Some(Self {
            planet,
            star: NameModel::train(gather(&STAR_LEXICONS), 5, 10),
            black_hole: NameModel::train(gather(&BLACK_HOLE_LEXICONS), 5, 10),
            tribe: NameModel::train(gather(&TRIBE_LEXICONS), 5, 10),
            excluded_words: lexicon("LexiconExclude")
                .into_iter()
                .map(|w| w.to_lowercase())
                .collect(),
            excluded_substrings: lexicon("LexiconFoul")
                .into_iter()
                .map(|w| w.to_lowercase())
                .collect(),
        })
    }

    /// ADAPTED: a short built-in lexicon for headless runs and tests without
    /// the installed game (the real lexicons hold thousands of words).
    pub fn builtin() -> Self {
        const WORDS: &[&str] = &[
            "Abattok", "Abderus", "Acacallis", "Acaldon", "Acamas", "Acarnan", "Acastor",
            "Acestes", "Achelois", "Acheron", "Achilles", "Acoetes", "Acrisius", "Actaeon",
            "Actaeus", "Adeanar", "Adephagia", "Adiarax", "Adigelna", "Admetor", "Adonis",
            "Adrasteia", "Adraston", "Aegeus", "Aeneas", "Aerope", "Aesacus", "Aeson", "Aether",
            "Aethra", "Agamedes", "Agamemnon", "Aganax", "Agapenor", "Agaranis", "Agave",
            "Agdistis", "Agenor", "Agorbel", "Agrian", "Belisama", "Borealis", "Calliope",
            "Cassandra", "Cerberus", "Daedalus", "Elektra", "Fortuna", "Galatea", "Hyperion",
            "Icarus", "Juventas", "Kassiopeia", "Lysander", "Morpheus", "Nereus", "Orpheus",
            "Persephone", "Quirinus", "Rhadamanthus", "Selene", "Tithonus", "Urania", "Vesper",
            "Xanthippe", "Zephyrus", "Andromeda", "Bellatrix", "Castor", "Deneb", "Electra",
            "Fomalhaut", "Gienah", "Hamal", "Izar", "Jabbah", "Kochab", "Lesath", "Merak",
            "Nashira", "Okab", "Procyon", "Rasalhague", "Sabik", "Tarazed", "Unukalhai",
            "Vindemiatrix", "Wezen", "Yildun", "Zaniah", "Alderamin", "Biham",
        ];
        let words: Vec<String> = WORDS.iter().map(|w| w.to_string()).collect();
        let model = NameModel::train(words, 5, 10);
        Self {
            planet: model.clone(),
            star: model.clone(),
            black_hole: model.clone(),
            tribe: model,
            excluded_words: HashSet::new(),
            excluded_substrings: Vec::new(),
        }
    }

    fn model(&self, kind: NameKind) -> &NameModel {
        match kind {
            NameKind::Planet => &self.planet,
            NameKind::Star => &self.star,
            NameKind::BlackHole if !self.black_hole.is_empty() => &self.black_hole,
            NameKind::BlackHole => &self.star,
            NameKind::Tribe if !self.tribe.is_empty() => &self.tribe,
            NameKind::Tribe => &self.planet,
        }
    }

    /// One random name (0x5EC3F0): walk the trigram chain until `$`; accept a
    /// name of `min..=max` letters that is neither an excluded word nor holds
    /// an excluded substring; after 5000 failures take a lexicon word.
    pub fn generate(&self, kind: NameKind, rng: &mut impl Rng) -> String {
        let model = self.model(kind);
        for _ in 0..5000 {
            let mut name = String::new();
            let mut count = 0usize;
            let mut state = ('^', '^');
            while let Some(next) = model.pick(state, rng) {
                if next == '$' {
                    break;
                }
                name.push(next);
                count += 1;
                state = (state.1, next);
                if count > model.max_len {
                    break;
                }
            }
            if count < model.min_len || count > model.max_len {
                continue;
            }
            let lower = name.to_lowercase();
            if self.excluded_words.contains(&lower)
                || self
                    .excluded_substrings
                    .iter()
                    .any(|s| !s.is_empty() && lower.contains(s.as_str()))
            {
                continue;
            }
            return name;
        }
        if model.words.is_empty() {
            "Unnamed".to_string()
        } else {
            model.words[rng.random_range(0..model.words.len())].clone()
        }
    }

    /// A name that depends only on `seed`.
    pub fn name_for(&self, kind: NameKind, seed: u64) -> String {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        self.generate(kind, &mut rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_lexicon_style_within_length_limits() {
        let names = SpaceNames::builtin();
        let (min, max) = names.model(NameKind::Planet).len_range();
        assert_eq!((min, max), (5, 10));
        for seed in 0..200 {
            let name = names.name_for(NameKind::Planet, seed);
            let letters = name.chars().count();
            assert!((min..=max).contains(&letters), "{name}");
            assert!(name.chars().next().unwrap().is_uppercase(), "{name}");
        }
        // Deterministic per seed, varied across seeds.
        assert_eq!(
            names.name_for(NameKind::Star, 7),
            names.name_for(NameKind::Star, 7)
        );
        let distinct: HashSet<String> = (0..200)
            .map(|s| names.name_for(NameKind::Star, s))
            .collect();
        assert!(distinct.len() > 100, "{} distinct names", distinct.len());
    }

    #[test]
    fn excluded_words_and_substrings_are_rejected() {
        let mut names = SpaceNames::builtin();
        names.excluded_substrings = vec!["an".into()];
        for seed in 0..100 {
            let name = names.name_for(NameKind::Planet, seed).to_lowercase();
            assert!(!name.contains("an"), "{name}");
        }
    }
}
