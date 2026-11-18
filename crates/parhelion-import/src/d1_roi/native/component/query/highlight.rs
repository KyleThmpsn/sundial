//! Keep native highlight visuals for source query outputs whose only event changes materials.
use super::*;

/// Output ordinals retain their meaning. Native graph dependencies supply native rendering,
/// while the containing query keeps the source candidate filters and presentation controls.
pub(super) fn retain(
    source: &Source,
    native: &mut Reader,
    original: &Payload,
    target: &Payload,
) -> Result<Value> {
    let mut outputs = Vec::new();
    let (s, n) = (original.pointer(24)?, target.pointer(24)?);
    for (slot, (sf, nf)) in [(0x168, 0x178), (0x170, 0x180), (0x178, 0x188)]
        .into_iter()
        .enumerate()
    {
        ensure!(
            (original.u64(s + sf)? == 0) == (target.u64(n + nf)? == 0),
            "Query highlight output presence differs"
        );
        if original.u64(s + sf)? == 0 {
            continue;
        }
        let from = original.pointer(s + sf)?;
        let to = target.pointer(n + nf)?;
        ensure!(
            original.u32(from - 4)? == 0x80801108
                && original.u64(from + 16)? == 0x80801109
                && original.u64(from + 24)? == 0
                && original.u32(from + 36)? == 0
                && target.u32(to - 4)? == 0x8080382A
                && target.u32(to + 4)? == 0x80803829
                && target.u64(to + 16)? == 0
                && target.u32(to + 28)? == 0,
            "Query highlight output record differs"
        );
        let source_graph = original.u32(from + 32)?;
        let native_graph = target.u32(to + 24)?;
        let (source_owner, source_event) = source_event(source, source_graph)?;
        let (native_owner, native_event) = native_event(native, native_graph)?;
        ensure!(
            source_owner.bytes::<8>(source_event)? == native_owner.bytes::<8>(native_event + 16)?
                && source_owner.bytes::<16>(source_event + 12)?
                    == native_owner.bytes::<16>(native_event + 28)?,
            "Query highlight event timing differs"
        );
        let array = native.tag(native_owner.u32(native_event + 0x40)?, None)?;
        ensure!(
            !array.array(0x70, 4, Some(0x80800014))?.is_empty(),
            "Native highlight materials are empty"
        );
        outputs.push(json!({"slot":slot,"source_graph":source_graph,"native_graph":native_graph,
            "visual_policy":"Shadowkeep native highlight visuals replace the source material effects",
            "gameplay_verified":false}));
    }
    Ok(json!(outputs))
}

fn source_event(source: &Source, tag: u32) -> Result<(Payload, usize)> {
    ensure!(
        source.reference(tag)? == 0x80800734,
        "Source highlight graph class"
    );
    let graph = source.read(tag)?;
    ensure!(
        graph.u64(0x30)? == 0 && graph.u64(0x40)? == 0,
        "Source highlight has external wiring"
    );
    let owners = graph.array(0x20, 12, Some(0x80800715))?;
    ensure!(
        owners.len() == 1,
        "Source highlight has another behavior owner"
    );
    let owner = source.read(graph.u32(owners[0])?)?;
    let r = owner.pointer(24)?;
    ensure!(
        owner.u32(r - 4)? == 0x80800610 && owner.u64(r + 0x170)? == 0,
        "Source highlight has another event family"
    );
    let controls = owner.array(r + 0x160, 8, Some(0x80800629))?;
    let events = owner.array(r + 0x180, 8, Some(0x80800629))?;
    ensure!(
        controls.len() == 1 && events.len() == 1,
        "Source highlight sequence is not a single material effect"
    );
    let control = owner.pointer(controls[0])?;
    let children = owner.array(control + 32, 4, Some(0x8080052C))?;
    ensure!(
        owner.u32(control - 4)? == 0x80800691
            && children.len() == 1
            && owner.u16(children[0])? == 2
            && owner.u16(children[0] + 2)? == 0,
        "Source highlight control differs"
    );
    let event = owner.pointer(events[0])?;
    ensure!(
        owner.u32(event - 4)? == 0x808007A2
            && owner.u32(event + 8)? == 0
            && owner.u32(event + 28)? == 1
            && owner.u32(event + 36)? == 0,
        "Source highlight has another effect body"
    );
    let material = owner.u32(event + 32)?;
    ensure!(
        source.reference(material)? == 0x80801B37
            && !source
                .read(material)?
                .array(0x60, 4, Some(0x80800014))?
                .is_empty(),
        "Source highlight material array differs"
    );
    Ok((owner, event))
}

fn native_event(native: &mut Reader, tag: u32) -> Result<(Arc<Payload>, usize)> {
    let graph = native.tag(tag, Some(0x80809C0F))?;
    let mut sequence = None;
    for row in graph.array(0x10, 12, Some(0x80809C04))? {
        let owner = native.tag(graph.u32(row)?, Some(0x80809C36))?;
        match owner.u32(owner.pointer(24)? - 4)? {
            0x808084E9 => ensure!(
                sequence.replace(owner).is_none(),
                "Native highlight has several sequences"
            ),
            0x80809790 => {}
            class => anyhow::bail!("Native highlight has unqualified owner {class:08X}"),
        }
    }
    let owner = sequence.context("Native highlight sequence is absent")?;
    let rows = owner.array(owner.pointer(24)? + 0x168, 24, Some(0x808093E6))?;
    ensure!(rows.len() == 1, "Native highlight has another effect");
    let event = owner.pointer(rows[0] + 16)?;
    ensure!(
        owner.u32(event - 4)? == 0x80806B98,
        "Native highlight is not a material effect"
    );
    Ok((owner, event))
}
