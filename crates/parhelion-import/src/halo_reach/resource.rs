use super::cache::{Cache, Tag, file_range};
use anyhow::{Context, Result, ensure};
use flate2::read::DeflateDecoder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_PAGE: usize = 512 * 1024 * 1024;
const CACHE_BYTES: usize = 384 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Resource {
    pub owner: u32,
    pub handle: u32,
    pub cache: String,
    pub file_offset: u64,
    pub compressed_bytes: usize,
    pub page_bytes: usize,
    pub page_offset: usize,
    pub fixup_offset: usize,
    pub fixup_size: usize,
    pub fixups: Vec<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Fingerprint {
    pub bytes: u64,
    pub sha256: String,
}

pub fn fingerprint(path: &Path) -> Result<Fingerprint> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    let mut sha = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
    }
    Ok(Fingerprint {
        bytes,
        sha256: hex::encode(sha.finalize()),
    })
}

#[derive(Default)]
pub struct Pages {
    pages: VecDeque<(PathBuf, u64, usize, Arc<Vec<u8>>)>,
    pub inputs: BTreeMap<PathBuf, Fingerprint>,
    pub resources: Vec<Resource>,
    stamps: BTreeMap<PathBuf, (u64, std::time::SystemTime)>,
}

impl Pages {
    pub fn record(&mut self, path: &Path) -> Result<()> {
        if !self.inputs.contains_key(path) {
            let metadata = path.metadata()?;
            self.stamps
                .insert(path.to_owned(), (metadata.len(), metadata.modified()?));
            self.inputs.insert(path.to_owned(), fingerprint(path)?);
        }
        Ok(())
    }
    pub fn verify_inputs(&self) -> Result<()> {
        for (path, (length, modified)) in &self.stamps {
            let metadata = path.metadata()?;
            ensure!(
                metadata.len() == *length && metadata.modified()? == *modified,
                "Reach source changed during conversion: {}",
                path.display()
            );
        }
        Ok(())
    }
    pub fn read(&mut self, cache: &Cache, resource: &Resource) -> Result<Arc<Vec<u8>>> {
        let directory = cache
            .path
            .parent()
            .context("Cache directory")?
            .canonicalize()?;
        let path = directory.join(&resource.cache).canonicalize()?;
        ensure!(
            path.starts_with(&directory),
            "Shared cache resolves outside the source maps directory"
        );
        if let Some(i) = self.pages.iter().position(|(p, o, size, _)| {
            p == &path && *o == resource.file_offset && *size == resource.page_bytes
        }) {
            let entry = self.pages.remove(i).unwrap();
            let bytes = entry.3.clone();
            self.pages.push_back(entry);
            return Ok(bytes);
        }
        self.record(&path)?;
        let mut file = File::open(&path)?;
        let packed = file_range(&mut file, resource.file_offset, resource.compressed_bytes)?;
        let data = if resource.compressed_bytes == resource.page_bytes {
            packed
        } else {
            let mut decoder = DeflateDecoder::new(packed.as_slice());
            let mut out = Vec::with_capacity(resource.page_bytes);
            decoder
                .by_ref()
                .take(resource.page_bytes as u64 + 1)
                .read_to_end(&mut out)?;
            ensure!(
                out.len() == resource.page_bytes,
                "Decoded resource page size differs"
            );
            // Reaching StreamEnd is checked by one additional read, bounded above.
            let mut tail = [0; 1];
            ensure!(
                decoder.read(&mut tail)? == 0 && decoder.total_in() as usize == packed.len(),
                "Incomplete page or trailing compressed bytes"
            );
            out
        };
        ensure!(
            data.len() == resource.page_bytes,
            "Resource page length differs"
        );
        while self.pages.iter().map(|p| p.3.len()).sum::<usize>() + data.len() > CACHE_BYTES
            && !self.pages.is_empty()
        {
            self.pages.pop_front();
        }
        let data = Arc::new(data);
        self.pages.push_back((
            path,
            resource.file_offset,
            resource.page_bytes,
            data.clone(),
        ));
        Ok(data)
    }
    pub fn bytes(
        &mut self,
        cache: &Cache,
        resource: &Resource,
        offset: usize,
        length: usize,
    ) -> Result<Vec<u8>> {
        let start = resource
            .page_offset
            .checked_add(offset)
            .context("Resource extent overflow")?;
        ensure!(
            start <= resource.page_bytes && length <= resource.page_bytes - start,
            "Asset extent outside resource page"
        );
        let data = self.read(cache, resource)?;
        if !self.resources.iter().any(|r| {
            r.owner == resource.owner
                && r.handle == resource.handle
                && r.cache == resource.cache
                && r.file_offset == resource.file_offset
                && r.page_offset == resource.page_offset
        }) {
            self.resources.push(resource.clone());
        }
        Ok(data[start..start + length].to_vec())
    }
}

impl Cache {
    pub fn resource(&self, owner: &Tag, handle: u32) -> Result<Resource> {
        ensure!(handle != u32::MAX, "Null resource handle");
        let zone = self.only("zone")?.address()?;
        let play = self.only("play")?.offset.unwrap_or(zone + 28);
        let entries = self.block(zone + 100, 64)?;
        let entry = *entries
            .get((handle & 65535) as usize)
            .context("Resource outside gestalt")?;
        ensure!(
            self.reference(entry, None)?
                .is_some_and(|t| t.datum == owner.datum),
            "Resource owner differs from tag"
        );
        let segment = *self
            .block(play + 60, 16)?
            .get(usize::try_from(self.i16(entry + 34)?)?)
            .context("Resource has no segment")?;
        let pages = self.block(play + 24, 88)?;
        let mut secondary = self.i16(segment + 2)? >= 0;
        let mut page = *pages
            .get(usize::try_from(
                self.i16(segment + if secondary { 2 } else { 0 })?,
            )?)
            .context("Resource page outside table")?;
        if self.i32(page + 8)? < 0 || self.i32(page + 12)? == 0 {
            secondary = false;
            page = *pages
                .get(usize::try_from(self.i16(segment)?)?)
                .context("Primary page outside table")?;
        }
        let page_offset = usize::try_from(self.i32(segment + if secondary { 8 } else { 4 })?)?;
        let index = self.i16(page + 4)?;
        let cache = if index < 0 {
            self.path
                .file_name()
                .context("Cache filename")?
                .to_str()
                .context("UTF-8 cache filename")?
                .to_owned()
        } else {
            let shared = self.block(play + 12, 264)?;
            let name = self.string(
                *shared
                    .get(index as usize)
                    .context("Shared cache outside table")?,
                256,
            )?;
            let name = name
                .rsplit(['/', '\\'])
                .next()
                .context("Shared cache filename")?;
            ensure!(
                !name.is_empty() && name.ends_with(".map") && name != ".map" && !name.contains(':'),
                "Invalid shared cache name"
            );
            name.to_owned()
        };
        let compressed_bytes = usize::try_from(self.i32(page + 12)?)?;
        let page_bytes = usize::try_from(self.i32(page + 16)?)?;
        ensure!(
            compressed_bytes > 0
                && compressed_bytes <= MAX_PAGE
                && page_bytes > 0
                && page_bytes <= MAX_PAGE
                && page_offset < page_bytes,
            "Invalid or oversized resource page"
        );
        let directory = self.path.parent().context("Cache parent")?.canonicalize()?;
        let target = directory.join(&cache).canonicalize()?;
        ensure!(
            target.starts_with(&directory),
            "Shared cache resolves outside the source maps directory"
        );
        let mut file = File::open(&target)
            .with_context(|| format!("Opening resource cache {}", target.display()))?;
        let head = file_range(&mut file, 0, 40960)?;
        ensure!(
            &head[..4] == b"daeh"
                && &head[40956..] == b"toof"
                && u32::from_le_bytes(head[4..8].try_into().unwrap()) == 13
                && &head[160..160 + super::cache::BUILD.len()] == super::cache::BUILD.as_bytes(),
            "Shared resource cache profile differs"
        );
        let data_base = u32::from_le_bytes(head[1232..1236].try_into().unwrap());
        let file_offset = u64::from(data_base) + u64::try_from(self.i32(page + 8)?)?;
        ensure!(
            file_offset <= file.metadata()?.len()
                && compressed_bytes as u64 <= file.metadata()?.len() - file_offset,
            "Resource outside shared cache"
        );
        let fixup_offset = usize::try_from(self.i32(entry + 20)?)?;
        let fixup_size = usize::try_from(self.i32(entry + 24)?)?;
        let total = usize::try_from(self.i32(zone + 328)?)?;
        ensure!(
            fixup_offset <= total && fixup_size <= total - fixup_offset,
            "Fixup outside owning data"
        );
        let fixup_offset = self
            .expand(self.u32(zone + 340)?)?
            .checked_add(fixup_offset)
            .context("Fixup address overflow")?;
        self.meta(fixup_offset, fixup_size)?;
        let fixups = self
            .block(entry + 40, 8)?
            .into_iter()
            .map(|r| self.u32(r + 4).map(|v| (v & 0x0fff_ffff) as usize))
            .collect::<Result<Vec<_>>>()?;
        Ok(Resource {
            owner: owner.datum,
            handle,
            cache,
            file_offset,
            compressed_bytes,
            page_bytes,
            page_offset,
            fixup_offset,
            fixup_size,
            fixups,
        })
    }
}
