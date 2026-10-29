//! Source ability choices, complete subclass intake and bounded native behavior translation.
use super::*;
use crate::tiger::reader::Reader;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use sundial::package_authoring::{
    ability_definition, ability_modifier, sandbox_perk::program::Program,
};

mod catalog;
pub(crate) mod movement;
pub use catalog::{AbilityCatalog, SubclassCatalog, SubclassNode, SubclassStep};
pub mod controller;
pub mod revival;
pub use super::lifecycle;
pub mod dependency;
pub use crate::d1_roi::native::animation::ability as animation;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Choice {
    pub subclass: u32,
    pub subclass_name: String,
    pub grid: u16,
    pub node: u32,
    pub step: u32,
    pub name: String,
    pub description: String,
}

pub struct Translation {
    pub choice: Choice,
    pub native_pattern: u32,
    pub native_row: u8,
    pub native_graph: u32,
    pub native_bank: u32,
    pub selected_keys: Vec<u32>,
    pub program: Program,
    pub evidence: Value,
    pub limits: Vec<String>,
}

#[derive(Clone, Copy)]
struct Bank {
    row: u8,
    pattern: u32,
    graph: u32,
    bank: u32,
}

struct Selection {
    choice: Choice,
    bank: Bank,
    keys: Vec<u32>,
    foundation: Value,
    variant: Value,
}

impl Source {
    /// List source choices whose whole runtime contribution has a bounded movement route.
    pub fn ability_choices(&self) -> Result<Vec<Choice>> {
        let (selected, _) = selections(self)?;
        self.unchanged()?;
        Ok(selected
            .into_iter()
            .map(|selection| selection.choice)
            .collect())
    }

    /// Translate the selected D1 property values onto an identity-matched native controller.
    pub fn prepare_ability(&self, native: &mut Reader, choice: &Choice) -> Result<Translation> {
        ensure!(
            native.is_native(),
            "Ability translation requires Shadowkeep packages"
        );
        let (selected, assets) = selections(self)?;
        let selected = selected
            .into_iter()
            .find(|selected| selected.choice == *choice)
            .context("Source ability choice changed or has an unsupported companion")?;
        let (native_row, native_graph, native_bank, target) =
            counterpart(native, selected.bank.pattern)?;
        let (program, properties) = movement::lower(
            self,
            native,
            selected.bank.bank,
            native_bank,
            &selected.keys,
            &choice.name,
        )?;
        ensure!(
            !program.actions.is_empty(),
            "Source ability translated no behavior"
        );
        let inputs = assets.iter().map(|(&tag, payload)| {
            json!({"tag":tag,"sha256":hex::encode(Sha256::digest(&payload.0)),"bytes":payload.0.len()})
        }).collect::<Vec<_>>();
        let limits = vec![
            "Source movement property values are transferred. The matching Shadowkeep controller, input handling, animation and base movement settings are retained.".into(),
            "Source unlock progression is not imported. The source choice and its clean movement foundation contribute no omitted stat, perk or appearance records.".into(),
            "Private property keys activate only with the selected authored perk.".into(),
        ];
        let evidence = json!({
            "source":"d1_roi", "source_builds":self.report.builds,"source_stamp":self.report.stamp,
            "choice":choice,"source_ability_row":selected.bank.row,"source_pattern":selected.bank.pattern,
            "source_graph":selected.bank.graph,"source_bank":selected.bank.bank,
            "source_variant":selected.variant,"source_foundation":selected.foundation,
            "selected_keys":selected.keys,"native":target,"properties":properties,"source_inputs":inputs,
            "limits":limits,"gameplay_verified":false,
        });
        self.unchanged()?;
        Ok(Translation {
            choice: selected.choice,
            native_pattern: selected.bank.pattern,
            native_row,
            native_graph,
            native_bank,
            selected_keys: selected.keys,
            program,
            evidence,
            limits,
        })
    }
}

fn source_banks(
    source: &Source,
    assets: &mut BTreeMap<u32, Payload>,
) -> Result<BTreeMap<u8, Bank>> {
    let (tag, definitions) = source.table(0x80803432)?;
    let rows = definitions.array(8, 40, Some(0x8080343C))?;
    ensure!(
        !rows.is_empty() && rows.len() < 255,
        "Source ability table is empty or too large"
    );
    let assignments = source.runtime_assignments(assets)?;
    let mut result = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for (index, row) in rows.into_iter().enumerate() {
        let pattern = definitions.u32(row)?;
        ensure!(
            identities.insert(pattern),
            "Duplicate source ability identity"
        );
        let graph_tag = *assignments
            .get(&pattern)
            .context("Source ability has no runtime assignment")?;
        ensure!(
            source.reference(graph_tag)? == 0x80800734,
            "Source ability graph type differs"
        );
        let graph = source.read(graph_tag)?;
        let mut bank = None;
        for owner in graph.array(32, 12, Some(0x80800715))? {
            let tag = graph.u32(owner)?;
            ensure!(
                source.reference(tag)? == 0x80800861,
                "Source ability owner type differs"
            );
            let data = source.read(tag)?;
            let (runtime, settings) = (data.pointer(16)?, data.pointer(24)?);
            let pair = (
                data.u32(
                    runtime
                        .checked_sub(4)
                        .context("Source ability runtime marker")?,
                )?,
                data.u32(
                    settings
                        .checked_sub(4)
                        .context("Source ability settings marker")?,
                )?,
            );
            if pair != (0x8080275C, 0x80802593) {
                continue;
            }
            ensure!(
                bank.replace(tag).is_none(),
                "Source ability has ambiguous movement banks"
            );
            ensure!(
                data.u32(runtime)? == tag
                    && data.u64(runtime + 8)? == settings as u64
                    && data.u32(runtime + 16)? == 0x80802593
                    && data.u32(settings)? == tag
                    && data.u64(settings + 8)? == runtime as u64
                    && data.u32(settings + 16)? == 0x8080275C,
                "Source movement bank is not reciprocal"
            );
            assets.insert(tag, data);
        }
        if let Some(bank) = bank {
            result.insert(
                u8::try_from(index)?,
                Bank {
                    row: u8::try_from(index)?,
                    pattern,
                    graph: graph_tag,
                    bank,
                },
            );
            assets.insert(graph_tag, graph);
        }
    }
    assets.insert(tag, definitions);
    Ok(result)
}

fn selections(source: &Source) -> Result<(Vec<Selection>, BTreeMap<u32, Payload>)> {
    ensure!(
        source
            .report
            .selected
            .iter()
            .all(|package| package.platform == 8),
        "Ability discovery currently supports Xbox One Rise of Iron packages"
    );
    let mut assets = BTreeMap::new();
    let banks = source_banks(source, &mut assets)?;
    let (definitions_tag, definitions) = source.table(0x80801933)?;
    let (strings_tag, strings) = source.table(0x808034C7)?;
    let definitions_rows = definitions.array(8, 24, Some(0x808016BD))?;
    let string_rows = strings.array(8, 16, Some(0x808033E8))?;
    ensure!(
        definitions_rows.len() == string_rows.len(),
        "Source item and string ordinals disagree"
    );
    let mut labels = super::localization::Labels::new(source)?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (row, text_row) in definitions_rows.into_iter().zip(string_rows) {
        let tag = definitions.u32(row + 16)?;
        let data = source.read(tag)?;
        let hash = definitions.u32(row)?;
        ensure!(
            source.reference(tag)? == 0x80801806 && data.u32(0x78)? == hash,
            "Source subclass item identity differs"
        );
        if data.u64(0x18)? == 0 {
            continue;
        }
        let client = data.pointer(0x18)?;
        ensure!(
            data.u32(client - 4)? == 0x80801706,
            "Source subclass client type differs"
        );
        if data.u8(client + 0x18)? != 0 {
            continue;
        }
        ensure!(seen.insert(hash), "Duplicate source subclass identity");
        let text_tag = strings.u32(text_row + 8)?;
        ensure!(
            source.reference(text_tag)? == 0x80803484,
            "Source subclass strings type differs"
        );
        let text = source.read(text_tag)?;
        let subclass_name = labels
            .optional_text(source, &text, 0x78)?
            .unwrap_or_default();
        let talents = source.item_talents(tag, &mut assets)?;
        let grid = u16::try_from(
            talents["ordinal"]
                .as_u64()
                .context("Source subclass grid")?,
        )?;
        let nodes = talents["nodes"]
            .as_array()
            .context("Source subclass nodes")?;
        for node in nodes {
            for step in node["steps"].as_array().context("Source subclass steps")? {
                let Some((target, keys, variant)) = property_choice(step)? else {
                    continue;
                };
                let Some(&bank) = banks.get(&target) else {
                    continue;
                };
                let Some(foundation) = foundation(nodes, target)? else {
                    continue;
                };
                if !movement::accepts(source, bank.bank, &keys)? {
                    continue;
                }
                result.push(Selection {
                    choice: Choice {
                        subclass: hash,
                        subclass_name: subclass_name.clone(),
                        grid,
                        node: u32::try_from(node["index"].as_u64().context("Source node index")?)?,
                        step: u32::try_from(step["index"].as_u64().context("Source step index")?)?,
                        name: step["name"]
                            .as_str()
                            .context("Source ability name")?
                            .to_owned(),
                        description: step["description"]
                            .as_str()
                            .context("Source ability description")?
                            .to_owned(),
                    },
                    bank,
                    keys,
                    foundation,
                    variant,
                });
            }
        }
        assets.insert(text_tag, text);
    }
    assets.insert(definitions_tag, definitions);
    assets.insert(strings_tag, strings);
    result.sort_by(|a, b| {
        (
            &a.choice.subclass_name,
            &a.choice.name,
            a.choice.subclass,
            a.choice.node,
            a.choice.step,
        )
            .cmp(&(
                &b.choice.subclass_name,
                &b.choice.name,
                b.choice.subclass,
                b.choice.node,
                b.choice.step,
            ))
    });
    Ok((result, assets))
}

fn clean_variant(step: &Value) -> Result<Option<&Value>> {
    let variants = step["variants"]
        .as_array()
        .context("Source ability variants")?;
    let [variant] = variants.as_slice() else {
        return Ok(None);
    };
    let record = Payload(hex::decode(
        variant["record"]
            .as_str()
            .context("Source ability variant bytes")?,
    )?);
    ensure!(record.0.len() == 88, "Source ability variant size differs");
    if step["appearance_choices"].as_u64() != Some(0)
        || record.bytes::<40>(48)? != [0; 40]
        || !variant["stat_adjustments"]
            .as_array()
            .context("Source ability stats")?
            .is_empty()
        || !variant["perks"]
            .as_array()
            .context("Source ability perks")?
            .is_empty()
    {
        return Ok(None);
    }
    Ok(Some(variant))
}

fn grants(variant: &Value) -> Result<Vec<Payload>> {
    variant["ability_grants"]
        .as_array()
        .context("Source ability grants")?
        .iter()
        .map(|value| {
            let record = Payload(hex::decode(
                value.as_str().context("Source ability grant bytes")?,
            )?);
            ensure!(record.0.len() == 16, "Source ability grant size differs");
            Ok(record)
        })
        .collect()
}

fn property_choice(step: &Value) -> Result<Option<(u8, Vec<u32>, Value)>> {
    let Some(variant) = clean_variant(step)? else {
        return Ok(None);
    };
    let records = grants(variant)?;
    if records.is_empty() {
        return Ok(None);
    }
    let mut target = None;
    let mut seen = BTreeSet::new();
    let mut keys = Vec::new();
    for record in records {
        let row = record.u8(4)?;
        let key = record.u32(0)?;
        if row == 255
            || matches!(key, 0 | u32::MAX | 0x811C9DC5)
            || record.bytes::<3>(5)? != [0; 3]
            || record.bytes::<8>(8)? != [255, 255, 255, 255, 0, 0, 0, 0]
            || target.is_some_and(|previous| previous != row)
            || !seen.insert(key)
        {
            return Ok(None);
        }
        target = Some(row);
        keys.push(key);
    }
    Ok(Some((
        target.context("Source ability target")?,
        keys,
        variant.clone(),
    )))
}

fn foundation(nodes: &[Value], target: u8) -> Result<Option<Value>> {
    let mut result = None;
    for node in nodes {
        for step in node["steps"]
            .as_array()
            .context("Source foundation steps")?
        {
            for variant in step["variants"]
                .as_array()
                .context("Source foundation variants")?
            {
                let records = grants(variant)?;
                if !records
                    .iter()
                    .any(|record| record.u8(11).ok() == Some(target))
                {
                    continue;
                }
                let Some(clean) = clean_variant(step)? else {
                    return Ok(None);
                };
                if clean != variant || records.len() != 1 {
                    return Ok(None);
                }
                let record = &records[0];
                if record.u32(0)? != 0x811C9DC5
                    || record.u8(4)? != 255
                    || record.bytes::<3>(5)? != [0; 3]
                    || record.bytes::<3>(8)? != [255; 3]
                    || record.u8(12)? != 3
                    || record.bytes::<3>(13)? != [0; 3]
                    || result.is_some()
                {
                    return Ok(None);
                }
                result = Some(
                    json!({"node":node["index"],"step":step["index"],"name":step["name"],"variant":variant}),
                );
            }
        }
    }
    Ok(result)
}

fn counterpart(native: &mut Reader, pattern: u32) -> Result<(u8, u32, u32, Value)> {
    let tags = native.classes(ability_definition::DEFINITION_TABLE_CLASS);
    ensure!(
        tags.len() == 1,
        "Native ability definitions are missing or ambiguous"
    );
    let definitions = native.tag(tags[0], Some(ability_definition::DEFINITION_TABLE_CLASS))?;
    let identity_tags = native.classes(ability_definition::IDENTITY_TABLE_CLASS);
    ensure!(
        identity_tags.len() == 1,
        "Native ability identities are missing or ambiguous"
    );
    let identities = native.tag(
        identity_tags[0],
        Some(ability_definition::IDENTITY_TABLE_CLASS),
    )?;
    let rows = ability_definition::definitions(&definitions.0, &identities.0)
        .map_err(anyhow::Error::msg)?;
    let mut index = None;
    for (ordinal, row) in rows.into_iter().enumerate() {
        if row.pattern == pattern {
            ensure!(
                index.replace(u8::try_from(ordinal)?).is_none(),
                "Ambiguous native ability identity"
            );
        }
    }
    let index = index.context("Source ability has no identical native controller identity")?;
    let roots = native
        .manager
        .lookup
        .named_tags
        .iter()
        .filter(|tag| tag.name == "investment_assets")
        .map(|tag| tag.hash.0)
        .collect::<Vec<_>>();
    ensure!(
        roots.len() == 1,
        "Native investment assets are missing or ambiguous"
    );
    let root = native.tag(roots[0], None)?;
    let assignment_tag = root.u32(16)?;
    let assignments = native.tag(assignment_tag, Some(0x80809780))?;
    let mut graph = None;
    for row in assignments.array(8, 8, None)? {
        if assignments.u32(row)? == pattern {
            ensure!(
                graph.replace(assignments.u32(row + 4)?).is_none(),
                "Ambiguous native ability assignment"
            );
        }
    }
    let graph_tag = graph.context("Native ability has no runtime assignment")?;
    let graph = native.tag(graph_tag, Some(0x80809C0F))?;
    let bank = ability_modifier::entity_bank(&graph.0)
        .map_err(anyhow::Error::msg)?
        .context("Native ability has no property bank")?;
    Ok((
        index,
        graph_tag,
        bank,
        json!({"ability_table":tags[0],"ability_identities":identity_tags[0],"runtime_assignments":assignment_tag,
        "row":index,"pattern":pattern,"graph":graph_tag,"bank":bank}),
    ))
}
