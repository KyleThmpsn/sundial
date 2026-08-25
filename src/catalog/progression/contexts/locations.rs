use super::*;

#[derive(Clone, Debug)]
pub(in crate::catalog) struct LocationContext {
    pub(in crate::catalog::progression) hash: u64,
    pub(in crate::catalog::progression) name: String,
    pub(in crate::catalog::progression) description: String,
    pub(in crate::catalog::progression) releases: Vec<LocationDefinitionRelease>,
}

#[derive(Clone, Debug)]
pub(in crate::catalog::progression) struct LocationDefinitionRelease {
    pub(in crate::catalog::progression) activity_index: Option<usize>,
    pub(in crate::catalog::progression) references: ConditionReferences,
}

pub(in crate::catalog::progression) fn location_definition_release_at(
    definitions: &[u8],
    release: usize,
) -> Result<LocationDefinitionRelease, String> {
    let activity_offset = release
        .checked_add(LOCATION_RELEASE_ACTIVITY_INDEX_OFFSET)
        .ok_or("Location release activity offset overflowed")?;
    let activity_index = usize::from(u16_at(definitions, activity_offset)?);
    Ok(LocationDefinitionRelease {
        activity_index: (activity_index != usize::from(u16::MAX)).then_some(activity_index),
        references: condition_references_at(definitions, release)?,
    })
}

pub(in crate::catalog) fn scan_location_condition_contexts(
    package: &mut ProgressionPackageData<'_>,
) -> Result<Vec<LocationContext>, String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + LOCATION_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read location definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + LOCATION_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read location strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, string_class) = array_at(&strings, 8)?;
    if definition_class != LOCATION_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed location table has unexpected row class 0x{definition_class:08X}"
        ));
    }
    if string_class != LOCATION_STRING_ROW_CLASS {
        return Err(format!(
            "The installed location-string table has unexpected row class 0x{string_class:08X}"
        ));
    }
    if definition_count != string_count {
        return Err("The installed location definition and string tables do not match".into());
    }

    let mut locations = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows
            .checked_add(
                index
                    .checked_mul(LOCATION_DEFINITION_ROW_SIZE)
                    .ok_or("Location definition row offset overflowed")?,
            )
            .ok_or("Location definition row offset overflowed")?;
        let string = string_rows
            .checked_add(
                index
                    .checked_mul(LOCATION_STRING_ROW_SIZE)
                    .ok_or("Location string row offset overflowed")?,
            )
            .ok_or("Location string row offset overflowed")?;
        let hash = u32_at(&definitions, definition)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Location definition and string row {index} do not match"
            ));
        }

        let mut name = String::new();
        let mut description = String::new();
        if u64_at(&strings, string + LOCATION_DISPLAY_LIST_OFFSET)? != 0 {
            let (display_count, display_rows, display_class) =
                array_at(&strings, string + LOCATION_DISPLAY_LIST_OFFSET)?;
            if display_class != LOCATION_DISPLAY_ROW_CLASS {
                return Err(format!("Location {index} has an unexpected display list"));
            }
            for display_index in 0..display_count {
                let display = display_rows
                    .checked_add(
                        display_index
                            .checked_mul(LOCATION_DISPLAY_ROW_SIZE)
                            .ok_or("Location display row offset overflowed")?,
                    )
                    .ok_or("Location display row offset overflowed")?;
                if name.trim().is_empty() {
                    name = resolve_string(
                        manager,
                        localized_tags,
                        localized_cache,
                        &strings,
                        display + 0x04,
                    )
                    .unwrap_or_default();
                }
                if description.trim().is_empty() {
                    description = resolve_string(
                        manager,
                        localized_tags,
                        localized_cache,
                        &strings,
                        display + 0x0C,
                    )
                    .unwrap_or_default();
                }
            }
        }

        let mut releases = Vec::new();
        if u64_at(&definitions, definition + LOCATION_RELEASE_LIST_OFFSET)? != 0 {
            let (release_count, release_rows, release_class) =
                array_at(&definitions, definition + LOCATION_RELEASE_LIST_OFFSET)?;
            if release_class != LOCATION_RELEASE_ROW_CLASS {
                return Err(format!("Location {index} has an unexpected release list"));
            }
            for release_index in 0..release_count {
                let release = release_rows
                    .checked_add(
                        release_index
                            .checked_mul(LOCATION_RELEASE_ROW_SIZE)
                            .ok_or("Location release row offset overflowed")?,
                    )
                    .ok_or("Location release row offset overflowed")?;
                releases.push(location_definition_release_at(&definitions, release)?);
            }
        }
        let context = LocationContext {
            hash: u64::from(hash),
            name,
            description,
            releases,
        };
        locations.push(context);
    }
    Ok(locations)
}
