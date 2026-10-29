//! One stored copy of each large import payload, shared by hard links.
//!
//! Every conversion writes a folder of its own, so reimports, and items made from the same
//! source art, repeat the same textures, audio and source records. Once a conversion is
//! finished, `share` moves each of its large payloads into the store, named by its SHA-256, and
//! leaves a hard link to the stored file in its place. A conversion still holds every file it
//! names, so content pins, copies, previews and builds read it as before, and removing a
//! conversion removes only its links. `collect` deletes stored files no conversion links any
//! more.
//!
//! A shared payload is never rewritten in place, since every conversion linking it would change
//! with it. A writer replaces such a file by renaming a new one over it. JSON manifests are
//! rewritten in place (`io::write_json`), so they are never shared.
//!
//! The store also remembers what a deterministic conversion made of an input, so a repeated
//! input skips the work. A remembered result lasts while some conversion links it. A conversion
//! whose result is a whole folder remembers where that folder lies instead, and a conversion of
//! the same input checks that the folder still holds it before copying it.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// The store, inside the data folder's `imports` folder, so that links stay on its volume.
pub const FOLDER: &str = ".shared";
/// Written in a folder once `share` has finished with it.
pub const MARKER: &str = ".shared";
/// Smaller files cost more in links than they save. In a library of imports, files of 64 KiB
/// and up hold nearly all of the repeated bytes.
pub const SMALLEST: u64 = 64 * 1024;

/// What `share` or `collect` did. A file it could not handle stays as it was.
#[derive(Debug, Default)]
pub struct Shared {
    /// Payloads now linked to a stored copy, or stored files deleted.
    pub files: usize,
    /// The bytes those files no longer hold on disk.
    pub bytes: u64,
    pub errors: Vec<String>,
}

impl Shared {
    pub fn add(&mut self, other: Shared) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.errors.extend(other.errors);
    }
}

/// Whether `share` has finished with `folder`.
pub fn is_shared(folder: &Path) -> bool {
    folder.join(MARKER).is_file()
}

/// Shares the large payloads below `folder` with the store of `models`, then marks `folder`
/// finished unless a payload failed. JSON, links and folders `descend` turns down are left out.
pub fn share(models: &Path, folder: &Path, descend: impl Fn(&Path) -> bool) -> Shared {
    let files = models.join(FOLDER).join("files");
    let mut shared = Shared::default();
    let mut pending = vec![folder.to_owned()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                shared
                    .errors
                    .push(format!("Could not list {}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    shared
                        .errors
                        .push(format!("Could not list {}: {error}", directory.display()));
                    continue;
                }
            };
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if redirected(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                if descend(&path) {
                    pending.push(path);
                }
                continue;
            }
            let json = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
            if !metadata.is_file() || metadata.len() < SMALLEST || json {
                continue;
            }
            match share_file(&files, &path, metadata.len()) {
                Ok(true) => {
                    shared.files += 1;
                    shared.bytes += metadata.len();
                }
                Ok(false) => {}
                Err(error) => shared
                    .errors
                    .push(format!("Could not share {}: {error}", path.display())),
            }
        }
    }
    if shared.errors.is_empty()
        && let Err(error) = fs::write(folder.join(MARKER), b"")
    {
        shared
            .errors
            .push(format!("Could not mark {}: {error}", folder.display()));
    }
    shared
}

/// Links `path` to the stored copy of its content, storing it first when none exists. Returns
/// whether `path` now shares an earlier stored copy.
fn share_file(files: &Path, path: &Path, size: u64) -> io::Result<bool> {
    // A file with other links is already shared, or linked by something else.
    if links(path)? > 1 {
        return Ok(false);
    }
    let digest = digest_file(path)?;
    // A stored copy at the file system's link limit, 1,024 names on NTFS, is full. Content that
    // many conversions repeat, such as a common dye texture, continues in a further copy.
    for copy in 0..COPIES {
        let stored = stored_copy(files, &digest, copy);
        if let Some(shared) = share_with(&stored, &digest, path, size)? {
            return Ok(shared);
        }
    }
    Ok(false)
}

/// The most copies of one content the store keeps, each linked by as many names as the file
/// system allows.
const COPIES: usize = 256;

/// Links `path` to `stored`, or stores it there when that copy is missing. Returns whether `path`
/// now shares an earlier copy, or `None` when `stored` is full.
fn share_with(stored: &Path, digest: &str, path: &Path, size: u64) -> io::Result<Option<bool>> {
    // Each pass resolves one race with another sharer or with `collect`.
    for _ in 0..4 {
        match fs::symlink_metadata(stored) {
            Ok(metadata) => {
                if metadata.len() != size || digest_file(stored)? != digest {
                    // Something wrote the stored copy in place. Linking to it would spread the
                    // change, so it leaves the store, and this payload is stored instead.
                    fs::remove_file(stored)?;
                    continue;
                }
                let temporary = temporary(path);
                match fs::hard_link(stored, &temporary) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) if full(&error) => return Ok(None),
                    // The payload stays a file of its own.
                    Err(error) => return Err(error),
                }
                return match fs::rename(&temporary, path) {
                    Ok(()) => Ok(Some(true)),
                    Err(error) => {
                        let _ = fs::remove_file(&temporary);
                        Err(error)
                    }
                };
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = stored.parent() {
                    fs::create_dir_all(parent)?;
                }
                match fs::hard_link(path, stored) {
                    Ok(()) => return Ok(Some(false)),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Some(false))
}

/// Whether a link failed because its file has as many names as the file system allows.
fn full(error: &io::Error) -> bool {
    // Windows reports ERROR_TOO_MANY_LINKS.
    error.kind() == io::ErrorKind::TooManyLinks
        || (cfg!(windows) && error.raw_os_error() == Some(1142))
}

/// Deletes the stored files of `models` that no conversion links, and the remembered results
/// whose stored file is gone. `progress` hears how many stored files are checked, of how many.
pub fn collect(models: &Path, progress: &mut dyn FnMut(usize, usize)) -> Shared {
    let root = models.join(FOLDER);
    let mut collected = Shared::default();
    let files = files_below(&root.join("files"), 1, &mut collected.errors);
    let total = files.len();
    for (checked, path) in files.into_iter().enumerate() {
        progress(checked, total);
        match (fs::symlink_metadata(&path), links(&path)) {
            (Ok(metadata), Ok(1)) => match fs::remove_file(&path) {
                Ok(()) => {
                    collected.files += 1;
                    collected.bytes += metadata.len();
                }
                Err(error) => collected
                    .errors
                    .push(format!("Could not remove {}: {error}", path.display())),
            },
            (Ok(_), Ok(_)) => {}
            (Err(error), _) | (_, Err(error)) => collected
                .errors
                .push(format!("Could not check {}: {error}", path.display())),
        }
    }
    for key in files_below(&root.join("keys"), 1, &mut collected.errors) {
        let found = fs::read_to_string(&key)
            .ok()
            .and_then(|digest| {
                valid(digest.trim()).then(|| stored(&root.join("files"), digest.trim()))
            })
            .is_some_and(|stored| stored.is_file());
        if !found {
            let _ = fs::remove_file(&key);
        }
    }
    for pointer in files_below(&root.join(FOLDERS), 1, &mut collected.errors) {
        if !pointed(&root, &pointer).is_some_and(|folder| folder.is_dir()) {
            let _ = fs::remove_file(&pointer);
        }
    }
    collected
}

/// The files inside folders `depth` levels below `directory`, such as `files/<aa>/<digest>` for
/// a depth of 1.
fn files_below(directory: &Path, depth: usize, errors: &mut Vec<String>) -> Vec<PathBuf> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            errors.push(format!("Could not list {}: {error}", directory.display()));
            return Vec::new();
        }
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if redirected(&metadata) {
            continue;
        }
        if depth > 0 && metadata.is_dir() {
            files.extend(files_below(&path, depth - 1, errors));
        } else if depth == 0 && metadata.is_file() {
            files.push(path);
        }
    }
    files
}

/// The store the importers remember results in, set by the application. Without one, every
/// conversion runs in full.
static REMEMBERED: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Lets conversions remember their results in the store of `models`.
pub fn open(models: &Path) {
    if let Ok(mut store) = REMEMBERED.write() {
        *store = Some(models.join(FOLDER));
    }
}

/// Moves the store an earlier version kept in `former`, the folder that held conversions then,
/// into `root`. Stored files keep every link, since a rename moves only their names, and folder
/// notes relative to `former` stop matching and go at the next collection. A store already in
/// `root` stays where it is.
pub fn relocate(former: &Path, root: &Path) {
    let (from, to) = (former.join(FOLDER), root.join(FOLDER));
    if from.is_dir() && !to.exists() && fs::create_dir_all(root).is_ok() {
        let _ = fs::rename(from, to);
    }
}

/// The store conversions remember their results in, if the application set one.
fn remembered() -> Option<PathBuf> {
    REMEMBERED.read().ok()?.clone()
}

/// What `recipe` made of `input` before, if the store still holds it.
pub fn recall(recipe: &str, input: &[u8]) -> Option<Vec<u8>> {
    let root = remembered()?;
    let digest = fs::read_to_string(key(&root, "keys", recipe, input)?).ok()?;
    let digest = digest.trim();
    if !valid(digest) {
        return None;
    }
    let bytes = fs::read(stored(&root.join("files"), digest)).ok()?;
    // A stored copy written in place no longer matches its name.
    (hex::encode(Sha256::digest(&bytes)) == digest).then_some(bytes)
}

/// Keeps `output` as what `recipe` makes of `input`. A failure only costs a later conversion
/// its shortcut.
pub fn remember(recipe: &str, input: &[u8], output: &[u8]) {
    let Some(root) = remembered() else {
        return;
    };
    let result = (|| -> io::Result<()> {
        let key = key(&root, "keys", recipe, input).ok_or(io::ErrorKind::InvalidInput)?;
        let digest = hex::encode(Sha256::digest(output));
        let stored = stored(&root.join("files"), &digest);
        let folder = stored.parent().ok_or(io::ErrorKind::NotFound)?;
        if !stored.is_file() {
            fs::create_dir_all(folder)?;
            let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
            temporary.write_all(output)?;
            if let Err(error) = temporary.persist_noclobber(&stored)
                && error.error.kind() != io::ErrorKind::AlreadyExists
            {
                return Err(error.error);
            }
        }
        let folder = key.parent().ok_or(io::ErrorKind::NotFound)?;
        fs::create_dir_all(folder)?;
        let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
        temporary.write_all(digest.as_bytes())?;
        temporary.persist(&key).map_err(|error| error.error)?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Conversion result not kept: {error}");
    }
}

/// Where what `recipe` made of `input` is noted in `section`: the stored digest of its result in
/// `keys`, or the folder that holds it in [`FOLDERS`]. Recipe names are plain folder names, such
/// as `ffmpeg-pcm-1`.
fn key(root: &Path, section: &str, recipe: &str, input: &[u8]) -> Option<PathBuf> {
    let plain = !recipe.is_empty()
        && recipe
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    plain.then(|| {
        root.join(section)
            .join(recipe)
            .join(hex::encode(Sha256::digest(input)))
    })
}

/// Where the store notes the folder each folder result lies in.
const FOLDERS: &str = "folders";

/// Folder results that conversions in this process are making, by where they are noted. A
/// second conversion of the same input waits for the first instead of repeating it.
static MAKING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
static MADE: Condvar = Condvar::new();

/// What [`claim`] found for an input whose conversion makes a folder.
pub enum Folder {
    /// A folder an earlier conversion made, which still holds the result.
    Made(PathBuf),
    /// No folder holds the result. The caller makes it, while other conversions of the input in
    /// this process wait for the claim.
    Make(Claim),
}

/// Held while a conversion makes a folder result. Dropping it lets waiting conversions go on.
pub struct Claim(Option<(PathBuf, PathBuf)>);

impl Claim {
    /// Notes `folder` as the claimed result, for waiting conversions and later imports.
    pub fn made(self, folder: &Path) {
        if let Some((root, pointer)) = &self.0 {
            point(root, pointer, folder);
        }
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some((_, pointer)) = self.0.take() {
            MAKING
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&pointer);
            MADE.notify_all();
        }
    }
}

/// The folder an earlier conversion made of `input` with `recipe`, when `holds` confirms that it
/// still holds the result. Folders move and are removed, so a noted folder is only a hint. While
/// another conversion in this process makes the same result, `waiting` hears it once and the call
/// waits for it. Without the store, every conversion makes its own.
pub fn claim(
    recipe: &str,
    input: &[u8],
    holds: impl Fn(&Path) -> bool,
    waiting: impl FnOnce(),
) -> anyhow::Result<Folder> {
    let Some((root, pointer)) = remembered()
        .and_then(|root| key(&root, FOLDERS, recipe, input).map(|pointer| (root, pointer)))
    else {
        return Ok(Folder::Make(Claim(None)));
    };
    let mut waiting = Some(waiting);
    let mut making = MAKING.lock().unwrap_or_else(PoisonError::into_inner);
    while making.contains(&pointer) {
        if let Some(waiting) = waiting.take() {
            waiting();
        }
        making = MADE
            .wait_timeout(making, Duration::from_millis(250))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
        crate::cancellation::check()?;
    }
    if let Some(folder) = pointed(&root, &pointer).filter(|folder| holds(folder)) {
        return Ok(Folder::Made(folder));
    }
    making.insert(pointer.clone());
    Ok(Folder::Make(Claim(Some((root, pointer)))))
}

/// Notes `folder` as what `recipe` made of `input`, such as a claimed result that has moved to
/// its final place.
pub fn remember_folder(recipe: &str, input: &[u8], folder: &Path) {
    if let Some(root) = remembered()
        && let Some(pointer) = key(&root, FOLDERS, recipe, input)
    {
        point(&root, &pointer, folder);
    }
}

/// Writes `folder` at `pointer`, relative to the models folder that holds the store `root` when
/// it lies inside it. A failure only costs a later conversion its shortcut.
fn point(root: &Path, pointer: &Path, folder: &Path) {
    let result = (|| -> io::Result<()> {
        let folder = folder.canonicalize()?;
        let models = root
            .parent()
            .ok_or(io::ErrorKind::NotFound)?
            .canonicalize()?;
        let noted = folder.strip_prefix(&models).unwrap_or(&folder);
        let parent = pointer.parent().ok_or(io::ErrorKind::NotFound)?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(noted.to_string_lossy().as_bytes())?;
        temporary.persist(pointer).map_err(|error| error.error)?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Conversion folder not noted: {error}");
    }
}

/// The folder noted at `pointer` in the store `root`.
fn pointed(root: &Path, pointer: &Path) -> Option<PathBuf> {
    let noted = PathBuf::from(fs::read_to_string(pointer).ok()?.trim());
    if noted.as_os_str().is_empty() {
        return None;
    }
    Some(if noted.is_relative() {
        root.parent()?.join(noted)
    } else {
        noted
    })
}

/// The stored copy of the content with `digest`, spread over 256 folders.
fn stored(files: &Path, digest: &str) -> PathBuf {
    files.join(&digest[..2]).join(digest)
}

/// Copy `copy` of the content with `digest`: the stored copy itself, then `<digest>.<copy>` once
/// earlier copies are full.
fn stored_copy(files: &Path, digest: &str, copy: usize) -> PathBuf {
    if copy == 0 {
        stored(files, digest)
    } else {
        files.join(&digest[..2]).join(format!("{digest}.{copy}"))
    }
}

fn valid(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// A free name beside `path` for a link that is then renamed over it.
fn temporary(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(
        ".{name}.{}-{}.link",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// How many names the file at `path` has.
#[cfg(windows)]
pub fn links(path: &Path) -> io::Result<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };
    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle stays open for the call and `information` is a valid output buffer.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }?;
    Ok(u64::from(information.nNumberOfLinks))
}

/// How many names the file at `path` has.
#[cfg(not(windows))]
pub fn links(path: &Path) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)?.nlink())
}

/// A symbolic link, junction or other reparse point.
fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}
