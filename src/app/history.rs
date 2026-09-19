//! Explicit edit commits and undo/redo for workspace documents.
use super::{DOCUMENT_HISTORY_LIMIT, DocumentHistoryEntry, SundialApp, equipment};

impl SundialApp {
    /// Called after a successful mutation, never while rendering an unchanged frame.
    pub(super) fn record_edit(&mut self, label: impl Into<String>) -> bool {
        self.commit_edit(label.into(), false)
    }

    /// Progression tables keep their local edit baselines until save, undo or reload.
    pub(super) fn record_progression_edit(&mut self, label: impl Into<String>) -> bool {
        self.commit_edit(label.into(), true)
    }

    pub(super) fn report_edit(&mut self, label: impl Into<String>) {
        let label = label.into();
        self.record_edit(label.clone());
        self.set_status(format!("{label}. Click Save to write it"), false);
    }

    fn commit_edit(&mut self, label: String, preserve_progression_edits: bool) -> bool {
        let previous = self
            .edit_baseline
            .as_ref()
            .unwrap_or(&self.persisted_document);
        if self.document == *previous {
            return false;
        }
        let previous = self
            .edit_baseline
            .replace(self.document.clone())
            .unwrap_or_else(|| self.persisted_document.clone());
        self.undo_history.push(DocumentHistoryEntry {
            document: previous,
            label,
        });
        if self.undo_history.len() > DOCUMENT_HISTORY_LIMIT {
            self.undo_history.remove(0);
        }
        self.redo_history.clear();
        self.dirty = self.document != self.persisted_document;
        self.refresh_edited_document(preserve_progression_edits);
        true
    }

    fn refresh_edited_document(&mut self, preserve_progression_edits: bool) {
        if preserve_progression_edits {
            self.progression_ui.invalidate_cache();
        } else {
            self.progression_ui.invalidate_document();
        }
        let selected = self
            .selected_character
            .min(self.character_count().saturating_sub(1));
        if selected != self.selected_character {
            self.selected_character = selected;
            self.collections_ui.reset_navigation();
        }
        self.document_repaint_pending = true;
    }

    /// Saving changes persistence bookkeeping without adding a user edit to history.
    pub(super) fn mark_document_saved(&mut self) {
        self.persisted_document = self.document.clone();
        self.edit_baseline = None;
        self.dirty = false;
        self.progression_ui.mark_saved();
    }

    pub(super) fn reset_document_history(&mut self) {
        self.edit_baseline = None;
        self.undo_history.clear();
        self.redo_history.clear();
        self.refresh_edited_document(false);
    }

    pub(super) fn restore_history_document(&mut self, mut entry: DocumentHistoryEntry, undo: bool) {
        let current = DocumentHistoryEntry {
            document: self.document.clone(),
            label: entry.label.clone(),
        };
        if undo {
            self.redo_history.push(current);
        } else {
            self.undo_history.push(current);
        }
        entry.document.rebase_account_revision_from(&self.document);
        self.document = entry.document;
        self.edit_baseline = Some(self.document.clone());
        self.dirty = self.document != self.persisted_document;
        self.refresh_edited_document(false);
        self.clear_picker_state();
        self.sync_raw_json();
        self.armor_stats_adjuster = equipment::ArmorStatsAdjusterState::default();
        self.set_status(
            format!("{}: {}", if undo { "Undid" } else { "Redid" }, entry.label),
            false,
        );
    }

    pub(super) fn undo(&mut self) {
        if !self.json_editor.has_unapplied_changes()
            && let Some(entry) = self.undo_history.pop()
        {
            self.restore_history_document(entry, true);
        }
    }

    pub(super) fn redo(&mut self) {
        if !self.json_editor.has_unapplied_changes()
            && let Some(entry) = self.redo_history.pop()
        {
            self.restore_history_document(entry, false);
        }
    }
}
