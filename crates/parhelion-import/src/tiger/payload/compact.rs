//! Rebuild a payload from its root record, keeping only what the root's pointers reach.
//!
//! Editors that clone a payload and append replacement arrays leave the replaced arrays behind
//! as unreachable bytes. This copies the root record, then every object and array its
//! self-relative pointers reach, depth first, using the definition table's native layout for
//! each class and its bases. Layout code 3 reaches an object, which keeps its class word before
//! its base and a stride of the class size, or an array, which keeps its allocation marker,
//! count and element class. Code 1 reaches string bytes, copied through their terminator.
//! Codes 4 and 11 hold tag and name words in place. A pointer whose target is neither, or any
//! other layout code, is refused rather than guessed.
use super::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

const ARRAY_MARKER: u32 = 0x8080_9FBD;
const MAX_DEPTH: usize = 32;

/// A rebuilt payload and the end of every copied object's subtree, by the object's new base.
pub struct Compacted {
    pub payload: Payload,
    pub extents: BTreeMap<usize, usize>,
}

fn layout(class: u32) -> Result<(usize, Vec<(usize, u32)>)> {
    sundial::package_authoring::runtime::native_pointer_layout(class)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("Class {class:08X} is outside the definition table"))
}

struct Rebuild<'a> {
    source: &'a Payload,
    out: Vec<u8>,
    /// Old target to new target, so a shared target is copied once.
    copied: BTreeMap<usize, usize>,
    extents: BTreeMap<usize, usize>,
}

impl Rebuild<'_> {
    /// Copy what each pointer of the `class` record at `old` reaches and retarget the copy at `new`.
    fn pointers(&mut self, old: usize, new: usize, class: u32, depth: usize) -> Result<()> {
        ensure!(
            depth < MAX_DEPTH,
            "Payload nesting exceeds {MAX_DEPTH} levels"
        );
        for (offset, code) in layout(class)?.1 {
            match code {
                1 | 3 => {}
                4 | 11 => continue,
                _ => anyhow::bail!(
                    "Layout code {code} at {class:08X} +{offset:X} has no known storage"
                ),
            }
            if self.source.u64(old + offset)? == 0 {
                continue;
            }
            let target = self.source.pointer(old + offset)?;
            let moved = match self.copied.get(&target) {
                Some(&moved) => moved,
                None if code == 1 => self.string(target)?,
                None => self.copy(target, depth + 1)?,
            };
            let delta = moved as i64 - (new + offset) as i64;
            self.out[new + offset..new + offset + 8].copy_from_slice(&delta.to_le_bytes());
        }
        Ok(())
    }

    fn string(&mut self, target: usize) -> Result<usize> {
        let length = self.source.0[target..]
            .iter()
            .position(|&byte| byte == 0)
            .with_context(|| format!("String at {target:X} is unterminated"))?;
        let start = (self.out.len() + 7) & !7;
        self.out.resize(start, 0);
        self.out
            .extend_from_slice(&self.source.0[target..=target + length]);
        self.copied.insert(target, start);
        Ok(start)
    }

    fn copy(&mut self, target: usize, depth: usize) -> Result<usize> {
        ensure!(target >= 4, "Pointer target {target:X} has no header");
        let source = self.source;
        if source.u32(target - 4)? == ARRAY_MARKER {
            let count = usize::try_from(source.u64(target)?)?;
            let class = u32::try_from(source.u64(target + 8)?)
                .with_context(|| format!("Array at {target:X} has an invalid element class"))?;
            let stride = layout(class)?.0;
            let end = count
                .checked_mul(stride)
                .and_then(|bytes| bytes.checked_add(target + 16))
                .filter(|&end| end <= source.0.len())
                .with_context(|| format!("Array at {target:X} exceeds the payload"))?;
            let header = (self.out.len() + 19) & !15;
            self.out.resize(header - 4, 0);
            self.out.extend(ARRAY_MARKER.to_le_bytes());
            self.out.extend_from_slice(&source.0[target..end]);
            self.copied.insert(target, header);
            for element in 0..count {
                let at = 16 + element * stride;
                self.pointers(target + at, header + at, class, depth)?;
            }
            Ok(header)
        } else {
            let class = source.u32(target - 4)?;
            let size = layout(class)
                .with_context(|| {
                    format!("Pointer target {target:X} is neither an array nor an object")
                })?
                .0;
            let end = target
                .checked_add(size)
                .filter(|&end| end <= source.0.len())
                .with_context(|| format!("Object {class:08X} at {target:X} exceeds the payload"))?;
            let base = (self.out.len() + 11) & !7;
            self.out.resize(base - 4, 0);
            self.out.extend(class.to_le_bytes());
            self.out.extend_from_slice(&source.0[target..end]);
            self.copied.insert(target, base);
            self.pointers(target, base, class, depth)?;
            self.extents.insert(base, self.out.len());
            Ok(base)
        }
    }
}

/// Rebuild `source`, whose root record has class `root`, from what that record reaches.
pub fn compact(source: &Payload, root: u32) -> Result<Compacted> {
    let size = layout(root)?.0;
    ensure!(
        source.u64(0)? == source.0.len() as u64 && size >= 8 && size <= source.0.len(),
        "Payload envelope differs from its {root:08X} root"
    );
    let mut rebuild = Rebuild {
        source,
        out: source.0[..size].to_vec(),
        copied: BTreeMap::new(),
        extents: BTreeMap::new(),
    };
    rebuild.pointers(0, 0, root, 0)?;
    let mut out = rebuild.out;
    let length = out.len() as u64;
    out[..8].copy_from_slice(&length.to_le_bytes());
    Ok(Compacted {
        payload: Payload(out),
        extents: rebuild.extents,
    })
}
