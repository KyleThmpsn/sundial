//! Source hand targets and the markers they resolve against.
//!
//! Rise of Iron first-person clips bind the hands to named markers, `82E952D3` and `9697BFD2`
//! with a second pair `2E085C75` and `92F2F49E`, as Shadowkeep's do. Their unconstrained
//! keyframes are a family default. The weapon's runtime marker set carries a default row of each
//! name, and a receiver part can carry its own row, which moves the off hand onto that receiver's
//! foregrip. The converted clips keep their bindings and the translated parts keep their source
//! rows. The rows recorded here give the weapon's runtime set the source runtime set's transform
//! for every name the clips target where the two sets differ, so both marker layers hold the
//! source's values whichever of them the arm postpass reads.
use super::*;
use crate::d1_roi::native::runtime_marker::{Comparison, Sets};
use std::collections::BTreeSet;

/// The marker names the converted clips target and the runtime rows the build must carry.
/// `converted` and `appended` are the replaced and added clip records, each naming its file.
pub(super) fn prepare(
    source: &Source,
    runtime: &Value,
    native: &mut Reader,
    rig: &Value,
    graph: &Path,
    converted: &[Value],
    appended: &[Value],
) -> Result<Value> {
    let mut names = BTreeSet::new();
    for clip in converted.iter().chain(appended) {
        let file = clip["file"].as_str().context("Converted clip file")?;
        let payload = Payload(fs::read(graph.join(file))?);
        names.extend(crate::tiger::animation::constraint::targets(&payload)?);
    }
    let listed = names
        .iter()
        .map(|name| format!("{name:08X}"))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Ok(json!({"names":listed,"runtime_rows":[]}));
    }
    let Some(sets) = Sets::read(source, native, runtime, rig)? else {
        return Ok(json!({"names":listed,"runtime_rows":[],
            "limits":["A runtime entity has no marker set, so the targets keep the native rows"]}));
    };
    let mut rows = Vec::new();
    let mut limits = Vec::new();
    for &name in &names {
        let label = format!("{name:08X}");
        match sets.compare(name) {
            Comparison::Same => {}
            Comparison::Counts { source, native } => limits.push(
                json!({"name":label,"source_rows":source,"native_rows":native,
                "reason":"A target name is absent or repeated in a runtime marker set, so its native row stays"}),
            ),
            Comparison::Bindings { from, to } => limits.push(
                json!({"name":label,"source_binding":from.binding,"native_binding":to.binding,
                "reason":"The runtime rows bind different bones, so the native row stays"}),
            ),
            Comparison::Moved { from, to, moved } => rows.push(
                json!({"name":label,"orientation":from.orientation,"position":from.position,
                "native_orientation":to.orientation,"native_position":to.position,"moved":moved}),
            ),
        }
    }
    Ok(
        json!({"names":listed,"source_markers":format!("{:08X}", sets.source_owner),
        "native_markers":format!("{:08X}", sets.native_owner),"runtime_rows":rows,"limits":limits}),
    )
}
