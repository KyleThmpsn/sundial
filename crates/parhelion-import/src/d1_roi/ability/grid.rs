//! Every source subclass grid step, lowered where a qualified route exists. A step that cannot be
//! lowered stays in the result with its reason, so no source choice disappears silently.
use super::*;
use crate::d1_roi::native::perk::subclass::Lowering;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use sundial::package_authoring::sandbox_perk::program::Program;

/// A grant that equips a source ability, by its source row and slot byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Equip {
    pub row: u8,
    pub slot: u8,
    /// The native graph assigned to the row's pattern, when one exists.
    pub native_graph: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Step {
    pub node: u32,
    pub step: u32,
    pub group: u32,
    pub row: i16,
    pub column: i16,
    /// The catalog's foundation role: automatic or required in a fully unlocked grid.
    pub foundation: Option<bool>,
    pub name: String,
    pub description: String,
    /// The step's source icon as a square PNG.
    #[serde(skip)]
    pub icon: Option<Vec<u8>>,
    /// The glyph the source HUD shows for its grants, when it shows one.
    pub hud_glyph: Option<u32>,
    /// That glyph's white art as a PNG, apart from the grid icon.
    #[serde(skip)]
    pub hud_icon: Option<Vec<u8>>,
    pub equips: Vec<Equip>,
    /// Property keys granted to the subclass's own Super row, which only its private bank holds.
    pub super_keys: Vec<u32>,
    /// Source key and ability row of each other property grant, which `programs` lower into
    /// private rows. A row of `FF` names every ability bank with the key.
    pub properties: Vec<(u32, u8)>,
    #[serde(skip)]
    pub programs: Vec<Program>,
    /// Source stat rows, which need their own native route.
    pub stats: Value,
    /// Why each of the step's perks or other grants that did not lower failed. Its other parts
    /// are in `programs`.
    pub unsupported: Vec<String>,
    pub report: Value,
}

impl Source {
    /// Lower every step of a subclass grid. Equip grants and grants into the subclass's own Super
    /// row are returned for the caller, which owns those destinations.
    pub fn subclass_grid(
        &self,
        native: &mut Reader,
        subclass: &SubclassCatalog,
        folder: &Path,
        xg: Option<&Path>,
    ) -> Result<Vec<Step>> {
        let lowering = Lowering::new(self, native)?;
        let order = subclass
            .nodes
            .iter()
            .flat_map(|node| node.steps.iter().map(move |step| (node, step)))
            .collect::<Vec<_>>();
        let mut built = order.iter().map(|_| None).collect::<Vec<Option<Step>>>();
        // Foundations lower first. A row gated on a foundation's own property key gates on the key
        // applied for it: the first private key of the foundation's lowered rows, which all apply
        // together, or the stock key itself while the foundation keeps its native node. The
        // caller places a gated step only where its tree applies that key.
        let mut gates = BTreeMap::new();
        for (index, &(node, step)) in order.iter().enumerate() {
            if step.foundation != Some(true) {
                continue;
            }
            let lowered = self.grid_step(
                native,
                &lowering,
                subclass,
                (node, step),
                (folder, xg),
                &gates,
            )?;
            for &(key, _) in &lowered.properties {
                let private = lowered
                    .unsupported
                    .is_empty()
                    .then(|| {
                        lowered.report["ability_grants"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|grant| grant["source_key"] == key)
                            .and_then(|grant| grant["property_key"].as_u64())
                            .and_then(|private| u32::try_from(private).ok())
                    })
                    .flatten();
                gates.insert(key, private.unwrap_or(key));
            }
            built[index] = Some(lowered);
        }
        let mut steps = Vec::new();
        for (index, &(node, step)) in order.iter().enumerate() {
            match built[index].take() {
                Some(lowered) => steps.push(lowered),
                None => steps.push(self.grid_step(
                    native,
                    &lowering,
                    subclass,
                    (node, step),
                    (folder, xg),
                    &gates,
                )?),
            }
        }
        self.unchanged()?;
        Ok(steps)
    }

    /// One step's source contributions, its presentation and its lowered parts.
    fn grid_step(
        &self,
        native: &mut Reader,
        lowering: &Lowering,
        subclass: &SubclassCatalog,
        (node, step): (&SubclassNode, &SubclassStep),
        (folder, xg): (&Path, Option<&Path>),
        gates: &BTreeMap<u32, u32>,
    ) -> Result<Step> {
        let variants = step.source["variants"]
            .as_array()
            .context("Source subclass variants")?;
        ensure!(
            variants.len() == 1,
            "Source subclass step {}:{} has several runtime variants",
            node.node,
            step.step
        );
        let variant = &variants[0];
        let mut equips = Vec::new();
        let mut super_keys = Vec::new();
        let mut properties = Vec::new();
        let mut grants = Vec::new();
        for grant in variant["ability_grants"]
            .as_array()
            .context("Source subclass grants")?
        {
            let bytes = hex::decode(grant.as_str().context("Source grant bytes")?)?;
            ensure!(bytes.len() == 16, "Source grant record size differs");
            let key = u32::from_le_bytes(bytes[..4].try_into()?);
            if bytes[11] != u8::MAX {
                ensure!(
                    key == 0x811C9DC5 && bytes[4] == u8::MAX,
                    "Source equip grant also names a property"
                );
                equips.push(Equip {
                    row: bytes[11],
                    slot: bytes[12],
                    native_graph: lowering.native_graph(bytes[11]),
                });
            } else if bytes[4] == subclass.ability_row {
                super_keys.push(key);
            } else {
                properties.push((key, bytes[4]));
                grants.push(grant.clone());
            }
        }
        let perks = variant["perks"]
            .as_array()
            .context("Source subclass perks")?;
        let container = step.source["icon"].as_u64().unwrap_or(u64::from(u32::MAX));
        let icon = self
            .icon_png(u32::try_from(container)?, xg)
            .with_context(|| format!("Source step {} icon", step.name.trim()))?;
        let hud_glyph = self
            .step_glyph(subclass, &equips, &properties)
            .with_context(|| format!("Source step {} HUD glyph", step.name.trim()))?;
        let hud_icon = hud_glyph
            .map(|key| self.hud_glyph_png(key))
            .transpose()?
            .flatten();
        let name = step.name.trim();
        let lowered = lowering.lower(
            self,
            native,
            folder,
            perks,
            &grants,
            name,
            gates,
            step.foundation != Some(true),
        );
        Ok(Step {
            node: node.node,
            step: step.step,
            group: node.group_key,
            row: node.row,
            column: node.column,
            foundation: step.foundation,
            name: name.to_owned(),
            description: step.description.trim().to_owned(),
            icon,
            hud_glyph,
            hud_icon,
            equips,
            super_keys,
            properties,
            programs: lowered.programs,
            stats: variant["stat_adjustments"].clone(),
            unsupported: lowered.failures,
            report: lowered.report,
        })
    }

    /// The glyph the source HUD shows for a step's grants. An equipped ability shows the glyph
    /// its controller definition names at +170 unless byte +174 hides it, which reset copies to
    /// instance +1BC and +1C5. A granted key's ungated rows of `80802650` and `8080206A` then
    /// replace the glyph and that byte, as they do on the controller.
    fn step_glyph(
        &self,
        subclass: &SubclassCatalog,
        equips: &[Equip],
        properties: &[(u32, u8)],
    ) -> Result<Option<u32>> {
        let ability = |row: u8| {
            subclass
                .abilities
                .iter()
                .find(|ability| ability.row == row)
                .with_context(|| format!("Source ability row {row} is not in the subclass"))
        };
        let controller = |row: u8| -> Result<(u32, bool)> {
            let controller = self.read(ability(row)?.controller)?;
            let (instance, definition) = (controller.pointer(16)?, controller.pointer(24)?);
            let key = controller.u32(definition + 0x170)?;
            let hidden = controller.u8(definition + 0x174)? != 0;
            ensure!(
                controller.u32(instance + 0x1BC)? == key
                    && (controller.u8(instance + 0x1C5)? != 0) == hidden,
                "Source controller glyph instance differs from its definition"
            );
            Ok((key, hidden))
        };
        let mut shown = None;
        for equip in equips {
            shown = Some(controller(equip.row)?);
        }
        for &(key, row) in properties {
            // Row FF names every bank with the key, so it selects no single tile.
            if row == u8::MAX {
                continue;
            }
            let bank = self.read(ability(row)?.bank)?;
            let mut state = None;
            for at in bank.array(bank.pointer(24)? + 0xA0, 40, Some(0x80802A84))? {
                // A gated row shows its glyph only beside another key.
                if bank.u32(at)? != key || bank.u64(at + 24)? != 0 {
                    continue;
                }
                let body = bank.pointer(at + 32)?;
                let class = bank.u32(body - 4)?;
                if !matches!(class, 0x80802650 | 0x8080206A) {
                    continue;
                }
                let (glyph, hidden) = match state {
                    Some(state) => state,
                    None => controller(row)?,
                };
                state = Some(if class == 0x80802650 {
                    (bank.u32(body)?, hidden)
                } else {
                    (glyph, bank.u8(body)? != 0)
                });
            }
            if state.is_some() {
                shown = state;
            }
        }
        Ok(shown.and_then(|(key, hidden)| (!hidden).then_some(key)))
    }
}
