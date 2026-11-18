//! One entry for every modern condition and effect class with a Shadowkeep translation: the
//! checked record table first, then the lowerings written for one class each.
use anyhow::{Context, Result, anyhow, ensure};
use sundial::investment::discovery::labels::Registry;

use super::Node;
use crate::d2_mot::gameplay::perks::table;
use crate::d2_mot::payload::Payload;

/// Refuse a node that names a label Shadowkeep's registry lacks. The package build compiles
/// every label row against that registry and stops on a name it does not hold. A label filter
/// of the table looks its names up as it writes them, but label rows also travel as plain rows
/// and through the lowerings written for one class, which copy names as they are. So every
/// node is read back: each array of label rows `808094B3`, 24 bytes with the name first,
/// behind the array marker and its count.
fn registered(node: Node, labels: Option<&Registry>) -> Result<Node> {
    let Some(registry) = labels else {
        return Ok(node);
    };
    let bytes = &node.bytes;
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    };
    let mut header = 4;
    while header + 16 <= bytes.len() {
        if word(header - 4) == Some(0x8080_9FBD)
            && word(header + 4) == Some(0)
            && word(header + 8) == Some(0x8080_94B3)
            && word(header + 12) == Some(0)
        {
            let count = word(header).context("label row count")? as usize;
            for row in 0..count {
                let name = word(header + 16 + row * 24).context("label rows leave their node")?;
                ensure!(
                    registry.get(name).is_some(),
                    "Shadowkeep has no label {name:08X}"
                );
            }
        }
        header += 4;
    }
    Ok(node)
}

/// The table's result when it holds the class, with the class lowering as the alternative.
/// A class the table refuses keeps the table's reason when nothing else translates it.
fn dispatch(
    source: &Payload,
    at: usize,
    labels: Option<&Registry>,
    lowering: impl FnOnce(u32) -> Option<Result<Node>>,
    role: &str,
) -> Result<Node> {
    ensure!(at >= 4, "source {role} has no class word");
    let class = source.u32(at - 4)?;
    let table = table::holds(class).then(|| table::node(source, at, labels));
    if let Some(Ok(node)) = table {
        return registered(node, labels);
    }
    match (lowering(class), table) {
        (Some(Ok(node)), _) => registered(node, labels),
        (_, Some(Err(error))) | (Some(Err(error)), _) => Err(error),
        (None, _) => Err(anyhow!(
            "source {role} class {class:08X} has no Shadowkeep translation"
        )),
    }
}

/// Translate any modern condition. `labels` is Shadowkeep's label registry when at hand.
pub fn condition_with(source: &Payload, at: usize, labels: Option<&Registry>) -> Result<Node> {
    dispatch(
        source,
        at,
        labels,
        |class| match class {
            0x8080_B7B2 => Some(super::kill_condition(source, at)),
            0x8080_3061 | 0x8080_305F => Some(super::predicate(source, at, labels)),
            0x8080_BDCF => Some(super::object_slot_condition(source, at)),
            _ => None,
        },
        "condition",
    )
}

/// Translate any modern effect that names no package resource. Attachments, spawns and other
/// effects that name an entity need their graph converted first and are not handled here.
pub fn effect_with(source: &Payload, at: usize, labels: Option<&Registry>) -> Result<Node> {
    dispatch(
        source,
        at,
        labels,
        |class| match class {
            0x8080_B4E1 => Some(super::event_modifier(source, at)),
            0x8080_22E5 => Some(super::component_value(source, at)),
            _ => None,
        },
        "effect",
    )
}
