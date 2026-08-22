use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use eframe::egui;
use serde::Deserialize;

pub(crate) const RELEASES_URL: &str = "https://github.com/kylethmpsn/sundial/releases";

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/kylethmpsn/sundial/releases/latest";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateStatus {
    NotStarted,
    Checking,
    Current,
    Available(String),
    Failed,
}

pub(crate) struct UpdateCheck {
    status: UpdateStatus,
    receiver: Option<Receiver<Result<Option<String>, String>>>,
}

impl Default for UpdateCheck {
    fn default() -> Self {
        Self {
            status: UpdateStatus::NotStarted,
            receiver: None,
        }
    }
}

impl UpdateCheck {
    pub(crate) fn start_if_needed(&mut self, ctx: &egui::Context) {
        if self.status == UpdateStatus::NotStarted {
            self.start(ctx);
        }
    }

    pub(crate) fn retry(&mut self, ctx: &egui::Context) {
        if self.status != UpdateStatus::Checking {
            self.start(ctx);
        }
    }

    pub(crate) fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(Some(version))) => {
                self.status = UpdateStatus::Available(version);
                self.receiver = None;
            }
            Ok(Ok(None)) => {
                self.status = UpdateStatus::Current;
                self.receiver = None;
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                self.status = UpdateStatus::Failed;
                self.receiver = None;
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    pub(crate) const fn status(&self) -> &UpdateStatus {
        &self.status
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        self.status = UpdateStatus::Checking;
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let result = latest_available_version();
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
}

fn latest_available_version() -> Result<Option<String>, String> {
    let body = crate::http::get(LATEST_RELEASE_URL, MAX_RESPONSE_BYTES)
        .map_err(|error| format!("Could not check GitHub for updates: {error}"))?;
    available_version_from_response(&body, env!("CARGO_PKG_VERSION"))
}

fn available_version_from_response(body: &[u8], current: &str) -> Result<Option<String>, String> {
    let release: LatestRelease = serde_json::from_slice(body)
        .map_err(|error| format!("GitHub returned an invalid release response: {error}"))?;
    Ok(version_is_newer(current, &release.tag_name).then_some(release.tag_name))
}

fn version_is_newer(current: &str, candidate: &str) -> bool {
    let (Some(mut current), Some(mut candidate)) =
        (version_components(current), version_components(candidate))
    else {
        return false;
    };
    let width = current.len().max(candidate.len());
    current.resize(width, 0);
    candidate.resize(width, 0);
    candidate > current
}

fn version_components(version: &str) -> Option<Vec<u64>> {
    let version = version.trim();
    let version = version.strip_prefix('v').unwrap_or(version);
    if version.is_empty() {
        return None;
    }
    version
        .split('.')
        .map(|component| {
            if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                component.parse().ok()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_handles_sundial_release_numbers() {
        assert!(version_is_newer("0.2", "v0.2.1"));
        assert!(version_is_newer("0.2.1", "v0.2.1.1"));
        assert!(version_is_newer("0.2.9", "v0.3"));
        assert!(!version_is_newer("0.2.1", "v0.2.1"));
        assert!(!version_is_newer("0.2.1", "v0.2"));
        assert!(!version_is_newer("0.2.1", "not-a-version"));
    }

    #[test]
    fn latest_release_response_only_reports_newer_versions() {
        assert_eq!(
            available_version_from_response(br#"{"tag_name":"v0.3"}"#, "0.2.1").unwrap(),
            Some("v0.3".into())
        );
        assert_eq!(
            available_version_from_response(br#"{"tag_name":"v0.2.1"}"#, "0.2.1").unwrap(),
            None
        );
        assert!(available_version_from_response(b"{}", "0.2.1").is_err());
    }
}
