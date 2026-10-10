//! Shadowkeep reflection layouts, restricted to the operators in the chosen state.
use super::{constraint::Simulation, frame::Frames, math::*, pack::Pack, skin::Skin};
use std::collections::BTreeSet;

pub(super) const MAX_PARTICLES: usize = 16_384;

pub(super) struct Graph {
    pub buffers: Vec<Buffer>,
    pub transforms: Vec<Vec<Matrix>>,
    pub simulations: Vec<Option<Simulation>>,
    pub operators: Vec<Option<Operator>>,
    pub initialize: Vec<usize>,
    pub update: Vec<usize>,
}

pub(super) struct Buffer {
    pub count: usize,
    pub kind: usize,
    pub subtype: usize,
}

pub(super) enum Operator {
    Skin(Skin),
    Gather {
        input: usize,
        output: usize,
        pairs: Vec<(usize, usize)>,
        normals: bool,
    },
    Move {
        simulation: usize,
        reference: usize,
        pairs: Vec<(usize, usize)>,
    },
    Simulate {
        simulation: usize,
        substeps: usize,
        iterations: usize,
        order: Vec<i32>,
    },
    Frames(Frames),
}

impl Graph {
    pub fn read(bytes: &[u8], states: [usize; 4]) -> Result<Self, String> {
        let p = Pack::read(bytes)?;
        let datas = p.objects(p.root + 0x20, 8)?;
        if datas.len() != 1 {
            return Err("Cloth playback requires one cloth definition".into());
        }
        let data = datas[0];
        p.expect(data, "hclClothData", 0x80)?;
        if !p.objects(data + 0x68, 128)?.is_empty() {
            return Err("Cloth actions require an unsupported scene host".into());
        }
        let mut buffers = Vec::new();
        for at in p.objects(data + 0x28, 64)? {
            p.expect(at, "hclBufferDefinition", 0x48)?;
            let count = p.u32(at + 0x20)?;
            let kind = p.u32(at + 0x18)?;
            if count == 0 || count > MAX_PARTICLES || !matches!(kind, 1 | 2 | 4) {
                return Err("Unsupported cloth buffer definition".into());
            }
            buffers.push(Buffer {
                count,
                kind,
                subtype: p.u32(at + 0x1c)?,
            });
        }
        let transforms = p
            .objects(data + 0x38, 8)?
            .into_iter()
            .map(|at| {
                p.expect(at, "hclTransformSetDefinition", 0x20)?;
                let count = p.u32(at + 0x1c)?;
                if count == 0 || count > 512 || p.u32(at + 0x18)? != 1 {
                    return Err("Unsupported cloth transform set".into());
                }
                Ok(vec![identity(); count])
            })
            .collect::<Result<Vec<_>, String>>()?;
        let operator_refs = p.objects(data + 0x48, 256)?;
        let state_refs = p.objects(data + 0x58, 64)?;
        let state = |index: usize| -> Result<Vec<usize>, String> {
            let at = *state_refs.get(index).ok_or("Cloth state is missing")?;
            p.expect(at, "hclClothState", 0x58)?;
            p.array(at + 0x18, 4, 256)?
                .into_iter()
                .map(|row| {
                    let value = p.u32(row)?;
                    if value >= operator_refs.len() {
                        return Err("Cloth state references a missing operator".into());
                    }
                    Ok(value)
                })
                .collect()
        };
        let update = state(states[0])?;
        let mut initialize = Vec::new();
        let mut seen = BTreeSet::new();
        for index in states.into_iter().skip(1) {
            if !seen.insert(index) {
                continue;
            }
            let ops = state(index)?;
            // Roles 2 and 3 initialize current and previous particle buffers in native
            // owners. A reused simulation state is not an initialization operation.
            if ops.iter().any(|i| {
                p.class(operator_refs[*i])
                    .is_ok_and(|c| c == "hclSimulateOperator")
            }) {
                continue;
            }
            initialize.extend(ops);
        }
        let needed: BTreeSet<_> = initialize.iter().chain(&update).copied().collect();
        let sim_refs = p.objects(data + 0x18, 16)?;
        let mut result = Self {
            buffers,
            transforms,
            simulations: (0..sim_refs.len()).map(|_| None).collect(),
            operators: (0..operator_refs.len()).map(|_| None).collect(),
            initialize,
            update,
        };
        for index in needed {
            let at = operator_refs[index];
            let operator = result.operator(&p, at, &sim_refs)?;
            result.operators[index] = Some(operator);
        }
        if !result
            .update
            .iter()
            .any(|i| matches!(result.operators[*i], Some(Operator::Simulate { .. })))
        {
            return Err("The selected cloth state has no simulation".into());
        }
        // Bind fixtures use the inverse of each skin operator's stored inverse bind.
        // A decoded animation replaces these with its owner's world transforms.
        for operator in result.operators.iter().flatten() {
            if let Operator::Skin(skin) = operator {
                for (&bone, bind) in skin.subset.iter().zip(&skin.binds) {
                    result.transforms[skin.set][bone] = inverse_rigid(bind)?;
                }
            }
        }
        Ok(result)
    }

    pub fn buffer(&self, index: usize) -> Result<usize, String> {
        self.buffers
            .get(index)
            .map(|b| b.count)
            .ok_or_else(|| "Missing cloth buffer".into())
    }

    fn simulation(&mut self, p: &Pack<'_>, index: usize, refs: &[usize]) -> Result<(), String> {
        let at = *refs.get(index).ok_or("Missing cloth simulation")?;
        if self.simulations[index].is_none() {
            self.simulations[index] = Some(Simulation::read(p, at, index, self)?);
        }
        Ok(())
    }

    fn operator(
        &mut self,
        p: &Pack<'_>,
        at: usize,
        sim_refs: &[usize],
    ) -> Result<Operator, String> {
        Ok(match p.class(at)? {
            "hclObjectSpaceSkinPNTOperator" => Operator::Skin(Skin::read(p, at, self)?),
            "hclGatherAllVerticesOperator" => {
                p.expect(at, "hclGatherAllVerticesOperator", 0x40)?;
                let input = p.u32(at + 0x30)?;
                let output = p.u32(at + 0x34)?;
                let input_count = self.buffer(input)?;
                let output_count = self.buffer(output)?;
                let rows = p.array(at + 0x20, 2, output_count)?;
                if p.u8(at + 0x39)? == 0 && rows.len() != output_count {
                    return Err("Incomplete cloth gather".into());
                }
                let mut pairs = Vec::new();
                for (out, row) in rows.into_iter().enumerate() {
                    let value = p.u16(row)? as u16 as i16;
                    if value < 0 {
                        continue;
                    }
                    if value as usize >= input_count {
                        return Err("Cloth gather exceeds its input".into());
                    }
                    pairs.push((value as usize, out));
                }
                Operator::Gather {
                    input,
                    output,
                    pairs,
                    normals: p.u8(at + 0x38)? != 0,
                }
            }
            "hclCopyVerticesOperator" => {
                p.expect(at, "hclCopyVerticesOperator", 0x38)?;
                let input = p.u32(at + 0x20)?;
                let output = p.u32(at + 0x24)?;
                let count = p.u32(at + 0x28)?;
                let first = p.u32(at + 0x2c)?;
                let target = p.u32(at + 0x30)?;
                if first + count > self.buffer(input)? || target + count > self.buffer(output)? {
                    return Err("Cloth copy exceeds its buffer".into());
                }
                Operator::Gather {
                    input,
                    output,
                    pairs: (0..count).map(|i| (first + i, target + i)).collect(),
                    normals: p.u8(at + 0x34)? != 0,
                }
            }
            "hclMoveParticlesOperator" => {
                p.expect(at, "hclMoveParticlesOperator", 0x38)?;
                let simulation = p.u32(at + 0x30)?;
                let reference = p.u32(at + 0x34)?;
                self.simulation(p, simulation, sim_refs)?;
                let count = self.simulations[simulation]
                    .as_ref()
                    .unwrap()
                    .particles
                    .len();
                let vertices = self.buffer(reference)?;
                let pairs = p
                    .array(at + 0x20, 4, count)?
                    .into_iter()
                    .map(|row| {
                        let pair = (p.u16(row)?, p.u16(row + 2)?);
                        if pair.0 >= vertices || pair.1 >= count {
                            return Err("Cloth attachment exceeds its buffer".into());
                        }
                        Ok(pair)
                    })
                    .collect::<Result<_, String>>()?;
                Operator::Move {
                    simulation,
                    reference,
                    pairs,
                }
            }
            "hclSimulateOperator" => {
                p.expect(at, "hclSimulateOperator", 0x50)?;
                let simulation = p.u32(at + 0x20)?;
                self.simulation(p, simulation, sim_refs)?;
                let substeps = p.u32(at + 0x24)?;
                let iterations = p.u32(at + 0x28)?;
                if !(1..=16).contains(&substeps) || !(1..=16).contains(&iterations) {
                    return Err("Cloth solve exceeds the preview iteration budget".into());
                }
                let constraints = self.simulations[simulation]
                    .as_ref()
                    .unwrap()
                    .constraints
                    .len();
                let order = p
                    .array(at + 0x30, 4, 512)?
                    .into_iter()
                    .map(|row| {
                        let index = p.u32(row)? as u32 as i32;
                        if index < -1 || index >= constraints as i32 {
                            return Err("Missing cloth constraint".into());
                        }
                        Ok(index)
                    })
                    .collect::<Result<_, String>>()?;
                Operator::Simulate {
                    simulation,
                    substeps,
                    iterations,
                    order,
                }
            }
            "hclUpdateSomeVertexFramesOperator" => Operator::Frames(Frames::read(p, at, self)?),
            class => {
                return Err(format!(
                    "Cloth operator {class} is not supported by playback"
                ));
            }
        })
    }
}
