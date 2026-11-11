//! Complete Jump and Glide properties retain one applied key and their source row order.
//!
//! Glide rows map class for class onto the native glide bank. The fifteen-lane glide factors
//! (`80802826`) are the same body native `80804258` stores, multiplied lane for lane by the same
//! handler in both builds, so the lanes copy unchanged. The impulse pair (`80802330`) maps to
//! `808044D4` as on Jump. The source horizontal burst factor (`80802622`) replaces a dword the
//! glide adds along the movement direction, which native moved into a value program bound by
//! `808044CC`. A constant program returning the source factor reproduces it.
use crate::d1_roi::Source;
use crate::tiger::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use sundial::package_authoring::ability_bank::{
    AbilityTarget, FIXED_WORDS, FixedRow, FixedSelection, FixedWitness,
};
use sundial::package_authoring::sandbox_perk::program::{
    AbilityRow, AbilityRowModifier, AbilityRowOrigin, Action, Program, PropertyOperation, Trigger,
};

/// The native glide burst program class and its block offsets: bytecode at +0x68, the
/// constant's first lane at +0x88.
const PROGRAM_CLASS: u32 = 0x808044CC;
/// The source glide controller definition. Its preparation defaults sit at +0x580, and the
/// accumulator reset copies the horizontal factor from preparation +0x194 (`010F0B60`).
const GLIDE_CONTROLLER: u32 = 0x808027B4;
const GLIDE_HORIZONTAL_DEFAULT: usize = 0x580 + 0x194;

/// Source movement bank families, by their runtime and settings markers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Family {
    Jump,
    Glide,
}

impl Family {
    const ALL: [Self; 2] = [Self::Jump, Self::Glide];

    fn markers(self) -> (u32, u32) {
        match self {
            Self::Jump => (0x8080275C, 0x80802593),
            Self::Glide => (0x808025EB, 0x808025C9),
        }
    }

    /// The family whose runtime marker the source bank carries.
    pub(crate) fn of(data: &Payload) -> Result<Option<Self>> {
        let runtime = data.u32(
            data.pointer(16)?
                .checked_sub(4)
                .context("Source bank marker")?,
        )?;
        Ok(Self::ALL
            .into_iter()
            .find(|family| family.markers().0 == runtime))
    }

    fn name(self) -> &'static str {
        match self {
            Self::Jump => "Jump",
            Self::Glide => "Glide",
        }
    }
}

struct Row {
    offset: usize,
    source_class: u32,
    class: u32,
    selection: FixedSelection,
    words: [u32; FIXED_WORDS],
}

fn origin(family: Family, bank: u32, key: u32) -> AbilityRowOrigin {
    AbilityRowOrigin {
        domain: match family {
            Family::Jump => "d1_roi_jump",
            Family::Glide => "d1_roi_glide",
        }
        .into(),
        bank,
        key,
    }
}

fn bank_rows(data: &Payload, bank: u32, family: Family) -> Result<Vec<usize>> {
    let runtime = data.pointer(16)?;
    let settings = data.pointer(24)?;
    let (runtime_class, settings_class) = family.markers();
    ensure!(
        data.u32(
            runtime
                .checked_sub(4)
                .context("Source movement runtime marker")?
        )? == runtime_class
            && data.u32(
                settings
                    .checked_sub(4)
                    .context("Source movement settings marker")?
            )? == settings_class
            && data.u32(runtime)? == bank
            && data.u32(settings)? == bank
            && data.u64(runtime + 8)? == settings as u64
            && data.u64(settings + 8)? == runtime as u64
            && data.u32(runtime + 16)? == settings_class
            && data.u32(settings + 16)? == runtime_class,
        "Source {} bank is not a reciprocal qualified pair",
        family.name()
    );
    data.array(settings + 0xA0, 40, Some(0x80802A84))
}

/// The source glide's default horizontal burst factor, which the accumulator reset restores
/// before any row replaces it. `owners` are the source ability's graph owners.
pub(crate) fn glide_default(source: &Source, owners: &[u32]) -> Result<Option<u32>> {
    let mut found = None;
    for &owner in owners {
        let data = source.read(owner)?;
        for field in [16, 24] {
            let Ok(at) = data.pointer(field) else {
                continue;
            };
            if at < 4 || data.u32(at - 4)? != GLIDE_CONTROLLER {
                continue;
            }
            let value = data.u32(at + GLIDE_HORIZONTAL_DEFAULT)?;
            ensure!(
                f32::from_bits(value).is_finite(),
                "Source glide default horizontal factor is not a number"
            );
            ensure!(
                found
                    .replace(value)
                    .is_none_or(|previous| previous == value),
                "Source glide controllers disagree on their horizontal default"
            );
        }
    }
    Ok(found)
}

fn read_row(data: &Payload, offset: usize, family: Family) -> Result<Option<Row>> {
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
    // Glide classes without a native witness, such as `80802631`, `80802BA9` and the height
    // threshold `808023D9`, are refused.
    let (class, count) = match (family, source_class) {
        (Family::Jump, 0x808027E0) => (0x808044D2, 1),
        (_, 0x80802330) => (0x808044D4, 2),
        (Family::Jump, 0x80802BC2) => (0x808044D6, 4),
        (Family::Jump, 0x8080209C) => (0x808042A5, 1),
        (Family::Glide, 0x80802826) => (0x80804258, 15),
        (Family::Glide, 0x80802622) => (PROGRAM_CLASS, 1),
        _ => return Ok(None),
    };
    let mut words = [0; FIXED_WORDS];
    if matches!(source_class, 0x80802826 | 0x80802622) {
        // Signed factors: stock glide rows store negative lanes too.
        for (index, word) in words[..count].iter_mut().enumerate() {
            *word = data.u32(body + 4 * index)?;
        }
        if words[..count]
            .iter()
            .any(|&word| !f32::from_bits(word).is_finite())
        {
            return Ok(None);
        }
    } else if source_class == 0x808027E0 {
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

fn group(data: &Payload, all: &[usize], key: u32, family: Family) -> Result<Option<Vec<Row>>> {
    ensure!(
        ![0, u32::MAX, 0x811C9DC5].contains(&key),
        "Invalid source {} property key",
        family.name()
    );
    let mut result = Vec::new();
    for &offset in all {
        if data.u32(offset)? != key {
            continue;
        }
        let Some(row) = read_row(data, offset, family)? else {
            return Ok(None);
        };
        result.push(row);
    }
    ensure!(
        !result.is_empty(),
        "Source {} key has no complete bank group",
        family.name()
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
                let Some(witness) = read_row(data, offset, family)? else {
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
    let all = bank_rows(&data, bank, Family::Jump)?;
    let unique = keys.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        !keys.is_empty() && unique.len() == keys.len(),
        "Source Jump selection repeats a key"
    );
    for &key in keys {
        if group(&data, &all, key, Family::Jump)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn native_words(data: &Payload, body: usize, class: u32) -> Result<[u32; FIXED_WORDS]> {
    let mut words = [0; FIXED_WORDS];
    if class == PROGRAM_CLASS {
        // The writer checks the whole program layout. Its constant identifies the witness.
        ensure!(
            data.bytes::<4>(body + 0x68)? == [0x34, 0, 0x3E, 0],
            "Native glide burst witness is not a constant program"
        );
        words[0] = data.u32(body + 0x88)?;
        return Ok(words);
    }
    let count = match class {
        0x808044D2 | 0x808042A5 => 1,
        0x808044D4 => 2,
        0x808044D6 => 4,
        0x80804258 => 15,
        _ => anyhow::bail!("Unqualified native movement body"),
    };
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

/// Shared by selected movement and armor grants, so they name the same logical property. Given
/// `glide_default`, the source glide's reset value, a glide group without its own burst factor
/// restores it, since native glides reset to another program. Only a group that applies alone
/// beside its foundation may restore it, as two replacing rows would depend on their order.
pub(crate) fn lower_group(
    source: &Source,
    native: &mut Reader,
    source_bank: u32,
    native_bank: u32,
    key: u32,
    glide_default: Option<u32>,
) -> Result<(AbilityRow, Vec<Value>)> {
    ensure!(
        source.reference(source_bank)? == 0x80800861,
        "Source movement bank owner differs"
    );
    let source_data = source.read(source_bank)?;
    let family = Family::of(&source_data)?.context("Source bank is not a movement bank")?;
    let source_rows = bank_rows(&source_data, source_bank, family)?;
    let mut rows = group(&source_data, &source_rows, key, family)?.with_context(|| {
        format!(
            "Source {} group has an unqualified gate or companion",
            family.name()
        )
    })?;
    let mut defaulted = None;
    if let Some(value) = glide_default
        .filter(|_| family == Family::Glide && rows.iter().all(|row| row.class != PROGRAM_CLASS))
    {
        let mut words = [0; FIXED_WORDS];
        words[0] = value;
        rows.push(Row {
            offset: usize::MAX,
            source_class: 0x80802622,
            class: PROGRAM_CLASS,
            selection: FixedSelection {
                name: 0x811C9DC5,
                priority: 255,
                required_key: 0,
            },
            words,
        });
        defaulted = Some(value);
    }
    let target = native.tag(native_bank, Some(0x80809C36))?;
    let templates = target.array(target.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let origin = origin(family, source_bank, key);
    let private_key = origin.identity(native_bank, AbilityTarget::Jump);
    let mut members = Vec::new();
    let mut report = Vec::new();
    for row in rows {
        let restored = row.offset == usize::MAX;
        let mut candidates = Vec::new();
        for &at in &templates {
            // A restored default has no same-key native row. Any constant program row of the
            // bank supplies its storage and handler.
            if (!restored && target.u32(at + 16)? != key)
                || target.u32(at + 32)? != 255
                || target.u32(at + 36)? != 0
            {
                continue;
            }
            let body = target.pointer(at + 48)?;
            if target.u32(body + 4)? != row.class {
                continue;
            }
            let selection = native_selection(&target, at)?;
            // Native glide rows keep priority 0 where the source used 255, so a glide witness
            // matches on name and gate and the emitted row keeps the source priority.
            let selected = match family {
                Family::Jump => selection == row.selection,
                Family::Glide => {
                    selection.name == row.selection.name
                        && selection.required_key == row.selection.required_key
                }
            };
            if selected {
                let Ok(words) = native_words(&target, body, row.class) else {
                    continue;
                };
                candidates.push((at, target.u32(at + 16)?, selection, words));
            }
        }
        // Repeated mode writes can share every selector. Their native literal distinguishes
        // the witness while the source row order remains authoritative for emitted members.
        if restored {
            // The first constant program row in bank order, so the choice is stable.
            candidates.truncate(1);
        } else if candidates.len() > 1 {
            candidates.retain(|(_, _, _, words)| *words == row.words);
        }
        ensure!(
            candidates.len() == 1,
            "Source {} row has no unique complete native witness",
            family.name()
        );
        let (template, witness_key, witness_selection, words) = candidates[0];
        let member = FixedRow {
            class: row.class,
            words: row.words,
            selection: row.selection,
            witness: FixedWitness {
                key: witness_key,
                selection: witness_selection,
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
            let gate = self::origin(family, source_bank, required)
                .identity(native_bank, AbilityTarget::Jump);
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
        let route = match family {
            Family::Jump => "source_jump_group",
            Family::Glide => "source_glide_group",
        };
        report.push(json!({"source_bank":source_bank,
            "source_property_offset":(!restored).then_some(row.offset),
            "source_key":key,"source_modifier_class":row.source_class,"native_bank":native_bank,
            "native_template_offset":template,"native_witness_key":witness_key,
            "native_witness_priority":witness_selection.priority,
            "native_modifier_class":row.class,"property_key":private_key,
            "source_priority":row.selection.priority,"private_gate":private_gate,
            "source_default":restored.then_some(defaulted).flatten(),
            "route":route,"balance":"source"}));
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
        let (row, evidence) = lower_group(source, native, source_bank, native_bank, key, None)?;
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
