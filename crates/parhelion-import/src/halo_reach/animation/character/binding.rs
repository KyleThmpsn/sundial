//! Publish private character parameters from the owning weapon or driver.
use super::*;
use crate::presentation::put;
use std::collections::BTreeSet;
use sundial::package_authoring::entity::weapon_component_bindings;

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("Native identity")?,
        16,
    )?)
}

fn append_patches(existing: &mut Vec<Value>, patches: Vec<Value>) -> Result<()> {
    for patch in patches {
        let matching = existing
            .iter()
            .filter(|old| {
                old["binding_hash"] == patch["binding_hash"]
                    && old["resource_index"] == patch["resource_index"]
                    && old["offset"] == patch["offset"]
            })
            .collect::<Vec<_>>();
        ensure!(
            matching.len() <= 1 && matching.first().is_none_or(|old| **old == patch),
            "Character profile overlaps a different existing resource patch"
        );
        if matching.is_empty() {
            existing.push(patch);
        }
    }
    Ok(())
}

pub(super) fn prepare(
    native: &mut Reader,
    entry: &ImportRequest,
    graph: &mut Value,
    recipe: &mut Value,
    root: &Path,
    private: u32,
) -> Result<Value> {
    if recipe["kind"].as_str().unwrap_or("weapon") == "weapon" {
        let donor = crate::graph::hash(&recipe["donor"], "item_hash")?;
        let item = crate::tiger::shadowkeep::extract(native, donor)?;
        let report = crate::tiger::rig::inspect(native, hex(&item["item_tag"])?, false)?;
        let runtime = hex(&report["runtime_entity"])?;
        let content = hex(&report["content_key"])?;
        let entity = native.tag(runtime, Some(0x80809C0F))?;
        let bindings =
            weapon_component_bindings(&entity.0, 0xD3A5500E).map_err(anyhow::Error::msg)?;
        ensure!(
            bindings.len() == 1,
            "Character attachment binding is ambiguous"
        );
        let binding = &bindings[0];
        let owner = native.tag(binding.owner_tag, Some(0x80809C36))?;
        let definition = owner.pointer(24)?;
        ensure!(
            owner.u32(definition - 4)? == 0x80804221,
            "Character attachment has an unsupported class"
        );
        let rows = crate::tiger::rig::attachment_rows(&owner, definition, content, false)?;
        // The attachment profile also selects first-person actions. A weapon with a
        // first-person entity needs the private profile in that dictionary as an alias.
        let mut first_person = BTreeSet::new();
        for &row in &rows {
            let entity = owner.u32(row + 0x78)?;
            if entity != u32::MAX && entity != 0 {
                first_person.insert(entity);
            }
        }
        ensure!(
            first_person.len() <= 1,
            "Character attachment rows select different first-person entities"
        );
        let first_person_parameters = first_person
            .first()
            .map(|&entity| -> Result<u32> {
                let lookups = report["components"]
                    .as_array()
                    .context("Native weapon components")?
                    .iter()
                    .filter(|c| c["class"] == "8080344B" && hex(&c["entity"]).ok() == Some(entity))
                    .map(|c| hex(&c["owner"]))
                    .collect::<Result<BTreeSet<_>>>()?
                    .into_iter()
                    .collect::<Vec<_>>();
                let [lookup] = lookups.as_slice() else {
                    anyhow::bail!("First-person lookup is absent or ambiguous")
                };
                let lookup = native.tag(*lookup, Some(0x80809C36))?;
                let at = lookup.pointer(24)?;
                ensure!(
                    lookup.u32(at - 4)? == 0x8080344B,
                    "First-person lookup has an unsupported class"
                );
                lookup.u32(at + 0x94)
            })
            .transpose()?;
        let mut parent = None;
        let mut patches = Vec::new();
        for row in rows {
            let before = owner.u32(row + 0x6C)?;
            ensure!(
                parent.is_none_or(|parent| parent == before),
                "Character attachment profiles disagree"
            );
            parent = Some(before);
            let offset = (row + 0x6C)
                .checked_sub(usize::try_from(binding.resource_offset)?)
                .context("Character attachment precedes its resource")?;
            patches.push(json!({"binding_hash":format!("0x{:08X}", binding.binding_hash),"resource_index":binding.resource_index,
                "offset":offset,"bytes":private.to_le_bytes().iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ")}));
        }
        let parent = parent.context("Character attachment profile is absent")?;
        let overrides = recipe["overrides"]
            .as_object_mut()
            .context("Recipe overrides are absent")?;
        let existing = overrides
            .entry("runtime_resource_patches")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context("Recipe resource patches are not an array")?;
        append_patches(existing, patches)?;
        Ok(
            json!({"builtin":3,"parent":parent,"private":private,"runtime":runtime,"alias":true,
            "owner":binding.owner_tag,"content":content,"first_person_parameters":first_person_parameters}),
        )
    } else {
        ensure!(
            entry.runtime_entity.is_some() && recipe["kind"] == "sparrow",
            "Character actions require a weapon or driver"
        );
        let nodes = graph["nodes"].as_array().context("Vehicle graph nodes")?;
        let mut selected = Vec::new();
        // The driver's seat sets the drive control bits 1 to 3 of its flags at `+0xE0`. A mounted
        // turret's seat in the same graph leaves bit 1 clear.
        for node in nodes {
            let Some(file) = node["file"].as_str() else {
                continue;
            };
            let payload = Payload(fs::read(root.join(file))?);
            if let Ok(at) = payload.pointer(24)
                && payload.u32(at.saturating_sub(4)).ok() == Some(0x80803D7D)
                && payload.u32(at + 0xE0)? & 0x0E == 0x0E
            {
                selected.push((file.to_owned(), payload, at));
            }
        }
        ensure!(
            selected.len() == 1,
            "Private vehicle driver is absent or ambiguous"
        );
        let (file, mut payload, at) = selected.pop().unwrap();
        let override_name = payload.u32(at + 0xEC)?;
        let parent = if override_name == 0x811C9DC5 {
            payload.u32(at + 0xE8)?
        } else {
            override_name
        };
        put(&mut payload.0, at + 0xEC, &private.to_le_bytes())?;
        fs::write(root.join(&file), &payload.0)?;
        Ok(json!({"builtin":2,"parent":parent,"private":private,"file":file}))
    }
}
