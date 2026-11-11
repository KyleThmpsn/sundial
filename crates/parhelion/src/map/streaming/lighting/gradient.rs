//! Neighboring cells supply the signed-axis gradients the native probe format already supports.
//! Source positions and visibility stay fixed. Structure-cluster changes remain separate, and
//! bounded gradients reduce artificial steps within a space without inventing a runtime blend.
use super::{Cell, Source};

const FACE_TOLERANCE: f32 = 0.0001;
/// A render part may extend beyond the position whose spatial cluster its owner selects.
const PART_MARGIN: f64 = 2.;

fn adjacent(a: Cell, b: Cell, axis: usize, side: usize) -> bool {
    let face = if side == 0 {
        a.bounds.minimum[axis]
    } else {
        a.bounds.maximum[axis]
    };
    let other = if side == 0 {
        b.bounds.maximum[axis]
    } else {
        b.bounds.minimum[axis]
    };
    (face - other).abs() <= FACE_TOLERANCE
        && (0..3).filter(|&k| k != axis).all(|k| {
            a.bounds.maximum[k].min(b.bounds.maximum[k])
                - a.bounds.minimum[k].max(b.bounds.minimum[k])
                > FACE_TOLERANCE
        })
}

/// Bound all extrema of the separable, two-sided function on the cell expanded for attached
/// parts. Positive and negative contributions have separate budgets, retaining the source value
/// at the anchor and avoiding negative light or overshoot between the corners.
fn bound(cell: Cell, source: [f32; 4], gradient: &mut [[f64; 2]; 3]) {
    let mut low = 0f64;
    let mut high = 0f64;
    for axis in 0..3 {
        let negative =
            (f64::from(cell.bounds.minimum[axis]) - PART_MARGIN - f64::from(source[axis]))
                * gradient[axis][0];
        let positive = (f64::from(cell.bounds.maximum[axis]) + PART_MARGIN
            - f64::from(source[axis]))
            * gradient[axis][1];
        low += negative.min(positive).min(0.);
        high += negative.max(positive).max(0.);
    }
    let darken = if low < 0. {
        (f64::from(source[3]) / -low).min(1.)
    } else {
        1.
    };
    let brighten = if high > 0. {
        ((1. - f64::from(source[3])) / high).min(1.)
    } else {
        1.
    };
    for coefficients in gradient {
        for (side, coefficient) in coefficients.iter_mut().enumerate() {
            let change = *coefficient * if side == 0 { -1. } else { 1. };
            *coefficient *= if change > 0. { brighten } else { darken };
        }
    }
}

pub(super) fn fit(cells: &mut [Cell], source: &Source<'_>) {
    for index in 0..cells.len() {
        let cell = cells[index];
        let Some(cluster) = source.clusters[cell.probe] else {
            continue;
        };
        let anchor = source.probes[cell.probe];
        let mut slopes = [[0f64; 2]; 3];
        let mut weights = [[0f64; 2]; 3];
        for (other_index, &other) in cells.iter().enumerate() {
            if other_index == index || source.clusters[other.probe] != Some(cluster) {
                continue;
            }
            let neighbor = source.probes[other.probe];
            let distance = (0..3)
                .map(|k| (f64::from(neighbor[k]) - f64::from(anchor[k])).powi(2))
                .sum::<f64>();
            if distance < 1e-8 {
                continue;
            }
            // Nearby samples govern the transition. Distant samples sharing a large face have
            // less influence, while the one-square-metre floor avoids unstable coincident fits.
            let weight = 1. / distance.max(1.).powi(2);
            for axis in 0..3 {
                let delta = f64::from(neighbor[axis]) - f64::from(anchor[axis]);
                if delta.abs() < f64::from(FACE_TOLERANCE) {
                    continue;
                }
                let side = usize::from(delta >= 0.);
                if adjacent(cell, other, axis, side) {
                    slopes[axis][side] +=
                        weight * (f64::from(neighbor[3]) - f64::from(anchor[3])) / delta;
                    weights[axis][side] += weight;
                }
            }
        }
        for axis in 0..3 {
            for side in 0..2 {
                if weights[axis][side] > 0. {
                    slopes[axis][side] /= weights[axis][side];
                }
            }
        }
        bound(cell, anchor, &mut slopes);
        cells[index].gradient = slopes.map(|sides| sides.map(|v| v as f32));
    }
}
