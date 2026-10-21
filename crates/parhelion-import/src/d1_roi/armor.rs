//! Armor identity and body selection from the Xbox One Rise of Iron investment tables.
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

mod sets;

#[derive(Clone, Debug, Serialize)]
pub struct Armor {
    pub hash: u32,
    pub name: String,
    pub kind: String,
    pub class_type: u8,
    pub slot: u8,
    pub rarity: u8,
    pub definition: u32,
    pub strings: u32,
    pub arrangements: Vec<u16>,
    /// A complete five-slot source palette family, when the source is unambiguous.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_set: Option<String>,
}

impl Source {
    pub(super) fn table(&self, class: u32) -> Result<(u32, Payload)> {
        let tags = self.classes(class);
        ensure!(
            tags.len() == 1,
            "Missing or ambiguous Rise of Iron table {class:08X}"
        );
        Ok((tags[0], self.read(tags[0])?))
    }

    pub fn armor(&self) -> Result<Vec<Armor>> {
        ensure!(
            self.report
                .selected
                .iter()
                .all(|package| package.platform == 8),
            "Only the Xbox One build of Rise of Iron is supported"
        );
        let (_, definitions) = self.table(0x80801933)?;
        let (_, strings) = self.table(0x808034C7)?;
        let definitions_rows = definitions.array(8, 24, Some(0x808016BD))?;
        let string_rows = strings.array(8, 16, Some(0x808033E8))?;
        ensure!(
            definitions_rows.len() == string_rows.len(),
            "Item and string ordinals disagree"
        );
        let mut labels = super::localization::Labels::new(self)?;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for (definition_row, string_row) in definitions_rows.into_iter().zip(string_rows) {
            if let Some(item) = self.armor_item(
                definitions.u32(definition_row)?,
                definitions.u32(definition_row + 16)?,
                strings.u32(string_row + 8)?,
                &mut labels,
            )? {
                ensure!(
                    seen.insert(item.hash),
                    "Duplicate armor item {:08X}",
                    item.hash
                );
                result.push(item);
            }
        }
        ensure!(
            !result.is_empty(),
            "No equippable armor in the Rise of Iron source"
        );
        sets::assign(self, &mut result)?;
        result.sort_by(|left, right| {
            (left.name.as_str(), left.hash).cmp(&(right.name.as_str(), right.hash))
        });
        self.unchanged()?;
        Ok(result)
    }

    fn armor_item(
        &self,
        hash: u32,
        definition: u32,
        strings_tag: u32,
        labels: &mut super::localization::Labels,
    ) -> Result<Option<Armor>> {
        let text = self.read(strings_tag)?;
        ensure!(
            self.reference(strings_tag)? == 0x80803484,
            "Unexpected armor string class"
        );
        let class_type = match text.u32(0x98)? {
            0xD60E6BA5 => 0,
            0xD74EBFB3 => 1,
            0x82E15A90 => 2,
            _ => return Ok(None),
        };

        let data = self.read(definition)?;
        ensure!(
            self.reference(definition)? == 0x80801806 && data.u32(0x78)? == hash,
            "Armor definition identity mismatch for {hash:08X}"
        );
        if data.u64(0x18)? == 0 || data.u64(0x60)? == 0 {
            return Ok(None);
        }
        let client = data.pointer(0x18)?;
        ensure!(
            client >= 4 && data.u32(client - 4)? == 0x80801706,
            "Unexpected item client record"
        );
        let slot = data.u8(client + 0x18)?;
        if !(1..=5).contains(&slot) {
            return Ok(None);
        }
        let gear = data.pointer(0x60)?;
        ensure!(
            gear >= 4 && data.u32(gear - 4)? == 0x80801020,
            "Unexpected armor gear record"
        );
        let mut arrangements = Vec::new();
        for row in data.array(gear, 4, Some(0x8080101A))? {
            let class = data.u8(row)? as i8;
            let art = data.u16(row + 2)?;
            if (class == -1 || class == class_type as i8) && art != u16::MAX {
                arrangements.push(art);
                break;
            }
        }
        if arrangements.is_empty() {
            return Ok(None);
        }
        Ok(Some(Armor {
            hash,
            name: labels
                .optional_text(self, &text, 0x78)?
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("Unnamed Armor ({hash:08X})")),
            kind: labels.text(self, &text, 0x80)?.trim().to_owned(),
            class_type,
            slot,
            rarity: data.u8(0x8A)?,
            definition,
            strings: strings_tag,
            arrangements,
            collection_set: None,
        }))
    }

    /// Export both body variants, component owners, model streams and texture plates.
    /// Material execution, cloth simulation and native D2 conversion remain separate work.
    pub fn export_armor(&self, item: u32, output: &Path) -> Result<PathBuf> {
        let output = self.output(output)?;
        let armor = self
            .armor()?
            .into_iter()
            .find(|armor| armor.hash == item)
            .context("Selected source armor is missing")?;
        let mut assets = BTreeMap::new();
        let bodies = self.armor_bodies(&armor, &mut assets)?;
        let ornaments = self.ornaments(&armor, &mut assets)?;
        let dyes = self.dyes(armor.definition, &mut assets)?;
        self.unchanged()?;
        fs::create_dir_all(&output)?;
        let assets = self.write_assets(&output, assets)?;
        let path = output.join("armor.json");
        crate::io::write_json(
            &path,
            &json!({"source":"d1_roi","native_conversion":false,"item":armor,
            "builds":self.report.builds,"package_stamp":self.report.stamp,"bodies":bodies,"ornaments":ornaments,"dyes":dyes,"assets":assets,
            "limits":["Source armor, ornaments, material programs and texture bytes are preserved. Material execution, body skinning, effects and cloth still need native D2 translation and gameplay verification."]}),
        )?;
        Ok(path)
    }

    pub(super) fn armor_bodies(
        &self,
        armor: &Armor,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Vec<Value>> {
        let (art_tag, art) = self.table(0x808034D7)?;
        let art_rows = art.array(8, 24, Some(0x80803433))?;
        let (assignments_tag, assignment_data) = self.table(0x80803AAA)?;
        let mut assignments = BTreeMap::new();
        for row in assignment_data.array(8, 8, Some(0x80803AA9))? {
            ensure!(
                assignments
                    .insert(assignment_data.u32(row)?, assignment_data.u32(row + 4)?)
                    .is_none(),
                "Duplicate source entity assignment"
            );
        }
        for tag in [armor.definition, armor.strings, art_tag, assignments_tag] {
            assets.insert(tag, self.read(tag)?);
        }
        let mut bodies = Vec::new();
        for &arrangement in &armor.arrangements {
            let row = *art_rows
                .get(usize::from(arrangement))
                .context("Armor arrangement exceeds source table")?;
            ensure!(
                art.u64(row + 8)? == 0,
                "Armor uses an unsupported multiple-assignment arrangement"
            );
            for (body, offset) in [("masculine", 0), ("feminine", 4)] {
                let key = art.u32(row + offset)?;
                let parent = *assignments
                    .get(&key)
                    .context("Armor body has no entity assignment")?;
                bodies.push(self.body(body, arrangement, key, parent, assets)?);
            }
        }
        Ok(bodies)
    }

    pub(super) fn body(
        &self,
        body: &str,
        arrangement: u16,
        key: u32,
        parent: u32,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let parent_data = self.read(parent)?;
        let entity = parent_data.u32(16)?;
        assets.insert(parent, parent_data);
        ensure!(
            self.reference(entity)? == 0x80800734,
            "Armor assignment does not resolve to an entity"
        );
        let entity_data = self.read(entity)?;
        let (models, components) = self.body_models(&entity_data, assets)?;
        ensure!(
            !models.is_empty(),
            "Armor body contains no supported model component"
        );
        assets.insert(entity, entity_data);
        Ok(
            json!({"body":body,"arrangement":arrangement,"assignment":key,"parent":parent,"entity":entity,"components":components,"models":models}),
        )
    }

    fn body_models(
        &self,
        entity_data: &Payload,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<(Vec<Value>, Vec<Value>)> {
        let mut models = Vec::new();
        let mut components = Vec::new();
        for component in entity_data.array(0x20, 12, Some(0x80800715))? {
            let owner = entity_data.u32(component)?;
            let resource = self.read(owner)?;
            let mut entry = json!({"owner":owner,"class":self.reference(owner)?});
            if self.reference(owner)? == 0x80800861 && resource.u64(24)? != 0 {
                let at = resource.pointer(24)?;
                ensure!(at >= 4, "Invalid armor component pointer");
                let class = resource.u32(at - 4)?;
                entry["resource_class"] = json!(class);
                if matches!(class, 0x80801A9C | 0x80801BF6) {
                    let model = resource.u32(at + 0x1C4)?;
                    let (mut report, streams) = self.model(model)?;
                    report["component"] = json!(owner);
                    report["cloth"] = json!(class == 0x80801BF6);
                    if class == 0x80801BF6 {
                        report["cloth_solver"] = self.cloth_solver(&resource, at, assets)?;
                    }
                    report["plates"] = self.plates(&resource, at, assets)?;
                    assets.extend(streams);
                    models.push(report);
                }
            }
            components.push(entry);
            assets.insert(owner, resource);
        }

        Ok((models, components))
    }

    fn plates(
        &self,
        resource: &Payload,
        at: usize,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let mut result = Vec::new();
        // Xbox One's component descriptors expand its plate rows to 56 bytes.
        for row in resource.array(at + 0x220, 56, Some(0x80801B12))? {
            let tag = resource.u32(row + 48)?;
            ensure!(
                self.reference(tag)? == 0x80801C3C,
                "Unexpected armor plate set"
            );
            let set = self.read(tag)?;
            let mut plates = Vec::new();
            for (kind, offset) in [("albedo", 0x24), ("normal", 0x28), ("material", 0x2C)] {
                let tag = set.u32(offset)?;
                if matches!(tag, 0 | u32::MAX) {
                    continue;
                }
                ensure!(
                    self.reference(tag)? == 0x80800147,
                    "Unexpected texture plate class"
                );
                let plate = self.read(tag)?;
                let mut placements = Vec::new();
                for row in plate.array(0x10, 20, Some(0x80800163))? {
                    let texture = plate.u32(row)?;
                    let metadata = self.texture(texture, assets)?;
                    let data = metadata["data"].as_u64().context("Plate texture data")?;
                    placements.push(json!({"texture":texture,"data":data,"descriptor":metadata,
                        "x":plate.u32(row+4)?,"y":plate.u32(row+8)?,"width":plate.u32(row+12)?,"height":plate.u32(row+16)?}));
                }
                assets.insert(tag, plate);
                plates.push(json!({"kind":kind,"tag":tag,"placements":placements}));
            }
            assets.insert(tag, set);
            result.push(json!({"set":tag,"plates":plates}));
        }
        Ok(json!(result))
    }
}
