use super::*;

fn property(id: u8) -> Result<u8> {
    // This source bank dialect adds a property at 0x38. Shared object IDs
    // establish 0x39 as PriorityDistanceOffset, 0x3A as DelayTime and 0x3B
    // as TransitionTime. Attenuation is consequently 0x56, not 0x55.
    ensure!(id != 0x38, "unvalidated source property 38");
    let id = if id >= 0x39 { id - 1 } else { id };
    Ok(match id {
        0 => 0,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        5 => 0x21,
        6 => 6,
        7 => 0x0A,
        8..=0x0B => id + 0x0A,
        0x0C..=0x0F => id + 0x0A,
        0x1B..=0x1D => id - 1,
        0x1E => 0x20,
        0x1F => 0x2F,
        0x20 => 0x30,
        0x21 => 0x36,
        0x22 => 0x3B,
        0x23 => 0x0B,
        0x24 => 0x0C,
        0x29 => 0x0D,
        0x38 => 7,
        0x39 => 0x0E,
        0x3A => 0x0F,
        0x3B => 0x10,
        0x3C => 0x11,
        0x3D..=0x3F => id - 0x20,
        0x40..=0x4A => id - 0x1E,
        0x4B => 0x2D,
        0x4C => 0x2E,
        0x4D..=0x51 => id - 0x1C,
        0x52 => 0x37,
        0x53 => 0x38,
        0x54 => 0x3A,
        _ => bail!("source property {id:02X} has no validated native equivalent"),
    })
}

fn parameter(id: u32) -> Result<u8> {
    // Version 150 uses its unified property numbering for RTPCs too. Matching
    // curve IDs in source and native banks verifies LPF 2 -> 3, Pitch 1 -> 2,
    // OutputBusVolume 13 -> 60 and GameAuxSendVolume 12 -> 19.
    Ok(match id {
        0 => 0,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        5 => 0x24,
        6 => 8,
        7 => 0x3F,
        8..=0x0B => (id + 0x30) as u8,
        0x0C => 0x13,
        0x0D => 0x3C,
        0x0E => 0x3D,
        0x0F => 0x3E,
        0x1B => 0x20,
        0x1C => 0x22,
        0x1D => 0x21,
        0x1E => 0x23,
        0x1F => 0x27,
        0x20 => 0x28,
        0x21 => 0x29,
        0x22 => 6,
        0x23 => 0x14,
        0x24 => 0x15,
        0x26 => 0x25,
        0x27 => 0x26,
        0x29 => 0x0B,
        0x2C => 0x0C,
        0x2D => 0x0D,
        0x2E => 0x0E,
        0x2F => 0x0F,
        0x31 => 0x1C,
        0x35 => 9,
        0x37 => 7,
        0x30 | 0x130 | 0x230 | 0x330 => 0x18 + (id >> 8) as u8,
        _ => bail!("source RTPC parameter {id:02X} has no validated native equivalent"),
    })
}

/// Properties are separate ID and value arrays, not interleaved records.
pub(super) fn props(r: &mut Read<'_>, w: &mut Write, ranged: bool) -> Result<Option<u32>> {
    let count = r.u8()? as usize;
    let ids = r.take(count)?.to_vec();
    let width = if ranged { 8 } else { 4 };
    let values = r.take(count * width)?;
    let mut keep = Vec::new();
    let mut attenuation = None;
    for (index, id) in ids.into_iter().enumerate() {
        if id == 0x56 {
            ensure!(!ranged, "randomized attenuation identity");
            ensure!(
                attenuation
                    .replace(u32::from_le_bytes(
                        values[index * width..index * width + 4].try_into()?
                    ))
                    .is_none(),
                "duplicate attenuation identity"
            );
        } else {
            keep.push((property(id)?, index));
        }
    }
    keep.sort_by_key(|(id, _)| *id);
    w.u8(u8::try_from(keep.len())?);
    for (id, _) in &keep {
        w.u8(*id);
    }
    for (_, index) in keep {
        w.bytes.extend(&values[index * width..(index + 1) * width]);
    }
    Ok(attenuation)
}

pub(super) fn rtpcs(r: &mut Read<'_>, w: &mut Write, c: &mut Convert) -> Result<()> {
    let count = r.u16()?;
    w.u16(count);
    for _ in 0..count {
        let id = r.u32()?;
        let kind = r.u8()?;
        ensure!(
            kind <= 1,
            "source RTPC kind {kind} needs a native controller"
        );
        w.u32(id); // Game and MIDI parameter IDs stay in the game's namespace.
        if kind == 0 {
            c.dependencies.params.insert(id);
        }
        w.u8(kind);
        let accumulation = r.u8()?;
        let source = r.var()?;
        let target = parameter(source)?;
        let accumulation = match (source, accumulation) {
            (6, 2) => 0, // Priority was an exclusive native parameter.
            (2 | 3 | 0x0E | 0x0F | 0x2E | 0x2F, 6) => 1, // Native filters use additive accumulation.
            (0x30 | 0x130 | 0x230 | 0x330 | 0x31, 4) => 0,
            (_, 1..=3) => accumulation - 1,
            _ => bail!("unsupported RTPC accumulation {accumulation} for {source:02X}"),
        };
        w.u8(accumulation);
        w.u8(target);
        w.copy(r, 5)?; // Curve ID and scaling.
        let points = r.u16()?;
        w.u16(points);
        w.copy(r, usize::from(points) * 12)?;
    }
    Ok(())
}

fn states(r: &mut Read<'_>, w: &mut Write, c: &mut Convert, owner: u32) -> Result<()> {
    let count = r.vcount()?;
    let mut properties = BTreeSet::new();
    for _ in 0..count {
        let source = r.var()?;
        let property = parameter(source)?;
        let accumulation = r.u8()?;
        let db = r.u8()?;
        ensure!(
            (accumulation == 2
                || (accumulation == 6 && matches!(source, 2 | 3 | 0x0E | 0x0F | 0x2E | 0x2F)))
                && db <= 1,
            "state property needs nonadditive native conversion"
        );
        ensure!(properties.insert(property), "duplicate state property");
    }
    let count = r.vcount()?;
    w.u32(count as u32);
    for _ in 0..count {
        let group = r.u32()?;
        c.dependencies.groups.insert(group);
        w.u32(group);
        w.copy(r, 1)?;
        let states = r.vcount()?;
        w.u16(u16::try_from(states)?);
        for _ in 0..states {
            let state = r.u32()?;
            w.u32(state);
            let id = c.state_id(owner, group, state);
            w.reference(id, Kind::Object);
            let mut payload = Write::default();
            payload.reference(id, Kind::Object);
            let count = r.u16()?;
            payload.u8(u8::try_from(count)?);
            let mut state_properties = Vec::new();
            for index in 0..count {
                let property = parameter(u32::from(r.u16()?))?;
                ensure!(
                    properties.contains(&property),
                    "state value lacks a property declaration"
                );
                state_properties.push((property, usize::from(index)));
            }
            let values = r.take(usize::from(count) * 4)?;
            state_properties.sort_by_key(|(id, _)| *id);
            for (id, _) in &state_properties {
                payload.u8(*id);
            }
            for (_, index) in state_properties {
                payload.bytes.extend(&values[index * 4..index * 4 + 4]);
            }
            c.extra.push((1, payload));
        }
    }
    Ok(())
}

fn positioning(r: &mut Read<'_>, w: &mut Write, attenuation: Option<u32>) -> Result<()> {
    let bits = r.u8()?;
    ensure!(bits & 0x90 == 0, "unsupported positioning flags {bits:02X}");
    if bits & 1 == 0 {
        ensure!(
            attenuation.is_none_or(|id| id == 0),
            "inherited position overrides attenuation"
        );
        w.u8(0xC0);
        return Ok(());
    }
    let panner = (bits >> 2) & 3;
    ensure!(panner <= 1, "unsupported speaker panner {panner}");
    if bits & 2 == 0 {
        ensure!(
            attenuation.is_none_or(|id| id == 0),
            "2D position has attenuation"
        );
        w.u8(0xC1 | if panner == 1 { 6 } else { 0 });
        return Ok(());
    }
    let spatial = r.u8()?;
    ensure!(
        spatial & 0x80 == 0,
        "diffraction needs a native implementation"
    );
    let mode = spatial & 3;
    ensure!(mode <= 2, "unknown spatialization mode");
    ensure!(bits & 0x60 == 0, "automated 3D paths need conversion");
    if mode == 0 {
        ensure!(
            attenuation.is_none_or(|id| id == 0),
            "nonspatial audio has attenuation"
        );
        w.u8(0xC7);
    } else {
        w.u8(0xD9);
        w.u8(1 | ((spatial & 0x70) >> 1));
        w.reference(
            if spatial & 8 != 0 {
                attenuation.unwrap_or(0)
            } else {
                0
            },
            Kind::Object,
        );
    }
    Ok(())
}

pub(super) fn node(r: &mut Read<'_>, w: &mut Write, c: &mut Convert, id: u32) -> Result<()> {
    w.copy(r, 1)?; // FX inheritance.
    let fx = r.u8()?;
    ensure!(fx <= 4, "too many native effect slots");
    w.u8(fx);
    if fx > 0 {
        let all = r.u8()?;
        ensure!(all <= 1, "invalid all-effects bypass");
        let mut bypass = if all == 1 { 0x10 } else { 0 };
        let mut effects = Vec::new();
        let mut slots = BTreeSet::new();
        for _ in 0..fx {
            let slot = r.u8()?;
            ensure!(slot < 4 && slots.insert(slot), "invalid effect slot");
            let id = r.u32()?;
            let flags = r.u8()?;
            ensure!(flags & !7 == 0, "unsupported effect flags");
            if flags & 1 != 0 {
                bypass |= 1 << slot;
            }
            effects.push((slot, id, (flags >> 1) & 1, (flags >> 2) & 1));
        }
        w.u8(bypass);
        for (slot, id, shared, rendered) in effects {
            w.u8(slot);
            w.reference(id, Kind::Object);
            w.u8(shared);
            w.u8(rendered);
        }
    }
    r.u8()?;
    ensure!(r.u8()? == 0, "source metadata plugin requires conversion");
    w.u8(0); // No attached effect override in this supported subset.
    w.copy(r, 4)?; // Output bus remains global.
    w.object(r)?;
    w.copy(r, 1)?;
    let attenuation = props(r, w, false)?;
    props(r, w, true)?;
    positioning(r, w, attenuation)?;
    let aux = r.u8()?;
    ensure!(aux & 0x10 == 0, "reflection bus override is unsupported");
    w.u8(aux);
    if aux & 8 != 0 {
        w.copy(r, 16)?;
    }
    ensure!(r.u32()? == 0, "reflection bus requires native conversion");
    w.copy(r, 6)?;
    states(r, w, c, id)?;
    rtpcs(r, w, c)?;
    Ok(())
}
