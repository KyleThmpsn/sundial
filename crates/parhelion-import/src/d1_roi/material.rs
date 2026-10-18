//! Preserve the actual Xbox material programs and their bound resources.
use super::*;
use serde_json::{Value, json};

impl Source {
    pub(super) fn constant_buffer(&self, tag: u32) -> Result<(Payload, Vec<u8>)> {
        ensure!(
            self.reference(tag)? == 0x80801AA5,
            "Unsupported source constant buffer class"
        );
        let data = self.read(tag)?;
        let values = data
            .array(8, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| data.u8(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !values.is_empty() && values.len().is_multiple_of(16),
            "Source constant buffer has an invalid vector extent"
        );
        Ok((data, values))
    }

    pub(super) fn texture(&self, tag: u32, assets: &mut BTreeMap<u32, Payload>) -> Result<Value> {
        let entry = self
            .package(tag)?
            .reader
            .entry(TagHash(tag).entry_index() as usize)
            .ok_or(MissingTag(tag))?;
        ensure!(
            entry.file_type == 32 && (1..=3).contains(&entry.file_subtype),
            "Unexpected texture header {tag:08X}"
        );
        let header = self.read(tag)?;
        ensure!(
            header.u32(0x2C)? == 0xBEEFCAFE,
            "Unsupported Xbox texture descriptor {tag:08X}"
        );
        let width = header.u16(0x30)?;
        let height = header.u16(0x32)?;
        ensure!(width > 0 && height > 0, "Empty source texture dimensions");
        let buffer = self
            .package(entry.reference)?
            .reader
            .entry(TagHash(entry.reference).entry_index() as usize)
            .ok_or(MissingTag(entry.reference))?;
        let high_mips = if buffer.file_type == 65 {
            ensure!(
                buffer.file_subtype == entry.file_subtype,
                "Streamed texture subtype mismatch"
            );
            let high = self
                .package(buffer.reference)?
                .reader
                .entry(TagHash(buffer.reference).entry_index() as usize)
                .ok_or(MissingTag(buffer.reference))?;
            ensure!(
                high.file_type == 5 && high.file_subtype == entry.file_subtype,
                "Unsupported high mip payload"
            );
            assets.insert(buffer.reference, self.read(buffer.reference)?);
            Some(buffer.reference)
        } else {
            ensure!(
                matches!(buffer.file_type, 1 | 3) && buffer.file_subtype == entry.file_subtype,
                "Unsupported texture payload"
            );
            None
        };
        let report = json!({"tag":tag,"data":entry.reference,"high_mips":high_mips,"format":header.u32(0)?,"width":width,"height":height,
            "depth":header.u16(0x34)?,"layers":header.u16(0x36)?,"tile_mode":header.u32(4)?,"mips":header.u8(0x39)?});
        assets.insert(entry.reference, self.read(entry.reference)?);
        assets.insert(tag, header);
        Ok(report)
    }

    pub(super) fn material(&self, tag: u32, assets: &mut BTreeMap<u32, Payload>) -> Result<Value> {
        ensure!(
            self.reference(tag)? == 0x80801C32,
            "Unsupported Xbox material class for {tag:08X}"
        );
        let data = self.read(tag)?;
        let mut stages = Vec::new();
        for (name, at) in [
            ("vertex", 0x28),
            ("hull", 0xC8),
            ("domain", 0x168),
            ("geometry", 0x208),
            ("pixel", 0x2A8),
        ] {
            let shader = data.u32(at)?;
            if matches!(shader, 0 | u32::MAX) {
                continue;
            }
            let program = self.read(shader)?;
            let code = program.array(8, 1, Some(0x80800009))?;
            let start = *code.first().context("Empty shader program")?;
            ensure!(
                program.bytes::<4>(start)? == *b"DXBC",
                "Source shader is not DXBC"
            );
            let mut textures = Vec::new();
            for row in data.array(at + 0x10, 8, Some(0x80801B28))? {
                let source_texture = data.u32(row + 4)?;
                if matches!(source_texture, 0 | u32::MAX) {
                    textures.push(json!({"slot":data.u32(row)?,"tag":source_texture}));
                    continue;
                }
                let mut texture = self.texture(source_texture, assets)?;
                texture["slot"] = json!(data.u32(row)?);
                textures.push(texture);
            }
            let mut samplers = Vec::new();
            for row in data.array(at + 0x48, 16, Some(0x80801ACC))? {
                let sampler = data.u32(row)?;
                let entry = self
                    .package(sampler)?
                    .reader
                    .entry(TagHash(sampler).entry_index() as usize)
                    .ok_or(MissingTag(sampler))?;
                ensure!(
                    entry.file_type == 32 && entry.file_subtype == 16,
                    "Unsupported Xbox sampler header"
                );
                let header = self.read(sampler)?;
                let descriptor = self.read(entry.reference)?;
                ensure!(
                    header.0 == [0; 8]
                        && descriptor.0.len() == 60
                        && descriptor.0[52..] == [255; 8],
                    "Unsupported Xbox sampler descriptor"
                );
                assets.insert(entry.reference, descriptor);
                assets.insert(sampler, header);
                samplers.push(sampler);
            }
            let tfx = data
                .array(at + 0x28, 1, Some(0x80800009))?
                .into_iter()
                .map(|row| data.u8(row))
                .collect::<Result<Vec<_>>>()?;
            let vectors = |offset| -> Result<Vec<Vec<Value>>> {
                data.array(offset, 16, Some(0x80800090))?
                    .into_iter()
                    .map(|row| {
                        (0..4)
                            .map(|lane| {
                                let bits = data.u32(row + lane * 4)?;
                                let value = f32::from_bits(bits);
                                Ok(if value.is_finite() {
                                    json!(value)
                                } else {
                                    json!({"bits":format!("{bits:08X}")})
                                })
                            })
                            .collect()
                    })
                    .collect()
            };
            let external = data.u32(at + 0x84)?;
            let external = if matches!(external, 0 | u32::MAX) {
                None
            } else {
                let (buffer, values) = self.constant_buffer(external)?;
                assets.insert(external, buffer);
                Some(json!({"tag":external,"bytes":values.len(),"vectors":values.len()/16}))
            };
            stages.push(json!({"stage":name,"shader":shader,"shader_class":self.reference(shader)?,"code_offset":start,"code_bytes":code.len(),
                "textures":textures,"samplers":samplers,"tfx":tfx,"constants":vectors(at+0x38)?,"cbuffer":vectors(at+0x58)?,"external_cbuffer":external}));
            assets.insert(shader, program);
        }
        ensure!(!stages.is_empty(), "Material has no executable stage");
        assets.insert(tag, data);
        Ok(json!({"tag":tag,"stages":stages}))
    }
}
