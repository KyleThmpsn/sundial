//! Timed control, conditional child graph and four property inputs.
use super::super::super::resources::selectors;
use super::*;

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
    metadata: &BTreeMap<u32, Payload>,
    bindings: &Bindings,
) -> Result<(Vec<Relocation>, Vec<Object>, Vec<u32>)> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    ensure!(
        source.bytes::<32>(si + 16)? == output.bytes::<32>(ni + 16)?
            && source.bytes::<56>(sd + 16)? == output.bytes::<56>(nd + 16)?
            && source.bytes::<16>(si + 0x310)? == [0; 16]
            && source.u64(sd + 0x620)? == 0,
        "timed root contains unsupported extension state"
    );
    // The last provider ends at +758. Cooked bytecode arrays can start their
    // allocation prefix there, so bytes after it are not root extension state.
    let ci = si + 0x290;
    let cd = sd + 0x48;
    let ti = ni + 0x1D0;
    let td = nd + 0x48;
    pair(source, ci, cd, 0x80802C84, 0x80802C85)?;
    pair(output, ti, td, 0x80803A60, 0x80803A61)?;
    ensure!(
        source.pointer(ci + 16)? == si
            && output.pointer(ti + 16)? == ni
            && source.u64(ci + 24)? == 0
            && source.bytes::<16>(ci + 0x70)? == [0; 16]
            && source.u32(cd + 16)? <= 4
            && source.f32(cd + 20)?.is_finite()
            && source.u64(cd + 0x90)? == output.u64(td + 0x78)?,
        "timed control mode or state differs"
    );
    put(&mut output.0, td + 16, &source.bytes::<8>(cd + 16)?)?;
    let mut objects = input(
        source,
        output,
        (ci + 0x20, cd + 0x18, ci),
        (ti + 0x20, td + 0x18, ti),
    )?;
    for (from, to, sc, nc, method) in [
        (0x50, 0x48, 0x808098CD, 0x80809ADE, 2),
        (0x70, 0x60, 0x808098D2, 0x80809AE1, 1),
        (0x98, 0x80, 0x80802C88, 0x80803A64, 0),
    ] {
        provider(
            source,
            cd + from,
            cd,
            sc,
            (0x80802C84, &[method]),
            true,
            metadata,
        )?;
        provider(
            output,
            td + to,
            td,
            nc,
            (0x80803A60, &[method]),
            false,
            metadata,
        )?;
        objects.push(object(source, output, cd + from, sc, td + to, nc)?);
    }
    objects.push(object(source, output, ci, 0x80802C84, ti, 0x80803A60)?);
    objects.push(object(source, output, cd, 0x80802C85, td, 0x80803A61)?);
    put(&mut output.0, nd + 0x400, &source.bytes::<8>(sd + 0x5E8)?)?;
    let selector = selectors::append_selector(
        source,
        sd + 0x5F0,
        output,
        nd + 0x408,
        &bindings.categories,
        &bindings.groups,
        &bindings.resolver,
    )?;
    ensure!(
        selector.gates.is_empty()
            && selector
                .references
                .iter()
                .all(|reference| reference.target.is_some()),
        "timed child selector has unresolved categories or dependencies"
    );
    ensure!(
        source.u64(sd + 0x608)? == 0,
        "timed child debug path requires translation"
    );
    let field = sd + 0x610;
    let target = if source.u32(field)? == u32::MAX
        && source.u32(field + 4)? == 1
        && source.u64(field + 8)? == 0
    {
        ensure!(
            !bindings.resources.contains_key(&field),
            "empty timed child has a binding"
        );
        u32::MAX
    } else {
        let binding = bindings
            .resources
            .get(&field)
            .context("timed conditional child graph is unbound")?;
        ensure!(
            binding.source_class == 0x80809AD8 && binding.native_class == 0x80809C0F,
            "timed child graph class differs"
        );
        binding.tag
    };
    put(&mut output.0, nd + 0x420, &[0; 16])?;
    put(&mut output.0, nd + 0x428, &target.to_le_bytes())?;
    put(&mut output.0, nd + 0x430, &source.bytes::<24>(sd + 0x628)?)?;
    ensure!(
        source.bytes::<16>(si + 0x320)? == output.bytes::<16>(ni + 0x260)?,
        "timed property state is initialized"
    );
    for index in 0..4 {
        let i = si + 0x330 + index * 64;
        let d = sd + 0x640 + index * 24;
        let ti = ni + 0x270 + index * 64;
        let td = nd + 0x448 + index * 24;
        pair(source, i, d, 0x80803568, 0x80803569)?;
        pair(output, ti, td, 0x8080420D, 0x8080420E)?;
        ensure!(
            source.pointer(i + 16)? == si
                && output.pointer(ti + 16)? == ni
                && source.bytes::<40>(i + 24)? == output.bytes::<40>(ti + 24)?
                && source.u64(d + 16)? == 0,
            "timed property input has live state"
        );
        put(&mut output.0, td + 16, &[0; 8])?;
        objects.push(object(source, output, i, 0x80803568, ti, 0x8080420D)?);
        objects.push(object(source, output, d, 0x80803569, td, 0x8080420E)?);
    }
    let mut unbound = vec![
        Object {
            owner: source.u32(si)?,
            class: 0x8080B11E,
            offset: (si + 0x318) as u64,
        },
        Object {
            owner: source.u32(si)?,
            class: 0x8080B120,
            offset: (sd + 0x620) as u64,
        },
    ];
    for (offset, class, method) in [
        (0x6A0, 0x8080955E, 4),
        (0x6C8, 0x8080955E, 5),
        (0x6F0, 0x8080955E, 6),
        (0x718, 0x808098CD, 1),
        (0x738, 0x808095C6, 2),
    ] {
        provider(
            source,
            sd + offset,
            sd,
            class,
            (0x80803564, &[method]),
            true,
            metadata,
        )?;
        unbound.push(Object {
            owner: source.u32(si)?,
            class,
            offset: (sd + offset) as u64,
        });
    }
    Ok((objects, unbound, selector.named_conditions))
}
