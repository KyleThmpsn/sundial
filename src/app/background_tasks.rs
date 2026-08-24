use std::{path::PathBuf, sync::mpsc::Receiver};

use serde_json::Value;

use crate::catalog::{Catalog as Manifest, CatalogProgress};

use super::SettingsLayout;

pub(super) struct PendingInstallLoad {
    pub(super) install_path: PathBuf,
    pub(super) settings_path: PathBuf,
    pub(super) settings_layout: SettingsLayout,
    pub(super) document: Value,
}

pub(super) enum CatalogTaskKind {
    LoadInstall(PendingInstallLoad),
    Rebuild,
}

impl CatalogTaskKind {
    pub(super) const fn title(&self) -> &'static str {
        match self {
            Self::LoadInstall(_) => "Loading Shadowkeep installation",
            Self::Rebuild => "Rebuilding local catalog",
        }
    }
}

pub(super) enum CatalogTaskEvent {
    Progress(CatalogProgress),
    Finished(Box<Result<Manifest, String>>),
}

pub(super) struct CatalogTask {
    pub(super) kind: CatalogTaskKind,
    pub(super) receiver: Receiver<CatalogTaskEvent>,
    pub(super) progress: CatalogProgress,
}
