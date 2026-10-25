//! Source weapon identities, selectable art and talent provenance.
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

mod runtime;
mod stats;

#[derive(Clone, Debug, Serialize)]
pub struct Weapon {
    pub hash: u32,
    pub name: String,
    pub kind: String,
    pub flavor: String,
    pub slot: u8,
    pub rarity: u8,
    pub definition: u32,
    pub strings: u32,
    pub pattern: u16,
    pub arrangements: Vec<u16>,
}

impl Source {
    pub fn weapons(&self) -> Result<Vec<Weapon>> {
        ensure!(
            self.report.selected.iter().all(|p| p.platform == 8),
            "Weapon discovery currently supports Xbox One Rise of Iron packages"
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
        for (row, text_row) in definitions_rows.into_iter().zip(string_rows) {
            let tag = definitions.u32(row + 16)?;
            let data = self.read(tag)?;
            let hash = definitions.u32(row)?;
            ensure!(
                self.reference(tag)? == 0x80801806 && data.u32(0x78)? == hash,
                "Source item definition identity differs"
            );
            if data.u64(0x18)? == 0 || data.u64(0x60)? == 0 {
                continue;
            }
            let client = data.pointer(0x18)?;
            ensure!(
                data.u32(client - 4)? == 0x80801706,
                "Source item client type differs"
            );
            let slot = data.u8(client + 0x18)?;
            if !(6..=8).contains(&slot) {
                continue;
            }
            let gear = data.pointer(0x60)?;
            ensure!(
                data.u32(gear - 4)? == 0x80801020,
                "Source weapon gear type differs"
            );
            let text_tag = strings.u32(text_row + 8)?;
            ensure!(
                self.reference(text_tag)? == 0x80803484,
                "Source weapon strings type differs"
            );
            let text = self.read(text_tag)?;
            let mut arrangements = Vec::new();
            for at in data.array(gear, 4, Some(0x8080101A))? {
                let index = data.u16(at + 2)?;
                if data.u8(at)? == 255 && index != u16::MAX {
                    arrangements.push(index);
                }
            }
            if arrangements.is_empty() {
                continue;
            }
            ensure!(
                seen.insert(hash),
                "Duplicate source weapon identity {hash:08X}"
            );
            result.push(Weapon {
                hash,
                name: labels
                    .optional_text(self, &text, 0x78)?
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Unnamed Weapon ({hash:08X})")),
                kind: labels.text(self, &text, 0x80)?.trim().to_owned(),
                flavor: labels.optional_text(self, &text, 0x88)?.unwrap_or_default(),
                slot,
                rarity: data.u8(0x8A)?,
                definition: tag,
                strings: text_tag,
                pattern: data.u16(gear + 0x60)?,
                arrangements,
            });
        }
        ensure!(
            !result.is_empty(),
            "No equippable weapons in the Rise of Iron source"
        );
        result.sort_by(|a, b| (&a.name, a.hash).cmp(&(&b.name, b.hash)));
        self.unchanged()?;
        Ok(result)
    }

    pub(super) fn weapon_art(
        &self,
        item: &Weapon,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let (art_tag, art) = self.table(0x808034D7)?;
        let arts = art.array(8, 24, Some(0x80803433))?;
        let (map_tag, map) = self.table(0x80803AAA)?;
        let mut assignments = BTreeMap::new();
        for at in map.array(8, 8, Some(0x80803AA9))? {
            ensure!(
                assignments.insert(map.u32(at)?, map.u32(at + 4)?).is_none(),
                "Duplicate source art assignment"
            );
        }
        let mut rows = Vec::new();
        let mut parts = BTreeMap::new();
        for &index in &item.arrangements {
            let row = *arts
                .get(usize::from(index))
                .context("Weapon arrangement exceeds its table")?;
            let singles = [art.u32(row)?, art.u32(row + 4)?];
            let mut slots = Vec::new();
            let mut selected = Vec::from(singles);
            for at in art.array(row + 8, 8, Some(0x80805D63))? {
                let placement = art.pointer(at)?;
                let selector = art.u32(placement)?;
                let choices = art
                    .array(placement + 8, 4, Some(0x808033A3))?
                    .into_iter()
                    .map(|at| art.u32(at))
                    .collect::<Result<Vec<_>>>()?;
                selected.extend(&choices);
                slots.push(json!({"selector":selector,"assignments":choices}));
            }
            for key in selected {
                if matches!(key, 0 | u32::MAX | 0x811C9DC5) || parts.contains_key(&key) {
                    continue;
                }
                let parent = *assignments
                    .get(&key)
                    .context("Source weapon art has no assignment")?;
                let part = self.body("weapon", index, key, parent, assets)?;
                parts.insert(key, part);
            }
            rows.push(
                json!({"class":-1,"flags":0,"art_index":index,"singles":singles,"slots":slots}),
            );
        }
        ensure!(!parts.is_empty(), "Source weapon has no model art");
        assets.insert(art_tag, art);
        assets.insert(map_tag, map);
        Ok(json!({"rows":rows,"parts":parts.into_values().collect::<Vec<_>>()}))
    }

    /// Export executable talent references and every selectable source model.
    /// This provenance bundle does not claim native runtime compatibility.
    pub fn export_weapon(&self, hash: u32, output: &Path) -> Result<PathBuf> {
        let output = self.output(output)?;
        let item = self
            .weapons()?
            .into_iter()
            .find(|v| v.hash == hash)
            .context("Selected source weapon is missing")?;
        let mut assets = BTreeMap::new();
        for tag in [item.definition, item.strings] {
            assets.insert(tag, self.read(tag)?);
        }
        let art = self.weapon_art(&item, &mut assets)?;
        let talents = self.item_talents(item.definition, &mut assets)?;
        let stats = self.weapon_stats(&item, &mut assets)?;
        let runtime = self.weapon_runtime(&item, &mut assets)?;
        let dyes = self.dyes(item.definition, &mut assets)?;
        self.unchanged()?;
        fs::create_dir_all(&output)?;
        let assets = self.write_assets(&output, assets)?;
        let path = output.join("weapon.json");
        crate::io::write_json(
            &path,
            &json!({"source":"d1_roi","native_conversion":false,
            "item":item,"art":art,"talents":talents,"stats":stats,"runtime":runtime,"dyes":dyes,
            "builds":self.report.builds,"package_stamp":self.report.stamp,"assets":assets,
            "limits":["Source records preserve original references. Export does not establish native behavior or in-game acceptance."]}),
        )?;
        Ok(path)
    }
}
