//! Undo/redo history for workspace documents.
use super::account_workspace::WorkspaceDocument;
use super::{DOCUMENT_HISTORY_LIMIT, DocumentHistoryEntry, SundialApp, equipment};

impl SundialApp {
    pub(super) fn record_document_change(&mut self, previous: WorkspaceDocument) {
        if self.document == previous {
            self.suppress_history_record = false;
            return;
        }
        if self.suppress_history_record {
            self.suppress_history_record = false;
            return;
        }
        let label = if self.status.trim().is_empty() {
            "Settings change".to_owned()
        } else {
            self.status
                .split(". Click Save")
                .next()
                .unwrap_or(&self.status)
                .trim()
                .to_owned()
        };
        self.undo_history.push(DocumentHistoryEntry {
            document: previous,
            label,
        });
        if self.undo_history.len() > DOCUMENT_HISTORY_LIMIT {
            self.undo_history.remove(0);
        }
        self.redo_history.clear();
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
        self.dirty = self.document != self.persisted_document;
        self.progression_ui.invalidate_document();
        self.clear_picker_state();
        self.sync_raw_json();
        self.armor_stats_adjuster = equipment::ArmorStatsAdjusterState::default();
        self.suppress_history_record = true;
        self.set_status(
            format!("{}: {}", if undo { "Undid" } else { "Redid" }, entry.label),
            false,
        );
    }

    pub(super) fn undo(&mut self) {
        if self.json_editor.has_unapplied_changes() {
            return;
        }
        if let Some(entry) = self.undo_history.pop() {
            self.restore_history_document(entry, true);
        }
    }

    pub(super) fn redo(&mut self) {
        if self.json_editor.has_unapplied_changes() {
            return;
        }
        if let Some(entry) = self.redo_history.pop() {
            self.restore_history_document(entry, false);
        }
    }
}
