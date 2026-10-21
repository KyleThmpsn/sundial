//! Fetch the pinned standalone 3Dmigoto tool from its upstream release once, or unpack the same
//! release archive the user downloaded.
//! Upstream source and license: <https://github.com/bo3b/3Dmigoto/tree/1.3.16>
use super::*;
use std::io::Read;

const URL: &str =
    "https://github.com/bo3b/3Dmigoto/releases/download/1.3.16/cmd_Decompiler-1.3.16.zip";
const ARCHIVE: &str = "5e72e067dfcb15c36f106efa74d805055eec5314dc84b8fca8e65d835683a1b2";
const FILES: &[(&str, &str)] = &[
    (
        "cmd_Decompiler.exe",
        "67582ced9261b8fb23a50cee09c788c821d0524d40264721369b410d5321dab8",
    ),
    (
        "d3dcompiler_46.dll",
        "60f76ec7169397c425023d5927a3c3c34599fa329814053cace6171e20adb353",
    ),
];

fn matches(bytes: &[u8], expected: &str) -> bool {
    hex::encode(Sha256::digest(bytes)) == expected
}

fn root() -> Result<PathBuf> {
    ensure!(cfg!(windows), "Source shader conversion requires Windows");
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("Local app data folder")?)
        .join("Sundial/parhelion/importer/tools/3dmigoto-1.3.16");
    fs::create_dir_all(&root)?;
    Ok(root)
}

fn complete(root: &Path) -> bool {
    FILES
        .iter()
        .all(|(name, hash)| fs::read(root.join(name)).is_ok_and(|b| matches(&b, hash)))
}

/// The unpacked tool, when every file matches its pinned hash.
pub(crate) fn cached() -> Option<PathBuf> {
    let root = root().ok()?;
    complete(&root).then(|| root.join(FILES[0].0))
}

/// Unpacks the pinned release archive from a file the user downloaded.
pub(crate) fn install(archive: &Path) -> Result<PathBuf> {
    let bytes = fs::read(archive).with_context(|| format!("Read {}", archive.display()))?;
    let root = root()?;
    let lock = open_lock(&root)?;
    crate::cancellation::lock(&lock)?;
    unpack(&root, bytes)
}

fn open_lock(root: &Path) -> Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("download.lock"))?)
}

pub(crate) fn prepare(progress: &mut dyn FnMut(String)) -> Result<PathBuf> {
    let root = root()?;
    let lock = open_lock(&root)?;
    crate::cancellation::lock(&lock)?;
    if complete(&root) {
        return Ok(root.join(FILES[0].0));
    }
    progress("Downloading shader conversion tool (first import only)…".into());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let bytes = crate::cancellation::compute(move || {
        let mut response = agent
            .get(URL)
            .header("User-Agent", "Sundial-Parhelion-Importer")
            .call()
            .context("Download the 3Dmigoto decompiler")?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(16 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    })?;
    unpack(&root, bytes)
}

fn unpack(root: &Path, bytes: Vec<u8>) -> Result<PathBuf> {
    ensure!(
        matches(&bytes, ARCHIVE),
        "Shader tool archive checksum differs"
    );
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut files = Vec::new();
    for &(name, hash) in FILES {
        let mut bytes = Vec::new();
        archive
            .by_name(name)?
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
        ensure!(matches(&bytes, hash), "Shader tool {name} checksum differs");
        files.push((name, bytes));
    }
    for (name, bytes) in files {
        crate::cancellation::check()?;
        let path = root.join(name);
        fs::write(&path, bytes)?;
    }
    Ok(root.join(FILES[0].0))
}
