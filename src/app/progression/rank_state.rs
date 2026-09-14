use super::*;

pub(in crate::app) fn progression_target(definition: &ProgressionDefinition) -> Option<i32> {
    if definition.steps.is_empty() {
        return None;
    }
    definition.steps.iter().try_fold(0_i32, |total, step| {
        (step.cost >= 0).then_some(())?;
        total.checked_add(step.cost)
    })
}

pub(in crate::app) fn saved_progression_lanes(
    document: &Value,
    scope: ProgressionScope,
    definition_index: usize,
) -> Option<[i32; 3]> {
    let key = match scope {
        ProgressionScope::Account => "account_progressions",
        ProgressionScope::Character => "character_progressions",
        ProgressionScope::Unreplicated => return None,
    };
    let rows = document
        .pointer(&format!("/state/unlocks/{key}"))?
        .as_array()?;
    let mut saved = None::<[i32; 3]>;
    for row in rows {
        let Some(values) = row.as_array() else {
            continue;
        };
        let [index, lane_0, lane_1, lane_2] = values.as_slice() else {
            continue;
        };
        if index.as_u64().and_then(|index| usize::try_from(index).ok()) != Some(definition_index) {
            continue;
        }
        let lanes = [lane_0, lane_1, lane_2]
            .map(|lane| lane.as_i64().and_then(|lane| i32::try_from(lane).ok()));
        let [Some(lane_0), Some(lane_1), Some(lane_2)] = lanes else {
            continue;
        };
        let lanes = [lane_0, lane_1, lane_2];
        if let Some(current) = saved.as_mut() {
            for lane in 0..3 {
                current[lane] = current[lane].max(lanes[lane]);
            }
        } else {
            saved = Some(lanes);
        }
    }
    saved
}
