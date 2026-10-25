//! One hashed check of an imported folder per recipe opening, shared by the readers that open
//! with it. The preview and the source card each validate before and after they read, which
//! hashed every payload four times. Within an opening, a check hashes again only when the folder's
//! listing changed since its last hashed check, which a write during a read still does.
use super::GraphReference;
use anyhow::Result;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

/// Advanced by each recipe opening, so each opening hashes its folders once.
static OPENING: AtomicU64 = AtomicU64::new(0);
/// Held through each hashed check, so readers that open together hash a folder once.
static CHECKED: Mutex<Vec<Checked>> = Mutex::new(Vec::new());

#[derive(PartialEq)]
enum Use {
    Item(u32),
    Reusable(String),
}

struct Checked {
    opening: u64,
    reference: GraphReference,
    used: Use,
    listing: Listing,
}

/// Every file under the folders a pin covers, with the length and write time that change when
/// a file is written or replaced.
type Listing = Vec<(PathBuf, u64, Option<SystemTime>)>;

/// Makes the next check of every folder hash it again. A recipe opening calls this.
pub fn forget_opened() {
    OPENING.fetch_add(1, Ordering::Relaxed);
}

impl GraphReference {
    /// [`Self::validate`] for the readers of an opened recipe, which share one hashed check.
    pub fn validate_opened(&self, item: u32) -> Result<()> {
        check(self, Use::Item(item), || self.validate(item))
    }

    /// [`Self::validate_reusable`] for the readers of an opened recipe, which share one hashed
    /// check.
    pub fn validate_reusable_opened(&self, kind: &str) -> Result<()> {
        check(self, Use::Reusable(kind.to_owned()), || {
            self.validate_reusable(kind)
        })
    }
}

fn check(
    reference: &GraphReference,
    used: Use,
    validate: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let opening = OPENING.load(Ordering::Relaxed);
    let mut checked = CHECKED.lock().unwrap_or_else(PoisonError::into_inner);
    // An unreadable or linked file leaves the check to hash every time.
    let before = listing(reference).ok();
    if before.as_ref().is_some_and(|before| {
        checked.iter().any(|kept| {
            kept.opening == opening
                && kept.reference == *reference
                && kept.used == used
                && kept.listing == *before
        })
    }) {
        return Ok(());
    }
    validate()?;
    // Only a listing that held through the hash stands for the files it hashed.
    if let Some(before) = before
        && listing(reference).ok().as_ref() == Some(&before)
    {
        checked.retain(|kept| {
            kept.opening == opening && !(kept.reference == *reference && kept.used == used)
        });
        checked.push(Checked {
            opening,
            reference: reference.clone(),
            used,
            listing: before,
        });
    }
    Ok(())
}

fn listing(reference: &GraphReference) -> io::Result<Listing> {
    let mut files = Vec::new();
    let mut references = vec![reference];
    while let Some(reference) = references.pop() {
        add(&reference.directory, &mut files)?;
        references.extend(&reference.attachments);
    }
    files.sort();
    Ok(files)
}

fn add(directory: &Path, files: &mut Listing) -> io::Result<()> {
    let mut folders = vec![directory.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                // A link's own listing doesn't change when its target does.
                return Err(io::Error::other("Imported folder holds a link"));
            }
            if kind.is_dir() {
                folders.push(entry.path());
                continue;
            }
            let metadata = entry.metadata()?;
            files.push((entry.path(), metadata.len(), metadata.modified().ok()));
        }
    }
    Ok(())
}
