//! Source firing inputs require the source table's baseline before private perks apply.
use super::*;
mod presentation;
mod projectile;
mod timing;

pub(super) fn prepare(assembly: &mut Assembly<'_>, source: &Value, donor: u32) -> Result<Value> {
    let result = timing::prepare(assembly.source, &mut assembly.native, source, donor);
    // A refusal cannot publish an incomplete set of firing inputs. The explicit receipt
    // distinguishes the supported timing conversion from the remaining donor behavior.
    let timing = match result {
        Ok(report) => report,
        Err(error) => {
            crate::cancellation::check()?;
            json!({"status":"unsupported","reason":format!("{error:#}")})
        }
    };
    let nodes = assembly.nodes.len();
    let evidence = assembly.evidence.len();
    let symbols = assembly.symbols.clone();
    let cues = assembly.projectile_cues.clone();
    let projectile = match projectile::prepare(assembly, source, donor) {
        Ok(report) => report,
        Err(error) => {
            assembly.nodes.truncate(nodes);
            assembly.evidence.truncate(evidence);
            assembly.symbols = symbols;
            assembly.projectile_cues = cues;
            assembly.projectile_names.clear();
            crate::cancellation::check()?;
            json!({"status":"native_donor","reason":format!("{error:#}")})
        }
    };
    // The source's own firing presentation replaces the base's flows. A refusal leaves every
    // node, symbol and cue as it was and keeps the base's own presentation. Shadowkeep keeps rows
    // under many Rise of Iron content keys in the base's content owner, but those that no stock
    // weapon selects fired without a firing sound in game, so neither route applies one.
    let nodes = assembly.nodes.len();
    let symbols = assembly.symbols.clone();
    let cues = assembly.projectile_cues.clone();
    let presentation = match presentation::prepare(assembly, source) {
        Ok(report) => report,
        Err(error) => {
            assembly.nodes.truncate(nodes);
            assembly.symbols = symbols;
            assembly.projectile_cues = cues;
            crate::cancellation::check()?;
            json!({"status":"native","reason":format!("{error:#}")})
        }
    };
    Ok(json!({"timing":timing,
        "projectile":projectile,
        "presentation":presentation,
        "gameplay_verified":false}))
}
