//! Translate HUD reports and refuse graph omissions with known numeric behavior.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action::native::{Graph, NodeKind},
    nodes,
    program::NativeRecord,
};

enum Timing {
    Absent,
    ActiveRemoval,
    Unsupported,
}

impl Timing {
    fn admits(&self, mask: u8) -> bool {
        match self {
            Self::Absent => true,
            Self::ActiveRemoval => mask == 2,
            Self::Unsupported => false,
        }
    }
}

fn timing(program: &Program) -> Result<Timing> {
    let mut timers = 0;
    for (kind, node) in program.native_nodes() {
        let NodeKind::Condition(_) = kind else {
            continue;
        };
        let class = nodes::condition(node.kind)
            .context("Native feedback timer condition")?
            .class;
        let graph = Graph::read(&node.bytes, 0, class).map_err(anyhow::Error::msg)?;
        for block in graph.blocks {
            // Source 6CB480 selects kind 1 only. Native ECCDB0 also selects kind 26,
            // so a counter in the timer list could supply a different remaining time.
            if block.class == 0x80803E30 {
                return Ok(Timing::Unsupported);
            }
            timers += usize::from(block.class == 0x80803DCD);
        }
    }
    if timers == 0 {
        // Both lookups leave the report at zero seconds without a selected timer.
        // Player and group holds have other kinds. Attached Duration state belongs
        // to its component and is not part of the action's linked timer list.
        Ok(Timing::Absent)
    } else if timers == 1
        && program
            .native_removal
            .as_ref()
            .is_some_and(|node| node.kind == 1)
        && program.alternative_removals.is_empty()
    {
        Ok(Timing::ActiveRemoval)
    } else {
        Ok(Timing::Unsupported)
    }
}

/// A graph feedback record whose graph changes component inputs, as native graph feedback
/// `80804087`. Feedback is a creation path, not a presentation type, so a modifier bank reached
/// through it changes numbers and cannot be left out. Both builds read the phase mask at `+00`,
/// admission at `+01` and signed target selector at `+02`, map action states 0, 1 and 2 to
/// phases 0, 1 and 3 and create the graph at `+10` without retaining it. A graph of only one
/// modifier bank names the stock graph holding exactly its rows. Other feedback graphs are
/// presentation and are left out.
fn graph_record(
    source: &Source,
    native: Option<&mut Reader>,
    p: &Payload,
    at: usize,
) -> Result<Option<NativeRecord>> {
    let tag = p.u32(at + 16)?;
    if tag == u32::MAX {
        return Ok(None);
    }
    ensure!(
        source.reference(tag)? == 0x80800734,
        "Source feedback resource is not a pattern"
    );
    let graph = source.read(tag)?;
    let mut modifier = false;
    for row in graph.array(0x20, 12, Some(0x80800715))? {
        let owner = graph.u32(row)?;
        ensure!(
            source.reference(owner)? == 0x80800861,
            "Source feedback owner is not a component"
        );
        let owner = source.read(owner)?;
        let definition = owner.pointer(24)?;
        modifier |= owner.u32(
            definition
                .checked_sub(4)
                .context("Feedback component marker")?,
        )? == 0x808020DE;
    }
    if !modifier {
        return Ok(None);
    }
    ensure!(
        p.u8(at + 1)? == 0xFF && p.u8(at)? & !15 == 0 && p.u8(at + 2)? <= 3,
        "Source feedback modifier record has another admission, phase or target"
    );
    let stock = pattern::stock_modifier_graph(
        source,
        native.context("Feedback modifiers need native graphs")?,
        tag,
    )?;
    let mut bytes = vec![0; 24];
    bytes[..3].copy_from_slice(&p.bytes::<3>(at)?);
    bytes[16..20].copy_from_slice(&stock.to_le_bytes());
    Ok(Some(NativeRecord {
        class: 0x80804087,
        bytes,
        hud_status: None,
    }))
}

pub(super) fn read(
    source: &Source,
    mut native: Option<&mut Reader>,
    p: &Payload,
    program: &Program,
) -> Result<(Vec<NativeRecord>, usize)> {
    let rows = p.array(0x10, 8, None)?;
    if rows.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let mut records = Vec::new();
    let mut omitted = 0;
    let timing = timing(program)?;
    for row in rows {
        let at = p.pointer(row)?;
        if p.u32(at - 4)? == 0x80802908
            && let Some(record) = graph_record(source, native.as_deref_mut(), p, at)?
        {
            records.push(record);
            continue;
        }
        if p.u32(at - 4)? != 0x8080314C
            || p.u8(at)? & !15 != 0
            || p.u8(at + 1)? != 255
            || p.u16(at + 2)? != 0
            || p.u8(at + 12)? > 1
            || p.u8(at + 13)? > 1
            || p.u8(at + 14)? != 255
            || p.u8(at + 15)? != 0
            // Reports with no selected timer preserve zero in every phase. Existing
            // countdowns remain bounded to their active, sole-removal-timer route.
            || (p.u8(at + 13)? != 0 && !timing.admits(p.u8(at)?))
        {
            omitted += 1;
            continue;
        }
        let name = p.u32(at + 4)?;
        let hud_status = if name == 0x811C9DC5 {
            None
        } else {
            hud::read(
                source,
                native
                    .as_deref_mut()
                    .context("Parameter feedback needs native HUD tables")?,
                name,
            )?
        };
        let mut bytes = vec![0; 24];
        bytes[..12].copy_from_slice(&p.bytes::<12>(at)?);
        // Original reporters 75ADC0/EF9950 retain priority and presentation flags.
        // FF keeps count at one and bypasses the native-only count remapping range.
        bytes[20] = 255;
        bytes[21] = p.u8(at + 12)?;
        bytes[22] = p.u8(at + 13)?;
        records.push(NativeRecord {
            class: 0x80804085,
            bytes,
            hud_status,
        });
    }
    Ok((records, omitted))
}
