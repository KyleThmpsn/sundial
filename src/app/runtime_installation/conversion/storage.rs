use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::app::runtime_installation) struct Receipt {
    pub version: u8,
    pub install: PathBuf,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub to_sqlite: bool,
    pub had_json: bool,
    pub had_database: bool,
    pub files: BTreeMap<String, String>,
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(err(error)),
    }
}

pub(super) fn write(path: PathBuf, contents: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(err)?;
    file.write_all(contents).map_err(err)?;
    file.sync_all().map_err(err)
}

pub(super) fn check_settings_path(root: &Path, path: &Path) -> Result<(), String> {
    if !SettingsLayout::ALL
        .into_iter()
        .any(|layout| layout.relative_path() == path)
    {
        return Err("The backup names an unsupported settings path.".into());
    }
    check_path(root, &root.join(path))
}

pub(super) fn check_path(root: &Path, path: &Path) -> Result<(), String> {
    let ancestor = path
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .ok_or("The installation is missing.")?;
    crate::package_runtime::installation::checked_path(root, ancestor)
}

pub(super) fn hashes(directory: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(directory).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or("Invalid backup filename.")?
            .to_owned();
        if !entry.file_type().map_err(err)?.is_file() {
            return Err("Invalid conversion backup entry.".into());
        }
        if name != "conversion.json" {
            result.insert(name, hash(&fs::read(entry.path()).map_err(err)?));
        }
    }
    Ok(result)
}

pub(super) fn save_backup(receipt: &Receipt, stage: &Path) -> Result<PathBuf, String> {
    if hashes(stage)? != receipt.files {
        return Err("The staged conversion changed. Review it again.".into());
    }
    let directory = receipt.install.join(".sunrise/backups");
    crate::package_runtime::installation::checked_directory(&receipt.install, &directory)?;
    let folder = tempfile::Builder::new()
        .prefix("account-conversion-")
        .tempdir_in(&directory)
        .map_err(err)?
        .keep();
    for name in receipt.files.keys() {
        write(folder.join(name), &fs::read(stage.join(name)).map_err(err)?)?;
    }
    if hashes(&folder)? != receipt.files {
        return Err("The conversion backup did not verify.".into());
    }
    write(
        folder.join("conversion.json"),
        &serde_json::to_vec_pretty(receipt).map_err(err)?,
    )?;
    Ok(folder)
}

pub(super) fn load_backup(install: &Path, folder: &Path) -> Result<Receipt, String> {
    let root = fs::canonicalize(install).map_err(err)?;
    let parent = fs::canonicalize(folder.parent().ok_or("Missing backup folder.")?).map_err(err)?;
    if parent != root.join(".sunrise/backups") {
        return Err("Choose an account conversion backup from this installation.".into());
    }
    let backup_path = parent.join(folder.file_name().ok_or("Missing backup name.")?);
    let folder = backup_path.as_path();
    crate::package_runtime::installation::checked_path(&root, folder)?;
    let actual_hashes = hashes(folder)?;
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(folder.join("conversion.json")).map_err(err)?)
            .map_err(err)?;
    if receipt.version != 1 || receipt.install != root {
        return Err("This conversion backup belongs to another installation or format.".into());
    }
    check_settings_path(&root, &receipt.source_path)?;
    check_settings_path(&root, &receipt.target_path)?;
    if actual_hashes != receipt.files {
        return Err("The conversion backup changed or is incomplete.".into());
    }
    let after: Value = crate::strict_json::from_str(
        std::str::from_utf8(&fs::read(folder.join("settings.after.json")).map_err(err)?)
            .map_err(err)?,
    )
    .map_err(err)?;
    if after["version"] != if receipt.to_sqlite { 18 } else { 6 } {
        return Err("The backup's target format is invalid.".into());
    }
    if receipt.to_sqlite {
        native::snapshot::read(&folder.join("account.after.sqlite3"))?;
    }
    if receipt.had_database {
        native::snapshot::read(&folder.join("account.before.sqlite3"))?;
    }
    if receipt.had_json {
        fs::read(folder.join("settings.before.json")).map_err(err)?;
    }
    Ok(receipt)
}

pub(super) fn install(
    receipt: &Receipt,
    folder: &Path,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    if load_backup(&receipt.install, folder)? != *receipt {
        return Err("The conversion backup changed after review.".into());
    }
    let target = receipt.install.join(&receipt.target_path);
    if receipt.to_sqlite {
        check()?;
        let database = crate::persistence::investment_path(&target);
        check_path(&receipt.install, &database)?;
        crate::package_runtime::installation::checked_directory(
            &receipt.install,
            database.parent().ok_or("Missing database folder.")?,
        )?;
        if receipt.had_database {
            let expected = native::snapshot::read(&folder.join("account.before.sqlite3"))?;
            native::conversion::restore(
                &database,
                &expected,
                &folder.join("account.after.sqlite3"),
            )?;
        } else {
            create_database(&folder.join("account.after.sqlite3"), &database)?;
        }
    }
    let after = fs::read(folder.join("settings.after.json")).map_err(err)?;
    let result = check().and_then(|()| replace_json(receipt, folder, &after));
    if let Err(error) = result {
        if read_optional(&target)? == Some(after) {
            return Err(format!(
                "Conversion was written, but its final check failed: {error}."
            ));
        }
        let rollback = if receipt.to_sqlite {
            revert_database(receipt, folder, check)
        } else {
            Ok(())
        };
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback) => {
                format!("Conversion stopped: {error}. Database recovery also stopped: {rollback}.")
            }
        });
    }
    Ok(())
}

fn replace_json(receipt: &Receipt, folder: &Path, after: &[u8]) -> Result<(), String> {
    let target = receipt.install.join(&receipt.target_path);
    check_path(&receipt.install, &target)?;
    if receipt.had_json {
        crate::storage::replace_file_if_unchanged(
            &target,
            after,
            &fs::read(folder.join("settings.before.json")).map_err(err)?,
        )
        .map_err(err)?;
    } else {
        create_json(&target, after)?;
    }
    if fs::read(&target).map_err(err)? != after {
        return Err("Converted settings did not verify.".into());
    }
    Ok(())
}

pub(super) fn create_json(target: &Path, contents: &[u8]) -> Result<(), String> {
    crate::storage::create_file(target, contents).map_err(err)
}

pub(super) fn create_database(source: &Path, target: &Path) -> Result<(), String> {
    let staged =
        tempfile::NamedTempFile::new_in(target.parent().ok_or("Missing database folder.")?)
            .map_err(err)?;
    let db =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(err)?;
    db.backup(rusqlite::MAIN_DB, staged.path(), None)
        .map_err(err)?;
    if native::snapshot::read(staged.path())? != native::snapshot::read(source)? {
        return Err("The converted database did not verify.".into());
    }
    crate::storage::publish_new(staged, target).map_err(err)
}

fn check_database_restore(receipt: &Receipt, folder: &Path) -> Result<(), String> {
    let path = crate::persistence::investment_path(&receipt.install.join(&receipt.target_path));
    check_path(&receipt.install, &path)?;
    let current = if path.try_exists().map_err(err)? {
        Some(native::snapshot::read(&path)?)
    } else {
        None
    };
    let before = if receipt.had_database {
        Some(native::snapshot::read(
            &folder.join("account.before.sqlite3"),
        )?)
    } else {
        None
    };
    if current == before
        || current
            == Some(native::snapshot::read(
                &folder.join("account.after.sqlite3"),
            )?)
    {
        Ok(())
    } else {
        Err("The database changed after conversion. The backup was not restored.".into())
    }
}

fn revert_database(
    receipt: &Receipt,
    folder: &Path,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    check()?;
    check_database_restore(receipt, folder)?;
    let path = crate::persistence::investment_path(&receipt.install.join(&receipt.target_path));
    if !path.exists() {
        return Ok(());
    }
    let after = native::snapshot::read(&folder.join("account.after.sqlite3"))?;
    if receipt.had_database {
        let before = native::snapshot::read(&folder.join("account.before.sqlite3"))?;
        if native::snapshot::read(&path)? != before {
            native::conversion::restore(&path, &after, &folder.join("account.before.sqlite3"))?;
        }
    } else {
        remove_database(&path, &after)?;
    }
    Ok(())
}

pub(super) fn remove_database(path: &Path, expected: &[u8]) -> Result<(), String> {
    // Force a checkpoint and leave WAL mode before removing a database created by conversion.
    // A live connection that cannot release the database makes this fail without deleting it.
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(err)?;
    let mode: String = db
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .map_err(err)?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err("The database is still in use. Close it before restoring.".into());
    }
    drop(db);
    if native::snapshot::read(path)? != expected {
        return Err("The database changed during restore.".into());
    }
    fs::remove_file(path).map_err(err)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
