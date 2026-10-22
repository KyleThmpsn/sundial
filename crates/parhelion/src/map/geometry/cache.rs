//! Immutable area conversions, separate from installed package-family reuse.
use anyhow::{Context, Result, ensure};
use parhelion_import::{
    cache::Generation,
    halo_reach::level::native::{Converted, Templates},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use sundial::package_authoring::PackageManager;
use tiger_pkg::TagHash;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    key: String,
    generation: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Native {
    tag: u32,
    reference: u32,
    kind: u8,
    subtype: u8,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    key: String,
    files: Vec<File>,
    native: Vec<Native>,
}

pub(super) struct Cache {
    root: PathBuf,
    key: String,
    generation: Generation,
    _previous: Option<Generation>,
    pub converted: Option<Converted>,
}

fn hash(value: &impl Serialize) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Map cache contains a link"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "Map cache contains a reparse point"
        );
    }
    Ok(metadata)
}

fn files(root: &Path) -> Result<Vec<File>> {
    let mut pending = vec![root.to_path_buf()];
    let mut result = Vec::new();
    while let Some(directory) = pending.pop() {
        parhelion_import::cancellation::check()?;
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let metadata = ordinary(&path)?;
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            ensure!(metadata.is_file(), "Map cache has a non-file entry");
            let relative = path.strip_prefix(root)?.to_owned();
            if relative == Path::new("cache.json") {
                continue;
            }
            let digest =
                crate::artifact::digest_file(&path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            result.push(File {
                path: relative,
                bytes: digest.byte_length,
                sha256: digest.sha256,
            });
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn native(root: &Path, files: &[File], manager: &PackageManager) -> Result<Vec<Native>> {
    let mut records = BTreeMap::new();
    for file in files.iter().filter(|file| {
        file.path
            .file_name()
            .is_some_and(|name| name == "source-manifest.json")
    }) {
        let source: Value = serde_json::from_slice(&fs::read(root.join(&file.path))?)?;
        for row in source["tags"]
            .as_object()
            .context("Native read manifest has no tags")?
            .values()
        {
            let tag = u32::try_from(row["tag"].as_u64().context("Native read identity")?)?;
            let path = root
                .join(file.path.parent().context("Native manifest parent")?)
                .join("raw")
                .join(format!("{tag:08X}.bin"));
            let bytes = fs::read(path)?;
            let entry = manager
                .get_entry(TagHash(tag))
                .context("Native dependency disappeared")?;
            ensure!(
                row["reference"].as_u64() == Some(u64::from(entry.reference)),
                "Native dependency reference changed"
            );
            ensure!(
                row["type"].as_u64() == Some(u64::from(entry.file_type))
                    && row["subtype"].as_u64() == Some(u64::from(entry.file_subtype)),
                "Native dependency type changed"
            );
            let current = manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)?;
            ensure!(
                bytes == current,
                "Native dependency changed during conversion"
            );
            records.insert(
                tag,
                Native {
                    tag,
                    reference: entry.reference,
                    kind: entry.file_type,
                    subtype: entry.file_subtype,
                    sha256: hex::encode(Sha256::digest(bytes)),
                },
            );
        }
    }
    ensure!(
        !records.is_empty(),
        "Area conversion has no recorded native dependencies"
    );
    Ok(records.into_values().collect())
}

fn load(folder: &Path, key: &str, manager: &PackageManager) -> Result<Converted> {
    ensure!(
        ordinary(&folder.join("cache.json"))?.len() <= 64 * 1024 * 1024,
        "Map cache manifest is too large"
    );
    let manifest: Manifest = serde_json::from_slice(&fs::read(folder.join("cache.json"))?)?;
    ensure!(
        manifest.key == key && !manifest.files.is_empty() && !manifest.native.is_empty(),
        "Map cache identity differs"
    );
    // Enumerating the tree rejects missing, extra and redirected payloads as well as changed bytes.
    ensure!(
        serde_json::to_vec(&files(folder)?)? == serde_json::to_vec(&manifest.files)?,
        "Map cache payload changed"
    );
    for row in manifest.native {
        parhelion_import::cancellation::check()?;
        let entry = manager
            .get_entry(TagHash(row.tag))
            .context("Cached native dependency disappeared")?;
        ensure!(
            entry.reference == row.reference
                && entry.file_type == row.kind
                && entry.file_subtype == row.subtype,
            "Cached native entry changed"
        );
        let bytes = manager
            .read_tag(TagHash(row.tag))
            .map_err(anyhow::Error::msg)?;
        ensure!(
            hex::encode(Sha256::digest(bytes)) == row.sha256,
            "Cached native payload changed"
        );
    }
    Ok(serde_json::from_slice(&fs::read(
        folder.join("conversion.json"),
    )?)?)
}

impl Cache {
    /// The caller holds the existing per-area lock through linking. A generation lease also
    /// prevents retention from removing a graph while its payloads are being read.
    pub(super) fn open(
        root: &Path,
        source: &crate::map::MapSource,
        templates: &Templates,
        manager: &PackageManager,
    ) -> Result<Option<Self>> {
        if std::env::var_os("PARHELION_MAP_CACHE").is_some_and(|value| value == "off") {
            return Ok(None);
        }
        let Some(inputs) = crate::mod_family::build::map_source_identity(source) else {
            return Ok(None);
        };
        let Some(compiler) = parhelion_import::tiger::shader::compiler_identity() else {
            return Ok(None);
        };
        let bindings = manager
            .lookup
            .tag64_entries
            .iter()
            .map(|(&key, value)| (key, (value.hash32.0, value.reference.0)))
            .collect::<BTreeMap<_, _>>();
        let entries = manager
            .lookup
            .tag32_entries_by_pkg
            .iter()
            .map(|(&package, entries)| {
                (
                    package,
                    entries
                        .iter()
                        .map(|entry| {
                            (
                                entry.reference,
                                entry.file_type,
                                entry.file_subtype,
                                entry.file_size,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let key = hash(
            &json!({ "format": 2, "converter": parhelion_import::CONVERTER_REVISION,
            "reach": parhelion_import::halo_reach::REVISION, "inputs": inputs, "source": source,
            "templates": templates, "bindings": hash(&bindings)?, "entries": hash(&entries)?,
            "compiler": compiler, "compute": std::env::var("PARHELION_COMPUTE").unwrap_or_else(|_| "auto".into()) }),
        )?;
        let current = fs::read(root.join("current.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Pointer>(&bytes).ok());
        let previous = current.as_ref().and_then(|current| {
            Generation::directory(&root.join("generations"), &current.generation)
        });
        if let (Some(current), Some(generation)) = (&current, &previous)
            && current.key == key
        {
            match load(generation.path(), &key, manager) {
                Ok(converted) => {
                    return Ok(Some(Self {
                        root: root.into(),
                        key,
                        generation: previous.unwrap(),
                        _previous: None,
                        converted: Some(converted),
                    }));
                }
                Err(error) if parhelion_import::cancellation::is_cancelled(&error) => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        let unique = hash(&(
            key.as_str(),
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        ))?;
        let Some(generation) = Generation::directory(&root.join("generations"), &unique) else {
            return Ok(None);
        };
        Ok(Some(Self {
            root: root.into(),
            key,
            generation,
            _previous: previous,
            converted: None,
        }))
    }

    pub(super) fn folder(&self) -> &Path {
        self.generation.path()
    }

    pub(super) fn publish(&self, converted: &Converted, manager: &PackageManager) -> Result<()> {
        parhelion_import::cache::write_json(&self.folder().join("conversion.json"), converted)?;
        let files = files(self.folder())?;
        let native = native(self.folder(), &files, manager)?;
        parhelion_import::cache::write_json(
            &self.folder().join("cache.json"),
            &Manifest {
                key: self.key.clone(),
                files,
                native,
            },
        )?;
        let generation = self
            .folder()
            .file_name()
            .context("Map cache generation")?
            .to_str()
            .context("Map cache generation encoding")?
            .to_owned();
        parhelion_import::cache::write_json(
            &self.root.join("current.json"),
            &Pointer {
                key: self.key.clone(),
                generation,
            },
        )
    }

    pub(super) fn lease(self) -> Generation {
        self.generation
    }
}
