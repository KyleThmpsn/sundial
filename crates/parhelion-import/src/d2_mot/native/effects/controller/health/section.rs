//! Health sections retain their compiled region, filter and effect-slot references.
use super::*;
use std::collections::BTreeMap;

pub(super) struct Sections {
    pub count: usize,
    pub objects: Vec<Relocation>,
    /// Null source-only interfaces have no implementation. The containing graph
    /// must still reject any connection attempting to bind one of these objects.
    pub unbound: Vec<Object>,
}

fn properties(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    bindings: &Bindings,
) -> Result<(BTreeMap<usize, usize>, Vec<Object>)> {
    let rows = source.array(from, 8, Some(0x80804074))?;
    ensure!(
        rows.len() <= 256 && (!rows.is_empty() || source.u64(from + 8)? == 0),
        "health property list extent differs"
    );
    let mut positions = BTreeMap::new();
    let mut targets = Vec::new();
    let mut unbound = Vec::new();
    for row in rows {
        let at = source.pointer(row)?;
        let class = source.u32(at.checked_sub(4).context("health property prefix")?)?;
        if class == 0x8080AF16 {
            // A 9B2B interface reference consists of metadata plus an object
            // reference. This form has neither an implementation nor a target.
            // It is not mapped to a different native interface.
            ensure!(
                source.bytes::<24>(at)? == [0; 24],
                "health property interface has an implementation requiring translation"
            );
            unbound.push(Object {
                owner: source.u32(source.pointer(16)?)?,
                class,
                offset: at as u64,
            });
            continue;
        }
        let (native, size) = match class {
            0x8080408A => (0x80804BDA, 24),
            0x80804088 => (0x80804BD8, 8),
            0x80804087 => (0x80804BD7, 40),
            0x80804089 => (0x80804BD9, 16),
            0x80804077 => (0x80804BC8, 144),
            class => anyhow::bail!("health property {class:08X} requires translation"),
        };
        let target = typed(output, native, size)?;
        ensure!(
            positions.insert(at, target).is_none(),
            "duplicate health property target"
        );
        targets.push(target);
        match class {
            0x8080408A => {
                ensure!(source.u32(at + 4)? == 0, "health property padding differs");
                put(&mut output.0, target, &source.bytes::<8>(at)?)?;
                let names = source.array(at + 8, 4, Some(0x80800070))?;
                ensure!(
                    names.len() <= 256 && (!names.is_empty() || source.u64(at + 16)? == 0),
                    "health property names exceed limits"
                );
                append_array(
                    &mut output.0,
                    target + 8,
                    0x80800070,
                    &array_bytes(source, at + 8, 4)?,
                    4,
                )?;
            }
            0x80804088 => {
                ensure!(
                    source.u32(at)? <= 1,
                    "health named property selector differs"
                );
                put(&mut output.0, target, &source.bytes::<8>(at)?)?;
            }
            0x80804087 => {
                ensure!(
                    source.u32(at + 4)? == 0
                        && source.u32(at + 12)? == 0
                        && source.bytes::<16>(at + 32)? == [0; 16]
                        && source.u32(at + 52)? == 0,
                    "additional health history names require translation"
                );
                for field in [0, 8, 48] {
                    ensure!(
                        source.f32(at + field)?.is_finite() && source.f32(at + field)? >= 0.0,
                        "invalid health history property"
                    );
                }
                put(&mut output.0, target, &source.bytes::<16>(at)?)?;
                string(source, at + 16, output, target + 16)?;
                string(source, at + 24, output, target + 24)?;
                put(&mut output.0, target + 32, &source.bytes::<4>(at + 48)?)?;
            }
            0x80804089 => {
                ensure!(
                    source.u64(at + 24)? == 0
                        && source.u32(at + 32)? == u32::MAX
                        && matches!(source.u32(at + 36)?, 1 | 2)
                        && source.u64(at + 40)? == 0,
                    "secondary health effect slot requires translation"
                );
                string(source, at, output, target)?;
                resource(
                    source,
                    at + 8,
                    output,
                    target + 8,
                    (0x8080873D, 0x80808BCB),
                    bindings,
                )?;
            }
            0x80804077 => {
                filter::damage(source, at, output, target, &bindings.categories)?;
                filter::ability(source, at + 120, output, target + 112, &bindings.categories)?;
            }
            _ => unreachable!(),
        }
    }
    append_array(
        &mut output.0,
        to,
        0x80804C0E,
        &vec![0; targets.len() * 8],
        8,
    )?;
    for (&field, &target) in output.array(to, 8, Some(0x80804C0E))?.iter().zip(&targets) {
        relative(output, field, target)?;
    }
    Ok((positions, unbound))
}

fn compiled(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    positions: &BTreeMap<usize, usize>,
    bindings: &Bindings,
) -> Result<()> {
    // These are a relative pointer to a compiled property and a direct slot tag,
    // not a debug path followed by a wide resource reference.
    let property = if source.u64(from + 0x158)? == 0 {
        None
    } else {
        Some(source.pointer(from + 0x158)?)
    };
    if let Some(property) = property {
        ensure!(
            source.u32(property - 4)? == 0x80804077,
            "compiled health property has an unsupported class"
        );
        relative(
            output,
            to + 0xF8,
            *positions
                .get(&property)
                .context("compiled health property is outside its property list")?,
        )?;
    }
    ensure!(
        source.u32(from + 0x164)? == u32::MAX && source.u64(from + 0x168)? == 0,
        "compiled health effect slot extension differs"
    );
    let slot = source.u32(from + 0x160)?;
    if slot == u32::MAX {
        ensure!(
            !bindings.resources.contains_key(&(from + 0x160)),
            "empty health slot has a binding"
        );
    } else {
        let binding = bindings
            .resources
            .get(&(from + 0x160))
            .context("compiled health effect slot is unbound")?;
        ensure!(
            binding.source_class == 0x8080873D
                && binding.native_class == 0x80808BCB
                && (0x80800001..=0x81FFFFFF).contains(&binding.tag),
            "compiled health effect slot class differs"
        );
        put(&mut output.0, to + 0xE0, &binding.tag.to_le_bytes())?;
    }
    Ok(())
}

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
    bindings: &Bindings,
    programs: usize,
) -> Result<Sections> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let definitions = source.array(sd + 0x210, 368, Some(0x80804091))?;
    let instances = source.array(si + 0x900, 64, Some(0x80804051))?;
    ensure!(
        !definitions.is_empty() && definitions.len() <= 256 && definitions.len() == instances.len(),
        "health section count differs"
    );
    let mut candidate = output.clone();
    append_array(
        &mut candidate.0,
        nd + 0x1B0,
        0x80804C11,
        &vec![0; definitions.len() * 312],
        312,
    )?;
    append_array(
        &mut candidate.0,
        ni + 0x708,
        0x80804BAB,
        &vec![0; definitions.len() * 64],
        64,
    )?;
    let targets = candidate.array(nd + 0x1B0, 312, Some(0x80804C11))?;
    let runtimes = candidate.array(ni + 0x708, 64, Some(0x80804BAB))?;
    let mut objects = Vec::new();
    let mut unbound = Vec::new();
    for (((&from, &source_i), &to), &native_i) in definitions
        .iter()
        .zip(&instances)
        .zip(&targets)
        .zip(&runtimes)
    {
        ensure!(
            source.u32(from)? == source.u32(si)?
                && source.u32(source_i)? == source.u32(si)?
                && source.u32(from + 4)? == 0x80804051
                && source.u32(source_i + 4)? == 0x80804091
                && source.u64(from + 8)? == source_i as u64
                && source.u64(source_i + 8)? == from as u64
                && source.pointer(source_i + 16)? == si
                && source.bytes::<40>(source_i + 24)? == [0; 40],
            "health section runtime or pair differs"
        );
        ensure!(
            source.bytes::<72>(from + 0x20)? == [0; 72]
                && source.bytes::<48>(from + 0x98)? == [0; 48]
                && source.u64(from + 0x100)? == 0
                && source.u32(from + 0x10C)? == 0
                && source.bytes::<16>(from + 0x110)? == [0; 16]
                && source.u32(from + 0x120)? == f32::MAX.to_bits()
                && source.u32(from + 0x124)? == 0
                && source.bytes::<32>(from + 0x128)? == [0; 32],
            "health section labels, overrides or extensions require translation"
        );
        pair(&mut candidate, native_i, to, 0x80804BAB, 0x80804C11, ni)?;
        put(&mut candidate.0, to + 16, &source.bytes::<16>(from + 16)?)?;
        put(
            &mut candidate.0,
            to + 0xD0,
            &source.bytes::<8>(from + 0xF8)?,
        )?;
        put(&mut candidate.0, to + 0xE0, &u32::MAX.to_le_bytes())?;
        put(
            &mut candidate.0,
            to + 0xE4,
            &source.bytes::<4>(from + 0x108)?,
        )?;
        put(
            &mut candidate.0,
            to + 0x120,
            &source.bytes::<16>(from + 0x148)?,
        )?;
        put(&mut candidate.0, to + 0x130, &1f32.to_le_bytes())?;
        for (field, target) in [(0xC8, 0xB0), (0xE0, 0xC0)] {
            string(source, from + field, &mut candidate, to + target)?;
            resource(
                source,
                from + field + 8,
                &mut candidate,
                to + target + 8,
                (policy::SOURCE_CLASS, policy::NATIVE_CLASS),
                bindings,
            )?;
        }
        rules::emit(
            source,
            from + 0x68,
            &mut candidate,
            to + 0x58,
            &bindings.categories,
            programs,
        )?;
        let (positions, omitted) =
            properties(source, from + 0x78, &mut candidate, to + 0x68, bindings)?;
        unbound.extend(omitted);
        compiled(source, from, &mut candidate, to, &positions, bindings)?;
        response::emit(source, from + 0x88, &mut candidate, to + 0x78, bindings)?;
        objects.push(object(
            source, &candidate, from, 0x80804091, to, 0x80804C11,
        )?);
        objects.push(object(
            source, &candidate, source_i, 0x80804051, native_i, 0x80804BAB,
        )?);
    }
    *output = candidate;
    Ok(Sections {
        count: definitions.len(),
        objects,
        unbound,
    })
}
