//! Original equipment dye layers and their source textures.
use super::*;
use serde_json::{Value, json};

impl Source {
    pub(super) fn dyes(
        &self,
        definition: u32,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Vec<Value>> {
        let item = self.read(definition)?;
        let gear = item.pointer(0x60)?;
        let (table_tag, table) = self.table(0x80803420)?;
        let rows = table.array(8, 4, Some(0x808033CC))?;
        let mut requested = Vec::new();
        for (name, offset) in [("default", 0x30), ("custom", 0x50), ("fixed", 0x20)] {
            for row in item.array(gear + offset, 4, Some(0x8080101C))? {
                let index = item.u16(row + 2)?;
                let manifest = table.u32(*rows.get(usize::from(index)).context("Dye ordinal")?)?;
                requested.push((name, item.u8(row)?, index, manifest));
            }
        }
        if requested.is_empty() {
            return Ok(Vec::new());
        }
        let mut assignments = BTreeMap::new();
        for tag in self.classes(0x80800341) {
            let data = self.read(tag)?;
            if data.u64(8)? == 0 {
                continue;
            }
            let header = data.pointer(16)?;
            if data.u32(header + 8)? != 0x808005D7 {
                continue;
            }
            let mut used = false;
            for row in data.array(8, 8, Some(0x808005D7))? {
                let key = data.u32(row)?;
                if requested.iter().any(|(_, _, _, manifest)| *manifest == key) {
                    ensure!(
                        assignments.insert(key, data.u32(row + 4)?).is_none(),
                        "Ambiguous dye assignment {key:08X}"
                    );
                    used = true;
                }
            }
            if used {
                assets.insert(tag, data);
            }
        }
        let mut result = Vec::new();
        for (layer, channel, index, manifest) in requested {
            let parent = *assignments
                .get(&manifest)
                .context("Missing dye assignment")?;
            let parent_data = self.read(parent)?;
            let tag = parent_data.u32(0x10)?;
            ensure!(
                self.reference(tag)? == 0x80801AF4,
                "Dye assignment has an unsupported class"
            );
            let data = self.read(tag)?;
            let mut textures = Vec::new();
            for (role, at) in [
                ("decal", 0x20),
                ("detail_albedo", 0x60),
                ("detail_normal", 0x64),
            ] {
                let texture = data.u32(at)?;
                if matches!(texture, 0 | u32::MAX) {
                    continue;
                }
                let mut texture = self.texture(texture, assets)?;
                texture["role"] = json!(role);
                textures.push(texture);
            }
            let vector = |at| -> Result<[f32; 4]> {
                Ok([
                    data.f32(at)?,
                    data.f32(at + 4)?,
                    data.f32(at + 8)?,
                    data.f32(at + 12)?,
                ])
            };
            result.push(json!({"layer":layer,"channel":channel,"index":index,"manifest":manifest,"parent":parent,"tag":tag,
                "slot_type":data.u32(0x10)?,"textures":textures,"decal_transform":vector(0x30)?,"decal_blend":data.u32(0x40)?,
                "specular":vector(0x50)?,"detail_transform":vector(0x70)?,"normal_strength":vector(0x80)?,
                "primary":vector(0x90)?,"secondary":vector(0xA0)?,"subsurface":vector(0xB0)?}));
            assets.insert(parent, parent_data);
            assets.insert(tag, data);
        }
        assets.insert(table_tag, table);
        Ok(result)
    }
}
