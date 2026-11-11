//! Driver-specific prepared programs, read and written only by compiler helpers.
use super::{Source, binary_now, load_program};
use crate::model_preview::timing;
use eframe::glow::{self, HasContext};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

// Change the revision when helper preparation changes without changing the shader text.
const REVISION: &[u8] = b"Sundial prepared preview program 1";
const MAGIC: &[u8; 8] = b"SDGLPR01";
const HEADER: usize = 80;
const MAX_BINARY: usize = 64 * 1024 * 1024;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;

pub(super) struct Cache {
    directory: Option<PathBuf>,
    driver: [u8; 32],
    formats: Vec<i32>,
}

impl Cache {
    /// The helper's current context supplies the device, driver and context identity.
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let mut driver = Sha256::new();
        driver.update(REVISION);
        // SAFETY: the helper owns this current context for its whole lifetime.
        let formats = unsafe {
            for parameter in [
                glow::VENDOR,
                glow::RENDERER,
                glow::VERSION,
                glow::SHADING_LANGUAGE_VERSION,
            ] {
                field(&mut driver, gl.get_parameter_string(parameter).as_bytes());
            }
            for parameter in [
                glow::MAJOR_VERSION,
                glow::MINOR_VERSION,
                glow::CONTEXT_PROFILE_MASK,
                glow::CONTEXT_FLAGS,
            ] {
                driver.update(gl.get_parameter_i32(parameter).to_le_bytes());
            }
            let mut formats = vec![
                0;
                gl.get_parameter_i32(glow::NUM_PROGRAM_BINARY_FORMATS)
                    .max(0) as usize
            ];
            gl.get_parameter_i32_slice(glow::PROGRAM_BINARY_FORMATS, &mut formats);
            for format in &formats {
                driver.update(format.to_le_bytes());
            }
            formats
        };
        Self {
            directory: crate::system::paths::cache_dir().map(|root| root.join("preview/programs")),
            driver: driver.finalize().into(),
            formats,
        }
    }

    /// A validated disk hit, or source compilation with best-effort atomic publication.
    pub unsafe fn binary(
        &self,
        gl: &glow::Context,
        source: &Source,
    ) -> Result<glow::ProgramBinary, String> {
        let mut digest = Sha256::new();
        digest.update(self.driver);
        field(&mut digest, source.vertex.as_bytes());
        field(&mut digest, source.fragment.as_bytes());
        let key: [u8; 32] = digest.finalize().into();
        let name = hex::encode(key);
        let path = self
            .directory
            .as_ref()
            .map(|root| root.join(format!("{name}.bin")));
        timing::measure(
            || format!("GPU program {name}"),
            "compiler",
            "program preparation",
            Duration::ZERO,
            || {
                if let Some(path) = &path
                    // SAFETY: cache validation uses only the helper's current context.
                    && let Some(binary) = unsafe { self.cached(gl, path, &key, &name) }
                {
                    return Ok(binary);
                }
                timing::mark(|| format!("GPU program source {name}"), "compiler");
                // SAFETY: all driver work stays on the helper's current context.
                let binary = unsafe { binary_now(gl, source) }?;
                if let Some(path) = &path {
                    let _ = write(path, &key, &binary);
                }
                Ok(binary)
            },
        )
    }

    unsafe fn cached(
        &self,
        gl: &glow::Context,
        path: &Path,
        key: &[u8; 32],
        name: &str,
    ) -> Option<glow::ProgramBinary> {
        let binary = match read(path, key, &self.formats) {
            Ok(binary) => binary,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    timing::mark(|| format!("GPU program cache invalid {name}"), "compiler");
                }
                return None;
            }
        };
        // SAFETY: validate and delete on the helper's current context. Source fallback must
        // start with no errors left by a refused program.
        unsafe {
            let Some(program) = load_program(gl, &binary) else {
                timing::mark(|| format!("GPU program cache rejected {name}"), "compiler");
                for _ in 0..8 {
                    if gl.get_error() == glow::NO_ERROR {
                        break;
                    }
                }
                return None;
            };
            gl.delete_program(program);
        }
        if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
        timing::mark(|| format!("GPU program cache hit {name}"), "compiler");
        Some(binary)
    }
}

fn field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn read(path: &Path, key: &[u8; 32], formats: &[i32]) -> io::Result<glow::ProgramBinary> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidData);
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if !(HEADER as u64..=(HEADER + MAX_BINARY) as u64).contains(&length) {
        return Err(invalid());
    }
    let mut header = [0; HEADER];
    file.read_exact(&mut header)?;
    let format = u32::from_le_bytes(header[40..44].try_into().unwrap());
    let size = u32::from_le_bytes(header[44..48].try_into().unwrap()) as usize;
    if &header[..8] != MAGIC
        || &header[8..40] != key
        || size == 0
        || size as u64 != length - HEADER as u64
        || !formats.contains(&(format as i32))
    {
        return Err(invalid());
    }
    let mut buffer = vec![0; size];
    file.read_exact(&mut buffer)?;
    if Sha256::digest(&buffer)[..] != header[48..80] {
        return Err(invalid());
    }
    Ok(glow::ProgramBinary { buffer, format })
}

fn write(path: &Path, key: &[u8; 32], binary: &glow::ProgramBinary) -> io::Result<()> {
    if binary.buffer.is_empty() || binary.buffer.len() > MAX_BINARY {
        return Ok(());
    }
    let directory = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(MAGIC)?;
    temporary.write_all(key)?;
    temporary.write_all(&binary.format.to_le_bytes())?;
    temporary.write_all(&(binary.buffer.len() as u32).to_le_bytes())?;
    temporary.write_all(&Sha256::digest(&binary.buffer))?;
    temporary.write_all(&binary.buffer)?;
    temporary.flush()?;
    temporary.persist(path)?;
    sweep(directory);
    Ok(())
}

/// Approximate LRU across devices and sources. Concurrent publishers may exceed the budget by
/// one in-flight record until the next publication. Temporary and unrelated files are untouched.
fn sweep(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?;
            if path.extension()? != "bin"
                || name.len() != 64
                || !name.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    metadata.len(),
                    path,
                )
            })
        })
        .collect();
    files.sort_unstable_by_key(|(modified, ..)| *modified);
    let mut count = files.len();
    let mut bytes: u64 = files.iter().map(|(_, size, _)| size).sum();
    for (_, size, path) in files {
        if count <= MAX_ENTRIES && bytes <= MAX_BYTES {
            break;
        }
        if fs::remove_file(path).is_ok() {
            count -= 1;
            bytes -= size;
        }
    }
}
