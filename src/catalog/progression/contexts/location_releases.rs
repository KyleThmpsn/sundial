use super::*;

pub(in crate::catalog::progression) fn attach_location_definition_release_contexts(
    locations: &[LocationContext],
    activities: &[ActivityContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    for location in locations {
        for release in &location.releases {
            let activity = match release.activity_index {
                Some(index) => Some(activities.get(index).ok_or_else(|| {
                    format!("Location release has out-of-range activity index {index}")
                })?),
                None => None,
            };
            attach_condition_context(
                flag_definitions,
                value_definitions,
                &release.references,
                &location_release_progression_context(location, activity),
            );
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::catalog::progression) struct LocationReleaseConditionRow {
    pub(in crate::catalog::progression) location_index: usize,
    pub(in crate::catalog::progression) references: ConditionReferences,
    pub(in crate::catalog::progression) activity_index: Option<usize>,
}

pub(in crate::catalog::progression) fn location_release_condition_row_at(
    data: &[u8],
    row: usize,
) -> Result<LocationReleaseConditionRow, String> {
    let location_offset = row
        .checked_add(LOCATION_RELEASE_LOCATION_INDEX_OFFSET)
        .ok_or("Location-release location offset overflowed")?;
    let condition_offset = row
        .checked_add(LOCATION_RELEASE_CONDITIONS_OFFSET)
        .ok_or("Location-release condition offset overflowed")?;
    let activity_offset = row
        .checked_add(LOCATION_RELEASE_CONDITION_ACTIVITY_INDEX_OFFSET)
        .ok_or("Location-release activity offset overflowed")?;
    let location_index = usize::try_from(u32_at(data, location_offset)?)
        .map_err(|_| "Location-release location index is too large")?;
    let activity_index = usize::from(u16_at(data, activity_offset)?);
    Ok(LocationReleaseConditionRow {
        location_index,
        references: condition_references_at(data, condition_offset)?,
        activity_index: (activity_index != usize::from(u16::MAX)).then_some(activity_index),
    })
}

pub(in crate::catalog::progression) fn location_release_activity(
    activities: &[ActivityContext],
    activity_index: Option<usize>,
    row_index: usize,
) -> Result<Option<&ActivityContext>, String> {
    activity_index
        .map(|activity_index| {
            activities.get(activity_index).ok_or_else(|| {
                format!(
                    "Location-release condition row {row_index} has out-of-range activity index {activity_index}"
                )
            })
        })
        .transpose()
}

pub(in crate::catalog::progression) fn attach_location_release_condition_contexts(
    manager: &PackageManager,
    root: &[u8],
    locations: &[LocationContext],
    activities: &[ActivityContext],
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<(), String> {
    let data = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + LOCATION_RELEASE_CONDITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read location-release conditions: {error}"))?;
    let (count, rows, row_class) = array_at(&data, 8)?;
    if row_class != LOCATION_RELEASE_CONDITION_ROW_CLASS {
        return Err(format!(
            "The installed location-release condition table has unexpected row class 0x{row_class:08X}"
        ));
    }
    for index in 0..count {
        let row = rows
            .checked_add(
                index
                    .checked_mul(LOCATION_RELEASE_CONDITION_ROW_SIZE)
                    .ok_or("Location-release condition row offset overflowed")?,
            )
            .ok_or("Location-release condition row offset overflowed")?;
        let release = location_release_condition_row_at(&data, row)?;
        let Some(location) = locations.get(release.location_index) else {
            return Err(format!(
                "Location-release condition row {index} has an out-of-range location index"
            ));
        };
        let activity = location_release_activity(activities, release.activity_index, index)?;
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &release.references,
            &location_release_progression_context(location, activity),
        );
    }
    Ok(())
}

fn location_release_progression_context(
    location: &LocationContext,
    activity: Option<&ActivityContext>,
) -> ProgressionContextDef {
    let activity = activity.filter(|activity| !activity.name.trim().is_empty());
    ProgressionContextDef {
        direct_references: Vec::new(),
        hash: activity.map_or(location.hash, |activity| activity.hash),
        kind: ProgressionContextKind::LocationRelease,
        name: activity.map_or_else(|| location.name.clone(), |activity| activity.name.clone()),
        type_name: String::new(),
        description: activity.map_or_else(
            || location.description.clone(),
            |activity| activity.description.clone(),
        ),
        paths: if activity.is_some() && !location.name.trim().is_empty() {
            vec![vec![location.name.clone()]]
        } else {
            Vec::new()
        },
        condition_programs: Vec::new(),
    }
}
