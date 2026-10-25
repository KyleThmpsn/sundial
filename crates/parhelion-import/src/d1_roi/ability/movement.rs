//! Complete Jump property groups whose recipients are independent of row application order.
use crate::d1_roi::Source;
use crate::tiger::reader::Reader;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use sundial::package_authoring::ability_bank::{AbilityTarget, FixedSelection};
use sundial::package_authoring::sandbox_perk::program::{
    AbilityRow, AbilityRowModifier, Action, Program, PropertyOperation, Trigger,
};

struct Row {
    offset: usize,
    key: u32,
    source_class: u32,
    class: u32,
    words: [u32; 8],
}

/// Discovery uses the same complete-group boundary as preparation. Unsupported shapes
/// remain unavailable instead of discarding a gate, companion or earlier mode write.
pub(super) fn accepts(source: &Source, bank: u32, keys: &[u32]) -> Result<bool> {
    Ok(rows(source, bank, keys)?.is_some())
}

fn rows(source: &Source, bank: u32, keys: &[u32]) -> Result<Option<Vec<Row>>> {
    ensure!(
        source.reference(bank)? == 0x80800861,
        "Source Jump bank owner differs"
    );
    let data = source.read(bank)?;
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
    let selected = keys.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        !selected.is_empty()
            && selected.len() == keys.len()
            && selected
                .iter()
                .all(|key| ![0, u32::MAX, 0x811C9DC5].contains(key)),
        "Source Jump selection has invalid property keys"
    );
    let mut found = BTreeSet::new();
    let mut recipients = BTreeSet::new();
    let mut result = Vec::new();
    for offset in data.array(settings + 0xA0, 40, Some(0x80802A84))? {
        let key = data.u32(offset)?;
        if !selected.contains(&key) {
            continue;
        }
        found.insert(key);
        if data.u32(offset + 4)? != 0x811C9DC5
            || data.u32(offset + 8)? != 255
            || data.u32(offset + 16)? != 255
            || data.u32(offset + 20)? != 0
            || data.u64(offset + 24)? != 0
        {
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
        // The current authored-row format gives each row its own applied key. It
        // cannot preserve a source key's internal ordering when two rows replace
        // or multiply the same recipient. Reject the whole group in that case.
        if !recipients.insert(class) {
            return Ok(None);
        }
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
            } else if words[..count].iter().any(|&value| {
                let value = f32::from_bits(value);
                !value.is_finite() || value < 0.0
            }) {
                return Ok(None);
            }
        }
        result.push(Row {
            offset,
            key,
            source_class,
            class,
            words,
        });
    }
    ensure!(
        found == selected,
        "Source Jump property key has no complete bank group"
    );
    Ok(Some(result))
}

pub(super) fn lower(
    source: &Source,
    native: &mut Reader,
    source_bank: u32,
    native_bank: u32,
    keys: &[u32],
    name: &str,
) -> Result<(Program, Vec<Value>)> {
    let rows = rows(source, source_bank, keys)?
        .context("Source Jump group has a gate, ordered recipient or unsupported companion")?;
    let target = native.tag(native_bank, Some(0x80809C36))?;
    let runtime = target.pointer(16)?;
    let settings = target.pointer(24)?;
    ensure!(
        target.u32(
            runtime
                .checked_sub(4)
                .context("Native Jump runtime marker")?
        )? == 0x8080429E
            && target.u32(
                settings
                    .checked_sub(4)
                    .context("Native Jump settings marker")?
            )? == 0x8080429F,
        "Native Jump bank has another controller family"
    );
    let templates = target.array(settings + 0x88, 56, Some(0x8080453B))?;
    let mut program = Program {
        name: name.to_owned(),
        trigger: Trigger::Always,
        duration_ms: 0,
        ..Program::default()
    };
    let mut report = Vec::new();
    for row in rows {
        let candidates = templates
            .iter()
            .copied()
            .filter(|&at| {
                target.u32(at + 16).ok() == Some(row.key)
                    && target.u32(at + 20).ok() == Some(0x811C9DC5)
                    && target.u32(at + 24).ok() == Some(255)
                    && target.u32(at + 32).ok() == Some(255)
                    && target.u32(at + 36).ok() == Some(0)
                    && target.u64(at + 40).ok() == Some(0)
                    && target
                        .pointer(at + 48)
                        .ok()
                        .and_then(|body| target.u32(body + 4).ok())
                        == Some(row.class)
            })
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 1,
            "Source Jump row has no unique native handler witness"
        );
        let authored = AbilityRow::new(
            native_bank,
            AbilityTarget::Jump,
            255,
            AbilityRowModifier::Fixed {
                template_key: row.key,
                class: row.class,
                words: row.words,
                selection: Some(FixedSelection {
                    name: 0x811C9DC5,
                    priority: 255,
                    required_key: 0,
                }),
            },
        );
        authored.check_bank(&target.0).map_err(anyhow::Error::msg)?;
        report.push(json!({
            "source_bank": source_bank, "source_property_offset": row.offset,
            "source_key": row.key, "source_modifier_class": row.source_class,
            "native_bank": native_bank, "native_template_offset": candidates[0],
            "native_modifier_class": row.class, "property_key": authored.key,
            "route": "source_jump_property", "balance": "source",
        }));
        program.actions.push(Action::AbilityProperty {
            target: AbilityTarget::Jump,
            key: authored.key,
            option: PropertyOperation::Apply,
        });
        program.ability_rows.push(authored);
    }
    program.validate().map_err(anyhow::Error::msg)?;
    Ok((program, report))
}
