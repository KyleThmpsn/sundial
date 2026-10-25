//! Imported base numeric inputs on the private native stat translator's selected table.
use super::rig::{
    TRANSLATOR_KEY_CLASS, TRANSLATOR_KEY_HASH, TRANSLATOR_KEY_SIZE, TRANSLATOR_TABLE_CLASS,
    TRANSLATOR_TABLE_SIZE, translator_array,
};
use crate::{
    AuthoringResult,
    error::invalid,
    item::WeaponRuntimeResourcePatch,
    tag_payload::{array_at, read_u32},
};
use serde::Deserialize;
use sundial::package_authoring::{
    PackageManager,
    entity::{WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, weapon_component_bindings},
};
use tiger_pkg::TagHash;
mod logic;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    component: u32,
    input: u32,
    value: f32,
}

fn literal(value: f32) -> Vec<u8> {
    let mut operand = vec![0; 20];
    operand[16..].copy_from_slice(&value.to_le_bytes());
    operand
}

fn fixed_refill(
    owner: &[u8],
    routes: &[usize],
    value: &Input,
    values: &[Input],
) -> AuthoringResult<bool> {
    if (value.component, value.input) != (1, 3)
        || !values
            .iter()
            .any(|v| (v.component, v.input) == (1, 0) && v.value == value.value)
    {
        return Ok(false);
    }
    let mut fixed = false;
    for &at in routes {
        // A paired source capacity supplies a full-magazine refill candidate. A native
        // literal quantum belongs to its reload mechanism, including per-shell reloads.
        if read_u32(owner, at + 12)? != 4
            || read_u32(owner, at + 16)? != 0
            || read_u32(owner, at + 28)? != 0
            || read_u32(owner, at + 32)? == 0x7F7FFFFF
        {
            return Ok(false);
        }
        let quantum = f32::from_bits(read_u32(owner, at + 32)?);
        if !quantum.is_finite() {
            return Err(invalid("Native refill quantum is nonfinite"));
        }
        fixed = true;
    }
    Ok(fixed)
}

impl Input {
    fn validate(&self) -> AuthoringResult<()> {
        let valid = match (self.component, self.input) {
            (1, 0 | 3) => (1. ..=500.).contains(&self.value) && self.value.fract() == 0.,
            (2, 0..=3) => (0.05..=100.).contains(&self.value),
            _ => false,
        };
        if !valid {
            return Err(invalid("Unsupported imported base numeric input"));
        }
        Ok(())
    }
}

pub(crate) fn imported(graph: &serde_json::Value) -> AuthoringResult<Vec<Input>> {
    let Some(values) = graph.pointer("/source_gameplay/inputs") else {
        return Ok(Vec::new());
    };
    if graph["source"] != "marathon" || graph["source_gameplay"]["source"] != "marathon" {
        return Err(invalid("Numeric input translation has no supported source"));
    }
    let values: Vec<Input> = serde_json::from_value(values.clone())
        .map_err(|e| invalid(format!("Imported numeric inputs: {e}")))?;
    let mut seen = std::collections::BTreeSet::new();
    for value in &values {
        value.validate()?;
        if !seen.insert((value.component, value.input)) {
            return Err(invalid("Duplicate imported numeric input"));
        }
    }
    Ok(values)
}

/// E83C40 returns a routing operand's literal instead of sampling when it is not Float32 MAX.
/// Requested writers retain other consumers of their shared curves. A paired full-magazine
/// refill candidate preserves a native literal quantum, including per-shell reloads. Native
/// component modifiers still operate on these base values. Investment bonuses do not resample
/// the imported baseline, and donor trigger/state selection remains a separate contract.
pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
    values: &[Input],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let bindings =
        weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid("Imported stats require one native stat translator"));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|e| invalid(e.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Stat translator offset overflow"))?;
    let keys = translator_array(&owner, TRANSLATOR_KEY_CLASS)?;
    let tables = translator_array(&owner, TRANSLATOR_TABLE_CLASS)?;
    if keys.0 == 0 || keys.0 != tables.0 {
        return Err(invalid("Imported stat table keys differ"));
    }
    let mut selected = None;
    for i in 0..keys.0 {
        if read_u32(
            &owner,
            keys.1 + i * TRANSLATOR_KEY_SIZE + TRANSLATOR_KEY_HASH,
        )? == group
            && selected.replace(i).is_some()
        {
            return Err(invalid("Imported stats have duplicate translation keys"));
        }
    }
    let table = tables.1 + selected.unwrap_or(0) * TRANSLATOR_TABLE_SIZE;
    let (count, _, rows, class) = array_at(&owner, table + 16)?;
    if class != 0x80803981 {
        return Err(invalid("Imported stat route layout differs"));
    }
    let (handled, mut writes) = logic::patches(&owner, table, values)?;
    for value in values {
        value.validate()?;
        let mut found = Vec::new();
        for at in (0..count).map(|i| rows + i * 80) {
            if read_u32(&owner, at)? == value.component && read_u32(&owner, at + 4)? == value.input
            {
                found.push(at);
            }
        }
        if fixed_refill(&owner, &found, value, values)? {
            continue;
        }
        for &at in &found {
            // Several native rows can write the same input in sequence or for different
            // slots. Give every writer the same base value and retain its destination and order.
            writes.push((at + 12, 4u32.to_le_bytes().to_vec()));
            writes.push((at + 16, literal(value.value)));
        }
        if found.is_empty() && !handled.contains(&(value.component, value.input)) {
            return Err(invalid(format!(
                "Native carrier has no component {} input {} route",
                value.component, value.input
            )));
        }
    }
    let mut unique = std::collections::BTreeMap::new();
    for (at, bytes) in writes {
        if let Some(prior) = unique.insert(at, bytes.clone())
            && prior != bytes
        {
            return Err(invalid(
                "Imported numeric inputs share conflicting operands",
            ));
        }
    }
    unique
        .into_iter()
        .map(|(cell, bytes)| {
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
                resource_index: 0,
                offset: u32::try_from(
                    cell.checked_sub(resource)
                        .ok_or_else(|| invalid("Imported stat route precedes its resource"))?,
                )
                .map_err(|_| invalid("Imported stat patch offset overflow"))?,
                bytes,
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            })
        })
        .collect()
}
