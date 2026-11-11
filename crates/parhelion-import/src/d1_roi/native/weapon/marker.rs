//! Source runtime marker rows the build writes over the weapon runtime's own.
//!
//! A weapon runtime's marker set holds family defaults, and a part's marker set can hold its own
//! row of a name. Casings start at `fx_ejection` (`9216EEA9`): Rise of Iron firing presentations
//! (`80800610`) and stock firing flows both bind locators to it, and the examined Rise of Iron
//! weapons keep that row in their runtime set alone. The converted weapon keeps the native runtime
//! and its firing flow, so its casings start at the donor family's ejection port unless the source
//! runtime row replaces the native one. Hand targets have their own rows, recorded with the
//! converted clips that bind them.
use super::*;
use crate::d1_roi::native::runtime_marker::{Comparison, Sets};
use crate::tiger::markers;

/// The runtime rows the build writes, each with the native row it replaces, and the names whose
/// native row stays with the reason.
pub(super) fn runtime_rows(
    assembly: &mut Assembly<'_>,
    source_runtime: &Value,
    native_rig: &Value,
) -> Result<Value> {
    let Some(sets) = Sets::read(
        assembly.source,
        &mut assembly.native,
        source_runtime,
        native_rig,
    )?
    else {
        return Ok(json!({"rows":[],
            "limits":["A runtime entity has no marker set, so the native runtime rows stay"]}));
    };
    let mut rows = Vec::new();
    let mut limits = Vec::new();
    let label = format!("{:08X}", markers::EJECTION);
    match sets.compare(markers::EJECTION) {
        Comparison::Same => {}
        Comparison::Counts { source, native } => limits.push(
            json!({"name":label,"source_rows":source,"native_rows":native,
            "reason":"The name is absent or repeated in a runtime marker set, so its native row stays"}),
        ),
        Comparison::Bindings { from, to } => limits.push(
            json!({"name":label,"source_binding":from.binding,"native_binding":to.binding,
            "reason":"The runtime rows bind different bones, so the native row stays"}),
        ),
        Comparison::Moved { from, to, moved } => rows.push(
            json!({"name":label,"role":"casing ejection",
            "orientation":from.orientation,"position":from.position,
            "native_orientation":to.orientation,"native_position":to.position,"moved":moved}),
        ),
    }
    Ok(
        json!({"source_markers":format!("{:08X}", sets.source_owner),
        "native_markers":format!("{:08X}", sets.native_owner),"rows":rows,"limits":limits}),
    )
}
