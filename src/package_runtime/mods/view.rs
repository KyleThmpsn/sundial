//! Cached, read-only filesystem views. The managed scope isolates installer ownership reads.
use super::{
    Refusal, checked_path, config, content, content_files, ensure_readable, files, is_package,
    runtime_directory,
};
use crate::package_runtime::reader;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use tiger_pkg::TagHash;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Managed,
    Effective,
}

/// Read-only package views, beside the mods folder rather than in it, so the loader never reads
/// them. The leading dot keeps them out of listings that skip hidden folders.
const VIEWS: &str = ".sundial-package-views";
const VIEW_PREFIX: &str = "view-";
const VIEW_OWNER: &str = "owner.lock";
const VIEW_GRACE: std::time::Duration = std::time::Duration::from_secs(120);

/// One view per installation and reader scope, shared by its readers. Linking every installed
/// package takes seconds, so a view is rebuilt only when packages or mod inputs change.
static SHARED: Mutex<Vec<Shared>> = Mutex::new(Vec::new());

struct Shared {
    install: PathBuf,
    scope: Scope,
    contents: Contents,
    /// Mods a reader refused for their configuration, which the next view of these contents
    /// leaves out.
    refused: Vec<Refusal>,
    view: Arc<View>,
}

/// Each package and mod file's path, size and write time, and a digest of the manifests,
/// receipts, sidecars and configuration that can change provider selection or payload values.
#[derive(PartialEq, Eq, Serialize)]
struct Contents {
    files: Vec<(PathBuf, u64, Option<SystemTime>)>,
    metadata: Vec<u8>,
}

/// Retain this view until every reader using it closes. The source packages are never edited.
pub struct View {
    /// Held while the view lives, so another opener knows it is in use. Declared first so it
    /// closes before the folder holding it is removed.
    _owner: File,
    directory: tempfile::TempDir,
    packages: PathBuf,
    /// The installation's own packages directory the view projects.
    source: PathBuf,
    layers: Vec<content::Layer>,
    /// Entries the loader serves with the rows several mods add merged.
    merged: Vec<(TagHash, Arc<[u8]>)>,
    actions: Vec<config::Action>,
    /// The mods this view leaves out for their configuration.
    refused: Vec<Refusal>,
    notices: Vec<String>,
}

impl View {
    /// The installation's view, or `None` while it has no visible mod inputs. Readers share a view
    /// until a package or mod file changes. Opening while another reader builds one waits for it.
    pub fn open(install: &Path) -> Result<Option<Arc<Self>>, String> {
        Self::open_scope(install, Scope::Effective)
    }

    pub(crate) fn open_managed(install: &Path) -> Result<Option<Arc<Self>>, String> {
        Self::open_scope(install, Scope::Managed)
    }

    /// A fresh prospective view. It never publishes mod files or changes the installed cache.
    pub(in crate::package_runtime) fn incoming(
        install: &Path,
        incoming: &content::Incoming,
        refused: Vec<Refusal>,
    ) -> Result<Arc<Self>, String> {
        ensure_readable(install)?;
        let plan = content::Plan::read(install, &refused, Some(incoming))?;
        if !plan.coexistence.problems.is_empty() {
            return Err(plan.coexistence.problems.join("\n"));
        }
        let mut additions = content_files(install)?;
        additions.extend(incoming.packages.iter().chain(&incoming.patches).cloned());
        additions.extend(
            incoming
                .packages
                .iter()
                .chain(&incoming.patches)
                .map(|path| path.with_extension(super::merge::sidecar::EXTENSION))
                .filter(|path| path.is_file()),
        );
        let contents = contents(install, &additions)?;
        Self::from_plan(install, plan, &contents, refused, &incoming.retired).map(Arc::new)
    }

    fn open_scope(install: &Path, scope: Scope) -> Result<Option<Arc<Self>>, String> {
        ensure_readable(install)?;
        let additions = if scope == Scope::Managed {
            files(install)?
        } else {
            content_files(install)?
        };
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if additions.is_empty() {
            shared.retain(|entry| entry.install != install || entry.scope != scope);
            return Ok(None);
        }
        let contents = contents(install, &additions)?;
        let mut refused = Vec::new();
        if let Some(entry) = shared.iter().find(|entry| {
            entry.install == install && entry.scope == scope && entry.contents == contents
        }) {
            if entry.view.refused == entry.refused && entry.view.packages.is_dir() {
                return Ok(Some(Arc::clone(&entry.view)));
            }
            refused.clone_from(&entry.refused);
        }
        let view = Self::create(install, &additions, scope, &contents, refused.clone())?;
        let view = Arc::new(view);
        shared.retain(|entry| entry.install != install || entry.scope != scope);
        shared.push(Shared {
            install: install.to_path_buf(),
            scope,
            contents,
            refused,
            view: Arc::clone(&view),
        });
        Ok(Some(view))
    }

    /// Leaves `refusal`'s mod out of the installation's next view, while its mod inputs stay as
    /// they are. A reader that finds the loader refuses a mod's configuration reads again.
    pub(in crate::package_runtime) fn refuse(install: &Path, view: &Arc<Self>, refusal: Refusal) {
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = shared
            .iter_mut()
            .find(|entry| entry.install == install && Arc::ptr_eq(&entry.view, view))
            && !entry.refused.contains(&refusal)
        {
            entry.refused.push(refusal);
        }
    }

    /// What the installation's current view leaves out and why, or `None` before a reader made
    /// one. Diagnostics read this without building a view, which takes seconds.
    pub(super) fn current_notices(install: &Path) -> Result<Option<Vec<String>>, String> {
        let additions = content_files(install)?;
        if additions.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let contents = contents(install, &additions)?;
        let shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(shared
            .iter()
            .find(|entry| {
                entry.install == install
                    && entry.scope == Scope::Effective
                    && entry.contents == contents
            })
            .map(|entry| entry.view.notices.clone()))
    }

    fn create(
        install: &Path,
        additions: &[PathBuf],
        scope: Scope,
        contents: &Contents,
        refused: Vec<Refusal>,
    ) -> Result<Self, String> {
        let plan = match scope {
            Scope::Managed => content::Plan::installed(additions),
            Scope::Effective => content::Plan::read(install, &refused, None)?,
        };
        Self::from_plan(install, plan, contents, refused, &[])
    }

    fn from_plan(
        install: &Path,
        plan: content::Plan,
        contents: &Contents,
        refused: Vec<Refusal>,
        retired: &[String],
    ) -> Result<Self, String> {
        reader::initialize_source_decoder(&install.join("packages"))?;
        // Hard links work only within one volume, so the view sits inside the installation. In
        // the system temp folder on another drive, every installed package was copied instead.
        let root = runtime_directory(install).join(VIEWS);
        checked_path(install, &root)?;
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        remove_stale_views(&root);
        let directory = tempfile::Builder::new()
            .prefix(VIEW_PREFIX)
            .tempdir_in(&root)
            .map_err(|e| e.to_string())?;
        let owner = File::create(directory.path().join(VIEW_OWNER)).map_err(|e| e.to_string())?;
        fs2::FileExt::try_lock_exclusive(&owner).map_err(|e| e.to_string())?;
        let packages = directory.path().join("packages");
        fs::create_dir(&packages).map_err(|e| e.to_string())?;
        link_stock(install, &packages, retired)?;
        for path in &plan.primary {
            link_or_copy(
                path,
                &packages.join(path.file_name().ok_or("Package has no filename")?),
            )?;
        }
        let layers = link_layers(directory.path(), plan.patches)?;
        // Cache identities must include providers that share a filename in different mods, and
        // the mods left out for their configuration.
        let refused_ids = refused
            .iter()
            .map(|refusal| &refusal.id)
            .collect::<Vec<_>>();
        let identity = hex::encode(Sha256::digest(
            serde_json::to_vec(&(contents, refused_ids)).map_err(|e| e.to_string())?,
        ));
        fs::write(packages.join(format!(".sundial-content-{identity}")), [])
            .map_err(|e| e.to_string())?;
        link_decoders(install, directory.path())?;
        Ok(Self {
            _owner: owner,
            directory,
            packages,
            source: install.join("packages"),
            layers,
            merged: plan
                .merged
                .into_iter()
                .map(|(tag, bytes)| (tag, Arc::from(bytes)))
                .collect(),
            actions: plan.actions,
            refused,
            notices: plan.notices,
        })
    }

    pub fn packages(&self) -> &Path {
        &self.packages
    }
    pub fn source(&self) -> &Path {
        &self.source
    }
    pub(crate) fn install(&self) -> &Path {
        self.directory.path()
    }
    pub(in crate::package_runtime) fn layers(&self) -> &[content::Layer] {
        &self.layers
    }
    pub(in crate::package_runtime) fn merged(&self) -> &[(TagHash, Arc<[u8]>)] {
        &self.merged
    }
    /// The entries mods bind, with their configured values, or the first mod the loader refuses
    /// because its values don't fit what they bind.
    pub(in crate::package_runtime) fn configured(
        &self,
        manager: &reader::PackageManager,
    ) -> Result<Vec<(TagHash, Vec<u8>)>, Refusal> {
        config::apply(manager, &self.actions)
    }

    /// Why each mod the loader refuses was left out, and whose version of a shared entry is read.
    pub fn notices(&self) -> &[String] {
        &self.notices
    }
}

fn link_stock(install: &Path, packages: &Path, retired: &[String]) -> Result<(), String> {
    for entry in fs::read_dir(install.join("packages")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if is_package(&path)
            && !retired
                .iter()
                .any(|name| entry.file_name() == name.as_str())
        {
            checked_path(install, &path)?;
            link(&path, &packages.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn link_layers(
    directory: &Path,
    patches: Vec<content::Patch>,
) -> Result<Vec<content::Layer>, String> {
    let mut layers = Vec::new();
    for (index, patch) in patches.into_iter().enumerate() {
        let layer = directory.join(format!("layer-{index}/packages"));
        fs::create_dir_all(&layer).map_err(|e| e.to_string())?;
        for path in patch.parents.iter().chain(std::iter::once(&patch.source)) {
            link_or_copy(
                path,
                &layer.join(path.file_name().ok_or("Package has no filename")?),
            )?;
        }
        layers.push(content::Layer {
            packages: layer,
            tags: patch.tags,
        });
    }
    Ok(layers)
}

fn link_decoders(install: &Path, directory: &Path) -> Result<(), String> {
    // A projected installation retains the source decoder location expected by readers.
    let bin = directory.join("bin/x64");
    fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    for name in ["oo2core_3_win64.dll", "oo2core_9_win64.dll"] {
        let source = install.join("bin/x64").join(name);
        if source.is_file() {
            link_or_copy(&source, &bin.join(name))?;
        }
    }
    Ok(())
}

/// Reads what a view of `install` would project. Directory listings carry sizes and write times
/// on Windows, so this opens no installed package.
fn contents(install: &Path, additions: &[PathBuf]) -> Result<Contents, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(install.join("packages")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if is_package(&path) {
            let metadata = entry.metadata().map_err(|e| e.to_string())?;
            files.push((path, metadata.len(), metadata.modified().ok()));
        }
    }
    for path in additions {
        let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
        files.push((path.clone(), metadata.len(), metadata.modified().ok()));
    }
    files.sort();
    let mut digest = Sha256::new();
    for path in additions.iter().filter(|p| !is_package(p)) {
        // A file the loader can't read leaves its mod out, so the error stands in for its bytes.
        if let Err(error) = digest_file(&mut digest, path) {
            digest.update(error.kind().to_string());
        }
    }
    let metadata = digest.finalize().to_vec();
    Ok(Contents { files, metadata })
}

fn digest_file(digest: &mut Sha256, path: &Path) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    digest.update(file.metadata()?.len().to_le_bytes());
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        digest.update(&buffer[..count]);
    }
}

/// Removes views left behind by a crash, or by a file still open when its reader closed. A view
/// in use holds its owner lock. A new one gets a grace period, because its owner lock is created
/// just after its folder. Removing a view removes only links, never installed packages.
fn remove_stale_views(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let view = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(VIEW_PREFIX))
            && entry.file_type().is_ok_and(|kind| kind.is_dir());
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > VIEW_GRACE);
        if !view || !old {
            continue;
        }
        let unowned = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(entry.path().join(VIEW_OWNER))
        {
            Ok(owner) => fs2::FileExt::try_lock_exclusive(&owner).is_ok(),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        };
        if unowned {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Installed packages are never copied: a copy of every one would take as much space again.
fn link(source: &Path, target: &Path) -> Result<(), String> {
    if target.try_exists().map_err(|e| e.to_string())? {
        return Err("A mod package aliases an installed package file".into());
    }
    fs::hard_link(source, target).map_err(|error| {
        format!(
            "Could not read the installed packages together with the Sunrise mod, because {} could not be linked: {error}. The game's drive must support hard links.",
            source.display()
        )
    })
}

fn link_or_copy(source: &Path, target: &Path) -> Result<(), String> {
    if target.try_exists().map_err(|e| e.to_string())? {
        return Err("A mod package aliases an installed package file".into());
    }
    if fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| e.to_string())?;
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    Ok(())
}
