//! Observational events from the existing install transaction boundaries.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallPhase {
    Checking,
    ReviewingAccount,
    BackingUp,
    Preparing,
    Rechecking,
    UpdatingAccount,
    Installing,
    Verifying,
    RefreshingCaches,
    Finalizing,
    SyncingCollections,
    CleaningUp,
    Complete,
    RollingBack,
}

impl InstallPhase {
    pub const STAGES: [Self; 12] = [
        Self::Checking,
        Self::ReviewingAccount,
        Self::BackingUp,
        Self::Preparing,
        Self::Rechecking,
        Self::UpdatingAccount,
        Self::Installing,
        Self::Verifying,
        Self::RefreshingCaches,
        Self::Finalizing,
        Self::SyncingCollections,
        Self::CleaningUp,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Checking => "Checking Installation",
            Self::ReviewingAccount => "Checking Reviewed Account Changes",
            Self::BackingUp => "Backing Up Existing Files",
            Self::Preparing => "Preparing Package Files",
            Self::Rechecking => "Rechecking Files and Account",
            Self::UpdatingAccount => "Applying Reviewed Account Changes",
            Self::Installing => "Installing Packages",
            Self::Verifying => "Verifying Installed Packages",
            Self::RefreshingCaches => "Refreshing Game Caches",
            Self::Finalizing => "Finalizing the Transaction",
            Self::SyncingCollections => "Updating Collections",
            Self::CleaningUp => "Finishing Backup and Cleanup",
            Self::Complete => "Installation Complete",
            Self::RollingBack => "Restoring the Previous Installation",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallProgress {
    pub phase: InstallPhase,
    pub current_artifact: Option<String>,
    pub completed: usize,
    pub total: usize,
}

impl InstallProgress {
    pub(super) fn stage(phase: InstallPhase) -> Self {
        Self {
            phase,
            current_artifact: None,
            completed: 0,
            total: 0,
        }
    }

    pub(super) fn item(phase: InstallPhase, name: &str, completed: usize, total: usize) -> Self {
        Self {
            phase,
            current_artifact: Some(name.to_owned()),
            completed,
            total,
        }
    }
}

pub(super) type Observer<'a> = &'a mut dyn FnMut(InstallProgress);
