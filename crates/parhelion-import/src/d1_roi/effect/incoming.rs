//! Rise of Iron incoming damage factor providers as Shadowkeep incoming damage modifiers.
//!
//! Source definition `808027FA` (runtime `80802390`) registers a callback with its entity's damage
//! receiver when it starts. Every damage the entity then takes is multiplied by a base program
//! times the factor of each matching row, behind owner, relation and property gates, into one of
//! two accumulators. Native definition `80803F8C` (runtime `80803F8B`) has the same start, cleanup
//! and factor methods with the same signature `6F009C93`. Source `007D9370` and native `00F83E80`
//! evaluate the same algorithm, so each field pairs with the native field its consumer reads.
use super::{owner::expression, *};
use crate::{
    d1_roi::native::{
        damage_profile::{NATIVE_DICTIONARY, SOURCE_DICTIONARY},
        perk::label,
    },
    presentation::append,
};

/// The record of a class's creation metadata, which names the method a created owner starts with:
/// source method 2 and native method 1, the start method in each table.
fn creation(metadata: &Payload, pair: (u32, u32), row: (u32, u32, u32)) -> Result<()> {
    let rows = metadata.array(0x10, 8, Some(row.0))?;
    ensure!(
        metadata.u32(8)? == pair.0
            && metadata.u32(12)? == pair.1
            && rows.len() == 1
            && metadata.u32(rows[0])? == row.1
            && metadata.u32(rows[0] + 4)? == row.2,
        "Incoming damage creation metadata selects another method"
    );
    Ok(())
}

/// Write a source incoming damage provider into a retargeted native template copy with the same
/// row count. The base program, gates, accumulator choice and every row's damage type masks,
/// label masks and factor take source values. Label masks carry no names, so source atoms are
/// read by bit and compiled against Shadowkeep's registry.
#[allow(clippy::too_many_lines)]
pub(super) fn write(
    source: &Source,
    native: &mut Reader,
    target: &mut Payload,
    p: &Payload,
) -> Result<Value> {
    let (si, sd) = (p.pointer(16)?, p.pointer(24)?);
    let owner = p.u32(sd)?;
    ensure!(
        p.u32(si - 4)? == 0x80802390 && p.u32(sd - 4)? == 0x808027FA,
        "Source incoming damage root differs"
    );
    let (ni, nd) = pair(target)?;
    let id = target.u32(nd)?;
    ensure!(
        target.u32(ni - 4)? == 0x80803F8B && target.u32(nd - 4)? == 0x80803F8C,
        "Native incoming damage root differs"
    );
    ensure!(
        p.pointer(sd + 0x60)? == sd && target.pointer(nd + 0x48)? == nd,
        "Incoming damage creation reference differs"
    );
    creation(
        &source.read(p.u32(sd + 0x68)?)?,
        (0x80800581, 0x808008FE),
        (0x808005C0, 0x80802390, 2),
    )?;
    let metadata = native.tag(target.u32(nd + 0x50)?, None)?;
    creation(
        &metadata,
        (0x80809ADE, 0x80809ADD),
        (0x80809C56, 0x80803F8B, 1),
    )?;
    // Neither factor method reads the array beside the creation reference.
    ensure!(
        p.u64(sd + 0x78)? == 0 && target.u64(nd + 0x60)? == 0,
        "Incoming damage has an unread array"
    );

    // The base program. Both builds reserve input slot zero, so slot one is the one named input.
    let e = sd + 0x98;
    pattern::embedded(p, owner, (si + 0x50, e), (0x808008AC, 0x8080038D))?;
    ensure!(
        p.u64(e + 88)? == 0 && [0, 0x811C9DC5].contains(&p.u64(e + 96)?),
        "Source incoming damage program has additional flags or a key"
    );
    let (code, constants, input) = pattern::program(p, e)?;
    let at = nd + 0x80;
    let inputs = target.array(at + 0x40, 40, Some(0x80809789))?;
    ensure!(
        target.u32(at)? == id
            && target.u32(at + 4)? == 0x808089F5
            && target.u64(at + 0x30)? == if input.is_some() { 2 } else { 1 }
            && target.u64(at + 0x38)? == 1
            && inputs.len() == usize::from(input.is_some()),
        "Native incoming damage program reads another input count"
    );
    if let (Some((name, _)), [row]) = (input, inputs.as_slice()) {
        ensure!(
            target.u32(*row)? == id
                && target.u32(*row + 4)? == 0x80809788
                && target.u64(*row + 0x10)? == 0
                && target.u64(*row + 0x18)? == 0x808097C1,
            "Native incoming damage input is not a direct named channel"
        );
        put(&mut target.0, *row + 0x20, &name.to_le_bytes())?;
    }
    let (native_code, values) = expression(target, at)?;
    let grown = native_code != code || values.len() != constants.len();
    if grown {
        append(&mut target.0, at + 0x10, 0x80800009, &code, 1)?;
        append(
            &mut target.0,
            at + 0x20,
            0x80800090,
            &constants.concat(),
            16,
        )?;
    } else {
        for (&at, value) in values.iter().zip(&constants) {
            put(&mut target.0, at, value)?;
        }
    }

    // Gates and accumulator: property key `+100` and invert `+104` at native `+E0/+E4`, owner
    // only `+108`, relation `+109` and accumulator `+10A` at native `+E8..+EA`. A property key
    // names a property whose Shadowkeep counterpart is not established, so only the absent key
    // carries.
    let gates = p.bytes::<16>(sd + 0x100)?;
    let native_gates = target.bytes::<16>(nd + 0xE0)?;
    ensure!(
        gates[..8] == [0xC5, 0x9D, 0x1C, 0x81, 0, 0, 0, 0]
            && native_gates[..8] == gates[..8]
            && gates[8..11].iter().all(|&flag| flag <= 1)
            && gates[11..].iter().all(|&byte| byte == 0)
            && native_gates[11..].iter().all(|&byte| byte == 0),
        "Source incoming damage has a property gate or unknown flags"
    );
    put(&mut target.0, nd + 0xE8, &gates[8..11])?;

    // Rows. Source `808029F8` holds damage type include and exclude masks at `+00/+04`, 256-bit
    // include-any and exclude-any label masks at `+08/+28`, the four test bypass flags at `+48`
    // and the factor at `+4C`. Native `80802A1C` holds the type masks and their bypass flags at
    // `+10/+14` and `+28/+29`, the label predicate at `+18`, an empty source list at `+30` and the
    // factor program at `+50`.
    let rows = p.array(sd + 0x88, 0x50, Some(0x808029F8))?;
    let native_rows = target.array(nd + 0x70, 0xA0, Some(0x80802A1C))?;
    ensure!(
        !rows.is_empty() && rows.len() == native_rows.len(),
        "Source incoming damage row count differs from the native template's"
    );
    let registry = native.tag(NATIVE_DICTIONARY, None)?;
    let mut evidence = Vec::new();
    for (&from, &to) in rows.iter().zip(&native_rows) {
        let types = (p.u32(from)?, p.u32(from + 4)?);
        let bypass = p.bytes::<4>(from + 0x48)?;
        let factor = p.f32(from + 0x4C)?;
        let (include, exclude) = (p.bytes::<32>(from + 8)?, p.bytes::<32>(from + 0x28)?);
        // Damage type indices 0 to 3 are the four types both builds share, as the stock port of
        // this source form keeps include mask 0F.
        ensure!(
            types.0 & !0x0F == 0
                && types.1 & !0x0F == 0
                && bypass.iter().all(|&flag| flag <= 1)
                && bypass[2] == u8::from(include == [0; 32])
                && bypass[3] == u8::from(exclude == [0; 32])
                && factor.is_finite(),
            "Source incoming damage row has other damage types, flags or factor"
        );
        let (lists, mode, record) =
            label::masks(source, SOURCE_DICTIONARY, include, exclude, &registry)?;
        let predicate = to + 0x18;
        // With no labels the shared compiler leaves the predicate unset. The two-operation record
        // with both bypass flags set skips both label tests, as an unset predicate does.
        let (mode, (class, bytes)) = match record {
            Some(record) => (mode, record),
            None => {
                let mut bytes = vec![0; 84];
                bytes[80..82].copy_from_slice(&[1, 1]);
                (0, (0x808093F6, bytes))
            }
        };
        let masks = target.pointer(predicate + 8)?;
        ensure!(
            target.u32(to)? == id
                && target.u32(to + 4)? == 0x80802A1B
                && target.bytes::<8>(predicate)? == [mode, 0, 0, 0, 0, 0, 0, 0]
                && target.u32(masks - 4)? == class
                && bytes.len() == 84
                && target.0.len() >= masks + 84
                && target.u64(to + 0x40)? == 0,
            "Native incoming damage row has another label predicate or a source list"
        );
        put(&mut target.0, to + 0x10, &types.0.to_le_bytes())?;
        put(&mut target.0, to + 0x14, &types.1.to_le_bytes())?;
        put(&mut target.0, to + 0x28, &bypass[..2])?;
        put(&mut target.0, masks, &bytes)?;
        let (code, values) = expression(target, to + 0x50)?;
        ensure!(
            target.u32(to + 0x50)? == id
                && target.u32(to + 0x54)? == 0x808089F7
                && code == [0x34, 0, 0x3E, 0]
                && values.len() == 1,
            "Native incoming damage row factor is not a constant"
        );
        put(&mut target.0, values[0], &factor.to_le_bytes())?;
        evidence.push(
            json!({"types":[types.0, types.1],"bypass":hex::encode(bypass),
            "include":lists[0],"exclude":lists[2],"factor":factor}),
        );
    }
    if grown {
        super::owner::finish(target)?;
    }
    Ok(
        json!({"code":hex::encode(&code),"constants":constants.iter().map(hex::encode)
        .collect::<Vec<_>>(),"input":input.map(|(name, _)| name),"gates":hex::encode(&gates[8..11]),
        "rows":evidence}),
    )
}
