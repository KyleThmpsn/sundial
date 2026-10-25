//! Preserve the source runtime's declared canvases and five BC mip levels.
use super::super::texture::{Texture, surface};
use super::material::Equipment;
use super::*;

impl Assembly<'_> {
    fn plate(&mut self, tag: u32) -> Result<String> {
        let symbol = format!("plate-{tag:08X}");
        if self.symbols.contains(&symbol) {
            return Ok(symbol);
        }
        ensure!(
            self.source.reference(tag)? == 0x80800147,
            "Source plate class differs"
        );
        let plate = self.source.read(tag)?;
        let kind = usize::from(plate.u16(8)?);
        let canvas = usize::from(plate.u16(10)?);
        // Xbox build 56578: 140248C40 indexes 1411F46F0 by equipment class
        // and canvas. Class 0 is player armor and class 1 is weapon.
        let canvases: &[usize] = if self.weapon {
            &[2048, 1024, 512]
        } else {
            &[2048, 2048, 1024, 1024]
        };
        let edge = *canvases
            .get(canvas)
            .context("Unknown canvas for the source equipment class")?;
        let format = *[72u32, 83, 71]
            .get(kind)
            .context("Unknown source plate channel")?;
        // 14089CD00 copies BC blocks without filtering, using these exact fallbacks.
        let fallback: &[u8] = match kind {
            0 => &[0x10, 0x84, 0x10, 0x84, 0, 0, 0, 0],
            1 => &[0x80, 0x7F, 0, 0, 0, 0, 0, 0, 0x80, 0x7F, 0, 0, 0, 0, 0, 0],
            2 => &[0xFF, 0xFF, 0, 0, 0, 0, 0, 0],
            _ => unreachable!(),
        };
        let block = fallback.len();
        let mut offsets = Vec::new();
        let mut bytes = Vec::new();
        for level in 0..5 {
            offsets.push(bytes.len());
            bytes.extend(fallback.repeat((edge >> (level + 2)).pow(2)));
        }
        let rows = plate.array(16, 20, Some(0x80800163))?;
        ensure!(!rows.is_empty(), "Required source plate is empty");
        let mut placements = Vec::new();
        for row in rows {
            let source = plate.u32(row)?;
            let (x, y, w, h) = (
                plate.u32(row + 4)? as usize,
                plate.u32(row + 8)? as usize,
                plate.u32(row + 12)? as usize,
                plate.u32(row + 16)? as usize,
            );
            ensure!(
                w > 0
                    && h > 0
                    && x.checked_add(w).is_some_and(|end| end <= edge)
                    && y.checked_add(h).is_some_and(|end| end <= edge),
                "Source plate placement exceeds its declared canvas"
            );
            let texture = self
                .source
                .linear_texture(source, self.options.xg_library.as_deref())?;
            let header = self.source.read(source)?;
            let valid = texture.layers == 1
                && texture.format == format
                && texture.mips == 5
                && usize::from(texture.width) == w
                && usize::from(texture.height) == h
                && usize::from(header.u8(0x38)?) * 2 == block;
            let mut input = 0;
            for (level, &output) in offsets.iter().enumerate() {
                let (bx, by, bw, bh, pitch) = (
                    x >> (level + 2),
                    y >> (level + 2),
                    w >> (level + 2),
                    h >> (level + 2),
                    edge >> (level + 2),
                );
                for line in 0..bh {
                    let to = output + ((by + line) * pitch + bx) * block;
                    if valid {
                        let from = input + line * bw * block;
                        let row = texture
                            .bytes
                            .get(from..from + bw * block)
                            .context("Source plate mip extent differs")?;
                        bytes[to..to + bw * block].copy_from_slice(row);
                    } else {
                        bytes[to..to + bw * block].copy_from_slice(&fallback.repeat(bw));
                    }
                }
                if valid {
                    input += surface(format, (w >> level).max(1), (h >> level).max(1))?.1;
                }
            }
            placements.push(
                json!({"source":source,"rectangle":[x,y,w,h],"source_runtime_fallback":!valid}),
            );
        }
        self.evidence.push(json!({"plate":tag,"symbol":symbol,"equipment_class":if self.weapon {"weapon"} else {"player"},"canvas":canvas,"edge":edge,"placements":placements,"format":format,"mips":5,"composition":"source_runtime_bc_block_copy","source_executable_build":"56578.17.04.12.1251.evil"}));
        self.texture_bytes(
            &symbol,
            Texture {
                format,
                width: edge as u16,
                height: edge as u16,
                depth: 1,
                layers: 1,
                mips: 5,
                bytes,
            },
        )
    }

    pub(super) fn equipment(&mut self, owner: &Payload, dyes: &[Value]) -> Result<Equipment> {
        let resource = owner.pointer(24)?;
        let rows = owner.array(resource + 0x220, 56, Some(0x80801B12))?;
        ensure!(
            rows.len() <= 1,
            "Source owner needs multiple plate selection semantics"
        );
        let mut plates = [String::new(), String::new(), String::new()];
        if let Some(row) = rows.first() {
            let set = self.source.read(owner.u32(row + 48)?)?;
            for (index, output) in plates.iter_mut().enumerate() {
                let tag = set.u32(0x24 + index * 4)?;
                if !matches!(tag, 0 | u32::MAX) {
                    *output = self.plate(tag)?;
                }
            }
        }
        let mut selected = BTreeMap::new();
        for dye in dyes {
            let tag = u32::try_from(dye["tag"].as_u64().context("Dye source")?)?;
            // 140884FE0 selects a local dye slot, not the investment channel.
            // Weapon investment channels 5 and 6 declare local slots 0 and 1.
            let slot = self.source.read(tag)?.u32(0x10)?;
            ensure!(slot < 4, "Source dye has an unsupported local slot");
            selected.insert(slot as u8, tag);
        }
        Ok(Equipment {
            plates,
            dyes: selected,
        })
    }
}
