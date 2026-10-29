//! Private native damage profiles with explicit response ownership.
use crate::{
    presentation::put,
    tiger::{
        payload::write_array,
        projectile::{Assets, retag},
        reader::Reader,
    },
};
use anyhow::{Result, ensure};

pub(super) enum Response {
    Contact,
    Damage,
    Push(f32),
}

pub(super) fn emit(
    native: &mut Reader,
    assets: &mut Assets,
    symbol: &str,
    template: u32,
    amount: f32,
    response: Response,
) -> Result<u32> {
    ensure!(
        amount.is_finite() && amount >= 0.,
        "Invalid translated melee damage"
    );
    let id = assets.reserve(native, symbol.into())?;
    let original = native.tag(template, Some(0x80809c36))?;
    let mut p = retag(&original, id)?;
    let d = p.pointer(24)?;
    ensure!(
        p.u32(d - 4)? == 0x80804b02,
        "Melee damage template has another type"
    );
    put(&mut p.0, d + 0x10, &[0])?;
    put(&mut p.0, d + 0x53, &[0])?;
    for (field, value) in [(0x14, amount), (0x18, amount), (0x1c, 0.)] {
        put(&mut p.0, d + field, &value.to_le_bytes())?;
    }
    if let Response::Push(strength) = response {
        ensure!(
            strength.is_finite() && strength > 0.,
            "Invalid translated melee push"
        );
        let responses = p.array(d + 0x78, 8, None)?;
        let matching = responses
            .iter()
            .map(|&r| p.pointer(r))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|&r| p.u32(r.saturating_sub(4)).ok() == Some(0x80804b57))
            .collect::<Vec<_>>();
        let [response] = matching.as_slice() else {
            anyhow::bail!("Native recipient-push response is ambiguous")
        };
        for field in [4, 8] {
            put(&mut p.0, response + field, &strength.to_le_bytes())?
        }
        let class = p.u32(p.pointer(d + 0x80)? + 8)?;
        write_array(&mut p.0, d + 0x78, class, 1, &[0; 8])?;
        let row = p.array(d + 0x78, 8, Some(class))?[0];
        put(
            &mut p.0,
            row,
            &(*response as i64 - row as i64).to_le_bytes(),
        )?;
    } else if matches!(response, Response::Damage) {
        // The health pulse has no recipient-motion or ancillary Warmind responses.
        put(&mut p.0, d + 0x78, &[0; 16])?;
    }
    let length = p.0.len() as u64;
    put(&mut p.0, 0, &length.to_le_bytes())?;
    assets.push(id, template, p, None)?;
    Ok(id)
}
