//! Include borrowed native shaders and textures in the material owner's scope.
use super::*;

fn material_references(data: &[u8]) -> AuthoringResult<BTreeSet<TagHash>> {
    let mut references = BTreeSet::new();
    for stage in [0x48, 0xE8, 0x188, 0x228, 0x2C8, 0x368] {
        references.insert(TagHash(read_u32(data, stage)?));
        references.insert(TagHash(read_u32(data, stage + 0x84)?));
        for (descriptor, stride, field) in [(stage + 8, 8, 4), (stage + 0x40, 16, 0)] {
            if crate::tag_payload::read_u64(data, descriptor)? == 0 {
                continue;
            }
            let (count, _, rows, _) = crate::tag_payload::array_at(data, descriptor)?;
            let end = count
                .checked_mul(stride)
                .and_then(|size| rows.checked_add(size))
                .filter(|&end| end <= data.len())
                .ok_or_else(|| invalid("Imported material resource array is truncated"))?;
            for row in (rows..end).step_by(stride) {
                references.insert(TagHash(read_u32(data, row + field)?));
            }
        }
    }
    references.retain(|tag| ![0, u32::MAX, 0x811C9DC5].contains(&tag.0));
    Ok(references)
}

pub(super) fn include_native_materials(
    manager: &sundial::package_authoring::PackageManager,
    folder: &Path,
    nodes: &[Value],
    symbols: &BTreeMap<String, TagHash>,
    groups: &mut BTreeMap<TagHash, Vec<TagHash>>,
) -> AuthoringResult<()> {
    let mut materials = BTreeMap::new();
    for node in nodes {
        let template = node["template"]
            .as_u64()
            .and_then(|tag| u32::try_from(tag).ok())
            .ok_or_else(|| invalid("Imported material template is missing"))?;
        if manager
            .get_entry(TagHash(template))
            .is_none_or(|entry| entry.reference != 0x808071E8)
        {
            continue;
        }
        let path = node["file"]
            .as_str()
            .ok_or_else(|| invalid("Imported material payload is missing"))?;
        let data = fs::read(folder.join(path)).map_err(|error| invalid(error.to_string()))?;
        let mut required = material_references(&data)?;
        for tag in required.clone() {
            let entry = manager.get_entry(tag).ok_or_else(|| {
                invalid(format!(
                    "Imported material requires missing native resource {tag}"
                ))
            })?;
            let backing = TagHash(entry.reference);
            if manager.get_entry(backing).is_some() {
                required.insert(backing);
            }
            if entry.file_type == 32 && entry.file_subtype == 2 {
                let header = manager
                    .read_tag(tag)
                    .map_err(|error| invalid(error.to_string()))?;
                if let Some(large) = streamed_texture_payload(&header)? {
                    if manager.get_entry(large).is_none() {
                        return Err(invalid(format!(
                            "Imported texture {tag} requires missing streamed payload {large}"
                        )));
                    }
                    required.insert(large);
                }
            }
        }
        let name = node["symbol"]
            .as_str()
            .ok_or_else(|| invalid("Imported material symbol is missing"))?;
        materials.insert(
            *symbols
                .get(name)
                .ok_or_else(|| invalid("Imported material was not allocated"))?,
            required,
        );
    }
    for group in groups.values_mut() {
        let mut complete: BTreeSet<_> = group.iter().copied().collect();
        for material in group.iter() {
            if let Some(required) = materials.get(material) {
                complete.extend(required);
            }
        }
        *group = complete.into_iter().collect();
    }
    Ok(())
}

fn streamed_texture_payload(header: &[u8]) -> AuthoringResult<Option<TagHash>> {
    let tag = read_u32(header, 36)?;
    Ok((![0, u32::MAX, 0x811C9DC5].contains(&tag)).then_some(TagHash(tag)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_native_textures_keep_their_large_payload() {
        let mut header = vec![0; 40];
        assert_eq!(streamed_texture_payload(&header).unwrap(), None);
        write_u32(&mut header, 36, 0x80C1B782).unwrap();
        assert_eq!(
            streamed_texture_payload(&header).unwrap(),
            Some(TagHash(0x80C1B782))
        );
        write_u32(&mut header, 36, u32::MAX).unwrap();
        assert_eq!(streamed_texture_payload(&header).unwrap(), None);
        assert!(streamed_texture_payload(&header[..39]).is_err());
    }

    #[test]
    fn borrowed_shader_references_survive_but_unresolved_private_slots_do_not() {
        let mut data = vec![0; 0x410];
        write_u32(&mut data, 0x48, u32::MAX).unwrap();
        write_u32(&mut data, 0xE8, 0x8161ECD2).unwrap();
        write_u32(&mut data, 0xE8 + 0x84, 0x811C9DC5).unwrap();
        assert_eq!(
            material_references(&data).unwrap(),
            BTreeSet::from([TagHash(0x8161ECD2)])
        );
        data.truncate(0x100);
        assert!(material_references(&data).is_err());
    }
}
