//! Hover motion targets, acceleration limits and boost force expressions.
use super::{Sparrow, expression, values};
use crate::AuthoringResult;

pub(super) fn tune(
    payload: &mut Vec<u8>,
    owner: u32,
    roots: impl IntoIterator<Item = usize>,
    settings: &Sparrow,
) -> AuthoringResult<()> {
    for root in roots {
        if settings.speed_percent != 100 {
            for at in [0x210, 0x270] {
                expression::scale(
                    payload,
                    owner,
                    root + at,
                    f32::from(settings.speed_percent) / 100.0,
                    "Driving Speed",
                )?;
            }
        }
        values::scale_float(
            payload,
            root + 0x2D0,
            f32::from(settings.driving.acceleration_percent) / 100.0,
            "Acceleration",
        )?;
        values::scale_float(
            payload,
            root + 0x2D4,
            f32::from(settings.driving.braking_percent) / 100.0,
            "Braking",
        )?;
        if settings.driving.boost_percent != 100 {
            for at in [0x6D0, 0x810] {
                expression::scale(
                    payload,
                    owner,
                    root + at,
                    f32::from(settings.driving.boost_percent) / 100.0,
                    "Boost Strength",
                )?;
            }
        }
    }
    Ok(())
}
