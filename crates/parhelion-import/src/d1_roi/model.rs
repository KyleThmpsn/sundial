//! Checked D1 model descriptors and original stream export for armor translation.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

impl Source {
    pub fn export_model(&self, tag: u32, output: &Path) -> Result<PathBuf> {
        let output = self.output(output)?;
        let (mut report, assets) = self.model(tag)?;
        self.unchanged()?;
        fs::create_dir_all(&output)?;
        let manifest = self.write_assets(&output, assets)?;
        report["source"] = json!("d1_roi");
        report["native_conversion"] = json!(false);
        report["builds"] = json!(self.report.builds);
        report["package_stamp"] = json!(self.report.stamp);
        report["assets"] = json!(manifest);
        let path = output.join("model.json");
        crate::io::write_json(&path, &report)?;
        Ok(path)
    }

    pub(super) fn write_assets(
        &self,
        output: &Path,
        assets: BTreeMap<u32, Payload>,
    ) -> Result<Vec<Value>> {
        let mut manifest = Vec::new();
        for (tag, payload) in assets {
            let file = format!("{tag:08X}.bin");
            fs::write(output.join(&file), &payload.0)?;
            manifest.push(json!({"tag":tag,"file":file,"bytes":payload.0.len(),
                "sha256":hex::encode(Sha256::digest(&payload.0)),
                "package":self.package(tag)?.header.path,"reference":self.reference(tag)?}));
        }
        Ok(manifest)
    }

    pub(super) fn model(&self, tag: u32) -> Result<(Value, BTreeMap<u32, Payload>)> {
        ensure!(
            self.reference(tag)? == 0x80801AB5,
            "Tag {tag:08X} is not a Rise of Iron model"
        );
        let model = self.read(tag)?;
        let mut assets = BTreeMap::new();
        let mut meshes = Vec::new();
        let mut materials = BTreeMap::new();
        for at in model.array(0x10, 0xA0, Some(0x80801BBF))? {
            let mut streams = Vec::new();
            for (offset, role, subtype) in [
                (0x30, "positions", 4),
                (0x34, "attributes", 4),
                (0x38, "weights", 4),
                (0x3C, "auxiliary", 4),
                (0x40, "indices", 6),
            ] {
                let header_tag = model.u32(at + offset)?;
                if matches!(header_tag, 0 | u32::MAX) {
                    continue;
                }
                let entry = self
                    .package(header_tag)?
                    .reader
                    .entry(TagHash(header_tag).entry_index() as usize)
                    .ok_or(MissingTag(header_tag))?;
                ensure!(
                    entry.file_type == 32 && entry.file_subtype == subtype,
                    "Unexpected {role} header type for {header_tag:08X}"
                );
                let header = self.read(header_tag)?;
                let data = self.read(entry.reference)?;
                let declared = header.u32(if subtype == 6 { 8 } else { 0 })? as usize;
                ensure!(
                    declared == data.0.len(),
                    "Declared {role} stream size disagrees with its data for {header_tag:08X}"
                );
                let stride = if subtype == 6 {
                    ensure!(header.u8(1)? <= 1, "Invalid source index width");
                    if header.u8(1)? == 1 { 4 } else { 2 }
                } else {
                    usize::from(header.u16(4)?)
                };
                ensure!(
                    stride > 0 && data.0.len() % stride == 0,
                    "Invalid {role} stream extent for {header_tag:08X}"
                );
                streams.push(json!({"role":role,"header":header_tag,"data":entry.reference,"stride":stride,"count":data.0.len()/stride}));
                assets.insert(header_tag, header);
                assets.insert(entry.reference, data);
            }
            let mut parts = Vec::new();
            for part in model.array(at + 0x48, 0x24, Some(0x80801AEF))? {
                let material = model.u32(part)?;
                if !matches!(material, 0 | u32::MAX) && !materials.contains_key(&material) {
                    materials.insert(material, self.material(material, &mut assets)?);
                }
                let first = model.u32(part + 8)?;
                let count = model.u32(part + 12)?;
                let index_count = streams
                    .iter()
                    .find(|stream| stream["role"] == "indices")
                    .and_then(|stream| stream["count"].as_u64())
                    .context("Mesh has no index stream")?;
                ensure!(
                    u64::from(first) + u64::from(count) <= index_count,
                    "Source part exceeds index stream"
                );
                parts.push(
                    json!({"material":model.u32(part)?,"variant_shader":model.i16(part+4)?,
                    "primitive":model.u16(part+6)?,"index_start":first,"index_count":count,
                    "external_id":model.i16(part+0x18)?,"flags":model.u16(part+0x1C)?,
                    "dye":model.u8(part+0x1E)?,"lod":model.u8(part+0x1F)?}),
                );
            }
            let floats = |offset, count| {
                (0..count)
                    .map(|index| model.f32(at + offset + index * 4))
                    .collect::<Result<Vec<_>>>()
            };
            meshes.push(json!({"scale":floats(0,4)?,"translation":floats(0x10,4)?,
                "uv_scale":floats(0x20,2)?,"uv_translation":floats(0x28,2)?,"streams":streams,"parts":parts,
                "stage_offsets":(0..20).map(|index|model.u16(at+0x58+index*2)).collect::<Result<Vec<_>>>()?,
                "stage_layouts":(0..19).map(|index|model.u8(at+0x80+index)).collect::<Result<Vec<_>>>()?}));
        }
        ensure!(!meshes.is_empty(), "Source model has no meshes");
        assets.insert(tag, model);
        Ok((
            json!({"model":tag,"meshes":meshes,"materials":materials.into_values().collect::<Vec<_>>()}),
            assets,
        ))
    }
}
