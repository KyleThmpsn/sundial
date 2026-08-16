use std::{
    ffi::c_void,
    ptr,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

use eframe::egui;
use serde::Deserialize;
use windows_sys::Win32::{
    Foundation::GetLastError,
    Networking::WinHttp::{
        INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    },
};

pub const RELEASES_URL: &str = "https://github.com/kylethmpsn/sundial/releases";

const API_HOST: &str = "api.github.com";
const LATEST_RELEASE_PATH: &str = "/repos/kylethmpsn/sundial/releases/latest";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const NETWORK_TIMEOUT_MS: i32 = 5_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    NotStarted,
    Checking,
    Current,
    Available(String),
    Failed,
}

pub struct UpdateCheck {
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
    pub fn start_if_needed(&mut self, ctx: &egui::Context) {
        if self.status == UpdateStatus::NotStarted {
            self.start(ctx);
        }
    }

    pub fn retry(&mut self, ctx: &egui::Context) {
        if self.status != UpdateStatus::Checking {
            self.start(ctx);
        }
    }

    pub fn poll(&mut self) {
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

    pub const fn status(&self) -> &UpdateStatus {
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
    let body = winhttp_get(API_HOST, LATEST_RELEASE_PATH)?;
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

struct HttpHandle(*mut c_void);

impl HttpHandle {
    fn new(raw: *mut c_void, operation: &str) -> Result<Self, String> {
        if raw.is_null() {
            Err(last_error(operation))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        // SAFETY: The handle is non-null, owned by this wrapper, and closed exactly once here.
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}

fn winhttp_get(host: &str, path: &str) -> Result<Vec<u8>, String> {
    let agent = wide(&format!("Sundial/{}", env!("CARGO_PKG_VERSION")));
    let host = wide(host);
    let path = wide(path);
    let get = wide("GET");

    // SAFETY: All strings are owned, NUL-terminated UTF-16 buffers. Handles are checked after
    // creation and kept alive until every dependent WinHTTP operation has completed.
    unsafe {
        let session = HttpHandle::new(
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            ),
            "starting the update check",
        )?;
        if WinHttpSetTimeouts(
            session.0,
            NETWORK_TIMEOUT_MS,
            NETWORK_TIMEOUT_MS,
            NETWORK_TIMEOUT_MS,
            NETWORK_TIMEOUT_MS,
        ) == 0
        {
            return Err(last_error("setting update-check timeouts"));
        }
        let connection = HttpHandle::new(
            WinHttpConnect(session.0, host.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0),
            "connecting to GitHub",
        )?;
        let request = HttpHandle::new(
            WinHttpOpenRequest(
                connection.0,
                get.as_ptr(),
                path.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "creating the GitHub request",
        )?;
        if WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) == 0 {
            return Err(last_error("sending the GitHub request"));
        }
        if WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0 {
            return Err(last_error("receiving the GitHub response"));
        }

        let mut status_code = 0_u32;
        let mut status_size = 4_u32;
        let mut header_index = 0_u32;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            (&raw mut status_code).cast(),
            &raw mut status_size,
            &raw mut header_index,
        ) == 0
        {
            return Err(last_error("reading the GitHub response status"));
        }
        if status_code != 200 {
            return Err(format!("GitHub returned HTTP status {status_code}"));
        }

        let mut response = Vec::new();
        let mut buffer = [0_u8; 8 * 1024];
        let buffer_size = u32::try_from(buffer.len())
            .map_err(|_| "Update-check read buffer is too large for WinHTTP")?;
        loop {
            let mut read = 0_u32;
            if WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer_size,
                &raw mut read,
            ) == 0
            {
                return Err(last_error("reading the GitHub response"));
            }
            if read == 0 {
                break;
            }
            let read = read as usize;
            if response.len().saturating_add(read) > MAX_RESPONSE_BYTES {
                return Err("GitHub returned an unexpectedly large release response".into());
            }
            response.extend_from_slice(&buffer[..read]);
        }
        Ok(response)
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn last_error(operation: &str) -> String {
    // SAFETY: GetLastError has no preconditions and reads the calling thread's error state.
    let code = unsafe { GetLastError() };
    format!("Could not finish {operation} (Windows error {code})")
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
