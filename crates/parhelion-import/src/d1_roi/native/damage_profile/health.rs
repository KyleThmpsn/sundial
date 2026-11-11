//! Rise of Iron healing profiles that profile events apply become private native healing profiles.
//!
//! The family is the zero-damage one the direct health effect already translates.
//! [`perk::direct::healing`](crate::d1_roi::native::perk::direct::healing) checks every source
//! field, and stock profile `8162C86E` supplies the native envelope, its two health categories and
//! its allocation. The two response operands and both flags take source values.
use super::*;
use crate::d1_roi::native::perk::direct;
use sundial::package_authoring::ability_damage;

pub(super) fn translate(
    source: &Source,
    native: &mut Reader,
    tag: u32,
    prefix: &str,
) -> Result<Translated> {
    let (health, flags) = direct::healing(source, native, tag)?;
    let template = direct::HEALING_TEMPLATE;
    let mut payload = (*native.tag(template, Some(0x80809C36))?).clone();
    let shape =
        ability_damage::healing_profile(template, &payload.0).map_err(anyhow::Error::msg)?;
    ability_damage::set_health_response(&mut payload.0, shape, health.mode, health.amounts())
        .map_err(anyhow::Error::msg)?;
    // The record at `22C` takes the source mode and flags, which both builds read the same way.
    ensure!(
        payload.u8(0x22C)? == health.mode,
        "Native healing response moved"
    );
    put(&mut payload.0, 0x22D, &flags)?;
    let symbol = format!("{prefix}-profile-{tag:08X}");
    let mut patches = Vec::new();
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        if payload.u32(at)? == template {
            put(&mut payload.0, at, &u32::MAX.to_le_bytes())?;
            patches.push(Patch {
                offset: at,
                symbol: symbol.clone(),
                class: 0x80809C36,
            });
        }
    }
    ensure!(
        !patches.is_empty(),
        "Native healing profile names no owner identity"
    );
    source.unchanged()?;
    Ok(Translated {
        assets: vec![Asset {
            symbol,
            class: 0x80809C36,
            payload,
            patches,
        }],
        cues: BTreeSet::new(),
        unqualified: [0, 0],
        absent: Vec::new(),
        silent: Vec::new(),
    })
}
