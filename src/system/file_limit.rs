//! Raise the open-file limit at startup on Linux.
//!
//! Reading a weapon means holding a package reader open, and the catalog, the icon cache, the
//! workbench's discovery scan and the window's own graphics sockets all hold descriptors at the
//! same time. The reader pool is budgeted so Sundial's own readers stay bounded, but that budget
//! assumes the process has room for them beside everything else. A distribution that ships a soft
//! limit of 1024 does not always leave that room, and the failure surfaces far from its cause, as
//! "Too many open files (os error 24)" while a scan is part way through.
//!
//! The soft limit is the process's own to raise, up to the hard limit the administrator set, so
//! this asks for the headroom rather than requiring the reader to find `ulimit` first. The hard
//! limit is never touched, an existing limit is never lowered, and a refusal is not fatal: the
//! pool still bounds Sundial's readers, and the reader can raise it themselves as before.

/// Descriptors to ask for. Comfortably above the reader pool's budget plus the icon, font and
/// graphics handles around it, and far below the hard limit a systemd distribution ships, so
/// this normally succeeds without asking for everything the system would allow.
#[cfg(target_os = "linux")]
const DESIRED: u64 = 8192;

/// Asks for [`DESIRED`] descriptors, capped by the hard limit. Returns what went wrong, for a
/// caller that wants to mention it; there is nothing here for a reader to act on when it works.
#[cfg(target_os = "linux")]
pub(crate) fn raise_open_file_limit() -> Result<(), String> {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

    let limits = getrlimit(Resource::Nofile);
    // No maximum means no ceiling to respect, so the request stands on its own.
    let target = limits
        .maximum
        .map_or(DESIRED, |maximum| DESIRED.min(maximum));
    // None represents RLIM_INFINITY, which is already above any finite target.
    if limits.current.is_none_or(|current| current >= target) {
        return Ok(());
    }
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(target),
            maximum: limits.maximum,
        },
    )
    .map_err(|error| {
        format!(
            "Could not raise the open-file limit to {target}: {error}. \
             Run `ulimit -n {target}` before Sundial if package reads fail."
        )
    })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn raise_open_file_limit() -> Result<(), String> {
    Ok(())
}
