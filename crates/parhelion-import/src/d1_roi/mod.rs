//! Rise of Iron source intake. D1 layouts never pass through the D2 reader.
mod armor;
mod audio;
mod cloth;
mod codec;
mod dye;
mod expression;
mod icon;
mod layout;
mod localization;
mod material;
mod model;
mod native;
mod ornament;
mod package;
mod shader;
pub mod texture;
mod weapon;
pub use armor::Armor;
pub use icon::Icon;
pub use native::NativeOptions;
pub use weapon::Weapon;

use crate::tiger::{payload::Payload, reader::package_stamp};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tiger_pkg::{DestinyVersion, Package, TagHash, Version};

const ARMOR_TABLES: [(u32, &str); 5] = [
    (0x80801933, "Item Definitions"),
    (0x808034C7, "Item Strings"),
    (0x808034CB, "String Banks"),
    (0x808034D7, "Art Arrangements"),
    (0x80803AAA, "Entity Assignments"),
];

pub fn packages_path(path: &Path) -> Result<PathBuf> {
    let path = if path.join("packages").is_dir() {
        path.join("packages")
    } else {
        path.to_owned()
    };
    let path = path.canonicalize().context("Open Rise of Iron packages")?;
    ensure!(
        path.is_dir(),
        "Choose the Rise of Iron installation or its packages folder"
    );
    package_stamp(&path)?;
    Ok(path)
}

#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    pub source: String,
    pub native_conversion: bool,
    pub packages: PathBuf,
    pub stamp: String,
    pub package_count: usize,
    pub entry_count: usize,
    pub model_count: usize,
    pub builds: Vec<String>,
    pub excluded: Vec<PathBuf>,
    pub missing_armor_tables: Vec<String>,
    pub selected: Vec<package::Header>,
}

impl Inspection {
    pub fn require_armor_tables(&self) -> Result<()> {
        ensure!(
            self.missing_armor_tables.is_empty(),
            "Armor source is incomplete. Missing: {}",
            self.missing_armor_tables.join(", ")
        );
        Ok(())
    }
}

struct OpenPackage {
    header: package::Header,
    reader: std::sync::Arc<dyn Package>,
    blocks: Vec<package::Block>,
}

type BlockCache = BTreeMap<(u16, usize), Arc<Vec<u8>>>;

pub struct Source {
    report: Inspection,
    packages: BTreeMap<u16, OpenPackage>,
    codec: Mutex<Option<codec::Decoder>>,
    blocks: Mutex<BlockCache>,
    human_skeleton: Mutex<Option<u32>>,
}

#[derive(Debug)]
struct MissingTag(u32);
impl std::fmt::Display for MissingTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rise of Iron source is missing tag {:08X}", self.0)
    }
}
impl std::error::Error for MissingTag {}

impl Source {
    pub fn open(path: &Path, mut progress: impl FnMut(usize, usize)) -> Result<Self> {
        let root = packages_path(path)?;
        let stamp = package_stamp(&root)?;
        let mut paths = fs::read_dir(&root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.retain(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
        });
        paths.sort();
        let mut headers = BTreeMap::new();
        let mut selected = BTreeMap::<u16, package::Header>::new();
        let mut excluded = Vec::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut platform = None;
        for (index, path) in paths.iter().enumerate() {
            crate::cancellation::check()?;
            let header = package::Header::read(path)
                .with_context(|| format!("Inspect {}", path.display()))?;
            ensure!(
                platform.is_none_or(|value| value == header.platform),
                "Mixed Rise of Iron platforms"
            );
            platform = Some(header.platform);
            if header.language > 1 {
                excluded.push(path.clone());
            } else {
                ensure!(
                    identities.insert(header.identity()?),
                    "Duplicate package identity {:04X}, patch {}",
                    header.id,
                    header.patch
                );
                let replace = selected
                    .get(&header.id)
                    .map(|previous| header.newer_than(previous))
                    .transpose()?
                    .unwrap_or(true);
                if replace {
                    selected.insert(header.id, header.clone());
                }
            }
            ensure!(
                headers.insert(path.clone(), header).is_none(),
                "Duplicate source package"
            );
            progress(index + 1, paths.len());
        }
        ensure!(!selected.is_empty(), "No English Rise of Iron packages");
        let mut packages = BTreeMap::new();
        for (id, header) in &selected {
            let blocks = header.validate_blocks(&headers)?;
            let reader = DestinyVersion::DestinyRiseOfIron
                .open(header.path.to_str().context("Package path is not UTF-8")?)?;
            packages.insert(
                *id,
                OpenPackage {
                    header: header.clone(),
                    reader,
                    blocks,
                },
            );
        }
        ensure!(
            package_stamp(&root)? == stamp,
            "Rise of Iron packages changed during inspection"
        );
        let mut source = Self {
            report: Inspection {
                source: "d1_roi".into(),
                native_conversion: false,
                packages: root,
                stamp,
                package_count: packages.len(),
                entry_count: packages
                    .values()
                    .map(|package| package.reader.entries().len())
                    .sum(),
                model_count: 0,
                builds: selected
                    .values()
                    .map(|header| header.build.clone())
                    .collect(),
                excluded,
                missing_armor_tables: Vec::new(),
                selected: selected.into_values().collect(),
            },
            packages,
            codec: Mutex::new(None),
            blocks: Mutex::new(BTreeMap::new()),
            human_skeleton: Mutex::new(None),
        };
        source.report.builds.sort();
        source.report.builds.dedup();
        source.report.model_count = source.classes(0x80801AB5).len();
        source.report.missing_armor_tables = ARMOR_TABLES
            .iter()
            .filter(|(class, _)| source.classes(*class).is_empty())
            .map(|(_, name)| (*name).to_owned())
            .collect();
        Ok(source)
    }

    pub fn report(&self) -> &Inspection {
        &self.report
    }

    pub fn classes(&self, class: u32) -> Vec<u32> {
        self.packages
            .iter()
            .flat_map(|(&id, package)| {
                package
                    .reader
                    .entries()
                    .iter()
                    .enumerate()
                    .filter(move |(_, entry)| entry.reference == class)
                    .map(move |(index, _)| 0x80800000 + (u32::from(id) << 13) + index as u32)
            })
            .collect()
    }

    fn package(&self, tag: u32) -> Result<&OpenPackage> {
        ensure!(tag >= 0x80800000, "Invalid Rise of Iron tag {tag:08X}");
        self.packages
            .get(&TagHash(tag).pkg_id())
            .ok_or_else(|| MissingTag(tag).into())
    }

    pub fn reference(&self, tag: u32) -> Result<u32> {
        Ok(self
            .package(tag)?
            .reader
            .entry(TagHash(tag).entry_index() as usize)
            .ok_or(MissingTag(tag))?
            .reference)
    }

    pub fn read(&self, tag: u32) -> Result<Payload> {
        crate::cancellation::check()?;
        let package = self.package(tag)?;
        let entry = package
            .reader
            .entry(TagHash(tag).entry_index() as usize)
            .ok_or(MissingTag(tag))?;
        let mut result = Vec::with_capacity(entry.file_size as usize);
        let mut block = entry.starting_block as usize;
        let mut skip = entry.starting_block_offset as usize;
        while result.len() < entry.file_size as usize {
            ensure!(
                block < package.header.block_count as usize,
                "Block outside package for {tag:08X}"
            );
            let bytes = self
                .block(package, block)
                .with_context(|| format!("Read Rise of Iron tag {tag:08X}, block {block}"))?;
            let bytes = bytes
                .get(skip..)
                .context("Entry offset exceeds decoded block")?;
            let count = bytes.len().min(entry.file_size as usize - result.len());
            ensure!(count > 0, "Empty block for {tag:08X}");
            result.extend_from_slice(&bytes[..count]);
            block += 1;
            skip = 0;
        }
        Ok(Payload(result))
    }

    fn block(&self, package: &OpenPackage, index: usize) -> Result<Arc<Vec<u8>>> {
        let key = (package.header.id, index);
        let mut cache = crate::cancellation::mutex(&self.blocks)?;
        if let Some(bytes) = cache.get(&key) {
            return Ok(Arc::clone(bytes));
        }
        let block = package.blocks.get(index).context("Block outside package")?;
        let stored = block.read()?;
        let bytes = if block.compressed {
            let mut codec = crate::cancellation::mutex(&self.codec)?;
            if codec.is_none() {
                *codec = Some(codec::Decoder::open()?);
            }
            codec
                .as_ref()
                .context("Rise of Iron decoder")?
                .decode(&stored)?
        } else {
            stored
        };
        let bytes = Arc::new(bytes);
        if cache.len() >= 64 {
            cache.pop_first();
        }
        cache.insert(key, Arc::clone(&bytes));
        Ok(bytes)
    }

    pub fn is_missing_tag_error(&self, error: &anyhow::Error) -> bool {
        error.downcast_ref::<MissingTag>().is_some()
    }

    fn unchanged(&self) -> Result<()> {
        ensure!(
            package_stamp(&self.report.packages)? == self.report.stamp,
            "Rise of Iron packages changed. Refresh the source first"
        );
        Ok(())
    }

    fn output(&self, output: &Path) -> Result<PathBuf> {
        self.unchanged()?;
        let root = if self
            .report
            .packages
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("packages"))
        {
            self.report.packages.parent().context("Source root")?
        } else {
            &self.report.packages
        };
        let output = crate::io::outside(output, root)?;
        ensure!(!output.exists(), "Use a fresh output directory");
        Ok(output)
    }

    pub fn write_inspection(&self, output: &Path) -> Result<PathBuf> {
        let output = self.output(output)?;
        fs::create_dir_all(&output)?;
        let path = output.join("inspection.json");
        crate::io::write_json(&path, &serde_json::to_value(&self.report)?)?;
        Ok(path)
    }
}
