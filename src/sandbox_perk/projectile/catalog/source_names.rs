//! Recover native entity name hashes from vocabulary present in the same installation.
//! A matching name identifies a source, never damage, targeting or impact behavior.
use super::*;
use crate::hash::fnv1_name_hash;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameEvidence {
    pub name: String,
    pub hash: u32,
    pub source: String,
}

pub(super) fn index(paths: &[tft::ContentPath]) -> BTreeMap<u32, NameEvidence> {
    let mut names = BTreeMap::<u32, BTreeMap<String, String>>::new();
    for path in paths {
        let stem = tft::asset_label(&path.path)
            .split('.')
            .next()
            .unwrap_or_default()
            .to_owned();
        if !stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            continue;
        }
        // Native entities often keep the shared stem of their animation/effect filenames.
        // Require the full 32-bit entity-name hash, rather than matching words in a payload.
        let mut ends = stem
            .match_indices('_')
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        ends.push(stem.len());
        for end in ends {
            let name = &stem[..end];
            if name.len() < 4 {
                continue;
            }
            names
                .entry(fnv1_name_hash(name))
                .or_default()
                .entry(name.to_owned())
                .or_insert_with(|| path.path.clone());
        }
    }
    names
        .into_iter()
        .filter_map(|(hash, candidates)| {
            if candidates.len() != 1 {
                return None;
            }
            let (name, source) = candidates.into_iter().next()?;
            Some((hash, NameEvidence { name, hash, source }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovered_names_require_exact_native_hashes_and_keep_their_source() {
        let path = tft::ContentPath {
            source: 1,
            offset: 24,
            path: "content/common/animation/thermal_hammer_disintegrate.death_forces.tft".into(),
        };
        let names = index(&[path.clone()]);
        let evidence = &names[&0xC897E947];
        assert_eq!(evidence.name, "thermal_hammer");
        assert_eq!(evidence.source, path.path);
        assert!(!names.contains_key(&0xC897E946));
        assert_ne!(fnv1_name_hash("thermal_hammer_disintegrate"), evidence.hash);
    }
}
