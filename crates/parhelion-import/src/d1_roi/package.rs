use super::*;
use sha1::{Digest, Sha1};
use std::io::{Read, Seek, SeekFrom};

pub(super) struct Block {
    path: PathBuf,
    offset: u64,
    bytes: usize,
    digest: [u8; 20],
    pub compressed: bool,
}

impl Block {
    pub fn read(&self) -> Result<Vec<u8>> {
        let mut file = fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; self.bytes];
        file.read_exact(&mut bytes)?;
        ensure!(
            Sha1::digest(&bytes).as_slice() == self.digest,
            "Source block checksum failed in {} at {:X}",
            self.path.display(),
            self.offset
        );
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Header {
    pub path: PathBuf,
    pub id: u16,
    pub platform: u16,
    pub language: u16,
    pub patch: u16,
    pub build: String,
    pub build_id: u32,
    pub build_time: u64,
    pub bytes: u64,
    pub block_count: u32,
    block_offset: u32,
}

impl Header {
    pub fn read(path: &Path) -> Result<Self> {
        let mut file = fs::File::open(path)?;
        let bytes = file.metadata()?.len();
        let mut raw = vec![0; 0x140];
        file.read_exact(&mut raw)?;
        let header = Payload(raw);
        ensure!(
            header.u16(0)? == 24,
            "Expected Rise of Iron package version 24"
        );
        let platform = header.u16(2)?;
        ensure!(
            matches!(platform, 7 | 8),
            "Unsupported Rise of Iron platform {platform}"
        );
        ensure!(
            u64::from(header.u32(0x13C)?) == bytes,
            "Package size does not match its header"
        );
        let patch = header.u16(0x20)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("Package filename")?;
        let (_, suffix) = stem
            .rsplit_once('_')
            .context("Package filename has no patch suffix")?;
        ensure!(
            suffix.parse::<u16>()? == patch,
            "Package filename and patch disagree"
        );
        let language = header.u16(0x22)?;
        ensure!(language <= 13, "Unknown source language {language}");
        ensure!(
            header.u32(0xB4)? <= 8192,
            "Package exceeds the tag index range"
        );
        for (offset, stride) in [(0xB4, 16u64), (0xD0, 32), (0xEC, 68)] {
            let count = u64::from(header.u32(offset)?);
            let start = u64::from(header.u32(offset + 4)?);
            ensure!(
                count == 0 || start >= 0x140 && start + count * stride <= bytes,
                "Package table exceeds file at {offset:X}"
            );
        }
        let build = String::from_utf8_lossy(&header.0[0x24..0xA4])
            .trim_end_matches('\0')
            .to_owned();
        Ok(Self {
            path: path.to_owned(),
            id: header.u16(4)?,
            platform,
            language,
            patch,
            build,
            build_id: header.u32(0x18)?,
            build_time: header.u64(0x10)?,
            bytes,
            block_count: header.u32(0xD0)?,
            block_offset: header.u32(0xD4)?,
        })
    }

    fn series(&self) -> Result<(&str, Option<u16>)> {
        let stem = self
            .path
            .file_stem()
            .and_then(|value| value.to_str())
            .context("Package filename")?;
        let (base, _) = stem.rsplit_once('_').context("Package patch suffix")?;
        if let Some((family, generation)) = base.rsplit_once("_unp")
            && !generation.is_empty()
            && generation.bytes().all(|byte| byte.is_ascii_digit())
        {
            ensure!(self.patch == 0, "Unpatchable package has a nonzero patch");
            return Ok((family, Some(generation.parse()?)));
        }
        Ok((base, None))
    }

    pub(super) fn identity(&self) -> Result<(u16, u16, u16, Option<u16>)> {
        Ok((self.id, self.language, self.patch, self.series()?.1))
    }

    pub(super) fn newer_than(&self, previous: &Self) -> Result<bool> {
        ensure!(
            self.language == previous.language,
            "Ambiguous language for package {:04X}",
            self.id
        );
        let (family, generation) = self.series()?;
        let (old_family, old_generation) = previous.series()?;
        ensure!(
            family == old_family,
            "Unrelated files share package identity {:04X}",
            self.id
        );
        match (generation, old_generation) {
            (Some(current), Some(old)) if current != old => {
                let newer = current > old;
                ensure!(
                    if newer {
                        self.build_id > previous.build_id && self.build_time > previous.build_time
                    } else {
                        self.build_id < previous.build_id && self.build_time < previous.build_time
                    },
                    "Unpatchable generation and build order disagree for package {:04X}",
                    self.id
                );
                Ok(newer)
            }
            (None, None) => Ok(self.patch > previous.patch),
            _ => anyhow::bail!("Ambiguous package generation {:04X}", self.id),
        }
    }

    pub(super) fn validate_blocks(
        &self,
        headers: &BTreeMap<PathBuf, Header>,
    ) -> Result<Vec<Block>> {
        let mut file = fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(u64::from(self.block_offset)))?;
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("Package filename")?;
        let (base, _) = stem.rsplit_once('_').context("Patch filename")?;
        let mut result = Vec::new();
        for index in 0..self.block_count {
            let mut bytes = vec![0; 32];
            file.read_exact(&mut bytes)?;
            let block = Payload(bytes);
            let patch = block.u16(8)?;
            let path = self.path.with_file_name(format!("{base}_{patch}.pkg"));
            let storage = headers
                .get(&path)
                .with_context(|| format!("Missing block patch {}", path.display()))?;
            ensure!(
                storage.id == self.id
                    && storage.platform == self.platform
                    && storage.language == self.language,
                "Block patch belongs to another source package"
            );
            let offset = u64::from(block.u32(0)?);
            let size = u64::from(block.u32(4)?);
            ensure!(
                size > 0 && size <= 0x40000 && offset + size <= storage.bytes,
                "Block {index} exceeds {}",
                path.display()
            );
            ensure!(
                block.u16(10)? & !1 == 0,
                "Unsupported block flags in {}",
                self.path.display()
            );
            result.push(Block {
                path,
                offset,
                bytes: size as usize,
                digest: block.bytes(12)?,
                compressed: block.u16(10)? & 1 != 0,
            });
        }
        Ok(result)
    }
}
