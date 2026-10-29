//! Resolve armor and weapon appearance choices through their original talent grid.
use super::*;
use serde_json::{Value, json};

/// One appearance item a talent grid step offers, with where the grid lists it.
struct Appearance {
    hash: u32,
    /// The item's own name, empty when it has none.
    name: String,
    kind: String,
    rarity: u8,
    definition: u32,
    strings: u32,
    /// The art arrangements of the owner's class, or the class-neutral ones of a weapon.
    arrangements: Vec<u16>,
    provenance: Value,
}

impl Source {
    pub(super) fn ornaments(
        &self,
        armor: &Armor,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        for appearance in self.appearances(armor.definition, Some(armor.class_type), assets)? {
            let item = Armor {
                collection_set: None,
                hash: appearance.hash,
                name: if appearance.name.is_empty() {
                    format!("{} Ornament", armor.name)
                } else {
                    appearance.name
                },
                kind: appearance.kind,
                class_type: armor.class_type,
                slot: armor.slot,
                rarity: appearance.rarity,
                definition: appearance.definition,
                strings: appearance.strings,
                arrangements: appearance.arrangements,
            };
            let bodies = self.armor_bodies(&item, assets)?;
            let mut row = appearance.provenance;
            row["target_armor"] = json!(armor.hash);
            row["item"] = serde_json::to_value(&item)?;
            row["bodies"] = json!(bodies);
            result.push(row);
        }
        Ok(result)
    }

    /// The ornaments `weapon`'s talent grid offers, each as a weapon of its own art with where
    /// the grid lists it.
    pub(super) fn weapon_ornaments(
        &self,
        weapon: &Weapon,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Vec<(Weapon, Value)>> {
        let mut result = Vec::new();
        for appearance in self.appearances(weapon.definition, None, assets)? {
            let item = Weapon {
                hash: appearance.hash,
                name: if appearance.name.is_empty() {
                    format!("{} Ornament", weapon.name)
                } else {
                    appearance.name
                },
                kind: appearance.kind,
                flavor: String::new(),
                slot: weapon.slot,
                rarity: appearance.rarity,
                definition: appearance.definition,
                strings: appearance.strings,
                pattern: weapon.pattern,
                arrangements: appearance.arrangements,
            };
            let mut row = appearance.provenance;
            row["target_weapon"] = json!(weapon.hash);
            row["item"] = serde_json::to_value(&item)?;
            result.push((item, row));
        }
        Ok(result)
    }

    /// The appearance items the talent grid of the item definition `definition` offers. An
    /// arrangement belongs to every class (`255`) or to one, and `class` names the one accepted
    /// besides the class-neutral ones.
    fn appearances(
        &self,
        definition: u32,
        class: Option<u8>,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Vec<Appearance>> {
        let definition = self.read(definition)?;
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
                            let row_class = data.u8(row).ok()?;
                            (row_class == u8::MAX || Some(row_class) == class)
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
                    result.push(Appearance {
                        hash,
                        name: labels.text(self, &text, 0x78)?,
                        kind: labels.text(self, &text, 0x80)?.trim().to_owned(),
                        rarity: data.u8(0x8A)?,
                        definition: tag,
                        strings: text_tag,
                        arrangements,
                        provenance: json!({"grid":grid_tag,"node":node,"step":step,
                            "appearance_offset":appearance,"item_ordinal":index,
                            "definition_table":definitions_tag}),
                    });
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
