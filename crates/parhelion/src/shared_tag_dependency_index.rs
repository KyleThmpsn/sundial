//! Dependency indexes consumed by native type-16 roots. Groups may encode entry indices as
//! either a dense u32 bitmap or a sparse u16 list; both are loading dependencies, not names.
use std::collections::{BTreeMap, BTreeSet};

use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::{invalid, validation},
    tag_payload::{read_u16, read_u32, read_u64, relative_target, write_i64, write_u64},
};

const MARKER: u32 = 0x8080_9FBD;
const GROUP_CLASS: u64 = 0x8080_9EFB;
const BITMAP_CLASS: u64 = 0x8080_000B;
const INDEX_CLASS: u64 = 0x8080_000A;
const GROUP_START: usize = 0x50;
const GROUP_SIZE: usize = 0x28;

#[derive(Clone, Debug, Default)]
struct Group {
    bitmap: Vec<u32>,
    indices: Vec<u16>,
}

fn array(
    payload: &[u8],
    descriptor: usize,
    class: u64,
    stride: usize,
    max: usize,
) -> AuthoringResult<Vec<u8>> {
    let count = usize::try_from(read_u64(payload, descriptor)?)
        .map_err(|_| invalid("Dependency array count overflow"))?;
    let relative = read_u64(payload, descriptor + 8)?;
    if count == 0 {
        if relative != 0 {
            return Err(invalid("Empty dependency array has a non-null pointer"));
        }
        return Ok(Vec::new());
    }
    if count > max {
        return Err(invalid(
            "Dependency array exceeds its native entry-index limit",
        ));
    }
    let header = relative_target(payload, descriptor + 8)?;
    if header < 4
        || read_u32(payload, header - 4)? != MARKER
        || read_u64(payload, header)? != count as u64
        || read_u64(payload, header + 8)? != class
    {
        return Err(invalid(
            "Dependency array marker, class, or repeated count is invalid",
        ));
    }
    let start = header
        .checked_add(16)
        .ok_or_else(|| invalid("Dependency array offset overflow"))?;
    let end = start
        .checked_add(count * stride)
        .ok_or_else(|| invalid("Dependency array size overflow"))?;
    payload
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| invalid("Dependency array exits payload"))
}

fn parse(
    payload: &[u8],
    companion: TagHash,
    owner: TagHash,
) -> AuthoringResult<BTreeMap<u16, Group>> {
    if payload.len() < GROUP_START
        || read_u64(payload, 0)? != payload.len() as u64
        || read_u32(payload, 8)? != companion.0
        || read_u32(payload, 12)? != owner.0
        || payload[0x20..0x3C].iter().any(|b| *b != 0)
    {
        return Err(invalid(
            "Shared dependency index identity or fixed envelope is invalid",
        ));
    }
    let rows = array(payload, 0x10, GROUP_CLASS, GROUP_SIZE, 4096)?;
    if rows.is_empty() || relative_target(payload, 0x18)? != 0x40 {
        return Err(invalid(
            "Shared dependency index has no canonical package-group table",
        ));
    }
    let mut groups = BTreeMap::new();
    let mut previous = None;
    for index in 0..rows.len() / GROUP_SIZE {
        let row = GROUP_START + index * GROUP_SIZE;
        let package = u16::try_from(read_u64(payload, row)?)
            .map_err(|_| invalid("Dependency package id exceeds u16"))?;
        // Stock indexes retain groups outside the installed/header-authoring window (for
        // example 0x0E06..0x0EC0). Preserve those encoded dependencies; do not prune them.
        if package > 0x1FFF || previous.is_some_and(|p| package <= p) {
            return Err(invalid(
                "Dependency package groups are invalid or not strictly sorted",
            ));
        }
        previous = Some(package);
        let bitmap = array(payload, row + 8, BITMAP_CLASS, 4, 256)?
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("word")))
            .collect::<Vec<_>>();
        let sparse = array(payload, row + 0x18, INDEX_CLASS, 2, 8192)?;
        let indices = (0..sparse.len() / 2)
            .map(|i| read_u16(&sparse, i * 2))
            .collect::<AuthoringResult<Vec<_>>>()?;
        if indices.iter().any(|i| *i >= 8192)
            || !indices.windows(2).all(|w| w[0] < w[1])
            || indices.iter().any(|i| {
                bitmap
                    .get(*i as usize / 32)
                    .is_some_and(|word| word & (1 << (*i % 32)) != 0)
            })
        {
            return Err(invalid(
                "Dependency indices are invalid, duplicated, or unsorted",
            ));
        }
        groups.insert(package, Group { bitmap, indices });
    }
    // Prove every array, padding byte and relative pointer was understood before editing.
    if encode(payload, &groups)? != payload {
        return Err(invalid(
            "Dependency index is not in the supported native layout",
        ));
    }
    Ok(groups)
}

fn append_array(
    payload: &mut Vec<u8>,
    descriptor: usize,
    class: u64,
    count: usize,
    bytes: &[u8],
) -> AuthoringResult<()> {
    if count == 0 {
        return Ok(());
    }
    let start = payload
        .len()
        .checked_add(0x23)
        .ok_or_else(|| invalid("Dependency alignment overflow"))?
        & !0xF;
    payload.resize(start - 0x14, 0);
    payload.extend_from_slice(&MARKER.to_le_bytes());
    payload.extend_from_slice(&(count as u64).to_le_bytes());
    payload.extend_from_slice(&class.to_le_bytes());
    payload.extend_from_slice(bytes);
    write_u64(payload, descriptor, count as u64)?;
    write_i64(
        payload,
        descriptor + 8,
        (start - 16) as i64 - (descriptor + 8) as i64,
    )?;
    Ok(())
}

fn encode(template: &[u8], groups: &BTreeMap<u16, Group>) -> AuthoringResult<Vec<u8>> {
    let mut payload = template[..0x30].to_vec();
    write_u64(&mut payload, 0x10, groups.len() as u64)?;
    write_i64(&mut payload, 0x18, 0x28)?;
    payload.resize(0x3C, 0);
    payload.extend_from_slice(&MARKER.to_le_bytes());
    payload.extend_from_slice(&(groups.len() as u64).to_le_bytes());
    payload.extend_from_slice(&GROUP_CLASS.to_le_bytes());
    payload.resize(GROUP_START + groups.len() * GROUP_SIZE, 0);
    for (index, (package, group)) in groups.iter().enumerate() {
        let row = GROUP_START + index * GROUP_SIZE;
        write_u64(&mut payload, row, *package as u64)?;
        let bitmap = group
            .bitmap
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        let sparse = group
            .indices
            .iter()
            .flat_map(|entry| entry.to_le_bytes())
            .collect::<Vec<_>>();
        append_array(
            &mut payload,
            row + 8,
            BITMAP_CLASS,
            group.bitmap.len(),
            &bitmap,
        )?;
        append_array(
            &mut payload,
            row + 0x18,
            INDEX_CLASS,
            group.indices.len(),
            &sparse,
        )?;
    }
    let size = payload.len() as u64;
    write_u64(&mut payload, 0, size)?;
    Ok(payload)
}

/// Reads both native dependency representations, retaining no pointers into package storage.
#[cfg(test)]
pub(crate) fn dependencies(
    payload: &[u8],
    companion: TagHash,
    owner: TagHash,
) -> AuthoringResult<BTreeSet<u32>> {
    let groups = parse(payload, companion, owner)?;
    Ok(dependency_set(&groups))
}

#[cfg(test)]
fn dependency_set(groups: &BTreeMap<u16, Group>) -> BTreeSet<u32> {
    let mut tags = BTreeSet::new();
    for (package, group) in groups {
        for (word_index, word) in group.bitmap.iter().enumerate() {
            for bit in 0..32 {
                if word & (1 << bit) != 0 {
                    tags.insert(TagHash::new(*package, (word_index * 32 + bit) as u16).0);
                }
            }
        }
        tags.extend(
            group
                .indices
                .iter()
                .map(|index| TagHash::new(*package, *index).0),
        );
    }
    tags
}

/// Extends a rooted loading index without removing or reinterpreting any stock dependency.
pub(crate) fn enroll_dependencies(
    payload: &[u8],
    companion: TagHash,
    owner: TagHash,
    additions: &[TagHash],
) -> AuthoringResult<Vec<u8>> {
    enroll_inherited_dependencies(payload, companion, owner, additions, &[])
}

fn indexed_dependencies(groups: &BTreeMap<u16, Group>) -> BTreeSet<(u16, u16)> {
    let mut entries = BTreeSet::new();
    for (&package, group) in groups {
        for (word_index, word) in group.bitmap.iter().enumerate() {
            for bit in 0..32 {
                if word & (1 << bit) != 0 {
                    entries.insert((package, (word_index * 32 + bit) as u16));
                }
            }
        }
        entries.extend(group.indices.iter().map(|&entry| (package, entry)));
    }
    entries
}

/// Copies validated native dependency groups without forcing their package IDs
/// through the narrower authored-tag encoding window.
pub(crate) fn enroll_inherited_dependencies(
    payload: &[u8],
    companion: TagHash,
    owner: TagHash,
    additions: &[TagHash],
    sources: &[(&[u8], TagHash, TagHash)],
) -> AuthoringResult<Vec<u8>> {
    let mut groups = parse(payload, companion, owner)?;
    let mut expected = indexed_dependencies(&groups);
    let mut incoming = BTreeSet::new();
    for &(source, source_companion, source_owner) in sources {
        incoming.extend(indexed_dependencies(&parse(
            source,
            source_companion,
            source_owner,
        )?));
    }
    for tag in additions {
        if !(0x100..=0xCFF).contains(&tag.pkg_id())
            || TagHash::new(tag.pkg_id(), tag.entry_index()) != *tag
        {
            return Err(invalid(
                "Authored dependency is not a canonical package tag",
            ));
        }
        incoming.insert((tag.pkg_id(), tag.entry_index()));
    }
    for (package, entry) in incoming {
        if !expected.insert((package, entry)) {
            continue;
        }
        let group = groups.entry(package).or_default();
        if group.bitmap.is_empty() {
            let position = group.indices.binary_search(&entry).unwrap_or_else(|p| p);
            group.indices.insert(position, entry);
        } else {
            group
                .bitmap
                .resize(group.bitmap.len().max(entry as usize / 32 + 1), 0);
            group.bitmap[entry as usize / 32] |= 1 << (entry % 32);
        }
    }
    let authored = encode(payload, &groups)?;
    if indexed_dependencies(&parse(&authored, companion, owner)?) != expected {
        return Err(validation(
            "Authored loading dependencies did not round-trip",
        ));
    }
    Ok(authored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag_payload::write_u32;
    const OWNER: TagHash = TagHash(0x80EC3F62);
    const COMPANION: TagHash = TagHash(0x80EE8CBD);

    fn fixture() -> Vec<u8> {
        let mut prefix = vec![0; GROUP_START];
        write_u32(&mut prefix, 8, COMPANION.0).unwrap();
        write_u32(&mut prefix, 12, OWNER.0).unwrap();
        encode(
            &prefix,
            &BTreeMap::from([
                (
                    0x1BB,
                    Group {
                        bitmap: vec![0x80000001],
                        indices: vec![],
                    },
                ),
                (
                    0x361,
                    Group {
                        bitmap: vec![],
                        indices: vec![OWNER.entry_index()],
                    },
                ),
                (
                    0x374,
                    Group {
                        bitmap: vec![],
                        indices: vec![COMPANION.entry_index()],
                    },
                ),
            ]),
        )
        .unwrap()
    }

    #[test]
    fn preserves_dense_and_sparse_dependencies_and_extends_word_and_package_boundaries() {
        let source = fixture();
        let before = source.clone();
        let original = dependencies(&source, COMPANION, OWNER).unwrap();
        let additions = [
            TagHash::new(0x1BB, 32),
            TagHash::new(0x1BB, 8191),
            TagHash::new(0xAA0, 1),
            TagHash::new(0x361, 7),
        ];
        let result = enroll_dependencies(&source, COMPANION, OWNER, &additions).unwrap();
        let expected = original.into_iter().chain(additions.map(|t| t.0)).collect();
        assert_eq!(dependencies(&result, COMPANION, OWNER).unwrap(), expected);
        assert_eq!(source, before);
        assert_eq!(
            enroll_dependencies(&result, COMPANION, OWNER, &additions).unwrap(),
            result
        );
    }

    #[test]
    fn rejects_bad_identity_pointer_count_and_noncanonical_alias_tags() {
        let source = fixture();
        for offset in [0, 8, 12, 0x18, 0x58, 0x60] {
            let mut bad = source.clone();
            bad[offset] ^= 0xFF;
            assert!(
                dependencies(&bad, COMPANION, OWNER).is_err(),
                "offset {offset:X}"
            );
        }
        assert!(enroll_dependencies(&source, COMPANION, OWNER, &[TagHash(0x00B77945)]).is_err());
    }

    #[test]
    fn inherited_groups_preserve_native_ids_outside_the_authoring_window() {
        let source = fixture();
        let mut groups = parse(&source, COMPANION, OWNER).unwrap();
        groups.insert(
            0xE06,
            Group {
                bitmap: vec![3],
                indices: vec![8191],
            },
        );
        groups.insert(
            0x1BB,
            Group {
                bitmap: vec![2],
                indices: vec![32],
            },
        );
        let native = encode(&source, &groups).unwrap();
        let additions = [TagHash::new(0xAA0, 4)];
        let result = enroll_inherited_dependencies(
            &source,
            COMPANION,
            OWNER,
            &additions,
            &[(&native, COMPANION, OWNER)],
        )
        .unwrap();
        let actual = indexed_dependencies(&parse(&result, COMPANION, OWNER).unwrap());
        let expected = indexed_dependencies(&groups)
            .into_iter()
            .chain(indexed_dependencies(
                &parse(&source, COMPANION, OWNER).unwrap(),
            ))
            .chain([(0xAA0, 4)])
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(
            enroll_inherited_dependencies(
                &result,
                COMPANION,
                OWNER,
                &additions,
                &[(&native, COMPANION, OWNER)],
            )
            .unwrap(),
            result
        );
        assert!(enroll_dependencies(&source, COMPANION, OWNER, &[TagHash::new(0xE06, 0)]).is_err());
    }

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
    fn real_investment_dependency_index_round_trips_and_enrolls_private_action() {
        let dir = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("stock packages");
        let manager =
            sundial::package_authoring::open_shadowkeep_package_manager(std::path::Path::new(&dir))
                .unwrap();
        let source = manager.read_tag(COMPANION).unwrap();
        let stock = dependencies(&source, COMPANION, OWNER).unwrap();
        assert!(
            stock.contains(&0x80BC2BBD),
            "stock Micro-Missile must be a native loading dependency"
        );
        assert!(!stock.contains(&0x80B77945));
        assert_eq!(
            enroll_dependencies(&source, COMPANION, OWNER, &[]).unwrap(),
            source
        );
        let result =
            enroll_dependencies(&source, COMPANION, OWNER, &[TagHash(0x80B77945)]).unwrap();
        assert_eq!(
            dependencies(&result, COMPANION, OWNER).unwrap(),
            stock.into_iter().chain([0x80B77945]).collect()
        );
    }
}
