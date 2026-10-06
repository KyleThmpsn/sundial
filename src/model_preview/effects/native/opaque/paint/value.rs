//! Versioned operands for the bounded painted material recipe.
use super::*;

#[derive(Clone)]
pub(super) struct Value {
    pub operand: Operand,
    pub before: usize,
}

pub(super) struct Node<'a> {
    pub instruction: &'a Instruction,
    pub at: usize,
    lanes: [usize; 3],
}

impl Value {
    pub fn new(operand: &Operand, before: usize) -> Self {
        Self {
            operand: operand.clone(),
            before,
        }
    }
    pub fn scalar(operand: &Operand, lane: usize, before: usize) -> Self {
        let mut value = Self::new(operand, before);
        value.operand.lanes = [operand.lanes[lane]; 4];
        value
    }
    pub fn node<'a>(&self, code: &'a Code) -> Option<Node<'a>> {
        if self.operand.modifier != 0 {
            return None;
        }
        let at = gain::rgb_producer(code, &self.operand, self.before)?;
        Some(Node {
            instruction: &code.instructions[at],
            at,
            lanes: self.operand.lanes[..3].try_into().ok()?,
        })
    }
    pub fn positive(&self) -> Option<Self> {
        if self.operand.modifier != 1 {
            return None;
        }
        let mut positive = self.clone();
        positive.operand.modifier = 0;
        Some(positive)
    }
    pub fn literal(&self, number: f32) -> bool {
        (0..3).all(|lane| literal(&self.operand, lane, number))
    }
    pub fn same(&self, code: &Code, other: &Self) -> bool {
        (0..3).all(|lane| {
            read_eq(&self.operand, lane, &other.operand, lane)
                && (self.operand.kind != 0
                    || gain::writer(code, &self.operand, lane, self.before)
                        == gain::writer(code, &other.operand, lane, other.before))
        })
    }
    pub fn constant(&self, code: &Code) -> Option<Constant> {
        let operand = &self.operand;
        if operand.kind == 8
            && operand.modifier == 0
            && operand.indices.len() == 2
            && operand.indices[0].base == 0
            && operand.indices.iter().all(|i| i.relative.is_none())
            && operand.lanes[..3]
                .iter()
                .all(|&lane| lane == operand.lanes[0])
        {
            return Some(Constant {
                row: operand.indices[1].base as usize,
                lane: operand.lanes[0],
            });
        }
        let node = self.node(code)?;
        if node.is(54, false) {
            node.arg(1).direct_constant()
        } else {
            None
        }
    }
    fn direct_constant(&self) -> Option<Constant> {
        // A MOV alias is allowed once. This never follows an unbounded producer chain.
        let operand = &self.operand;
        (operand.kind == 8
            && operand.modifier == 0
            && operand.indices.len() == 2
            && operand.indices[0].base == 0
            && operand.indices.iter().all(|i| i.relative.is_none())
            && operand.lanes[..3]
                .iter()
                .all(|&lane| lane == operand.lanes[0]))
        .then(|| Constant {
            row: operand.indices[1].base as usize,
            lane: operand.lanes[0],
        })
    }
    pub fn bank(&self, code: &Code) -> Option<Bank> {
        let mut value = self.clone();
        let saturated = if value.operand.kind == 0 {
            let node = value.node(code)?;
            if !node.is(54, true) {
                return None;
            }
            value = node.arg(1);
            true
        } else {
            false
        };
        let operand = &value.operand;
        if operand.kind != 8
            || operand.modifier != 0
            || operand.indices.len() != 2
            || operand.indices[0].base != 0
            || operand.indices[0].relative.is_some()
        {
            return None;
        }
        let relative = operand.indices[1].relative.as_ref()?;
        let address = Value::scalar(relative, 0, value.before).node(code)?;
        if !address.is(28, false) {
            return None;
        }
        let sum = address.arg(1).node(code)?;
        if !sum.is(0, false) {
            return None;
        }
        let (root, bias) = if sum.arg(1).operand.kind == 4 {
            (sum.arg(2), sum.arg(1))
        } else {
            (sum.arg(1), sum.arg(2))
        };
        let bias_bits = bias.operand.literal[bias.operand.lanes[0]];
        let bias_number = f32::from_bits(bias_bits);
        if !bias.literal(bias_number)
            || !bias_number.is_finite()
            || !(0.0..=128.0).contains(&bias_number)
            || bias_number.fract() != 0.0
        {
            return None;
        }
        let root_node = root.node(code)?;
        if !stride(&root_node, code) {
            return None;
        }
        Some(Bank {
            root: Root {
                register: root.operand.indices[0].base,
                lane: root.operand.lanes[0],
                at: root_node.at,
            },
            row: operand.indices[1].base.checked_add(bias_number as u32)? as usize,
            lanes: operand.lanes[..3].try_into().ok()?,
            saturated,
        })
    }
}

impl Node<'_> {
    pub fn is(&self, opcode: u16, saturated: bool) -> bool {
        self.instruction.code == opcode && self.instruction.saturate == saturated
    }
    pub fn arg(&self, index: usize) -> Value {
        let mut operand = self.instruction.operands[index].clone();
        operand.lanes = [
            operand.lanes[self.lanes[0]],
            operand.lanes[self.lanes[1]],
            operand.lanes[self.lanes[2]],
            0,
        ];
        Value {
            operand,
            before: self.at,
        }
    }
}

fn stride(node: &Node<'_>, code: &Code) -> bool {
    if node.is(56, false) {
        return node.arg(1).literal(9.0) || node.arg(2).literal(9.0);
    }
    if !node.is(43, false) {
        return false;
    }
    let Some(integer) = node.arg(1).node(code) else {
        return false;
    };
    if !integer.is(38, false) {
        return false;
    }
    let low = &integer.instruction.operands[1];
    // IMUL has separate high and low destinations. Only its low product is a bank offset.
    low.kind == 0
        && low.indices[0].base == node.arg(1).operand.indices[0].base
        && low.mask & (1 << node.arg(1).operand.lanes[0]) != 0
        && [2, 3].into_iter().any(|arg| {
            let v = integer.arg(arg);
            v.operand.kind == 4
                && v.operand.modifier == 0
                && (0..3).all(|lane| v.operand.literal[v.operand.lanes[lane]] == 9)
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Constant {
    pub row: usize,
    pub lane: usize,
}
impl Constant {
    pub fn valid(self, constants: &[[f32; 4]]) -> bool {
        self.row < constants.len() && self.row < 128
    }
    pub fn value(self, frame: &Frame) -> f32 {
        frame[self.row][self.lane]
    }
    pub fn glsl(self) -> String {
        format!(
            "nConstant(0,{}).{}",
            self.row,
            ['x', 'y', 'z', 'w'][self.lane]
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Root {
    register: u32,
    lane: usize,
    at: usize,
}
#[derive(Clone, Copy)]
pub(super) struct Bank {
    root: Root,
    pub row: usize,
    lanes: [usize; 3],
    saturated: bool,
}
impl Bank {
    pub fn matches(self, base: Self, offset: usize, lanes: [usize; 3], saturated: bool) -> bool {
        self.root == base.root
            && self.row == base.row + offset
            && self.lanes == lanes
            && self.saturated == saturated
    }
    pub fn rgb(self) -> bool {
        self.lanes == [0, 1, 2] && !self.saturated
    }
}
