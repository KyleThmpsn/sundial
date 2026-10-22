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
