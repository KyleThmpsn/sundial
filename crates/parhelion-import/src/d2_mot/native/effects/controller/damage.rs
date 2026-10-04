//! Package-validated projectile damage cores and their native allocation trees.
//!
//! Modern impact modifiers are a separate format. Active modifiers are refused
//! until their conversion is available, never replaced with an empty native field.
use super::{Relocation, procedural, put};
use crate::d2_mot::{entity::links::Object, payload::Payload};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

mod allocation;
mod emission;
mod interfaces;
mod plan;
use interfaces::PROVIDERS;

#[cfg(test)]
mod tests;

pub struct Damage {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    /// These two modern methods have no native counterpart. Entity assembly
    /// must reject a caller that requests either method.
    pub omitted_methods: Vec<Omitted>,
}

#[derive(Serialize)]
pub struct Omitted {
    pub object: Object,
    pub methods: Vec<u32>,
}

const CLASSES: &[(u32, u32)] = &[
    (0x80802960, 0x8080377D),
    (0x80802D47, 0x80803778),
    (0x80808631, 0x808089F7),
    (0x80808632, 0x808089F8),
    (0x8080862F, 0x808089F5),
    (0x80808630, 0x808089F6),
    (0x80809A9F, 0x80809BD9),
    (0x80809A9E, 0x80809BD8),
    (0x80809590, 0x80809788),
    (0x80809591, 0x80809789),
    (0x80802959, 0x80803774),
    (0x8080295A, 0x80803775),
    (0x80802957, 0x80803772),
    (0x80802958, 0x80803773),
];
const PROGRAMS: [usize; 4] = [0x58, 0xB0, 0x108, 0x170];

fn native(class: u32) -> Result<u32> {
    CLASSES
        .iter()
        .find(|(source, _)| *source == class)
        .map(|(_, target)| *target)
        .with_context(|| format!("damage record class {class:08X}"))
}

#[derive(Clone, Copy)]
struct Record {
    at: usize,
    class: u32,
    twin: usize,
}

fn records(p: &Payload) -> Result<BTreeMap<usize, Record>> {
    ensure!(p.u64(0)? == p.0.len() as u64, "damage payload size differs");
    let root = p.pointer(16)?;
    let owner = p.u32(root)?;
    let mut out = BTreeMap::new();
    for at in (0..p.0.len().saturating_sub(15)).step_by(8) {
        if p.u32(at)? != owner {
            continue;
        }
        let class = p.u32(at + 4)?;
        if class & 0xFFFF0000 != 0x80800000 {
            continue;
        }
        let twin = usize::try_from(p.u64(at + 8)?)?;
        if twin == at || twin % 8 != 0 {
            continue;
        }
        if p.u32(twin).ok() == Some(owner) && p.u64(twin + 8).ok() == Some(at as u64) {
            out.insert(at, Record { at, class, twin });
        }
    }
    ensure!(!out.is_empty(), "damage has no reciprocal records");
    Ok(out)
}

#[derive(Clone)]
struct Array {
    header: usize,
    class: u32,
    count: usize,
    stride: usize,
    data: Vec<u8>,
}

fn array(p: &Payload, field: usize, class: u32, stride: usize) -> Result<Option<Array>> {
    let rows = p.array(field, stride, Some(class))?;
    if rows.is_empty() {
        ensure!(p.u64(field + 8)? == 0, "damage empty array has a pointer");
        return Ok(None);
    }
    let header = p.pointer(field + 8)?;
    ensure!(
        header >= 4 && header % 16 == 0 && p.u32(header - 4)? == 0x80809FB8,
        "damage array marker or alignment differs"
    );
    let end = rows
        .len()
        .checked_mul(stride)
        .and_then(|size| (header + 16).checked_add(size))
        .context("damage array overflow")?;
    Ok(Some(Array {
        header,
        class,
        count: rows.len(),
        stride,
        data: p
            .0
            .get(header + 16..end)
            .context("damage array extent")?
            .to_vec(),
    }))
}

enum Event {
    Record(Record),
    Array(Array),
    Conditions,
    State,
    Tail,
}
struct Fix {
    at: usize,
    source: usize,
    absolute: bool,
}
struct Writer {
    bytes: Vec<u8>,
    positions: BTreeMap<usize, usize>,
    fixes: Vec<Fix>,
}

impl Writer {
    fn pointer(&mut self, at: usize, source: usize) {
        self.fixes.push(Fix {
            at,
            source,
            absolute: false,
        });
    }
    fn prefix(&mut self, class: u32, alignment: usize) -> Result<()> {
        let at = self
            .bytes
            .len()
            .checked_add(4 + alignment - 1)
            .context("damage alignment overflow")?
            & !(alignment - 1);
        self.bytes.resize(at, 0);
        put(&mut self.bytes, at - 4, &class.to_le_bytes())
    }
    fn finish(mut self) -> Result<Payload> {
        for fix in self.fixes {
            let target = *self
                .positions
                .get(&fix.source)
                .context("unplaced damage reference")?;
            if fix.absolute {
                put(&mut self.bytes, fix.at, &(target as u64).to_le_bytes())?;
            } else {
                let delta = i64::try_from(target)? - i64::try_from(fix.at)?;
                put(&mut self.bytes, fix.at, &delta.to_le_bytes())?;
            }
        }
        let len = (self.bytes.len() + 15) & !15;
        self.bytes.resize(len, 0);
        put(&mut self.bytes, 0, &(len as u64).to_le_bytes())?;
        Ok(Payload(self.bytes))
    }
}

fn modern_state() -> [u8; 112] {
    let mut b = [0; 112];
    for at in [0x1C, 0x2C] {
        b[at..at + 4].copy_from_slice(&1f32.to_le_bytes());
    }
    for at in [0x30, 0x34, 0x38, 0x50, 0x54] {
        b[at..at + 4].fill(255);
    }
    b[0x6C..].copy_from_slice(&0x00030000u32.to_le_bytes());
    b
}

fn native_state() -> [u8; 96] {
    let mut b = [0; 96];
    b[0x1C..0x20].copy_from_slice(&1f32.to_le_bytes());
    for at in [0, 0x20, 0x24, 0x28, 0x40] {
        b[at..at + 4].fill(255);
    }
    b
}

fn input_default(p: &Payload) -> Result<[u8; 96]> {
    ensure!(
        p.u64(0)? == p.0.len() as u64,
        "native damage input envelope size differs"
    );
    let d = p.pointer(24)?;
    let row = *p
        .array(d + 0x188, 40, Some(0x80809789))?
        .first()
        .context("native damage input envelope has no inputs")?;
    let i = usize::try_from(p.u64(row + 8)?)?;
    ensure!(
        p.u32(row + 4)? == 0x80809788
            && p.u32(i + 4)? == 0x80809789
            && p.u64(i + 8)? == row as u64
            && p.pointer(i + 16)? == p.pointer(16)?,
        "native damage input envelope pair differs"
    );
    let default = p.bytes::<96>(i)?;
    for (at, value) in [
        (24, 0),
        (32, u64::MAX),
        (40, 0),
        (48, u32::MAX as u64),
        (56, u64::MAX),
        (64, 0),
        (72, 0),
        (80, 0),
        (88, 0),
    ] {
        ensure!(
            p.u64(i + at)? == value,
            "native damage input envelope is initialized"
        );
    }
    Ok(default)
}

/// Convert supported damage owners completely. The template supplies the
/// native dispatch envelope, provider metadata references and runtime defaults.
/// An owner with modern impact objects returns an error before writing output.
pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation_template: &Payload,
    input_envelope: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Damage> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid damage output tags"
    );
    let source_records = records(source)?;
    let native_records = records(template)?;
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = template.pointer(16)?;
    let nd = template.pointer(24)?;
    ensure!(
        source.u32(si + 4)? == 0x80802960
            && source.u32(sd + 4)? == 0x80802D47
            && source.u64(si + 8)? == sd as u64
            && source.u64(sd + 8)? == si as u64,
        "unsupported source damage root"
    );
    ensure!(
        template.u32(ni + 4)? == 0x8080377D
            && template.u32(nd + 4)? == 0x80803778
            && native_records.contains_key(&ni)
            && native_records.contains_key(&nd),
        "unsupported native damage root"
    );
    for record in source_records.values() {
        if (0x8080204D..=0x80802055).contains(&record.class) {
            bail!(
                "damage impact class {:08X} at {:X} requires conversion",
                record.class,
                record.at
            );
        }
        native(record.class)?;
    }
    ensure!(
        source.bytes::<24>(sd + 0x1E0)? == [0; 24],
        "damage impact fields require conversion"
    );
    ensure!(
        source.bytes::<8>(sd + 0x168)? == [0; 8],
        "damage condition extension differs"
    );
    ensure!(
        source.bytes::<112>(si + 0x100)? == modern_state(),
        "damage runtime state is initialized"
    );
    ensure!(
        template.bytes::<96>(ni + 0x100)? == native_state(),
        "native damage runtime state is initialized"
    );
    ensure!(
        source.bytes::<32>(si + 16)? == template.bytes::<32>(ni + 16)?,
        "damage root runtime defaults differ"
    );
    let settings = sd + 0x1F8;
    let native_settings = nd + 0x1F8;
    ensure!(
        source.u32(settings + 4)? == 0x80809A9E && template.u32(native_settings + 4)? == 0x80809BD8,
        "damage settings record differs"
    );
    let settings_i = usize::try_from(source.u64(settings + 8)?)?;
    let native_settings_i = usize::try_from(template.u64(native_settings + 8)?)?;
    ensure!(
        source_records
            .get(&settings)
            .is_some_and(|r| r.class == 0x80809A9E && r.twin == settings_i)
            && source_records
                .get(&settings_i)
                .is_some_and(|r| r.class == 0x80809A9F)
            && source.pointer(settings_i + 16)? == si,
        "damage settings pair or parent differs"
    );
    ensure!(
        source.bytes::<32>(settings_i + 0x50)? == [0; 32],
        "damage impact runtime arrays require conversion"
    );
    ensure!(
        source.bytes::<56>(settings_i + 24)? == template.bytes::<56>(native_settings_i + 24)?,
        "damage settings runtime defaults differ"
    );
    ensure!(
        source.u64(settings + 16)? == 1
            && source.u64(settings + 24)? == 0x80802AA2
            && template.u64(native_settings + 16)? == 1
            && template.u64(native_settings + 24)? == 0x80803890,
        "damage movement consumer contract differs"
    );
    for (definition_field, runtime_field, definition_class, instance_class) in [
        (0x158, 0x90, 0x80802959, 0x8080295A),
        (0x160, 0x98, 0x80802957, 0x80802958),
    ] {
        let present = source.u64(sd + definition_field)? != 0;
        ensure!(
            present == (source.u64(si + runtime_field)? != 0),
            "damage wrapper runtime presence differs"
        );
        if present {
            let d = source.pointer(sd + definition_field)?;
            let i = source.pointer(si + runtime_field)?;
            ensure!(
                source_records
                    .get(&d)
                    .is_some_and(|r| r.class == definition_class && r.twin == i)
                    && source_records
                        .get(&i)
                        .is_some_and(|r| r.class == instance_class)
                    && source.pointer(i + 16)? == si,
                "damage wrapper pair or parent differs"
            );
            ensure!(
                source_records
                    .get(&(d + 24))
                    .is_some_and(|r| r.class == 0x8080862F && r.twin == i + 32),
                "damage wrapper expression pair differs"
            );
        }
    }
    for (definition_field, runtime_field) in PROGRAMS.into_iter().zip([0xA0, 0x30, 0x60, 0xD0]) {
        ensure!(
            source_records
                .get(&(sd + definition_field))
                .is_some_and(|r| r.class == 0x80808631 && r.twin == si + runtime_field),
            "damage inline program layout differs"
        );
    }
    let default = input_default(input_envelope)?;
    let plan::Plan {
        lifecycle,
        arrays,
        descriptors,
        inputs,
    } = plan::Plan::read(source, &source_records, si)?;
    let events = plan::events(&source_records, &arrays, si, sd)?;
    let mut writer = Writer {
        bytes: template
            .0
            .get(..ni.checked_sub(16).context("damage native prefix")?)
            .context("damage native prefix")?
            .to_vec(),
        positions: BTreeMap::new(),
        fixes: Vec::new(),
    };
    let emitter = emission::Emitter {
        source,
        template,
        source_records: &source_records,
        descriptors: &descriptors,
        default: &default,
        si,
        sd,
        native_settings,
        owner_tag,
    };
    writer.pointer(0x38, lifecycle.header);
    for (old, event) in events {
        match event {
            Event::Conditions => {
                let new = writer.bytes.len();
                writer.bytes.extend(source.bytes::<16>(old)?);
                for field in [old, old + 8] {
                    if source.u64(field)? != 0 {
                        writer.pointer(new + field - old, source.pointer(field)?);
                    }
                }
            }
            Event::State => writer.bytes.extend(template.bytes::<96>(ni + 0x100)?),
            Event::Tail => {
                writer.bytes.extend(source.bytes::<32>(old)?);
                writer.bytes.extend(0u64.to_le_bytes());
                writer.bytes.extend(u64::from(u32::MAX).to_le_bytes());
                writer.bytes.extend(0u64.to_le_bytes());
            }
            Event::Array(row) => emitter.array(&mut writer, old, row)?,
            Event::Record(record) => emitter.record(&mut writer, old, record)?,
        }
    }
    let positions = writer.positions.clone();
    writer.pointer(16, si);
    writer.pointer(24, sd);
    let native_i = *positions.get(&si).context("damage root instance missing")?;
    let native_d = *positions
        .get(&sd)
        .context("damage root definition missing")?;
    put(&mut writer.bytes, 0x44, &allocation_tag.to_le_bytes())?;
    put(
        &mut writer.bytes,
        0x48,
        &u64::try_from(native_d - native_i)?.to_le_bytes(),
    )?;
    let owner = writer.finish()?;
    let source_tag = source.u32(si)?;
    let mut objects = Vec::new();
    for record in source_records.values() {
        let class = source.u32(record.twin + 4)?;
        objects.push(Relocation {
            source: Object {
                owner: source_tag,
                class,
                offset: record.at as u64,
            },
            target: Object {
                owner: owner_tag,
                class: native(class)?,
                offset: positions[&record.at] as u64,
            },
        });
    }
    let (provider_objects, omitted_methods) = interfaces::emit(source, &owner, metadata)?;
    objects.extend(provider_objects);
    ensure!(
        objects
            .iter()
            .map(|r| r.source)
            .collect::<BTreeSet<_>>()
            .len()
            == objects.len(),
        "duplicate damage relocation key"
    );
    let allocation = allocation::emit(source, &inputs, allocation_template)?;
    Ok(Damage {
        owner,
        allocation,
        objects,
        omitted_methods,
    })
}
