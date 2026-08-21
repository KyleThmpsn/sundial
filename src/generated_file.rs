use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn path(settings_path: &Path, file_name: &str) -> Result<PathBuf, String> {
    let directory = settings_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", settings_path.display()))?;
    Ok(directory.join(file_name))
}

pub(crate) fn save(
    settings_path: &Path,
    file_name: &str,
    document: &str,
    size_limit: usize,
    description: &str,
) -> Result<PathBuf, String> {
    if document.len() > size_limit {
        return Err(format!(
            "the generated {description} is {} bytes, above Sunrise's {size_limit}-byte limit",
            document.len(),
        ));
    }
    let path = path(settings_path, file_name)?;
    crate::storage::replace_file(&path, document.as_bytes())
        .map_err(|error| format!("Could not safely replace {}: {error}", path.display()))?;
    let saved =
        fs::read(&path).map_err(|error| format!("Could not verify {}: {error}", path.display()))?;
    if saved != document.as_bytes() {
        return Err(format!(
            "Could not verify {}: the saved {description} did not match the generated file",
            path.display()
        ));
    }
    Ok(path)
}
