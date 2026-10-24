//! The staged run of each layout's last install, kept so a switch between runtimes restores it
//! without a rebuild.
//!
//! Dawn and Sunrise before 0.6 load Parhelion's packages from stock `packages`, and Sunrise 0.6
//! loads them from its mod folder. An install for one runtime removes the other's build, since the
//! game would load both after a switch back. Each successful install keeps its staged run here,
//! hard-linked where the volume allows, per installation and layout. After a switch, the upgrade
//! review offers to install the kept run again. It goes through the usual review and install,
//! which check it against the installation and the other mods as they check any build.
use super::*;
use crate::workflow::staging_retention;
use sundial::package_authoring::mod_content;

const ROOT: &str = "installed";
const RECORD: &str = "kept-build.json";
const RECORD_SCHEMA: u32 = 1;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    target_packages_directory: PathBuf,
    installed_unix_seconds: u64,
}

/// A run kept from an earlier install for the active runtime's layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeptBuild {
    pub run_directory: PathBuf,
    pub installed: SystemTime,
    /// Whether it installs to Sunrise's mod folder rather than stock `packages`.
    pub mod_folder: bool,
}

fn layout(mod_folder: bool) -> &'static str {
    if mod_folder { "mods" } else { "packages" }
}

/// The folder holding one installation's kept runs, named for its packages path, or `None` without
/// a Parhelion data folder.
fn installation_directory(target: &Path) -> Option<PathBuf> {
    // Unit tests install into temporary folders, and each would leave a kept run in the user's
    // data folder under a key nothing reads again.
    if cfg!(test) {
        return None;
    }
    let data = sundial::package_authoring::parhelion_data_directory()?;
    // Windows paths compare without case.
    let key = Sha256::digest(target.to_string_lossy().to_lowercase().as_bytes());
    Some(data.join(ROOT).join(hex::encode(&key[..8])))
}

/// Keeps `staged`, the run an install just published to `target`, as that layout's build. On the
/// mod loader an importer mod the build kept installed comes from the installed folder, so its
/// files join the copy, and the copy's manifest installs them like any other.
pub(super) fn keep(staged: &Path, target: &Path, mod_folder: bool) -> Result<(), String> {
    let Some(root) = installation_directory(target) else {
        return Ok(());
    };
    fs::create_dir_all(&root).map_err(|e| format!("Could not create {}: {e}", root.display()))?;
    let temporary = root.join(format!(".{}-{}", layout(mod_folder), unique_token()));
    let result = (|| {
        link_tree(staged, &temporary, true)?;
        if mod_folder {
            own_kept_members(&temporary, target)?;
        }
        let record = Record {
            schema: RECORD_SCHEMA,
            target_packages_directory: target.to_owned(),
            installed_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
        fs::write(temporary.join(RECORD), bytes).map_err(|e| e.to_string())?;
        replace(&root.join(layout(mod_folder)), &temporary)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

/// Links every file under `source` into `destination`, copying where a link can't be made. The
/// staging markers stay out, so the copy is an unmarked run that staging cleanup never removes.
fn link_tree(source: &Path, destination: &Path, top: bool) -> Result<(), String> {
    fs::create_dir(destination)
        .map_err(|e| format!("Could not create {}: {e}", destination.display()))?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        // A restored run carries its old record, which a link would let the new one write through.
        if kind.is_symlink() || (top && (staging_retention::is_marker(&name) || name == RECORD)) {
            continue;
        }
        let (from, to) = (entry.path(), destination.join(&name));
        if kind.is_dir() {
            link_tree(&from, &to, false)?;
        } else {
            link_file(&from, &to)?;
        }
    }
    Ok(())
}

fn link_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::hard_link(from, to)
        .or_else(|_| fs::copy(from, to).map(drop))
        .map_err(|e| format!("Could not keep {}: {e}", from.display()))
}

/// Brings the files of each importer mod the build kept installed into the copy, and has the
/// copy's manifest install them from there. The installed folders leave with a later install for
/// the other runtime, and the copy must not depend on them.
fn own_kept_members(run: &Path, target: &Path) -> Result<(), String> {
    let path = run.join(MANIFEST_FILE_NAME);
    let mut manifest: ManifestDocument =
        sundial::package_authoring::read_json(File::open(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if !manifest.mods.iter().any(|member| member.kept) {
        return Ok(());
    }
    let install = target.parent().ok_or("Missing game root")?;
    for member in manifest.mods.iter_mut().filter(|member| member.kept) {
        link_member(install, member, run)?;
        member.kept = false;
    }
    manifest.validate()?;
    // The manifest is a link to the staged run's own, so it is replaced, never written through.
    fs::remove_file(&path).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(&path, bytes).map_err(|e| e.to_string())
}

/// Links the installed files of one importer mod into `run` by file name, where a build stages
/// them, after checking each against its receipt.
fn link_member(
    install: &Path,
    member: &crate::mod_family::Member,
    run: &Path,
) -> Result<(), String> {
    let directory = mod_content::member_directory(install, &member.manifest.id);
    for artifact in &member.receipt.artifacts {
        let name = artifact
            .path
            .rsplit('/')
            .next()
            .ok_or("An importer artifact has no file name")?;
        let source = directory.join(&artifact.path);
        mod_content::checked_path(install, &source)?;
        let digest = digest_file(&source).map_err(|e| e.to_string())?;
        if digest.byte_length != artifact.byte_length
            || !digest.sha256.eq_ignore_ascii_case(&artifact.sha256)
        {
            return Err(format!(
                "{} changed after the install. Its build was not kept",
                member.manifest.name
            ));
        }
        link_file(&source, &run.join(name))?;
    }
    Ok(())
}

/// Puts `incoming` in place of `kept`, removing the run it replaces.
fn replace(kept: &Path, incoming: &Path) -> Result<(), String> {
    let previous = kept.with_file_name(format!(
        ".{}-previous-{}",
        kept.file_name().and_then(|n| n.to_str()).unwrap_or("kept"),
        unique_token()
    ));
    let had = kept.try_exists().map_err(|e| e.to_string())?;
    if had {
        fs::rename(kept, &previous).map_err(|e| e.to_string())?;
    }
    if let Err(error) = fs::rename(incoming, kept) {
        if had {
            let _ = fs::rename(&previous, kept);
        }
        return Err(error.to_string());
    }
    if had {
        let _ = fs::remove_dir_all(&previous);
    }
    Ok(())
}

/// The run kept for `target` and the layout `mod_folder` names, if one was kept for it.
pub(super) fn find(target: &Path, mod_folder: bool) -> Result<Option<KeptBuild>, String> {
    let Some(root) = installation_directory(target) else {
        return Ok(None);
    };
    let run = root.join(layout(mod_folder));
    let record_path = run.join(RECORD);
    if !record_path.try_exists().map_err(|e| e.to_string())?
        || !run
            .join(MANIFEST_FILE_NAME)
            .try_exists()
            .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    // The upgrade review reads this at every start, so a record another version wrote, or one
    // that doesn't parse, only means there is no build to offer.
    let Ok(record) = serde_json::from_slice::<Record>(
        &fs::read(&record_path)
            .map_err(|e| format!("Could not read {}: {e}", record_path.display()))?,
    ) else {
        return Ok(None);
    };
    if record.schema != RECORD_SCHEMA || !paths_equal(&record.target_packages_directory, target) {
        return Ok(None);
    }
    Ok(Some(KeptBuild {
        run_directory: run,
        installed: UNIX_EPOCH + std::time::Duration::from_secs(record.installed_unix_seconds),
        mod_folder,
    }))
}
