//! Initialize decompression before reading a staged runtime-map payload.
use std::{path::Path, sync::OnceLock};

// tiger-pkg retains its decoder for the process lifetime. Raw package validation must
// remain usable without it, but compressed payload reads must never reach its panic path.
static READY: OnceLock<()> = OnceLock::new();

pub(super) fn ensure_initialized(target_packages: &Path) -> Result<(), String> {
    if READY.get().is_some() {
        return Ok(());
    }
    initialize(target_packages)?;
    let _ = READY.set(());
    Ok(())
}

#[cfg(windows)]
fn initialize(target_packages: &Path) -> Result<(), String> {
    let target = target_packages.canonicalize().map_err(|error| {
        format!("Could not resolve the target package directory for decompression: {error}")
    })?;
    let runtime = target
        .parent()
        .ok_or_else(|| "The target package directory has no install parent".to_owned())?
        .join("bin/x64/oo2core_3_win64.dll");
    // SAFETY: This is the selected installation's absolute Oodle path. Retain the library
    // until tiger-pkg initializes its process-global decoder from that same path.
    let library = unsafe { libloading::Library::new(&runtime) }.map_err(|error| {
        format!(
            "Could not initialize compressed package validation from {}: {error}",
            runtime.display()
        )
    })?;
    // SAFETY: Only verify the export address. No call or ABI-dependent value is read here.
    unsafe { library.get::<*const ()>(b"OodleLZ_Decompress\0") }.map_err(|error| {
        format!(
            "Compressed package decoder {} is missing OodleLZ_Decompress: {error}",
            runtime.display()
        )
    })?;
    // The public manager initializes the decoder. Direct PackageD2PreBL readers do not.
    let _manager = sundial::package_authoring::open_shadowkeep_package_manager(&target)?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use std::{fs, os::windows::process::CommandExt, process::Command};

    const CHILD: &str = "PARHELION_MISSING_DECODER_CHILD";

    #[test]
    fn missing_decoder_returns_an_error_in_a_fresh_process() {
        if std::env::var_os(CHILD).is_some() {
            let packages = std::env::current_dir().unwrap().join("packages");
            let error = super::ensure_initialized(&packages).unwrap_err();
            assert!(error.contains("compressed package validation"), "{error}");
            assert!(error.contains("oo2core_3_win64.dll"), "{error}");
            assert!(super::READY.get().is_none());
            assert!(!packages.parent().unwrap().join("bin").exists());
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("decoder-error-test.exe");
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        fs::create_dir(root.path().join("packages")).unwrap();
        let output = Command::new(executable)
            .args([
                "install::validation::decoder::tests::missing_decoder_returns_an_error_in_a_fresh_process",
                "--exact", "--nocapture", "--test-threads=1",
            ])
            .env(CHILD, "1")
            .current_dir(root.path())
            .creation_flags(0x0800_0000)
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(not(windows))]
fn initialize(target_packages: &Path) -> Result<(), String> {
    // Let Sundial prepare its checked Linux runtime before touching the lazy decoder.
    // Initializing tiger's lazy value first would cache a failure before the runtime exists.
    let _manager = sundial::package_authoring::open_shadowkeep_package_manager(target_packages)?;
    Ok(())
}
