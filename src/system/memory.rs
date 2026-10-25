//! Give memory back after the scans that needed it, and report what is still held.
//!
//! Building the catalog, assembling the native cache and reading every perk behaviour all work
//! the same way: thousands of package payloads are decompressed into buffers on a pool of
//! worker threads, decoded into something much smaller, and dropped. The peak is unavoidable.
//! What is avoidable is holding the peak afterwards.
//!
//! glibc gives each thread its own arena and keeps freed chunks on that arena's free list
//! rather than returning them, so a run of differently sized buffers across eight threads
//! leaves the process resident at close to its high-water mark long after the work is done.
//! The same run on Windows comes back down on its own, which is why the two platforms report
//! such different figures for the same catalog. Capping the arenas and trimming after each
//! scan asks glibc for the behaviour Windows already has.
//!
//! None of this changes what is kept: the catalog, the icons and the behaviour index are the
//! same on both platforms. It changes only whether the allocator holds the space the scan
//! borrowed. [`resident_bytes`] is what says whether it worked, and goes in the diagnostics
//! report so a reader can say what their install actually costs.

/// Arenas glibc may open. The scans are bounded by package reads rather than by processor
/// time, so the contention a low cap adds is small beside the space it saves.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const ARENAS: libc::c_int = 2;

/// Caps the allocator's arenas. Call once, before the worker pools start, because glibc keeps
/// arenas it has already created.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn limit_allocator_arenas() {
    // SAFETY: `mallopt` takes two integers by value and returns one. `M_ARENA_MAX` with a
    // positive count is a documented glibc tuning parameter, and a refusal is reported in the
    // return value rather than by any other effect. Nothing here owns or frees memory.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, ARENAS);
    }
}

/// Returns free chunks the allocator is holding to the operating system. Call after work that
/// allocated heavily has finished, never inside it: the next allocation would fault the pages
/// straight back in, and the trim walks every arena.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn release_free_memory() {
    // SAFETY: `malloc_trim` takes the bytes of headroom to keep and returns whether it freed
    // anything. It only releases chunks the allocator already owns and no longer lends out,
    // so no live allocation is affected.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(crate) fn limit_allocator_arenas() {}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(crate) fn release_free_memory() {}

/// Resident memory in bytes, or `None` where the platform will not say.
///
/// Resident rather than virtual, because it is the number a reader sees in their task manager
/// and the one that decides whether the machine starts swapping.
#[cfg(target_os = "linux")]
pub(crate) fn resident_bytes() -> Option<u64> {
    // statm reports pages: total size first, then resident.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    let page_size = u64::try_from(rustix::param::page_size()).ok()?;
    Some(pages * page_size)
}

#[cfg(windows)]
pub(crate) fn resident_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    // SAFETY: The counters are plain integers, for which all-zero is a valid value and is what
    // the call expects to find before it writes its own.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    // SAFETY: The handle is the pseudo-handle for this process, which needs no closing. The
    // struct is fully initialised above and its `cb` field declares its own size, which is how
    // the call decides what it may write. Failure is reported in the return value.
    let wrote = unsafe {
        GetProcessMemoryInfo(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            &raw mut counters,
            counters.cb,
        )
    };
    (wrote != 0).then_some(counters.WorkingSetSize as u64)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn resident_bytes() -> Option<u64> {
    None
}
