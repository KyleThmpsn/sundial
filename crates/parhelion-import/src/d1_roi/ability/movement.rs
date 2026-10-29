//! Complete Jump properties retain one applied key and their source row order.
use crate::d1_roi::Source;
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use sundial::package_authoring::ability_bank::{
    AbilityTarget, FixedRow, FixedSelection, FixedWitness,
};
use sundial::package_authoring::sandbox_perk::program::{
    AbilityRow, AbilityRowModifier, AbilityRowOrigin, Action, Program, PropertyOperation, Trigger,
};

struct Row {
    offset: usize,
    source_class: u32,
    class: u32,
    selection: FixedSelection,
    words: [u32; 8],
}

fn origin(bank: u32, key: u32) -> AbilityRowOrigin {
    AbilityRowOrigin {
        domain: "d1_roi_jump".into(),
        bank,
        key,
    }
}

fn bank_rows(data: &Payload, bank: u32) -> Result<Vec<usize>> {
    let runtime = data.pointer(16)?;
    let settings = data.pointer(24)?;
    ensure!(
        data.u32(
            runtime
                .checked_sub(4)
                .context("Source Jump runtime marker")?
        )? == 0x8080275C
            && data.u32(
                settings
                    .checked_sub(4)
                    .context("Source Jump settings marker")?
            )? == 0x80802593
            && data.u32(runtime)? == bank
            && data.u32(settings)? == bank
            && data.u64(runtime + 8)? == settings as u64
            && data.u64(settings + 8)? == runtime as u64
            && data.u32(runtime + 16)? == 0x80802593
            && data.u32(settings + 16)? == 0x8080275C,
        "Source Jump bank is not a reciprocal qualified pair"
    );
    data.array(settings + 0xA0, 40, Some(0x80802A84))
}

fn read_row(data: &Payload, offset: usize) -> Result<Option<Row>> {
    if data.u32(offset + 16)? != 255 || data.u32(offset + 20)? != 0 {
        return Ok(None);
    }
    let priority = data.u32(offset + 8)?;
    if !matches!(priority, 0 | 1 | 2 | 255) {
        return Ok(None);
    }
    let required_key = if data.u64(offset + 24)? == 0 {
        0
    } else {
        let gate = data.pointer(offset + 24)?;
        if gate < 4 || data.u32(gate - 4)? != 0x80802981 {
            return Ok(None);
        }
        // The source gate has only one word. The next word may be another class marker.
        data.u32(gate)?
    };
    if [u32::MAX, 0x811C9DC5].contains(&required_key) {
        return Ok(None);
    }
    let body = data.pointer(offset + 32)?;
    let source_class = data.u32(body.checked_sub(4).context("Source movement body marker")?)?;
    let (class, count) = match source_class {
        0x808027E0 => (0x808044D2, 1),
        0x80802330 => (0x808044D4, 2),
        0x80802BC2 => (0x808044D6, 4),
        0x8080209C => (0x808042A5, 1),
        _ => return Ok(None),
    };
    let mut words = [0; 8];
    if source_class == 0x808027E0 {
        words[0] = u32::from(data.u8(body)?);
        if words[0] > 1 {
            return Ok(None);
        }
    } else {
        for (index, word) in words[..count].iter_mut().enumerate() {
            *word = data.u32(body + 4 * index)?;
        }
        if source_class == 0x8080209C {
            if !(1..=16).contains(&words[0]) {
                return Ok(None);
            }
        } else if words[..count].iter().any(|&word| {
            let value = f32::from_bits(word);
            !value.is_finite() || value < 0.0
        }) {
            return Ok(None);
        }
    }
    Ok(Some(Row {
        offset,
        source_class,
        class,
        words,
        selection: FixedSelection {
            name: data.u32(offset + 4)?,
            priority: priority as u8,
            required_key,
        },
    }))
}

fn group(data: &Payload, all: &[usize], key: u32) -> Result<Option<Vec<Row>>> {
    ensure!(
        ![0, u32::MAX, 0x811C9DC5].contains(&key),
        "Invalid source Jump property key"
    );
    let mut result = Vec::new();
    for &offset in all {
        if data.u32(offset)? != key {
            continue;
        }
        let Some(row) = read_row(data, offset)? else {
            return Ok(None);
        };
        result.push(row);
    }
    ensure!(
        !result.is_empty(),
        "Source Jump key has no complete bank group"
    );
    let gated = result
        .iter()
        .filter(|row| row.selection.required_key != 0)
        .collect::<Vec<_>>();
    if !gated.is_empty() {
        // Only signed maxima can duplicate a conditional row when both memberships exist.
        // Gated impulse and preparation groups need a different, qualified membership route.
        if result.iter().any(|row| row.class != 0x808042A5) {
            return Ok(None);
        }
        for row in gated {
            let required = row.selection.required_key;
            if required == key {
                return Ok(None);
            }
            let mut found = false;
            for &offset in all {
                if data.u32(offset)? != required {
                    continue;
                }
                let Some(witness) = read_row(data, offset)? else {
                    return Ok(None);
                };
                if witness.selection.required_key != 0 {
                    return Ok(None);
                }
                found = true;
            }
            if !found {
                return Ok(None);
            }
        }
    }
    Ok(Some(result))
}

/// Discovery and preparation retain every source companion under the same admission boundary.
pub(super) fn accepts(source: &Source, bank: u32, keys: &[u32]) -> Result<bool> {
    ensure!(
        source.reference(bank)? == 0x80800861,
        "Source Jump bank owner differs"
    );
    let data = source.read(bank)?;
    let all = bank_rows(&data, bank)?;
    let unique = keys.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        !keys.is_empty() && unique.len() == keys.len(),
        "Source Jump selection repeats a key"
    );
    for &key in keys {
        if group(&data, &all, key)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn native_words(data: &Payload, body: usize, class: u32) -> Result<[u32; 8]> {
    let count = match class {
        0x808044D2 | 0x808042A5 => 1,
        0x808044D4 => 2,
        0x808044D6 => 4,
        _ => anyhow::bail!("Unqualified native Jump body"),
    };
    let mut words = [0; 8];
    for (index, word) in words[..count].iter_mut().enumerate() {
        *word = data.u32(body + 16 + 4 * index)?;
    }
    Ok(words)
}

fn native_selection(data: &Payload, at: usize) -> Result<FixedSelection> {
    let required_key = if data.u64(at + 40)? == 0 {
        0
    } else {
        let gate = data.pointer(at + 40)?;
        ensure!(
            gate >= 4 && data.u32(gate - 4)? == 0x8080454A && data.u32(gate + 4)? == 0,
            "Native Jump witness has another gate layout"
        );
        data.u32(gate)?
    };
    Ok(FixedSelection {
        name: data.u32(at + 20)?,
        priority: u8::try_from(data.u32(at + 24)?)?,
        required_key,
    })
}

/// Shared by selected movement and armor grants, so they name the same logical property.
pub(crate) fn lower_group(
    source: &Source,
    native: &mut Reader,
    source_bank: u32,
    native_bank: u32,
    key: u32,
) -> Result<(AbilityRow, Vec<Value>)> {
    ensure!(
        source.reference(source_bank)? == 0x80800861,
        "Source Jump bank owner differs"
    );
    let source_data = source.read(source_bank)?;
    let source_rows = bank_rows(&source_data, source_bank)?;
    let rows = group(&source_data, &source_rows, key)?
        .context("Source Jump group has an unqualified gate or companion")?;
    let target = native.tag(native_bank, Some(0x80809C36))?;
    let templates = target.array(target.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let origin = origin(source_bank, key);
    let private_key = origin.identity(native_bank, AbilityTarget::Jump);
    let mut members = Vec::new();
    let mut report = Vec::new();
    for row in rows {
        let mut candidates = Vec::new();
        for &at in &templates {
            if target.u32(at + 16)? != key
                || target.u32(at + 32)? != 255
                || target.u32(at + 36)? != 0
            {
                continue;
            }
            let body = target.pointer(at + 48)?;
            if target.u32(body + 4)? == row.class && native_selection(&target, at)? == row.selection
            {
                candidates.push((at, native_words(&target, body, row.class)?));
            }
        }
        // Repeated mode writes can share every selector. Their native literal distinguishes
        // the witness while the source row order remains authoritative for emitted members.
        if candidates.len() > 1 {
            candidates.retain(|(_, words)| *words == row.words);
        }
        ensure!(
            candidates.len() == 1,
            "Source Jump row has no unique complete native witness"
        );
        let (template, words) = candidates[0];
        let member = FixedRow {
            class: row.class,
            words: row.words,
            selection: row.selection,
            witness: FixedWitness {
                key,
                selection: row.selection,
                words,
            },
        };
        member.validate().map_err(anyhow::Error::msg)?;
        members.push(member);
        let mut private_gate = None;
        if row.selection.required_key != 0 {
            let required = row.selection.required_key;
            ensure!(
                templates
                    .iter()
                    .any(|&at| target.u32(at + 16).ok() == Some(required)),
                "Conditional Jump count has no corresponding native property"
            );
            let gate =
                self::origin(source_bank, required).identity(native_bank, AbilityTarget::Jump);
            ensure!(
                ![0, u32::MAX, 0x811C9DC5, private_key, required].contains(&gate)
                    && !templates
                        .iter()
                        .any(|&at| target.u32(at + 16).ok() == Some(gate)),
                "Private Jump membership identity collides with a native property"
            );
            members.push(FixedRow {
                selection: FixedSelection {
                    required_key: gate,
                    ..row.selection
                },
                ..member
            });
            private_gate = Some(gate);
        }
        report.push(json!({"source_bank":source_bank,"source_property_offset":row.offset,
            "source_key":key,"source_modifier_class":row.source_class,"native_bank":native_bank,
            "native_template_offset":template,"native_modifier_class":row.class,"property_key":private_key,
            "source_priority":row.selection.priority,"private_gate":private_gate,
            "route":"source_jump_group","balance":"source"}));
    }
    let authored = AbilityRow::new(
        native_bank,
        AbilityTarget::Jump,
        255,
        AbilityRowModifier::Ordered {
            origin,
            rows: members,
        },
    );
    authored.check_bank(&target.0).map_err(anyhow::Error::msg)?;
    Ok((authored, report))
}

pub(super) fn lower(
    source: &Source,
    native: &mut Reader,
    source_bank: u32,
    native_bank: u32,
    keys: &[u32],
    name: &str,
) -> Result<(Program, Vec<Value>)> {
    ensure!(
        accepts(source, source_bank, keys)?,
        "Source Jump group is incomplete or unsupported"
    );
    let mut program = Program {
        name: name.to_owned(),
        trigger: Trigger::Always,
        duration_ms: 0,
        ..Program::default()
    };
    let mut report = Vec::new();
    for &key in keys {
        let (row, evidence) = lower_group(source, native, source_bank, native_bank, key)?;
        program.actions.push(Action::AbilityProperty {
            target: AbilityTarget::Jump,
            key: row.key,
            option: PropertyOperation::Apply,
        });
        program.ability_rows.push(row);
        report.extend(evidence);
    }
    program.validate().map_err(anyhow::Error::msg)?;
    Ok((program, report))
}
