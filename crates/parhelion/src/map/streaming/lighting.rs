//! Probe-driven lighting regions within the native spatial query budget. Squared sky visibility
//! is proportional to the ambient light the deferred consumer applies to a dynamic object.
//!
//! Reach's structure clusters bound its rooms and open spaces, so their box faces are candidate
//! split planes beside the planes between probes. A face is taken over a better split between
//! probes when it separates nearly as well ([`FACE_PREFERENCE`]), so cells end at walls rather
//! than halfway between a room's probe and the street's. A cell's probe comes from the structure
//! cluster most of its probes stand in ([`Source::pick`]).
use super::{Bounds, tome};
use crate::{AuthoringResult, error::invalid};
use parhelion_import::halo_reach::level::native::cluster_at;
use std::collections::{BTreeMap, VecDeque};

const MARGIN: f32 = 16.;
const MIN_WIDTH: f32 = 4.;
const MAX_CELLS: usize = 512;
const NODE_BUDGET: usize = tome::MAX_TREE_NODES * 3 / 4;
const EPSILON: f64 = 1e-12;
/// The share of the best split's gain that a split on a structure cluster's face must reach to be
/// taken in its place.
const FACE_PREFERENCE: f64 = 0.5;

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

    /// The squared error of `samples`, whose moments these are, against the sample value nearest
    /// their mean, the least any one of their probes leaves. It measures how far light varies
    /// within a region, apart from the probe the region finally takes.
    fn error(self, samples: &[usize], probes: &[[f32; 4]]) -> f64 {
        let mean = self.sum / self.count as f64;
        let value = samples
            .iter()
            .map(|&index| light(probes[index]))
            .min_by(|a, b| (a - mean).abs().total_cmp(&(b - mean).abs()))
            .expect("A lighting region has at least one probe");
        let error = self.squares - 2. * value * self.sum + self.count as f64 * value * value;
        error.max(0.)
    }
}

/// The probes a plan divides, with the structure clusters they stand in.
struct Source<'a> {
    probes: &'a [[f32; 4]],
    /// Each sample's structure cluster by probe index ([`cluster_at`]), `None` for other probes
    /// and for a sample outside every cluster.
    clusters: Vec<Option<usize>>,
    /// The structure cluster box faces on each axis, ascending and without repeats.
    faces: [Vec<f32>; 3],
}

impl<'a> Source<'a> {
    /// `probes`, each of `samples` assigned its structure cluster among the `clusters` that reach
    /// into `bounds`, whose faces become split planes.
    fn new(probes: &'a [[f32; 4]], samples: &[usize], clusters: &[Bounds], bounds: Bounds) -> Self {
        let clusters = clusters
            .iter()
            .copied()
            .filter(|cluster| {
                (0..3).all(|k| {
                    cluster.minimum[k].is_finite()
                        && cluster.maximum[k].is_finite()
                        && cluster.minimum[k] < bounds.maximum[k]
                        && bounds.minimum[k] < cluster.maximum[k]
                })
            })
            .collect::<Vec<_>>();
        let mut membership = vec![None; probes.len()];
        for &index in samples {
            let probe = probes[index];
            membership[index] = cluster_at(&clusters, [probe[0], probe[1], probe[2]]);
        }
        Self {
            probes,
            clusters: membership,
            faces: std::array::from_fn(|axis| {
                let mut faces = clusters
                    .iter()
                    .flat_map(|cluster| [cluster.minimum[axis], cluster.maximum[axis]])
                    .collect::<Vec<_>>();
                faces.sort_by(f32::total_cmp);
                faces.dedup();
                faces
            }),
        }
    }

    /// The probe a region of `samples` takes: of the samples standing in the structure cluster
    /// most of them stand in, the one at the first quartile of squared visibility. A dynamic
    /// object can stand anywhere in the region. A region that reaches through a doorway or under
    /// a window keeps the room's light, as Reach lit an object from the floor below it, rather
    /// than the brighter light at the region's mean.
    fn pick(&self, samples: &[usize]) -> usize {
        let mut counts = BTreeMap::<Option<usize>, usize>::new();
        for &index in samples {
            *counts.entry(self.clusters[index]).or_default() += 1;
        }
        // The most samples, then a cluster over none, then the first cluster.
        let majority = counts
            .into_iter()
            .max_by(|a, b| {
                a.1.cmp(&b.1)
                    .then(a.0.is_some().cmp(&b.0.is_some()))
                    .then(b.0.cmp(&a.0))
            })
            .map(|(cluster, _)| cluster)
            .expect("A lighting region has at least one probe");
        let mut members = samples
            .iter()
            .copied()
            .filter(|&index| self.clusters[index] == majority)
            .collect::<Vec<_>>();
        members.sort_by(|&a, &b| {
            light(self.probes[a])
                .total_cmp(&light(self.probes[b]))
                .then(a.cmp(&b))
        });
        members[(members.len() - 1) / 4]
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
    fn new(bounds: Bounds, samples: Vec<usize>, source: &Source) -> Self {
        let mut moments = Moments::default();
        for &index in &samples {
            moments.add(light(source.probes[index]));
        }
        let error = moments.error(&samples, source.probes);
        let mut leaf = Self {
            cell: Cell {
                bounds,
                probe: source.pick(&samples),
            },
            samples,
            error,
            split: None,
        };
        if error > EPSILON {
            leaf.split = leaf.best_split(source, moments);
        }
        leaf
    }

    /// Evaluate separations between probe positions and on structure cluster faces. Both
    /// children hold a probe and keep at least [`MIN_WIDTH`]. Between probes, their combined error
    /// cannot exceed their parent's by more than rounding. A zero-gain split can still expose a
    /// contrast on another axis, so it remains eligible after positive-gain splits. The best face
    /// split wins when its gain reaches [`FACE_PREFERENCE`] of the best split between probes.
    fn best_split(&self, source: &Source, total: Moments) -> Option<Split> {
        let probes = source.probes;
        let bounds = self.cell.bounds;
        // The best split between probes, then the best on a face.
        let mut best: [Option<Split>; 2] = [None; 2];
        for axis in 0..3 {
            let mut sorted = self.samples.clone();
            sorted.sort_by(|&a, &b| probes[a][axis].total_cmp(&probes[b][axis]).then(a.cmp(&b)));
            // The moments of the first `n` sorted samples, at `n`.
            let mut prefix = vec![Moments::default(); sorted.len() + 1];
            for (n, &index) in sorted.iter().enumerate() {
                prefix[n + 1] = prefix[n];
                prefix[n + 1].add(light(probes[index]));
            }
            let wide = |position: f32| {
                position - bounds.minimum[axis] >= MIN_WIDTH
                    && bounds.maximum[axis] - position >= MIN_WIDTH
            };
            let consider =
                |kind: usize, cut: usize, position: f32, best: &mut [Option<Split>; 2]| {
                    let low_error = prefix[cut].error(&sorted[..cut], probes);
                    let high_error = total.without(prefix[cut]).error(&sorted[cut..], probes);
                    let gain = self.error - low_error - high_error;
                    if gain < -EPSILON {
                        return;
                    }
                    let candidate = Split {
                        axis,
                        position,
                        gain: gain.max(0.),
                        balance: cut.min(sorted.len() - cut),
                    };
                    if best[kind].is_none_or(|old| {
                        candidate
                            .gain
                            .total_cmp(&old.gain)
                            .then(candidate.balance.cmp(&old.balance))
                            .is_gt()
                    }) {
                        best[kind] = Some(candidate);
                    }
                };
            for cut in 1..sorted.len() {
                let a = probes[sorted[cut - 1]][axis];
                let b = probes[sorted[cut]][axis];
                let position = a + (b - a) * 0.5;
                if a >= position || position > b || !wide(position) {
                    continue;
                }
                consider(0, cut, position, &mut best);
            }
            let faces = &source.faces[axis];
            let first = faces.partition_point(|&face| face <= bounds.minimum[axis]);
            for &position in &faces[first..] {
                if position >= bounds.maximum[axis] {
                    break;
                }
                if !wide(position) {
                    continue;
                }
                let cut = sorted.partition_point(|&index| probes[index][axis] < position);
                if cut == 0 || cut == sorted.len() {
                    continue;
                }
                consider(1, cut, position, &mut best);
            }
        }
        match best {
            [Some(between), Some(face)] if face.gain >= between.gain * FACE_PREFERENCE => {
                Some(face)
            }
            [between, face] => between.or(face),
        }
    }
}

fn contains(bounds: Bounds, point: [f32; 4]) -> bool {
    (0..3).all(|k| bounds.minimum[k] <= point[k] && point[k] < bounds.maximum[k])
}

/// The indices of the `probes` that `keep` accepts. A donor-sized probe array can repeat
/// source samples. Their original indices stay valid, and each repeated sample counts once, so
/// padding does not count as further evidence for a region's brightness or refinement priority.
fn distinct(probes: &[[f32; 4]], keep: impl Fn([f32; 4]) -> bool) -> Vec<usize> {
    let mut samples = (0..probes.len())
        .filter(|&i| keep(probes[i]))
        .collect::<Vec<_>>();
    samples.sort_by(|&a, &b| {
        (0..4)
            .map(|k| probes[a][k].total_cmp(&probes[b][k]))
            .find(|ordering| !ordering.is_eq())
            .unwrap_or_else(|| a.cmp(&b))
    });
    samples.dedup_by(|a, b| probes[*a] == probes[*b]);
    samples
}

/// The probe crossing region `bounds` takes from the probes inside it, chosen as a lighting cell
/// chooses its own ([`Source::pick`]). `None` when it holds no probe.
pub(super) fn region_probe(
    bounds: Bounds,
    probes: &[[f32; 4]],
    clusters: &[Bounds],
) -> Option<usize> {
    let samples = distinct(probes, |probe| contains(bounds, probe));
    (!samples.is_empty()).then(|| Source::new(probes, &samples, clusters, bounds).pick(&samples))
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

/// Lighting cells over the probes inside `area`, outside every crossing, split on their own
/// positions and on the faces of the structure `clusters`, within the node budget of a tome whose
/// partition starts from `root` before the regions grow it.
pub(super) fn cells(
    area: Bounds,
    root: Bounds,
    probes: &[[f32; 4]],
    clusters: &[Bounds],
    crossings: &[tome::Region],
) -> AuthoringResult<Vec<Cell>> {
    if probes.len() > u16::MAX as usize || probes.iter().flatten().any(|value| !value.is_finite()) {
        return Err(invalid("Invalid map lighting probes"));
    }
    let samples = distinct(probes, |probe| {
        (0..3).all(|k| area.minimum[k] - MARGIN <= probe[k] && probe[k] <= area.maximum[k] + MARGIN)
            && !crossings.iter().any(|r| contains(r.bounds, probe))
    });
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
    let source = Source::new(probes, &samples, clusters, bounds);
    let mut leaves = vec![Leaf::new(bounds, samples, &source)];
    let mut cells = vec![leaves[0].cell];
    if !fits(root, crossings, &cells) {
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
        if !fits(root, crossings, &cells) {
            cells[index] = previous;
            cells.pop();
            continue;
        }
        let (low_samples, high_samples) = leaves[index]
            .samples
            .iter()
            .copied()
            .partition(|&i| probes[i][split.axis] < split.position);
        leaves[index] = Leaf::new(low.bounds, low_samples, &source);
        leaves.push(Leaf::new(high.bounds, high_samples, &source));
        cells[index] = leaves[index].cell;
        *cells.last_mut().unwrap() = leaves.last().unwrap().cell;
    }
    Ok(order(&cells)
        .into_iter()
        .map(|index| cells[index])
        .collect())
}
