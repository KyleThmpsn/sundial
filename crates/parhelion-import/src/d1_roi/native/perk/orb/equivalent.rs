//! Compare complete payloads, allowing only declared resource identities to differ.
use super::*;
use crate::d1_roi::texture::{self, Descriptor};
use std::sync::OnceLock;

// Source build 56578.17.04.12.1251.evil .schmr declarations. Each row is a class,
// byte stride and expanded fields. Code 3 is a typed relative pointer, 4 a tag,
// 9 an owner/offset reference and 10 an owner/offset/class reference. Other bytes
// stay exact, including unknown fields, paths, array ordering and scalar values.
type Declaration = (u32, usize, Vec<(usize, u32)>);
static DECLARATIONS: OnceLock<Vec<Declaration>> = OnceLock::new();

fn declaration(class: u32) -> Result<&'static Declaration> {
    DECLARATIONS
        .get_or_init(|| {
            serde_json::from_str(include_str!("schema.json"))
                .expect("embedded source orb declarations")
        })
        .iter()
        .find(|row| row.0 == class)
        .with_context(|| format!("Source orb comparison needs declaration {class:08X}"))
}

fn references(data: &Payload, class: u32) -> Result<BTreeMap<usize, u32>> {
    let mut pending = vec![(0usize, class, 1usize)];
    let mut visited = BTreeSet::new();
    let mut result = BTreeMap::new();
    while let Some((base, class, count)) = pending.pop() {
        if !visited.insert((base, class, count)) {
            continue;
        }
        let (_, size, fields) = declaration(class)?;
        ensure!(
            visited.len() <= 8192 && count <= 4096 && *size > 0,
            "Source orb allocation traversal exceeds its limits"
        );
        ensure!(
            count
                .checked_mul(*size)
                .and_then(|n| base.checked_add(n))
                .is_some_and(|end| end <= data.0.len()),
            "Source orb allocation is truncated"
        );
        for row in 0..count {
            for &(offset, code) in fields {
                let width = match code {
                    3 => 8,
                    4 => 4,
                    9 => 16,
                    10 => 24,
                    _ => anyhow::bail!("Invalid source orb declaration"),
                };
                ensure!(
                    offset.checked_add(width).is_some_and(|end| end <= *size),
                    "Source orb field exceeds its declaration"
                );
                let at = base + row * size + offset;
                if code != 3 {
                    if let Some(previous) = result.insert(at, code) {
                        ensure!(previous == code, "Source orb resource fields overlap");
                    }
                    continue;
                }
                if data.u64(at)? == 0 {
                    continue;
                }
                let target = data.pointer(at)?;
                let marker = data.u32(
                    target
                        .checked_sub(4)
                        .context("Source orb pointer has no class")?,
                )?;
                if marker == 0x80800184 {
                    let count = data.u64(target)?;
                    ensure!(
                        at >= 8 && data.u64(at - 8)? == count && data.u32(target + 12)? == 0,
                        "Source orb array header disagrees with its descriptor"
                    );
                    pending.push((target + 16, data.u32(target + 8)?, usize::try_from(count)?));
                } else {
                    pending.push((target, marker, 1));
                }
            }
        }
    }
    Ok(result)
}

struct Comparison<'a> {
    source: &'a Source,
    complete: BTreeSet<(u32, u32)>,
    active: BTreeSet<(u32, u32)>,
}

impl Comparison<'_> {
    fn texture(&self, tag: u32, header: &Payload) -> Result<Vec<u8>> {
        let mut assets = BTreeMap::new();
        let description = self.source.texture(tag, &mut assets)?;
        let mut raw = Vec::new();
        for field in ["high_mips", "data"] {
            if let Some(tag) = description[field].as_u64() {
                raw.extend_from_slice(
                    &assets
                        .get(&u32::try_from(tag)?)
                        .context("Source orb texture data is missing")?
                        .0,
                );
            }
        }
        texture::decode(
            &Descriptor {
                format: header.u32(0)?,
                tile_mode: header.u32(4)?,
                width: u32::from(header.u16(0x30)?),
                height: u32::from(header.u16(0x32)?),
                depth: u32::from(header.u16(0x34)?),
                layers: u32::from(header.u16(0x36)?),
                mips: u32::from(header.u8(0x39)?),
            },
            &raw,
        )
    }

    fn compare(&mut self, left: u32, right: u32) -> Result<()> {
        if left == right || self.complete.contains(&(left, right)) {
            return Ok(());
        }
        crate::cancellation::check()?;
        ensure!(
            self.active.len() < 24
                && self.complete.len() < 512
                && self.active.insert((left, right)),
            "Source orb dependencies contain a cycle or exceed limits"
        );
        let a = self.source.read(left)?;
        let b = self.source.read(right)?;
        ensure!(
            a.0.len() == b.0.len(),
            "Source pickup differs from the configured default"
        );
        if a.0.len() == 68 && a.u32(0x2C)? == 0xBEEFCAFE {
            // The source texture reader checks entry types, subtypes and streamed buffers.
            // Compare every authored mip, excluding Xbox tiling padding.
            ensure!(
                a.0 == b.0 && self.texture(left, &a)? == self.texture(right, &b)?,
                "Source pickup texture differs from the configured default"
            );
        } else {
            let class = self.source.reference(left)?;
            ensure!(
                class == self.source.reference(right)?,
                "Source orb dependency classes differ"
            );
            if a.0 != b.0 {
                ensure!(
                    a.0.len() <= 16 * 1024 * 1024,
                    "Source orb resource exceeds comparison limit"
                );
                let fields = references(&a, class)?;
                ensure!(
                    fields == references(&b, class)?,
                    "Source orb reference layouts differ"
                );
                let mut normalized = a.0.clone();
                for (at, code) in fields {
                    let (x, y) = (a.u32(at)?, b.u32(at)?);
                    if x == y {
                        continue;
                    }
                    if (x, y) == (left, right) {
                        ensure!(
                            class == 0x80800861
                                && code == 10
                                && a.u32(at + 4)? == 0
                                && a.u32(at + 20)? == 0,
                            "Source orb self-reference is not a typed owner field"
                        );
                        let size = declaration(a.u32(at + 16)?)?.1;
                        ensure!(
                            usize::try_from(a.u64(at + 8)?)?
                                .checked_add(size)
                                .is_some_and(|end| end <= a.0.len()),
                            "Source orb self-reference exceeds owner"
                        );
                    } else {
                        self.compare(x, y)?;
                    }
                    normalized[at..at + 4].copy_from_slice(&y.to_le_bytes());
                }
                ensure!(
                    normalized == b.0,
                    "Source pickup has non-resource differences from its configured default"
                );
            }
        }
        self.active.remove(&(left, right));
        self.complete.insert((left, right));
        Ok(())
    }
}

pub(super) fn check(source: &Source, selected: u32, default: u32) -> Result<()> {
    ensure!(
        source.reference(selected)? == 0x80800734,
        "Source orb selection is not a pattern"
    );
    Comparison {
        source,
        complete: BTreeSet::new(),
        active: BTreeSet::new(),
    }
    .compare(selected, default)
}
