//! Checked Wwise 150 to 113 lowering. Object references are recorded while parsing,
//! so private IDs never replace coincidentally equal floats or curve values.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod linked;
mod one_shot;
mod params;
pub use linked::Namespace;
pub mod settings;
use params::{node, rtpcs};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bank {
    pub bytes: Vec<u8>,
    pub bank_id: u32,
    pub objects: BTreeSet<u32>,
    refs: Vec<Reference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Reference {
    offset: usize,
    kind: Kind,
    value: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum Kind {
    Object,
    Bank,
    Media,
    MediaSize,
}

struct Read<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Read<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).context("bank offset overflow")?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .with_context(|| format!("bank truncated at {} for {n} bytes", self.at))?;
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn var(&mut self) -> Result<u32> {
        let mut value = 0u32;
        for _ in 0..5 {
            let byte = self.u8()?;
            ensure!(value <= u32::MAX >> 7, "bank variable integer overflow");
            value = (value << 7) | u32::from(byte & 127);
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        bail!("bank variable integer exceeds five bytes")
    }
    fn count(&mut self) -> Result<usize> {
        bounded(self.u32()?)
    }
    fn vcount(&mut self) -> Result<usize> {
        bounded(self.var()?)
    }
    fn end(&self) -> Result<()> {
        ensure!(
            self.at == self.bytes.len(),
            "unparsed bank bytes at {}/{}",
            self.at,
            self.bytes.len()
        );
        Ok(())
    }
}
fn bounded(n: u32) -> Result<usize> {
    ensure!(n <= 65536, "bank count {n} exceeds limit");
    Ok(n as usize)
}

#[derive(Default)]
struct Write {
    bytes: Vec<u8>,
    refs: Vec<Reference>,
}
impl Write {
    fn u8(&mut self, n: u8) {
        self.bytes.push(n);
    }
    fn u16(&mut self, n: u16) {
        self.bytes.extend(n.to_le_bytes());
    }
    fn u32(&mut self, n: u32) {
        self.bytes.extend(n.to_le_bytes());
    }
    fn copy(&mut self, r: &mut Read<'_>, n: usize) -> Result<()> {
        self.bytes.extend(r.take(n)?);
        Ok(())
    }
    fn reference(&mut self, value: u32, kind: Kind) {
        self.refs.push(Reference {
            offset: self.bytes.len(),
            kind,
            value,
        });
        self.u32(value);
    }
    fn object(&mut self, r: &mut Read<'_>) -> Result<()> {
        self.reference(r.u32()?, Kind::Object);
        Ok(())
    }
    fn append(&mut self, other: Write) {
        let base = self.bytes.len();
        self.bytes.extend(other.bytes);
        self.refs.extend(other.refs.into_iter().map(|mut r| {
            r.offset += base;
            r
        }));
    }
    fn chunk(&mut self, name: &[u8; 4], payload: Write) -> Result<()> {
        self.bytes.extend(name);
        self.u32(u32::try_from(payload.bytes.len())?);
        self.append(payload);
        Ok(())
    }
}

struct Convert {
    objects: BTreeSet<u32>,
    source_plugins: BTreeMap<u32, u32>,
    extra: Vec<(u8, Write)>,
    dependencies: settings::Dependencies,
}
impl Convert {
    fn state_id(&mut self, owner: u32, group: u32, state: u32) -> u32 {
        let mut id = hash(&format!("state/{owner:08x}/{group:08x}/{state:08x}"));
        while id == 0 || !self.objects.insert(id) {
            id = id.wrapping_add(1);
        }
        id
    }
}

pub fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811C9DC5u32, |h, b| {
        h.wrapping_mul(16777619) ^ u32::from(b.to_ascii_lowercase())
    })
}

/// Convert a complete source bank, retaining its hierarchy, actions and curves.
/// Unsupported active features are errors, never silently flattened or dropped.
pub fn lower(bytes: &[u8]) -> Result<Bank> {
    lower_inner(bytes, None)
}

/// Include source globals absent from the target Init bank before loading HIRC.
pub fn lower_with_settings(
    bytes: &[u8],
    source: &settings::Settings,
    native: &settings::Settings,
) -> Result<Bank> {
    lower_inner(bytes, Some((source, native)))
}

fn lower_inner(
    bytes: &[u8],
    settings: Option<(&settings::Settings, &settings::Settings)>,
) -> Result<Bank> {
    ensure!(bytes.len() <= 64 * 1024 * 1024, "source bank exceeds limit");
    let mut r = Read { bytes, at: 0 };
    let mut header = None;
    let mut objects = None;
    while r.at < bytes.len() {
        let name = r.take(4)?;
        let size = r.u32()? as usize;
        let body = r.take(size)?;
        match name {
            b"BKHD" => {
                ensure!(header.replace(body).is_none(), "duplicate bank header");
            }
            b"HIRC" => {
                ensure!(objects.replace(body).is_none(), "duplicate HIRC");
            }
            b"STID" => {} // Debug bank names are not part of playback.
            b"DIDX" | b"DATA" => ensure!(body.is_empty(), "embedded media needs extraction"),
            _ => bail!("unsupported source bank chunk {name:?}"),
        }
    }
    let mut h = Read {
        bytes: header.context("bank header missing")?,
        at: 0,
    };
    ensure!(h.u32()? == 150, "source bank must be version 150");
    let bank_id = h.u32()?;
    // The source uses language hashes. Native SFX banks use language ordinal zero.
    let language = h.u32()?;
    ensure!(
        language == 0 || language == hash("sfx"),
        "localized audio language needs a native ordinal"
    );
    let alignment = h.u32()?;
    ensure!(
        alignment & 0xFFFF <= 256,
        "unsupported bank media alignment"
    );
    let project = h.u32()?;
    ensure!(h.u32()? == 0, "non-user bank type");
    h.take(16)?;
    h.end()?;
    let body = objects.context("bank HIRC missing")?;
    let mut r = Read { bytes: body, at: 0 };
    let count = r.count()?;
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for _ in 0..count {
        let kind = r.u8()?;
        let size = r.u32()? as usize;
        let data = r.take(size)?;
        let mut item = Read { bytes: data, at: 0 };
        let id = item.u32()?;
        ensure!(
            id != 0 && ids.insert(id),
            "duplicate or zero bank object {id:08X}"
        );
        entries.push((kind, id, item));
    }
    r.end()?;
    let source_plugins = entries
        .iter()
        .filter(|(kind, _, _)| matches!(kind, 16 | 17))
        .map(|(_, id, entry)| {
            let mut r = Read {
                bytes: entry.bytes,
                at: entry.at,
            };
            Ok((*id, r.u32()?))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut convert = Convert {
        objects: ids,
        source_plugins,
        extra: Vec::new(),
        dependencies: settings::Dependencies::default(),
    };
    let mut converted = Vec::new();
    for (kind, id, mut r) in entries {
        let mut w = Write::default();
        w.reference(id, Kind::Object);
        object(kind, id, &mut r, &mut w, &mut convert)
            .with_context(|| format!("bank object {id:08X} type {kind} at {}", r.at))?;
        r.end()
            .with_context(|| format!("bank object {id:08X} type {kind}"))?;
        // Feedback objects occupied 16 and 17 before Wwise 2017. Effects moved
        // down two slots when those objects were removed.
        converted.push((
            if matches!(kind, 16 | 17) {
                kind + 2
            } else {
                kind
            },
            w,
        ));
    }
    // Native nodes resolve State objects immediately while reading their state
    // chunks. Register generated states before any node can reference them.
    convert.extra.append(&mut converted);
    let converted = convert.extra;
    let mut hirc = Write::default();
    hirc.u32(u32::try_from(converted.len())?);
    for (kind, w) in converted {
        hirc.u8(kind);
        hirc.u32(u32::try_from(w.bytes.len())?);
        hirc.append(w);
    }
    let mut header = Write::default();
    header.u32(113);
    header.reference(bank_id, Kind::Bank);
    header.u32(0);
    header.u32(0);
    header.u32(project);
    header.u32(0);
    let mut out = Write::default();
    out.chunk(b"BKHD", header)?;
    if let Some((source, native)) = settings
        && let Some(payload) = source.supplement(native, &convert.dependencies)?
    {
        out.chunk(
            b"STMG",
            Write {
                bytes: payload,
                refs: Vec::new(),
            },
        )?;
    }
    out.chunk(b"HIRC", hirc)?;
    Ok(Bank {
        bytes: out.bytes,
        bank_id,
        objects: convert.objects,
        refs: out.refs,
    })
}

impl Bank {
    /// Native media reference fields recorded by the parser, including unaligned fields.
    /// Their values are package-allocated media IDs, separate from Wwise object identities.
    pub fn media_fields(&self) -> Vec<(usize, u32)> {
        self.refs
            .iter()
            .filter(|reference| matches!(reference.kind, Kind::Media))
            .map(|reference| (reference.offset, reference.value))
            .collect()
    }

    /// Allocate identities from a private namespace. Event and bank IDs are independent.
    pub fn instantiate(
        &self,
        namespace: &str,
        event: u32,
        media: &BTreeMap<u32, u32>,
        sizes: &BTreeMap<u32, u32>,
    ) -> Result<(Vec<u8>, u32)> {
        ensure!(
            self.objects.contains(&event),
            "source event is absent from bank"
        );
        let event_id = hash(namespace);
        let bank_id = hash(&format!("{namespace}/bank"));
        let mut ids = BTreeMap::new();
        let mut unique = BTreeSet::from([0, bank_id]);
        for id in &self.objects {
            let private = if *id == event {
                event_id
            } else {
                hash(&format!("{namespace}/object/{id:08x}"))
            };
            ensure!(
                unique.insert(private),
                "private audio object identity collision"
            );
            ids.insert(*id, private);
        }
        let mut bytes = self.bytes.clone();
        let mut seen = BTreeSet::new();
        for reference in &self.refs {
            let value = match reference.kind {
                Kind::Object => ids
                    .get(&reference.value)
                    .copied()
                    .unwrap_or(reference.value),
                Kind::Bank => {
                    ensure!(
                        reference.value == self.bank_id,
                        "cross-bank play action needs imported dependency"
                    );
                    bank_id
                }
                Kind::Media => {
                    seen.insert(reference.value);
                    *media
                        .get(&reference.value)
                        .context("source bank medium was not converted")?
                }
                Kind::MediaSize => *sizes
                    .get(&reference.value)
                    .context("converted medium size missing")?,
            };
            bytes
                .get_mut(reference.offset..reference.offset + 4)
                .context("bank relocation out of bounds")?
                .copy_from_slice(&value.to_le_bytes());
        }
        ensure!(
            seen.len() == media.len(),
            "converted media and source bank differ"
        );
        Ok((bytes, event_id))
    }
}

fn children(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    let count = r.count()?;
    w.u32(count as u32);
    for _ in 0..count {
        w.object(r)?;
    }
    Ok(())
}
fn exceptions(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    let count = r.vcount()?;
    w.u32(count as u32);
    for _ in 0..count {
        w.object(r)?;
        w.copy(r, 1)?;
    }
    Ok(())
}

/// Convert an action body after its kind and ID.
fn action(r: &mut Read<'_>, w: &mut Write, c: &mut Convert) -> Result<()> {
    let action = r.u16()?;
    w.u16(if matches!(action >> 8, 0x1A | 0x1B) {
        action + 0x200
    } else {
        action
    });
    // State values and game parameters use the game's global namespace.
    // They must not be renamed even if their numeric ID matches a local object.
    if matches!(action >> 8, 0x12..=0x14 | 0x19) {
        let target = r.u32()?;
        w.u32(target);
        if matches!(action >> 8, 0x13 | 0x14) {
            c.dependencies.params.insert(target);
        }
    } else {
        w.object(r)?;
    }
    w.copy(r, 1)?;
    params::props(r, w, false)?;
    params::props(r, w, true)?;
    match action >> 8 {
        4 => {
            w.copy(r, 1)?;
            w.reference(r.u32()?, Kind::Bank);
            ensure!(r.u32()? == 0, "non-user play bank");
        }
        1..=3 => {
            w.copy(r, 1)?;
            let bits = r.u8()?;
            if action >> 8 == 1 {
                ensure!(
                    bits == 6,
                    "stop action needs unsupported scope flags {bits}"
                );
            } else {
                w.u8(bits);
            }
            exceptions(r, w)?;
        }
        8..=11 => {
            w.copy(r, 14)?;
            exceptions(r, w)?;
        }
        0x12 | 0x19 => {
            let group = r.u32()?;
            if action >> 8 == 0x12 {
                c.dependencies.groups.insert(group);
            } else {
                c.dependencies.switches.insert(group);
            }
            w.u32(group);
            w.copy(r, 4)?; // State or switch value, not a HIRC object.
        }
        0x13 | 0x14 => {
            w.copy(r, 15)?;
            exceptions(r, w)?;
        }
        0x1A | 0x1B => {} // Break and Trigger moved down two action kinds in v150.
        0x1E => {
            w.copy(r, 14)?;
            exceptions(r, w)?;
        }
        0x21 => {}
        other => bail!("unsupported action kind {other:02X}"),
    }
    Ok(())
}

/// Convert the layer controllers of a layer container after its children.
fn layers(r: &mut Read<'_>, w: &mut Write, c: &mut Convert) -> Result<()> {
    let count = r.count()?;
    w.u32(count as u32);
    for _ in 0..count {
        let layer = r.u32()?;
        ensure!(
            c.objects.insert(layer),
            "layer ID collides with another object"
        );
        w.reference(layer, Kind::Object);
        rtpcs(r, w, c)?;
        let parameter = r.u32()?;
        let kind = r.u8()?;
        ensure!(kind <= 1, "unsupported layer controller {kind}");
        if kind == 0 {
            c.dependencies.params.insert(parameter);
        }
        w.u32(parameter);
        w.u8(kind);
        let associated = r.count()?;
        w.u32(associated as u32);
        for _ in 0..associated {
            w.object(r)?;
            let points = r.count()?;
            w.u32(points as u32);
            w.copy(r, points * 12)?;
        }
    }
    ensure!(
        r.u8()? == 0,
        "continuous layer container needs native scheduler conversion"
    );
    Ok(())
}

fn object(kind: u8, id: u32, r: &mut Read<'_>, w: &mut Write, c: &mut Convert) -> Result<()> {
    match kind {
        2 => {
            let plugin = r.u32()?;
            if plugin == 0x00650002 {
                w.u32(plugin);
                ensure!(r.u8()? == 0, "silence generator cannot stream media");
                w.u8(0);
                let source = r.u32()?;
                w.reference(source, Kind::Object);
                ensure!(r.u32()? == 0, "silence generator has media bytes");
                w.u32(0);
                w.copy(r, 1)?;
                let size = r.u32()?;
                ensure!(
                    size == 12 || (size == 0 && c.source_plugins.get(&source) == Some(&plugin)),
                    "silence source needs inline parameters or a matching shared preset"
                );
                w.u32(size);
                w.copy(r, size as usize)?;
                node(r, w, c, id)?;
                return Ok(());
            }
            ensure!(
                matches!(
                    plugin,
                    0x00010001 | 0x00040001 | 0x00110001 | 0x00130001 | 0x00140001
                ),
                "unsupported media plugin {plugin:08X}"
            );
            w.u32(0x00010001);
            let stream = r.u8()?;
            ensure!(stream == 2, "source medium is not fully streamed");
            w.u8(2);
            let media = r.u32()?;
            w.reference(media, Kind::Media);
            r.u32()?;
            w.reference(media, Kind::MediaSize);
            let bits = r.u8()?;
            ensure!(bits & !8 == 0, "source has embedded or prefetched media");
            w.u8(bits);
            node(r, w, c, id)?;
        }
        3 => action(r, w, c)?,
        4 => {
            let count = r.vcount()?;
            w.u32(count as u32);
            for _ in 0..count {
                w.object(r)?;
            }
        }
        5 => {
            node(r, w, c, id)?;
            w.copy(r, 24)?;
            children(r, w)?;
            let count = r.u16()?;
            w.u16(count);
            for _ in 0..count {
                w.object(r)?;
                w.copy(r, 4)?;
            }
        }
        6 => {
            node(r, w, c, id)?;
            let kind = r.u8()?;
            let group = r.u32()?;
            ensure!(kind <= 1, "unsupported switch group kind {kind}");
            if kind == 0 {
                c.dependencies.switches.insert(group);
            } else {
                c.dependencies.groups.insert(group);
            }
            w.u8(kind);
            w.u32(group);
            w.copy(r, 5)?;
            children(r, w)?;
            let count = r.count()?;
            w.u32(count as u32);
            for _ in 0..count {
                w.copy(r, 4)?;
                children(r, w)?;
            }
            let count = r.count()?;
            w.u32(count as u32);
            for _ in 0..count {
                w.object(r)?;
                w.copy(r, 10)?;
            }
        }
        7 => {
            node(r, w, c, id)?;
            children(r, w)?;
        }
        9 => {
            node(r, w, c, id)?;
            children(r, w)?;
            layers(r, w, c)?;
        }
        14 => {
            let height = r.u8()?;
            ensure!(height <= 1, "unknown height-spread mode");
            let cone = r.u8()?;
            ensure!(cone <= 1, "invalid attenuation cone");
            w.u8(cone);
            if cone == 1 {
                w.copy(r, 20)?;
            }
            let indices = r.take(19)?;
            ensure!(
                indices[7..].iter().all(|b| (*b as i8) < 0),
                "active modern-only attenuation curve"
            );
            w.bytes.extend(&indices[..7]);
            let count = r.u8()?;
            w.u8(count);
            for _ in 0..count {
                w.copy(r, 1)?;
                let points = r.u16()?;
                w.u16(points);
                w.copy(r, usize::from(points) * 12)?;
            }
            rtpcs(r, w, c)?;
        }
        16 | 17 => {
            let plugin = r.u32()?;
            let size = r.u32()? as usize;
            if plugin == 0x006E1003 && size == 159 {
                // The source FutzBox preset extends the legacy 139-byte block
                // with four zero-valued controls and a trailing mode value 1.
                // Shared presets in both package sets are byte-identical after
                // removing these inactive extensions. Reject active extensions.
                let payload = r.take(size)?;
                ensure!(
                    payload[38..54].iter().all(|byte| *byte == 0)
                        && payload[155..159] == 1u32.to_le_bytes(),
                    "FutzBox uses modern-only effect controls"
                );
                w.u32(plugin);
                w.u32(139);
                w.bytes.extend(&payload[..38]);
                w.bytes.extend(&payload[54..155]);
            } else {
                ensure!(
                    matches!(
                        (plugin, size),
                        (0x00650002, 12)
                            | (0x00690003, 56)
                            | (0x00760003, 186)
                            // Shared native and source Init Tremolo presets have
                            // identical 38-byte parameter blocks and plugin ID.
                            | (0x00830003, 38)
                    ),
                    "unvalidated effect plugin {plugin:08X} parameter size {size}"
                );
                w.u32(plugin);
                w.u32(size as u32);
                w.copy(r, size)?;
            }
            let media = r.u8()?;
            ensure!(media == 0, "effect media dependency requires conversion");
            w.u8(0);
            ensure!(
                r.u16()? == 0,
                "effect plugin RTPC conversion is unsupported"
            );
            w.u16(0);
            ensure!(
                r.vcount()? == 0 && r.vcount()? == 0,
                "effect state conversion is unsupported"
            );
            ensure!(
                r.u16()? == 0,
                "effect initial property conversion is unsupported"
            );
            w.u16(0);
        }
        other => bail!("unsupported HIRC object type {other}"),
    }
    Ok(())
}
