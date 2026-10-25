//! Keep every animation consumer's layer buffers at the pose controller's size.
use super::*;

pub(super) fn build(
    assembly: &mut Assembly,
    native: &mut Reader,
    graph: &mut Graph,
    before: usize,
) -> Result<Value> {
    let owner = assembly.owner(0x808036e2)?;
    let p = assembly.payloads.get_mut(&owner).unwrap();
    let root = p.pointer(16)?;
    let mut arrays = Vec::new();
    for (offset, implementation, definition_class, row_class, stride) in [
        (0x560, 0x80803708, 0x80803709, 0x8080371a, 120),
        (0x5a0, 0x80803708, 0x80803709, 0x8080371a, 120),
        (0x620, 0x8080370c, 0x8080370d, 0x8080370f, 56),
        (0x660, 0x8080370c, 0x8080370d, 0x8080370f, 56),
        (0x6a0, 0x8080370c, 0x8080370d, 0x8080370f, 56),
    ] {
        let instance = root + offset;
        let definition = usize::try_from(p.u64(instance + 8)?)?;
        ensure!(
            p.u32(instance)? == owner
                && p.u32(instance + 4)? == definition_class
                && p.u32(definition)? == owner
                && p.u32(definition + 4)? == implementation
                && p.u64(definition + 8)? == instance as u64
                && p.u32(definition + 16)? as usize == before,
            "Wheel output buffer definition differs"
        );
        let rows = p.array(instance + 32, stride, Some(row_class))?;
        ensure!(rows.len() == before, "Wheel output buffer count differs");
        let mut data = bytes(p, &rows, stride);
        data.resize(data.len() + 3 * stride, 0);
        append(&mut p.0, instance + 32, u64::from(row_class), &data, stride)?;
        put(
            &mut p.0,
            definition + 16,
            &u32::try_from(before + 3)?.to_le_bytes(),
        )?;
        arrays.push(
            json!({"instance":instance,"definition":definition,"class":row_class,"count":before+3}),
        );
    }
    let allocation_tag = p.u32(0x44)?;
    let mut allocation = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
    let mut pending = allocation.array(32, 40, Some(0x80808852))?;
    let mut seen = BTreeSet::new();
    let mut counts = [0; 2];
    while let Some(row) = pending.pop() {
        ensure!(
            seen.insert(row) && seen.len() < 1024,
            "Wheel output allocation repeats a node"
        );
        let kind = match (allocation.u32(row)?, allocation.u32(row + 16)?) {
            (0xe5b438c2, 0x8080371a) => Some(0),
            (0x87e5340d, 0x8080370f) => Some(1),
            _ => None,
        };
        if let Some(kind) = kind {
            ensure!(
                allocation.u32(row + 20)? as usize == before,
                "Wheel output allocation count differs"
            );
            put(
                &mut allocation.0,
                row + 20,
                &u32::try_from(before + 3)?.to_le_bytes(),
            )?;
            counts[kind] += 1;
        }
        if allocation.u64(row + 8)? != 0 {
            pending.push(allocation.pointer(row + 8)?);
        }
        pending.extend(allocation.array(row + 24, 40, Some(0x80808852))?);
    }
    ensure!(counts == [2, 3], "Wheel output allocation is incomplete");
    let allocation_symbol = "vehicle-wheel-output-allocation";
    graph.add(
        allocation_symbol,
        allocation_tag,
        &allocation.0,
        None,
        Vec::new(),
    )?;
    assembly.allocations.insert(owner, allocation_symbol.into());
    let symbol = &assembly.symbols[&owner];
    let mut declared = patches(graph, symbol)?;
    let end = p.0.len();
    let seal = instance::seal_span(p, &mut declared, end)?;
    graph.replace(symbol, &p.0, declared)?;
    Ok(json!({"buffers":arrays,"seal":seal}))
}
