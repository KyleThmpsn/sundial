use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tiger_pkg::{DestinyVersion, GameVersion, MarathonVersion, PackageManager, TagHash};

// Resolve junctions in the existing ancestor before creating any directories.
pub fn resolved(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    let mut tail = vec![];
    while !ancestor.exists() {
        tail.push(
            ancestor
                .file_name()
                .context("invalid output path")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("no output ancestor")?;
    }
    let mut result = ancestor.canonicalize()?;
    for part in tail.into_iter().rev() {
        result.push(part)
    }
    Ok(result)
}
pub fn outside(output: &Path, source: &Path) -> Result<PathBuf> {
    let output = resolved(output)?;
    let source = resolved(source)?;
    ensure!(
        !output.starts_with(&source),
        "output must be outside source tree: {}",
        source.display()
    );
    Ok(output)
}
pub struct Reader {
    pub manager: PackageManager,
    pub output: PathBuf,
    cache: BTreeMap<u32, Arc<Payload>>,
    tags: BTreeMap<String, Value>,
    packages: PathBuf,
    version: String,
    record_reads: bool,
}
impl Reader {
    pub fn marathon(packages: &Path, output: &Path) -> Result<Self> {
        let packages = packages.canonicalize()?;
        let output = outside(output, packages.parent().context("source has no parent")?)?;
        let manager = PackageManager::new(
            &packages,
            GameVersion::Marathon(MarathonVersion::Marathon),
            None,
        )?;
        fs::create_dir_all(output.join("raw"))?;
        Ok(Self {
            manager,
            output,
            cache: BTreeMap::new(),
            tags: BTreeMap::new(),
            packages,
            version: "marathon".into(),
            record_reads: true,
        })
    }
    pub fn new(packages: &Path, output: &Path, modern: bool) -> Result<Self> {
        let packages = packages.canonicalize()?;
        let output = outside(output, packages.parent().context("source has no parent")?)?;
        let version = if modern {
            DestinyVersion::Destiny2TheEdgeOfFate
        } else {
            DestinyVersion::Destiny2Shadowkeep
        };
        let manager = PackageManager::new(&packages, GameVersion::Destiny(version), None)?;
        fs::create_dir_all(output.join("raw"))?;
        Ok(Self {
            manager,
            output,
            cache: BTreeMap::new(),
            tags: BTreeMap::new(),
            packages,
            version: if modern { "modern" } else { "shadowkeep" }.into(),
            record_reads: true,
        })
    }
    /// Browsing needs cached reads, but not thousands of raw model-export files.
    pub fn discovery(packages: &Path, output: &Path, modern: bool) -> Result<Self> {
        let mut reader = Self::new(packages, output, modern)?;
        reader.record_reads = false;
        Ok(reader)
    }
    pub fn tag(&mut self, tag: u32, class: Option<u32>) -> Result<Arc<Payload>> {
        let entry = self
            .manager
            .get_entry(TagHash(tag))
            .with_context(|| format!("missing tag {tag:08X}"))?;
        if let Some(c) = class {
            ensure!(entry.reference == c, "unexpected class for {tag:08X}")
        }
        if let Some(data) = self.cache.get(&tag) {
            return Ok(data.clone());
        }
        let data = self
            .manager
            .read_tag(TagHash(tag))
            .with_context(|| format!("reading {tag:08X}"))?;
        if self.record_reads {
            let path = self.output.join("raw").join(format!("{tag:08X}.bin"));
            fs::write(&path, &data)?;
            self.tags.insert(format!("{tag:08X}"),json!({"tag":tag,"reference":entry.reference,"type":entry.file_type,"subtype":entry.file_subtype,"size":data.len(),"path":path}));
        }
        let data = Arc::new(Payload(data));
        self.cache.insert(tag, data.clone());
        Ok(data)
    }
    pub fn reference(&self, tag: u32) -> Result<u32> {
        Ok(self
            .manager
            .get_entry(TagHash(tag))
            .with_context(|| format!("missing entry {tag:08X}"))?
            .reference)
    }
    /// Start another independent export without reopening every package index.
    pub(crate) fn begin_export(&mut self, output: &Path) -> Result<()> {
        ensure!(self.record_reads, "Discovery readers do not export assets");
        let output = outside(output, self.packages.parent().context("package parent")?)?;
        fs::create_dir_all(output.join("raw"))?;
        self.output = output;
        self.cache.clear();
        self.tags.clear();
        Ok(())
    }
    pub fn ref64(&self, p: &Payload, o: usize) -> Result<u32> {
        let tag = p.u32(o)?;
        if tag != u32::MAX || matches!(p.u32(o + 4)?, 1 | 2) {
            return Ok(tag);
        }
        Ok(self
            .manager
            .lookup
            .tag64_entries
            .get(&p.u64(o + 8)?)
            .context("missing 64-bit tag")?
            .hash32
            .0)
    }
    pub fn classes(&self, c: u32) -> Vec<u32> {
        self.manager
            .get_all_by_reference(c)
            .iter()
            .map(|(t, _)| t.0)
            .collect()
    }
    pub fn finish(&self) -> Result<()> {
        write_json(
            &self.output.join("source-manifest.json"),
            &json!({"packages":self.packages,"version":self.version,"tags":self.tags}),
        )
    }
}
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_source_before_creation() {
        let t = tempfile::tempdir().unwrap();
        let child = t.path().join("not-created/raw");
        assert!(outside(&child, t.path()).is_err());
        assert!(!child.exists());
    }
}
