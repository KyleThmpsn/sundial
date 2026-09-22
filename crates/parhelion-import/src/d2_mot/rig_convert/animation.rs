//! Read animation-to-skeleton connections into a version-independent form.
//! Clip payloads and controller programs still need their own format conversion.
use super::*;
use serde::Serialize;
use std::collections::BTreeMap;
pub mod clips;
pub mod first_person;

/// Export clip dependencies through validated lookup-component and bank fields.
/// This deliberately avoids scanning arbitrary integers for apparent tag hashes.
pub fn export_clips(
    r: &mut crate::d2_mot::reader::Reader,
    rig: &Value,
    modern: bool,
) -> Result<Value> {
    let mut banks = Vec::new();
    let mut exported = BTreeMap::new();
    for component in rig["components"]
        .as_array()
        .context("animation components")?
    {
        let class = if modern { "808025F8" } else { "8080344B" };
        if component["class"] != class {
            continue;
        }
        let owner =
            u32::from_str_radix(component["owner"].as_str().context("animation owner")?, 16)?;
        let p = r.tag(owner, Some(if modern { 0x80809B06 } else { 0x80809C36 }))?;
        let bank_tag = p.u32(p.pointer(24)? + if modern { 0xA8 } else { 0x90 })?;
        let bank = r.tag(bank_tag, Some(if modern { 0x8080289F } else { 0x808036F6 }))?;
        let mut members = Vec::new();
        for row in bank.array(
            8,
            if modern { 16 } else { 4 },
            Some(if modern { 0x80808BDF } else { 0x80808F48 }),
        )? {
            let tag = if modern {
                r.ref64(&bank, row)?
            } else {
                bank.u32(row)?
            };
            let clip = r.tag(tag, Some(if modern { 0x80808BE0 } else { 0x80808F49 }))?;
            ensure!(
                clip.0.len() >= 0x190 && clip.u64(0)? == clip.0.len() as u64,
                "animation clip payload size differs"
            );
            let name = format!("{tag:08X}");
            let report = if modern {
                match clips::convert(&clip.0) {
                    Ok((_, report)) => report,
                    Err(error) => {
                        json!({"runtime_ready":false,"conversion_error":format!("{error:#}")})
                    }
                }
            } else {
                json!({"native":true})
            };
            exported.insert(name.clone(), report);
            members.push(name);
        }
        banks.push(
            json!({"owner":component["owner"],"bank":format!("{bank_tag:08X}"),"clips":members}),
        );
    }
    let report = json!({"banks":banks,"clips":exported,"runtime_ready":false});
    write_json(&r.output.join("animation-clips.json"), &report)?;
    r.finish()?;
    Ok(report)
}

#[derive(Clone, Debug, Serialize)]
struct Endpoint {
    owner: String,
    interface: String,
    offset: u64,
    component: usize,
}

#[derive(Clone, Debug, Serialize)]
struct Connection {
    input: Option<Endpoint>,
    output: Option<Endpoint>,
    channel: u32,
}

fn connections(entity: &Payload, modern: bool) -> Result<Vec<Connection>> {
    let components = entity.array(
        if modern { 8 } else { 16 },
        12,
        Some(if modern { 0x80809ACD } else { 0x80809C04 }),
    )?;
    let (descriptor, stride, class) = if modern {
        (0x18, 56, 0x80809A8F)
    } else {
        (0x20, 72, 0x80809BC9)
    };
    let mut result = Vec::new();
    for row in entity.array(descriptor, stride, Some(class))? {
        let endpoint = |output: bool| -> Result<Option<Endpoint>> {
            let (tag_offset, index_offset) = match (modern, output) {
                (true, false) => (0, 20),
                (true, true) => (24, 44),
                (false, false) => (8, 24),
                (false, true) => (40, 56),
            };
            let index = if modern {
                u64::from(entity.u32(row + index_offset)?)
            } else {
                entity.u64(row + index_offset)?
            };
            let owner = entity.u32(row + tag_offset)?;
            let interface = entity.u32(row + tag_offset + 4)?;
            let offset = entity.u64(row + tag_offset + 8)?;
            if index == 0xFFFF {
                ensure!(
                    owner == u32::MAX && interface == u32::MAX && offset == 0,
                    "disconnected animation endpoint contains a live reference"
                );
                return Ok(None);
            }
            let component = usize::try_from(index)?;
            let at = *components
                .get(component)
                .context("animation connection component ordinal")?;
            ensure!(
                entity.u32(at)? == owner,
                "animation connection component identity differs"
            );
            Ok(Some(Endpoint {
                owner: format!("{owner:08X}"),
                interface: format!("{interface:08X}"),
                offset,
                component,
            }))
        };
        result.push(Connection {
            input: endpoint(false)?,
            output: endpoint(true)?,
            channel: entity.u32(row + if modern { 48 } else { 64 })?,
        });
    }
    Ok(result)
}

/// Preserve typed driver bindings for every weapon and first-person skeleton.
/// This is an analysis/export boundary, not a claim that compressed clips can run.
pub fn inspect(root: &Path, modern: bool) -> Result<Value> {
    let rig = load(&root.join("rig.json"))?;
    let skeletons = rig["skeletons"].as_array().context("animation skeletons")?;
    let components = rig["components"]
        .as_array()
        .context("animation components")?;
    let owners = components
        .iter()
        .map(|c| {
            Ok((
                c["owner"].as_str().context("component owner")?.to_owned(),
                c,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut entities = BTreeSet::new();
    for component in components {
        entities.insert(component["entity"].as_str().context("component entity")?);
    }
    let mut bindings = Vec::new();
    for entity_tag in entities {
        for connection in connections(&raw(root, entity_tag)?, modern)? {
            if !skeletons.iter().any(|s| {
                [&connection.input, &connection.output]
                    .into_iter()
                    .flatten()
                    .any(|endpoint| s["owner"] == endpoint.owner)
            }) {
                continue;
            }
            for endpoint in [&connection.input, &connection.output]
                .into_iter()
                .flatten()
            {
                ensure!(
                    owners.contains_key(&endpoint.owner),
                    "animation endpoint owner was not exported"
                );
                let data = raw(root, &endpoint.owner)?;
                ensure!(
                    endpoint.offset < data.0.len() as u64,
                    "animation endpoint outside component payload"
                );
            }
            bindings.push(json!({"entity":entity_tag,"connection":connection}));
        }
    }
    Ok(
        json!({"schema":1,"item_tag":rig["item_tag"],"skeletons":skeletons,"bindings":bindings,
        "clip_conversion_complete":false,"runtime_ready":false,
        "remaining":["decode clip tracks and event tables","convert controller programs and attachment bindings","link first-person and weapon skeletons into the native runtime"]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_entity_layouts_preserve_binding_identity_and_reject_wrong_ordinals() {
        check_connections(true);
        check_connections(false);
    }

    fn check_connections(modern: bool) {
        let (descriptor, stride, class, index, left, right) = if modern {
            (0x18, 56, 0x80809A8F, 44, 0, 24)
        } else {
            (0x20, 72, 0x80809BC9, 56, 8, 40)
        };
        let mut data = vec![0; 0x100];
        let mut components = vec![0; 24];
        components[..4].copy_from_slice(&11u32.to_le_bytes());
        components[12..16].copy_from_slice(&22u32.to_le_bytes());
        write_array(
            &mut data,
            if modern { 8 } else { 16 },
            if modern { 0x80809ACD } else { 0x80809C04 },
            2,
            &components,
        )
        .unwrap();
        let mut row = vec![0; stride];
        for (at, owner, interface, offset) in [(left, 11u32, 100u32, 64u64), (right, 22, 200, 128)]
        {
            row[at..at + 4].copy_from_slice(&owner.to_le_bytes());
            row[at + 4..at + 8].copy_from_slice(&interface.to_le_bytes());
            row[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        }
        row[index..index + 4].copy_from_slice(&1u32.to_le_bytes());
        write_array(&mut data, descriptor, class, 1, &row).unwrap();
        let mut payload = Payload(data);
        let found = connections(&payload, modern).unwrap();
        assert_eq!(found[0].input.as_ref().unwrap().owner, "0000000B");
        assert_eq!(found[0].output.as_ref().unwrap().owner, "00000016");
        assert_eq!(found[0].output.as_ref().unwrap().offset, 128);
        let at = payload.array(descriptor, row.len(), None).unwrap()[0];
        payload.0[at + index..at + index + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(connections(&payload, modern).is_err());
        payload.0[at + index..at + index + 4].copy_from_slice(&0xFFFFu32.to_le_bytes());
        let tag = at + right;
        payload.0[tag..tag + 8].fill(0xFF);
        payload.0[tag + 8..tag + 16].fill(0);
        assert!(connections(&payload, modern).unwrap()[0].output.is_none());
        payload.0[tag] = 0;
        assert!(connections(&payload, modern).is_err());
    }
}
