//! Source identities for literal scenario and tag arguments. These are not target handles.
use super::{Cache, Program, Tag, vocabulary};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Default, Serialize)]
pub struct Bindings {
    pub named: Vec<Named>,
    pub tags: Vec<Asset>,
}

#[derive(Debug, Serialize)]
pub struct Named {
    pub expression: u32,
    pub value_type: u16,
    pub value: u32,
    pub domain: &'static str,
    pub index: u32,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Asset {
    pub expression: u32,
    pub value_type: u16,
    pub value: u32,
    pub tag: Option<Tag>,
}

/// Block offset, stride, name offset, and ASCII width (zero means string ID).
fn context(value_type: u16) -> Option<(&'static str, usize, usize, usize, usize)> {
    Some(match value_type {
        13 => ("trigger_volume", 0x280, 0x7C, 0, 0),
        14 => ("cutscene_flag", 0x46C, 0x20, 4, 0),
        15 => ("cutscene_camera_point", 0x478, 0x40, 4, 32),
        16 => ("cutscene_title", 0x484, 0x30, 0, 0),
        18 => ("device_group", 0x174, 0x2C, 0, 32),
        25 => ("starting_profile", 0x268, 0x68, 0, 32),
        28 => ("zone_set", 0xAC, 0x13C, 0, 0),
        29 => ("designer_zone", 0x740, 0xBC, 0, 0),
        78..=84 => ("object_name", 0xF0, 8, 0, 0),
        _ => return None,
    })
}

pub(super) fn read(cache: &Cache, program: &Program) -> Result<Bindings> {
    let at = program.scenario.address()?;
    let mut names = BTreeMap::<usize, Vec<String>>::new();
    let mut result = Bindings::default();
    for node in program
        .nodes
        .iter()
        .flatten()
        .filter(|n| n.kind() == 9 && n.value_type != 2 && n.value != u32::MAX)
    {
        let value_type = node.literal_type();
        if let Some((domain, offset, stride, name_offset, width)) = context(value_type) {
            let table = match names.entry(offset) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let table = cache
                        .block(at + offset, stride)?
                        .into_iter()
                        .map(|row| {
                            if width == 0 {
                                cache.string_id(cache.u32(row + name_offset)?)
                            } else {
                                cache.string(row + name_offset, width)
                            }
                        })
                        .collect::<Result<Vec<_>>>()?;
                    entry.insert(table)
                }
            };
            let index = if value_type == 18 {
                node.value
            } else {
                node.value & 0xFFFF
            };
            result.named.push(Named {
                expression: node.datum,
                value_type,
                value: node.value,
                domain,
                index,
                name: table.get(index as usize).cloned(),
            });
        } else if vocabulary::get()
            .types
            .get(&value_type)
            .is_some_and(|ty| ty.tag.is_some())
        {
            let tag = cache
                .tags
                .get((node.value & 0xFFFF) as usize)
                .and_then(Option::as_ref)
                .filter(|tag| tag.datum == node.value)
                .cloned();
            result.tags.push(Asset {
                expression: node.datum,
                value_type,
                value: node.value,
                tag,
            });
        }
    }
    Ok(result)
}
