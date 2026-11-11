//! What one build turn keeps for its later work: the digests of the import files and native tags
//! the runtime cache checks, and during a mod family build the packages each pass emitted. A
//! family builds every member again after a review changes it, and most of those inputs and
//! packages come out the same. Nothing here outlives the turn, so the next build hashes and
//! emits everything again.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::SystemTime,
};

use crate::ExtendedOverlayArtifact;
use sundial::package_authoring::mod_content::Importer;

pub(crate) type Hash = [u8; 32];

#[derive(Default)]
struct Store {
    /// File digests by path, kept with the length and modification time they were read at, so a
    /// file rewritten during the turn is read again.
    #[cfg(feature = "d2-model-importer")]
    files: BTreeMap<PathBuf, (u64, Option<SystemTime>, Hash)>,
    /// Native tag payload digests by the source identity of the package set they were read from.
    tags: BTreeMap<(Hash, u32), Hash>,
    packages: Packages,
    /// The runtime cache's pruning, which waits for the turn's last compile.
    prune: Option<Prune>,
}

/// The runtime cache to prune when the turn ends, from the time the turn's first compile opened
/// it, so every fragment a compile of the turn reused or saved stays.
struct Prune {
    root: PathBuf,
    opened: SystemTime,
    run: fn(&Path, SystemTime),
}

/// The packages of the running pass and the pass before it, by the digest of everything their
/// build reads, each with the member that built it. A package of the pass before that its member
/// doesn't ask for again is dropped once that member has built.
#[derive(Default)]
struct Packages {
    /// Whether a family build's passes are running. Other builds keep no packages.
    active: bool,
    current: BTreeMap<Hash, Kept>,
    previous: BTreeMap<Hash, Kept>,
}

struct Kept {
    member: Option<Importer>,
    artifact: ExtendedOverlayArtifact,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn store() -> MutexGuard<'static, Option<Store>> {
    STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts or ends a build turn with nothing kept.
pub(crate) fn clear() {
    *store() = None;
}

/// Leaves pruning the runtime cache at `root` to [`finish`], with `run`, keeping what the turn's
/// compiles touched from the earliest `opened`. False outside a build turn, where the caller
/// prunes at once.
pub(crate) fn defer_prune(root: &Path, opened: SystemTime, run: fn(&Path, SystemTime)) -> bool {
    if !crate::package_profile::holding() {
        return false;
    }
    let mut guard = store();
    let store = guard.get_or_insert_with(Store::default);
    if let Some(prune) = store.prune.as_mut().filter(|prune| prune.root == root) {
        prune.opened = prune.opened.min(opened);
    } else {
        store.prune = Some(Prune {
            root: root.to_owned(),
            opened,
            run,
        });
    }
    true
}

/// Runs what waits for the end of the turn: pruning the runtime cache once every compile of the
/// turn has used it.
pub(crate) fn finish() {
    let prune = store().as_mut().and_then(|store| store.prune.take());
    if let Some(Prune { root, opened, run }) = prune {
        run(&root, opened);
    }
}

/// The digest of the file at `path`, from `read` unless the turn already read the file at its
/// current length and modification time.
#[cfg(feature = "d2-model-importer")]
pub(crate) fn file_digest(path: &Path, read: impl FnOnce(&Path) -> Option<Hash>) -> Option<Hash> {
    let metadata = std::fs::metadata(path).ok()?;
    let stamp = (metadata.len(), metadata.modified().ok());
    if let Some((length, modified, digest)) = store()
        .as_ref()
        .and_then(|store| store.files.get(path).copied())
        && (length, modified) == stamp
    {
        return Some(digest);
    }
    let digest = read(path)?;
    store()
        .get_or_insert_with(Store::default)
        .files
        .insert(path.to_owned(), (stamp.0, stamp.1, digest));
    Some(digest)
}

/// The payload digest of `tag` in the package set `source` identifies, if the turn read it.
pub(crate) fn tag_digest(source: Hash, tag: u32) -> Option<Hash> {
    store()
        .as_ref()
        .and_then(|store| store.tags.get(&(source, tag)).copied())
}

pub(crate) fn keep_tag_digest(source: Hash, tag: u32, digest: Hash) {
    store()
        .get_or_insert_with(Store::default)
        .tags
        .insert((source, tag), digest);
}

/// A family build's passes. Each pass keeps the packages it emits for the next one, and the
/// packages end with the passes, including when a pass fails.
pub(crate) struct Passes(());

impl Passes {
    pub(crate) fn begin() -> Self {
        let mut guard = store();
        let packages = &mut guard.get_or_insert_with(Store::default).packages;
        *packages = Packages {
            active: true,
            ..Packages::default()
        };
        Self(())
    }

    /// Starts the next pass. The packages the pass before emitted or reused stay available.
    pub(crate) fn next(&self) {
        if let Some(store) = store().as_mut() {
            let packages = &mut store.packages;
            packages.previous = std::mem::take(&mut packages.current);
        }
    }

    /// Drops the packages `member` built in the pass before and didn't ask for again in this one.
    pub(crate) fn built(&self, member: Option<Importer>) {
        if let Some(store) = store().as_mut() {
            store
                .packages
                .previous
                .retain(|_, kept| kept.member != member);
        }
    }
}

impl Drop for Passes {
    fn drop(&mut self) {
        if let Some(store) = store().as_mut() {
            store.packages = Packages::default();
        }
    }
}

/// Whether the running build keeps its packages for a later pass.
pub(crate) fn keeps_packages() -> bool {
    store().as_ref().is_some_and(|store| store.packages.active)
}

/// The package an earlier pass emitted from the inputs `key` digests, kept for the next pass too.
pub(crate) fn package(key: &Hash) -> Option<ExtendedOverlayArtifact> {
    let mut guard = store();
    let packages = &mut guard.as_mut()?.packages;
    if let Some(kept) = packages.current.get(key) {
        return Some(kept.artifact.clone());
    }
    let kept = packages.previous.remove(key)?;
    let artifact = kept.artifact.clone();
    packages.current.insert(*key, kept);
    Some(artifact)
}

/// Keeps a package the member now compiling built, for the next pass.
pub(crate) fn keep_package(key: Hash, artifact: &ExtendedOverlayArtifact) {
    if let Some(store) = store().as_mut()
        && store.packages.active
    {
        store.packages.current.insert(
            key,
            Kept {
                member: crate::package_profile::member(),
                artifact: artifact.clone(),
            },
        );
    }
}

/// Whether a reused package is built again and compared, set by
/// `PARHELION_VERIFY_PACKAGE_REUSE`. A difference stops the build, since it means the reuse key
/// misses an input.
pub(crate) fn verifies_packages() -> bool {
    std::env::var_os("PARHELION_VERIFY_PACKAGE_REUSE").is_some()
}
