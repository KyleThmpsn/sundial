//! Rise of Iron feedback resources (`80802637`) become private native `80803EB0` resources.
//!
//! Both builds store the same fixed fields, programs and member rows. Translation copies the
//! fixed region and re-appends each array named by a fixed-region descriptor, in descriptor
//! order, with the native array marker and row class. Applied to Firefly's query and profile
//! feedback (`80AAC391`, `80AAC651`) and their four members, this rebuild reproduces stock native
//! `80C23C06`, `80C1B696`, `80C23C08`, `80C23C07`, `80C23A42` and `80C23A40` byte for byte
//! (`examples/d1_feedback_rebuild.py`).
//!
//! | Source Kind | Native Kind | Difference |
//! | --- | --- | --- |
//! | `80802637` | `80803EB0` | Row class `808022E5` to `80803D95`, member tags |
//! | `8080272B` | `80803E94` | Array markers only |
//! | `80802668` | `80803EA5` | Array markers only |
//! | `80802099` | `80803EA0` | Array markers only. Radiance's two members reproduce stock `80BAAE38` and `80BAAE3A`, and their roots `80BAAE39` and `80BAAE3B` |
//! | `80802487` | `80803E99` | Array markers only. Explosive Rounds member `80AAC3AD` and stock `80C70634` share every byte but the markers and constant values |
//! | `808024A4` | `80803EAA` | Header `+08` word removed. Three source-only identity lanes at `+3E8`, `+430`, `+468`, the Float32 values before each and the Float32 one at `+49C` have no native slot |
use super::*;
use crate::{
    d1_roi::dependency::{Asset, Patch},
    presentation::{append, put},
};

const SOURCE_MARKER: u32 = 0x80800184;

struct Kind {
    native: u32,
    /// Native fixed-region size, or `None` when it ends at the first array.
    fixed: Option<usize>,
    /// Source-only value-program lanes, left out whole.
    dropped: &'static [usize],
}

fn kind(reference: u32) -> Result<Kind> {
    Ok(match reference {
        0x80802637 => Kind {
            native: 0x80803EB0,
            fixed: Some(0x88),
            dropped: &[],
        },
        0x8080272B => Kind {
            native: 0x80803E94,
            fixed: None,
            dropped: &[],
        },
        0x80802668 => Kind {
            native: 0x80803EA5,
            fixed: None,
            dropped: &[],
        },
        0x80802099 => Kind {
            native: 0x80803EA0,
            fixed: None,
            dropped: &[],
        },
        0x80802487 => Kind {
            native: 0x80803E99,
            fixed: None,
            dropped: &[],
        },
        0x808024A4 => Kind {
            native: 0x80803EAA,
            fixed: Some(0x3E0),
            dropped: &[0x3E8, 0x430, 0x468],
        },
        _ => anyhow::bail!("Source feedback member {reference:08X} has no native kind"),
    })
}

fn stride(class: u32) -> Result<usize> {
    Ok(match class {
        0x80800009 => 1,
        0x80800007 => 4,
        0x80800090 => 16,
        0x808022E5 => 0x24,
        _ => anyhow::bail!("Source feedback array class {class:08X} is unqualified"),
    })
}

/// Fixed-region array descriptors in offset order: field, count and header.
fn descriptors(p: &Payload) -> Result<Vec<(usize, u64, usize)>> {
    let mut out = Vec::new();
    for field in (8..p.0.len().saturating_sub(15)).step_by(8) {
        let count = p.u64(field)?;
        let relative = p.u64(field + 8)? as i64;
        if !(1..=65536).contains(&count) || relative <= 0 {
            continue;
        }
        let Some(header) = usize::try_from(relative)
            .ok()
            .and_then(|r| (field + 8).checked_add(r))
        else {
            continue;
        };
        if header % 16 == 0
            && header + 16 <= p.0.len()
            && p.u64(header)? == count
            && p.u32(header - 4)? == SOURCE_MARKER
        {
            out.push((field, count, header));
        }
    }
    Ok(out)
}

/// Translate one feedback resource and its members into private assets. Returns the assets with
/// the root first. Members shared between roots must be deduplicated by the caller by symbol.
pub fn translate(source: &Source, tag: u32, prefix: &str) -> Result<Vec<Asset>> {
    ensure!(
        source.reference(tag)? == 0x80802637,
        "Source feedback root has another kind"
    );
    let mut assets = Vec::new();
    let mut members = BTreeMap::new();
    let root = source.read(tag)?;
    let rows = root.array(8, 0x24, Some(0x808022E5))?;
    for row in &rows {
        for word in (*row..row + 0x24).step_by(4) {
            let member = root.u32(word)?;
            if member == u32::MAX || member >> 24 != 0x80 || source.reference(member).is_err() {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = members.entry(member) {
                let symbol = format!("{prefix}-feedback-{member:08X}");
                assets.push(rebuild(source, member, &symbol, &BTreeMap::new())?);
                entry.insert(symbol);
            }
        }
    }
    let symbol = format!("{prefix}-feedback-{tag:08X}");
    let asset = rebuild(source, tag, &symbol, &members)?;
    assets.insert(0, asset);
    source.unchanged()?;
    Ok(assets)
}

fn rebuild(
    source: &Source,
    tag: u32,
    symbol: &str,
    members: &BTreeMap<u32, String>,
) -> Result<Asset> {
    let reference = source.reference(tag)?;
    let kind = kind(reference)?;
    let p = source.read(tag)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64,
        "Source feedback payload size differs"
    );
    let arrays = descriptors(&p)?;
    let first = arrays
        .iter()
        .map(|&(_, _, header)| header - 4)
        .min()
        .context("Source feedback has no arrays")?;
    let fixed = kind.fixed.unwrap_or(first);
    ensure!(
        fixed <= first,
        "Source feedback fixed region overlaps its arrays"
    );
    let mut out = p.0[..fixed].to_vec();
    if reference == 0x808024A4 {
        // Native drops the source's leading word one and keeps the next three fields.
        ensure!(p.u32(8)? == 1, "Source feedback member header differs");
        out[8..0x14].copy_from_slice(&p.0[0xC..0x18]);
        out[0x14..0x18].fill(0);
        // Each source-only lane has a Float32 eight bytes before it, zero in Firefly's members
        // and 0.2 or 0.35 in Wrath Conductors' and Accelerant's, where the lane is a falloff
        // curve rather than the identity clamp. The native member has no slot for the lanes or
        // their values, so both are left out and that feedback channel is lost. That it only
        // shapes presentation is inferred from the resource's role, since its reader was not
        // traced.
        let values = kind.dropped.iter().map(|&d| d - 8).collect::<Vec<_>>();
        for &at in &values {
            ensure!(
                p.f32(at)?.is_finite(),
                "Source feedback member lane value is not finite"
            );
        }
        // Two Float32 words at `+420` and `+424`, between the first lane and the second lane's
        // value, are zero in the examined ability members and -0.015 each in the Necrochasm shot's
        // feedback member `80AA2CA4`. They lie in the same source-only region, which the native
        // member has no slot for, so they leave with the lanes. Whether they bound the first lane
        // is not established.
        let tail = 0x420..0x428;
        for at in tail.clone().step_by(4) {
            ensure!(
                p.f32(at)?.is_finite(),
                "Source feedback member lane bound is not finite"
            );
        }
        ensure!(
            p.f32(0x49C)? == 1.0
                && p.0[0x3E0..0x4A0].iter().enumerate().all(|(i, b)| {
                    let at = 0x3E0 + i;
                    *b == 0
                        || kind.dropped.iter().any(|&d| (d..d + 0x38).contains(&at))
                        || values.iter().any(|&v| (v..v + 4).contains(&at))
                        || tail.contains(&at)
                        || (0x49C..0x4A0).contains(&at)
                }),
            "Source feedback member has another source-only field"
        );
    }
    let mut patches = Vec::new();
    let mut seen = 0usize;
    for (field, count, header) in arrays {
        let class = p.u32(header + 8)?;
        let size = usize::try_from(count)? * stride(class)?;
        let rows =
            p.0.get(header + 16..header + 16 + size)
                .context("Source feedback rows")?
                .to_vec();
        // A source-only lane is a value program: bytecode and, unless it is the identity clamp
        // `3C 00 23 3E 00`, a Float4 constant array. Wrath Conductors' and Accelerant's members
        // hold a cubic falloff with constants 2, -3, 0 and 1 under their 0.2 and 0.35 values.
        // The native member has no slot for any lane, so it is left out whole.
        if kind.dropped.iter().any(|&d| (d..d + 0x38).contains(&field)) {
            ensure!(
                (kind.dropped.contains(&field) && class == 0x80800009)
                    || (kind.dropped.contains(&(field - 0x10)) && class == 0x80800090),
                "Source-only feedback lane is not a value program"
            );
            continue;
        }
        ensure!(
            field < fixed,
            "Source feedback descriptor lies outside its fixed region"
        );
        let native_class = if class == 0x808022E5 {
            0x80803D95
        } else {
            class
        };
        append(
            &mut out,
            field,
            u64::from(native_class),
            &rows,
            stride(class)?,
        )?;
        if class == 0x808022E5 {
            // The rows are the bytes just appended.
            let start = out.len() - size;
            for word in (start..start + size).step_by(4) {
                let member = u32::from_le_bytes(out[word..word + 4].try_into()?);
                if let Some(symbol) = members.get(&member) {
                    put(&mut out, word, &u32::MAX.to_le_bytes())?;
                    patches.push(Patch {
                        offset: word,
                        symbol: symbol.clone(),
                        class: kind_of(source, member)?,
                    });
                    seen += 1;
                }
            }
        }
    }
    ensure!(
        seen >= members.len() && (members.is_empty() || reference == 0x80802637),
        "Source feedback member rows differ"
    );
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    Ok(Asset {
        symbol: symbol.into(),
        class: kind.native,
        payload: Payload(out),
        patches,
    })
}

fn kind_of(source: &Source, member: u32) -> Result<u32> {
    Ok(kind(source.reference(member)?)?.native)
}
