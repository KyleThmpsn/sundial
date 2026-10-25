//! Recover native entity name hashes from vocabulary present in the same installation.
//! A matching name identifies a source, never damage, targeting or impact behavior.
use super::*;
use crate::hash::fnv1_name_hash;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameEvidence {
    pub name: String,
    pub hash: u32,
    pub source: String,
    /// Recovered from Destiny 1 strings rather than the installed packages. A legacy name
    /// is often a template name shared by many graphs, so any installed ancestry outranks it.
    #[serde(default)]
    pub legacy: bool,
}

/// A name the engine hashed for a graph that the installed Destiny 2 strings no longer
/// carry, recovered from the Destiny 1 packages by the same exact hash. The source names
/// the game and the string it came from, so the evidence is never mistaken for a Destiny 2
/// path.
#[derive(Deserialize)]
struct LegacyName {
    hash: String,
    name: String,
    source: String,
}

fn legacy_names() -> Vec<NameEvidence> {
    let legacy: Vec<LegacyName> =
        serde_json::from_str(include_str!("legacy_names.json")).expect("bundled legacy names");
    legacy
        .into_iter()
        .map(|legacy| NameEvidence {
            hash: u32::from_str_radix(&legacy.hash, 16).expect("legacy name hash"),
            name: legacy.name,
            source: legacy.source,
            legacy: true,
        })
        .collect()
}

/// Every installed engine string is a candidate: `.tft` content paths, and the wwise event
/// paths and enum tables the tft scan records as vocabulary. Legacy names fill only hashes
/// that no installed string resolves.
pub(super) fn index(
    paths: &[tft::ContentPath],
    vocabulary: &[tft::ContentPath],
) -> BTreeMap<u32, NameEvidence> {
    let mut names = BTreeMap::<u32, BTreeMap<String, String>>::new();
    for path in paths.iter().chain(vocabulary) {
        // Entity names also survive in parent directories and inside longer source
        // names. Only contiguous native tokens are candidates. The full 32-bit hash
        // must match and collisions between distinct candidates remain unresolved.
        for component in path.path.split(['/', '\\']) {
            let stem = component.split('.').next().unwrap_or_default();
            if !stem
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                continue;
            }
            let mut starts = vec![0];
            starts.extend(stem.match_indices('_').map(|(index, _)| index + 1));
            let mut ends = stem
                .match_indices('_')
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            ends.push(stem.len());
            for start in starts {
                for &end in &ends {
                    if end < start + 4 {
                        continue;
                    }
                    let name = &stem[start..end];
                    names
                        .entry(fnv1_name_hash(name))
                        .or_default()
                        .entry(name.to_owned())
                        .or_insert_with(|| path.path.clone());
                }
            }
        }
    }
    let mut names = names
        .into_iter()
        .filter_map(|(hash, candidates)| {
            if candidates.len() != 1 {
                return None;
            }
            let (name, source) = candidates.into_iter().next()?;
            Some((
                hash,
                NameEvidence {
                    name,
                    hash,
                    source,
                    legacy: false,
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
    for legacy in legacy_names() {
        names.entry(legacy.hash).or_insert(legacy);
    }
    names
}
