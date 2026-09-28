//! The selected attachment's animation category and profile are name hashes.
//! Carry a source profile only when the native first-person parameter dictionary
//! recognizes it. Clip substitution alone leaves the donor's profile selected.
use crate::d2_mot::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Patch {
    pub content_key: u32,
    pub category: u32,
    pub before: u32,
    pub after: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_category: Option<u32>,
}

fn selected(owner: &Payload, content: u32, modern: bool) -> Result<usize> {
    let resource = owner.pointer(24)?;
    let class = if modern { 0x8080356E } else { 0x80804221 };
    ensure!(
        owner.u32(resource - 4)? == class,
        "animation attachment layout differs"
    );
    let rows = crate::d2_mot::rig::attachment_rows(owner, resource, content, modern)?;
    ensure!(
        rows.len() == 1,
        "selected animation attachment is missing or ambiguous"
    );
    Ok(rows[0])
}

fn key(rig: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        rig["content_key"]
            .as_str()
            .context("animation content key")?,
        16,
    )?)
}

pub(super) fn prepare(
    source: &mut Reader,
    native: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    attachments: &Payload,
    lookup: &Payload,
) -> Result<Patch> {
    let patch = requested(source, source_rig, native_rig, attachments)?;
    let table = native.tag(lookup.u32(lookup.pointer(24)? + 0x94)?, Some(0x80808EE1))?;
    recognized(&table, &patch)?;
    Ok(patch)
}

pub(super) fn requested(
    source: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    attachments: &Payload,
) -> Result<Patch> {
    requested_with_category(source, source_rig, native_rig, attachments, false)
}

pub(super) fn requested_with_category(
    source: &mut Reader,
    source_rig: &Value,
    native_rig: &Value,
    attachments: &Payload,
    source_owned: bool,
) -> Result<Patch> {
    let owners = source_rig["components"]
        .as_array()
        .context("source animation components")?
        .iter()
        .filter(|c| c["entity"] == source_rig["runtime_entity"] && c["class"] == "8080356E")
        .collect::<Vec<_>>();
    ensure!(
        owners.len() == 1,
        "source animation attachment owner is ambiguous"
    );
    let tag = u32::from_str_radix(owners[0]["owner"].as_str().context("attachment owner")?, 16)?;
    let owner = source.tag(tag, Some(0x80809B06))?;
    let source_row = selected(&owner, key(source_rig)?, true)?;
    let content_key = key(native_rig)?;
    let native_row = selected(attachments, content_key, false)?;
    let category = owner.u32(source_row + 0x90)?;
    ensure!(
        source_owned || category == attachments.u32(native_row + 0x68)?,
        "animation categories differ"
    );
    let after = owner.u32(source_row + 0x94)?;
    let before = attachments.u32(native_row + 0x6C)?;
    Ok(Patch {
        content_key,
        category,
        before,
        after,
        previous_category: source_owned
            .then(|| attachments.u32(native_row + 0x68))
            .transpose()?,
    })
}

pub(super) fn recognized(table: &Payload, patch: &Patch) -> Result<()> {
    let groups = table.array(8, 32, Some(0x80808EE5))?;
    ensure!(
        !groups.is_empty(),
        "native animation parameter dictionary is empty"
    );
    let mut recognized = false;
    for group in groups {
        let names = table
            .array(group, 8, Some(0x80808EE9))?
            .into_iter()
            .map(|at| table.u32(at))
            .collect::<Result<Vec<_>>>()?;
        recognized |= names.contains(&patch.category) && names.contains(&patch.after);
    }
    ensure!(
        recognized,
        "source animation profile is absent from the native parameter dictionary"
    );
    Ok(())
}

/// Apply only to the same selected row and original profile used in preparation.
pub fn apply(owner: &mut Payload, patch: &Patch) -> Result<()> {
    let row = selected(owner, patch.content_key, false)?;
    ensure!(
        owner.u32(row + 0x68)? == patch.previous_category.unwrap_or(patch.category)
            && owner.u32(row + 0x6C)? == patch.before,
        "animation attachment profile changed since preparation"
    );
    owner.0[row + 0x6C..row + 0x70].copy_from_slice(&patch.after.to_le_bytes());
    owner.0[row + 0x68..row + 0x6C].copy_from_slice(&patch.category.to_le_bytes());
    Ok(())
}
