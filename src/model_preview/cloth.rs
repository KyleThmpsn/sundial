//! Deterministic 60 Hz studio playback of supported Shadowkeep cloth states.
//! Native CPU witnesses cover the recovered operators. Scene wind, landscape
//! collision, character controllers and automatic detail transitions are separate.
use super::{Load, Model, animation};
use graph::{Graph, Operator};
use math::*;
mod collision;
mod constraint;
mod frame;
mod graph;
mod math;
mod pack;
mod skin;

#[derive(Clone, Copy, Default)]
struct Point {
    position: Vector,
    normal: Vector,
    tangent: Vector,
}

pub(super) struct Display {
    pub first: usize,
    pub buffer: usize,
    pub count: usize,
}
pub(super) struct Pending {
    graph: Graph,
    display: Vec<Display>,
    bindings: Vec<(usize, usize, usize)>,
}
pub(super) struct Timeline {
    vertices: Vec<usize>,
    frames: Vec<Vec<Point>>,
}

impl Pending {
    pub fn read(bytes: &[u8], states: [usize; 4], display: Vec<Display>) -> Result<Self, String> {
        let graph = Graph::read(bytes, states)?;
        if display.is_empty() {
            return Err("The cloth simulation state has no display buffers".into());
        }
        for (i, binding) in display.iter().enumerate() {
            if binding.count == 0
                || binding.count > graph.buffer(binding.buffer)?
                || binding.first.checked_add(binding.count).is_none()
            {
                return Err("Invalid cloth display buffer extent".into());
            }
            if display[..i].iter().any(|b| {
                binding.first < b.first + b.count && b.first < binding.first + binding.count
            }) {
                return Err("Overlapping cloth display buffers".into());
            }
        }
        Ok(Self {
            graph,
            display,
            bindings: Vec::new(),
        })
    }

    pub fn bind(&mut self, base: usize, selected: &[u32]) -> Result<(), String> {
        for (offset, &vertex) in selected.iter().enumerate() {
            let vertex = vertex as usize;
            let binding = self
                .display
                .iter()
                .find(|b| vertex >= b.first && vertex < b.first + b.count)
                .ok_or("Cloth draw vertex has no simulation buffer binding")?;
            self.bindings
                .push((base + offset, binding.buffer, vertex - binding.first));
        }
        Ok(())
    }

    pub fn bake(
        self,
        animation: Option<&animation::Animation>,
        cancel: &Load,
    ) -> Result<Timeline, String> {
        if self.bindings.is_empty() {
            return Err("The cloth render group has no visible vertices".into());
        }
        let seconds = animation.map_or(4., |a| a.duration().max(1. / 60.));
        if seconds > 30. {
            return Err("The cloth animation exceeds the 30-second studio budget".into());
        }
        let count = (seconds * 60.).ceil() as usize + 1;
        if count
            .checked_mul(self.bindings.len())
            .is_none_or(|n| n > 2_000_000)
        {
            return Err("The cloth timeline exceeds the studio memory budget".into());
        }
        let mut runtime = Runtime::new(&self.graph, animation)?;
        runtime.execute(&self.graph.initialize, &self.graph)?;
        let mut frames = Vec::with_capacity(count);
        for frame in 0..count {
            cancel.check()?;
            if frame % 30 == 0 {
                cancel.say("Simulating cloth", frame, count);
            }
            if frame != 0 {
                runtime.transforms = transforms(&self.graph, animation, frame as f32 / 60.)?;
                runtime.execute(&self.graph.update, &self.graph)?;
            }
            let mut points = Vec::with_capacity(self.bindings.len());
            for &(_, buffer, vertex) in &self.bindings {
                if !runtime.written[buffer][vertex] {
                    return Err("The cloth state leaves a display vertex uninitialized".into());
                }
                // Skinning has already expanded normalized stored vertices into
                // object space. Native simulation draws do not normalize again.
                let point = runtime.buffers[buffer][vertex];
                if point
                    .position
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 10_000.)
                    || point
                        .normal
                        .iter()
                        .chain(&point.tangent)
                        .any(|v| !v.is_finite())
                {
                    return Err("Cloth simulation produced an invalid display frame".into());
                }
                points.push(point);
            }
            frames.push(points);
        }
        Ok(Timeline {
            vertices: self.bindings.into_iter().map(|b| b.0).collect(),
            frames,
        })
    }
}

impl Timeline {
    /// Retained samples for one upload. Runtime interpolation runs after skinning on the GPU.
    pub(super) fn gpu_samples(&self) -> (&[usize], Vec<[f32; 4]>) {
        (
            &self.vertices,
            self.frames
                .iter()
                .flatten()
                .flat_map(|point| {
                    [point.position, point.normal, point.tangent].map(|v| [v[0], v[1], v[2], 0.0])
                })
                .collect(),
        )
    }

    pub(super) fn sample_time(&self, seconds: f32) -> (usize, usize, f32) {
        let duration = self.duration();
        let seconds = if !seconds.is_finite() || duration <= 0.0 {
            0.0
        } else if (0.0..=duration).contains(&seconds) {
            seconds
        } else {
            seconds.rem_euclid(duration)
        };
        let time = seconds * 60.0;
        let first = (time.floor() as usize).min(self.frames.len() - 1);
        (first, (first + 1).min(self.frames.len() - 1), time.fract())
    }

    pub fn duration(&self) -> f32 {
        (self.frames.len() - 1) as f32 / 60.
    }
    pub fn offset(&mut self, offset: usize) {
        for index in &mut self.vertices {
            *index += offset;
        }
    }
    pub fn apply(&self, seconds: f32, result: &mut animation::Deformed) {
        let duration = self.duration();
        let seconds = if !seconds.is_finite() {
            0.
        } else if (0.0..=duration).contains(&seconds) {
            seconds
        } else {
            seconds.rem_euclid(duration)
        };
        let time = seconds * 60.;
        let first = (time.floor() as usize).min(self.frames.len() - 1);
        let next = (first + 1).min(self.frames.len() - 1);
        for (i, &vertex) in self.vertices.iter().enumerate() {
            let a = self.frames[first][i];
            let b = self.frames[next][i];
            let blend = |a: Vector, b: Vector| add(mul(a, 1. - time.fract()), mul(b, time.fract()));
            result.positions[vertex] = blend(a.position, b.position);
            result.normals[vertex] = blend(a.normal, b.normal);
            result.tangents[vertex][..3].copy_from_slice(&blend(a.tangent, b.tangent));
        }
    }
}

pub(super) fn finish(
    model: &mut Model,
    pending: Vec<(u32, Pending)>,
    cancel: &Load,
) -> Result<Vec<(u32, String)>, String> {
    let mut failed = Vec::new();
    for (tag, pending) in pending {
        match pending.bake(model.animation.as_ref(), cancel) {
            Ok(timeline) => model.cloth.push(timeline),
            Err(error) if cancel.stopped() => return Err(error),
            Err(error) => failed.push((tag, error)),
        }
    }
    if !model.cloth.is_empty() {
        model.notices.push("Cloth plays in a 60 Hz studio simulation with its authored constraints and body colliders. Scene wind and landscape collision are unavailable.".into());
    }
    Ok(failed)
}

fn transforms(
    graph: &Graph,
    animation: Option<&animation::Animation>,
    seconds: f32,
) -> Result<Vec<Vec<Matrix>>, String> {
    let Some(animation) = animation else {
        return Ok(graph.transforms.clone());
    };
    if graph.transforms.len() != 1 {
        return Err("Animated cloth requires one owned skeleton transform set".into());
    }
    let matrices = animation.world_matrices(animation.looped_seconds(seconds));
    if matrices.len() != graph.transforms[0].len() {
        return Err("Cloth and animation skeleton counts differ".into());
    }
    for matrix in &matrices {
        inverse_rigid(matrix)?;
    }
    Ok(vec![matrices])
}

struct Runtime {
    buffers: Vec<Vec<Point>>,
    written: Vec<Vec<bool>>,
    transforms: Vec<Vec<Matrix>>,
    colliders: Vec<Vec<Matrix>>,
}
impl Runtime {
    fn new(graph: &Graph, animation: Option<&animation::Animation>) -> Result<Self, String> {
        let transforms = transforms(graph, animation, 0.)?;
        let colliders = graph
            .simulations
            .iter()
            .map(|s| {
                s.as_ref()
                    .map_or_else(Vec::new, |s| s.collisions.initial(&transforms))
            })
            .collect();
        Ok(Self {
            buffers: graph
                .buffers
                .iter()
                .map(|b| vec![Point::default(); b.count])
                .collect(),
            written: graph.buffers.iter().map(|b| vec![false; b.count]).collect(),
            transforms,
            colliders,
        })
    }
    fn execute(&mut self, operators: &[usize], graph: &Graph) -> Result<(), String> {
        for &index in operators {
            let operator = graph.operators[index]
                .as_ref()
                .ok_or("Missing prepared cloth operator")?;
            match operator {
                Operator::Skin(skin) => {
                    skin.apply(&self.transforms, &mut self.buffers, &mut self.written)
                }
                Operator::Gather {
                    input,
                    output,
                    pairs,
                    normals,
                } => {
                    for &(source, target) in pairs {
                        if !self.written[*input][source] {
                            return Err("Cloth gather reads an uninitialized vertex".into());
                        }
                        let p = self.buffers[*input][source];
                        self.buffers[*output][target].position = p.position;
                        if *normals {
                            self.buffers[*output][target].normal = p.normal;
                        }
                        self.written[*output][target] = true;
                    }
                }
                Operator::Move {
                    simulation,
                    reference,
                    pairs,
                } => {
                    let sim = graph.simulations[*simulation].as_ref().unwrap();
                    for &(source, target) in pairs {
                        if !self.written[*reference][source] {
                            return Err("Cloth attachment reads an uninitialized vertex".into());
                        }
                        // At the explicit studio scheduling scales (1, 1), the native
                        // MoveParticles consumer resets both positions to the anchor.
                        let position = self.buffers[*reference][source].position;
                        self.buffers[sim.previous][target].position = position;
                        self.buffers[sim.current][target].position = position;
                    }
                }
                Operator::Simulate {
                    simulation,
                    substeps,
                    iterations,
                    order,
                } => {
                    let sim = graph.simulations[*simulation].as_ref().unwrap();
                    if self.written[sim.current]
                        .iter()
                        .chain(&self.written[sim.previous])
                        .any(|v| !*v)
                    {
                        return Err("Cloth simulation reads uninitialized particles".into());
                    }
                    sim.step(
                        &mut self.buffers,
                        &self.transforms,
                        &mut self.colliders[*simulation],
                        *substeps,
                        *iterations,
                        order,
                    );
                }
                Operator::Frames(frames) => frames.apply(&mut self.buffers),
            }
        }
        Ok(())
    }
}
