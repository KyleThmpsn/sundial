//! Recover numeric stored varyings without executing camera or skeleton palette code.
//! Unsupported dependencies stay unavailable. The bounded DAG evaluates only outputs used
//! for detail UVs and vertex color, with native vertex identity before mesh concatenation.
use super::*;
use program::{Instruction, Operand, Program as Code};
mod scaled;
mod table;
use table::Table;

type Value = Option<usize>;
#[derive(Clone)]
struct State {
    temps: Vec<[Value; 4]>,
    outputs: [[Value; 4]; 16],
}
enum Node {
    Literal(u32),
    Input(usize, usize),
    Operation(u16, Vec<usize>, bool),
    Select(usize, Value, Value, bool),
    Load(usize, [usize; 4], usize, [i32; 3]),
    Divide(usize, usize, bool),
}
struct Branch {
    condition: Value,
    nonzero: bool,
    before: State,
    yes: Option<State>,
}

pub(in crate::model_preview) struct Attributes {
    nodes: Vec<Node>,
    inputs: Vec<program::Semantic>,
    tables: std::collections::BTreeMap<usize, Table>,
    detail: Option<[Value; 2]>,
    color: Option<[Value; 4]>,
    scaled_detail: bool,
}

pub(in crate::model_preview) fn load(
    manager: &PackageManager,
    tag: u32,
) -> Result<Option<Attributes>, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    let tag = u32_at(&bytes, 0x48)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, tag, 1)?;
    let Ok(code) = Code::read_stored(&raw) else {
        return Ok(None);
    };
    let scaled_detail = scaled::recover(&code);
    if !scaled_detail && !motion::stored(&code) {
        return Ok(None);
    }
    let mut result = Attributes {
        nodes: Vec::new(),
        inputs: code.inputs.clone(),
        tables: Default::default(),
        detail: None,
        color: None,
        scaled_detail,
    };
    if scaled_detail {
        return Ok(Some(result));
    }
    let mut state = State {
        temps: vec![[None; 4]; code.temps],
        outputs: [[None; 4]; 16],
    };
    let mut branches = Vec::<Branch>::new();
    for instruction in &code.instructions {
        match instruction.code {
            31 => {
                let condition = result.read(&state, &instruction.operands[0], 0);
                branches.push(Branch {
                    condition,
                    nonzero: instruction.nonzero,
                    before: state.clone(),
                    yes: None,
                });
            }
            18 => {
                let branch = branches.last_mut().ok_or("Invalid attribute branch")?;
                branch.yes = Some(state);
                state = branch.before.clone();
            }
            21 => {
                let branch = branches.pop().ok_or("Invalid attribute branch")?;
                let (yes, no) = match branch.yes {
                    Some(yes) => (yes, state),
                    None => (state, branch.before),
                };
                state = result.merge(yes, no, branch.condition, branch.nonzero);
            }
            62 => break,
            _ => result.write(&mut state, instruction),
        }
    }
    for semantic in &code.outputs {
        if semantic.name != "TEXCOORD" {
            continue;
        }
        let values = state.outputs[semantic.register];
        match semantic.index {
            3 if values[2].is_some() || values[3].is_some() => {
                result.detail = Some([values[2], values[3]])
            }
            8 => result.color = Some(values),
            _ => {}
        }
    }
    // Load only tables reachable from supported attribute outputs. Palette-only resources
    // do not impose requirements on detail/color recovery.
    let mut reachable = std::collections::BTreeSet::new();
    let mut pending: Vec<_> = result
        .detail
        .iter()
        .flatten()
        .chain(result.color.iter().flatten())
        .flatten()
        .copied()
        .collect();
    while let Some(node) = pending.pop() {
        if !reachable.insert(node) {
            continue;
        }
        match &result.nodes[node] {
            Node::Operation(_, args, _) => pending.extend(args),
            Node::Select(c, yes, no, _) => {
                pending.push(*c);
                pending.extend(yes);
                pending.extend(no);
            }
            Node::Load(_, coordinates, _, _) => pending.extend(coordinates),
            Node::Divide(numerator, divisor, _) => pending.extend([numerator, divisor]),
            _ => {}
        }
    }
    let (count, rows) = super::super::vertex::table(&bytes, 0x50, 0x8080_7211, 8, 32)?;
    let mut bindings = std::collections::BTreeMap::new();
    for row in (0..count).map(|i| rows + i * 8) {
        if bindings
            .insert(u32_at(&bytes, row)? as usize, u32_at(&bytes, row + 4)?)
            .is_some()
        {
            return Err("The stored attribute texture slot is bound more than once".into());
        }
    }
    for node in reachable {
        if let Node::Load(slot, _, _, _) = result.nodes[node] {
            if result.tables.contains_key(&slot) {
                continue;
            }
            let resource = code
                .resources
                .iter()
                .find(|r| r.slot == slot)
                .ok_or("Missing attribute resource declaration")?;
            let tag = *bindings
                .get(&slot)
                .ok_or("The stored attribute texture is missing")?;
            result
                .tables
                .insert(slot, Table::load(manager, tag, resource.integer)?);
        }
    }
    Ok(Some(result))
}

impl Attributes {
    pub fn has_detail(&self) -> bool {
        self.detail.is_some()
    }
    pub fn scaled_detail(&self) -> bool {
        self.scaled_detail
    }
    fn node(&mut self, node: Node) -> Value {
        // Each instruction writes at most four scalar results plus input/literal nodes.
        if self.nodes.len() >= 32768 {
            return None;
        }
        self.nodes.push(node);
        Some(self.nodes.len() - 1)
    }
    fn read(&mut self, state: &State, operand: &Operand, lane: usize) -> Value {
        if operand.indices.iter().any(|i| i.relative.is_some()) {
            return None;
        }
        let lane = operand.lanes[lane];
        let register = operand.indices.first().map_or(0, |i| i.base as usize);
        let value = match operand.kind {
            0 => state.temps.get(register)?[lane],
            2 => state.outputs.get(register)?[lane],
            1 => self.node(Node::Input(register, lane)),
            4 => self.node(Node::Literal(operand.literal[lane])),
            // Only the validated identity UV transform is a constant in the imported
            // envelope. Other scene, animated material and palette buffers are unavailable.
            8 if register == 11 && operand.indices.get(1)?.base == 6 => self.node(Node::Literal(
                (if lane < 2 { 1.0f32 } else { 0.0 }).to_bits(),
            )),
            _ => None,
        };
        match operand.modifier {
            0 => value,
            1..=3 => self.node(Node::Operation(
                1000 + u16::from(operand.modifier),
                vec![value?],
                false,
            )),
            _ => None,
        }
    }
    fn merge(&mut self, yes: State, no: State, condition: Value, nonzero: bool) -> State {
        let mut result = no.clone();
        for (dest, (yes, no)) in result.temps.iter_mut().chain(&mut result.outputs).zip(
            yes.temps
                .iter()
                .chain(&yes.outputs)
                .zip(no.temps.iter().chain(&no.outputs)),
        ) {
            for lane in 0..4 {
                dest[lane] = if yes[lane] == no[lane] {
                    yes[lane]
                } else {
                    condition.and_then(|c| self.node(Node::Select(c, yes[lane], no[lane], nonzero)))
                };
            }
        }
        result
    }
    fn write(&mut self, state: &mut State, instruction: &Instruction) {
        let destinations = match instruction.code {
            13 => 0,
            38 | 77 | 78 => 2,
            _ => 1,
        };
        let mut writes = Vec::new();
        for (result, dest) in instruction.operands.iter().take(destinations).enumerate() {
            if !matches!(dest.kind, 0 | 2) {
                continue;
            }
            let register = dest.indices[0].base as usize;
            for lane in 0..4 {
                if dest.mask & (1 << lane) == 0 {
                    continue;
                }
                let value = if instruction.code == 78 {
                    self.divide(state, instruction, lane, result != 0)
                } else {
                    self.operation(state, instruction, lane, destinations)
                };
                writes.push((dest.kind, register, lane, value));
            }
        }
        for (kind, register, lane, value) in writes {
            if let Some(dest) = if kind == 0 {
                state.temps.get_mut(register)
            } else {
                state.outputs.get_mut(register)
            } {
                dest[lane] = value;
            }
        }
    }
    fn divide(&mut self, state: &State, i: &Instruction, lane: usize, remainder: bool) -> Value {
        let numerator = self.read(state, &i.operands[2], lane)?;
        let divisor = self.read(state, &i.operands[3], lane)?;
        self.node(Node::Divide(numerator, divisor, remainder))
    }
    fn operation(
        &mut self,
        state: &State,
        i: &Instruction,
        lane: usize,
        destinations: usize,
    ) -> Value {
        if i.code == 45 && i.operands.len() == 3 && !i.saturate {
            let resource = &i.operands[2];
            if resource.kind != 7 || resource.modifier != 0 {
                return None;
            }
            let mut coordinates = [0; 4];
            for (lane, dest) in coordinates.iter_mut().enumerate() {
                *dest = self.read(state, &i.operands[1], lane)?;
            }
            return self.node(Node::Load(
                resource.indices[0].base as usize,
                coordinates,
                resource.lanes[lane],
                i.offset,
            ));
        }
        if i.code == 55 {
            let condition = self.read(state, &i.operands[1], lane)?;
            let yes = self.read(state, &i.operands[2], lane);
            let no = self.read(state, &i.operands[3], lane);
            return self.node(Node::Select(condition, yes, no, true));
        }
        if !matches!(i.code, 0 | 1 | 25..=28 | 29 | 30 | 32..=35 | 36 | 37 | 39 | 40 | 41 | 42 | 43 | 49..=52 | 54 | 56 | 57 | 59 | 60 | 64..=68 | 80 | 85 | 86 | 131 | 139 | 140)
        {
            return None;
        }
        let args: Option<Vec<_>> = i.operands[destinations..]
            .iter()
            .map(|operand| self.read(state, operand, lane))
            .collect();
        self.node(Node::Operation(i.code, args?, i.saturate))
    }

    pub fn apply(
        &self,
        vertices: &mut crate::model_preview::vertex::Vertices,
        identity: &[u32],
    ) -> Result<(), String> {
        if identity.len() != vertices.positions.len() {
            return Err("Stored vertex identities do not match the geometry".into());
        }
        if self.scaled_detail {
            return if vertices.has_detail_scales {
                Ok(())
            } else {
                Err("The native detail formula requires auxiliary texture coordinates".into())
            };
        }
        let mut detail = Vec::with_capacity(vertices.positions.len());
        let mut colors = Vec::with_capacity(vertices.positions.len());
        let mut cache = vec![None; self.nodes.len()];
        for (index, &vertex_id) in identity.iter().enumerate() {
            let mut inputs = [[None; 4]; 16];
            for semantic in &self.inputs {
                let (value, mask) = match semantic.name.as_str() {
                    _ if semantic.system == 6 => {
                        inputs[semantic.register][0] = Some(vertex_id);
                        continue;
                    }
                    "POSITION" if semantic.index == 0 => (vertices.positions[index], 15),
                    "NORMAL" if semantic.index == 0 => {
                        let n = vertices.normals[index];
                        ([n[0], n[1], n[2], 0.0], 7)
                    }
                    "TANGENT" if semantic.index == 0 => (vertices.tangents[index], 15),
                    "COLOR" if semantic.index == 0 => (vertices.colors[index], 15),
                    "TEXCOORD" if semantic.index == 0 && vertices.has_uv => {
                        let uv = vertices.uvs[index];
                        ([uv[0], uv[1], 0.0, 0.0], 3)
                    }
                    _ => ([0.0; 4], 0),
                };
                inputs[semantic.register] = std::array::from_fn(|lane| {
                    (mask & (1 << lane) != 0).then(|| value[lane].to_bits())
                });
            }
            cache.fill(None);
            let mut evaluate = |node: Value| -> Result<f32, String> {
                let value = f32::from_bits(self.evaluate(
                    node.ok_or("The stored attribute dependency is unavailable")?,
                    &inputs,
                    &mut cache,
                    0,
                )?);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err("The stored attribute is not finite".into())
                }
            };
            if let Some(nodes) = self.detail {
                detail.push([evaluate(nodes[0])?, evaluate(nodes[1])?]);
            }
            if let Some(nodes) = self.color {
                colors.push([
                    evaluate(nodes[0])?,
                    evaluate(nodes[1])?,
                    evaluate(nodes[2])?,
                    evaluate(nodes[3])?,
                ]);
            }
        }
        // Commit all values together. A bad row cannot leave half the mesh recolored.
        if self.detail.is_some() {
            vertices.detail_uvs = detail;
        }
        if self.color.is_some() {
            vertices.colors = colors;
        }
        Ok(())
    }

    fn evaluate(
        &self,
        index: usize,
        inputs: &[[Option<u32>; 4]; 16],
        cache: &mut [Option<u32>],
        depth: usize,
    ) -> Result<u32, String> {
        if depth > 128 {
            return Err("The stored attribute dependency exceeds limits".into());
        }
        if let Some(value) = cache[index] {
            return Ok(value);
        }
        let value = match &self.nodes[index] {
            Node::Literal(value) => *value,
            Node::Input(register, lane) => {
                inputs[*register][*lane].ok_or("The stored attribute input lane is unavailable")?
            }
            Node::Select(condition, yes, no, nonzero) => {
                let active =
                    (self.evaluate(*condition, inputs, cache, depth + 1)? != 0) == *nonzero;
                self.evaluate(
                    (if active { yes } else { no })
                        .ok_or("The selected stored attribute dependency is unavailable")?,
                    inputs,
                    cache,
                    depth + 1,
                )?
            }
            Node::Load(slot, nodes, lane, offset) => {
                let mut coordinates = [0i32; 4];
                for i in 0..4 {
                    coordinates[i] = self.evaluate(nodes[i], inputs, cache, depth + 1)? as i32;
                }
                self.tables
                    .get(slot)
                    .ok_or("The stored attribute texture is unavailable")?
                    .texel(coordinates, *offset)?[*lane]
            }
            Node::Operation(code, nodes, saturate) => {
                let args: Result<Vec<_>, _> = nodes
                    .iter()
                    .map(|node| self.evaluate(*node, inputs, cache, depth + 1))
                    .collect();
                numeric(*code, &args?, *saturate)
                    .ok_or("The stored attribute operation is unavailable")?
            }
            Node::Divide(numerator, divisor, remainder) => {
                let numerator = self.evaluate(*numerator, inputs, cache, depth + 1)?;
                let divisor = self.evaluate(*divisor, inputs, cache, depth + 1)?;
                if *remainder {
                    numerator.checked_rem(divisor)
                } else {
                    numerator.checked_div(divisor)
                }
                .unwrap_or(u32::MAX)
            }
        };
        cache[index] = Some(value);
        Ok(value)
    }
}

fn numeric(code: u16, a: &[u32], saturate: bool) -> Option<u32> {
    let f = |i: usize| f32::from_bits(a[i]);
    let float = match code {
        0 => Some(f(0) + f(1)),
        25 => Some(f(0).exp2()),
        26 => Some(f(0) - f(0).floor()),
        27 => return Some(f(0) as i32 as u32),
        28 => return Some(f(0).max(0.0) as u32),
        43 => return Some((a[0] as i32 as f32).to_bits()),
        50 => Some(f(0) * f(1) + f(2)),
        51 => Some(f(0).min(f(1))),
        52 => Some(f(0).max(f(1))),
        54 if !saturate => return Some(a[0]),
        54 => Some(f(0)),
        56 => Some(f(0) * f(1)),
        64 => Some(f(0).round_ties_even()),
        65 => Some(f(0).floor()),
        66 => Some(f(0).ceil()),
        67 => Some(f(0).trunc()),
        68 => Some(1.0 / f(0).sqrt()),
        86 => return Some((a[0] as f32).to_bits()),
        131 => Some(crate::model_preview::vertex::half(a[0] as u16)),
        1001 => return Some(a[0] ^ 0x8000_0000),
        1002 => return Some(a[0] & 0x7FFF_FFFF),
        1003 => return Some(a[0] | 0x8000_0000),
        _ => None,
    };
    if let Some(value) = float {
        return Some(
            (if saturate {
                value.clamp(0.0, 1.0)
            } else {
                value
            })
            .to_bits(),
        );
    }
    let yes = |value| if value { u32::MAX } else { 0 };
    Some(match code {
        1 => a[0] & a[1],
        29 => yes(f(0) >= f(1)),
        30 => a[0].wrapping_add(a[1]),
        32 => yes(a[0] == a[1]),
        33 => yes(a[0] as i32 >= a[1] as i32),
        34 => yes((a[0] as i32) < a[1] as i32),
        35 => a[0].wrapping_mul(a[1]).wrapping_add(a[2]),
        36 => (a[0] as i32).max(a[1] as i32) as u32,
        37 => (a[0] as i32).min(a[1] as i32) as u32,
        39 => yes(a[0] != a[1]),
        41 => a[0].wrapping_shl(a[1] & 31),
        42 => (a[0] as i32).wrapping_shr(a[1] & 31) as u32,
        49 => yes(f(0) < f(1)),
        57 => yes(f(0) != f(1)),
        59 => !a[0],
        60 => a[0] | a[1],
        80 => yes(a[0] >= a[1]),
        85 => a[0].wrapping_shr(a[1] & 31),
        139 => {
            let width = a[0] & 31;
            if width == 0 {
                0
            } else {
                (((a[2].wrapping_shr(a[1] & 31) << (32 - width)) as i32) >> (32 - width)) as u32
            }
        }
        140 => {
            let width = a[0] & 31;
            let offset = a[1] & 31;
            let mask = if width == 0 {
                0
            } else {
                (u32::MAX >> (32 - width)).wrapping_shl(offset)
            };
            (a[3] & !mask) | (a[2].wrapping_shl(offset) & mask)
        }
        _ => return None,
    })
}
