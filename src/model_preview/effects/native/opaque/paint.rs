//! Recognize the native painted color recipe and consume the selected dye properties.
//! Default combined-bank values never replace the equipped or edited dye.
use super::*;
mod detail;
mod grain;
pub(super) mod legacy;
mod metal;
mod normal;
mod remap;
mod value;
use value::{Bank, Constant, Node, Value};

pub(in crate::model_preview) struct Paint {
    smooth: Constant,
    gain: Constant,
    metal: Option<Constant>,
    metal_unavailable: bool,
    normal: Option<normal::Normal>,
    normal_unavailable: bool,
    grain: Option<grain::Grain>,
    grain_unavailable: bool,
}
impl Paint {
    pub(in crate::model_preview) fn grain(&self, frame: &Frame) -> Option<[f32; 3]> {
        self.grain.as_ref()?.frame(frame)
    }
    pub(in crate::model_preview) fn grain_glsl(&self) -> Option<String> {
        Some(self.grain.as_ref()?.glsl())
    }
    pub(in crate::model_preview) fn grain_valid_glsl(&self) -> Option<String> {
        Some(self.grain.as_ref()?.valid_glsl())
    }
    pub(super) fn grain_unavailable(&self) -> bool {
        self.grain_unavailable
    }
    pub(in crate::model_preview) fn normal(&self, frame: &Frame) -> Option<[[f32; 2]; 4]> {
        self.normal.as_ref()?.frame(frame)
    }
    pub(in crate::model_preview) fn normal_glsl(&self) -> Option<String> {
        Some(self.normal.as_ref()?.glsl())
    }
    pub(in crate::model_preview) fn normal_valid_glsl(&self) -> Option<String> {
        Some(self.normal.as_ref()?.valid_glsl())
    }
    pub(in crate::model_preview) fn value(&self, frame: &Frame) -> Option<[f32; 2]> {
        let smooth = self.smooth.value(frame);
        let values = [smooth, smooth * self.gain.value(frame)];
        values.iter().all(|v| v.is_finite()).then_some(values)
    }
    pub(in crate::model_preview) fn glsl(&self) -> String {
        let smooth = self.smooth.glsl();
        format!("vec2({smooth},{smooth}*{})", self.gain.glsl())
    }
    pub(in crate::model_preview) fn metal(&self, frame: &Frame) -> Option<f32> {
        let value = self.metal?.value(frame);
        value.is_finite().then(|| value.clamp(0.0, 1.0))
    }
    pub(in crate::model_preview) fn metal_glsl(&self) -> Option<String> {
        Some(format!("clamp({},0.0,1.0)", self.metal?.glsl()))
    }
    pub(super) fn metal_unavailable(&self) -> bool {
        self.metal_unavailable
    }
    pub(super) fn normal_unavailable(&self) -> bool {
        self.normal_unavailable
    }
}

fn factors(code: &Code, high: Value, low: Value) -> Option<Value> {
    let multiply = high.node(code)?;
    let add = low.node(code)?;
    if !multiply.is(56, true) || !add.is(0, true) {
        return None;
    }
    let raw = if multiply.arg(2).literal(4.0) {
        multiply.arg(1)
    } else if multiply.arg(1).literal(4.0) {
        multiply.arg(2)
    } else {
        return None;
    };
    let shifted = if add.arg(2).literal(-0.25) {
        add.arg(1)
    } else if add.arg(1).literal(-0.25) {
        add.arg(2)
    } else {
        return None;
    };
    raw.same(code, &shifted).then_some(raw)
}

struct Layer {
    color: Bank,
    params: Bank,
    detail: Value,
}
fn layer(code: &Code, value: Value, plate: &Value) -> Option<Layer> {
    let overlay = value.node(code)?;
    if !overlay.is(50, true) {
        return None;
    }
    let plate_value = factors(code, overlay.arg(2), overlay.arg(3))?;
    if !plate_value.same(code, plate) {
        return None;
    }
    let mix = overlay.arg(1).node(code)?;
    if !mix.is(50, false) {
        return None;
    }
    let color = mix.arg(3).bank(code)?;
    let params = mix.arg(1).bank(code)?;
    let delta = mix.arg(2).node(code)?;
    if !delta.is(0, false) || !color.rgb() {
        return None;
    }
    let subtracted = delta.arg(2).positive()?.bank(code)?;
    if !subtracted.matches(color, 0, [0, 1, 2], false) {
        return None;
    }
    let detailed = delta.arg(1).node(code)?;
    if !detailed.is(50, true)
        || !detailed
            .arg(1)
            .bank(code)?
            .matches(color, 0, [0, 1, 2], false)
    {
        return None;
    }
    Some(Layer {
        color,
        params,
        detail: factors(code, detailed.arg(2), detailed.arg(3))?,
    })
}

fn unpainted_smooth(
    code: &Code,
    fallback: &Node<'_>,
    smooth: Constant,
    constants: &[[f32; 4]],
) -> Option<Constant> {
    let destination = &fallback.instruction.operands[0];
    let rgb_lanes: Vec<_> = (0..3)
        .map(|lane| fallback.arg(2).operand.lanes[lane])
        .collect();
    let gain_row = fallback.arg(2).operand.indices.get(1)?.base as usize;
    let mut found = None;
    for lane in 0..4 {
        if destination.mask & (1 << lane) == 0 {
            continue;
        }
        let value = Value::scalar(destination, lane, fallback.at + 1).node(code)?;
        let raw = value.arg(1).constant(code);
        let gain = value.arg(2).constant(code);
        if let Some(gain) =
            gain.filter(|g| g.row == gain_row && !rgb_lanes.contains(&g.lane) && g.valid(constants))
            && raw == Some(smooth)
        {
            found = Some(gain);
        }
    }
    found
}

fn match_recipe(
    code: &Code,
    mix: &Node<'_>,
    final_op: &Node<'_>,
    plate: Value,
    constants: &[[f32; 4]],
) -> Option<Paint> {
    let delta = mix.arg(2).node(code)?;
    if !delta.is(0, false) {
        return None;
    }
    let worn_value = delta.arg(1).positive()?;
    if !worn_value.same(code, &mix.arg(3)) {
        return None;
    }
    let pristine = layer(code, delta.arg(2), &plate)?;
    let worn = layer(code, worn_value, &plate)?;
    if !worn.color.matches(pristine.color, 5, [0, 1, 2], false)
        || !pristine.params.matches(pristine.color, 2, [0; 3], true)
        || !worn.params.matches(pristine.color, 8, [0; 3], true)
        || !pristine.detail.same(code, &worn.detail)
    {
        return None;
    }
    let bank_end = pristine.color.row.checked_add(54)?;
    let declared = code.buffers.iter().find(|&&(buffer, _)| buffer == 0)?.1;
    if bank_end > declared || bank_end > constants.len() || bank_end > 128 {
        return None;
    }
    let selector = final_op.arg(1);
    detail::validate(code, constants, &pristine.detail)?;
    let mask_at = gain::selector(code, &selector.operand, selector.before)?;
    remap::wear(code, &mix.arg(1), pristine.color, mask_at)?;
    let smooth = remap::smooth(
        code,
        pristine.color,
        constants,
        &mix.arg(1),
        &pristine.detail,
    )?;
    let fallback = final_op.arg(3).node(code)?;
    let gain = unpainted_smooth(code, &fallback, smooth, constants)?;
    let metal = metal::recover(code, pristine.color, &mix.arg(1), &selector, constants);
    let normal = normal::recover(
        code,
        constants,
        pristine.color,
        &mix.arg(1),
        &selector,
        &plate,
    );
    let normal_unavailable = normal.is_none() && normal::present(code);
    let grain = normal.as_ref().and_then(|normal| {
        grain::recover(
            code,
            constants,
            pristine.color,
            &mix.arg(1),
            &pristine.detail,
            smooth,
            normal,
        )
    });
    let grain_unavailable = normal.is_some() && normal::smooth(code).is_some() && grain.is_none();
    Some(Paint {
        smooth,
        gain,
        metal: metal.as_ref().ok().copied().flatten(),
        metal_unavailable: metal.is_err(),
        normal,
        normal_unavailable,
        grain,
        grain_unavailable,
    })
}

pub(super) fn recover(
    code: &Code,
    at: usize,
    base: &Operand,
    constants: &[[f32; 4]],
) -> Result<Option<Paint>, String> {
    let Some(final_op) = Value::new(base, at).node(code) else {
        return Ok(None);
    };
    if !final_op.is(50, false) {
        return Ok(None);
    }
    let Some(subtract) = final_op.arg(2).node(code) else {
        return Ok(None);
    };
    let Some(mix) = subtract.arg(3).node(code) else {
        return Ok(None);
    };
    if !mix.is(50, false) {
        return Ok(None);
    }
    let fail = || "Opaque painted color has an unsupported material recipe".to_owned();
    let plate = subtract.arg(1).positive().ok_or_else(fail)?;
    match_recipe(code, &mix, &final_op, plate, constants)
        .map(Some)
        .ok_or_else(fail)
}
