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
        let names = index(&[path.clone()], &[]);
        let evidence = &names[&0xC897E947];
        assert_eq!(evidence.name, "thermal_hammer");
        assert_eq!(evidence.source, path.path);
        assert!(!names.contains_key(&0xC897E946));
        assert_ne!(fnv1_name_hash("thermal_hammer_disintegrate"), evidence.hash);
    }

    #[test]
    fn nested_source_vocabulary_requires_contiguous_native_tokens() {
        let paths = [tft::ContentPath {
            source: 1,
            offset: 24,
            path: "content/ambient_life/nessus_frog/effects/fx_frog_spawn.pattern.tft".into(),
        }];
        let names = index(&paths, &[]);
        for name in ["nessus_frog", "frog_spawn", "frog"] {
            assert_eq!(names[&fnv1_name_hash(name)].name, name);
            assert_eq!(names[&fnv1_name_hash(name)].source, paths[0].path);
        }
        assert!(!names.contains_key(&fnv1_name_hash("nessus_spawn")));
    }

    #[test]
    fn enum_tables_and_wwise_paths_are_engine_vocabulary() {
        let vocabulary = [
            tft::ContentPath {
                source: 2,
                offset: 0,
                path: "thermal_maul_super".into(),
            },
            tft::ContentPath {
                source: 3,
                offset: 40,
                path: "content\\audio\\wwise_events\\abilities\\ward_of_dawn_npc\\npc_ward_of_dawn_in.wwise_event".into(),
            },
        ];
        let names = index(&[], &vocabulary);
        assert_eq!(names[&fnv1_name_hash("thermal_maul")].name, "thermal_maul");
        assert_eq!(
            names[&fnv1_name_hash("thermal_maul")].source,
            "thermal_maul_super"
        );
        assert_eq!(names[&fnv1_name_hash("ward_of_dawn")].name, "ward_of_dawn");
        assert_eq!(
            names[&fnv1_name_hash("ward_of_dawn")].source,
            vocabulary[1].path
        );
    }

    #[test]
    fn legacy_names_hash_exactly_and_yield_to_installed_strings() {
        for legacy in legacy_names() {
            assert_eq!(fnv1_name_hash(&legacy.name), legacy.hash, "{}", legacy.name);
            assert!(legacy.source.starts_with("Destiny 1 "), "{}", legacy.source);
        }
        let names = index(&[], &[]);
        assert_eq!(
            names[&fnv1_name_hash("pulse_grenade")].name,
            "pulse_grenade"
        );
        assert!(
            names[&fnv1_name_hash("pulse_grenade")]
                .source
                .starts_with("Destiny 1 ")
        );
        // An installed string for the same hash is the Destiny 2 name, and it wins.
        let installed = [tft::ContentPath {
            source: 4,
            offset: 0,
            path: "content\\audio\\wwise_events\\pulse_grenade_loop.wwise_event".into(),
        }];
        let names = index(&[], &installed);
        assert_eq!(
            names[&fnv1_name_hash("pulse_grenade")].source,
            installed[0].path
        );
    }
}
