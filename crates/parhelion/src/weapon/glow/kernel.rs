//! The native gear-mask and deferred emission equations, using fresh temporaries.
use super::dxbc::{Instruction, cb, dst, ins, lit, neg, temp};
use super::{SAT, W, X, XYZ, Y, Z, plan::Plan};
use crate::AuthoringResult;

pub(super) fn instructions(code: &[Instruction], plan: &Plan) -> AuthoringResult<Vec<Instruction>> {
    let Plan {
        slot,
        sample,
        albedo,
        first,
        ..
    } = *plan;
    let base = code[albedo].args[1].clone();
    let mask = first;
    let glow = first + 1;
    let wear = first + 2;
    let color = first + 3;
    let factors = first + 4;
    let mut texture = code[sample].args.clone();
    texture[0] = dst(mask, 15);
    // Read all gearstack components, including blue omitted by non-emissive shaders.
    texture[2].0[0] = (texture[2].0[0] & !0xFFF) | 2 | (1 << 2) | (XYZ << 4);
    let suffix = vec![
        code[sample].replace(texture)?,
        ins(
            0,
            vec![dst(glow, 7), neg(cb(slot, 3, XYZ)), cb(slot, 4, XYZ)],
        ),
        ins(
            50,
            vec![dst(glow, 7), cb(0, 0, X), temp(glow, XYZ), cb(slot, 3, XYZ)],
        ),
        ins(0, vec![dst(glow, 8), neg(cb(slot, 25, Y)), cb(slot, 26, Y)]),
        ins(
            50,
            vec![dst(glow, 8), cb(0, 0, X), temp(glow, W), cb(slot, 25, Y)],
        ),
        ins(57, vec![dst(glow, 8), temp(glow, W), lit(0.0)]),
        // Use the same blue threshold, eligibility and dye wear factor as native glow.
        ins(0, vec![dst(mask, 4), temp(mask, Z), lit(-40.0 / 255.0)]),
        ins(
            56 | SAT,
            vec![
                dst(mask, 4),
                temp(mask, Z),
                lit(f32::from_bits(0x3F97_D05F)),
            ],
        ),
        ins(29, vec![dst(factors, 1), temp(mask, W), lit(40.0 / 255.0)]),
        ins(1, vec![dst(factors, 1), temp(factors, X), temp(glow, W)]),
        ins(1, vec![dst(factors, 1), temp(factors, X), lit(1.0)]),
        ins(56, vec![dst(mask, 4), temp(mask, Z), temp(factors, X)]),
        ins(
            0,
            vec![dst(wear, 15), neg(cb(slot, 18, XYZ)), cb(slot, 22, XYZ)],
        ),
        ins(
            50,
            vec![
                dst(wear, 15),
                cb(0, 0, X),
                temp(wear, XYZ),
                cb(slot, 18, XYZ),
            ],
        ),
        ins(0, vec![dst(mask, 8), temp(mask, W), lit(-48.0 / 255.0)]),
        ins(56 | SAT, vec![dst(mask, 8), temp(mask, W), lit(1.231884)]),
        ins(
            50 | SAT,
            vec![dst(mask, 8), temp(wear, Y), temp(mask, W), temp(wear, X)],
        ),
        ins(
            50 | SAT,
            vec![dst(mask, 8), temp(wear, W), temp(mask, W), temp(wear, Z)],
        ),
        ins(56, vec![dst(mask, 4), temp(mask, Z), temp(mask, W)]),
        ins(54, vec![dst(color, 15), base]),
        ins(54, vec![dst(factors, 8), lit(0.0078125)]),
        // Preserve the exact original path on zero masks or disabled selectors.
        ins(31 | (1 << 18), vec![temp(mask, Z)]),
        ins(54 | SAT, vec![dst(glow, 7), temp(glow, XYZ)]),
        ins(56, vec![dst(glow, 7), temp(glow, XYZ), temp(mask, Z)]),
        ins(52, vec![dst(factors, 8), temp(glow, X), temp(glow, Y)]),
        ins(52, vec![dst(factors, 8), temp(factors, W), temp(glow, Z)]),
        ins(0, vec![dst(factors, 8), temp(factors, W), lit(0.0078125)]),
        ins(0, vec![dst(wear, 7), temp(color, XYZ), temp(glow, XYZ)]),
        ins(52, vec![dst(factors, 1), temp(wear, X), temp(wear, Y)]),
        ins(52, vec![dst(factors, 1), temp(factors, X), temp(wear, Z)]),
        ins(SAT, vec![dst(factors, 1), temp(factors, X), lit(-1.0)]),
        ins(0, vec![dst(factors, 1), neg(temp(factors, X)), lit(1.0)]),
        ins(
            50,
            vec![
                dst(color, 7),
                temp(color, XYZ),
                temp(factors, X),
                temp(glow, XYZ),
            ],
        ),
        ins(52, vec![dst(factors, 1), temp(color, X), temp(color, Y)]),
        ins(52, vec![dst(factors, 1), temp(factors, X), temp(color, Z)]),
        ins(52, vec![dst(factors, 1), temp(factors, X), lit(1.0)]),
        ins(14, vec![dst(color, 7), temp(color, XYZ), temp(factors, X)]),
        ins(21, vec![]),
    ];
    Ok(suffix)
}
