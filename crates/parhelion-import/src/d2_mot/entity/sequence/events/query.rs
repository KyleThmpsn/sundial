//! Area queries retain their geometry, predicates, distance functions and actions.
use super::*;
use crate::d2_mot::native::effects::resources::selectors;

fn dependency(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    sc: u32,
    nc: u32,
) -> Result<()> {
    let tag = if source.u32(from)? == u32::MAX
        && source.u32(from + 4)? == 1
        && source.u64(from + 8)? == 0
    {
        ensure!(
            !bindings.resources.contains_key(&from),
            "empty query reference has a binding"
        );
        u32::MAX
    } else {
        link(bindings, from, sc, nc)?
    };
    put(output, to, &tag.to_le_bytes())
}

fn predicate(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
) -> Result<()> {
    let bindings = bindings
        .selectors
        .as_ref()
        .context("query predicate bindings are absent")?;
    let fragment = selectors::append_selector(
        source,
        from,
        output,
        to,
        &bindings.resources,
        &bindings.groups,
        &bindings.resolver,
    )?;
    ensure!(
        fragment.gates.is_empty()
            && fragment.references.iter().all(|r| r.target.is_some())
            && fragment
                .named_conditions
                .iter()
                .all(|name| bindings.accepts_name(name)),
        "query predicate has unbound categories, resources or named inputs"
    );
    Ok(())
}

fn shape(source: &Payload, from: usize, output: &mut Payload, to: usize) -> Result<()> {
    let source_shape = source.pointer(from)?;
    ensure!(
        source.u32(source_shape - 4)? == 0x80809138 && source.u32(source_shape)? == 2,
        "unsupported query geometry"
    );
    let radius = source.f32(source_shape + 4)?;
    ensure!(radius.is_finite() && radius >= 0.0, "invalid query radius");
    let source_bound = source.pointer(source_shape + 8)?;
    ensure!(
        source.u32(source_bound - 4)? == 0x8080914C,
        "unsupported query geometry provider"
    );
    let source_owner = source.u32(source.pointer(16)?)?;
    for method in 0..5 {
        let at = source_bound + method * 24;
        ensure!(
            source.u32(at)? == 0x8080914C
                && source.u32(at + 4)? == method as u32
                && source.u32(at + 8)? == source_owner
                && source.u32(at + 12)? == 0x8080914C
                && source.u64(at + 16)? == source_bound as u64,
            "query geometry method descriptor differs"
        );
    }
    let scale = source.f32(source_bound + 120)?;
    ensure!(
        scale.is_finite() && scale >= 0.0 && source.u32(source_bound + 124)? == 0,
        "query geometry scale or padding differs"
    );
    let target_shape = object(output, 0x808092B1, 16);
    let target_bound = object(output, 0x808092C3, 104);
    put(output, target_shape, &source.bytes::<8>(source_shape)?)?;
    relative(output, target_shape + 8, target_bound)?;
    relative(output, to, target_shape)?;
    let owner = output.u32(output.pointer(16)?)?;
    for method in 0..4 {
        let at = target_bound + method * 24;
        put(output, at, &0x808092C3u32.to_le_bytes())?;
        put(output, at + 4, &(method as u32).to_le_bytes())?;
        put(output, at + 8, &owner.to_le_bytes())?;
        put(output, at + 12, &0x808092C3u32.to_le_bytes())?;
        put(output, at + 16, &(target_bound as u64).to_le_bytes())?;
    }
    put(output, target_bound + 96, &scale.to_le_bytes())
}

fn action(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    bindings: &Bindings,
    inputs: &BTreeMap<u32, u32>,
) -> Result<usize> {
    let class = source.u32(from - 4)?;
    match class {
        0x80803F31 => {
            ensure!(
                source.u64(from)? == 0 && source.u64(from + 24)? <= 0xFF,
                "query attachment mode or exclusion flags differ"
            );
            let to = object(output, 0x80804AC6, 24);
            dependency(
                source,
                from + 8,
                output,
                to + 8,
                bindings,
                0x80809AD8,
                0x80809C0F,
            )?;
            put(output, to + 16, &source.bytes::<8>(from + 24)?)?;
            Ok(to)
        }
        0x80803F30 => {
            ensure!(
                source.u64(from)? <= 1 && source.u64(from + 8)? == 0,
                "query presentation action extension differs"
            );
            let to = object(output, 0x80804AC5, 24);
            put(output, to, &source.bytes::<8>(from)?)?;
            dependency(
                source,
                from + 16,
                output,
                to + 16,
                bindings,
                0x8080873F,
                0x80808BCD,
            )?;
            Ok(to)
        }
        0x80803F34 => {
            ensure!(
                source.u64(from)? == 0 && source.bytes::<24>(from + 24)? == [0; 24],
                "query damage action has unsupported named scalar references"
            );
            let to = object(output, 0x80804AC9, 232);
            dependency(
                source,
                from + 8,
                output,
                to + 8,
                bindings,
                0x80809B06,
                0x80809C36,
            )?;
            for (sd, nd) in [(0x30, 0x28), (0x70, 0x68), (0xB0, 0xA8)] {
                expression::bound(source, from + sd, output, to + nd, 1, inputs)?;
            }
            Ok(to)
        }
        _ => anyhow::bail!("unsupported area-query action {class:08X}"),
    }
}

fn actions(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    inputs: &BTreeMap<u32, u32>,
) -> Result<()> {
    let mut rows = source.array(from, 96, Some(0x80803F2A))?;
    ensure!(rows.len() <= 4096, "too many area-query actions");
    rows.retain(|row| {
        source
            .pointer(*row + 88)
            .ok()
            .is_none_or(|node| !bindings.omitted_query_actions.contains(&node))
    });
    let start = array(output, to, 0x80804ABF, &vec![0; rows.len() * 88], 88)?;
    for (index, row) in rows.into_iter().enumerate() {
        let target = start + index * 88;
        expression::bound(source, row, output, target, 1, inputs)?;
        ensure!(
            source.u64(row + 64)? == 0
                && source.u32(row + 72)? == u32::MAX
                && source.u32(row + 76)? == 1
                && source.u64(row + 80)? == 0,
            "query action has an unsupported auxiliary resource"
        );
        put(output, target + 72, &u32::MAX.to_le_bytes())?;
        let node = action(source, source.pointer(row + 88)?, output, bindings, inputs)?;
        relative(output, target + 80, node)?;
    }
    Ok(())
}

pub(super) fn write(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
    inputs: &BTreeMap<u32, u32>,
) -> Result<()> {
    ensure!(
        source.u64(from + 0x48)? == u64::from(u32::MAX)
            && source.u8(from + 0x81)? == 0xFF
            && source.bytes::<6>(from + 0x82)? == [0; 6]
            && source.u32(from + 0x94)? == 0,
        "query source extension is active"
    );
    put(output, to + 0x40, &source.bytes::<4>(from + 0x24)?)?;
    shape(source, from + 0x28, output, to + 0x48)?;
    predicate(source, from + 0x30, output, to + 0x50, bindings)?;
    put(output, to + 0x70, &source.bytes::<48>(from + 0x50)?)?;
    put(output, to + 0xA0, &[source.u8(from + 0x80)?])?;
    put(output, to + 0xA4, &source.bytes::<12>(from + 0x88)?)?;
    expression::program(source, from + 0x98, output, to + 0xB0, 1)?;
    for (sd, nd) in [(0xC8, 0xE0), (0x108, 0x120)] {
        expression::bound(source, from + sd, output, to + nd, 1, inputs)?;
    }
    ensure!(
        source.u64(from + 0x148)? == 0
            && source.bytes::<16>(from + 0x150)? == [0; 16]
            && source.bytes::<16>(from + 0x1C8)? == [0; 16],
        "query contains unsupported parameter or response overrides"
    );
    actions(source, from + 0x160, output, to + 0x178, bindings, inputs)?;
    put(output, to + 0x188, &source.bytes::<16>(from + 0x170)?)?;
    expression::program(source, from + 0x180, output, to + 0x198, 1)?;
    ensure!(
        source.u64(from + 0x1B0)? == 0,
        "query auxiliary debug path requires translation"
    );
    dependency(
        source,
        from + 0x1B8,
        output,
        to + 0x1D0,
        bindings,
        0x8080873F,
        0x80808BCD,
    )?;
    let damage = from + 0x1D8;
    let target = to + 0x1E8;
    ensure!(
        source.u64(damage)? == 0 && source.bytes::<80>(damage + 24)? == [0; 80],
        "query damage block contains unsupported named scalar references"
    );
    dependency(
        source,
        damage + 8,
        output,
        target + 8,
        bindings,
        0x808031DE,
        0x80803EB0,
    )?;
    put(output, target + 0x60, &source.bytes::<8>(damage + 0x68)?)?;
    for i in 0..10 {
        expression::bound(
            source,
            damage + 0x70 + i * 64,
            output,
            target + 0x68 + i * 64,
            2,
            inputs,
        )?;
    }
    // Modern separates the query's hit flag and its retirement timestamp flag.
    // Shadowkeep uses one byte for both paths, so only equal settings fit.
    let flags = source.bytes::<8>(from + 0x4C8)?;
    ensure!(
        flags[0] <= 1 && flags[2] == flags[0] && flags[1] == 0 && flags[3..] == [0; 5],
        "query hit and retirement policies require independent native controls"
    );
    put(output, to + 0x4D0, &[flags[0]])?;
    expression::bound(source, from + 0x4D0, output, to + 0x4D8, 1, inputs)?;
    put(output, to + 0x518, &source.bytes::<8>(from + 0x510)?)?;
    Ok(())
}
