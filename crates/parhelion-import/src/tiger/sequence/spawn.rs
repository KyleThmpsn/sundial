//! Native kind-15 object creation and its qualified launch parameters.
use super::*;

pub(crate) struct Parameters {
    pub locator: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 2],
    pub speed: [f32; 2],
    pub cone: f32,
    pub angular_min: [f32; 3],
    pub angular_max: [f32; 3],
}

fn floats(payload: &mut Payload, at: usize, values: &[f32]) -> Result<()> {
    ensure!(
        values.iter().all(|value| value.is_finite()),
        "Nonfinite object creation parameter"
    );
    for (index, value) in values.iter().enumerate() {
        put(&mut payload.0, at + index * 4, &value.to_le_bytes())?;
    }
    Ok(())
}

/// Emit one native polar spread sample. The radial exponent is one half and
/// both angular strata are fully jittered. This preserves a cone bound, not a
/// claim that another engine uses the same random distribution or seed stream.
pub(crate) fn spread(payload: &mut Payload, field: usize, cone: f32) -> Result<()> {
    ensure!(
        cone.is_finite() && (0.0..=std::f32::consts::PI).contains(&cone),
        "Invalid launch cone"
    );
    if cone == 0. {
        return put(&mut payload.0, field, &[0; 8]);
    }
    let class = 0x80808891u32;
    let at = object(payload, class, 0x60)?;
    let owner = payload.u32(payload.pointer(16)?)?;
    // Three registered interfaces share this definition. Their object references
    // must name the current owner and the newly allocated object offset.
    for ordinal in 0..3 {
        let row = at + ordinal * 24;
        put(&mut payload.0, row, &class.to_le_bytes())?;
        put(
            &mut payload.0,
            row + 4,
            &u32::try_from(ordinal)?.to_le_bytes(),
        )?;
        put(&mut payload.0, row + 8, &owner.to_le_bytes())?;
        put(&mut payload.0, row + 12, &class.to_le_bytes())?;
        put(&mut payload.0, row + 16, &u64::try_from(at)?.to_le_bytes())?;
    }
    put(&mut payload.0, at + 0x48, &[1, 0, 1, 0])?;
    floats(payload, at + 0x4c, &[0., cone, 0.5, 1.])?;
    relative(payload, field, at)
}

/// Return the entity-reference field for the caller's private resource patch.
pub(crate) fn write(payload: &mut Payload, event: usize, parameters: &Parameters) -> Result<usize> {
    ensure!(
        payload.u32(event - 4)? == 0x80808881 && payload.u32(event + 0x30)? == 15,
        "Object creation event class differs"
    );
    let definition = payload.pointer(24)?;
    let locators = payload.array(definition + 0x1b0, 6, Some(0x80808617))?;
    ensure!(
        (parameters.locator as usize) < locators.len(),
        "Object creation locator is undeclared"
    );
    ensure!(
        parameters
            .scale
            .iter()
            .all(|value| value.is_finite() && *value > 0.),
        "Invalid object scale"
    );
    let norm = parameters
        .rotation
        .iter()
        .map(|value| value * value)
        .sum::<f32>();
    ensure!(
        norm.is_finite() && (norm - 1.).abs() < 0.0001,
        "Object rotation is not normalized"
    );
    put(&mut payload.0, event + 0x40, &[0; 0xa0])?;
    floats(payload, event + 0x40, &parameters.position)?;
    floats(payload, event + 0x50, &parameters.rotation)?;
    // World creation leaves fragment retirement with its own lifecycle.
    put(&mut payload.0, event + 0x72, &[2, 0xff])?;
    put(&mut payload.0, event + 0x74, &u32::MAX.to_le_bytes())?;
    put(
        &mut payload.0,
        event + 0x98,
        &parameters.locator.to_le_bytes(),
    )?;
    floats(payload, event + 0x9c, &parameters.speed)?;
    floats(payload, event + 0xa4, &parameters.scale)?;
    put(&mut payload.0, event + 0xb8, &1u32.to_le_bytes())?;
    spread(payload, event + 0xb0, parameters.cone)?;
    // 0040BBA0 evaluates base + random * extent independently on each axis.
    // Extents and the base vector both use X, Y, Z. The random draws arrive
    // in reverse axis order before the native unpack instructions join them.
    let extent = std::array::from_fn::<_, 3, _>(|index| {
        parameters.angular_max[index] - parameters.angular_min[index]
    });
    payload.0[event + 0xc0] = u8::from(extent.iter().any(|value| *value != 0.));
    floats(payload, event + 0xc4, &extent)?;
    floats(payload, event + 0xd0, &parameters.angular_min)?;
    Ok(event + 0x74)
}
