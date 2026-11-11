//! Patchy fog attenuation from HREK's PC constant producer and two shader passes.

/// The height coefficient at original PC RVA `8939BD`. Both shader passes square their input,
/// so the final optical depth contains the fourth power of this clamped linear height value.
/// The original slope uses the negative fourth root of one half, not a slope fitted to the
/// parameter labels. `full` and `half` have the same coordinate origin.
pub(super) fn height(full: f32, half: f32) -> (f32, f32) {
    let span = half - full;
    let slope = if span.abs() > 1e-4 {
        -0.5f32.sqrt().sqrt() / span
    } else {
        0.
    };
    (slope, 1. - slope * full)
}

/// Density scales the optical depth once. The prepass squares the height fade and multiplies
/// the sheet fade, then the application pass squares that complete sample. Patchy color
/// intensity is not consumed by the original patchy parameter accumulator or this calculation.
pub(super) fn function(density: f32, full: f32, half: f32, reference: f32) -> String {
    let (slope, offset) = height(full + reference, half + reference);
    format!(
        r#"
float reachPatchyTransmission(float noise,float worldHeight,float depthFade,float sheetFade) {{
    float height=saturate({slope:.9}*worldHeight+{offset:.9});
    float sample=noise*height*height*sheetFade;
    return exp2(-{density:.9}*sample*sample*depthFade);
}}
"#,
    )
}
