//! Probe-driven lighting regions within the native spatial query budget. Squared sky visibility
//! is proportional to the ambient light the deferred consumer applies to a dynamic object.
use super::{Bounds, tome};
use crate::{AuthoringResult, error::invalid};
use std::collections::VecDeque;

const MARGIN: f32 = 16.;
const MIN_WIDTH: f32 = 4.;
const MAX_CELLS: usize = 512;
const NODE_BUDGET: usize = tome::MAX_TREE_NODES * 3 / 4;
const EPSILON: f64 = 1e-12;

#[derive(Clone, Copy)]
pub(super) struct Cell {
    pub bounds: Bounds,
    /// Index in the source probe array, before the native record's default probe is inserted.
    pub probe: usize,
}

#[derive(Clone, Copy, Default)]
struct Moments {
    count: usize,
    sum: f64,
    squares: f64,
}

fn light(probe: [f32; 4]) -> f64 {
    f64::from(probe[3]).powi(2)
}

impl Moments {
    fn add(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        self.squares += value * value;
    }

    fn without(self, other: Self) -> Self {
        Self {
            count: self.count - other.count,
            sum: self.sum - other.sum,
            squares: self.squares - other.squares,
        }
    }

    fn representative(self, samples: &[usize], probes: &[[f32; 4]]) -> (usize, f64) {
        let mean = self.sum / self.count as f64;
        let probe = *samples
            .iter()
            .min_by(|&&a, &&b| {
                (light(probes[a]) - mean)
                    .abs()
                    .total_cmp(&(light(probes[b]) - mean).abs())
                    .then(a.cmp(&b))
            })
            .expect("A lighting region has at least one probe");
        let value = light(probes[probe]);
        let error = self.squares - 2. * value * self.sum + self.count as f64 * value * value;
        (probe, error.max(0.))
    }
}

#[derive(Clone, Copy)]
struct Split {
    axis: usize,
    position: f32,
    gain: f64,
    balance: usize,
}

struct Leaf {
    cell: Cell,
    samples: Vec<usize>,
    error: f64,
    split: Option<Split>,
}

impl Leaf {
    fn new(bounds: Bounds, samples: Vec<usize>, probes: &[[f32; 4]]) -> Self {
        let mut moments = Moments::default();
        for &index in &samples {
            moments.add(light(probes[index]));
        }
        let (probe, error) = moments.representative(&samples, probes);
        let mut leaf = Self {
            cell: Cell { bounds, probe },
            samples,
            error,
            split: None,
        };
        if error > EPSILON {
            leaf.split = leaf.best_split(probes, moments);
        }
        leaf
    }

    /// Evaluate separations between probe positions. Both children select a probe they contain,
    /// and their combined error cannot exceed their parent's. A zero-gain split can still expose
    /// a contrast on another axis, so it remains eligible after positive-gain splits.
    fn best_split(&self, probes: &[[f32; 4]], total: Moments) -> Option<Split> {
        let mut best: Option<Split> = None;
        for axis in 0..3 {
            let mut sorted = self.samples.clone();
            sorted.sort_by(|&a, &b| probes[a][axis].total_cmp(&probes[b][axis]).then(a.cmp(&b)));
            let mut left = Moments::default();
            for cut in 1..sorted.len() {
                left.add(light(probes[sorted[cut - 1]]));
                let a = probes[sorted[cut - 1]][axis];
                let b = probes[sorted[cut]][axis];
                let position = a + (b - a) * 0.5;
                if a >= position
                    || position > b
                    || position - self.cell.bounds.minimum[axis] < MIN_WIDTH
                    || self.cell.bounds.maximum[axis] - position < MIN_WIDTH
                {
                    continue;
                }
                let (_, low_error) = left.representative(&sorted[..cut], probes);
                let (_, high_error) = total.without(left).representative(&sorted[cut..], probes);
                let gain = self.error - low_error - high_error;
                if gain < -EPSILON {
                    continue;
                }
                let candidate = Split {
                    axis,
                    position,
                    gain: gain.max(0.),
                    balance: cut.min(sorted.len() - cut),
                };
                if best.is_none_or(|old| {
                    candidate
                        .gain
                        .total_cmp(&old.gain)
                        .then(candidate.balance.cmp(&old.balance))
                        .is_gt()
                }) {
                    best = Some(candidate);
                }
            }
        }
        best
    }
}

fn contains(bounds: Bounds, point: [f32; 4]) -> bool {
    (0..3).all(|k| bounds.minimum[k] <= point[k] && point[k] < bounds.maximum[k])
}

/// Choose central regions first. The tome cuts at the first intersecting region's faces, so
/// spatially sorted leaves would peel thin slices from one edge and exhaust its depth limit.
fn order(cells: &[Cell]) -> Vec<usize> {
    let mut pending = VecDeque::from([(0..cells.len()).collect::<Vec<_>>()]);
    let mut result = Vec::with_capacity(cells.len());
    while let Some(mut group) = pending.pop_front() {
        if group.is_empty() {
            continue;
        }
        let axis = (0..3)
            .max_by(|&a, &b| {
                let span = |axis: usize| {
                    let values = group.iter().map(|&i| cells[i].bounds.center()[axis]);
                    values.clone().fold(f32::NEG_INFINITY, f32::max)
                        - values.fold(f32::INFINITY, f32::min)
                };
                span(a).total_cmp(&span(b))
            })
            .unwrap_or(0);
        group.sort_by(|&a, &b| {
            cells[a].bounds.center()[axis]
                .total_cmp(&cells[b].bounds.center()[axis])
                .then(a.cmp(&b))
        });
        let right = group.split_off(group.len() / 2 + 1);
        result.push(group.pop().unwrap());
        pending.extend([group, right]);
    }
    result
}

fn fits(area: Bounds, crossings: &[tome::Region], cells: &[Cell]) -> bool {
    let regions = crossings
        .iter()
        .copied()
        .chain(order(cells).into_iter().map(|index| tome::Region {
            bounds: cells[index].bounds,
            cluster: 1,
        }))
        .collect::<Vec<_>>();
    let mut bounds = area;
    for region in &regions {
        for k in 0..3 {
            bounds.minimum[k] = bounds.minimum[k].min(region.bounds.minimum[k] - MARGIN);
            bounds.maximum[k] = bounds.maximum[k].max(region.bounds.maximum[k] + MARGIN);
        }
    }
    tome::fits(bounds, &regions, NODE_BUDGET)
}

pub(super) fn cells(
    area: Bounds,
    probes: &[[f32; 4]],
    crossings: &[tome::Region],
) -> AuthoringResult<Vec<Cell>> {
    if probes.len() > u16::MAX as usize || probes.iter().flatten().any(|value| !value.is_finite()) {
        return Err(invalid("Invalid map lighting probes"));
    }
    let mut samples = (0..probes.len())
        .filter(|&i| {
            (0..3).all(|k| {
                area.minimum[k] - MARGIN <= probes[i][k] && probes[i][k] <= area.maximum[k] + MARGIN
            }) && !crossings.iter().any(|r| contains(r.bounds, probes[i]))
        })
        .collect::<Vec<_>>();
    // A donor-sized probe array can repeat source samples. Keep their original indices without
    // treating padding as additional evidence for a cell's brightness or refinement priority.
    samples.sort_by(|&a, &b| {
        (0..4)
            .map(|k| probes[a][k].total_cmp(&probes[b][k]))
            .find(|ordering| !ordering.is_eq())
            .unwrap_or_else(|| a.cmp(&b))
    });
    samples.dedup_by(|a, b| probes[*a] == probes[*b]);
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let bounds = Bounds {
        minimum: std::array::from_fn(|k| {
            samples
                .iter()
                .map(|&i| probes[i][k])
                .fold(f32::INFINITY, f32::min)
                - MARGIN
        }),
        maximum: std::array::from_fn(|k| {
            samples
                .iter()
                .map(|&i| probes[i][k])
                .fold(f32::NEG_INFINITY, f32::max)
                + MARGIN
        }),
    };
    if (0..3).any(|k| {
        !bounds.minimum[k].is_finite()
            || !bounds.maximum[k].is_finite()
            || bounds.minimum[k] >= bounds.maximum[k]
    }) {
        return Err(invalid("Invalid map lighting bounds"));
    }
    let mut leaves = vec![Leaf::new(bounds, samples, probes)];
    let mut cells = vec![leaves[0].cell];
    if !fits(area, crossings, &cells) {
        return Err(invalid(
            "Map crossings leave no spatial budget for lighting",
        ));
    }
    while leaves.len() < MAX_CELLS {
        let Some(index) = (0..leaves.len())
            .filter(|&i| leaves[i].split.is_some())
            .max_by(|&a, &b| {
                leaves[a]
                    .split
                    .unwrap()
                    .gain
                    .total_cmp(&leaves[b].split.unwrap().gain)
                    .then(leaves[a].error.total_cmp(&leaves[b].error))
                    .then_with(|| b.cmp(&a))
            })
        else {
            break;
        };
        // A refused split is not retried, keeping work bounded even when the tree is full.
        let split = leaves[index].split.take().unwrap();
        let mut low = leaves[index].cell;
        let mut high = low;
        low.bounds.maximum[split.axis] = split.position;
        high.bounds.minimum[split.axis] = split.position;
        let previous = std::mem::replace(&mut cells[index], low);
        cells.push(high);
        if !fits(area, crossings, &cells) {
            cells[index] = previous;
            cells.pop();
            continue;
        }
        let (low_samples, high_samples) = leaves[index]
            .samples
            .iter()
            .copied()
            .partition(|&i| probes[i][split.axis] < split.position);
        leaves[index] = Leaf::new(low.bounds, low_samples, probes);
        leaves.push(Leaf::new(high.bounds, high_samples, probes));
        cells[index] = leaves[index].cell;
        *cells.last_mut().unwrap() = leaves.last().unwrap().cell;
    }
    Ok(order(&cells)
        .into_iter()
        .map(|index| cells[index])
        .collect())
}
