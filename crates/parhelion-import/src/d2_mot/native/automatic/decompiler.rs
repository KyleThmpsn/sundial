//! Fetch the pinned standalone 3Dmigoto tool from its upstream release once.
//! Upstream source and license: https://github.com/bo3b/3Dmigoto/tree/1.3.16
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

pub(super) fn prepare(progress: &mut dyn FnMut(String)) -> Result<PathBuf> {
    ensure!(cfg!(windows), "Source shader conversion requires Windows");
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("Local app data folder")?)
        .join("Sundial/parhelion/importer/tools/3dmigoto-1.3.16");
    fs::create_dir_all(&root)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("download.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    if FILES
        .iter()
        .all(|(name, hash)| fs::read(root.join(name)).is_ok_and(|b| matches(&b, hash)))
    {
        return Ok(root.join(FILES[0].0));
    }
    progress("Downloading shader conversion tool (first import only)…".into());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let mut response = agent
        .get(URL)
        .header("User-Agent", "Sundial-Parhelion-Importer")
        .call()
        .context("Download the source shader conversion tool")?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(16 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
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
        let path = root.join(name);
        fs::write(&path, bytes)?;
    }
    Ok(root.join(FILES[0].0))
}
