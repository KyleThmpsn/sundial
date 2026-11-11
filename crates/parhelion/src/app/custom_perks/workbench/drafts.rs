//! The drafts file beside Custom Perks, where the workbench's open perks wait between sessions.
//! Each write carries every open draft and is checked against the bytes this window last read or
//! wrote, so another window's drafts are never replaced. A window that loses the file to another
//! keeps its own drafts beside it for the rest of the session, and the next start opens both.
use super::*;
use std::time::{Duration, Instant};

/// How long an edit that repeats every frame, such as typing or dragging, waits to be written.
const WAIT: Duration = Duration::from_secs(1);
/// How long a failed write waits before it is tried again.
const RETRY: Duration = Duration::from_secs(3);

/// How the open drafts reach the drafts file.
#[derive(Default)]
pub(super) struct Drafts {
    /// The drafts file's bytes as this window last read or wrote them.
    baseline: Option<Vec<u8>>,
    pub(super) writable: bool,
    /// Why the last draft write failed, or where a window that lost the drafts file keeps its own.
    pub(super) error: Option<String>,
    /// When the last draft write failed. While set the latest drafts exist only in the window,
    /// and autosave tries them again.
    retry: Option<Instant>,
    /// When an edit first left the drafts unwritten. Typing and dragging change the perk every
    /// frame, so their edits are written at most once a second.
    due: Option<Instant>,
    /// Drafts file entries that do not open and could not be kept in a file of their own. Every
    /// write of the drafts file carries them, so autosave never drops them.
    unopened: Vec<serde_json::Value>,
    /// This window's own drafts file, named after it, once another window wrote the shared
    /// drafts file since this one last did. Autosave writes there for the rest of the session.
    set_aside: Option<String>,
}

impl Drafts {
    /// Reads the drafts file beside `library` into `documents`, and returns what the window
    /// reports. Drafts that do not open are kept aside before any write can drop them, and a
    /// file that cannot be read pauses autosave.
    pub(super) fn open(
        &mut self,
        library: &Library,
        documents: &mut Vec<Document>,
    ) -> Option<String> {
        let path = library.root().join("workbench-drafts.json");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let (opened, rejected) = read_drafts(&bytes);
                *documents = opened;
                for document in documents.iter_mut() {
                    if document.origin.is_none() {
                        document.origin = Some(document.recipe.clone());
                    }
                    document.read_baseline();
                }
                self.writable = true;
                // A draft the workbench cannot read is set aside before any write can drop it,
                // so the ones it can read open and autosave carries on.
                let report =
                    (!rejected.is_empty()).then(|| self.keep_rejected(library, &bytes, rejected));
                self.baseline = Some(bytes);
                report
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.writable = true;
                None
            }
            Err(error) => Some(format!(
                "Could not read workbench drafts: {error}. Draft autosave is paused."
            )),
        }
    }

    /// Keeps the drafts file's entries that no longer open in a new file of their own, so an
    /// earlier batch is never replaced, and returns what the window reports. When that fails they
    /// stay in the drafts file and every write carries them. A drafts file that is not a list at
    /// all cannot carry them, so autosave pauses instead and its bytes stay as they are.
    fn keep_rejected(
        &mut self,
        library: &Library,
        bytes: &[u8],
        rejected: Vec<serde_json::Value>,
    ) -> String {
        let whole = serde_json::from_slice::<Vec<serde_json::Value>>(bytes).is_err();
        let copy = if whole {
            Ok(bytes.to_vec())
        } else {
            serde_json::to_vec_pretty(&rejected).map_err(|error| error.to_string())
        };
        let kept = copy.and_then(|copy| library.save_rejected_drafts(&draft_session(), &copy));
        let count = drafts(rejected.len());
        match kept {
            Ok(path) => format!(
                "{count} could not be opened. Kept in {}.",
                path.display().to_string().trim_start_matches(r"\\?\")
            ),
            Err(error) if whole => {
                self.writable = false;
                format!(
                    "The drafts file could not be opened or set aside: {error}. Draft autosave is paused."
                )
            }
            Err(error) => {
                self.unopened = rejected;
                format!(
                    "{count} could not be opened or set aside: {error}. They stay in the drafts file."
                )
            }
        }
    }

    /// Writes the drafts soon after an edit that can repeat every frame, such as typing or
    /// dragging. Any other write, and the host's exit check, takes the edit along.
    pub(super) fn schedule(&mut self) {
        self.due.get_or_insert_with(Instant::now);
    }

    /// Writes `documents` to the drafts file beside `library`, leaving out socket copies nobody
    /// changed.
    pub(super) fn persist(&mut self, library: Option<&Library>, documents: &[Document]) {
        self.due = None;
        if !self.writable {
            return;
        }
        let Some(library) = library else {
            return;
        };
        let documents = documents
            .iter()
            .filter(|document| !document.untouched_copy())
            .collect::<Vec<_>>();
        // Another window wrote the drafts file since this one last did. Taking that file as the
        // baseline would let the next write replace the other window's drafts with these, so
        // this window keeps its own drafts beside it for the rest of the session instead.
        let durable = if let Some(session) = &self.set_aside {
            let kept = set_aside(library, session, &documents, self.baseline.as_deref());
            let durable = kept.is_ok();
            self.error = Some(kept.unwrap_or_else(|error| error));
            durable
        } else {
            self.write(library, &documents)
        };
        // A failed write, such as while another window holds the library, is tried again
        // against the same baseline without waiting for the next edit, and the host asks
        // before exiting while it keeps failing.
        self.retry = (!durable).then(Instant::now);
    }

    /// Writes the shared drafts file, or this window's own once another window wrote the shared
    /// one since, and returns whether the drafts are on disk.
    fn write(&mut self, library: &Library, documents: &[&Document]) -> bool {
        let bytes = match drafts_file(documents, &self.unopened) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error = Some(error);
                return false;
            }
        };
        match library.save_drafts(&bytes, self.baseline.as_deref()) {
            Ok(DraftsWrite::Written) => {
                self.baseline = Some(bytes);
                self.error = None;
                true
            }
            Ok(DraftsWrite::Conflict) => {
                let session = draft_session();
                let kept = set_aside(library, &session, documents, self.baseline.as_deref());
                let durable = kept.is_ok();
                self.error = Some(kept.unwrap_or_else(|error| error));
                self.set_aside = Some(session);
                durable
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    /// Writes edits scheduled a second ago or more, and tries the drafts again after a failed
    /// write, every few seconds while it keeps failing.
    pub(super) fn poll(
        &mut self,
        library: Option<&Library>,
        documents: &[Document],
        ctx: &egui::Context,
    ) {
        if let Some(due) = self.due {
            let waited = due.elapsed();
            if waited >= WAIT {
                self.persist(library, documents);
            } else {
                ctx.request_repaint_after(WAIT - waited);
            }
        }
        let Some(failed) = self.retry else {
            return;
        };
        if failed.elapsed() >= RETRY {
            self.persist(library, documents);
        }
        ctx.request_repaint_after(RETRY);
    }

    /// Writes the drafts when an edit is waiting or the last write left them unsaved, and
    /// returns why edited drafts are still unsaved.
    pub(super) fn unsaved(
        &mut self,
        library: Option<&Library>,
        documents: &[Document],
    ) -> Option<String> {
        if self.retry.is_some() || self.due.is_some() {
            self.persist(library, documents);
        }
        if self.retry.is_some() {
            return Some(format!(
                "Parhelion could not save its perk drafts: {}",
                self.error.as_deref().unwrap_or("the write failed")
            ));
        }
        let edited = documents.iter().any(|document| {
            !document.untouched_copy() && (document.changed() || document.pending_effect.is_some())
        });
        // A library that never opened leaves the workbench editable with nowhere to save, so
        // its drafts are as unsaved as those of a paused autosave.
        if !edited || self.writable {
            None
        } else if library.is_none() {
            Some(
                "Custom Perks could not open, so Parhelion's edited perk drafts are not saved."
                    .into(),
            )
        } else {
            Some(
                "Parhelion's draft autosave is paused, so its edited perk drafts are not saved."
                    .into(),
            )
        }
    }

    /// Opens the drafts other windows set aside after losing the drafts file into `documents`,
    /// folds them into the file, and returns what the window reports. A draft already open
    /// under the same id with other content opens as a copy.
    pub(super) fn adopt_set_aside(
        &mut self,
        library: &Library,
        documents: &mut Vec<Document>,
    ) -> Option<String> {
        if !self.writable {
            return None;
        }
        let found = match library.set_aside_drafts() {
            Ok(found) => found,
            Err(error) => return Some(format!("Could not read set-aside drafts: {error}")),
        };
        if found.is_empty() {
            return None;
        }
        let mut adopted = Vec::new();
        for (path, bytes) in found {
            let (opened, rejected) = read_drafts(&bytes);
            // A file that does not read whole stays where it is for the reader to inspect.
            if !rejected.is_empty() {
                continue;
            }
            for document in opened {
                adopt(documents, document);
            }
            adopted.push((path, bytes));
        }
        self.persist(Some(library), documents);
        if self.set_aside.is_some() || self.error.is_some() {
            return None;
        }
        let mut report = None;
        for (path, bytes) in adopted {
            if let Err(error) = library.remove_set_aside_drafts(&path, &bytes) {
                report = Some(format!("Could not remove {}: {error}", path.display()));
            }
        }
        report
    }

    /// The warning Custom Perks shows about autosave, with its detail.
    pub(super) fn status(&self) -> Option<(&'static str, Option<&str>)> {
        if !self.writable {
            Some(("Draft autosave is paused.", None))
        } else if let (Some(_), Some(note)) = (&self.set_aside, &self.error) {
            Some(("Drafts are kept separately.", Some(note)))
        } else {
            self.error
                .as_deref()
                .map(|error| ("Draft autosave failed.", Some(error)))
        }
    }
}

/// Adds a set-aside `document` to `documents`, unless the same draft is already open. One open
/// under its id with other content keeps it, and the set-aside one opens as a copy.
fn adopt(documents: &mut Vec<Document>, mut document: Document) {
    if document.origin.is_none() {
        document.origin = Some(document.recipe.clone());
    }
    document.read_baseline();
    match documents
        .iter()
        .find(|open| open.recipe.id == document.recipe.id)
    {
        Some(open)
            if open.recipe == document.recipe && open.pending_effect == document.pending_effect =>
        {
            return;
        }
        Some(_) => {
            document.recipe.id = PerkRecipe::new().id;
            document.recipe.name = format!(
                "{} Copy",
                super::library::display_name(&document.recipe.name)
            );
            document.origin = Some(document.recipe.clone());
            document.set_baseline(None);
        }
        None => {}
    }
    documents.push(document);
}

/// "1 draft" or "N drafts".
fn drafts(count: usize) -> String {
    if count == 1 {
        "1 draft".to_owned()
    } else {
        format!("{count} drafts")
    }
}

/// The documents in a drafts file that still open, and the entries that no longer do. A file
/// that is not a list of drafts at all is one rejected entry, so it is set aside whole.
fn read_drafts(bytes: &[u8]) -> (Vec<Document>, Vec<serde_json::Value>) {
    let Ok(entries) = serde_json::from_slice::<Vec<serde_json::Value>>(bytes) else {
        let whole = serde_json::from_slice::<serde_json::Value>(bytes)
            .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(bytes).into()));
        return (Vec::new(), vec![whole]);
    };
    let mut documents = Vec::new();
    let mut rejected = Vec::new();
    for entry in entries {
        match serde_json::from_value::<Document>(entry.clone()) {
            Ok(document) if document.recipe.validate_draft().is_ok() => documents.push(document),
            _ => rejected.push(entry),
        }
    }
    (documents, rejected)
}

/// A name for this window's set-aside drafts that no other window shares.
fn draft_session() -> String {
    let started = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    format!("{started}-{}", std::process::id())
}

/// Writes the drafts this window changed since it last wrote the drafts file into its own file
/// beside it, and returns what the status line says about it. Drafts the window left as it
/// found them stay out, so a stale copy never comes back as a second draft.
fn set_aside(
    library: &Library,
    session: &str,
    documents: &[&Document],
    baseline: Option<&[u8]>,
) -> Result<String, String> {
    let before = baseline
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(bytes).ok())
        .unwrap_or_default();
    let mut changed = Vec::new();
    for document in documents {
        let value = serde_json::to_value(document).map_err(|error| error.to_string())?;
        if !before.contains(&value) {
            changed.push(value);
        }
    }
    let bytes = serde_json::to_vec_pretty(&changed).map_err(|error| error.to_string())?;
    let path = library.save_set_aside_drafts(session, &bytes)?;
    Ok(format!(
        "Another window changed the drafts file. This window's drafts are kept in {} and open with the others on the next start.",
        path.display().to_string().trim_start_matches(r"\\?\")
    ))
}

/// The drafts file's contents: the open drafts, then the entries that do not open and could not
/// be kept elsewhere, as they were.
fn drafts_file(documents: &[&Document], unopened: &[serde_json::Value]) -> Result<Vec<u8>, String> {
    if unopened.is_empty() {
        return serde_json::to_vec_pretty(documents).map_err(|error| error.to_string());
    }
    let mut entries = documents
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.extend_from_slice(unopened);
    serde_json::to_vec_pretty(&entries).map_err(|error| error.to_string())
}
