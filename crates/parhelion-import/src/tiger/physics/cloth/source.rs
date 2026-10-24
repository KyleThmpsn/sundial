//! Account for every ITEM and PTCH before allowing semantic conversion.
use super::*;
use schema::Schema;

#[derive(Clone)]
struct Item {
    kind: u32,
    flag: u8,
    offset: usize,
    count: usize,
    span: usize,
}

#[derive(Clone)]
pub(super) struct Decoded {
    pub object: Object,
    pub flag: u8,
    pub previous: Option<[u32; 2]>,
}

fn slice(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(at..at.checked_add(len).context("Cloth range overflow")?)
        .context("Truncated cloth data")
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(slice(bytes, at, 4)?.try_into()?))
}

fn sections(
    bytes: &[u8],
    start: usize,
    end: usize,
    depth: usize,
    found: &mut BTreeMap<String, (usize, usize)>,
) -> Result<()> {
    ensure!(
        depth <= 4,
        "Cloth section nesting exceeds the supported format"
    );
    let mut at = start;
    while at < end {
        let field = u32::from_be_bytes(slice(bytes, at, 4)?.try_into()?);
        let size = (field & 0x3fff_ffff) as usize;
        let stop = at.checked_add(size).context("Cloth section overflow")?;
        ensure!(size >= 8 && stop <= end, "Invalid cloth section extent");
        let name = std::str::from_utf8(slice(bytes, at + 4, 4)?)?.to_owned();
        ensure!(
            found.insert(name, (at + 8, stop)).is_none(),
            "Duplicate cloth section"
        );
        if field >> 30 == 0 {
            sections(bytes, at + 8, stop, depth + 1, found)?;
        }
        at = stop;
    }
    ensure!(at == end, "Cloth sections do not cover their container");
    Ok(())
}

struct Reader<'a> {
    data: &'a [u8],
    schema: &'a Schema,
    items: BTreeMap<usize, Item>,
    patches: BTreeMap<usize, u32>,
    used: BTreeSet<usize>,
    assignments: BTreeMap<u32, String>,
    patch_types: BTreeMap<u32, String>,
    values: BTreeMap<usize, Decoded>,
}

pub(super) fn read(bytes: &[u8], schema: &Schema) -> Result<BTreeMap<usize, Decoded>> {
    ensure!(
        bytes.len() <= 64 * 1024 * 1024 && slice(bytes, 0, 16)? == [0; 16],
        "Unsupported cloth envelope"
    );
    let mut found = BTreeMap::new();
    sections(bytes, 16, bytes.len(), 0, &mut found)?;
    let section = |name: &str| -> Result<&[u8]> {
        let &(at, end) = found
            .get(name)
            .with_context(|| format!("Missing cloth {name} section"))?;
        slice(bytes, at, end - at)
    };
    ensure!(
        section("SDKV")? == b"20180100" && section("TCRF")? == [0; 24],
        "Unsupported cloth SDK or type reference"
    );
    let data = section("DATA")?;
    let entries = section("ITEM")?;
    ensure!(
        entries.len().is_multiple_of(12) && entries.len() / 12 <= 100_000,
        "Invalid cloth item table"
    );
    ensure!(slice(entries, 0, 12)? == [0; 12], "Cloth null item differs");
    let mut items = BTreeMap::new();
    let mut ordered = BTreeMap::new();
    for (index, row) in entries.chunks_exact(12).enumerate().skip(1) {
        let bits = word(row, 0)?;
        let flag = (bits >> 24) as u8;
        let offset = word(row, 4)? as usize;
        let count = word(row, 8)? as usize;
        ensure!(
            matches!(flag, 16 | 32) && count > 0 && (flag != 16 || count == 1),
            "Invalid cloth item kind or count"
        );
        ensure!(
            offset < data.len() && ordered.insert(offset, index).is_none(),
            "Overlapping cloth item allocations"
        );
        items.insert(
            index,
            Item {
                kind: bits & 0xff_ffff,
                flag,
                offset,
                count,
                span: 0,
            },
        );
    }
    let mut end = data.len();
    for (&offset, &index) in ordered.iter().rev() {
        items.get_mut(&index).context("Cloth allocation")?.span = end - offset;
        end = offset;
    }
    ensure!(end == 0, "Unaccounted cloth data prefix");
    let patch_data = section("PTCH")?;
    let mut patches = BTreeMap::new();
    let mut at = 0;
    while at < patch_data.len() {
        let kind = word(patch_data, at)?;
        let count = word(patch_data, at + 4)? as usize;
        at += 8;
        let offsets = slice(
            patch_data,
            at,
            count.checked_mul(4).context("Cloth patch overflow")?,
        )?;
        for row in offsets.chunks_exact(4) {
            let offset = word(row, 0)? as usize;
            slice(data, offset, 8)?;
            ensure!(
                patches.insert(offset, kind).is_none(),
                "Duplicate cloth pointer patch"
            );
        }
        at += offsets.len();
    }
    let mut reader = Reader {
        data,
        schema,
        items,
        patches,
        used: BTreeSet::new(),
        assignments: BTreeMap::new(),
        patch_types: BTreeMap::new(),
        values: BTreeMap::new(),
    };
    let root = schema
        .types
        .iter()
        .find(|(_, t)| t.name == "hclClothContainer")
        .context("Cloth root schema")?
        .0;
    reader.item(1, *root, 0)?;
    ensure!(
        reader.values.len() == reader.items.len(),
        "Unaccounted cloth items"
    );
    ensure!(
        reader.used.len() == reader.patches.len(),
        "Unaccounted cloth pointer patches"
    );
    Ok(reader.values)
}

fn assign(values: &mut BTreeMap<u32, String>, id: u32, name: String) -> Result<()> {
    if let Some(previous) = values.insert(id, name.clone()) {
        ensure!(
            previous == name,
            "Conflicting cloth type assignment for {id}"
        );
    }
    Ok(())
}

impl Reader<'_> {
    fn pointer(&mut self, at: usize, tid: u32) -> Result<usize> {
        let value = usize::try_from(u64::from_le_bytes(slice(self.data, at, 8)?.try_into()?))?;
        ensure!(
            value == 0 || (self.items.contains_key(&value) && self.patches.contains_key(&at)),
            "Unpatched cloth pointer"
        );
        if let Some(&kind) = self.patches.get(&at) {
            self.used.insert(at);
            assign(&mut self.patch_types, kind, self.schema.type_name(tid)?)?;
        }
        Ok(value)
    }

    fn item(&mut self, index: usize, mut tid: u32, depth: usize) -> Result<Value> {
        ensure!(depth < 128, "Cloth graph nesting exceeds supported bounds");
        if index == 0 {
            return Ok(Value::Null);
        }
        let row = self
            .items
            .get(&index)
            .context("Missing cloth item")?
            .clone();
        if row.flag == 16 {
            let actual = *self
                .schema
                .concrete
                .get(&row.kind)
                .with_context(|| format!("Unsupported cloth object type {}", row.kind))?;
            let mut parent = actual;
            while parent != 0 && parent != tid {
                parent = self.schema.source(parent)?.parent;
            }
            ensure!(
                parent == tid,
                "Cloth object does not implement its pointer type"
            );
            tid = actual;
        }
        let name = self.schema.type_name(tid)?;
        assign(&mut self.assignments, row.kind, name.clone())?;
        if let Some(value) = self.values.get(&index) {
            ensure!(value.object.name == name, "Conflicting cloth item type");
            return Ok(json!({"item":index}));
        }
        let stride = self.schema.source(tid)?.size;
        let extended = row.flag == 16 && name == "hclObjectSpaceMeshMeshDeformPOperator";
        let size = stride
            .checked_mul(row.count)
            .and_then(|s| s.checked_add(if extended { 8 } else { 0 }))
            .context("Cloth item size overflow")?;
        ensure!(
            size <= row.span && row.span - size < 16,
            "Cloth {name} allocation differs from its layout"
        );
        ensure!(
            slice(self.data, row.offset + size, row.span - size)?
                .iter()
                .all(|b| *b == 0),
            "Nonzero cloth allocation padding"
        );
        let previous = if extended {
            Some([
                word(self.data, row.offset + stride)?,
                word(self.data, row.offset + stride + 4)?,
            ])
        } else {
            None
        };
        self.values.insert(
            index,
            Decoded {
                object: Object {
                    name,
                    value: Value::Null,
                },
                flag: row.flag,
                previous,
            },
        );
        let values = (0..row.count)
            .map(|i| self.value(row.offset + i * stride, tid, depth + 1))
            .collect::<Result<Vec<_>>>()?;
        self.values
            .get_mut(&index)
            .context("Decoded cloth item")?
            .object
            .value = json!(values);
        Ok(json!({"item":index}))
    }

    fn value(&mut self, at: usize, tid: u32, depth: usize) -> Result<Value> {
        ensure!(depth < 128, "Cloth value nesting exceeds supported bounds");
        let row = self.schema.source(tid)?;
        let (fmt, size) = (row.format, row.size);
        let bytes = slice(self.data, at, size)?;
        match fmt & 15 {
            2 | 4 => {
                ensure!(
                    matches!(size, 1 | 2 | 4 | 8),
                    "Unsupported cloth integer width"
                );
                let mut raw = [0; 8];
                raw[..size].copy_from_slice(bytes);
                if fmt & 512 != 0 {
                    let shift = 64 - size * 8;
                    Ok(json!((i64::from_le_bytes(raw) << shift) >> shift))
                } else {
                    Ok(json!(u64::from_le_bytes(raw)))
                }
            }
            5 => {
                let number = match size {
                    2 => f64::from(
                        half::f16::from_bits(u16::from_le_bytes(bytes.try_into()?)).to_f32(),
                    ),
                    4 => f64::from(f32::from_le_bytes(bytes.try_into()?)),
                    8 => f64::from_le_bytes(bytes.try_into()?),
                    _ => anyhow::bail!("Unsupported cloth float width"),
                };
                ensure!(number.is_finite(), "Nonfinite cloth value");
                Ok(json!(number))
            }
            3 => {
                let index = self.pointer(at, tid)?;
                if index == 0 {
                    return Ok(json!(""));
                }
                let item = self.items.get(&index).context("Cloth string item")?.clone();
                ensure!(
                    item.kind == 8 && item.flag == 32,
                    "Cloth string declaration differs"
                );
                let data = slice(self.data, item.offset, item.count)?;
                ensure!(
                    item.count <= item.span
                        && item.span - item.count < 16
                        && slice(self.data, item.offset + item.count, item.span - item.count)?
                            .iter()
                            .all(|b| *b == 0),
                    "Invalid cloth string padding"
                );
                ensure!(
                    data.last() == Some(&0) && !data[..data.len() - 1].contains(&0),
                    "Unterminated cloth string"
                );
                let value = std::str::from_utf8(&data[..data.len() - 1])?.to_owned();
                assign(&mut self.assignments, item.kind, "char".into())?;
                self.values.insert(
                    index,
                    Decoded {
                        object: Object {
                            name: "char".into(),
                            value: json!(data),
                        },
                        flag: 32,
                        previous: None,
                    },
                );
                Ok(json!(value))
            }
            6 => {
                let index = self.pointer(at, tid)?;
                self.item(index, self.schema.subtype(tid)?, depth + 1)
            }
            7 => {
                let mut fields = serde_json::Map::new();
                for m in self.schema.source_members(tid)? {
                    fields.insert(m.name, self.value(at + m.offset, m.kind, depth + 1)?);
                }
                Ok(Value::Object(fields))
            }
            8 if row.name == "hkArray" => {
                ensure!(
                    slice(self.data, at + 8, 8)? == [0; 8],
                    "Cloth array header differs"
                );
                let index = self.pointer(at, tid)?;
                self.item(index, self.schema.subtype(tid)?, depth + 1)
            }
            8 if row.name == "hkPropertyBag" => {
                ensure!(
                    bytes.iter().all(|b| *b == 0),
                    "Nonempty cloth property bag is unsupported"
                );
                Ok(Value::Null)
            }
            8 => {
                let count = (fmt >> 8) as usize;
                let sub = self.schema.subtype(tid)?;
                let stride = self.schema.source(sub)?.size;
                ensure!(
                    count != 0 && stride.checked_mul(count) == Some(size),
                    "Cloth tuple extent differs"
                );
                Ok(json!(
                    (0..count)
                        .map(|i| self.value(at + i * stride, sub, depth + 1))
                        .collect::<Result<Vec<_>>>()?
                ))
            }
            _ if size == 0 => Ok(Value::Null),
            _ => anyhow::bail!("Unsupported cloth value format {fmt}"),
        }
    }
}
