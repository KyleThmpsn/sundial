//! Empty effect-instance pools retain capacity and native retirement bookkeeping.
use super::*;

pub(super) struct Pool {
    pub count: usize,
    pub objects: Vec<Relocation>,
}

pub(super) fn emit(source: &Payload, output: &mut Payload) -> Result<Pool> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let mut candidate = output.clone();
    put(&mut candidate.0, ni + 0x6E0, &0u64.to_le_bytes())?;
    put(&mut candidate.0, nd + 0x588, &0u64.to_le_bytes())?;
    if source.u64(si + 0x8D0)? == 0 {
        ensure!(
            source.u64(sd + 0x6D8)? == 0,
            "health effect-pool definition has no runtime"
        );
        *output = candidate;
        return Ok(Pool {
            count: 0,
            objects: Vec::new(),
        });
    }
    let from = source.pointer(si + 0x8D0)?;
    let definition = source.pointer(sd + 0x6D8)?;
    let owner = source.u32(si)?;
    ensure!(
        source.u32(from - 4)? == 0x808040C0
            && source.u32(definition - 4)? == 0x808040C1
            && source.u32(from)? == owner
            && source.u32(definition)? == owner
            && source.u32(from + 4)? == 0x808040C1
            && source.u32(definition + 4)? == 0x808040C0
            && source.u64(from + 8)? == definition as u64
            && source.u64(definition + 8)? == from as u64
            && source.pointer(from + 16)? == si
            && source.u64(from + 24)? == 0
            && source.bytes::<16>(from + 48)? == [0; 16]
            && source.u32(definition + 20)? == 0,
        "health effect-pool pair or initialized state differs"
    );
    let rows = source.array(from + 32, 48, Some(0x808040C3))?;
    ensure!(
        !rows.is_empty()
            && rows.len() <= 4096
            && source.u32(definition + 16)? as usize == rows.len(),
        "health effect-pool capacity differs"
    );
    let mut data = Vec::new();
    for row in rows.iter().copied() {
        // Native stock 80B570B1 supplies the same two invalid handles and
        // inactive state bytes in each 4C45 row. The source adds eight zero bytes.
        ensure!(
            source.u64(row)? == u64::MAX
                && source.u64(row + 8)? == 0
                && source.u64(row + 16)? == 0xFF00
                && source.u64(row + 24)? == u64::MAX
                && source.bytes::<16>(row + 32)? == [0; 16],
            "health effect pool contains live or unsupported slot state"
        );
        data.extend_from_slice(&source.bytes::<40>(row)?);
    }
    let target = typed(&mut candidate, 0x80804C42, 64)?;
    let target_definition = typed(&mut candidate, 0x80804C43, 24)?;
    pair(
        &mut candidate,
        target,
        target_definition,
        0x80804C42,
        0x80804C43,
        ni,
    )?;
    relative(&mut candidate, ni + 0x6E0, target)?;
    relative(&mut candidate, nd + 0x588, target_definition)?;
    put(
        &mut candidate.0,
        target_definition + 16,
        &u32::try_from(rows.len())?.to_le_bytes(),
    )?;
    append_array(&mut candidate.0, target + 32, 0x80804C45, &data, 40)?;
    let objects = vec![
        object(source, &candidate, from, 0x808040C0, target, 0x80804C42)?,
        object(
            source,
            &candidate,
            definition,
            0x808040C1,
            target_definition,
            0x80804C43,
        )?,
    ];
    *output = candidate;
    Ok(Pool {
        count: rows.len(),
        objects,
    })
}
