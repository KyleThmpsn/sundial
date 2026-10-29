//! Source firing inputs require the source table's baseline before private perks apply.
use super::*;
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
    Ok(json!({"timing":timing,
        "projectile":{"status":"native_donor","reason":"Source projectile movement, contact profiles and effect closure are not yet supported."},
        "gameplay_verified":false}))
}
