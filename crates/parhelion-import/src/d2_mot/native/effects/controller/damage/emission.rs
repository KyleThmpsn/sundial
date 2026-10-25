//! Native damage record bodies and typed array emission.
use super::*;

pub(super) struct Emitter<'a> {
    pub source: &'a Payload,
    pub template: &'a Payload,
    pub source_records: &'a BTreeMap<usize, Record>,
    pub descriptors: &'a BTreeMap<usize, usize>,
    pub default: &'a [u8; 96],
    pub si: usize,
    pub sd: usize,
    pub native_settings: usize,
    pub owner_tag: u32,
}

impl Emitter<'_> {
    pub(super) fn array(&self, writer: &mut Writer, old: usize, row: Array) -> Result<()> {
        let source = self.source;
        let source_records = self.source_records;
        let si = self.si;
        writer.prefix(0x80809FBD, 16)?;
        writer.positions.insert(old, writer.bytes.len());
        let class = match row.class {
            0x80809590 => 0x80809788,
            0x80809591 => 0x80809789,
            0x8080907C => 0x808091A4,
            class => class,
        };
        writer.bytes.extend((row.count as u64).to_le_bytes());
        writer.bytes.extend(u64::from(class).to_le_bytes());
        match row.class {
            0x80809590 | 0x80809591 => {
                let actual = if row.class == 0x80809590 {
                    0x80809591
                } else {
                    0x80809590
                };
                for index in 0..row.count {
                    let r = source_records
                        .get(&(row.header + 16 + index * row.stride))
                        .context("damage input array record missing")?;
                    ensure!(r.class == actual, "damage input array record class differs");
                }
            }
            0x8080907C => {
                let from = row.header + 16;
                ensure!(
                    source.u32(from + 8)? == 0x80802D47 && source.u32(from + 12)? == 0x00050003,
                    "damage lifecycle methods differ"
                );
                ensure!(
                    source.pointer(from)? == si,
                    "damage lifecycle parent differs"
                );
                writer.pointer(writer.bytes.len(), source.pointer(from)?);
                writer.bytes.extend(source.bytes::<8>(from)?);
                writer.bytes.extend(0x80803778u32.to_le_bytes());
                writer.bytes.extend(source.bytes::<4>(from + 12)?);
            }
            _ => writer.bytes.extend(row.data),
        }
        Ok(())
    }

    pub(super) fn record(&self, writer: &mut Writer, old: usize, record: Record) -> Result<()> {
        let source = self.source;
        let descriptors = self.descriptors;
        let si = self.si;
        let sd = self.sd;
        let owner_tag = self.owner_tag;
        let class = record.class;
        let size = match class {
            0x80802960 | 0x80808632 | 0x80808630 => 48,
            0x80802D47 => 88,
            0x8080862F => 96,
            0x80808631 => match old
                .checked_sub(sd)
                .context("damage inline program offset")?
            {
                0x58 | 0xB0 => 88,
                0x108 => 104,
                0x170 => 80,
                _ => bail!("unexpected damage inline program"),
            },
            0x80809A9F => 96,
            0x80809A9E => 272,
            0x80809590 => 40,
            0x80809591 => 96,
            0x80802959 | 0x80802957 => 24,
            0x8080295A | 0x80802958 => 32,
            _ => bail!("unsupported damage record"),
        };
        if old == si || matches!(class, 0x8080295A | 0x80802958) {
            writer.prefix(native(source.u32(old - 4)?)?, 16)?;
        }
        if old == sd || matches!(class, 0x80802959 | 0x80802957) {
            writer.prefix(native(source.u32(old - 4)?)?, 8)?;
        }
        let new = writer.bytes.len();
        writer.positions.insert(old, new);
        let mut bytes = self.record_bytes(writer, class, old, new, size)?;
        put(&mut bytes, 0, &owner_tag.to_le_bytes())?;
        put(&mut bytes, 4, &native(class)?.to_le_bytes())?;
        writer.fixes.push(Fix {
            at: new + 8,
            source: record.twin,
            absolute: true,
        });
        if matches!(
            class,
            0x80808632 | 0x80808630 | 0x80809A9F | 0x80809591 | 0x8080295A | 0x80802958
        ) {
            writer.pointer(new + 16, source.pointer(old + 16)?);
        }
        if class == 0x80809590 {
            ensure!(
                source.u64(old + 16)? == 0
                    && source.u64(old + 24)? == 0x808095CE
                    && source.u32(old + 36)? == 0,
                "damage scalar input definition differs"
            );
            put(&mut bytes, 24, &0x808097C1u64.to_le_bytes())?;
        }
        let source_size = match class {
            0x80809591 => 48,
            0x80809A9F => 112,
            0x80809A9E => 344,
            _ => size,
        };
        for (&field, &header) in descriptors.range(old..old + source_size) {
            writer.pointer(new + field - old + 8, header);
        }
        if class == 0x80808631 && old == sd + 0x108 {
            for field in [80, 88] {
                if source.u64(old + field)? != 0 {
                    writer.pointer(new + field, source.pointer(old + field)?);
                }
            }
        }
        if matches!(class, 0x80802959 | 0x80802957) {
            ensure!(source.u64(old + 16)? == 0, "damage wrapper state differs");
        }
        writer.bytes.extend(bytes);
        Ok(())
    }

    pub(super) fn record_bytes(
        &self,
        writer: &mut Writer,
        class: u32,
        old: usize,
        new: usize,
        size: usize,
    ) -> Result<Vec<u8>> {
        let source = self.source;
        let template = self.template;
        let default = self.default;
        let native_settings = self.native_settings;
        let mut bytes = vec![0; size];
        match class {
            0x80809A9F => bytes[..80].copy_from_slice(&source.bytes::<80>(old)?),
            0x80809A9E => {
                bytes[..32].copy_from_slice(&source.bytes::<32>(old)?);
                put(&mut bytes, 24, &0x80803890u64.to_le_bytes())?;
                for (sm, sn, _, _) in PROVIDERS {
                    ensure!(
                        source.bytes::<8>(old + sm + 24)? == [0; 8],
                        "damage provider extension is initialized"
                    );
                    ensure!(
                        source.bytes::<8>(old + sm + 16)?
                            == template.bytes::<8>(native_settings + sn + 16)?,
                        "damage provider state differs from native defaults"
                    );
                    bytes[sn..sn + 24].copy_from_slice(&source.bytes::<24>(old + sm)?);
                    put(
                        &mut bytes,
                        sn + 8,
                        &template.u32(native_settings + sn + 8)?.to_le_bytes(),
                    )?;
                    writer.pointer(new + sn, source.pointer(old + sm)?);
                }
                for (sm, sn) in [(0x100, 0xC8), (0x128, 0xE8), (0x150, 0x108)] {
                    bytes[sn..sn + 8].copy_from_slice(&source.bytes::<8>(old + sm)?);
                }
            }
            0x80809591 => {
                ensure!(
                    source.bytes::<24>(old + 24)?
                        == [
                            0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                            0, 0, 0
                        ],
                    "damage scalar input runtime is initialized"
                );
                bytes.copy_from_slice(default);
            }
            _ => bytes.copy_from_slice(
                source
                    .0
                    .get(old..old + size)
                    .context("damage record extent")?,
            ),
        }
        Ok(bytes)
    }
}
