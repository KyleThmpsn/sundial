//! Wwise 89 streamed and generated-source banks to 113. Both dialects are little endian.
//!
//! The hierarchy remains intact. Typed references join the shared private namespace, while
//! media must be decoded separately. Global buses and game parameters need target validation
//! by the caller. Unsupported features fail before any converted bank is returned.
use super::*;
mod effect;
mod parameter;
mod silence;

/// Upgrade the supported Wwise 89 dialect found in Rise of Iron to native 113.
pub fn upgrade(bytes: &[u8]) -> Result<Bank> {
    convert(bytes, None)
}

/// Preserve source Init defaults that the target does not already define.
pub fn upgrade_with_settings(
    bytes: &[u8],
    source: &settings::Settings,
    native: &settings::Settings,
) -> Result<Bank> {
    convert(bytes, Some((source, native)))
}

fn convert(
    bytes: &[u8],
    settings: Option<(&settings::Settings, &settings::Settings)>,
) -> Result<Bank> {
    ensure!(bytes.len() <= 64 * 1024 * 1024, "source bank exceeds limit");
    let mut r = Read { bytes, at: 0 };
    let mut header = None;
    let mut hierarchy = None;
    while r.at < bytes.len() {
        let name = r.take(4)?;
        let count = r.u32()? as usize;
        let body = r.take(count)?;
        match name {
            b"BKHD" => ensure!(
                header.replace(body).is_none(),
                "duplicate source bank header"
            ),
            b"HIRC" => ensure!(
                hierarchy.replace(body).is_none(),
                "duplicate source hierarchy"
            ),
            b"STID" => {}
            b"DIDX" | b"DATA" => ensure!(body.is_empty(), "embedded early media needs extraction"),
            _ => bail!("unsupported early bank chunk {name:?}"),
        }
    }
    let mut h = Read {
        bytes: header.context("early bank header absent")?,
        at: 0,
    };
    ensure!(h.u32()? == 89, "early bank must be version 89");
    let bank_id = h.u32()?;
    ensure!(
        bank_id != 0 && h.u32()? == 0,
        "early bank identity or language differs"
    );
    ensure!(h.u32()? == 0, "early feedback bank needs conversion");
    let project = h.u32()?;
    ensure!(
        h.take(h.bytes.len() - h.at)?.iter().all(|v| *v == 0),
        "early header padding differs"
    );
    let mut r = Read {
        bytes: hierarchy.context("early bank hierarchy absent")?,
        at: 0,
    };
    let count = r.count()?;
    ensure!(count > 0, "empty early bank hierarchy");
    let mut objects = BTreeSet::new();
    let mut entries = Vec::with_capacity(count);
    let mut source_plugins = BTreeMap::new();
    for _ in 0..count {
        let kind = r.u8()?;
        let size = r.u32()? as usize;
        let mut item = Read {
            bytes: r.take(size)?,
            at: 0,
        };
        let id = item.u32()?;
        ensure!(
            id != 0 && objects.insert(id),
            "duplicate early object {id:08X}"
        );
        if kind == 19 {
            source_plugins.insert(id, item.u32()?);
        }
        entries.push((kind, id, item.bytes));
    }
    r.end()?;
    let mut dependencies = settings::Dependencies::default();
    let mut converted = Write::default();
    converted.u32(count as u32);
    for (kind, id, bytes) in entries {
        let mut item = Read { bytes, at: 4 };
        let mut w = Write::default();
        w.reference(id, Kind::Object);
        object(kind, &mut item, &mut w, &mut dependencies, &source_plugins)
            .with_context(|| format!("early object {id:08X} type {kind} at {}", item.at))?;
        item.end()
            .with_context(|| format!("early object {id:08X} type {kind}"))?;
        converted.u8(kind);
        converted.u32(u32::try_from(w.bytes.len())?);
        converted.append(w);
    }
    let mut header = Write::default();
    header.u32(113);
    header.reference(bank_id, Kind::Bank);
    header.u32(0);
    header.u32(0);
    header.u32(project);
    header.u32(0);
    let mut out = Write::default();
    out.chunk(b"BKHD", header)?;
    if let Some((source, native)) = settings
        && let Some(bytes) = source.supplement(native, &dependencies)?
    {
        out.chunk(
            b"STMG",
            Write {
                bytes,
                refs: Vec::new(),
            },
        )?;
    }
    out.chunk(b"HIRC", converted)?;
    Ok(Bank {
        bytes: out.bytes,
        bank_id,
        objects,
        refs: out.refs,
    })
}

fn flag(r: &mut Read<'_>) -> Result<u8> {
    let value = r.u8()?;
    ensure!(value <= 1, "invalid early bank boolean {value}");
    Ok(value)
}

fn flags(r: &mut Read<'_>, count: u8) -> Result<u8> {
    let mut bits = 0;
    for bit in 0..count {
        bits |= flag(r)? << bit;
    }
    Ok(bits)
}

fn exceptions(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    let count = r.count()?;
    w.u32(count as u32);
    for _ in 0..count {
        w.object(r)?;
        w.u8(flag(r)?);
    }
    Ok(())
}

fn action(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    let action = r.u16()?;
    w.u16(action);
    if matches!(action >> 8, 0x12..=0x14 | 0x19) {
        let id = r.u32()?;
        w.u32(id);
        if matches!(action >> 8, 0x13 | 0x14) {
            dependencies.params.insert(id);
        }
    } else if matches!(action >> 8, 1..=3 | 6 | 7) {
        // A stop, pause, resume, mute or unmute acts on its element's instances, so an element
        // that no bank defines leaves it nothing to act on.
        w.reference(r.u32()?, Kind::Stopped);
    } else {
        w.object(r)?;
    }
    w.u8(flag(r)?);
    parameter::properties(r, w, false)?;
    parameter::properties(r, w, true)?;
    match action >> 8 {
        4 => {
            w.copy(r, 1)?;
            w.reference(r.u32()?, Kind::Bank);
        }
        1..=3 => {
            w.copy(r, 1)?;
            if action >> 8 != 1 {
                w.copy(r, 1)?;
            }
            exceptions(r, w)?;
        }
        // Mute and unmute keep only their fade curve before the exceptions. Stock version 113
        // banks store the same body, as 112 mute and unmute actions across 59,386 banks show.
        // No Land Beyond's shot event `55D686AA` in bank `80AA3FDE` ends with a mute of
        // `19A3FA95`.
        6 | 7 => {
            w.copy(r, 1)?;
            exceptions(r, w)?;
        }
        8..=11 => {
            w.copy(r, 14)?;
            exceptions(r, w)?;
        }
        0x12 | 0x19 => {
            let group = r.u32()?;
            w.u32(group);
            if action >> 8 == 0x12 {
                dependencies.groups.insert(group);
            } else {
                dependencies.switches.insert(group);
            }
            w.copy(r, 4)?;
        }
        0x13 | 0x14 => {
            w.copy(r, 1)?; // Fade curve.
            w.u8(0); // Version 89 transitions never bypass the interpolation.
            w.copy(r, 13)?;
            exceptions(r, w)?;
        }
        0x1C | 0x1D => {}
        _ => bail!("unsupported early action {action:04X}"),
    }
    Ok(())
}

fn sound(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
    source_plugins: &BTreeMap<u32, u32>,
) -> Result<()> {
    let plugin = r.u32()?;
    if plugin == silence::PLUGIN {
        silence::source(r, w, source_plugins)?;
        return parameter::node(r, w, dependencies);
    }
    ensure!(
        matches!(plugin, 0x00010001 | 0x00030001 | 0x00040001),
        "unsupported early source plugin {plugin:08X}"
    );
    ensure!(r.u32()? == 1, "early source is not fully streamed");
    let media = r.u32()?;
    ensure!(
        media != 0 && r.u32()? == media,
        "early media and file identities differ"
    );
    r.u32()?;
    ensure!(r.u8()? == 0, "early source has localized or external media");
    w.u32(0x00010001);
    w.u8(2);
    w.reference(media, Kind::Media);
    w.reference(media, Kind::MediaSize);
    w.u8(0);
    parameter::node(r, w, dependencies)
}

fn random(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    parameter::node(r, w, dependencies)?;
    w.copy(r, 23)?; // Loop range, transition range, repeat avoidance and modes.
    w.u8(flags(r, 5)?);
    children(r, w)?;
    let count = r.u16()?;
    w.u16(count);
    for _ in 0..count {
        w.object(r)?;
        w.copy(r, 4)?;
    }
    Ok(())
}

fn switch(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    parameter::node(r, w, dependencies)?;
    let kind = r.u32()?;
    ensure!(kind <= 1, "early switch group type differs");
    w.u8(kind as u8);
    let group = r.u32()?;
    if kind == 0 {
        dependencies.switches.insert(group);
    } else {
        dependencies.groups.insert(group);
    }
    w.u32(group);
    w.copy(r, 4)?;
    w.u8(flag(r)?);
    let count = r.count()?;
    w.u32(count as u32);
    let mut members = BTreeSet::new();
    for _ in 0..count {
        let child = r.u32()?;
        members.insert(child);
        w.reference(child, Kind::Object);
    }
    // Each switch value lists the children it plays by ID. As with the parameter rows below, an
    // entry naming a node the container does not hold leaves with the conversion. Rise of Iron
    // switch container `170DF364` in Hereafter's weapon bank `80AA81CE` lists `316F602A` under
    // switch value `FCE565B9` beside its three children and keeps a parameter row for it. None of
    // the 12,526 readable source banks defines it, so the entry plays nothing.
    let count = r.count()?;
    w.u32(count as u32);
    for _ in 0..count {
        w.copy(r, 4)?;
        let entries = r.count()?;
        let at = w.bytes.len();
        w.u32(0);
        let mut kept = 0u32;
        for _ in 0..entries {
            let child = r.u32()?;
            if members.contains(&child) {
                w.reference(child, Kind::Object);
                kept += 1;
            }
        }
        w.bytes[at..at + 4].copy_from_slice(&kept.to_le_bytes());
    }
    // Switch parameters configure the container's children by ID. A row naming a node the
    // container does not hold configures nothing and leaves with the conversion. Rise of Iron
    // switch container `28089B55`, repeated in weapon banks such as `80AA37FE` and `80AA32A9`,
    // keeps a row for `3873E04F` beside its four children, and no source bank defines it.
    let count = r.count()?;
    let at = w.bytes.len();
    w.u32(count as u32);
    let mut kept = 0u32;
    for _ in 0..count {
        let node = r.u32()?;
        let bits = flags(r, 2)?;
        let mode = r.u32()?;
        ensure!(mode <= 2, "early switch transition mode differs");
        let fades = r.take(8)?;
        if !members.contains(&node) {
            continue;
        }
        w.reference(node, Kind::Object);
        w.u8(bits);
        w.u8(mode as u8);
        w.bytes.extend_from_slice(fades);
        kept += 1;
    }
    w.bytes[at..at + 4].copy_from_slice(&kept.to_le_bytes());
    Ok(())
}

fn layer(r: &mut Read<'_>, w: &mut Write, dependencies: &mut settings::Dependencies) -> Result<()> {
    parameter::node(r, w, dependencies)?;
    let count = r.count()?;
    w.u32(count as u32);
    let mut members = BTreeSet::new();
    for _ in 0..count {
        let child = r.u32()?;
        members.insert(child);
        w.reference(child, Kind::Object);
    }
    let count = r.count()?;
    w.u32(count as u32);
    for _ in 0..count {
        w.copy(r, 4)?; // A layer identity is scoped to its container.
        parameter::rtpcs(r, w, dependencies)?;
        let id = r.u32()?;
        dependencies.params.insert(id);
        w.u32(id);
        w.u8(0); // Game parameter, not a MIDI or modulator source.
        // A layer's crossfade curves apply to the container's children. An association naming
        // a node the container does not hold applies to nothing and leaves with the conversion,
        // its curve still validated. Rise of Iron blend container `2017BE70` in weapon bank
        // `80AA315E` holds only `3C73E3A0` and associates `0E64E2A5` as well, which no source
        // bank defines.
        let associations = r.count()?;
        let at = w.bytes.len();
        w.u32(associations as u32);
        let mut kept = 0u32;
        for _ in 0..associations {
            let child = r.u32()?;
            let points = r.count()?;
            if members.contains(&child) {
                w.reference(child, Kind::Object);
                w.u32(points as u32);
                parameter::curve(r, w, points)?;
                kept += 1;
            } else {
                parameter::curve(r, &mut Write::default(), points)?;
            }
        }
        w.bytes[at..at + 4].copy_from_slice(&kept.to_le_bytes());
    }
    Ok(())
}

fn attenuation(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    let cone = flag(r)?;
    w.u8(cone);
    if cone != 0 {
        w.copy(r, 16)?;
        w.u32(0);
    }
    let indices = r.take(5)?;
    // Volume, game/user sends, LPF and spread. The native layout inserts HPF
    // before spread and adds focus. Neither has an authored source curve.
    w.bytes.extend_from_slice(&indices[..4]);
    w.u8(255);
    w.u8(indices[4]);
    w.u8(255);
    let count = r.u8()?;
    ensure!(
        indices.iter().all(|i| (*i as i8) < 0 || *i < count),
        "early attenuation curve index is invalid"
    );
    w.u8(count);
    for _ in 0..count {
        w.copy(r, 1)?;
        let points = r.u16()?;
        w.u16(points);
        parameter::curve(r, w, usize::from(points))?;
    }
    parameter::rtpcs(r, w, dependencies)
}

fn object(
    kind: u8,
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
    source_plugins: &BTreeMap<u32, u32>,
) -> Result<()> {
    match kind {
        1 => parameter::properties(r, w, false),
        2 => sound(r, w, dependencies, source_plugins),
        3 => action(r, w, dependencies),
        4 => children(r, w),
        5 => random(r, w, dependencies),
        6 => switch(r, w, dependencies),
        7 => {
            parameter::node(r, w, dependencies)?;
            children(r, w)
        }
        9 => layer(r, w, dependencies),
        14 => attenuation(r, w, dependencies),
        18 => effect::share_set(r, w, dependencies),
        19 => effect::custom(r, w, dependencies),
        _ => bail!("unsupported early hierarchy object type {kind}"),
    }
}
