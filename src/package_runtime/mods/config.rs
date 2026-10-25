//! Startup scalar projection, following Sunrise f1e7c66b's source identity and CUI profiles.
use super::{Refusal, content, read_json, runtime_directory};
use crate::package_payload::write_bytes;
use crate::package_runtime::reader::PackageManager;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
use tiger_pkg::TagHash;

mod scalar;

const CUI_CLASS: u32 = 0x80804825;
const HEADER_SIZE: usize = 344;
const ENTRY_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum Kind {
    Int,
    Float,
    Bool,
}

impl Kind {
    fn bytes(self, value: &serde_json::Value) -> Option<Vec<u8>> {
        match self {
            Self::Int => Some(i32::try_from(value.as_i64()?).ok()?.to_le_bytes().to_vec()),
            Self::Float => {
                let value = value.as_f64()? as f32;
                value.is_finite().then(|| value.to_le_bytes().to_vec())
            }
            Self::Bool => Some(vec![u8::from(value.as_bool()?)]),
        }
    }
    fn width(self) -> usize {
        if self == Self::Bool { 1 } else { 4 }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Declaration {
    key: String,
    name: String,
    #[serde(rename = "type")]
    kind: Kind,
    default: serde_json::Value,
}

fn key(value: &str) -> bool {
    (1..=32).contains(&value.len())
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl Declaration {
    pub(super) fn valid(&self) -> bool {
        key(&self.key)
            && !self.name.is_empty()
            && self.name.len() <= 64
            && !self.name.contains(char::is_control)
            && self.kind.bytes(&self.default).is_some()
    }
    pub(super) fn key(&self) -> &str {
        &self.key
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Binding {
    key: String,
    profile: String,
    tag: u32,
    #[serde(rename = "class")]
    class: u32,
    size: usize,
    source_sha256: String,
    offset: usize,
    #[serde(rename = "type")]
    kind: Kind,
    property_offset: Option<usize>,
    pool_offset: Option<usize>,
    binding_offset: Option<usize>,
}

impl Binding {
    pub(super) fn tag(&self) -> TagHash {
        TagHash(self.tag)
    }
    pub(super) fn valid(&self, declarations: &[Declaration]) -> bool {
        self.valid_profile()
            && self.class == CUI_CLASS
            && super::super::is_valid_package_tag(self.tag())
            && (HEADER_SIZE..=ENTRY_LIMIT).contains(&self.size)
            && self
                .offset
                .checked_add(self.kind.width())
                .is_some_and(|end| end <= self.size)
            && self.source_sha256.len() == 64
            && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && declarations
                .iter()
                .any(|d| d.key == self.key && d.kind == self.kind)
    }

    fn valid_profile(&self) -> bool {
        match self.profile.as_str() {
            "cui-property-v1" => {
                self.property_offset.is_some()
                    && self.pool_offset.is_some()
                    && self.binding_offset.is_none()
                    && self.kind != Kind::Int
            }
            "cui-comparison-v1" => {
                self.property_offset.is_none()
                    && self.pool_offset.is_none()
                    && self.binding_offset.is_some()
                    && self.kind != Kind::Bool
            }
            _ => false,
        }
    }
}

/// A validated binding paired with the saved startup value or its typed default.
pub(super) struct Action {
    owner: String,
    site: Binding,
    value: Vec<u8>,
}

impl Action {
    pub(super) fn tag(&self) -> TagHash {
        self.site.tag()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    #[serde(default)]
    mods: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

impl Saved {
    fn read(install: &Path) -> Option<Self> {
        let path = runtime_directory(install).join("mod_config.json");
        fs::metadata(&path)
            .ok()
            .filter(|metadata| metadata.len() <= 512 * 1024)
            .and_then(|_| read_json::<Self>(&path).ok())
            .filter(Self::valid)
    }

    fn valid(&self) -> bool {
        self.version == 1
            && self.mods.values().map(BTreeMap::len).sum::<usize>() <= 2048
            && self.mods.iter().all(|(id, values)| {
                content::valid_id(id)
                    && values.iter().all(|(key_name, value)| {
                        key(key_name) && (value.is_number() || value.is_boolean())
                    })
            })
    }
}

pub(super) fn prepare(
    install: &Path,
    id: &str,
    sites: &[Binding],
    declarations: &[Declaration],
) -> Vec<Action> {
    // Like Sunrise, an invalid saved-values file contributes no values. It is never rewritten.
    let saved = Saved::read(install);
    sites
        .iter()
        .map(|site| {
            let declaration = declarations
                .iter()
                .find(|d| d.key == site.key)
                .expect("binding declaration was validated");
            let value = saved
                .as_ref()
                .and_then(|s| s.mods.get(id))
                .and_then(|m| m.get(&site.key))
                .and_then(|v| site.kind.bytes(v))
                .unwrap_or_else(|| {
                    site.kind
                        .bytes(&declaration.default)
                        .expect("declaration default was validated")
                });
            Action {
                owner: id.to_owned(),
                site: site.clone(),
                value,
            }
        })
        .collect()
}

/// Each bound entry with its configured values written, or the first mod whose values don't fit
/// the entry they bind, which the loader refuses.
pub(super) fn apply(
    manager: &PackageManager,
    actions: &[Action],
) -> Result<Vec<(TagHash, Vec<u8>)>, Refusal> {
    let mut groups = BTreeMap::<u32, Vec<&Action>>::new();
    for action in actions {
        groups.entry(action.site.tag).or_default().push(action);
    }
    // A later mod can't replace an entry another binds, so each entry's values are one mod's.
    groups
        .into_iter()
        .map(|(tag, actions)| {
            configure(manager, tag, &actions).map_err(|reason| Refusal {
                id: actions[0].owner.clone(),
                reason: format!("its configuration of entry 0x{tag:08X} was refused: {reason}"),
            })
        })
        .collect()
}

/// One entry with its bound values written, once it matches the source the bindings name.
fn configure(
    manager: &PackageManager,
    tag: u32,
    actions: &[&Action],
) -> Result<(TagHash, Vec<u8>), String> {
    let mut bytes = manager.read_tag(TagHash(tag))?;
    let header = manager
        .get_entry(TagHash(tag))
        .ok_or("the entry has no metadata")?;
    let digest = hex::encode(Sha256::digest(&bytes));
    let mut sites = actions.iter().map(|a| &a.site).collect::<Vec<_>>();
    sites.sort_by_key(|s| s.offset);
    if !matches!(header.file_type, 8 | 16)
        || header.reference != CUI_CLASS
        || sites
            .iter()
            .any(|s| s.size != bytes.len() || !s.source_sha256.eq_ignore_ascii_case(&digest))
        || sites
            .windows(2)
            .any(|w| w[0].offset + w[0].kind.width() > w[1].offset)
    {
        return Err("it doesn't match the source its bindings name".into());
    }
    scalar::validate(&bytes, &sites)?;
    for action in actions {
        write_bytes(&mut bytes, action.site.offset, &action.value)?;
    }
    Ok((TagHash(tag), bytes))
}
