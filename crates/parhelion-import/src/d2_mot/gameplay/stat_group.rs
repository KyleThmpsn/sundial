//! A stat group of the recipe's own, built from the source weapon's, so the inspect screen
//! shows the stats and numbers the source weapon shows.
use super::{limits::Shown, *};

/// Shield Duration, the modern glaive guard's stat, has no Shadowkeep definition. Guard
/// Endurance, the sword guard's duration, measures the same thing and stands in for it.
const STAND_INS: [(u32, u32); 1] = [(crate::d2_mot::glaive::SHIELD_DURATION, 0xDEBB_C6DC)];
/// Ammo Generation reuses the identity of Shadowkeep's Inventory Size, which sets reserves
/// rather than ammunition drops, so neither its value nor its display carries over.
const REPURPOSED: [u32; 1] = [0x7323_05CC];
const MAXIMUM_STATS: usize = 32;
const MAXIMUM_POINTS: usize = 64;

/// The Shadowkeep stat a source stat lands on, by hash, and whether it is a stand-in.
pub(super) fn target(hash: u32) -> Option<(u32, bool)> {
    if REPURPOSED.contains(&hash) {
        return None;
    }
    Some(
        STAND_INS
            .iter()
            .find(|(source, _)| *source == hash)
            .map_or((hash, false), |(_, native)| (*native, true)),
    )
}

fn display(stat: &Value, maximum: i32) -> Option<Vec<[i32; 2]>> {
    let points = stat["display"]
        .as_array()?
        .iter()
        .map(|point| {
            Some([
                i32::try_from(point[0].as_i64()?).ok()?,
                i32::try_from(point[1].as_i64()?).ok()?,
            ])
        })
        .collect::<Option<Vec<_>>>()?;
    // The table holds curves that rise within the group's range.
    (!points.is_empty()
        && points.len() <= MAXIMUM_POINTS
        && points
            .iter()
            .all(|[value, _]| (0..=maximum).contains(value))
        && points.windows(2).all(|pair| pair[0][0] < pair[1][0]))
    .then_some(points)
}

/// The source group's stats that have a Shadowkeep definition, in source order, each with its
/// source curve and display flags, so the investment values the source sets show the numbers
/// the source weapon shows. `None` when the source names no group or none of its stats exist
/// in Shadowkeep. The weapon's runtime still turns each value into its gameplay effect.
pub(super) fn plan(
    source: &Value,
    definitions: &BTreeMap<u32, u16>,
    fallbacks: &mut Vec<Value>,
) -> Result<Option<(i32, Vec<Shown>)>> {
    let group = &source["stat_group"];
    if group.is_null() {
        return Ok(None);
    }
    let maximum = i32::try_from(group["maximum"].as_i64().context("source group maximum")?)?;
    ensure!(maximum > 0, "source stat group has no range");
    let mut rows: Vec<Shown> = Vec::new();
    for stat in group["stats"].as_array().context("source group stats")? {
        let hash = u32::try_from(stat["hash"].as_u64().context("source group stat")?)?;
        let index = target(hash).and_then(|(native_hash, _)| definitions.get(&native_hash));
        let Some(&index) = index.filter(|index| u8::try_from(**index).is_ok()) else {
            fallbacks.push(
                json!({"stat_group_stat":hash,"reason":"no Shadowkeep stat shows this stat"}),
            );
            continue;
        };
        let Some(display) = display(stat, maximum) else {
            fallbacks.push(
                json!({"stat_group_stat":hash,"reason":"display curve does not fit a stat group"}),
            );
            continue;
        };
        if rows.iter().all(|shown| shown.index != index) {
            rows.push(Shown {
                index,
                numeric: stat["numeric"] == true,
                linear: stat["linear"] == true,
                display,
            });
        }
    }
    ensure!(
        rows.len() <= MAXIMUM_STATS,
        "source stat group shows more stats than a stat group holds"
    );
    Ok((!rows.is_empty()).then_some((maximum, rows)))
}

/// The recipe's `custom_stat_group`.
pub(super) fn recipe(maximum: i32, rows: &[Shown]) -> Value {
    json!({"maximum_value":maximum,"stats":rows.iter().map(|row| {
        let mut stat = json!({"definition_index":row.index,"display":row.display});
        if row.numeric {
            stat["display_as_numeric"] = json!(true);
        }
        if row.linear {
            stat["is_linear"] = json!(true);
        }
        stat
    }).collect::<Vec<_>>()})
}
