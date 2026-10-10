//! Explicit, structurally checked substitutions for modern-only behavior.
use super::*;

/// Native lifecycle registrations replace the source registration tables. The
/// modern-only callback has no native registration or graph consumer. Retain the
/// strict interface converter's refusal information and accept only this bounded
/// compatibility omission, never an explicit edge or another initialized shape.
pub(super) fn movement(active: &[(Object, u64)], graph: &Graph) -> Result<Vec<Value>> {
    let mut report = Vec::new();
    for &(object, state) in active {
        ensure!(
            object.class == 0x80802284 && state == 2,
            "unsupported active modern movement provider {object:?} with state {state:X}"
        );
        ensure!(
            !graph
                .connections
                .iter()
                .chain(&graph.named_connections)
                .any(|edge| edge.provider.object == Some(object)
                    || edge.consumer.object == Some(object)),
            "modern-only movement provider has an explicit graph connection"
        );
        report.push(json!({"source_provider":format!("{:08X}",object.class),"source_state":state,
            "difference":"A modern-only movement callback without a Shadowkeep equivalent is omitted. Its additional behavior is not reproduced."}));
    }
    Ok(report)
}

/// Apply the same bounded compensation as native projectile grafting. Both launch
/// copies and any distance-curve endpoint are kept consistent in the private owner.
pub(super) fn launch(payload: &mut Payload, boost: f32) -> Result<Option<Value>> {
    ensure!(
        boost.is_finite() && (1.0..=9998.0).contains(&boost),
        "invalid projectile launch compensation"
    );
    let instance = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    let before = payload.f32(definition + 0x88)?;
    ensure!(
        before.is_finite() && before > 0.0,
        "invalid source projectile launch multiplier"
    );
    let after = (before * boost).min(boost).max(before);
    if after == before {
        return Ok(None);
    }
    for field in [definition + 0x88, instance + 0x144] {
        put(payload, field, &after.to_le_bytes())?;
    }
    if payload.u64(definition + 0xC8)? != 0 {
        let curve = payload.pointer(definition + 0xC8)?;
        ensure!(
            curve >= 4 && payload.u32(curve - 4)? == 0x80803803,
            "projectile launch curve class differs"
        );
        let value = payload.f32(curve)?;
        ensure!(
            value.is_finite() && value >= 0.0,
            "invalid projectile curve speed"
        );
        let value = (value * boost).min(boost).max(value);
        for field in [curve, instance + 0x14C] {
            put(payload, field, &value.to_le_bytes())?;
        }
    }
    Ok(Some(
        json!({"difference":"Launch speed compensates for the native weapon carrier's lower input.",
        "source_multiplier":before,"native_multiplier":after,"boost":boost}),
    ))
}

fn array(p: &mut Payload, field: usize, class: u32, stride: usize, bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.len().is_multiple_of(stride),
        "projectile array width differs"
    );
    let count = bytes.len() / stride;
    put(p, field, &(count as u64).to_le_bytes())?;
    if count == 0 {
        put(p, field + 8, &[0; 8])?;
        return Ok(0);
    }
    let header = (p.0.len() + 19) & !15;
    p.0.resize(header + 16, 0);
    put(p, header - 4, &0x80809FB8u32.to_le_bytes())?;
    put(p, header, &(count as u64).to_le_bytes())?;
    put(p, header + 8, &class.to_le_bytes())?;
    put(
        p,
        field + 8,
        &(header as i64 - (field + 8) as i64).to_le_bytes(),
    )?;
    p.0.extend(bytes);
    Ok(header + 16)
}

pub(super) struct Steering {
    pub payload: Payload,
    pub conditional: Option<Object>,
    pub constant: Option<Payload>,
}

/// The supported modern extension chooses between four-lane one and zero targeting
/// multipliers. Shadowkeep uses the one branch, keeping this separate from hit damage.
pub(super) fn steering(source: &Payload, parameters: &Payload) -> Result<Steering> {
    let mut payload = source.clone();
    let owner = source.u32(source.pointer(16)?)?;
    let mut records = BTreeMap::new();
    for at in (0..source.0.len().saturating_sub(15)).step_by(8) {
        if source.u32(at)? == owner && (0x8080204D..=0x80802055).contains(&source.u32(at + 4)?) {
            let twin = usize::try_from(source.u64(at + 8)?)?;
            ensure!(
                twin + 16 <= source.0.len() && source.u64(twin + 8)? == at as u64,
                "modern targeting extension is not reciprocal"
            );
            ensure!(
                records.insert(source.u32(at + 4)?, at).is_none(),
                "multiple modern targeting modifiers require conversion"
            );
        }
    }
    if records.is_empty() {
        return Ok(Steering {
            payload,
            conditional: None,
            constant: None,
        });
    }
    ensure!(
        records.keys().copied().collect::<Vec<_>>()
            == [0x8080204F, 0x80802050, 0x80802054, 0x80802055],
        "unsupported modern targeting modifier classes"
    );
    // The conditional definition publishes its vector at +20.
    let definition = records[&0x8080204F];
    let rows = source.array(definition + 0x48, 48, Some(0x80802052))?;
    ensure!(rows.len() == 2, "modern targeting branch count differs");
    for (index, &row) in rows.iter().enumerate() {
        for lane in 0..4 {
            ensure!(
                source.f32(row + lane * 4)? == if index == 0 { 1.0 } else { 0.0 },
                "modern targeting values require conversion"
            );
        }
    }
    let name = source.u32(definition + 0x40)?;
    let conditional = Object {
        owner,
        class: 0x808098D2,
        offset: (definition + 0x20) as u64,
    };
    for &at in records.values() {
        put(&mut payload, at, &u32::MAX.to_le_bytes())?;
    }
    let sd = source.pointer(24)?;
    put(&mut payload, sd + 0x1E0, &[0; 24])?;
    let settings = usize::try_from(source.u64(sd + 0x1F8 + 8)?)?;
    put(&mut payload, settings + 0x50, &[0; 32])?;
    // Reuse the checked source parameter format with one declaration. The normal
    // converter emits a fresh one-element native allocation and provider.
    let mut constant = parameters.clone();
    let si = constant.pointer(16)?;
    let sd = constant.pointer(24)?;
    let input = *constant
        .array(si + 0x40, 48, Some(0x8080875C))?
        .first()
        .context("targeting constant input")?;
    let declaration = *constant
        .array(sd + 0xB8, 80, Some(0x8080875D))?
        .first()
        .context("targeting constant declaration")?;
    for field in [si + 0x40, sd + 0xB8] {
        let header = constant.pointer(field + 8)?;
        put(&mut constant, field, &1u64.to_le_bytes())?;
        put(&mut constant, header, &1u64.to_le_bytes())?;
    }
    put(&mut constant, declaration + 48, &name.to_le_bytes())?;
    for lane in 0..4 {
        put(&mut constant, input + 32 + lane * 4, &1f32.to_le_bytes())?;
        put(
            &mut constant,
            declaration + 56 + lane * 4,
            &1f32.to_le_bytes(),
        )?;
    }
    Ok(Steering {
        payload,
        conditional: Some(conditional),
        constant: Some(constant),
    })
}

pub(crate) fn sequence(
    source: &Payload,
    particles: &Value,
    native: &Reader,
    assets: &mut Assets,
    namespace: &str,
    element: f32,
    sound: u32,
) -> Result<(Payload, sequence::Bindings, Vec<Value>)> {
    let parsed = sequence::Sequence::read(source)?;
    let definition = source.pointer(24)?;
    let owner = source.u32(source.pointer(16)?)?;
    let events = source.array(definition + 0x1D8, 24, Some(0x808091F1))?;
    let mut output = source.clone();
    let mut bindings = sequence::Bindings::default();
    identity(
        &mut bindings,
        namespace,
        owner,
        source.u32(definition + 0x30C)?,
    )?;
    let mut kept = BTreeMap::new();
    let mut report = Vec::new();
    for (index, &row) in events.iter().enumerate() {
        let event = source.pointer(row + 16)?;
        let kind = source.u32(event + 0x1C)?;
        let retain = match kind {
            4 => {
                let systems = source.array(event + 0x28, 24, Some(0x808067BB))?;
                let fields = systems
                    .into_iter()
                    .map(|row| sequence::presentation::fields(source, row))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                let ready = fields.iter().all(|&field| {
                    source
                        .u32(field)
                        .ok()
                        .and_then(|tag| particles["systems"][format!("{tag:08X}")].as_str())
                        .is_some()
                });
                if ready {
                    for field in fields {
                        let tag = source.u32(field)?;
                        let symbol = particles["systems"][format!("{tag:08X}")]
                            .as_str()
                            .context("converted bolt particle")?;
                        bindings.resources.insert(
                            field,
                            sequence::Resource {
                                source_class: 0x80806920,
                                native_class: 0x80806E28,
                                tag: assets.external(native, symbol)?,
                            },
                        );
                    }
                    let label = source.u32(event + 0x108)?;
                    identity(&mut bindings, namespace, owner, label)?;
                }
                ready
            }
            6 => {
                bindings.resources.insert(
                    event + 0x50,
                    sequence::Resource {
                        source_class: 0x80809738,
                        native_class: 0x80809802,
                        tag: sound,
                    },
                );
                report.push(json!({"event":index,"difference":"Projectile audio uses the native compatibility cue."}));
                true
            }
            33 | 39 => false,
            _ => anyhow::bail!("unsupported projectile sequence event {kind}"),
        };
        if retain {
            kept.insert(index, kept.len());
        } else {
            report.push(json!({"event":index,"kind":kind,"difference":"This auxiliary presentation event is omitted."}));
        }
    }
    ensure!(
        !kept.is_empty(),
        "projectile sequence has no converted events"
    );
    let mut rows = Vec::new();
    for &index in kept.keys() {
        rows.extend(&source.0[events[index]..events[index] + 24]);
    }
    let start = array(&mut output, definition + 0x1D8, 0x808091F1, 24, &rows)?;
    for (&old, &new) in &kept {
        let field = start + new * 24 + 16;
        put(
            &mut output,
            field,
            &(source.pointer(events[old] + 16)? as i64 - field as i64).to_le_bytes(),
        )?;
    }
    controls(source, &mut output, &parsed.controls, &kept, element)?;
    let size = output.0.len() as u64;
    put(&mut output, 0, &size.to_le_bytes())?;
    sequence::Sequence::read(&output)?;
    Ok((output, bindings, report))
}

fn identity(
    bindings: &mut sequence::Bindings,
    namespace: &str,
    owner: u32,
    label: u32,
) -> Result<()> {
    if bindings.identities.contains_key(&label) {
        return Ok(());
    }
    let text = format!("{namespace}/projectile/{owner:08X}/{label:08X}");
    let value = text.bytes().fold(0x811C9DC5u32, |h, b| {
        h.wrapping_mul(16777619) ^ u32::from(b)
    });
    ensure!(
        value != u32::MAX
            && !bindings
                .identities
                .values()
                .any(|&previous| previous == value),
        "projectile compiled identity is reserved or collides"
    );
    bindings.identities.insert(label, value);
    Ok(())
}

fn controls(
    source: &Payload,
    output: &mut Payload,
    controls: &[sequence::Control],
    kept: &BTreeMap<usize, usize>,
    element: f32,
) -> Result<()> {
    for control in controls {
        if control.class == 0x808091D9 && source.u32(control.offset + 0x4C)? == 0x49FCE899 {
            let selected = (source.f32(control.offset + 0x40)?
                ..=source.f32(control.offset + 0x44)?)
                .contains(&element);
            // The source element chooses presentation even when Shadowkeep cannot
            // represent its gameplay damage type, as with Stasis and Strand.
            let (lower, upper) = if selected {
                (-f32::MAX, f32::MAX)
            } else {
                (f32::MAX, f32::MAX)
            };
            put(output, control.offset + 0x40, &lower.to_le_bytes())?;
            put(output, control.offset + 0x44, &upper.to_le_bytes())?;
        }
        let mut children = Vec::new();
        let mut weights = Vec::new();
        let original_weights = if control.class == 0x808091E1 {
            source.array(control.offset + 0x40, 4, Some(0x8080000F))?
        } else {
            Vec::new()
        };
        for (ordinal, child) in control.children.iter().enumerate() {
            let index = if child.event {
                kept.get(&child.index).copied()
            } else {
                Some(child.index)
            };
            if let Some(index) = index {
                children.extend(u16::from(child.event).to_le_bytes());
                children.extend(u16::try_from(index)?.to_le_bytes());
                if !original_weights.is_empty() {
                    weights.extend(source.bytes::<4>(original_weights[ordinal])?);
                }
            }
        }
        array(output, control.offset + 0x20, 0x808094E9, 4, &children)?;
        if !original_weights.is_empty() {
            ensure!(
                !weights.is_empty(),
                "compatibility filtering removes every weighted sequence branch"
            );
            array(output, control.offset + 0x40, 0x8080000F, 4, &weights)?;
        }
    }
    Ok(())
}
