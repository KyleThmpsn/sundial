//! Preserve the source Radiance property's priority groups on its retained native bank.
use super::{defaults, owner::Owner};
use crate::{
    d1_roi::Source,
    presentation::{append, put},
    tiger::{allocation::Row as Allocation, payload::Payload},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::ability_bank;

const NO_NAME: u32 = 0x811C9DC5;
const SOURCE_CLASSES: (u32, u32) = (0x80802343, 0x80802B06);
const NATIVE_CLASSES: (u32, u32) = (0x8080417A, 0x8080417B);

/// An unallocated native component and its complete private allocation metadata.
/// Symbolic patches must be resolved before either payload is loaded by the game.
pub struct Built {
    pub payload: Payload,
    pub patches: Vec<(usize, String)>,
    pub allocation: Payload,
    pub runtime: usize,
    pub definition: usize,
    pub evidence: Value,
}

struct Property {
    source_offset: usize,
    source_class: u32,
    key: u32,
    name: u32,
    priority: u32,
    slot: u32,
    mode: u32,
    class: u32,
    body: Vec<u8>,
    resource: Option<u32>,
}

struct Bank {
    source: Payload,
    rows: Vec<Property>,
    parameters: Vec<u8>,
    priorities: Vec<u8>,
}

fn finite(word: u32) -> Result<f32> {
    let value = f32::from_bits(word);
    ensure!(value.is_finite(), "Source Radiance property is not finite");
    Ok(value)
}

fn read(source: &Source, tag: u32) -> Result<Bank> {
    ensure!(
        source.reference(tag)? == 0x80800861,
        "Source Radiance bank owner type differs"
    );
    let data = source.read(tag)?;
    let runtime = data.pointer(16)?;
    let definition = data.pointer(24)?;
    ensure!(
        runtime >= 4
            && definition >= 4
            && data.u32(runtime - 4)? == SOURCE_CLASSES.0
            && data.u32(definition - 4)? == SOURCE_CLASSES.1
            && data.u32(runtime)? == tag
            && data.u32(definition)? == tag
            && data.u64(runtime + 8)? == definition as u64
            && data.u64(definition + 8)? == runtime as u64
            && data.u32(runtime + 16)? == SOURCE_CLASSES.1
            && data.u32(definition + 16)? == SOURCE_CLASSES.0,
        "Source Radiance bank is not a reciprocal retained-family owner"
    );
    ensure!(
        data.bytes::<16>(0x30)? == [0; 16]
            && data.bytes::<16>(runtime + 0x180)? == [0; 16]
            && [0x50, 0x90, 0xC0, 0xD0]
                .into_iter()
                .all(|field| data.bytes::<16>(definition + field).ok() == Some([0; 16])),
        "Source Radiance bank has unqualified initializers or provider arrays"
    );
    let priority_rows = data.array(runtime + 0x190, 1, Some(0x80800009))?;
    ensure!(
        !priority_rows.is_empty() && priority_rows.len() <= 256,
        "Source Radiance priority slots differ"
    );
    let priorities = priority_rows
        .iter()
        .map(|&at| data.u8(at))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        priorities.iter().all(|&value| value == 0),
        "Source Radiance priorities are not reset"
    );
    let parameter_rows = data.array(definition + 0xB0, 16, Some(0x80802122))?;
    let mut parameters = Vec::new();
    for &at in &parameter_rows {
        finite(data.u32(at + 4)?)?;
        finite(data.u32(at + 8)?)?;
        ensure!(
            data.u32(at + 12)? <= 1,
            "Source Radiance parameter operation differs"
        );
        parameters.extend(data.bytes::<16>(at)?);
    }
    let source_rows = data.array(definition + 0xA0, 40, Some(0x80802A84))?;
    ensure!(
        !source_rows.is_empty() && source_rows.len() <= 4096,
        "Source Radiance property table is empty or too large"
    );
    let mut rows = Vec::new();
    for at in source_rows {
        let body = data.pointer(at + 32)?;
        let source_class = data.u32(
            body.checked_sub(4)
                .context("Source Radiance modifier marker")?,
        )?;
        let priority = data.u32(at + 8)?;
        let slot = data.u32(at + 12)?;
        let mode = data.u32(at + 16)?;
        ensure!(
            priority <= 255
                && mode == 255
                && data.u32(at + 20)? == 0
                && data.u64(at + 24)? == 0
                && (slot as usize) < priorities.len(),
            "Source Radiance property has unqualified selection metadata"
        );
        let mut resource = None;
        let (class, native_body) = match source_class {
            0x8080243C => {
                ensure!(
                    finite(data.u32(body)?)? >= 0.0,
                    "Source active energy rate is negative"
                );
                let mut native = vec![0; 24];
                native[16..20].copy_from_slice(&data.bytes::<4>(body)?);
                (0x8080452E, native)
            }
            0x80802974 => {
                ensure!(
                    data.bytes::<16>(body)? == [0; 16],
                    "Source parameter modifier has unknown fields"
                );
                let first = usize::try_from(data.u32(body + 16)?)?;
                let count = usize::try_from(data.u32(body + 20)?)?;
                ensure!(
                    count > 0
                        && first
                            .checked_add(count)
                            .is_some_and(|end| end <= parameter_rows.len()),
                    "Source parameter modifier range leaves its bank"
                );
                let mut native = vec![0; 40];
                native[32..40].copy_from_slice(&data.bytes::<8>(body + 16)?);
                (0x80804516, native)
            }
            0x808026D1 => (
                0x80804520,
                ability_bank::constant_protection_definition(
                    f32::from_bits(data.u32(body)?),
                    finite(data.u32(body + 4)?)?,
                )
                .map_err(anyhow::Error::msg)?,
            ),
            0x80802988 | 0x808029F6 => {
                let value = data.u8(body)?;
                ensure!(
                    value != 0 && value & !3 == 0,
                    "Source Radiance flag modifier has unknown bits"
                );
                let mut native = vec![0; 24];
                native[16] = value;
                (
                    if source_class == 0x80802988 {
                        0x80804189
                    } else {
                        0x80804187
                    },
                    native,
                )
            }
            0x8080236B | 0x8080216B => {
                ensure!(
                    data.u64(body)? == 0 && data.u32(body + 12)? == 0,
                    "Source Radiance resource modifier has unknown fields"
                );
                let tag = data.u32(body + 8)?;
                ensure!(
                    !matches!(tag, 0 | u32::MAX),
                    "Source Radiance resource modifier is empty"
                );
                resource = Some(tag);
                let mut native = vec![0; 32];
                native[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
                (
                    if source_class == 0x8080236B {
                        0x80804183
                    } else {
                        0x80804181
                    },
                    native,
                )
            }
            _ => anyhow::bail!(
                "Source Radiance modifier {source_class:08X} has no complete native route"
            ),
        };
        rows.push(Property {
            source_offset: at,
            source_class,
            key: data.u32(at)?,
            name: data.u32(at + 4)?,
            priority,
            slot,
            mode,
            class,
            body: native_body,
            resource,
        });
    }
    Ok(Bank {
        source: data,
        rows,
        parameters,
        priorities,
    })
}

/// Every resource-valued property requires an explicitly translated native symbol.
/// This list covers the whole bank, including currently unselected companion rows.
pub fn resources(source: &Source, source_bank: u32) -> Result<Vec<u32>> {
    Ok(read(source, source_bank)?
        .rows
        .iter()
        .filter_map(|row| row.resource)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn block(output: &mut Vec<u8>, class: u32, bytes: &[u8]) -> Result<usize> {
    let at = output
        .len()
        .checked_add(4)
        .context("Radiance block alignment overflow")?
        .next_multiple_of(16);
    output.resize(at, 0);
    put(output, at - 4, &class.to_le_bytes())?;
    output.extend(bytes);
    Ok(at)
}

fn pointer(output: &mut [u8], at: usize, target: usize) -> Result<()> {
    let delta = i64::try_from(target)?
        .checked_sub(i64::try_from(at)?)
        .context("Radiance relative pointer overflow")?;
    put(output, at, &delta.to_le_bytes())
}

fn allocation(template: &Payload, bank: &Bank) -> Result<Payload> {
    ensure!(
        template.u64(0)? == template.0.len() as u64
            && template.u32(8)? == NO_NAME
            && template.u32(24)? == u32::MAX
            && template.u32(28)? == 0,
        "Native ability bank allocation envelope differs"
    );
    // Templates can be the common bank itself or a derived bank with empty own fields.
    // Find the common allocation by its declared arrays, rather than inheriting a donor's
    // class depth. The retained 417A bank has exactly one base, 44FC.
    let mut at = 8;
    for _ in 0..32 {
        if template.u64(at + 24)? != 0 {
            break;
        }
        ensure!(
            template.u64(at + 24)? == 0 && template.u64(at + 8)? != 0,
            "Native bank allocation inheritance differs"
        );
        at = template.pointer(at + 8)?;
        ensure!(
            at >= 4
                && template.u32(at - 4)? == 0x80808852
                && template.u32(at)? == NO_NAME
                && template.u32(at + 16)? == u32::MAX
                && template.u32(at + 20)? == 0,
            "Native bank allocation base differs"
        );
    }
    ensure!(
        template.u64(at + 8)? == 0,
        "Native bank allocation has another base"
    );
    let fields = template.array(at + 24, 40, Some(0x80808852))?;
    ensure!(
        fields.len() == 2
            && template.u32(fields[0])? == 0x656F80EA
            && template.u32(fields[0] + 16)? == 0x8080453A
            && template.u32(fields[1])? == 0x089921DA
            && template.u32(fields[1] + 16)? == 0x80800009,
        "Native bank allocation fields differ"
    );
    let mut out = Payload(template.0[..48].to_vec());
    put(&mut out.0, 16, &[0; 8])?;
    put(&mut out.0, 32, &[0; 16])?;
    let parent = block(&mut out.0, 0x80808852, &[0; 40])?;
    put(&mut out.0, parent, &NO_NAME.to_le_bytes())?;
    put(&mut out.0, parent + 16, &u32::MAX.to_le_bytes())?;
    pointer(&mut out.0, 16, parent)?;
    let rows = bank
        .rows
        .iter()
        .map(|row| {
            Allocation::named(
                NO_NAME,
                u32::MAX,
                0,
                vec![Allocation::named(0x74CB23BD, row.class, 1, Vec::new())],
            )
        })
        .collect();
    Allocation::write(
        &[
            Allocation::named(0x656F80EA, 0x8080453A, bank.rows.len(), rows),
            Allocation::named(0x089921DA, 0x80800009, bank.priorities.len(), Vec::new()),
        ],
        &mut out,
        parent + 24,
    )?;
    let size = out.0.len() as u64;
    put(&mut out.0, 0, &size.to_le_bytes())?;
    Ok(out)
}

/// Build the complete source bank on the retained Shadowkeep implementation.
/// The template supplies only the checked native envelope and common interface bindings.
#[allow(clippy::too_many_arguments)]
pub fn build(
    source: &Source,
    source_bank: u32,
    template: &Payload,
    allocation_template: &Payload,
    id: u32,
    symbol: &str,
    resources: &BTreeMap<u32, String>,
) -> Result<Built> {
    let bank = read(source, source_bank)?;
    write(
        bank,
        source_bank,
        template,
        allocation_template,
        id,
        symbol,
        resources,
    )
}

/// Prepare only complete selected source groups for a bounded ability choice. Unselected
/// choices are not advertised by this bank. Every row of every admitted key stays ordered.
#[allow(clippy::too_many_arguments)]
pub fn selected(
    source: &Source,
    source_bank: u32,
    keys: &BTreeSet<u32>,
    template: &Payload,
    allocation_template: &Payload,
    id: u32,
    symbol: &str,
    resources: &BTreeMap<u32, String>,
) -> Result<Built> {
    ensure!(
        !keys.is_empty(),
        "A selected source bank needs at least one property key"
    );
    let mut bank = read(source, source_bank)?;
    let present = bank.rows.iter().map(|row| row.key).collect::<BTreeSet<_>>();
    ensure!(
        keys.is_subset(&present),
        "A selected property key is absent from its source bank"
    );
    bank.rows.retain(|row| keys.contains(&row.key));
    write(
        bank,
        source_bank,
        template,
        allocation_template,
        id,
        symbol,
        resources,
    )
}

/// Admission for a self-revive choice requires the entire group to be the original flag and
/// positive active-energy drain. Other properties need their own integration contract.
pub(in crate::d1_roi::ability) fn revival_keys(
    source: &Source,
    source_bank: u32,
    keys: &BTreeSet<u32>,
) -> Result<bool> {
    let payload = source.read(source_bank)?;
    let instance = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    if instance < 4
        || definition < 4
        || payload.u32(instance - 4)? != SOURCE_CLASSES.0
        || payload.u32(definition - 4)? != SOURCE_CLASSES.1
    {
        return Ok(false);
    }
    let bank = read(source, source_bank)?;
    let rows = bank
        .rows
        .iter()
        .filter(|row| keys.contains(&row.key))
        .collect::<Vec<_>>();
    Ok(keys.len() == 1
        && rows.len() == 2
        && rows.iter().all(|row| row.priority == 255)
        && rows
            .iter()
            .filter(|row| row.class == 0x80804189 && row.body[16] == 1)
            .count()
            == 1
        && rows
            .iter()
            .filter(|row| {
                row.class == 0x8080452E
                    && f32::from_le_bytes(row.body[16..20].try_into().unwrap()) > 0.0
            })
            .count()
            == 1)
}

#[allow(clippy::too_many_arguments)]
fn write(
    bank: Bank,
    source_bank: u32,
    template: &Payload,
    allocation_template: &Payload,
    id: u32,
    symbol: &str,
    resources: &BTreeMap<u32, String>,
) -> Result<Built> {
    for tag in bank.rows.iter().filter_map(|row| row.resource) {
        ensure!(
            resources.get(&tag).is_some_and(|value| !value.is_empty()),
            "Source Radiance resource {tag:08X} has no translated native binding"
        );
    }
    ability_bank::validate(&template.0).map_err(anyhow::Error::msg)?;
    ensure!(
        template.bytes::<16>(0x30)? == [0; 16],
        "Native bank template has unqualified initializer rows"
    );
    let old_definition = template.pointer(24)?;
    for (field, tag) in [(0x48, 0x80BFA51Cu32), (0x60, 0x80BFA52Bu32)] {
        ensure!(
            template.pointer(old_definition + field)? == old_definition
                && template.u32(old_definition + field + 8)? == tag,
            "Native bank interface resource differs"
        );
    }
    let mut runtime = Payload(defaults::runtime_bank());
    let mut definition = Payload(defaults::definition_bank());
    ensure!(
        runtime.0.len() == 0x1B0 && definition.0.len() == 0xD0,
        "Retained bank schema size differs"
    );
    append(
        &mut runtime.0,
        0x170,
        0x8080453A,
        &vec![0; bank.rows.len() * 64],
        64,
    )?;
    let runtime_rows = runtime.pointer(0x178)? + 16;
    append(&mut runtime.0, 0x190, 0x80800009, &bank.priorities, 1)?;
    append(
        &mut definition.0,
        0x88,
        0x8080453B,
        &vec![0; bank.rows.len() * 56],
        56,
    )?;
    let definition_rows = definition.pointer(0x90)? + 16;
    append(&mut definition.0, 0x98, 0x80804519, &bank.parameters, 16)?;
    put(
        &mut definition.0,
        0xB8,
        &(bank.priorities.len() as u64).to_le_bytes(),
    )?;
    let mut modifiers = Vec::new();
    for row in &bank.rows {
        let live = block(&mut runtime.0, row.class, &[0; 40])?;
        let stored = block(&mut definition.0, row.class + 1, &row.body)?;
        modifiers.push((live, stored));
    }
    let mut owner = Owner::new(
        template,
        id,
        symbol,
        NATIVE_CLASSES,
        runtime.0,
        definition.0,
    )?;
    for (field, implementation, index) in [
        (0x30, NATIVE_CLASSES.0, 0),
        (0x48, NATIVE_CLASSES.0, 1),
        (0x60, 0x808044FC, 12),
    ] {
        owner.callback(owner.runtime + field, implementation, index, owner.runtime)?;
    }
    for (field, tag) in [(0x48, 0x80BFA51Cu32), (0x60, 0x80BFA52Bu32)] {
        pointer(
            &mut owner.payload.0,
            owner.definition + field,
            owner.definition,
        )?;
        put(
            &mut owner.payload.0,
            owner.definition + field + 8,
            &tag.to_le_bytes(),
        )?;
    }
    let mut evidence_rows = Vec::new();
    for (index, (row, (live, stored))) in bank.rows.iter().zip(modifiers).enumerate() {
        let live_row = owner.runtime + runtime_rows + index * 64;
        let stored_row = owner.definition + definition_rows + index * 56;
        let live = owner.runtime + live;
        let stored = owner.definition + stored;
        owner.pair(
            live_row,
            stored_row,
            (0x8080453A, 0x8080453B),
            Some(owner.runtime),
        )?;
        owner.pair(live, stored, (row.class, row.class + 1), Some(live_row))?;
        pointer(&mut owner.payload.0, live_row + 48, live)?;
        pointer(&mut owner.payload.0, stored_row + 48, stored)?;
        for (field, value) in [
            (16, row.key),
            (20, row.name),
            (24, row.priority),
            (28, row.slot),
            (32, row.mode),
        ] {
            put(
                &mut owner.payload.0,
                stored_row + field,
                &value.to_le_bytes(),
            )?;
        }
        if let Some(tag) = row.resource {
            owner.patches.push((stored + 24, resources[&tag].clone()));
        }
        evidence_rows.push(json!({"source_offset":row.source_offset,"source_class":row.source_class,
            "key":row.key,"name":row.name,"priority":row.priority,"priority_slot":row.slot,
            "mode":row.mode,"native_class":row.class,"native_row":stored_row,"native_modifier":stored,
            "resource":row.resource,"resource_symbol":row.resource.map(|tag| &resources[&tag])}));
    }
    let mutable_end = owner.definition;
    owner.finish(mutable_end)?;
    ability_bank::validate(&owner.payload.0).map_err(anyhow::Error::msg)?;
    let allocation = allocation(allocation_template, &bank)?;
    let evidence = json!({"source_bank":source_bank,
        "source_sha256":hex::encode(Sha256::digest(&bank.source.0)),"rows":evidence_rows,
        "parameters":hex::encode(&bank.parameters),"priority_slots":bank.priorities.len(),
        "runtime_class":NATIVE_CLASSES.0,"definition_class":NATIVE_CLASSES.1,
        "allocation_sha256":hex::encode(Sha256::digest(&allocation.0)),
        "limits":["The private bank preserves source properties, priority groups and parameter records.",
            "Symbolic resource bindings require separately translated native owners.",
            "The bank does not supply the Radiance controller, its connected scripts or a playable subclass.",
            "Package construction does not establish resurrection, native gameplay or cleanup."],
        "gameplay_verified":false});
    Ok(Built {
        payload: owner.payload,
        patches: owner.patches,
        allocation,
        runtime: owner.runtime,
        definition: owner.definition,
        evidence,
    })
}
