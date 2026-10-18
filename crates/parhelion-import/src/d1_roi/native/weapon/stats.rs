//! Source base stats as the recipe's own investment values and display group.
//!
//! Rise of Iron and Shadowkeep keep weapon investment values on the same 0 to 100 scale and
//! name the same stat hashes, so a source row carries straight to the Shadowkeep definition
//! of its hash. The native runtime's family translation turns each carried value into its
//! gameplay effect. The source display group carries as a group of the recipe's own, so the
//! inspect screen shows the stats and numbers the source weapon showed.
use super::*;
use crate::d2_mot::gameplay::{limits::Shown, stat_definitions, stat_group};
use std::collections::BTreeSet;

pub(super) struct Translated {
    /// Recipe `investment_stats` rows.
    pub stats: Vec<Value>,
    /// Recipe `custom_stat_group`, when the source names a display group.
    pub group: Option<Value>,
    /// The graph's `stat_translation` report.
    pub report: Value,
}

fn hash(row: &Value) -> Result<u32> {
    Ok(u32::try_from(
        row["hash"].as_u64().context("Source stat hash")?,
    )?)
}

pub(super) fn translate(assembly: &mut Assembly<'_>, source: &Value) -> Result<Translated> {
    let definitions = stat_definitions(&mut assembly.native)?;
    let mut stats = Vec::new();
    let mut carried = Vec::new();
    let mut omitted = Vec::new();
    let mut indices = BTreeSet::new();
    for row in source["stats"]["rows"]
        .as_array()
        .context("Source base stats")?
    {
        let hash = hash(row)?;
        let value = i32::try_from(row["value"].as_i64().context("Source stat value")?)?;
        let Some(&index) = definitions.get(&hash) else {
            omitted.push(
                json!({"source_index":row["index"],"source_hash":hash,"value":value,
                "reason":"no Shadowkeep stat definition has this hash"}),
            );
            continue;
        };
        ensure!(
            u8::try_from(index).is_ok(),
            "Source stat {hash:08X} exceeds the item stat row index"
        );
        ensure!(indices.insert(index), "Source stat {hash:08X} is set twice");
        stats.push(json!({"definition_index":index,"value":value}));
        carried.push(json!({"source_index":row["index"],"source_hash":hash,
            "definition_index":index,"value":value}));
    }
    ensure!(
        !stats.is_empty(),
        "No source base stat has a Shadowkeep definition"
    );
    let mut shown: Vec<Shown> = Vec::new();
    let mut unshown = Vec::new();
    let group = &source["stats"]["group"];
    let group = if group.is_null() {
        None
    } else {
        let maximum = i32::try_from(group["maximum"].as_i64().context("Source group maximum")?)?;
        ensure!(maximum > 0, "Source stat group has no range");
        for stat in group["stats"].as_array().context("Source group stats")? {
            let hash = hash(stat)?;
            let Some(&index) = definitions.get(&hash).filter(|i| u8::try_from(**i).is_ok()) else {
                unshown.push(json!({"source_hash":hash,"reason":"no Shadowkeep stat definition has this hash"}));
                continue;
            };
            let display = stat_group::display(stat, maximum).with_context(|| {
                format!("Source stat {hash:08X} display curve does not fit a stat group")
            })?;
            ensure!(
                shown.iter().all(|row| row.index != index),
                "Source stat group shows {hash:08X} twice"
            );
            shown.push(Shown {
                index,
                numeric: stat["numeric"] == true,
                linear: false,
                display,
            });
        }
        ensure!(
            !shown.is_empty(),
            "Source stat group shows no Shadowkeep stat"
        );
        for row in &stats {
            let value = row["value"].as_i64().context("Carried stat value")?;
            ensure!(
                (0..=i64::from(maximum)).contains(&value),
                "Source stat value {value} is outside the display group's range"
            );
        }
        Some(stat_group::recipe(maximum, &shown))
    };
    let report = json!({"carried":carried,"omitted":omitted,
        "display_group":group.as_ref().map(|_| json!({"source_hash":source["stats"]["group"]["hash"],
            "source_index":source["stats"]["group"]["index"],"unshown":unshown})),
        "scale":"Source and Shadowkeep investment values share the 0 to 100 scale and the stat hashes. Rate of Fire carries as Rounds Per Minute, which the source shows as a bar and Shadowkeep shows as a number. The native runtime's family translation decides the firing rate, magazine, reserves and handling each value produces.",
        "gameplay_verified":false});
    Ok(Translated {
        stats,
        group,
        report,
    })
}
