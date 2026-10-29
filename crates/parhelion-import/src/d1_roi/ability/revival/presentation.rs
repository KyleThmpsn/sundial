//! Retain native presentation without scheduling a donor's attacks, queries or spawned objects.
use crate::{presentation::put, tiger::payload::Payload};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// Filter only external-event edges. Control timing, original event bodies, native inputs and
/// allocation storage stay intact. Unscheduled events remain in their native tables for lifecycle
/// compatibility, but no control can activate them. Numeric-event control domains are rejected.
pub(super) fn retain(payload: &mut Payload) -> Result<Value> {
    let instance = payload.pointer(16)?;
    let definition = usize::try_from(payload.u64(instance + 8)?)?;
    ensure!(
        payload.u32(instance - 4)? == 0x808084D7 && payload.u32(definition - 4)? == 0x808084E9,
        "Native presentation is not a sequence pair"
    );
    let events = payload
        .array(definition + 0x168, 24, Some(0x808093E6))?
        .into_iter()
        .map(|row| {
            let at = payload.pointer(row + 16)?;
            let class = payload.u32(at - 4)?;
            let kind = payload.u16(at + 0x30)?;
            let allowed = matches!(
                (class, kind),
                (0x808093CB, 0)
                    | (0x80806CC6, 4)
                    | (0x80806B38, 6)
                    | (0x80803FE9, 18)
                    | (0x80804595, 19)
            );
            Ok((at, class, kind, allowed))
        })
        .collect::<Result<Vec<_>>>()?;
    let controls = payload.array(definition + 0x158, 24, Some(0x808093E6))?;
    ensure!(
        !controls.is_empty(),
        "Native presentation has no root control"
    );
    let mut edges = Vec::new();
    let mut retained = Vec::new();
    let mut removed = Vec::new();
    for (index, row) in controls.iter().enumerate() {
        let at = payload.pointer(row + 16)?;
        ensure!(
            matches!(
                (payload.u32(at - 4)?, payload.u16(at + 0x30)?),
                (0x808093D9, 0) | (0x808093D7, 1) | (0x808093CD, 6)
            ),
            "Native presentation uses an unqualified control kind"
        );
        let children = payload.array(at + 0x38, 4, Some(0x808093FB))?;
        let mut kept = Vec::new();
        let mut branches = Vec::new();
        for child in &children {
            let domain = payload.u16(*child)?;
            let selected = usize::from(payload.u16(*child + 2)?);
            let keep = match domain {
                0 => {
                    ensure!(
                        selected > index && selected < controls.len(),
                        "Native presentation controls are cyclic or unordered"
                    );
                    branches.push(selected);
                    true
                }
                1 => {
                    let event = events
                        .get(selected)
                        .context("Native event selector is out of bounds")?;
                    if event.3 {
                        retained.push(selected);
                    } else {
                        removed.push(selected);
                    }
                    event.3
                }
                _ => anyhow::bail!("Native presentation uses another child domain"),
            };
            if keep {
                kept.push(payload.bytes::<4>(*child)?);
            }
        }
        if kept.len() != children.len() {
            if kept.is_empty() {
                put(&mut payload.0, at + 0x38, &[0; 16])?;
            } else {
                let header = payload.pointer(at + 0x40)?;
                let count = u64::try_from(kept.len())?.to_le_bytes();
                put(&mut payload.0, at + 0x38, &count)?;
                put(&mut payload.0, header, &count)?;
                for (&destination, bytes) in children.iter().zip(kept) {
                    put(&mut payload.0, destination, &bytes)?;
                }
            }
        }
        edges.push(branches);
    }
    // Require a connected original tree. Orphan controls would make the retained-event count
    // misleading and could hide a second scheduler root in an unsupported envelope.
    let mut reachable = std::collections::BTreeSet::new();
    let mut pending = vec![0];
    while let Some(index) = pending.pop() {
        if reachable.insert(index) {
            pending.extend(edges[index].iter().copied());
        }
    }
    ensure!(
        reachable.len() == controls.len(),
        "Native presentation has orphan controls"
    );
    let describe = |selected: Vec<usize>| {
        selected
            .into_iter()
            .map(|index| {
                let (offset, class, kind, _) = events[index];
                json!({"index":index,"offset":offset,"class":class,"kind":kind})
            })
            .collect::<Vec<_>>()
    };
    Ok(
        json!({"retained":describe(retained),"unscheduled":describe(removed),
        "limits":["Uses native animation, particle and sound resources.",
            "Event scheduling is filtered. Original event storage remains for native lifecycle handling."]}),
    )
}
