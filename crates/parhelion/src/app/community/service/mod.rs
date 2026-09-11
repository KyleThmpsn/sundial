//! Community catalog, server transport, and checked local recipe downloads.
mod client;
mod library;
mod submission;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(crate) use client::Client;
pub(crate) use library::{Downloaded, Receipt, install, load_receipt, remix, remix_origin};
pub(crate) use submission::Submission;

pub(crate) const ENDPOINT: &str = "https://bngcodex.ktrs.io/api/parhelion";
pub(crate) const MAX_RECIPE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct TestedWith {
    pub sundial: String,
    pub sunrise: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Listing {
    pub id: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
    pub version: u32,
    pub license: String,
    pub source_url: String,
    pub tested_with: TestedWith,
    pub gameplay_status: String,
    pub gameplay_notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remix_of: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Entry {
    #[serde(flatten)]
    pub listing: Listing,
    pub name: String,
    pub namespace: String,
    pub download: String,
    pub sha256: String,
    pub bytes: usize,
    #[serde(default)]
    pub downloads: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Catalog {
    pub schema: u32,
    pub recipes: Vec<Entry>,
}

pub(crate) fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

pub(crate) fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn text(value: &str, limit: usize, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > limit {
        return Err(format!(
            "{field} must contain between 1 and {limit} characters"
        ));
    }
    Ok(())
}

impl Listing {
    pub fn validate(&self) -> Result<(), String> {
        if !slug(&self.id) || self.version == 0 || self.license != "GPL-3.0-only" {
            return Err("Invalid recipe ID, revision, or sharing license".into());
        }
        for (value, label) in [
            (&self.author, "Creator"),
            (&self.description, "Description"),
            (&self.gameplay_notes, "Gameplay notes"),
            (&self.source_url, "Source"),
        ] {
            text(value, 2000, label)?;
        }
        let source = self
            .source_url
            .parse::<ureq::http::Uri>()
            .map_err(|_| "Invalid source URL")?;
        if source.scheme_str() != Some("https")
            || source.host().is_none()
            || source
                .authority()
                .is_some_and(|value| value.as_str().contains('@'))
        {
            return Err("The source link must use HTTPS".into());
        }
        if self.tags.is_empty()
            || self.tags.len() > 12
            || self.tags.iter().any(|tag| tag.len() > 40 || !slug(tag))
            || self.tags.iter().collect::<BTreeSet<_>>().len() != self.tags.len()
        {
            return Err("Use 1 to 12 distinct lowercase tags separated by commas".into());
        }
        if !matches!(
            self.gameplay_status.as_str(),
            "unverified" | "author-tested"
        ) {
            return Err("Unknown gameplay testing status".into());
        }
        for (version, label) in [
            (&self.tested_with.sundial, "Sundial version"),
            (&self.tested_with.sunrise, "Sunrise version"),
        ] {
            text(version, 100, label)?;
            if self.gameplay_status == "author-tested" && version.eq_ignore_ascii_case("unknown") {
                return Err("Gameplay testing needs the Sundial and Sunrise versions used".into());
            }
        }
        if self
            .remix_of
            .as_ref()
            .is_some_and(|id| !slug(id) || id == &self.id)
        {
            return Err("Choose a different, valid original recipe ID for a remix".into());
        }
        Ok(())
    }
}

impl Catalog {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || self.recipes.len() > 5000 {
            return Err("This community catalog is not supported by this Parhelion version".into());
        }
        let mut ids = BTreeSet::new();
        let mut namespaces = BTreeSet::new();
        for entry in &self.recipes {
            entry.listing.validate()?;
            text(&entry.name, 200, "Recipe name")?;
            crate::recipe::validate_parhelion_namespace(&entry.namespace)?;
            if !ids.insert(&entry.listing.id)
                || !namespaces.insert(&entry.namespace)
                || entry.download != format!("recipes/{}/recipe.parhelion.json", entry.listing.id)
                || entry.bytes == 0
                || entry.bytes > MAX_RECIPE_BYTES
                || entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("The catalog contains a duplicate or invalid recipe download".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
