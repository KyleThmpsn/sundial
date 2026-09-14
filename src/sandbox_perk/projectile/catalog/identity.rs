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
        let direct = self
            .native_paths
            .iter()
            .chain(self.native_name.iter())
            .filter_map(|path| source_name(path))
            .collect::<BTreeSet<_>>();
        if let Some(name) = common_name(&direct.iter().map(String::as_str).collect()) {
            return Some(name);
        }
        if let Some(name) = familiar_name(self.graph) {
            return Some(name);
        }
        let named = self
            .contexts
            .iter()
            .filter_map(|context| {
                let name = source_name(&context.path)
                    .or_else(|| context.item.and_then(&mut item_name).map(|item| item.name))
                    .or_else(|| context.perk.and_then(&mut perk_name))?;
                (!name.starts_with("Ability ") && !name.starts_with("Effect ") && !name.is_empty())
                    .then_some((context.depth, name))
            })
            .collect::<Vec<_>>();
        let Some(depth) = named.iter().map(|(depth, _)| *depth).min() else {
            // Direct perk references remain useful even when ancestry has no path.
            let names = self
                .perk_indices
                .iter()
                .filter_map(|&index| perk_name(index))
                .filter(|name| {
                    !name.is_empty()
                        && !name.starts_with("Ability ")
                        && !name.starts_with("Effect ")
                })
                .collect::<BTreeSet<_>>();
            return common_name(&names.iter().map(String::as_str).collect())
                .map(|name| format!("{name} {}", self.kind.label()));
        };
        let names = named
            .iter()
            .filter(|(d, _)| *d == depth)
            .map(|(_, name)| name.as_str())
            .collect::<BTreeSet<_>>();
        let common = common_name(&names).or_else(|| {
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
    if lower.is_empty()
        || shared_metadata_path(&lower)
        || lower.contains("debug")
        || lower.ends_with(".sequence.tft")
        || lower.ends_with(".fx_sequence.tft")
    {
        return None;
    }
    let parts = lower.split('/').collect::<Vec<_>>();
    let stem = parts.last()?.split('.').next()?;
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
    if words.is_empty()
        || words.iter().all(|word| {
            matches!(
                *word,
                "entity" | "pattern" | "projectile" | "default" | "base"
            )
        })
    {
        return None;
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
    Some(
        words
            .iter()
            .enumerate()
            .filter(|(index, word)| {
                !(**word == "bauble" && words.get(index + 1) == Some(&"pickup"))
            })
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
            .join(" "),
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
        assert!(source_name("[debug]_fallen.sequence.tft").is_none());
        assert!(source_name("label_globals.label_globals.tft").is_none());
        assert!(common_name(&BTreeSet::from(["Fallen Shank", "Fallen Captain"])).is_none());
    }
}
