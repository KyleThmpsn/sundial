//! What the background workers send the app: catalog loads, build progress, the actions
//! waiting on a confirmation and their diagnostics.
use super::*;

pub(super) enum CatalogEvent {
    Progress(CatalogLoadProgress),
    Finished(Box<Result<InvestmentCatalog, String>>),
}

#[derive(Clone, Debug)]
pub(super) struct TimedBuildProgress {
    pub(super) phase: BuildPhase,
    pub(super) current_artifact: Option<String>,
    pub(super) completed: usize,
    pub(super) total: usize,
    pub(super) elapsed: Duration,
    pub(super) activity: Option<crate::workflow::BuildActivity>,
}

impl TimedBuildProgress {
    pub(super) fn from_progress(progress: BuildProgress, started: Instant) -> Self {
        Self {
            phase: progress.phase,
            current_artifact: progress.current_artifact,
            completed: progress.completed,
            total: progress.total,
            elapsed: progress.timestamp.saturating_duration_since(started),
            activity: progress.activity,
        }
    }

    pub(super) fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.completed as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

pub(super) enum BuildWorkerEvent {
    Progress(TimedBuildProgress),
    Finished {
        result: Result<BuildReport, BuildFailure>,
        elapsed: Duration,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PendingRecipeAction {
    Close,
    New(ItemKind),
    Open(PathBuf),
    Import,
}

/// The failure the bottom bar reports. Only an activity-log notice can be dismissed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionDiagnostic {
    AccountSync,
    Installation,
    Build,
    Notice,
}

impl ActionDiagnostic {
    pub(super) const fn title(self) -> &'static str {
        match self {
            Self::AccountSync => "Account Sync Incomplete",
            Self::Installation => "Installation Blocked",
            Self::Build => "Build Blocked",
            Self::Notice => "Action Needs Attention",
        }
    }
}
