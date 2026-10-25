//! Emit checked section-relative Havok 2012 packfiles with native signatures.
use super::*;
use schema::{Member, Schema};

struct Writer<'a> {
    schema: &'a Schema,
    objects: &'a BTreeMap<usize, Object>,
    data: Vec<u8>,
    offsets: BTreeMap<usize, usize>,
    fixups: BTreeMap<usize, usize>,
    finishes: BTreeMap<usize, String>,
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) -> Result<()> {
    bytes
        .get_mut(
            at..at
                .checked_add(value.len())
                .context("Cloth write overflow")?,
        )
        .context("Cloth write exceeds allocation")?
        .copy_from_slice(value);
    Ok(())
}

fn u32_bytes(value: usize) -> Result<[u8; 4]> {
    Ok(u32::try_from(value)?.to_le_bytes())
}

fn align(bytes: &mut Vec<u8>, value: u8) {
    bytes.resize(bytes.len().next_multiple_of(16), value);
}

pub(super) fn write(objects: &BTreeMap<usize, Object>, schema: &Schema) -> Result<Vec<u8>> {
    let mut writer = Writer {
        schema,
        objects,
        data: Vec::new(),
        offsets: BTreeMap::new(),
        fixups: BTreeMap::new(),
        finishes: BTreeMap::new(),
    };
    let root = writer.object(1, 0)?;
    let mut names = Vec::new();
    let mut classes = BTreeMap::new();
    for name in writer.finishes.values().collect::<BTreeSet<_>>() {
        classes.insert(name.clone(), names.len() + 5);
        names.extend(
            schema
                .native(name)?
                .signature
                .context("Missing native cloth class signature")?
                .to_le_bytes(),
        );
        names.push(9);
        names.extend(name.as_bytes());
        names.push(0);
    }
    align(&mut names, 0xff);
    align(&mut writer.data, 0);
    let local = writer.data.len();
    for (source, target) in &writer.fixups {
        writer.data.extend(u32_bytes(*source)?);
        writer.data.extend(u32_bytes(*target)?);
    }
    align(&mut writer.data, 0xff);
    let virtuals = writer.data.len();
    for (source, name) in &writer.finishes {
        writer.data.extend(u32_bytes(*source)?);
        writer.data.extend(0u32.to_le_bytes());
        writer.data.extend(u32_bytes(classes[name])?);
    }
    align(&mut writer.data, 0xff);
    let end = writer.data.len();
    let mut header = vec![0; 64];
    put(&mut header, 0, &0x57e0_e057u32.to_le_bytes())?;
    put(&mut header, 4, &0x10c0_c010u32.to_le_bytes())?;
    put(&mut header, 12, &9u32.to_le_bytes())?;
    put(&mut header, 16, &[8, 1, 0, 1])?;
    let class = &objects.get(&1).context("Native cloth root")?.name;
    for (i, value) in [3, 2, root, 0, classes[class]].into_iter().enumerate() {
        put(&mut header, 20 + i * 4, &u32_bytes(value)?)?;
    }
    put(&mut header, 40, b"hk_2012.2.0-r1\0\xff")?;
    put(&mut header, 60, &[0xff; 4])?;
    let start = 208 + names.len();
    for (name, values) in [
        (
            "__classnames__",
            [
                208,
                names.len(),
                names.len(),
                names.len(),
                names.len(),
                names.len(),
                names.len(),
            ],
        ),
        ("__types__", [start, 0, 0, 0, 0, 0, 0]),
        (
            "__data__",
            [start, local, virtuals, virtuals, end, end, end],
        ),
    ] {
        let at = header.len();
        header.resize(at + 20, 0);
        put(&mut header, at, name.as_bytes())?;
        for value in values {
            header.extend(u32_bytes(value)?);
        }
    }
    header.extend(names);
    header.extend(writer.data);
    Ok(header)
}

impl Writer<'_> {
    fn alloc(&mut self, size: usize) -> Result<usize> {
        align(&mut self.data, 0);
        let at = self.data.len();
        let end = at
            .checked_add(size)
            .context("Native cloth allocation overflow")?;
        ensure!(
            end <= 256 * 1024 * 1024,
            "Native cloth exceeds the supported size"
        );
        self.data.resize(end, 0);
        Ok(at)
    }

    fn object(&mut self, id: usize, depth: usize) -> Result<usize> {
        ensure!(
            depth < 128,
            "Native cloth object nesting exceeds supported bounds"
        );
        if let Some(offset) = self.offsets.get(&id) {
            return Ok(*offset);
        }
        let object = self
            .objects
            .get(&id)
            .context("Unresolved native cloth object")?
            .clone();
        ensure!(
            self.schema.native(&object.name)?.signature.is_some(),
            "Missing native cloth object signature"
        );
        let at = self.alloc(self.schema.native(&object.name)?.size)?;
        self.offsets.insert(id, at);
        self.finishes.insert(at, object.name.clone());
        self.record(at, &object.name, &object.value, depth + 1)?;
        Ok(at)
    }

    fn record(&mut self, at: usize, name: &str, value: &Value, depth: usize) -> Result<()> {
        let fields = value.as_object().context("Native cloth record")?;
        let members = self.schema.members(name)?;
        ensure!(
            fields.len() == members.len(),
            "Native cloth record fields differ"
        );
        for member in members {
            self.field(
                at + member.offset,
                &member,
                fields
                    .get(&member.name)
                    .context("Missing native cloth field")?,
                depth + 1,
            )
            .with_context(|| format!("{name}.{}", member.name))?;
        }
        Ok(())
    }

    fn field(&mut self, at: usize, member: &Member, value: &Value, depth: usize) -> Result<()> {
        ensure!(
            depth < 128,
            "Native cloth field nesting exceeds supported bounds"
        );
        let class = member.class.as_deref();
        if member.length != 0 {
            let values = array(value)?;
            ensure!(
                values.len() == member.length,
                "Native cloth tuple length differs"
            );
            let stride = self.schema.size(member.kind, class)?;
            let mut element = member.clone();
            element.length = 0;
            for (i, v) in values.iter().enumerate() {
                self.field(at + i * stride, &element, v, depth + 1)?;
            }
            return Ok(());
        }
        match member.kind {
            24 | 31 => {
                let mut element = member.clone();
                element.kind = member.subtype;
                self.field(at, &element, value, depth + 1)?;
            }
            1..=10 => {
                let size = self.schema.size(member.kind, None)?;
                let signed = matches!(member.kind, 3 | 5 | 7 | 9);
                let bytes = if signed {
                    let value = value.as_i64().context("Native cloth signed integer")?;
                    let shift = 64 - size * 8;
                    ensure!(
                        (value << shift) >> shift == value,
                        "Cloth integer exceeds its native width"
                    );
                    value.to_le_bytes()
                } else {
                    let value = value.as_u64().context("Native cloth unsigned integer")?;
                    ensure!(
                        size == 8 || value < (1u64 << (size * 8)),
                        "Cloth integer exceeds its native width"
                    );
                    if member.kind == 1 {
                        ensure!(value <= 1, "Invalid cloth Boolean");
                    }
                    value.to_le_bytes()
                };
                put(&mut self.data, at, &bytes[..size])?;
            }
            11 => {
                let value = value.as_f64().context("Native cloth float")? as f32;
                ensure!(value.is_finite(), "Nonfinite native cloth float");
                put(&mut self.data, at, &value.to_le_bytes())?;
            }
            12..=18 => {
                let values = array(value)?;
                ensure!(
                    values.len() == self.schema.size(member.kind, None)? / 4,
                    "Native cloth vector size differs"
                );
                for (i, value) in values.iter().enumerate() {
                    let value = value.as_f64().context("Native cloth vector lane")? as f32;
                    ensure!(value.is_finite(), "Nonfinite native cloth vector");
                    put(&mut self.data, at + i * 4, &value.to_le_bytes())?;
                }
            }
            29 | 33 => {
                let value = value.as_str().context("Native cloth string")?;
                ensure!(
                    !value.as_bytes().contains(&0),
                    "Embedded null in cloth string"
                );
                let target = self.alloc(value.len() + 1)?;
                put(&mut self.data, target, value.as_bytes())?;
                self.fixups.insert(at, target);
            }
            20 => {
                if !value.is_null() {
                    let target = self.object(integer(&value["object"])?, depth + 1)?;
                    self.fixups.insert(at, target);
                }
            }
            22 => {
                let values = array(value)?;
                let count = u32::try_from(values.len())?;
                ensure!(
                    count < 0x4000_0000,
                    "Native cloth array count exceeds capacity bits"
                );
                put(&mut self.data, at + 8, &count.to_le_bytes())?;
                put(
                    &mut self.data,
                    at + 12,
                    &(count | 0x8000_0000).to_le_bytes(),
                )?;
                if !values.is_empty() {
                    let stride = self.schema.size(member.subtype, class)?;
                    let target = self.alloc(
                        stride
                            .checked_mul(values.len())
                            .context("Cloth array allocation overflow")?,
                    )?;
                    self.fixups.insert(at, target);
                    let mut element = member.clone();
                    element.kind = member.subtype;
                    for (i, value) in values.iter().enumerate() {
                        self.field(target + i * stride, &element, value, depth + 1)?;
                    }
                }
            }
            25 => self.record(
                at,
                class.context("Native cloth struct class")?,
                value,
                depth + 1,
            )?,
            30 => put(
                &mut self.data,
                at,
                &u16::try_from(integer(value)?)?.to_le_bytes(),
            )?,
            kind => anyhow::bail!("Unsupported native cloth field kind {kind}"),
        }
        Ok(())
    }
}
