use super::*;

pub(super) const WRINKLE: usize = 32;
pub(super) const MASK_A: usize = 33;
pub(super) const MASK_B: usize = 34;

/// Reach's bump functions add detail slopes before applying the wrinkle normal.
/// Wrinkle weights are material inputs, not weights inferred from the geometry.
pub(super) fn expression(
    surface: &surface::Surface<'_>,
    samples: &BTreeMap<usize, String>,
) -> String {
    let option = surface
        .material
        .options
        .get("bump_mapping")
        .map(String::as_str);
    if option == Some("off") {
        return "float3(0,0,1)".into();
    }
    let unit = |slot| {
        samples.get(&slot).map_or_else(
            || "float3(0,0,1)".into(),
            |s| format!("normalize(({s}).xyz*2-1)"),
        )
    };
    let base = unit(1);
    let detail = match option {
        Some("detail" | "detail_wrinkle") => Some(unit(BUMP_DETAIL_SLOT)),
        Some("detail_blend") => {
            let alpha = samples.get(&0).map_or("float4(1,1,1,1)", String::as_str);
            Some(format!(
                "lerp({},{},saturate(({alpha}).a*{}))",
                unit(BUMP_DETAIL_SLOT),
                unit(BUMP_DETAIL2_SLOT),
                surface.scalar("blend_alpha", 1.)
            ))
        }
        Some("three_detail_blend") => {
            let (first, second) = three_detail_weights(surface, samples);
            Some(format!(
                "lerp(lerp({},{},{first}),{},{second})",
                unit(BUMP_DETAIL_SLOT),
                unit(BUMP_DETAIL2_SLOT),
                unit(BUMP_DETAIL3_SLOT)
            ))
        }
        _ => None,
    };
    let base = detail.map_or(base.clone(), |d| {
        format!("normalize(float3(({base}).xy+({d}).xy,({base}).z))")
    });
    if !matches!(option, Some("standard_wrinkle" | "detail_wrinkle")) {
        return base;
    }
    let mask = |slot| samples.get(&slot).map_or("float4(0,0,0,0)", String::as_str);
    let weight = format!(
        "saturate(dot({}, {})+dot({}, {}))",
        mask(MASK_A),
        surface.value("wrinkle_weights_a", [0.; 4]),
        mask(MASK_B),
        surface.value("wrinkle_weights_b", [0.; 4])
    );
    let wrinkle = format!("lerp(float3(0,0,1),{},{weight})", unit(WRINKLE));
    format!("normalize(float3(({base}).xy+({wrinkle}).xy,({base}).z*({wrinkle}).z))")
}
