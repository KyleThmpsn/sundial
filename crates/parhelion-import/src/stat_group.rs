//! A recipe's own stat group: the native donor's group read from the installed investment
//! tables, the bounds it puts on investment values, and the `custom_stat_group` a recipe carries.
//! Every importer builds the same recipe shape from its source weapon's stats.
pub(crate) mod limits;

use crate::tiger::reader::Reader;
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub(crate) use limits::Shown;

/// The most points a stat's display curve can hold.
const MAXIMUM_POINTS: usize = 64;

/// A source stat's display curve, when it fits a group whose largest investment value is
/// `maximum`: no more than [`MAXIMUM_POINTS`] points, rising within the group's range.
pub(crate) fn display(stat: &Value, maximum: i32) -> Option<Vec<[i32; 2]>> {
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

/// The recipe's `custom_stat_group`.
pub(crate) fn recipe(maximum: i32, rows: &[Shown]) -> Value {
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

/// The native donor's complete display group for importers that replace selected unit-bearing
/// outputs while retaining its other display curves and flags.
pub(crate) fn native(r: &mut Reader, donor: u32) -> Result<Value> {
    let tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("Native investment globals")?
        .hash
        .0;
    let globals = r.tag(tag, None)?;
    let group = limits::read(r, &globals, donor, &Value::Null)?;
    Ok(group
        .maximum
        .map_or(Value::Null, |maximum| recipe(maximum, &group.shown)))
}
