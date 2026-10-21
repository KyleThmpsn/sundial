//! The content Sunrise 0.6's loader serves from its mods folder. Each mod is checked in load order
//! and left out, with a notice, wherever the loader would refuse it, so another editor's mod never
//! stops a reader. Where several mods change one entry, it is merged as the loader merges it, or
//! read from the mod that loads later.
use super::merge::{
    Status,
    sidecar::{self, METADATA_DIGEST_SIZE, Sidecar},
};
use super::{
    FOLDER_LIMIT, ID, MANIFEST, Refusal, checked_path, config, is_package, runtime_directory,
};
use crate::package_payload::bytes_at;
use resolution::{Mod, resolve};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ffi::OsString,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use tiger_pkg::{TagHash, manager::PackagePath};

mod resolution;
mod shared;
pub(super) use resolution::valid_id;
pub use shared::Coexistence;

/// The loader serves at most this many mod package files together.
const FILE_LIMIT: usize = 1024;

/// Parhelion's mod as an installation would publish it: its new packages and its patches, each
/// with its merge data beside it under the same name. A plan reads these files in place of
/// Parhelion's installed folder.
#[derive(Clone, Debug, Default)]
pub struct Incoming {
    pub manifest: super::Manifest,
    pub packages: Vec<PathBuf>,
    pub patches: Vec<PathBuf>,
    /// Importer members published with the main mod, including verified installed members.
    pub family: Vec<Incoming>,
    /// Names of files in stock `packages` the installation removes, which an earlier layout left.
    pub retired: Vec<String>,
}

/// The mod folders the loader reads, sorted by name, and those past its limit.
#[derive(Default)]
struct Folders {
    read: Vec<PathBuf>,
    skipped: Vec<PathBuf>,
}

/// Every folder in Sunrise's `mods` folder whose name doesn't start with a dot, as the loader
/// lists them. A leading dot marks a folder being staged or set aside. With `incoming`,
/// Parhelion's folder counts toward the limit whether or not it is installed yet.
fn folders(install: &Path, incoming: Option<&Incoming>) -> Result<Folders, String> {
    let root = runtime_directory(install).join("mods");
    checked_path(install, &root)?;
    let entries = match fs::read_dir(&root) {
        Ok(entries) => Some(entries),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Could not read {}: {e}", root.display())),
    };
    let mut names = Vec::new();
    for entry in entries.into_iter().flatten() {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        if !name.as_encoded_bytes().starts_with(b".") && root.join(&name).is_dir() {
            names.push(name);
        }
    }
    if let Some(incoming) = incoming {
        names.retain(|name| !name.to_str().is_some_and(super::is_family_id));
        names.push(incoming.manifest.id.clone().into());
        names.extend(
            incoming
                .family
                .iter()
                .map(|member| OsString::from(&member.manifest.id)),
        );
    }
    names.sort();
    let skipped = names.split_off(names.len().min(FOLDER_LIMIT));
    let paths = |names: Vec<OsString>| -> Vec<PathBuf> {
        names.into_iter().map(|name| root.join(name)).collect()
    };
    Ok(Folders {
        read: paths(names),
        skipped: paths(skipped),
    })
}

fn name(path: &Path) -> Cow<'_, str> {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
}

/// The package files and merge data in one mod subfolder, sorted. The loader skips folders and
/// other files there, and a missing subfolder is empty. Sundial doesn't follow links.
fn children(path: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("its {} folder can't be listed: {e}", name(path))),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let listed = is_package(&path)
            || path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("merge"));
        if listed && kind.is_symlink() {
            return Err(format!(
                "{} is a link, which Sundial doesn't follow",
                name(&path)
            ));
        }
        if listed && kind.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn packages(path: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(children(path)?
        .into_iter()
        .filter(|p| is_package(p))
        .collect())
}

/// The files that decide what the loader serves: each mod folder's manifest, packages and merge
/// data, and the saved configuration. Refused mods are included, since an edit can admit one.
/// A runtime that doesn't load Sunrise mods reads none of them.
pub(super) fn files(install: &Path) -> Result<Vec<PathBuf>, String> {
    let folders = folders(install, None)?;
    if folders.read.is_empty() || !super::loads_mods(install)? {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for folder in folders.read {
        // A folder or manifest Sundial won't read is left out with a notice, adding no inputs.
        let manifest = folder.join(MANIFEST);
        if checked_path(install, &manifest).is_err() {
            continue;
        }
        if manifest.is_file() {
            files.push(manifest);
        }
        for child in ["packages", "patches"] {
            files.extend(children(&folder.join(child)).unwrap_or_default());
        }
    }
    let config = runtime_directory(install).join("mod_config.json");
    if config.is_file() {
        files.push(config);
    }
    Ok(files)
}

/// A patch read in a layer of its own: its package's earlier generations, the patch itself, and
/// the entries the layer serves.
pub(super) struct Patch {
    pub source: PathBuf,
    pub parents: Vec<PathBuf>,
    pub tags: Vec<TagHash>,
}

pub(in crate::package_runtime) struct Layer {
    pub packages: PathBuf,
    pub tags: Vec<TagHash>,
}

/// What a view projects: packages shown in the view itself, patches read in layers, entries the
/// loader serves merged, configured values, and what was left out and why.
pub(super) struct Plan {
    pub primary: Vec<PathBuf>,
    pub patches: Vec<Patch>,
    pub merged: Vec<(TagHash, Vec<u8>)>,
    pub actions: Vec<config::Action>,
    pub notices: Vec<String>,
    pub coexistence: Coexistence,
}

impl Plan {
    /// Parhelion's installed mod folder alone, as installation and removal read it.
    pub(super) fn installed(files: &[PathBuf]) -> Self {
        Self {
            primary: files.iter().filter(|p| is_package(p)).cloned().collect(),
            patches: Vec::new(),
            merged: Vec::new(),
            actions: Vec::new(),
            notices: Vec::new(),
            coexistence: Coexistence::default(),
        }
    }

    /// What the loader serves, admitting mods one at a time in load order. `refused` are mods a
    /// reader found the loader refuses once their configured values are written. `incoming` is
    /// read in place of Parhelion's installed folder.
    pub(super) fn read(
        install: &Path,
        refused: &[Refusal],
        incoming: Option<&Incoming>,
    ) -> Result<Self, String> {
        let mut folders = folders(install, incoming)?;
        let mut notices = folders
            .skipped
            .iter()
            .map(|folder| {
                format!(
                    "{} was left out: Sunrise reads only the {FOLDER_LIMIT} mod folders whose names sort first",
                    name(folder)
                )
            })
            .collect::<Vec<_>>();
        let extra = incoming
            .into_iter()
            .flat_map(|incoming| std::iter::once(incoming).chain(&incoming.family))
            .filter(|member| {
                folders
                    .read
                    .contains(&super::member_directory(install, &member.manifest.id))
            })
            .cloned()
            .map(Mod::incoming)
            .collect();
        if incoming.is_some() {
            folders
                .read
                .retain(|folder| !super::is_family_id(&name(folder)));
        }
        let (mods, left_out) = resolve(install, &folders.read, extra);
        let mut refusal = left_out
            .iter()
            .find(|notice| notice.starts_with(&format!("{ID} was left out")))
            .cloned();
        notices.extend(left_out);
        let retired = incoming.map_or(&[][..], |incoming| &incoming.retired[..]);
        let mut stage = Stage::new(install, retired)?;
        for item in mods {
            let checked = match refused.iter().find(|refusal| refusal.id == item.id) {
                Some(refusal) => Err(refusal.reason.clone()),
                None => stage.check(install, &item),
            };
            match checked {
                Ok(mut staged) => {
                    notices.append(&mut staged.notices);
                    stage.commit(&item.id, staged);
                }
                Err(reason) => {
                    let notice = format!("{} was left out: {reason}", item.id);
                    if super::is_family_id(&item.id) {
                        refusal = Some(notice.clone());
                    }
                    notices.push(notice);
                }
            }
        }
        let admitted = stage.admitted.iter().any(|id| id == ID);
        let mut plan = stage.compose(notices);
        if incoming.is_some() && !admitted {
            plan.coexistence.problems.insert(
                0,
                refusal.unwrap_or_else(|| {
                    format!(
                        "{ID} was left out: Sunrise reads only the {FOLDER_LIMIT} mod folders whose names sort first"
                    )
                }),
            );
        }
        Ok(plan)
    }
}

/// A package's id and name. The loader takes `w64_<family>_<id>_<generation>.pkg`, and never a
/// startup `_unp1_` package.
fn parsed(path: &Path) -> Result<(u16, PackagePath), String> {
    let refused = || format!("{} isn't a package name the loader accepts", name(path));
    let item = path
        .to_str()
        .and_then(PackagePath::parse)
        .ok_or_else(refused)?;
    let id = u16::from_str_radix(&item.id, 16).map_err(|_| refused())?;
    if item.platform != "w64"
        || item.filename.contains("_unp1_")
        || item
            .language
            .as_deref()
            .is_some_and(|language| language != "en")
    {
        return Err(refused());
    }
    Ok((id, item))
}

fn generation(path: &Path) -> u8 {
    parsed(path).map_or(0, |(_, item)| item.patch)
}

/// The id in a startup `_unp1_` package's header. The loader keeps mods from adding one.
fn startup_id(path: &Path) -> Option<u16> {
    if !name(path).contains("_unp1_") {
        return None;
    }
    let mut header = [0; 6];
    File::open(path).ok()?.read_exact(&mut header).ok()?;
    Some(u16::from_le_bytes([header[4], header[5]]))
}

/// A package's 16-byte entry rows, and the header digest its merge data binds to.
struct Table {
    rows: Vec<[u8; 16]>,
    digest: [u8; METADATA_DIGEST_SIZE],
}

/// The entry table of a package whose header is a Shadowkeep package matching its name.
fn table(path: &Path) -> Result<Table, String> {
    let unreadable = |error: std::io::Error| format!("{} can't be read: {error}", name(path));
    let mut file = File::open(path).map_err(unreadable)?;
    let mut header = [0; 0x170];
    file.read_exact(&mut header).map_err(unreadable)?;
    let count = u32::from_le_bytes(bytes_at(&header, 0xb4)?);
    let offset = u64::from(u32::from_le_bytes(bytes_at(&header, 0x110)?)) + 96;
    let length = file.metadata().map_err(unreadable)?.len();
    if header[..4] != [38, 0, 2, 0]
        || header[0x1a] != 1
        || !(1..=8192).contains(&count)
        || offset + u64::from(count) * 16 > length
    {
        return Err(format!(
            "{} isn't a Shadowkeep package the loader reads",
            name(path)
        ));
    }
    let (id, item) = parsed(path)?;
    if u16::from_le_bytes(bytes_at(&header, 4)?) != id
        || u16::from_le_bytes(bytes_at(&header, 0x20)?) != u16::from(item.patch)
    {
        return Err(format!(
            "the header of {} names another package or generation",
            name(path)
        ));
    }
    file.seek(SeekFrom::Start(offset)).map_err(unreadable)?;
    let mut rows = vec![[0; 16]; count as usize];
    file.read_exact(rows.as_flattened_mut())
        .map_err(unreadable)?;
    Ok(Table {
        rows,
        digest: bytes_at(&header, sidecar::PACKAGE_DIGEST_OFFSET)?,
    })
}

/// The whole merge data file at `path`, or `None` when there is none.
fn read_sidecar(path: &Path) -> Result<Option<Vec<u8>>, Status> {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Status::Format),
        Ok(metadata) if metadata.len() > sidecar::BYTE_LIMIT as u64 => {
            return Err(Status::Capacity);
        }
        Ok(_) => {}
    }
    fs::read(path).map(Some).map_err(|_| Status::Format)
}

/// The file name of the generation after `path`'s, which a patch on it must have.
fn next_generation(path: &Path) -> Result<String, String> {
    let (_, item) = parsed(path)?;
    let stem = item.filename.strip_suffix(&format!("_{}.pkg", item.patch));
    match (stem, item.patch.checked_add(1)) {
        (Some(stem), Some(next)) => Ok(format!("{stem}_{next}.pkg")),
        _ => Err(format!(
            "package {} can't take another generation",
            item.filename
        )),
    }
}

/// A package's generations, oldest first, and the mod that added it. Installed packages have no
/// owner. A patch extends the newest generation.
struct Base {
    generations: Vec<PathBuf>,
    owner: Option<String>,
}

/// One mod's patch, the entries it changes, and its merge data.
struct Claim {
    owner: String,
    package: u16,
    path: PathBuf,
    changed: BTreeSet<TagHash>,
    sidecar: Option<Sidecar>,
}

/// One generation of a package a mod adds, and its merge data.
struct Added {
    owner: String,
    package: u16,
    path: PathBuf,
    sidecar: Option<Sidecar>,
}

/// What one mod adds if the loader admits it.
#[derive(Default)]
struct Staged {
    packages: BTreeMap<u16, Vec<Added>>,
    claims: Vec<Claim>,
    actions: Vec<config::Action>,
    /// Merge data the loader ignores, which doesn't stop the mod from loading.
    notices: Vec<String>,
}

impl Staged {
    /// The merge data beside `path`, which the loader reads only when it was built for that file.
    fn sidecar(
        &mut self,
        owner: &str,
        path: &Path,
        package: u16,
        digest: &[u8; METADATA_DIGEST_SIZE],
    ) -> Option<Sidecar> {
        let parsed = read_sidecar(&path.with_extension(sidecar::EXTENSION))
            .transpose()?
            .and_then(|file| Sidecar::parse(&file, package, u16::from(generation(path)), digest));
        parsed
            .map_err(|reason| self.notices.push(shared::ignored(owner, package, reason)))
            .ok()
    }
}

/// What the mods admitted so far add, as the loader stages them.
struct Stage {
    bases: BTreeMap<u16, Base>,
    /// The ids of the startup packages, which no mod may add.
    reserved: HashSet<u16>,
    added: Vec<Added>,
    claims: Vec<Claim>,
    /// Each entry an admitted mod binds configured values to, and that mod.
    bound: HashMap<TagHash, String>,
    actions: Vec<config::Action>,
    /// In load order.
    admitted: Vec<String>,
}

impl Stage {
    /// The installed packages, leaving out the `retired` files an installation removes.
    fn new(install: &Path, retired: &[String]) -> Result<Self, String> {
        let mut bases = BTreeMap::<u16, Base>::new();
        let mut reserved = HashSet::new();
        for entry in fs::read_dir(install.join("packages")).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            let leaving = retired
                .iter()
                .any(|name| path.file_name().is_some_and(|own| own == name.as_str()));
            if !is_package(&path) || leaving {
                continue;
            }
            if let Ok((id, _)) = parsed(&path) {
                bases
                    .entry(id)
                    .or_insert_with(|| Base {
                        generations: Vec::new(),
                        owner: None,
                    })
                    .generations
                    .push(path);
            } else if let Some(id) = startup_id(&path) {
                reserved.insert(id);
            }
        }
        for base in bases.values_mut() {
            base.generations.sort_by_key(|path| generation(path));
        }
        Ok(Self {
            bases,
            reserved,
            added: Vec::new(),
            claims: Vec::new(),
            bound: HashMap::new(),
            actions: Vec::new(),
            admitted: Vec::new(),
        })
    }

    /// What `item` adds, or why the loader refuses it, checked in the loader's order.
    fn check(&self, install: &Path, item: &Mod) -> Result<Staged, String> {
        if let Some(needed) = item
            .needs()
            .find(|id| !self.admitted.iter().any(|admitted| admitted == id))
        {
            return Err(format!("it needs {needed}, which was left out"));
        }
        let mut staged = Staged::default();
        let new_packages = match &item.incoming {
            Some(incoming) => incoming.packages.clone(),
            None => packages(&item.folder.join("packages"))?,
        };
        for path in new_packages {
            let (id, _) = parsed(&path)?;
            if !(0x100..=0xcff).contains(&id) {
                return Err(format!(
                    "{}: new package ids must be 0100 to 0CFF",
                    name(&path)
                ));
            }
            if let Some(base) = self.bases.get(&id) {
                return Err(match &base.owner {
                    None => format!("package {id:04X} is already installed"),
                    Some(owner) => format!("package {id:04X} is also added by {owner}"),
                });
            }
            if self.reserved.contains(&id) {
                return Err(format!("package {id:04X} is used by a startup package"));
            }
            let digest = table(&path)?.digest;
            let sidecar = staged.sidecar(&item.id, &path, id, &digest);
            staged.packages.entry(id).or_default().push(Added {
                owner: item.id.clone(),
                package: id,
                path,
                sidecar,
            });
        }
        for (id, generations) in &mut staged.packages {
            generations.sort_by_key(|added| generation(&added.path));
            let family = parsed(&generations[0].path)?.1.name;
            let complete = generations.iter().enumerate().all(|(index, added)| {
                parsed(&added.path)
                    .is_ok_and(|(_, item)| usize::from(item.patch) == index && item.name == family)
            });
            if !complete {
                return Err(format!(
                    "package {id:04X} must ship every generation from 0 under one name"
                ));
            }
        }
        let patches = match &item.incoming {
            Some(incoming) => incoming.patches.clone(),
            None => packages(&item.folder.join("patches"))?,
        };
        for path in patches {
            let (id, _) = parsed(&path)?;
            let base = self.bases.get(&id).ok_or_else(|| {
                format!(
                    "{} patches package {id:04X}, which isn't installed",
                    name(&path)
                )
            })?;
            let newest = base.generations.last().expect("a base has a generation");
            let next = next_generation(newest)?;
            if name(&path) != next.as_str() {
                return Err(format!(
                    "{}: the next generation of package {id:04X} is {next}",
                    name(&path)
                ));
            }
            if staged.claims.iter().any(|claim| claim.package == id) {
                return Err(format!("it has two patches for package {id:04X}"));
            }
            let original = table(newest)?.rows;
            let patched = table(&path)?;
            if patched.rows.len() != original.len() {
                return Err(format!(
                    "{} changes the entry count of package {id:04X}",
                    name(&path)
                ));
            }
            let changed = (0..)
                .zip(original.iter().zip(&patched.rows))
                .filter(|(_, (before, after))| before != after)
                .map(|(index, _)| TagHash::new(id, index))
                .collect();
            let sidecar = staged.sidecar(&item.id, &path, id, &patched.digest);
            staged.claims.push(Claim {
                owner: item.id.clone(),
                package: id,
                path,
                changed,
                sidecar,
            });
        }
        let mut replaced = staged.claims.iter().flat_map(|claim| &claim.changed);
        if let Some((tag, owner)) = replaced.find_map(|tag| Some((tag, self.bound.get(tag)?))) {
            return Err(format!(
                "entry 0x{:08X} has a required config binding from {owner}",
                tag.0
            ));
        }
        let supplied = |tag: TagHash| {
            staged.packages.contains_key(&tag.pkg_id())
                || staged
                    .claims
                    .iter()
                    .any(|claim| claim.changed.contains(&tag))
        };
        if let Some(site) = item
            .config_bindings
            .iter()
            .find(|site| !supplied(site.tag()))
        {
            return Err(format!(
                "config entry 0x{:08X} must be supplied by this mod",
                site.tag().0
            ));
        }
        staged.actions = config::prepare(install, &item.id, &item.config_bindings, &item.config);
        let files = staged.packages.values().map(Vec::len).sum::<usize>() + staged.claims.len();
        if self.added.len() + self.claims.len() + files > FILE_LIMIT {
            return Err(format!(
                "mods add more than {FILE_LIMIT} package files together"
            ));
        }
        Ok(staged)
    }

    fn commit(&mut self, owner: &str, staged: Staged) {
        for (id, generations) in staged.packages {
            let paths = generations.iter().map(|added| added.path.clone()).collect();
            self.added.extend(generations);
            self.bases.insert(
                id,
                Base {
                    generations: paths,
                    owner: Some(owner.to_owned()),
                },
            );
        }
        for action in &staged.actions {
            self.bound.insert(action.tag(), owner.to_owned());
        }
        self.claims.extend(staged.claims);
        self.actions.extend(staged.actions);
        self.admitted.push(owner.to_owned());
    }

    /// Shows each mod's new packages in the view, and for each patched package one patch,
    /// Parhelion's where it has one so its files read back as installed. The other patches are
    /// read in layers, each serving the entries no later mod changes. Entries the loader merges
    /// are read merged.
    fn compose(self, mut notices: Vec<String>) -> Plan {
        let mut groups = BTreeMap::<u16, Vec<&Claim>>::new();
        for claim in &self.claims {
            groups.entry(claim.package).or_default().push(claim);
        }
        let merged = shared::merge(&self, &groups, &mut notices);
        let mut primary = self
            .added
            .iter()
            .map(|added| added.path.clone())
            .collect::<Vec<_>>();
        let mut patches = Vec::new();
        for (id, group) in groups {
            let winners = group
                .iter()
                .enumerate()
                .flat_map(|(index, claim)| claim.changed.iter().map(move |&tag| (tag, index)))
                .collect::<HashMap<_, _>>();
            let shown = group
                .iter()
                .position(|claim| super::is_family_id(&claim.owner))
                .unwrap_or(0);
            primary.push(group[shown].path.clone());
            for (index, claim) in group.iter().enumerate() {
                for earlier in &group[..index] {
                    let count = claim
                        .changed
                        .iter()
                        .filter(|&tag| {
                            earlier.changed.contains(tag) && !merged.entries.contains(tag)
                        })
                        .count();
                    if count > 0 {
                        notices.push(overlap(claim, earlier, count));
                    }
                }
                let tags = claim
                    .changed
                    .iter()
                    .copied()
                    .filter(|tag| winners[tag] == index)
                    .collect::<Vec<_>>();
                if index != shown && !tags.is_empty() {
                    patches.push(Patch {
                        source: claim.path.clone(),
                        parents: self.bases[&id].generations.clone(),
                        tags,
                    });
                }
            }
        }
        Plan {
            primary,
            patches,
            merged: merged.replacements,
            actions: self.actions,
            notices,
            coexistence: merged.coexistence,
        }
    }
}

/// Says whose version of the entries two mods change the loader serves, where it doesn't merge
/// them.
fn overlap(later: &Claim, earlier: &Claim, count: usize) -> String {
    let entries = if count == 1 { "entry" } else { "entries" };
    format!(
        "{} changes {count} {entries} in package {:04X} that {} also changes. Sunrise uses {}'s, which loads later.",
        later.owner, later.package, earlier.owner, later.owner
    )
}
