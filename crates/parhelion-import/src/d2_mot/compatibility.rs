//! Verified recipe defaults, independent of any user's previously converted assets.
use serde::Deserialize;
use serde_json::Value;
use std::{borrow::Cow, sync::OnceLock};

#[derive(Deserialize)]
pub(crate) struct Profile {
    pub source: u32,
    pub name: String,
    pub namespace: String,
    pub model_donor: u32,
    pub donor: Value,
    pub overrides: Value,
}

fn profiles() -> &'static [Profile] {
    static PROFILES: OnceLock<Vec<Profile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        serde_json::from_str(include_str!("compatibility/profiles.json"))
            .expect("embedded and tested import profiles")
    })
}

pub(crate) fn profile(source: u32) -> Option<&'static Profile> {
    profiles().iter().find(|p| p.source == source)
}

pub(crate) fn known_weapons() -> impl Iterator<Item = (u32, &'static str)> {
    profiles().iter().map(|p| (p.source, p.name.as_str()))
}

pub(crate) fn namespace(source: u32) -> Cow<'static, str> {
    profile(source).map_or_else(
        || Cow::Owned(format!("parhelion.bulk.{source:08x}")),
        |p| Cow::Borrowed(p.namespace.as_str()),
    )
}

/// Donor-specific runtime defaults must never be applied to a different model conversion.
pub(crate) fn apply(source: u32, model_donor: u32, recipe: &mut Value) -> bool {
    let Some(profile) = profile(source).filter(|p| p.model_donor == model_donor) else {
        return false;
    };
    recipe["presentation_donor"] = recipe["donor"].clone();
    recipe["donor"] = profile.donor.clone();
    for (key, value) in profile
        .overrides
        .as_object()
        .expect("tested profile overrides")
    {
        recipe["overrides"][key] = value.clone();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn verified_profiles_are_unique_and_contain_no_local_assets_or_source_text() {
        assert_eq!(profiles().len(), 47);
        let mut sources = std::collections::BTreeSet::new();
        let mut namespaces = std::collections::BTreeSet::new();
        for p in profiles() {
            assert!(sources.insert(p.source));
            assert!(namespaces.insert(&p.namespace));
            assert!(!p.name.is_empty());
            assert!(p.donor["item_hash"].as_str().is_some());
            for field in ["imported_graph", "icon_edit", "lore", "remove_lore"] {
                assert!(p.overrides.get(field).is_none());
            }
            assert!(!serde_json::to_string(&p.overrides).unwrap().contains("C:"));
        }
    }
    #[test]
    fn sword_profile_keeps_separate_gameplay_and_model_donors_without_replacing_lore() {
        let mut recipe =
            json!({"donor":{"item_hash":"0x02222CBF"},"overrides":{"lore":"Current source lore"}});
        assert!(apply(0xC223445E, 0x02222CBF, &mut recipe));
        assert_eq!(recipe["donor"]["item_hash"], "0xFC2FD6BF");
        assert_eq!(recipe["presentation_donor"]["item_hash"], "0x02222CBF");
        assert_eq!(
            recipe["overrides"]["weapon_pattern_donor_hash"],
            "0xFC2FD6BF"
        );
        assert_eq!(recipe["overrides"]["lore"], "Current source lore");
        assert!(!apply(0xC223445E, 1, &mut recipe));
    }
}
