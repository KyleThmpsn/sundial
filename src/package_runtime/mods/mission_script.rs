//! Mission controllers owned by the same generation as their activity packages.
use super::{Path, PathBuf, checked_path, fs};

pub const BYTE_LIMIT: u64 = 128 * 1024;

/// A staged controller has a flat filename. Sunrise loads it from the activity's own folder.
pub fn path(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".lua")?;
    if !(9..=96).contains(&stem.len())
        || !stem.starts_with("mission_")
        || !stem
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    {
        return None;
    }
    Some(format!("scripts/{stem}/{file_name}"))
}

pub fn valid_path(relative: &str) -> bool {
    relative.rsplit('/').next().and_then(path).as_deref() == Some(relative)
}

/// Enumerate exactly the controller layout, rejecting links, extra nesting and foreign files.
pub fn files(directory: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let root = directory.join("scripts");
    checked_path(directory, &root)?;
    if !root.try_exists().map_err(|e| e.to_string())? {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        checked_path(directory, &entry.path())?;
        let stem = entry
            .file_name()
            .into_string()
            .map_err(|_| "Non-Unicode mission folder")?;
        let file_name = format!("{stem}.lua");
        let relative = path(&file_name).ok_or("Invalid managed mission folder")?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            return Err("Managed mission content must be a controller folder".into());
        }
        let children = fs::read_dir(entry.path())
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        // A journaled copy or removal can stop before or after the controller file itself.
        if children.is_empty() {
            continue;
        }
        if children.len() != 1 || children[0].file_name() != file_name.as_str() {
            return Err("Managed mission folder differs from its controller layout".into());
        }
        let file = children[0].path();
        checked_path(directory, &file)?;
        if !children[0]
            .file_type()
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Err("Managed mission controller is not a regular file".into());
        }
        found.push((relative, file));
    }
    Ok(found)
}
