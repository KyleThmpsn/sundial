//! Derive player-stat units from original identity tables and tier definitions.
use super::*;

/// Owner-target Player Stats must keep the same receiving unit as the stock armor
/// Movement attachment. Follow both interfaces on every original player graph.
pub(super) fn owner(
    reader: &Source,
    stock: &mut Reader,
    emitted: &mut Reader,
    source: &Payload,
    s: usize,
    native: &Payload,
    n: usize,
) -> CheckResult<()> {
    if source.u8(s + 2)? != 0 {
        return Ok(());
    }
    let source_tag = source.u32(s + 16)?;
    let attached = reader.read(source_tag)?;
    let mut inputs = Vec::new();
    for at in attached.array(32, 12, Some(0x80800715))? {
        let owner = reader.read(attached.u32(at)?)?;
        let definition = owner.pointer(24)?;
        if owner.u32(definition - 4)? != 0x808020DE {
            continue;
        }
        for row in owner.array(definition + 96, 96, Some(0x8080205F))? {
            if owner.u8(row + 84)? == 13 {
                inputs.push(owner.u16(row + 82)?);
            }
        }
    }
    if inputs.is_empty() {
        return Ok(());
    }
    assert_eq!(
        native.u8(n + 2)?,
        0,
        "Player-stat attachment changed its receiver"
    );

    let witness = stock.tag(0x80BBCC8D, Some(0x808040B5))?;
    let effects = witness
        .array(56, 8, Some(0x808040AC))?
        .into_iter()
        .map(|at| witness.pointer(at))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|&at| witness.u8(at).ok() == Some(1))
        .collect::<Vec<_>>();
    assert_eq!(effects.len(), 1);
    let effect = effects[0];
    assert_eq!(witness.u8(effect + 2)?, 0);
    let graph = stock.tag(witness.u32(effect + 16)?, Some(0x80809C0F))?;
    let owners = graph.array(16, 12, Some(0x80809C04))?;
    assert_eq!(owners.len(), 1);
    let movement = stock.tag(graph.u32(owners[0])?, Some(0x80809C36))?;
    let rows = movement.array(movement.pointer(24)? + 88, 88, Some(0x80803B06))?;
    assert!(!rows.is_empty());
    for row in rows {
        assert_eq!(movement.u8(row + 76)?, 5);
    }

    let mut receivers = Vec::new();
    for (source_graph, native_graph) in [
        (0x80A6A022, 0x80BAB778),
        (0x80A6A05E, 0x80BB1E44),
        (0x80A6A06C, 0x80BB2C9A),
    ] {
        let source_graph = reader.read(source_graph)?;
        let native_graph = stock.tag(native_graph, Some(0x80809C0F))?;
        for (source_metadata, native_metadata, source_class, native_class) in [
            (0x80AAEDDE, 0x80BFDE2F, 0x8080264E, 0x80803A18),
            (0x80AADCC5, 0x80BB2772, 0x80802B52, 0x80803930),
        ] {
            let (source_owner, source_instance, source_provider) =
                publication(&source_graph, source_metadata, true)?;
            let (native_owner, native_instance, native_provider) =
                publication(&native_graph, native_metadata, false)?;
            let original_owner = reader.read(source_owner)?;
            let target_owner = stock.tag(native_owner, Some(0x80809C36))?;
            for (owner, instance, provider, metadata) in [
                (
                    &original_owner,
                    source_instance,
                    source_provider,
                    source_metadata,
                ),
                (
                    &*target_owner,
                    native_instance,
                    native_provider,
                    native_metadata,
                ),
            ] {
                assert_eq!(owner.pointer(16)?, instance);
                assert_eq!(owner.pointer(provider)?, owner.pointer(24)?);
                assert_eq!(owner.u32(provider + 8)?, metadata);
            }
            assert_eq!(reader.read(source_metadata)?.u32(12)?, source_class);
            assert_eq!(stock.tag(native_metadata, None)?.u32(12)?, native_class);
            receivers.push(json!({
                "source_owner":source_owner,"native_owner":native_owner,
                "source_interface":source_class,"native_interface":native_class,
                "source_provider":source_provider,"native_provider":native_provider
            }));
        }
    }
    let native_tag = native.u32(n + 16)?;
    fs::write(
        emitted.output.join(format!(
            "owner-player-stat-{source_tag:08X}-{native_tag:08X}.json"
        )),
        serde_json::to_vec_pretty(&json!({
            "source_graph":source_tag,"emitted_graph":native_tag,"target":0,
            "inputs":inputs,"receivers":receivers,
            "limit":"Persisted attachment target and original receiver interfaces. Natural armor equip and gameplay are not exercised."
        }))?,
    )?;
    Ok(())
}

fn publication(graph: &Payload, metadata: u32, source: bool) -> CheckResult<(u32, usize, usize)> {
    let shift = if source { 16 } else { 0 };
    let owners = graph.array(
        16 + shift,
        12,
        Some(if source { 0x80800715 } else { 0x80809C04 }),
    )?;
    let exports = graph.array(
        88 + shift,
        40,
        Some(if source { 0x80800370 } else { 0x80809C22 }),
    )?;
    let references = graph.array(
        104 + shift,
        24,
        Some(if source { 0x80800790 } else { 0x80809C20 }),
    )?;
    assert_eq!(exports.len(), references.len());
    let matching = exports
        .into_iter()
        .zip(references)
        .filter(|&(export, _)| graph.u32(export + 32).ok() == Some(metadata))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "Player interface publication is ambiguous"
    );
    let (export, reference) = matching[0];
    let owner = graph.u32(export + 16)?;
    assert_eq!(graph.u32(reference)?, owner);
    let ordinal = usize::try_from(graph.u64(reference + 16)?)?;
    assert_eq!(
        graph.u32(*owners.get(ordinal).ok_or("Player owner ordinal")?)?,
        owner
    );
    assert_eq!(graph.u32(export + 4)?, 0);
    Ok((
        owner,
        usize::try_from(graph.u64(reference + 8)?)?,
        usize::try_from(graph.u64(export + 24)?)?,
    ))
}

pub(super) fn unit(reader: &Source, stock: &mut Reader, input: u16) -> CheckResult<f32> {
    // The original getters select these constant bytes before indexing the stat
    // tables. The numeric input is not the investment table's row ordinal.
    let source_root = reader.read(reader.read(0x80A58000)?.u32(16)?)?;
    let native_globals = stock.tag(0x81A2926E, None)?;
    let native_root = stock.tag(native_globals.u32(16)?, None)?;
    let source_constants = reader.read(source_root.u32(0xA8)?)?;
    let native_constants = stock.tag(native_root.u32(0xB8)?, None)?;
    let source_stats = reader.read(source_root.u32(0x448)?)?;
    let native_stats = stock.tag(native_root.u32(0x5F8)?, None)?;
    let from = source_stats.array(8, 16, Some(0x80801870))?;
    let to = native_stats.array(8, 32, Some(0x80807D09))?;
    let source_index = source_constants.u8(0x149 + usize::from(input))?;
    let native_index = native_constants.u8(0x259 + usize::from(input))?;
    let key = source_stats.u32(from[usize::from(source_index)])?;
    assert_eq!(native_stats.u32(to[usize::from(native_index)])?, key);
    assert_eq!(
        key,
        [0x1769266F, 0xB295931F, 0x73D4C363][usize::from(input)]
    );

    let mut scale = None;
    for (source_owner, native_owner) in [
        (0x80A7468B, 0x80C0C47A),
        (0x80A746C4, 0x80EF142C),
        (0x80A746E3, 0x80EF62F3),
    ] {
        let source = reader.read(source_owner)?;
        let native = stock.tag(native_owner, Some(0x80809C36))?;
        let source_groups = source.array(source.pointer(24)? + 0x60, 48, Some(0x80802945))?;
        let native_groups = native.array(native.pointer(24)? + 0x48, 48, Some(0x80803975))?;
        assert_eq!(source_groups.len(), 3);
        assert_eq!(native_groups.len(), source_groups.len());
        for (s, n) in source_groups.into_iter().zip(native_groups) {
            let mut steps = Vec::new();
            for (data, group, class) in [(&source, s, 0x80802A32), (&*native, n, 0x80803979)] {
                let rows = data.array(group, 56, Some(class))?;
                let rows = rows
                    .into_iter()
                    .filter(|&at| data.u32(at + 32).ok() == Some(u32::from(input)))
                    .collect::<Vec<_>>();
                assert_eq!(rows.len(), 1);
                let row = rows[0];
                let tiers = data.u64(row + 16)?;
                assert_eq!(tiers, 17);
                assert_eq!(i32::from_le_bytes(data.bytes::<4>(row + 36)?), -3);
                assert_eq!(data.u8(row + 48)?, 0);
                let step = (data.f32(row + 44)? - data.f32(row + 40)?) / (tiers - 1) as f32;
                assert!(step.is_finite() && step > 0.0);
                steps.push(step);
            }
            let ratio = steps[1] / steps[0];
            if let Some(previous) = scale {
                assert_eq!(previous, ratio);
            }
            scale = Some(ratio);
        }
    }
    scale.ok_or_else(|| "No original player-stat tier correspondence".into())
}
