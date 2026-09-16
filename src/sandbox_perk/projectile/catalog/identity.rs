//! Derive a useful source identity from native paths and reference ancestry.
//! A source name never claims damage, targeting or impact behavior.
use super::*;
use std::collections::BTreeSet;

/// Familiar catalog names are presentation metadata, separate from test observations.
/// A native identity takes precedence. These names make otherwise anonymous saved
/// ingredients recognizable without implying a gameplay contract or an asset path.
fn familiar_name(graph: u32) -> Option<String> {
    static NAMES: std::sync::OnceLock<BTreeMap<u32, String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            serde_json::from_str(include_str!("display_names.json"))
                .expect("bundled effect display names")
        })
        .get(&graph)
        .cloned()
}

impl Entry {
    /// Identity attached to this asset itself, without borrowing an ancestor's name.
    pub fn direct_name(&self) -> Option<String> {
        let direct = self
            .native_paths
            .iter()
            .chain(self.native_name.iter())
            .filter_map(|path| source_name(path))
            .collect::<BTreeSet<_>>();
        common_name(&direct.iter().map(String::as_str).collect())
            .or_else(|| familiar_name(self.graph))
    }

    pub fn discovery_name(&self) -> Option<String> {
        self.discovery_name_with(|_| None, |_| None)
    }

    pub fn discovery_name_with(
        &self,
        perk_name: impl FnMut(u16) -> Option<String>,
        item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> Option<String> {
        self.source_identity_with(perk_name, item_name)
    }

    fn source_identity_with(
        &self,
        mut perk_name: impl FnMut(u16) -> Option<String>,
        mut item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> Option<String> {
        if let Some(name) = self.direct_name() {
            return Some(name);
        }
        // A Destiny 1 name on the graph itself must not hide the items and perks that
        // reach it; only an installed native path outranks those.
        let native_context = self.contexts.iter().any(|context| {
            source_name(&context.path).is_some()
                && !context
                    .name_evidence
                    .as_ref()
                    .is_some_and(|evidence| evidence.legacy)
        });
        let named = self
            .contexts
            .iter()
            .filter(|context| !native_context || source_name(&context.path).is_some())
            .filter_map(|context| {
                let name = source_name(&context.path)
                    .or_else(|| context.item.and_then(&mut item_name).map(|item| item.name))
                    .or_else(|| context.perk.and_then(&mut perk_name))?;
                meaningful_name(&name).then_some((context.depth, name))
            })
            .collect::<Vec<_>>();
        let Some(depth) = named.iter().map(|(depth, _)| *depth).min() else {
            // Direct perk references remain useful even when ancestry has no path.
            let names = self
                .perk_indices
                .iter()
                .filter_map(|&index| perk_name(index))
                .filter(|name| meaningful_name(name))
                .collect::<BTreeSet<_>>();
            return common_name(&names.iter().map(String::as_str).collect())
                .map(|name| format!("{name} {}", self.kind.label()));
        };
        let names = named
            .iter()
            .filter(|(d, _)| *d == depth)
            .map(|(_, name)| name.as_str())
            .collect::<BTreeSet<_>>();
        let common = common_name(&names)
            .or_else(|| {
                let items = self
                    .contexts
                    .iter()
                    .filter(|context| context.depth == depth)
                    .filter_map(|context| context.item.and_then(&mut item_name))
                    .collect::<Vec<_>>();
                let kinds = items
                    .iter()
                    .map(|item| item.kind.trim())
                    .filter(|kind| !kind.is_empty())
                    .collect::<BTreeSet<_>>();
                // Unnamed/retired item rows do not contradict the known type of their shared
                // pattern. Keep genuine disagreements, but do not let missing strings erase
                // a Machine Gun or Hand Cannon family already established by named sources.
                (kinds.len() == 1).then(|| format!("Shared {}", kinds.first().unwrap()))
            })
            .or_else(|| {
                // Named ancestors that agree on nothing are still the engine's own record
                // of who reaches this asset. Listing them states that record rather than
                // inventing a family, and leaves the entry identifiable in a picker.
                let owned = names.iter().map(|name| (*name).to_owned()).collect();
                Some(format!("Shared by {}", listed(&owned)))
            })?;
        Some(format!("{common} {}", self.kind.label()))
    }

    /// Use a stable identifier only where ancestry cannot distinguish native variants.
    pub fn discovery_label_with(
        &self,
        perk_name: impl FnMut(u16) -> Option<String>,
        item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> String {
        self.discovery_name_with(perk_name, item_name)
            .unwrap_or_else(|| format!("Unidentified {} · 0x{:08X}", self.kind.label(), self.graph))
    }

    /// Only decoded usage belongs in the behavior summary. A filename alone supplies identity.
    pub fn discovery_summary(&self) -> String {
        self.source_hint
            .as_deref()
            .filter(|role| meaningful_role(role))
            .unwrap_or_default()
            .to_owned()
    }

    /// A source role from a directly named pattern, distinct from its engine object type.
    pub fn pickup_role(&self) -> Option<&'static str> {
        let roles = self
            .native_paths
            .iter()
            .chain(self.native_name.iter())
            .filter_map(|path| {
                let stem = path
                    .rsplit(['/', '\\'])
                    .next()?
                    .split('.')
                    .next()?
                    .to_lowercase();
                if !(stem.contains("pickup")
                    || stem.contains("bauble")
                    || stem.contains("ammo_brick"))
                {
                    return None;
                }
                Some(if stem.contains("feedback") {
                    "Pickup Feedback"
                } else if stem.contains("spawner") || stem.contains("spawn_") {
                    "Pickup Spawner"
                } else if stem.contains("collected")
                    || stem.ends_with("_collect")
                    || stem.contains("counter")
                {
                    "Pickup Collection Effect"
                } else if stem.contains("hopon") || stem.contains("hop_on") {
                    "Pickup Attachment"
                } else {
                    "Pickup Source"
                })
            })
            .collect::<BTreeSet<_>>();
        (roles.len() == 1).then(|| *roles.first().unwrap())
    }
}

pub(super) fn meaningful_role(role: &str) -> bool {
    !role.trim().is_empty()
        && !matches!(
            role,
            "Shared Across Different Operations"
                | "Shared Perk Asset"
                | "Shared Asset, Role Unmapped"
        )
}

fn meaningful_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    !lower.is_empty()
        && ![
            "ability ",
            "effect ",
            "unknown plug",
            "unknown item",
            "unidentified ",
            "0x",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

impl Catalog {
    /// Disambiguate same-family assets consistently, independent of filtering or sort order.
    /// A variant number identifies an asset. It does not imply a behavioral difference.
    pub fn discovery_labels_with(
        &self,
        mut perk_name: impl FnMut(u16) -> Option<String>,
        mut item_name: impl FnMut(u32) -> Option<ItemName>,
    ) -> BTreeMap<u32, String> {
        let mut groups = BTreeMap::<String, Vec<u32>>::new();
        for entry in &self.entries {
            groups
                .entry(entry.discovery_label_with(&mut perk_name, &mut item_name))
                .or_default()
                .push(entry.graph);
        }
        let mut labels = BTreeMap::new();
        for (name, mut graphs) in groups {
            graphs.sort_unstable();
            let shared = graphs.len() > 1;
            for (index, graph) in graphs.into_iter().enumerate() {
                labels.insert(
                    graph,
                    if shared {
                        format!("{name} · Variant {}", index + 1)
                    } else {
                        name.clone()
                    },
                );
            }
        }
        labels
    }
}

pub(super) fn source_name(path: &str) -> Option<String> {
    let lower = path.to_lowercase().replace('\\', "/");
    if lower.is_empty() || shared_metadata_path(&lower) || lower.contains("debug") {
        return None;
    }
    let parts = lower.split('/').collect::<Vec<_>>();
    let file = parts.last()?;
    let stem = file.split('.').next()?;
    // A sequence path is the engine's own name for the resource that plays it. The kind
    // stays in the name so a sequence is never mistaken for the thing it animates.
    let suffix = if file.ends_with(".fx_sequence.tft") {
        Some("FX Sequence")
    } else if file.ends_with(".sequence.tft") {
        Some("Sequence")
    } else {
        None
    };
    let mut words = stem
        .split('_')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    // Content versions share a display family. Their graph identities remain distinct.
    while words.last().is_some_and(|word| {
        *word == "base"
            || *word == "hs"
            || word
                .strip_prefix('v')
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    }) {
        words.pop();
    }
    if words.is_empty() {
        return None;
    }
    if words.iter().all(|word| generic_source_word(word)) {
        // A generic filename is not an identity. The nearest specific directory
        // can establish its source, such as riposte_sword/spawner/spawner.pattern.tft.
        let parent = parts[..parts.len() - 1].iter().rev().find(|part| {
            !part.split('_').all(generic_source_word)
                && !matches!(
                    **part,
                    "content"
                        | "sandbox"
                        | "objects"
                        | "effects"
                        | "shared"
                        | "common"
                        | "weapons"
                        | "characters"
                )
                && !part.starts_with('_')
                && !part
                    .strip_prefix('v')
                    .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit()))
        })?;
        let mut specific = parent
            .split('_')
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>();
        specific.extend(words);
        words = specific;
    }
    let family = parts.iter().copied().find(|part| {
        matches!(
            *part,
            "taken" | "fallen" | "hive" | "cabal" | "vex" | "scorn"
        )
    });
    if let Some(family) = family
        && words.first() != Some(&family)
    {
        words.insert(0, family);
    }
    let mut name = words
        .iter()
        .enumerate()
        .filter(|(index, word)| !(**word == "bauble" && words.get(index + 1) == Some(&"pickup")))
        .map(|(_, word)| word)
        .map(|word| {
            match *word {
                "bauble" => return "Pickup".to_owned(),
                "hopon" => return "Attachment".to_owned(),
                "pve" => return "PvE".to_owned(),
                "pvp" => return "PvP".to_owned(),
                _ => {}
            }
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(suffix) = suffix {
        name.push(' ');
        name.push_str(suffix);
    }
    Some(name)
}

fn generic_source_word(word: &str) -> bool {
    matches!(
        word,
        "" | "entity" | "pattern" | "projectile" | "default" | "base" | "spawner"
    )
}

fn common_name(names: &BTreeSet<&str>) -> Option<String> {
    let first = names.first()?;
    if names.len() == 1 {
        return Some((*first).to_owned());
    }
    let mut common = first.split_whitespace().collect::<Vec<_>>();
    for name in names.iter().skip(1) {
        let count = common
            .iter()
            .copied()
            .zip(name.split_whitespace())
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(count);
    }
    if common.len() < 2 {
        // Variant prefixes can differ while naming the same family. Require one
        // unambiguous contiguous phrase shared by every source, not a bag of words.
        let words = first.split_whitespace().collect::<Vec<_>>();
        for length in (2..=words.len()).rev() {
            let candidates = words
                .windows(length)
                .filter(|candidate| {
                    names.iter().skip(1).all(|name| {
                        name.split_whitespace()
                            .collect::<Vec<_>>()
                            .windows(length)
                            .any(|window| window == *candidate)
                    })
                })
                .collect::<BTreeSet<_>>();
            if candidates.len() == 1 {
                common = candidates.first().unwrap().to_vec();
                break;
            }
            if candidates.len() > 1 {
                return None;
            }
        }
    }
    if common.is_empty()
        || (common.len() == 1
            && matches!(
                common[0],
                "Taken"
                    | "Fallen"
                    | "Hive"
                    | "Cabal"
                    | "Vex"
                    | "Scorn"
                    | "Weapon"
                    | "Effect"
                    | "Ability"
            ))
    {
        return None;
    }
    Some(common.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pickup_sources_keep_their_spawn_collection_and_attachment_roles() {
        for (path, name) in [
            (
                "collectable_bauble_spawner_dark.pattern.tft",
                "Collectable Pickup Spawner Dark",
            ),
            (
                "dark_bauble_pickup_feedback_hopon.pattern.tft",
                "Dark Pickup Feedback Attachment",
            ),
            ("health_orb_pickup.pattern.tft", "Health Orb Pickup"),
            (
                "bauble_under_player_hopon.pattern.tft",
                "Pickup Under Player Attachment",
            ),
        ] {
            assert_eq!(source_name(path).as_deref(), Some(name));
        }
    }
    #[test]
    fn enemy_identity_uses_native_family_and_ignores_content_versions() {
        let names = BTreeSet::from([
            source_name("content/activities/v400/sandbox_custom/characters/taken/_factions/base/taken_wizard/taken_wizard_v400.pattern.tft").unwrap(),
            source_name("content/sandbox/characters/taken/_factions/base/taken_wizard/taken_wizard.pattern.tft").unwrap(),
        ]);
        assert_eq!(
            common_name(&names.iter().map(String::as_str).collect()),
            Some("Taken Wizard".into())
        );
        assert_eq!(
            source_name("content/characters/hive/shrieker/shrieker_hs.pattern.tft"),
            Some("Hive Shrieker".into())
        );
        // A sequence path is the engine's name for the resource that plays it.
        assert_eq!(
            source_name(
                "content/sandbox/characters/fotc/vendors/sequences/vendor_greeting.sequence.tft"
            )
            .as_deref(),
            Some("Vendor Greeting Sequence")
        );
        assert_eq!(
            source_name(
                "content/fx/fx_library/destruction/damage_generic/damage_burst_med.fx_sequence.tft"
            )
            .as_deref(),
            Some("Damage Burst Med FX Sequence")
        );
        assert!(source_name("[debug]_fallen.sequence.tft").is_none());
        assert!(source_name("label_globals.label_globals.tft").is_none());
        assert!(common_name(&BTreeSet::from(["Fallen Shank", "Fallen Captain"])).is_none());
    }

    #[test]
    fn identity_uses_specific_parent_and_shared_family_without_placeholder_names() {
        assert_eq!(
            source_name("content/sandbox/riposte_sword/spawner/spawner.pattern.tft").as_deref(),
            Some("Riposte Sword Spawner")
        );
        assert!(source_name("content/sandbox/shared/projectile.pattern.tft").is_none());
        assert_eq!(
            common_name(&BTreeSet::from([
                "Cache Guardian Tank",
                "Challenger Guardian Tank"
            ]))
            .as_deref(),
            Some("Guardian Tank")
        );
        for name in [
            "Unknown plug",
            "Unknown Plug 0x12345678",
            "Effect 412",
            "0x12345678",
        ] {
            assert!(!meaningful_name(name));
        }
    }

    #[test]
    fn disagreeing_named_ancestors_are_listed_rather_than_dropped() {
        let context = |graph: u32, path: &str| Context {
            graph,
            owner: graph,
            offset: 0,
            path: path.to_owned(),
            name_evidence: None,
            item: None,
            perk: None,
            depth: 2,
        };
        let entry = Entry {
            graph: 0x80B3_0001,
            kind: Kind::Emitter,
            object_type: 0,
            owners: Vec::new(),
            package: "sandbox".into(),
            native_name: None,
            native_paths: Vec::new(),
            contexts: vec![
                context(
                    1,
                    "content/sandbox/characters/taken/taken_wizard/taken_wizard.pattern.tft",
                ),
                context(
                    2,
                    "content/sandbox/characters/hive/knight/hive_knight.pattern.tft",
                ),
                context(
                    3,
                    "content/sandbox/characters/cabal/psion/cabal_psion.pattern.tft",
                ),
                context(
                    4,
                    "content/sandbox/characters/vex/goblin/vex_goblin.pattern.tft",
                ),
            ],
            perk_indices: Vec::new(),
            source_hint: None,
        };
        assert_eq!(
            entry.discovery_name().as_deref(),
            Some("Shared by Cabal Psion, Hive Knight, Taken Wizard (+1) Emitter")
        );
    }

    #[test]
    fn a_legacy_name_on_the_graph_yields_to_a_perk_that_references_it() {
        let legacy = Context {
            graph: 0x80B3_0002,
            owner: 0x80B3_0002,
            offset: 8,
            path: "frag_grenade".into(),
            name_evidence: Some(NameEvidence {
                name: "frag_grenade".into(),
                hash: 0xDAD7_E57E,
                source: "Destiny 1 PS4 alpha wwise event frag_grenade_throw".into(),
                legacy: true,
            }),
            item: None,
            perk: None,
            depth: 1_000,
        };
        let perk = Context {
            graph: 0x80B3_0003,
            owner: 0x80B3_0002,
            offset: 0,
            path: String::new(),
            name_evidence: None,
            item: None,
            perk: Some(7),
            depth: 1,
        };
        let mut entry = Entry {
            graph: 0x80B3_0002,
            kind: Kind::Emitter,
            object_type: 17,
            owners: Vec::new(),
            package: "sandbox".into(),
            native_name: None,
            native_paths: Vec::new(),
            contexts: vec![legacy.clone(), perk],
            perk_indices: Vec::new(),
            source_hint: None,
        };
        let perk_name = |index: u16| (index == 7).then(|| "New Tricks".to_owned());
        assert_eq!(
            entry.discovery_name_with(perk_name, |_| None).as_deref(),
            Some("New Tricks Emitter")
        );
        entry.contexts = vec![legacy];
        assert_eq!(
            entry.discovery_name_with(perk_name, |_| None).as_deref(),
            Some("Frag Grenade Emitter")
        );
    }
}
