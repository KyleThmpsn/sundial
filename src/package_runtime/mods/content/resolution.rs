//! The loader's manifest checks and dependency ordering, which decide the mods it places.
use super::{Incoming, name};
use crate::package_runtime::mods::{MANIFEST, checked_path, config, read_json};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashSet},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Mod {
    manifest_version: u32,
    pub(super) id: String,
    version: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    src: String,
    #[serde(default)]
    dependencies: Vec<Dependency>,
    #[serde(default)]
    incompatible: Vec<String>,
    #[serde(default)]
    cheat: bool,
    #[serde(default)]
    pub(super) config: Vec<config::Declaration>,
    #[serde(default)]
    pub(super) config_bindings: Vec<config::Binding>,
    /// Version 4's new package ids the mod ships, each a JSON integer or a quoted `0x` hex
    /// token. `None` when the key is absent, which the version check tells apart from `[]`.
    #[serde(default)]
    packages: Option<Vec<serde_json::Value>>,
    #[serde(skip)]
    pub(super) folder: PathBuf,
    /// Files read in place of the folder's packages and patches.
    #[serde(skip)]
    pub(super) incoming: Option<Incoming>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Dependency {
    id: String,
    min_version: String,
    max_version: Option<String>,
}

fn version(text: &str) -> Option<[u16; 3]> {
    let parts = text.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || !p.bytes().all(|b| b.is_ascii_digit())
                || (p.len() > 1 && p.starts_with('0'))
        })
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

pub(in crate::package_runtime::mods) fn valid_id(id: &str) -> bool {
    let parts = id.split('.').collect::<Vec<_>>();
    (3..=64).contains(&id.len())
        && parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && part.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_')
                })
        })
}

impl Mod {
    /// Parhelion's mod as an installation would publish it, with its own manifest.
    pub(super) fn incoming(incoming: Incoming) -> Self {
        let manifest =
            serde_json::to_value(&incoming.manifest).expect("Parhelion's manifest serializes");
        let mut item: Self =
            serde_json::from_value(manifest).expect("Parhelion's manifest reads as a mod");
        item.incoming = Some(incoming);
        item
    }

    /// The name Sunrise's Mods page shows.
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// The ids of the mods this one needs.
    pub(super) fn needs(&self) -> impl Iterator<Item = &str> {
        self.dependencies
            .iter()
            .map(|dependency| dependency.id.as_str())
    }

    fn valid(&self) -> bool {
        self.valid_identity()
            && self.valid_relationships()
            && self.valid_config()
            && self.valid_packages()
    }

    fn valid_identity(&self) -> bool {
        let text = |s: &str, max| s.len() <= max && !s.contains(char::is_control);
        (1..=4).contains(&self.manifest_version)
            && valid_id(&self.id)
            && version(&self.version).is_some()
            && !self.name.is_empty()
            && text(&self.name, 64)
            && text(&self.description, 512)
            && self.authors.len() <= 8
            && self.authors.iter().all(|a| !a.is_empty() && text(a, 64))
            && text(&self.src, 256)
    }

    fn valid_relationships(&self) -> bool {
        self.dependencies.len() <= 32
            && self.incompatible.len() <= 32
            && self
                .incompatible
                .iter()
                .all(|id| valid_id(id) && id != &self.id)
            && self
                .dependencies
                .iter()
                .all(|dependency| dependency.valid(&self.id))
    }

    fn valid_config(&self) -> bool {
        (self.manifest_version >= 2 || (!self.cheat && self.config.is_empty()))
            && (self.manifest_version >= 3 || self.config_bindings.is_empty())
            && self.config.len() <= 32
            && self.config.iter().all(config::Declaration::valid)
            && self
                .config
                .iter()
                .map(config::Declaration::key)
                .collect::<BTreeSet<_>>()
                .len()
                == self.config.len()
            && self.config_bindings.len() <= 256
            && self.config_bindings.iter().all(|b| b.valid(&self.config))
    }

    /// The loader's own package list rules: the key needs version 4 even when the list is
    /// empty, and it holds at most 256 distinct ids from `0x0100` to `0x0CFF`.
    fn valid_packages(&self) -> bool {
        let Some(packages) = &self.packages else {
            return true;
        };
        let Some(mut ids) = packages
            .iter()
            .map(own_package_id)
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        ids.sort_unstable();
        self.manifest_version >= 4
            && ids.len() <= OWN_PACKAGE_LIMIT
            && ids.windows(2).all(|pair| pair[0] != pair[1])
    }
}

/// Own package ids one manifest may list. The loader keeps them in fixed storage.
const OWN_PACKAGE_LIMIT: usize = 256;

/// One listed id, as the loader reads it: an unsigned JSON integer or a quoted `0x` or `0X` hex
/// token with no sign, inside the range a mod may own.
fn own_package_id(value: &serde_json::Value) -> Option<u16> {
    let id = match value {
        serde_json::Value::Number(number) => number.as_u64()?,
        serde_json::Value::String(text) => {
            let digits = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            u64::from_str_radix(digits, 16).ok()?
        }
        _ => return None,
    };
    (0x0100..=0x0CFF)
        .contains(&id)
        .then(|| u16::try_from(id).ok())
        .flatten()
}

impl Dependency {
    fn valid(&self, owner: &str) -> bool {
        let Some(minimum) = version(&self.min_version) else {
            return false;
        };
        valid_id(&self.id)
            && self.id != owner
            && self
                .max_version
                .as_deref()
                .is_none_or(|maximum| version(maximum).is_some_and(|maximum| maximum >= minimum))
    }

    fn accepts(&self, candidate: &str) -> bool {
        let candidate = version(candidate).expect("candidate version was validated");
        let minimum = version(&self.min_version).expect("dependency minimum was validated");
        candidate >= minimum
            && self.max_version.as_deref().is_none_or(|maximum| {
                candidate <= version(maximum).expect("dependency maximum was validated")
            })
    }
}

/// The mods the loader places from `folders` and `extra`, in load order, and why it leaves out each
/// other one. It refuses unreadable manifests, duplicate ids, dependencies that are missing, the
/// wrong version or refused, declared incompatibilities and dependency cycles. Each mod is placed
/// once its dependencies are, lowest id first.
pub(super) fn resolve(
    install: &Path,
    folders: &[PathBuf],
    extra: Vec<Mod>,
) -> (Vec<Mod>, Vec<String>) {
    let mut notices = Vec::new();
    let mut mods = Vec::new();
    for folder in folders {
        let manifest = folder.join(MANIFEST);
        let read = checked_path(install, &manifest).and_then(|()| {
            if manifest.is_file() {
                read_json::<Mod>(&manifest)
            } else {
                Err("it has no mod.json".to_owned())
            }
        });
        match read {
            Ok(mut item) if item.valid() => {
                item.folder = folder.clone();
                mods.push(item);
            }
            Ok(_) => notices.push(format!(
                "{} was left out: its mod.json has values the loader refuses",
                name(folder)
            )),
            Err(error) => notices.push(format!("{} was left out: {error}", name(folder))),
        }
    }
    mods.extend(extra);
    mods.sort_by(|a, b| a.id.cmp(&b.id));
    let mut refusals = vec![None; mods.len()];
    for pair in 0..mods.len().saturating_sub(1) {
        if mods[pair].id == mods[pair + 1].id {
            for index in [pair, pair + 1] {
                refusals[index] = Some("another mod folder has the same ID".to_owned());
            }
        }
    }
    refuse_dependents(&mods, &mut refusals);
    // The loader checks incompatibilities against the mods loadable before the check.
    let loadable = refusals.iter().map(Option::is_none).collect::<Vec<_>>();
    for (item, refusal) in mods.iter().zip(&mut refusals) {
        let declared = item.incompatible.iter().find(|id| {
            mods.iter()
                .zip(&loadable)
                .any(|(other, &ok)| ok && other.id == **id)
        });
        if refusal.is_none()
            && let Some(other) = declared
        {
            *refusal = Some(format!("it declares {other} incompatible"));
        }
    }
    refuse_dependents(&mods, &mut refusals);
    let mut order = Vec::new();
    {
        let mut placed = HashSet::new();
        while let Some(index) = (0..mods.len()).find(|&index| {
            refusals[index].is_none()
                && !placed.contains(mods[index].id.as_str())
                && mods[index].needs().all(|id| placed.contains(id))
        }) {
            placed.insert(mods[index].id.as_str());
            order.push(index);
        }
    }
    for (index, refusal) in refusals.iter_mut().enumerate() {
        if refusal.is_none() && !order.contains(&index) {
            *refusal = Some("its dependencies form a cycle".to_owned());
        }
    }
    for (item, refusal) in mods.iter().zip(&refusals) {
        if let Some(reason) = refusal {
            notices.push(format!("{} was left out: {reason}", item.id));
        }
    }
    let mut mods = mods.into_iter().map(Some).collect::<Vec<_>>();
    let ordered = order
        .into_iter()
        .map(|index| mods[index].take().expect("each mod is placed once"))
        .collect::<Vec<_>>();
    (ordered, notices)
}

/// The family `folders` as installation and removal read them: every member, each after the
/// members it needs that are present. A manifest that can't be read fails the read.
pub(super) fn family(install: &Path, folders: &[PathBuf]) -> Result<Vec<Mod>, String> {
    let mut mods = Vec::new();
    for folder in folders {
        let manifest = folder.join(MANIFEST);
        checked_path(install, &manifest)?;
        let mut item = read_json::<Mod>(&manifest)?;
        item.folder = folder.clone();
        mods.push(item);
    }
    Ok(ordered(mods))
}

/// `mods` lowest id first, each after the mods it needs among them. A cycle keeps id order.
pub(super) fn ordered(mut mods: Vec<Mod>) -> Vec<Mod> {
    mods.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ordered = Vec::with_capacity(mods.len());
    while !mods.is_empty() {
        let next = (0..mods.len())
            .find(|&index| {
                mods[index]
                    .needs()
                    .all(|id| id == mods[index].id || !mods.iter().any(|item| item.id == id))
            })
            .unwrap_or(0);
        ordered.push(mods.remove(next));
    }
    ordered
}

/// Refuses each loadable mod whose dependency is missing, the wrong version or refused, until a
/// pass refuses no more.
fn refuse_dependents(mods: &[Mod], refusals: &mut [Option<String>]) {
    loop {
        let mut changed = false;
        for index in 0..mods.len() {
            if refusals[index].is_some() {
                continue;
            }
            let missing = mods[index].dependencies.iter().find(|dependency| {
                !mods.iter().zip(refusals.iter()).any(|(item, refusal)| {
                    refusal.is_none()
                        && item.id == dependency.id
                        && dependency.accepts(&item.version)
                })
            });
            if let Some(dependency) = missing {
                refusals[index] = Some(format!(
                    "it needs {} in a version it accepts, which isn't loaded",
                    dependency.id
                ));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}
