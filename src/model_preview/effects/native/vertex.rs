use super::*;

pub(in crate::model_preview) type Varyings = [[f32; 4]; 9];

pub(in crate::model_preview) struct Input {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub detail_uv: [f32; 2],
}

impl Input {
    pub(super) fn default_varyings(&self) -> Varyings {
        let n = self.normal;
        let t = self.tangent;
        let b = [
            (n[1] * t[2] - n[2] * t[1]) * t[3],
            (n[2] * t[0] - n[0] * t[2]) * t[3],
            (n[0] * t[1] - n[1] * t[0]) * t[3],
        ];
        [
            [n[0], n[1], n[2], 1.0],
            t,
            [b[0], b[1], b[2], 0.0],
            [self.uv[0], self.uv[1], self.detail_uv[0], self.detail_uv[1]],
            [self.position[0], self.position[1], self.position[2], 1.0],
            self.color,
            [0.0; 4],
            [0.0; 4],
            self.color,
        ]
    }
}

impl Native {
    pub(in crate::model_preview) fn vertex(
        &self,
        input: &Input,
        constants: &Frame,
    ) -> Option<Varyings> {
        let Some(vertex) = &self.vertex else {
            let mut values = input.default_varyings();
            if let Some(uv) = self.opaque_uv {
                values[3][0] = input.uv[0] * uv[0] + uv[2];
                values[3][1] = input.uv[1] * uv[1] + uv[3];
            }
            return Some(values);
        };
        if let Some(uv) = vertex.stored_uv {
            let mut values = input.default_varyings();
            values[3][0] = input.uv[0] * uv[0] + uv[2];
            values[3][1] = input.uv[1] * uv[1] + uv[3];
            return Some(values);
        }
        let has_weights = vertex.code.inputs.iter().any(|s| s.name == "BLENDWEIGHT");
        let mut values = [[0; 4]; 16];
        for semantic in &vertex.code.inputs {
            let value = match semantic.name.as_str() {
                "POSITION" => [input.position[0], input.position[1], input.position[2], 0.0],
                "NORMAL" => [input.normal[0], input.normal[1], input.normal[2], 0.0],
                "TANGENT" => input.tangent,
                "COLOR" => input.color,
                "BLENDWEIGHT" => [1.0, 0.0, 0.0, 0.0],
                "BLENDINDICES" => {
                    values[semantic.register] = if has_weights { [0; 4] } else { [0, 0, 255, 0] };
                    continue;
                }
                "TEXCOORD" if semantic.index == 0 => [input.uv[0], input.uv[1], 0.0, 0.0],
                "TEXCOORD" => [
                    if input.uv[0].abs() > 1e-8 {
                        input.detail_uv[0] / input.uv[0]
                    } else {
                        1.0
                    },
                    if input.uv[1].abs() > 1e-8 {
                        input.detail_uv[1] / input.uv[1]
                    } else {
                        1.0
                    },
                    0.0,
                    0.0,
                ],
                _ => return None,
            };
            values[semantic.register] = value.map(f32::to_bits);
        }
        let outputs = vertex.code.evaluate(&Context {
            values,
            constants,
            quaternion: vertex.quaternion,
        })?;
        let mut varyings = input.default_varyings();
        for semantic in &vertex.code.outputs {
            if semantic.name == "TEXCOORD" && semantic.index < 9 {
                varyings[semantic.index as usize] = outputs[semantic.register];
            }
        }
        varyings
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(varyings)
    }
}

pub(super) fn evaluate(
    code: &program::Program,
    input: &Input,
    constants: &Frame,
) -> Option<[[f32; 4]; 16]> {
    let mut values = [[0; 4]; 16];
    for semantic in &code.inputs {
        let value = match semantic.name.as_str() {
            "POSITION" => [input.position[0], input.position[1], input.position[2], 0.0],
            "NORMAL" => [input.normal[0], input.normal[1], input.normal[2], 0.0],
            "TANGENT" => input.tangent,
            _ => [0.0; 4],
        };
        values[semantic.register] = value.map(f32::to_bits);
    }
    code.evaluate(&Context {
        values,
        constants,
        quaternion: false,
    })
}

struct Context<'a> {
    values: [[u32; 4]; 16],
    constants: &'a Frame,
    quaternion: bool,
}

pub(super) fn constant(
    buffer: usize,
    index: usize,
    quaternion: bool,
    constants: &Frame,
) -> [f32; 4] {
    match (buffer, index) {
        (0, i) => constants.get(i).copied().unwrap_or([0.0; 4]),
        (11, 5) => [0.0, 0.0, 0.0, 1.0],
        (11, 6) => [1.0, 1.0, 0.0, 0.0],
        (11, 7) => [0.0, 0.0, 0.0, 1.0],
        (11, 8) if quaternion => [0.0, 0.0, 0.0, 1.0],
        (11, 9) if quaternion => [0.0; 4],
        (11, 8..=10) => std::array::from_fn(|lane| if lane == index - 8 { 1.0 } else { 0.0 }),
        (12, 0..=3) => std::array::from_fn(|lane| if lane == index { 1.0 } else { 0.0 }),
        _ => [0.0; 4],
    }
}

impl evaluate::Context for Context<'_> {
    fn input(&self, register: usize) -> [u32; 4] {
        self.values.get(register).copied().unwrap_or([0; 4])
    }
    fn constant(&self, buffer: usize, index: usize) -> [u32; 4] {
        constant(buffer, index, self.quaternion, self.constants).map(f32::to_bits)
    }
    fn sample(&self, _: usize, _: usize, _: [f32; 4], _: Option<f32>, _: [i32; 3]) -> [u32; 4] {
        [0; 4]
    }
    fn load(&self, _: usize, _: [i32; 4], _: [i32; 3]) -> [u32; 4] {
        [0; 4]
    }
    fn lod(&self, _: usize, _: [f32; 4]) -> [f32; 4] {
        [0.0; 4]
    }
    fn derivative(&self, _: &program::Operand, _: bool) -> [f32; 4] {
        [0.0; 4]
    }
}
