//! Resolve armor appearance choices through their original talent grid.
use super::*;
use serde_json::{Value, json};

impl Source {
    pub(super) fn ornaments(
        &self,
        armor: &Armor,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Vec<Value>> {
        let definition = self.read(armor.definition)?;
        if definition.u64(0x58)? == 0 {
            return Ok(Vec::new());
        }
        let talent = definition.pointer(0x58)?;
        ensure!(
            talent >= 4 && definition.u32(talent - 4)? == 0x808017BD,
            "Unknown item talent record"
        );
        let ordinal = usize::from(definition.u16(talent)?);
        if ordinal == usize::from(u16::MAX) {
            return Ok(Vec::new());
        }
        let (table_tag, table) = self.table(0x808018C2)?;
        let rows = table.array(8, 24, Some(0x808017CB))?;
        let grid_tag = table.u32(*rows.get(ordinal).context("Talent grid ordinal")? + 16)?;
        let grid = self.read(grid_tag)?;
        let (definitions_tag, definitions) = self.table(0x80801933)?;
        let definitions_rows = definitions.array(8, 24, Some(0x808016BD))?;
        let (strings_tag, strings) = self.table(0x808034C7)?;
        let string_rows = strings.array(8, 16, Some(0x808033E8))?;
        let mut labels = super::localization::Labels::new(self)?;
        let mut result = Vec::new();
        for (node, row) in grid.array(0x10, 64, None)?.into_iter().enumerate() {
            for (step, row) in grid.array(row + 24, 144, None)?.into_iter().enumerate() {
                for appearance in grid.array(row + 0x70, 4, Some(0x80800F94))? {
                    let index = usize::from(grid.u16(appearance + 2)?);
                    if index == usize::from(u16::MAX) {
                        continue;
                    }
                    let row = *definitions_rows
                        .get(index)
                        .context("Ornament item ordinal")?;
                    let hash = definitions.u32(row)?;
                    let tag = definitions.u32(row + 16)?;
                    let data = self.read(tag)?;
                    ensure!(
                        self.reference(tag)? == 0x80801806 && data.u32(0x78)? == hash,
                        "Ornament identity mismatch"
                    );
                    let gear = data.pointer(0x60)?;
                    ensure!(
                        gear >= 4 && data.u32(gear - 4)? == 0x80801020,
                        "Ornament has no appearance record"
                    );
                    let arrangements = data
                        .array(gear, 4, Some(0x8080101A))?
                        .into_iter()
                        .filter_map(|row| {
                            let class = data.u8(row).ok()? as i8;
                            (class == -1 || class == armor.class_type as i8)
                                .then(|| data.u16(row + 2))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    ensure!(
                        !arrangements.is_empty() && !arrangements.contains(&u16::MAX),
                        "Ornament has no compatible art arrangement"
                    );
                    let text_tag = strings
                        .u32(*string_rows.get(index).context("Ornament strings ordinal")? + 8)?;
                    let text = self.read(text_tag)?;
                    let source_name = labels.text(self, &text, 0x78)?;
                    let item = Armor {
                        collection_set: None,
                        hash,
                        name: if source_name.is_empty() {
                            format!("{} Ornament", armor.name)
                        } else {
                            source_name
                        },
                        kind: labels.text(self, &text, 0x80)?.trim().to_owned(),
                        class_type: armor.class_type,
                        slot: armor.slot,
                        rarity: data.u8(0x8A)?,
                        definition: tag,
                        strings: text_tag,
                        arrangements,
                    };
                    let bodies = self.armor_bodies(&item, assets)?;
                    result.push(json!({"target_armor":armor.hash,"grid":grid_tag,"node":node,"step":step,
                        "appearance_offset":appearance,"item_ordinal":index,"definition_table":definitions_tag,"item":item,"bodies":bodies}));
                    assets.insert(tag, data);
                    assets.insert(text_tag, text);
                }
            }
        }
        for (tag, data) in [
            (table_tag, table),
            (grid_tag, grid),
            (definitions_tag, definitions),
            (strings_tag, strings),
        ] {
            assets.insert(tag, data);
        }
        Ok(result)
    }
}
