//! Probe-driven lighting regions within the native spatial query budget. Regions are divided and
//! judged by sky visibility itself ([`lightness`]).
//!
//! Reach's structure clusters bound its rooms and open spaces, so their box faces are candidate
//! split planes beside the planes between probes. A face is taken over a better split between
//! probes when it separates nearly as well ([`FACE_PREFERENCE`]), so cells end at walls rather
//! than halfway between a room's probe and the street's. A cell's probe comes from the structure
//! cluster most of its probes stand in ([`Source::pick`]).
use super::{Bounds, tome};
use crate::{AuthoringResult, error::invalid};
use parhelion_import::halo_reach::level::native::cluster_at;
use std::collections::BTreeMap;

mod gradient;

const MARGIN: f32 = 16.;
const MIN_WIDTH: f32 = 4.;
const NODE_BUDGET: usize = tome::MAX_TREE_NODES * 3 / 4;
/// The most leaves a tree within [`NODE_BUDGET`] holds, so the budget alone ends subdivision.
const MAX_CELLS: usize = NODE_BUDGET / 2;
const EPSILON: f64 = 1e-12;
/// The share of the best split's gain that a split on a structure cluster's face must reach to be
/// taken in its place.
const FACE_PREFERENCE: f64 = 0.5;

#[derive(Clone, Copy)]
pub(super) struct Cell {
    pub bounds: Bounds,
    /// Index in the source probe array, before the native record's default probe is inserted.
    pub probe: usize,
    pub gradient: [[f32; 2]; 3],
}

#[derive(Clone, Copy, Default)]
struct Moments {
    count: usize,
    sum: f64,
    squares: f64,
}

/// A probe's value for dividing regions: its visibility. The ambient light the deferred consumer
/// applies to a dynamic object is the square of it, so equal steps of visibility are nearer to
/// equal steps of perceived lightness than equal steps of that light are. Judged by the light, a
/// tunnel at 0.06 beside ground at 0.2 differs by 0.04, less than open ground between 0.3 and 0.4,
/// and it kept the ground's probe: a player there drew about ten times brighter than its walls.
fn lightness(probe: [f32; 4]) -> f64 {
    f64::from(probe[3])
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
            .map(|&index| lightness(probes[index]))
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

    /// The probe a region of `samples` takes: among the samples in its majority structure
    /// cluster, the one nearest their mean visibility, which minimizes the group's total squared
    /// error in [`lightness`]. Restricting the candidates to one structure cluster keeps a
    /// doorway's other room out of that average.
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
        let members = samples
            .iter()
            .copied()
            .filter(|&index| self.clusters[index] == majority)
            .collect::<Vec<_>>();
        let mean = members
            .iter()
            .map(|&index| lightness(self.probes[index]))
            .sum::<f64>()
            / members.len() as f64;
        members
            .into_iter()
            .min_by(|&a, &b| {
                (lightness(self.probes[a]) - mean)
                    .abs()
                    .total_cmp(&(lightness(self.probes[b]) - mean).abs())
                    .then(a.cmp(&b))
            })
            .expect("A lighting region has a majority structure cluster")
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
            moments.add(lightness(source.probes[index]));
        }
        let error = moments.error(&samples, source.probes);
        let mut leaf = Self {
            cell: Cell {
                bounds,
                probe: source.pick(&samples),
                gradient: [[0.; 2]; 3],
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
                prefix[n + 1].add(lightness(probes[index]));
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

/// A part of the plan's division while it grows: a leaf by its index, or a plane on an axis with
/// the parts below and above it.
#[derive(Clone, Copy)]
enum Part {
    Leaf(usize),
    Split(usize, f32, usize, usize),
}

/// The division below part `at` of `parts`, as the tome reads it.
fn division(parts: &[Part], at: usize) -> tome::Division {
    match parts[at] {
        Part::Leaf(leaf) => tome::Division::Cell(leaf as u32),
        Part::Split(axis, position, low, high) => tome::Division::Split(
            axis,
            position,
            Box::new(division(parts, low)),
            Box::new(division(parts, high)),
        ),
    }
}

/// An area's lighting cells, which tile `bounds` by `division`. A cell's index in `cells` is the
/// index the division names it by.
pub(super) struct Plan {
    pub cells: Vec<Cell>,
    pub division: tome::Division,
    pub bounds: Bounds,
}

/// Whether the tome of `crossings` and the cells that `parts` divide `cells` into keeps to the
/// node budget, over the root the tome will take: `area` grown around each crossing and the cells.
fn fits(area: Bounds, crossings: &[tome::Region], cells: Bounds, parts: &[Part]) -> bool {
    let mut bounds = area;
    for extent in crossings.iter().map(|region| region.bounds).chain([cells]) {
        for k in 0..3 {
            bounds.minimum[k] = bounds.minimum[k].min(extent.minimum[k] - MARGIN);
            bounds.maximum[k] = bounds.maximum[k].max(extent.maximum[k] + MARGIN);
        }
    }
    let division = division(parts, 0);
    tome::fits(
        bounds,
        crossings,
        Some(tome::Lighting {
            division: &division,
            bounds: cells,
            base: 1,
        }),
        NODE_BUDGET,
    )
}

/// Lighting cells over the probes inside `area`, outside every crossing, split on their own
/// positions and on the faces of the structure `clusters`, within the node budget of a tome whose
/// partition starts from `root` before the regions grow it. `None` when no probe qualifies.
pub(super) fn cells(
    area: Bounds,
    root: Bounds,
    probes: &[[f32; 4]],
    clusters: &[Bounds],
    crossings: &[tome::Region],
) -> AuthoringResult<Option<Plan>> {
    if probes.len() > u16::MAX as usize || probes.iter().flatten().any(|value| !value.is_finite()) {
        return Err(invalid("Invalid map lighting probes"));
    }
    let samples = distinct(probes, |probe| {
        (0..3).all(|k| area.minimum[k] - MARGIN <= probe[k] && probe[k] <= area.maximum[k] + MARGIN)
            && !crossings.iter().any(|r| contains(r.bounds, probe))
    });
    if samples.is_empty() {
        return Ok(None);
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
    // The division so far, from part 0, and each leaf's part in it.
    let mut parts = vec![Part::Leaf(0)];
    let mut places = vec![0];
    if !fits(root, crossings, bounds, &parts) {
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
        // The leaf keeps the low side and a new leaf takes the high side.
        let place = places[index];
        let (low_part, high_part) = (parts.len(), parts.len() + 1);
        parts.extend([Part::Leaf(index), Part::Leaf(leaves.len())]);
        parts[place] = Part::Split(split.axis, split.position, low_part, high_part);
        if !fits(root, crossings, bounds, &parts) {
            parts.truncate(low_part);
            parts[place] = Part::Leaf(index);
            continue;
        }
        places[index] = low_part;
        places.push(high_part);
        let mut low = leaves[index].cell.bounds;
        let mut high = low;
        low.maximum[split.axis] = split.position;
        high.minimum[split.axis] = split.position;
        let (low_samples, high_samples) = leaves[index]
            .samples
            .iter()
            .copied()
            .partition(|&i| probes[i][split.axis] < split.position);
        leaves[index] = Leaf::new(low, low_samples, &source);
        leaves.push(Leaf::new(high, high_samples, &source));
    }
    let mut cells = leaves.iter().map(|leaf| leaf.cell).collect::<Vec<_>>();
    gradient::fit(&mut cells, &source);
    Ok(Some(Plan {
        cells,
        division: division(&parts, 0),
        bounds,
    }))
}
