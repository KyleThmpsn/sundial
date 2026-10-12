//! Anchor process-global decoders in the source installation before opening a temporary view.
#[cfg(windows)]
use std::fs;
use std::path::Path;

#[cfg(windows)]
pub fn initialize_source_decoder(source: &Path) -> Result<(), String> {
    let source = source.canonicalize().map_err(|error| {
        format!(
            "Could not resolve package source {}: {error}",
            source.display()
        )
    })?;
    let bin = source
        .parent()
        .ok_or_else(|| "Package source has no install parent".to_owned())?
        .join("bin");
    if !["oo2core_3_win64.dll", "oo2core_9_win64.dll"]
        .iter()
        .any(|name| bin.join("x64").join(name).is_file())
    {
        return Ok(());
    }
    for entry in fs::read_dir(&bin).map_err(|error| {
        format!(
            "Could not inspect decoder directory {}: {error}",
            bin.display()
        )
    })? {
        let entry =
            entry.map_err(|error| format!("Could not inspect decoder directory: {error}"))?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pkg"))
        {
            return Err(format!(
                "Decoder bootstrap refuses unexpected package data in {}",
                bin.display()
            ));
        }
    }
    // tiger-pkg 0.21 loads ../bin/x64 DLLs into process-global storage BEFORE enumerating the
    // supplied directory. Pointing it at install/bin anchors those DLLs in the stable install
    // without scanning package data, including authored overlays intentionally excluded below.
    // Both dependencies enable ignore_caches. The expected empty census performs no writes.
    match super::PackageManager::new(
        &bin,
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        None,
    ) {
        Err(error) if error == "No packages found" => Ok(()),
        Err(error) => Err(format!(
            "Could not initialize the source package decoder: {error}"
        )),
        Ok(_) => Err("Decoder bootstrap unexpectedly discovered package data".to_owned()),
    }
}

#[cfg(not(windows))]
pub fn initialize_source_decoder(_source: &Path) -> Result<(), String> {
    Ok(())
}
