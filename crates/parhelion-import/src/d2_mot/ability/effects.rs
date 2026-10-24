//! Inspect the actual state and transition nodes retained by the source perk tracer.
//! Recorded node translations are evidence about those nodes, not an ability emitter.

use super::source::{number, tag};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Effect,
    Condition,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lowering {
    Recorded,
    Unsupported,
    Unexamined,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub controller: String,
    pub perk_hash: u32,
    pub state_key: u32,
    pub transition_key: Option<u32>,
    pub domain: Domain,
    pub lowering: Lowering,
    /// Complete retained node, including offset, source dispatch, expressions and unknown fields.
    pub record: Value,
}

#[derive(Debug, Serialize)]
pub struct Auxiliary {
    pub controller: String,
    pub record: Value,
}

#[derive(Debug, Default, Serialize)]
pub struct Inspection {
    pub nodes: Vec<Node>,
    pub auxiliary: Vec<Auxiliary>,
}

fn node(
    controller: &str,
    perk_hash: u32,
    state_key: u32,
    transition_key: Option<u32>,
    record: &Value,
) -> Result<Node> {
    tag(record["class"]
        .as_str()
        .context("Effect or condition class")?)?;
    u8::try_from(number(&record["source_kind"], "source dispatch kind")?)?;
    record["offset"]
        .as_u64()
        .context("Effect or condition source offset")?;
    let lowering = if record["native_node"].is_object() {
        ensure!(
            record["lowering_error"].is_null(),
            "Node has both a translation and a lowering error"
        );
        ensure!(
            number(&record["native_node"]["kind"], "recorded native dispatch")?
                == number(&record["native_kind"], "native dispatch")?,
            "Recorded node dispatch differs"
        );
        tag(record["native_class"]
            .as_str()
            .context("Recorded native class")?)?;
        // Keep the recorded bytes as evidence. They are not linked into a native program here.
        let bytes = record["native_node"]["bytes"]
            .as_str()
            .context("Recorded native node bytes")?;
        let decoded = hex::decode(
            bytes
                .strip_prefix("0x")
                .context("Recorded native bytes prefix")?,
        )?;
        ensure!(!decoded.is_empty(), "Recorded native node is empty");
        Lowering::Recorded
    } else if record["lowering_error"].is_string() {
        Lowering::Unsupported
    } else {
        Lowering::Unexamined
    };
    Ok(Node {
        controller: controller.to_owned(),
        perk_hash,
        state_key,
        transition_key,
        domain: if transition_key.is_some() {
            Domain::Condition
        } else {
            Domain::Effect
        },
        lowering,
        record: record.clone(),
    })
}

pub(crate) fn inspect(trace: &Value) -> Result<Inspection> {
    let mut result = Inspection::default();
    for perk in trace["perks"].as_array().context("Source perks")? {
        crate::cancellation::check()?;
        if perk["decoded_controller"] != true {
            continue;
        }
        let controller = perk["tag"].as_str().context("Decoded controller tag")?;
        let perk_hash = number(&perk["perk_hash"], "perk hash")?;
        for state in perk["states"].as_array().context("Controller states")? {
            let state_key = number(&state["key"], "source state key")?;
            for effect in state["effects"]
                .as_array()
                .context("Source state effects")?
            {
                result
                    .nodes
                    .push(node(controller, perk_hash, state_key, None, effect)?);
            }
            for group in state["condition_groups"]
                .as_array()
                .context("Source condition groups")?
            {
                let transition_key = number(&group["key"], "source transition key")?;
                for condition in group["conditions"]
                    .as_array()
                    .context("Source transition conditions")?
                {
                    result.nodes.push(node(
                        controller,
                        perk_hash,
                        state_key,
                        Some(transition_key),
                        condition,
                    )?);
                }
            }
        }
        if let Some(records) = perk["auxiliary"].as_array() {
            for record in records {
                result.auxiliary.push(Auxiliary {
                    controller: controller.to_owned(),
                    record: record.clone(),
                });
            }
        }
    }
    Ok(result)
}

impl Inspection {
    /// Only traced attachment and spawn fields establish entity dependencies.
    pub(crate) fn attachments(&self) -> Result<Vec<&Value>> {
        let mut result = Vec::new();
        for node in &self.nodes {
            if !node.record["entity"].is_null() {
                ensure!(
                    matches!(
                        tag(node.record["class"].as_str().context("Attachment class")?)?,
                        0x80803130 | 0x8080B7C6 | 0x80803128
                    ),
                    "Unrecognized effect contains an entity dependency"
                );
                let expected = match tag(node.record["class"]
                    .as_str()
                    .context("Entity effect class")?)?
                {
                    0x80803130 => 2,
                    0x8080B7C6 => 1,
                    _ => 3,
                };
                ensure!(
                    number(&node.record["source_kind"], "entity effect kind")? == expected,
                    "Entity effect dispatch differs from its class"
                );
                let entity = &node.record["entity"];
                tag(entity["tag"].as_str().context("Attachment entity tag")?)?;
                entity["components"]
                    .as_array()
                    .context("Attachment entity components")?;
                result.push(entity);
            }
        }
        Ok(result)
    }
}
