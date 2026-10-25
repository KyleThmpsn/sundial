//! Native premultiplied decal targets, composited as lit layers in the studio.
use super::*;

fn zero_target(code: &Code, target: u32) -> bool {
    (0..4).all(|lane| {
        output(code, target, lane)
            .is_some_and(|(_, row)| row.code == 54 && literal(&row.operands[1], lane, 0.0))
    })
}

fn opacity_control(code: &Code) -> bool {
    let Some((alpha_at, alpha)) = output(code, 0, 3) else {
        return false;
    };
    let Some((color_at, color)) = output(code, 0, 0) else {
        return false;
    };
    if alpha.code != 50
        || alpha.saturate
        || !literal(&alpha.operands[3], 3, 1.0)
        || color.code != 56
        || color.saturate
        || color.operands[0].mask != 7
    {
        return false;
    }
    let Some((control_at, control, lane)) = producer(code, &alpha.operands[1], 3, alpha_at) else {
        return false;
    };
    if control.code != 54 || !control.saturate || control.operands[1].kind != 8 {
        return false;
    }
    (1..=2).any(|source| {
        (0..3).all(|axis| {
            producer(code, &color.operands[source], axis, color_at)
                .is_some_and(|(at, _, other)| at == control_at && other == lane)
        })
    })
}

pub(super) fn supported(code: &Code) -> bool {
    if !zero_target(code, 2) || !opacity_control(code) {
        return false;
    }
    if zero_target(code, 1) {
        return true;
    }
    let Some((at, row)) = output(code, 1, 0) else {
        return false;
    };
    row.code == 56
        && !row.saturate
        && row.operands[0].mask == 7
        && (1..=2).any(|source| {
            producer(code, &row.operands[source], 0, at)
                .is_some_and(|(at, encoded, _)| normal_vector(code, at, encoded))
        })
}

// Blend state 26 writes premultiplied targets with inverse coverage in W.
// Follow the common coverage operand, including the fused target-two Y/W writer.
// The normal radius itself can depend on material branches, so finite packed values
// are checked when the original program executes.
pub(super) fn full(code: &Code) -> bool {
    let Some((at, rgb)) = output(code, 1, 0) else {
        return false;
    };
    let Some((_, alpha)) = output(code, 1, 3) else {
        return false;
    };
    let Some((_, surface)) = output(code, 2, 1) else {
        return false;
    };
    let Some((_, metal)) = output(code, 2, 0) else {
        return false;
    };
    if rgb.code != 56
        || rgb.saturate
        || rgb.operands[0].mask != 7
        || alpha.code != 0
        || alpha.saturate
        || !literal(&alpha.operands[2], 3, 1.0)
        || surface.code != 50
        || surface.saturate
        || surface.operands[0].mask != 10
        || !literal(&surface.operands[2], 1, 0.5)
        || !literal(&surface.operands[2], 3, -1.0)
        || !literal(&surface.operands[3], 1, 0.0)
        || !literal(&surface.operands[3], 3, 1.0)
        || metal.code != 54
        || !literal(&metal.operands[1], 0, 0.0)
    {
        return false;
    }
    let same = |a: &Operand, lane: usize, b: &Operand, other: usize| {
        a.kind == 0
            && b.kind == 0
            && a.indices.len() == 1
            && b.indices.len() == 1
            && a.indices[0].relative.is_none()
            && b.indices[0].relative.is_none()
            && a.indices[0].base == b.indices[0].base
            && a.lanes[lane] == b.lanes[other]
    };
    (1..=2).any(|coverage| {
        let factor = &rgb.operands[coverage];
        factor.modifier == 0
            && (0..3).all(|lane| same(factor, lane, &surface.operands[1], 1))
            && surface.operands[1].modifier == 0
            && same(factor, 0, &surface.operands[1], 3)
            && alpha.operands[1].modifier == 1
            && same(factor, 0, &alpha.operands[1], 3)
            && producer(code, &rgb.operands[3 - coverage], 0, at).is_some_and(|(_, packed, _)| {
                packed.code == 50
                    && packed.saturate
                    && packed.operands[0].mask == 7
                    && (0..3).all(|lane| literal(&packed.operands[3], lane, 0.5))
            })
    })
}

pub(super) fn empty() -> Sample {
    Sample {
        surface: shader::Sample {
            albedo: [0.0; 3],
            roughness: 1.0,
            metal: 0.0,
            ao: 1.0,
            emission: [0.0; 3],
        },
        normal: [0.0, 0.0, 1.0],
        ambient: 1.0,
        coverage: 0.0,
    }
}

pub(super) fn unpack(
    targets: &mut [[f32; 4]; 16],
    pixel: Pixel<'_>,
    coverage: f32,
    full: bool,
) -> Option<()> {
    for value in &mut targets[0][..3] {
        *value /= coverage;
    }
    if targets[1] == [0.0; 4] {
        // A color-only decal keeps the source mesh normal in the studio. The native
        // deferred pass instead leaves the already rendered normal target untouched.
        let normal = shader::normal::normalize([
            pixel.varyings[0][0],
            pixel.varyings[0][1],
            pixel.varyings[0][2],
        ])?;
        targets[1] = [
            normal[0] * 0.375 + 0.5,
            normal[1] * 0.375 + 0.5,
            normal[2] * 0.375 + 0.5,
            0.0,
        ];
    } else {
        let normal_coverage = (1.0 - targets[1][3]).clamp(0.0, 1.0);
        if normal_coverage <= 1e-8 {
            return None;
        }
        for value in &mut targets[1][..3] {
            *value /= normal_coverage;
        }
    }
    if full {
        let alpha = (1.0 - targets[2][3]).clamp(0.0, 1.0);
        if alpha <= 1e-8 {
            return None;
        }
        for value in &mut targets[2][..3] {
            *value /= alpha;
        }
        targets[2][3] = 1.0;
    } else {
        targets[2] = [0.0, 0.5, 0.0, 1.0];
    }
    Some(())
}
