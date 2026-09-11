//! Coordinates saves across the independently persisted workspace sources.
//!
//! The JSON and SQLite adapters remain separate. This module only owns ordering and rollback when
//! one user action changes both sources.

use std::path::Path;

use super::{
    account_workspace::WorkspaceDocument,
    settings::{SaveJsonError, SaveJsonResult, save_json},
};
use crate::persistence::sqlite_account::SqliteSaveReceipt;

pub(super) struct WorkspaceSaveReceipt {
    pub json: Option<SaveJsonResult>,
    pub sqlite: Option<SqliteSaveReceipt>,
}

#[derive(Debug)]
pub(super) struct WorkspaceSaveError {
    pub message: String,
    pub sqlite_rollback: Option<Result<(), String>>,
}

struct CoordinatedSaveReceipt<J, S> {
    json: Option<J>,
    sqlite: Option<S>,
}

fn coordinate_source_saves<C, J, S>(
    context: &mut C,
    json_changed: bool,
    account_changed: bool,
    save_sqlite: impl FnOnce(&mut C) -> Result<S, String>,
    save_json: impl FnOnce(&mut C) -> Result<J, SaveJsonError>,
    restore_sqlite: impl FnOnce(&mut C, &S) -> Result<(), String>,
) -> Result<CoordinatedSaveReceipt<J, S>, WorkspaceSaveError> {
    let sqlite = if account_changed {
        Some(save_sqlite(context).map_err(|message| WorkspaceSaveError {
            message,
            sqlite_rollback: None,
        })?)
    } else {
        None
    };

    let json = if json_changed {
        match save_json(context) {
            Ok(receipt) => Some(receipt),
            Err(error) => {
                let sqlite_rollback = sqlite
                    .as_ref()
                    .filter(|_| !error.may_have_committed)
                    .map(|receipt| restore_sqlite(context, receipt));
                let mut message = error.message;
                if error.may_have_committed && sqlite.is_some() {
                    message.push_str(" SQLite was already saved and was not rolled back because the JSON outcome is uncertain. Reload and review both sources before continuing.");
                }
                return Err(WorkspaceSaveError {
                    message,
                    sqlite_rollback,
                });
            }
        }
    } else {
        None
    };

    Ok(CoordinatedSaveReceipt { json, sqlite })
}

struct SaveContext<'a> {
    document: &'a mut WorkspaceDocument,
    persisted_document: &'a WorkspaceDocument,
    settings_path: &'a Path,
}

pub(super) fn save_changed_sources(
    document: &mut WorkspaceDocument,
    persisted_document: &WorkspaceDocument,
    settings_path: &Path,
    json_changed: bool,
    account_changed: bool,
) -> Result<WorkspaceSaveReceipt, WorkspaceSaveError> {
    save_changed_sources_with_json(
        document,
        persisted_document,
        settings_path,
        json_changed,
        account_changed,
        save_json,
    )
}

pub(super) fn save_changed_sources_with_json(
    document: &mut WorkspaceDocument,
    persisted_document: &WorkspaceDocument,
    settings_path: &Path,
    json_changed: bool,
    account_changed: bool,
    save_json: impl FnOnce(
        &Path,
        &serde_json::Value,
        &serde_json::Value,
        bool,
    ) -> Result<SaveJsonResult, SaveJsonError>,
) -> Result<WorkspaceSaveReceipt, WorkspaceSaveError> {
    let mut context = SaveContext {
        document,
        persisted_document,
        settings_path,
    };

    let receipt = coordinate_source_saves(
        &mut context,
        json_changed,
        account_changed,
        |context| context.document.save_sqlite(),
        |context| save_context_json(context, save_json),
        |context, receipt| {
            context.document.rollback_sqlite_save(receipt)?;
            context
                .document
                .rebase_account_revision_from(context.persisted_document);
            Ok(())
        },
    )?;

    Ok(WorkspaceSaveReceipt {
        json: receipt.json,
        sqlite: receipt.sqlite,
    })
}

fn save_context_json(
    context: &SaveContext<'_>,
    save_json: impl FnOnce(
        &Path,
        &serde_json::Value,
        &serde_json::Value,
        bool,
    ) -> Result<SaveJsonResult, SaveJsonError>,
) -> Result<SaveJsonResult, SaveJsonError> {
    save_json(
        context.settings_path,
        context.document.json(),
        context.persisted_document.json(),
        context.persisted_document.uses_json_account(),
    )
}

#[cfg(test)]
mod tests {
    use super::coordinate_source_saves;

    #[derive(Default)]
    struct TestContext {
        events: Vec<&'static str>,
        sqlite_fails: bool,
        json_fails: bool,
        rollback_fails: bool,
    }

    #[test]
    fn uncertain_json_outcome_does_not_blindly_roll_back_sqlite() {
        let mut context = TestContext::default();
        let error = coordinate_source_saves(
            &mut context,
            true,
            true,
            |context| {
                context.events.push("save sqlite");
                Ok(())
            },
            |context| {
                context.events.push("save json");
                Err::<(), _>(super::SaveJsonError {
                    message: "verification unavailable".into(),
                    may_have_committed: true,
                })
            },
            |_, _| panic!("must not undo SQLite when JSON may have committed"),
        )
        .err()
        .unwrap();
        assert!(error.sqlite_rollback.is_none());
        assert!(error.message.contains("review both sources"));
        assert_eq!(context.events, ["save sqlite", "save json"]);
    }

    #[test]
    fn verified_json_commit_with_warning_keeps_the_sqlite_commit() {
        let mut context = TestContext::default();
        let receipt = coordinate_source_saves(
            &mut context,
            true,
            true,
            |context| {
                context.events.push("save sqlite");
                Ok(())
            },
            |context| {
                context.events.push("save json");
                Ok("verified with durability warning")
            },
            |_, _| panic!("a verified JSON commit must not trigger rollback"),
        )
        .unwrap();
        assert_eq!(receipt.json, Some("verified with durability warning"));
        assert!(receipt.sqlite.is_some());
    }

    #[test]
    fn sqlite_failure_stops_before_json_save() {
        let mut context = TestContext {
            sqlite_fails: true,
            ..TestContext::default()
        };

        let error = coordinate_source_saves(
            &mut context,
            true,
            true,
            |context| {
                context.events.push("save sqlite");
                Err::<(), _>("sqlite failed".to_owned())
            },
            |context| {
                context.events.push("save json");
                Ok(())
            },
            |context, _receipt| {
                context.events.push("restore sqlite");
                Ok(())
            },
        )
        .err()
        .expect("the injected SQLite failure should be returned");

        assert!(context.sqlite_fails);
        assert_eq!(context.events, ["save sqlite"]);
        assert_eq!(error.message, "sqlite failed");
        assert!(error.sqlite_rollback.is_none());
    }

    #[test]
    fn json_failure_after_sqlite_save_restores_sqlite() {
        let mut context = TestContext {
            json_fails: true,
            ..TestContext::default()
        };

        let error = coordinate_source_saves(
            &mut context,
            true,
            true,
            |context| {
                context.events.push("save sqlite");
                Ok("sqlite receipt")
            },
            |context| {
                context.events.push("save json");
                Err::<(), _>("json failed".into())
            },
            |context, receipt| {
                assert_eq!(*receipt, "sqlite receipt");
                context.events.push("restore sqlite");
                Ok(())
            },
        )
        .err()
        .expect("the injected JSON failure should be returned");

        assert!(context.json_fails);
        assert_eq!(
            context.events,
            ["save sqlite", "save json", "restore sqlite"]
        );
        assert_eq!(error.message, "json failed");
        assert!(matches!(error.sqlite_rollback, Some(Ok(()))));
    }

    #[test]
    fn rollback_failure_is_preserved_for_critical_status() {
        let mut context = TestContext {
            json_fails: true,
            rollback_fails: true,
            ..TestContext::default()
        };

        let error = coordinate_source_saves(
            &mut context,
            true,
            true,
            |context| {
                context.events.push("save sqlite");
                Ok(())
            },
            |context| {
                context.events.push("save json");
                Err::<(), _>("json failed".into())
            },
            |context, _receipt| {
                context.events.push("restore sqlite");
                Err("rollback failed".to_owned())
            },
        )
        .err()
        .expect("the injected JSON failure should be returned");

        assert!(context.rollback_fails);
        assert!(matches!(
            error.sqlite_rollback,
            Some(Err(ref rollback)) if rollback == "rollback failed"
        ));
    }

    #[test]
    fn json_only_save_never_calls_sqlite_operations() {
        let mut context = TestContext::default();

        let receipt = coordinate_source_saves(
            &mut context,
            true,
            false,
            |context| {
                context.events.push("save sqlite");
                Ok(())
            },
            |context| {
                context.events.push("save json");
                Ok("json receipt")
            },
            |context, _receipt| {
                context.events.push("restore sqlite");
                Ok(())
            },
        )
        .expect("the JSON-only save should succeed");

        assert_eq!(context.events, ["save json"]);
        assert_eq!(receipt.json, Some("json receipt"));
        assert!(receipt.sqlite.is_none());
    }
}
