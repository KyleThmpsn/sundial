//! Paired numeric output rows and their checked scalar-provider metadata.
use super::*;

pub(super) fn dispatch(
    assembly: &mut Assembly<'_>,
    source: &Payload,
    from: usize,
    native: &Payload,
    to: usize,
) -> Result<()> {
    ensure!(
        source.pointer(from + 24)? == from && native.pointer(to + 16)? == to,
        "Sequence numeric provider points outside its definition"
    );
    let a = assembly.source.read(source.u32(from + 32)?)?;
    let b = assembly.native.tag(native.u32(to + 24)?, None)?;
    for (p, interface, abstract_class, implementation, row_class) in [
        (&a, 0x8080053B, 0x80800539, 0x8080051C, 0x808005C0),
        (&*b, 0x80809AE1, 0x80809AE2, 0x808084DE, 0x80809C56),
    ] {
        let rows = p.array(16, 24, Some(row_class))?;
        ensure!(
            p.u32(8)? == interface
                && p.u32(12)? == abstract_class
                && rows.len() == 1
                && p.u32(rows[0])? == implementation
                && p.u32(rows[0] + 4)? == 0
                && p.bytes::<16>(rows[0] + 8)? == [0; 16],
            "Sequence numeric provider dispatch differs"
        );
    }
    Ok(())
}

pub(super) fn write(
    assembly: &mut Assembly<'_>,
    source: &Payload,
    component: &mut Component,
    template: &Payload,
) -> Result<usize> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let instances = source.array(si + 0xA0, 64, Some(0x8080051C))?;
    let definitions = source.array(sd + 0x190, 56, Some(0x80800788))?;
    ensure!(
        !definitions.is_empty() && definitions.len() <= 256 && instances.len() == definitions.len(),
        "Source sequence output count differs"
    );
    let tr = *template
        .array(template.pointer(24)? + 0x178, 48, Some(0x808084DF))?
        .first()
        .context("Native sequence numeric output envelope absent")?;
    let ti = usize::try_from(template.u64(tr + 8)?)?;
    ensure!(
        template.u32(ti + 4)? == 0x808084DF
            && template.u32(tr + 4)? == 0x808084DE
            && template.u64(ti + 8)? == tr as u64
            && template.bytes::<40>(ti + 24)? == [0; 40]
            && template.u64(tr + 32)? == 0,
        "Native sequence output state is initialized"
    );
    let p = &mut component.payload;
    let ni = p.pointer(16)?;
    let nd = p.pointer(24)?;
    array(
        &mut p.0,
        ni + 0xC0,
        0x808084DE,
        &vec![0; definitions.len() * 64],
        64,
    )?;
    array(
        &mut p.0,
        nd + 0x178,
        0x808084DF,
        &template.bytes::<48>(tr)?.repeat(definitions.len()),
        48,
    )?;
    let mut names = BTreeSet::new();
    for ((si_row, sd_row), (ni_row, nd_row)) in instances.into_iter().zip(&definitions).zip(
        p.array(ni + 0xC0, 64, Some(0x808084DE))?
            .into_iter()
            .zip(p.array(nd + 0x178, 48, Some(0x808084DF))?),
    ) {
        let sd_row = *sd_row;
        dispatch(assembly, source, sd_row, template, tr)?;
        ensure!(
            source.u64(si_row)? == u64::from(source.u32(si)?)
                && source.u64(sd_row)? == u64::from(source.u32(si)?)
                && source.u64(si_row + 8)? == sd_row as u64
                && source.u64(sd_row + 8)? == si_row as u64
                && source.u64(si_row + 16)? == 0x80800788
                && source.u64(sd_row + 16)? == 0x8080051C
                && source.pointer(si_row + 24)? == si
                && source.bytes::<32>(si_row + 32)? == [0; 32]
                && source.u64(sd_row + 40)? == 0
                && source.u32(sd_row + 52)? == 1
                && names.insert(source.u32(sd_row + 48)?),
            "Source numeric output state or pair differs"
        );
        pair(p, component.id, ni_row, nd_row, 0x808084DE, 0x808084DF)?;
        sequence::relative(p, ni_row + 16, ni)?;
        sequence::relative(p, nd_row + 16, nd_row)?;
        put(&mut p.0, nd_row + 40, &source.bytes::<8>(sd_row + 48)?)?;
    }
    Ok(definitions.len())
}

pub(in crate::d1_roi::native::component) fn provider(
    assembly: &mut Assembly<'_>,
    source: &Payload,
    offset: u64,
    component: &Component,
) -> Result<usize> {
    let sd = source.pointer(24)?;
    let p = &component.payload;
    let nd = usize::try_from(p.u64(p.pointer(16)? + 8)?)?;
    let from = source.array(sd + 0x190, 56, Some(0x80800788))?;
    let to = p.array(nd + 0x178, 48, Some(0x808084DF))?;
    ensure!(
        p.u32(nd - 4)? == 0x808084E9 && from.len() == to.len(),
        "Translated sequence outputs differ"
    );
    let index = from
        .iter()
        .position(|&at| (at + 24) as u64 == offset)
        .context("Unknown source sequence output")?;
    dispatch(assembly, source, from[index], p, to[index])?;
    ensure!(
        source.bytes::<8>(from[index] + 48)? == p.bytes::<8>(to[index] + 40)?,
        "Sequence output identity differs"
    );
    Ok(to[index] + 16)
}
