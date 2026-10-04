//! Compares stock and compiled actions to report what a round trip would not reproduce.
use super::*;

/// One native byte range the round trip would not reproduce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    /// Node kind and role, for the reader.
    pub node: String,
    /// Offset inside the node.
    pub offset: usize,
    /// Stock bytes at that offset.
    pub stock: Vec<u8>,
    /// Bytes the compiler emits instead.
    pub compiled: Vec<u8>,
}

/// Compare complete allocation graphs, excluding only compiler-owned routing metadata.
/// Policies, auxiliary records, all groups and every unknown scalar remain in the comparison.
pub fn native_fidelity(stock: &[u8], compiled: &[u8]) -> Result<Vec<Difference>, String> {
    use crate::sandbox_perk::action::{ACTION_ROOT_CLASS, native::Graph};
    let canonical = |payload: &[u8]| -> Result<Vec<u8>, String> {
        let mut graph = Graph::read(payload, 0, ACTION_ROOT_CLASS)?;
        graph.validate_program()?;
        let root = &mut graph.blocks[0].bytes;
        root[..8].fill(0);
        root[0x88..0xA8].fill(0);
        root[0xCC..0xCE].fill(0);
        for block in &mut graph.blocks {
            if crate::sandbox_perk::nodes::CONDITIONS
                .iter()
                .any(|node| node.observed() && node.class == block.class)
            {
                block.bytes[6..8].fill(0);
            }
        }
        graph.emit()
    };
    let mut out = Vec::new();
    compare_bytes(
        "Complete Program".into(),
        &canonical(stock)?,
        &canonical(compiled)?,
        &[],
        &mut out,
    );
    Ok(out)
}

/// Compares a stock action with the action compiled from its recovered program.
///
/// Pointer fields, list descriptors, compiled ordinals and linked-state flags are derived data
/// and are masked. Every other byte of every node is compared, so a difference here names a
/// native setting the program model does not carry.
pub fn fidelity(stock: &[u8], compiled: &[u8]) -> Result<Vec<Difference>, String> {
    let stock_action = decode(stock)?;
    let compiled_action = decode(compiled)?;
    let mut out = Vec::new();
    if stock == compiled {
        return Ok(out);
    }
    compare_auxiliary(&stock_action, &compiled_action, &mut out);
    compare_policy(&stock_action, &compiled_action, &mut out);
    if stock_action
        .conditions()
        .iter()
        .chain(compiled_action.conditions().iter())
        .any(|node| {
            node.catalog().is_none_or(|entry| {
                entry.support != crate::sandbox_perk::nodes::Support::Authorable
            })
        })
        || stock_action
            .effects()
            .chain(compiled_action.effects())
            .any(|node| {
                node.catalog().is_none_or(|entry| {
                    entry.support != crate::sandbox_perk::nodes::Support::Authorable
                })
            })
    {
        return Err(
            "Exact conversion cannot be checked for a node outside the authored program model."
                .into(),
        );
    }
    compare_bytes(
        "Action Routing and State".into(),
        &stock[0x88..0xD0],
        &compiled[0x88..0xD0],
        &[(0x20, 16)],
        &mut out,
    );
    compare_bytes(
        "Action Identity".into(),
        &stock[8..0x10],
        &compiled[8..0x10],
        &[],
        &mut out,
    );
    if stock_action.groups.is_empty() || compiled_action.groups.is_empty() {
        return Err("An action without a program cannot be compared".into());
    }
    for (left, right) in stock_action.groups.iter().zip(&compiled_action.groups) {
        compare_group(stock, compiled, left, right, &mut out);
    }
    if stock_action.groups.len() != compiled_action.groups.len() {
        out.push(Difference {
            node: "Program Count".into(),
            offset: 0,
            stock: (stock_action.groups.len() as u64).to_le_bytes().to_vec(),
            compiled: (compiled_action.groups.len() as u64).to_le_bytes().to_vec(),
        });
    }
    Ok(out)
}

/// Compares one program's four lists node by node, then their lengths.
fn compare_group(
    stock: &[u8],
    compiled: &[u8],
    left: &DecodedGroup,
    right: &DecodedGroup,
    out: &mut Vec<Difference>,
) {
    for (role, a, b) in [
        ("activation", &left.activation, &right.activation),
        ("removal", &left.removal, &right.removal),
        ("rearm", &left.rearm, &right.rearm),
    ] {
        for (x, y) in a.iter().zip(b) {
            compare_condition(stock, compiled, x, y, role, out);
        }
        if a.len() != b.len() {
            out.push(Difference {
                node: format!("{role} list length"),
                offset: 0,
                stock: (a.len() as u64).to_le_bytes().to_vec(),
                compiled: (b.len() as u64).to_le_bytes().to_vec(),
            });
        }
    }
    for (x, y) in left.effects.iter().zip(&right.effects) {
        compare_effect(stock, compiled, x, y, out);
    }
    if left.effects.len() != right.effects.len() {
        out.push(Difference {
            node: "Effect List Length".into(),
            offset: 0,
            stock: (left.effects.len() as u64).to_le_bytes().to_vec(),
            compiled: (right.effects.len() as u64).to_le_bytes().to_vec(),
        });
    }
}

fn node_bytes(payload: &[u8], offset: usize, size: usize) -> &[u8] {
    payload.get(offset..offset + size).unwrap_or(&[])
}

/// The policy selector and modifier sit inside the routing range compared elsewhere. The root
/// key and the configuration record are carried verbatim, so they are compared byte for byte.
fn compare_policy(stock: &DecodedAction, compiled: &DecodedAction, out: &mut Vec<Difference>) {
    compare_bytes(
        "Root Key".into(),
        &stock.root_key.to_le_bytes(),
        &compiled.root_key.to_le_bytes(),
        &[],
        out,
    );
    match (&stock.policy_configuration, &compiled.policy_configuration) {
        (None, None) => {}
        (Some(x), Some(y)) => {
            if x.class != y.class {
                out.push(Difference {
                    node: "Policy Configuration Class".into(),
                    offset: 0,
                    stock: x.class.to_le_bytes().to_vec(),
                    compiled: y.class.to_le_bytes().to_vec(),
                });
            }
            compare_bytes(
                format!("Policy Configuration 0x{:08X}", x.class),
                &x.bytes,
                &y.bytes,
                &[],
                out,
            );
        }
        (x, y) => out.push(Difference {
            node: "Policy Configuration Presence".into(),
            offset: 0,
            stock: vec![u8::from(x.is_some())],
            compiled: vec![u8::from(y.is_some())],
        }),
    }
}

/// Auxiliary records are carried verbatim, so every byte of every record is compared.
fn compare_auxiliary(stock: &DecodedAction, compiled: &DecodedAction, out: &mut Vec<Difference>) {
    for (index, (x, y)) in stock.auxiliary.iter().zip(&compiled.auxiliary).enumerate() {
        let node = format!("Auxiliary Record {index} 0x{:08X}", x.class);
        if x.class != y.class {
            out.push(Difference {
                node: format!("{node} Class"),
                offset: 0,
                stock: x.class.to_le_bytes().to_vec(),
                compiled: y.class.to_le_bytes().to_vec(),
            });
        }
        compare_bytes(node, &x.bytes, &y.bytes, &[], out);
    }
    if stock.auxiliary.len() != compiled.auxiliary.len() {
        out.push(Difference {
            node: "Auxiliary Record List Length".into(),
            offset: 0,
            stock: (stock.auxiliary.len() as u64).to_le_bytes().to_vec(),
            compiled: (compiled.auxiliary.len() as u64).to_le_bytes().to_vec(),
        });
    }
}

fn condition_mask(kind: u8, size: usize) -> Vec<(usize, usize)> {
    // Ordinals are rebuilt. Linked state changes the evaluator contract.
    let mut mask = vec![(7, 1)];
    match kind {
        // Label globals reference pointers, label rows and the predicate pointer.
        2 => mask.extend([(0x48, 8), (0xD0, 16), (0x110, 8), (0x130, 8)]),
        // Weapon events keep a label globals pointer.
        13..=19 => mask.push((0x50, 8)),
        _ => {}
    }
    mask.retain(|(start, len)| start + len <= size);
    mask
}

fn effect_mask(kind: u8, size: usize) -> Vec<(usize, usize)> {
    let mut mask = Vec::new();
    match kind {
        1 | 3 | 26 => mask.push((8, 8)),
        // The nested condition list descriptor.
        32 => mask.push((0x10, 16)),
        // The value program's two array pointers. Their contents are compared separately.
        10 => mask.extend([(0x20, 8), (0x30, 8)]),
        // The label globals path pointer inside the source label filter.
        14 | 15 => mask.push((0x48, 8)),
        _ => {}
    }
    mask.retain(|(start, len)| start + len <= size);
    mask
}

/// The bytecode and constant rows of a value program, for comparison outside the node.
fn program_bytes(payload: &[u8], program: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (descriptor, stride) in [(program, 1), (program + 0x10, 16)] {
        let count = usize::try_from(u64_at(payload, descriptor).ok()?).ok()?;
        let rows =
            relative_offset(descriptor + 8, 0, i64_at(payload, descriptor + 8).ok()?).ok()? + 16;
        out.extend_from_slice(payload.get(rows..rows + count * stride)?);
    }
    Some(out)
}

fn compare_bytes(
    node: String,
    stock: &[u8],
    compiled: &[u8],
    mask: &[(usize, usize)],
    out: &mut Vec<Difference>,
) {
    let size = stock.len().max(compiled.len());
    let differs = (0..size)
        .map(|offset| {
            let masked = mask
                .iter()
                .any(|(start, len)| (*start..start + len).contains(&offset));
            !masked && byte_at(stock, offset) != byte_at(compiled, offset)
        })
        .collect::<Vec<_>>();
    let mut offset = 0;
    while offset < size {
        if !differs[offset] {
            offset += 1;
            continue;
        }
        let start = offset;
        while offset < size && differs[offset] {
            offset += 1;
        }
        out.push(Difference {
            node: node.clone(),
            offset: start,
            stock: stock.get(start..offset).unwrap_or(&[]).to_vec(),
            compiled: compiled.get(start..offset).unwrap_or(&[]).to_vec(),
        });
    }
}

fn byte_at(bytes: &[u8], offset: usize) -> u8 {
    bytes.get(offset).copied().unwrap_or(0)
}

fn condition_size(condition: &DecodedCondition) -> usize {
    condition
        .catalog()
        .map_or(8, |node| node.struct_size as usize)
}

fn compare_condition(
    stock: &[u8],
    compiled: &[u8],
    a: &DecodedCondition,
    b: &DecodedCondition,
    role: &str,
    out: &mut Vec<Difference>,
) {
    let node = format!("{} ({role})", a.name());
    if a.kind != b.kind {
        out.push(Difference {
            node,
            offset: 5,
            stock: vec![a.kind],
            compiled: vec![b.kind],
        });
        return;
    }
    if layout::condition_layout(a.kind).is_none() && !matches!(a.kind, 2 | 14..=17) {
        compare_native(&node, a.class, &a.native, &b.native, out);
        return;
    }
    let size = condition_size(a);
    compare_bytes(
        node.clone(),
        node_bytes(stock, a.offset, size),
        node_bytes(compiled, b.offset, size),
        &condition_mask(a.kind, size),
        out,
    );
    compare_label_facts(&node, &a.facts, &b.facts, out);
    if a.kind == 2 {
        compare_predicate(
            stock,
            compiled,
            a.offset + 0x130,
            b.offset + 0x130,
            &node,
            out,
        );
    }
}

fn compare_native(node: &str, class: u32, left: &[u8], right: &[u8], out: &mut Vec<Difference>) {
    let canonical = |bytes: &[u8]| {
        let mut graph = crate::sandbox_perk::action::native::Graph::read(bytes, 0, class)?;
        for block in &mut graph.blocks {
            if crate::sandbox_perk::nodes::CONDITIONS
                .iter()
                .any(|entry| entry.observed() && entry.class == block.class)
            {
                block.bytes[7] = 0;
            }
        }
        graph.emit()
    };
    match (canonical(left), canonical(right)) {
        (Ok(left), Ok(right)) => compare_bytes(
            format!("{node} Complete Native Data"),
            &left,
            &right,
            &[],
            out,
        ),
        _ => out.push(Difference {
            node: format!("{node} Unreadable Native Data"),
            offset: 0,
            stock: left.to_vec(),
            compiled: right.to_vec(),
        }),
    }
}

fn compare_label_facts(
    node: &str,
    a: &[crate::sandbox_perk::action::Fact],
    b: &[crate::sandbox_perk::action::Fact],
    out: &mut Vec<Difference>,
) {
    let labels = |facts: &[crate::sandbox_perk::action::Fact]| {
        facts
            .iter()
            .filter_map(|fact| {
                if let FactValue::Labels(values) = &fact.value {
                    Some((
                        fact.label,
                        values
                            .iter()
                            .flat_map(|value| value.to_le_bytes())
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                }
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let a = labels(a);
    let b = labels(b);
    for label in a
        .keys()
        .chain(b.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let left = a.get(label).cloned().unwrap_or_default();
        let right = b.get(label).cloned().unwrap_or_default();
        if left != right {
            out.push(Difference {
                node: format!("{node} {label}"),
                offset: 0,
                stock: left,
                compiled: right,
            });
        }
    }
}

fn compare_predicate(
    stock: &[u8],
    compiled: &[u8],
    a: usize,
    b: usize,
    node: &str,
    out: &mut Vec<Difference>,
) {
    let read = |payload: &[u8], at| -> Option<Vec<u8>> {
        let relative = i64_at(payload, at).ok()?;
        if relative == 0 {
            return Some(Vec::new());
        }
        let target = relative_offset(at, 0, relative).ok()?;
        let marker = target.checked_sub(4)?;
        let class = crate::package_payload::u32_at(payload, marker).ok()?;
        let size = match class {
            0x8080_93F6 => 84,
            0x8080_93F5 => 164,
            _ => return None,
        };
        Some(payload.get(marker..target.checked_add(size)?)?.to_vec())
    };
    let left = read(stock, a);
    let right = read(compiled, b);
    // An unreadable predicate must never become an empty successful comparison.
    if left.is_none() || right.is_none() || left != right {
        out.push(Difference {
            node: format!("{node} Compiled Predicate"),
            offset: 0x130,
            stock: left.unwrap_or_else(|| b"Unreadable".to_vec()),
            compiled: right.unwrap_or_else(|| b"Unreadable".to_vec()),
        });
    }
}

fn compare_effect(
    stock: &[u8],
    compiled: &[u8],
    a: &DecodedEffect,
    b: &DecodedEffect,
    out: &mut Vec<Difference>,
) {
    let node = a.name();
    if a.kind != b.kind {
        out.push(Difference {
            node,
            offset: 0,
            stock: vec![a.kind],
            compiled: vec![b.kind],
        });
        return;
    }
    if layout::effect_layout(a.kind).is_none() && !matches!(a.kind, 1 | 3 | 10 | 14 | 15 | 26 | 32)
    {
        compare_native(&node, a.class, &a.native, &b.native, out);
        return;
    }
    let size = a.catalog().map_or(2, |node| node.struct_size as usize);
    compare_bytes(
        node.clone(),
        node_bytes(stock, a.offset, size),
        node_bytes(compiled, b.offset, size),
        &effect_mask(a.kind, size),
        out,
    );
    compare_label_facts(&node, &a.facts, &b.facts, out);
    match a.kind {
        32 => {
            for (x, y) in a.conditions.iter().zip(&b.conditions) {
                compare_condition(stock, compiled, x, y, "nested", out);
            }
            if a.conditions.len() != b.conditions.len() {
                out.push(Difference {
                    node: format!("{node} nested list length"),
                    offset: 0x10,
                    stock: (a.conditions.len() as u64).to_le_bytes().to_vec(),
                    compiled: (b.conditions.len() as u64).to_le_bytes().to_vec(),
                });
            }
        }
        10 => {
            let left = program_bytes(stock, a.offset + 0x18).unwrap_or_default();
            let right = program_bytes(compiled, b.offset + 0x18).unwrap_or_default();
            if left != right {
                out.push(Difference {
                    node: format!("{node} value program"),
                    offset: 0x18,
                    stock: left,
                    compiled: right,
                });
            }
        }
        _ => {}
    }
}
