//! Read native packfiles using checked section relocations and exact class signatures.
use super::*;
use crate::tiger::payload::Payload;
use schema::{Member, Schema};

struct Section {
    start: usize,
    offsets: [usize; 6],
}

struct Reader<'a> {
    data: Payload,
    schema: &'a Schema,
    sections: Vec<Section>,
    pointers: BTreeMap<usize, usize>,
    classes: BTreeMap<usize, String>,
    used: BTreeSet<usize>,
    objects: BTreeMap<usize, Object>,
}

pub(super) fn read(bytes: &[u8], schema: &Schema) -> Result<BTreeMap<usize, Object>> {
    let mut r = Reader {
        data: Payload(bytes.to_vec()),
        schema,
        sections: Vec::new(),
        pointers: BTreeMap::new(),
        classes: BTreeMap::new(),
        used: BTreeSet::new(),
        objects: BTreeMap::new(),
    };
    ensure!(
        bytes.len() <= 32 * 1024 * 1024
            && r.data.bytes::<8>(0)? == [0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10]
            && r.data.u32(12)? == 9
            && r.data.bytes::<4>(16)? == [8, 1, 0, 1]
            && r.data.bytes::<15>(40)? == *b"hk_2012.2.0-r1\0",
        "Unsupported native cloth packfile"
    );
    let count = r.data.u32(20)? as usize;
    ensure!((1..=8).contains(&count), "Invalid cloth section count");
    for i in 0..count {
        let at = 64 + i * 48 + 20;
        let start = r.data.u32(at)? as usize;
        let mut offsets = [0; 6];
        for (j, value) in offsets.iter_mut().enumerate() {
            *value = r.data.u32(at + 4 + j * 4)? as usize;
        }
        ensure!(
            start >= 64 + count * 48
                && offsets.windows(2).all(|w| w[0] <= w[1])
                && start
                    .checked_add(offsets[5])
                    .is_some_and(|end| end <= bytes.len())
                && offsets[3] == offsets[4]
                && offsets[4] == offsets[5],
            "Invalid cloth section or external relocation"
        );
        ensure!(
            !r.sections
                .iter()
                .any(|s| start < s.start + s.offsets[5] && s.start < start + offsets[5]),
            "Overlapping cloth sections"
        );
        r.sections.push(Section { start, offsets });
    }
    let root = r.address(r.data.u32(24)? as usize, r.data.u32(28)? as usize, 1)?;
    let name = r.address(r.data.u32(32)? as usize, r.data.u32(36)? as usize, 1)?;
    ensure!(
        r.class_name(name)? == "hclClothContainer",
        "Unsupported cloth root"
    );
    for i in 0..count {
        let start = r.sections[i].start;
        let bounds = r.sections[i].offsets;
        for (table, stride) in [(0, 8), (1, 12), (2, 12)] {
            let end = start + bounds[table + 1];
            let mut at = start + bounds[table];
            while at + stride <= end {
                let source = r.data.u32(at)? as usize;
                if source == u32::MAX as usize {
                    ensure!(
                        bytes[at..at + stride].iter().all(|b| *b == 255),
                        "Invalid cloth relocation padding"
                    );
                } else {
                    let source = r.address(i, source, 8)?;
                    let (section, offset) = if table == 0 {
                        (i, r.data.u32(at + 4)? as usize)
                    } else {
                        (r.data.u32(at + 4)? as usize, r.data.u32(at + 8)? as usize)
                    };
                    let target = r.address(section, offset, 1)?;
                    if table == 2 {
                        let name = r.class_name(target)?;
                        ensure!(
                            r.classes.insert(source, name).is_none(),
                            "Duplicate cloth finish record"
                        );
                    } else {
                        ensure!(
                            r.pointers.insert(source, target).is_none(),
                            "Duplicate cloth pointer"
                        );
                    }
                }
                at += stride;
            }
            ensure!(
                bytes[at..end].iter().all(|b| *b == 255),
                "Invalid cloth relocation tail"
            );
        }
    }
    r.object(root, "hclClothContainer", 0)?;
    ensure!(
        r.pointers.keys().all(|at| r.used.contains(at))
            && r.classes.keys().all(|at| r.objects.contains_key(at)),
        "Cloth packfile has unconsumed pointers or objects"
    );
    Ok(r.objects)
}

impl Reader<'_> {
    fn address(&self, section: usize, offset: usize, size: usize) -> Result<usize> {
        let s = self.sections.get(section).context("Absent cloth section")?;
        ensure!(
            offset
                .checked_add(size)
                .is_some_and(|end| end <= s.offsets[0]),
            "Cloth address exceeds its section data"
        );
        Ok(s.start + offset)
    }

    fn span(&self, at: usize, size: usize) -> Result<()> {
        ensure!(
            self.sections.iter().any(|s| at >= s.start
                && at
                    .checked_add(size)
                    .is_some_and(|end| end <= s.start + s.offsets[0])),
            "Cloth field exceeds section data"
        );
        Ok(())
    }

    fn string(&self, at: usize) -> Result<String> {
        let s = self
            .sections
            .iter()
            .find(|s| at >= s.start && at < s.start + s.offsets[0])
            .context("Cloth string outside section data")?;
        let bytes = &self.data.0[at..s.start + s.offsets[0]];
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .context("Unterminated cloth string")?;
        Ok(std::str::from_utf8(&bytes[..end])?.to_owned())
    }

    fn class_name(&self, at: usize) -> Result<String> {
        let start = at
            .checked_sub(5)
            .context("Cloth class signature underflow")?;
        self.span(start, 6)?;
        let name = self.string(at)?;
        ensure!(
            self.data.u8(at - 1)? == 9
                && self.schema.native(&name)?.signature == Some(self.data.u32(start)?),
            "Cloth class signature differs for {name}"
        );
        Ok(name)
    }

    fn pointer(&mut self, at: usize) -> Result<Option<usize>> {
        self.span(at, 8)?;
        if let Some(target) = self.pointers.get(&at).copied() {
            self.used.insert(at);
            Ok(Some(target))
        } else {
            ensure!(self.data.u64(at)? == 0, "Cloth pointer lacks a relocation");
            Ok(None)
        }
    }

    fn object(&mut self, at: usize, expected: &str, depth: usize) -> Result<Value> {
        ensure!(
            depth < 128 && self.objects.len() < 100_000,
            "Cloth object graph exceeds bounds"
        );
        let name = self
            .classes
            .get(&at)
            .context("Cloth object lacks a finish record")?
            .clone();
        let mut ancestor = Some(name.as_str());
        while ancestor.is_some_and(|n| n != expected) {
            ancestor = self.schema.native(ancestor.unwrap())?.parent.as_deref();
        }
        ensure!(
            ancestor.is_some(),
            "Cloth object {name} is not a {expected}"
        );
        if !self.objects.contains_key(&at) {
            self.span(at, self.schema.native(&name)?.size)?;
            self.objects.insert(
                at,
                Object {
                    name: name.clone(),
                    value: Value::Null,
                },
            );
            let value = self.record(at, &name, depth + 1)?;
            self.objects.get_mut(&at).unwrap().value = value;
        }
        Ok(json!({"object":at}))
    }

    fn record(&mut self, at: usize, name: &str, depth: usize) -> Result<Value> {
        self.span(at, self.schema.native(name)?.size)?;
        let mut result = serde_json::Map::new();
        for member in self.schema.members(name)? {
            let value = self
                .field(at + member.offset, &member, depth + 1)
                .with_context(|| format!("{name}.{}", member.name))?;
            result.insert(member.name, value);
        }
        Ok(Value::Object(result))
    }

    fn field(&mut self, at: usize, member: &Member, depth: usize) -> Result<Value> {
        ensure!(depth < 128, "Cloth field nesting exceeds bounds");
        let class = member.class.as_deref();
        if member.length != 0 {
            let stride = self.schema.size(member.kind, class)?;
            self.span(
                at,
                stride
                    .checked_mul(member.length)
                    .context("Cloth tuple overflow")?,
            )?;
            let mut element = member.clone();
            element.length = 0;
            return (0..member.length)
                .map(|i| self.field(at + i * stride, &element, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array);
        }
        if matches!(member.kind, 24 | 31) {
            let mut element = member.clone();
            element.kind = member.subtype;
            return self.field(at, &element, depth + 1);
        }
        self.span(at, self.schema.size(member.kind, class)?)?;
        Ok(match member.kind {
            1..=10 => {
                let size = self.schema.size(member.kind, class)?;
                let mut raw = [0; 8];
                raw[..size].copy_from_slice(&self.data.0[at..at + size]);
                let value = u64::from_le_bytes(raw);
                if matches!(member.kind, 3 | 5 | 7 | 9) {
                    let shift = 64 - size * 8;
                    json!(((value << shift) as i64) >> shift)
                } else {
                    ensure!(member.kind != 1 || value <= 1, "Invalid cloth Boolean");
                    json!(value)
                }
            }
            11 => json!(self.data.f32(at)?),
            12..=18 => (0..self.schema.size(member.kind, class)? / 4)
                .map(|i| self.data.f32(at + i * 4).map(|v| json!(v)))
                .collect::<Result<Vec<_>>>()?
                .into(),
            29 | 33 => match self.pointer(at)? {
                Some(target) => json!(self.string(target)?),
                None => json!(""),
            },
            20 => match self.pointer(at)? {
                Some(target) => {
                    self.object(target, class.context("Cloth pointer type")?, depth + 1)?
                }
                None => Value::Null,
            },
            22 => {
                let target = self.pointer(at)?;
                let count = self.data.u32(at + 8)? as usize;
                ensure!(count <= 1_000_000, "Cloth array exceeds supported bounds");
                if count == 0 {
                    Value::Array(Vec::new())
                } else {
                    let target = target.context("Cloth array has no storage")?;
                    let stride = self.schema.size(member.subtype, class)?;
                    self.span(
                        target,
                        stride.checked_mul(count).context("Cloth array overflow")?,
                    )?;
                    let mut element = member.clone();
                    element.kind = member.subtype;
                    (0..count)
                        .map(|i| self.field(target + i * stride, &element, depth + 1))
                        .collect::<Result<Vec<_>>>()?
                        .into()
                }
            }
            25 => self.record(at, class.context("Cloth struct type")?, depth + 1)?,
            30 => json!(self.data.u16(at)?),
            _ => anyhow::bail!("Unsupported native cloth field {}", member.kind),
        })
    }
}
