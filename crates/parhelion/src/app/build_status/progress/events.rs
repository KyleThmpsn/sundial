// Failure cases: an active or failed operation is marked finished, advancing to a
// new substep leaves the previous one active, finish events add duplicate rows,
// duplicate notifications undo completion, rollback marks failed work successful,
// or completion wording changes an asset or recipe name.
use super::*;
use std::borrow::Cow;

pub(super) struct Entry {
    label: String,
    item: Option<String>,
    completed: usize,
    total: usize,
    item_is_operation: bool,
    finished: bool,
    phase_finished: bool,
}

impl Entry {
    fn message(&self) -> String {
        let label = if self.phase_finished {
            finished_label(&self.label)
        } else {
            Cow::Borrowed(self.label.as_str())
        };
        let item = self.item.as_deref().map(|item| {
            if self.finished && self.item_is_operation {
                finished_label(item)
            } else {
                Cow::Borrowed(item)
            }
        });
        message(&label, item.as_deref(), self.completed, self.total)
    }
}

/// Only generated operation labels are inflected. Asset and recipe names stay verbatim.
fn finished_label(label: &str) -> Cow<'_, str> {
    let (verb, rest) = label.split_once(' ').unwrap_or((label, ""));
    let past = match verb.trim_end_matches(['.', '…']) {
        "Inspecting" => "Inspected",
        "Loading" => "Loaded",
        "Checking" => "Checked",
        "Comparing" => "Compared",
        "Rechecking" => "Rechecked",
        "Preparing" => "Prepared",
        "Recording" => "Recorded",
        "Compiling" => "Compiled",
        "Resolving" => "Resolved",
        "Planning" => "Planned",
        "Reading" => "Read",
        "Adding" => "Added",
        "Building" => "Built",
        "Linking" => "Linked",
        "Encoding" => "Encoded",
        "Finalizing" => "Finalized",
        "Validating" => "Validated",
        "Verifying" => "Verified",
        "Authoring" => "Authored",
        "Reusing" => "Reused",
        "Writing" => "Wrote",
        "Staging" => "Staged",
        "Backing" => "Backed",
        "Applying" => "Applied",
        "Copying" => "Copied",
        "Installing" => "Installed",
        "Updating" => "Updated",
        "Refreshing" => "Refreshed",
        "Finishing" => "Finished",
        "Restoring" => "Restored",
        "Finding" => "Found",
        "Confirming" => "Confirmed",
        _ => return Cow::Borrowed(label),
    };
    Cow::Owned(if rest.is_empty() {
        past.to_owned()
    } else {
        format!("{past} {rest}")
    })
}

impl Activity {
    /// Advancing to the next operation completes the preceding log entry. A repeated
    /// operation completes when its counter advances. Errors never advance this state,
    /// and rollback explicitly leaves the failed operation unfinished.
    pub(in crate::app) fn progress(
        &mut self,
        elapsed: Duration,
        label: &str,
        item: Option<&str>,
        counts: (usize, usize),
        item_is_operation: bool,
        finish_previous: bool,
    ) -> Option<String> {
        let (completed, total) = counts;
        let mut entry = Entry {
            label: label.to_owned(),
            item: item.map(str::to_owned),
            completed,
            total,
            item_is_operation,
            finished: total > 0 && completed == total,
            phase_finished: false,
        };
        if let Some(mut previous) = self.operation.take() {
            let same = previous.label == label
                && previous.item.as_deref() == item
                && previous.total == total
                && previous.completed <= completed;
            if same {
                entry.finished |= previous.finished || completed > previous.completed;
                entry.phase_finished = previous.phase_finished;
                let changed =
                    previous.completed != completed || previous.finished != entry.finished;
                let text = entry.message();
                self.update_last(elapsed, text.clone());
                self.operation = Some(entry);
                return changed.then_some(text);
            }
            let phase_changed = previous.label != label;
            if finish_previous && (!previous.finished || phase_changed) {
                previous.finished = true;
                previous.phase_finished |= phase_changed;
                if previous.label == label && previous.total == total {
                    previous.completed = previous.completed.max(completed);
                }
                self.update_last(elapsed, previous.message());
            }
        }
        let text = entry.message();
        self.push(elapsed, text.clone());
        self.operation = Some(entry);
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn activity_tracks_operation_completion_and_keeps_failures_active() {
        let mut activity = Activity::default();
        let elapsed = Duration::from_secs(1);
        activity.progress(
            elapsed,
            "Building package payloads",
            Some("Reading Source Package: a.pkg"),
            (0, 2),
            true,
            true,
        );
        assert!(
            activity
                .lines
                .back()
                .unwrap()
                .contains("Reading Source Package")
        );
        activity.progress(
            elapsed,
            "Building package payloads",
            Some("Verifying Payloads: a.pkg"),
            (0, 2),
            true,
            true,
        );
        assert!(activity.lines[0].contains("Read Source Package"));
        activity.progress(
            elapsed,
            "Building package payloads",
            Some("Verifying Payloads: a.pkg"),
            (1, 2),
            true,
            true,
        );
        assert_eq!(activity.lines.len(), 2);
        assert!(activity.lines.back().unwrap().contains("Verified Payloads"));
        assert!(activity.lines.iter().all(|line| {
            line.contains("Building package payloads") && !line.contains("Built package payloads")
        }));
        activity.progress(
            elapsed,
            "Building package payloads",
            Some("Verifying Payloads: a.pkg"),
            (1, 2),
            true,
            true,
        );
        assert!(activity.lines.back().unwrap().contains("Verified Payloads"));
        activity.progress(
            elapsed,
            "Building package payloads",
            Some("Verifying Payloads: b.pkg"),
            (1, 2),
            true,
            true,
        );
        activity.push(elapsed, "Build failed".into());
        assert!(activity.lines[2].contains("Verifying Payloads: b.pkg"));
    }

    #[test]
    fn activity_preserves_names_and_does_not_complete_work_on_rollback() {
        let mut activity = Activity::default();
        let elapsed = Duration::from_secs(1);
        activity.progress(
            elapsed,
            "Checking donor overrides",
            Some("Loading Storm"),
            (0, 1),
            false,
            true,
        );
        activity.progress(
            elapsed,
            "Checking donor overrides",
            Some("Loading Storm"),
            (1, 1),
            false,
            true,
        );
        assert!(activity.lines[0].contains("Checking donor overrides"));
        assert!(activity.lines[0].ends_with(": Loading Storm"));
        activity.progress(
            elapsed,
            "Verifying Installed Packages",
            Some("a.pkg"),
            (0, 2),
            false,
            true,
        );
        activity.progress(
            elapsed,
            "Restoring the Previous Installation",
            None,
            (0, 0),
            false,
            false,
        );
        assert!(activity.lines[1].contains("Verifying Installed Packages"));
        assert!(!activity.lines[1].contains("Verified Installed Packages"));
    }
}
