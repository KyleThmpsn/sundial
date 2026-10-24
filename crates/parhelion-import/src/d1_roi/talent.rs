//! Join source grid choices to their client artwork, finished runtime actions and stat
//! adjustments.
use super::*;
use serde_json::{Value, json};
mod choice;

impl Source {
    pub(super) fn runtime_assignments(
        &self,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<BTreeMap<u32, u32>> {
        let root_tag = self.named("investment_assets")?;
        let root = self.read(root_tag)?;
        let tag = root.u32(0x10)?;
        ensure!(
            self.reference(tag)? == 0x80800341,
            "Source runtime assignment root differs"
        );
        let map = self.read(tag)?;
        let mut result = BTreeMap::new();
        for row in map.array(8, 8, None)? {
            ensure!(
                result.insert(map.u32(row)?, map.u32(row + 4)?).is_none(),
                "Duplicate source runtime assignment"
            );
        }
        assets.insert(root_tag, root);
        assets.insert(tag, map);
        Ok(result)
    }

    pub(super) fn item_talents(
        &self,
        definition_tag: u32,
        assets: &mut BTreeMap<u32, Payload>,
    ) -> Result<Value> {
        let definition = self.read(definition_tag)?;
        let holder = definition.pointer(0x58)?;
        ensure!(
            definition.u32(holder - 4)? == 0x808017BD,
            "Source talent holder type differs"
        );
        let ordinal = definition.u16(holder)?;
        let (grids_tag, grids) = self.table(0x808018C2)?;
        let (clients_tag, clients) = self.table(0x80803461)?;
        let grids_rows = grids.array(8, 24, None)?;
        let clients_rows = clients.array(8, 16, None)?;
        ensure!(
            grids_rows.len() == clients_rows.len(),
            "Source shared and client grids disagree"
        );
        let shared_tag = grids.u32(
            *grids_rows
                .get(usize::from(ordinal))
                .context("Source shared talent grid")?
                + 16,
        )?;
        let client_tag = clients.u32(
            *clients_rows
                .get(usize::from(ordinal))
                .context("Source client talent grid")?
                + 8,
        )?;
        ensure!(
            self.reference(client_tag)? == 0x80803397,
            "Source client talent type differs"
        );
        let shared = self.read(shared_tag)?;
        let client = self.read(client_tag)?;
        let shared_nodes = shared.array(0x10, 64, Some(0x80801728))?;
        let client_nodes = client.array(8, 72, Some(0x8080348B))?;
        ensure!(
            shared_nodes.len() == client_nodes.len(),
            "Source talent node ordinals disagree"
        );
        let (meta_tag, meta) = self.table(0x808017FE)?;
        let (finished_tag, finished) = self.table(0x80803483)?;
        // Adjustment rows name a stat by its ordinal in the shared stat table, hash at `+0`.
        let (stats_tag, stats) = self.table(0x808017EC)?;
        let stat_rows = stats.array(8, 16, Some(0x80801870))?;
        let metadata = meta.array(8, 8, None)?;
        let finishes = finished.array(8, 16, Some(0x808033E0))?;
        ensure!(
            metadata.len() == finishes.len(),
            "Source finished perk ordinals disagree"
        );
        let assignments = self.runtime_assignments(assets)?;
        let mut labels = super::localization::Labels::new(self)?;
        let mut nodes = Vec::new();
        for (index, (node, display)) in shared_nodes.into_iter().zip(client_nodes).enumerate() {
            let mut steps = Vec::new();
            for (step, choice) in choice::read(&shared, &client, node, display)?
                .into_iter()
                .enumerate()
            {
                let (at, text) = (choice.offset, choice.text);
                let mut variants = Vec::new();
                for variant in choice.variants {
                    let mut perks = Vec::new();
                    for row in shared.array(variant + 32, 2, Some(0x80804628))? {
                        let index = shared.u16(row)?;
                        let finish = *finishes
                            .get(usize::from(index))
                            .context("Talent finished perk exceeds table")?;
                        let identity = *metadata
                            .get(usize::from(index))
                            .context("Talent perk identity exceeds table")?;
                        let key = finished.u32(finish)?;
                        let assigned = assignments.get(&key).copied();
                        let action = assigned.filter(|tag| !matches!(*tag, 0 | u32::MAX));
                        let class = if let Some(tag) = action {
                            let class = self.reference(tag)?;
                            assets.insert(tag, self.read(tag)?);
                            Some(class)
                        } else {
                            None
                        };
                        perks.push(json!({"ordinal":index,"hash":meta.u32(identity)?,"runtime_key":key,
                            "action":action,"action_class":class,"assignment_present":assigned.is_some()}));
                    }
                    // Row: value `+0`, two scale factors `+4` and `+8`, stat ordinal `+12`.
                    let mut adjustments = Vec::new();
                    for row in shared.array(variant + 16, 16, Some(0x80801787))? {
                        let ordinal = shared.u8(row + 12)?;
                        let stat = *stat_rows
                            .get(usize::from(ordinal))
                            .context("Talent stat adjustment exceeds the stat table")?;
                        adjustments.push(json!({"index":ordinal,"hash":stats.u32(stat)?,
                            "value":i32::from_le_bytes(shared.bytes::<4>(row)?),
                            "scale":[shared.f32(row + 4)?, shared.f32(row + 8)?],
                            "record":hex::encode(shared.bytes::<16>(row)?)}));
                    }
                    let ability_grants = shared
                        .array(variant, 16, Some(0x808018E8))?
                        .into_iter()
                        .map(|at| shared.bytes::<16>(at).map(hex::encode))
                        .collect::<Result<Vec<_>>>()?;
                    variants.push(
                        json!({"offset":variant,"record":hex::encode(shared.bytes::<88>(variant)?),
                        "stat_adjustments":adjustments,"ability_grants":ability_grants,"perks":perks}),
                    );
                }
                steps.push(
                    json!({"index":step,"offset":at,"shared_step":choice.shared_index,
                    "source_variant":choice.variant_index,
                    "appearance_choices":shared.u64(at + 0x70)?,
                    "name":labels.optional_text(self,&client,text)?.unwrap_or_default(),
                    "description":labels.optional_text(self,&client,text+8)?.unwrap_or_default(),
                    "icon":client.u32(text+32)?,"variants":variants}),
                );
            }
            nodes.push(json!({"index":index,"offset":node,"hash":shared.u32(node)?,
                "row":client.i16(display+64)?,"column":client.i16(display+66)?,"steps":steps}));
        }
        for (tag, payload) in [
            (definition_tag, definition),
            (grids_tag, grids),
            (clients_tag, clients),
            (shared_tag, shared),
            (client_tag, client),
            (meta_tag, meta),
            (finished_tag, finished),
            (stats_tag, stats),
        ] {
            assets.insert(tag, payload);
        }
        Ok(
            json!({"ordinal":ordinal,"shared_grid":shared_tag,"client_grid":client_tag,
            "stat_table":stats_tag,"nodes":nodes}),
        )
    }
}
