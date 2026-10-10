//! Pure geometry around a translated vertex program's skeleton-palette selection.
//! Slice by register lanes so metadata loads and unimplemented skinning never execute.
use super::*;
use program::{Instruction, Operand, Program as Code};

pub(crate) struct Motion {
    pub vertices: std::ops::Range<usize>,
    code: Code,
    constants: Vec<[f32; 4]>,
    expression: Option<Program>,
}

impl Motion {
    pub(in crate::model_preview) fn frame(&self, seconds: f32) -> Option<Frame> {
        let mut frame = [[0.0; 4]; 128];
        frame[..self.constants.len()].copy_from_slice(&self.constants);
        if let Some(expression) = &self.expression {
            expression
                .run_shader_into(&mut frame[..self.constants.len()], seconds)
                .ok()?;
        }
        Some(frame)
    }

    /// Emit the accepted geometry slice with the same input contract as `vertex::evaluate`.
    pub(in crate::model_preview) fn gpu_source(&self, name: &str) -> String {
        use std::fmt::Write;
        let mut source = self.code.glsl(&format!("{name}Code"));
        // Slicing deliberately omits the original return instruction. Capture the registers
        // on fallthrough as well, as the CPU evaluator does for this straight-line program.
        source.truncate(source.len() - 2);
        source.push_str("for(int i=0;i<16;i++)result[i]=uintBitsToFloat(o[i]);}\n");
        writeln!(source, "void {name}(inout vec3 p,inout vec3 n,inout vec4 t){{vec4 v[16],o[16];for(int i=0;i<16;i++)v[i]=vec4(0.0);").unwrap();
        for semantic in &self.code.inputs {
            let value = match semantic.name.as_str() {
                "POSITION" => "vec4(p,0.0)",
                "NORMAL" => "vec4(n,0.0)",
                "TANGENT" => "t",
                _ => "vec4(0.0)",
            };
            writeln!(source, "v[{}]={value};", semantic.register).unwrap();
        }
        writeln!(source, "{name}Code(v,o);for(int i=0;i<3;i++)if(any(isnan(o[i]))||any(isinf(o[i])))return;p=o[0].xyz;float len=length(o[1].xyz);if(len>1e-8&&!isinf(len))n=o[1].xyz/len;t.xyz=o[2].xyz;}}").unwrap();
        source
    }

    pub fn animated(&self) -> bool {
        self.expression.as_ref().is_some_and(Program::animated)
    }

    pub fn apply(&self, model: &Model, seconds: f32, pose: &mut animation::Deformed) {
        let mut frame = [[0.0; 4]; 128];
        frame[..self.constants.len()].copy_from_slice(&self.constants);
        if self.expression.as_ref().is_some_and(|p| {
            p.run_shader_into(&mut frame[..self.constants.len()], seconds)
                .is_err()
        }) {
            return;
        }
        for index in self.vertices.clone() {
            let input = Input {
                position: pose.positions[index],
                normal: pose.normals[index],
                tangent: pose.tangents[index],
                color: model.colors.get(index).copied().unwrap_or([1.0; 4]),
                uv: model.uvs[index],
                detail_uv: model.detail_uvs.get(index).copied().unwrap_or([0.0; 2]),
            };
            let Some(values) = vertex::evaluate(&self.code, &input, &frame) else {
                continue;
            };
            if values[..3].iter().flatten().any(|v| !v.is_finite()) {
                continue;
            }
            pose.positions[index].copy_from_slice(&values[0][..3]);
            pose.normals[index] = shader::normal::normalize(values[1][..3].try_into().unwrap())
                .unwrap_or(input.normal);
            pose.tangents[index][..3].copy_from_slice(&values[2][..3]);
        }
    }
}

/// This envelope is produced by the importer for source vertex equations on stored native
/// geometry. Other vertex programs keep their existing checked renderer path.
pub(super) fn stored(code: &Code) -> bool {
    code.buffers.contains(&(11, 24))
        && code.buffers.contains(&(12, 14))
        && (code.resources.len() == 2
            || code.resources.len() == 3
                && code
                    .outputs
                    .iter()
                    .any(|s| s.name == "TEXCOORD" && s.index == 8))
        && code.resources.iter().all(|r| {
            r.dimension == 3
                && if r.integer {
                    matches!(r.slot, 1 | 3)
                } else {
                    r.slot == 0
                }
        })
        && [1, 3].iter().all(|slot| {
            code.resources
                .iter()
                .any(|r| r.slot == *slot && r.integer && r.dimension == 3)
        })
        && code.inputs.iter().any(|s| s.system == 6)
        && ["POSITION", "NORMAL", "TANGENT"]
            .iter()
            .all(|name| code.inputs.iter().any(|s| s.name == *name && s.index == 0))
}

fn destinations(i: &Instruction) -> usize {
    match i.code {
        13 | 18 | 21 | 31 | 62 => 0,
        38 | 77 | 78 => 2,
        _ => 1,
    }
}

fn same_register(a: &Operand, b: &Operand) -> bool {
    a.kind == b.kind
        && a.indices.len() == b.indices.len()
        && a.indices
            .iter()
            .zip(&b.indices)
            .all(|(a, b)| a.base == b.base && a.relative.is_none() && b.relative.is_none())
}

fn capture(output: usize, source: &Operand) -> Instruction {
    Instruction {
        code: 54,
        saturate: false,
        nonzero: false,
        operands: vec![
            Operand {
                kind: 2,
                indices: vec![program::Index {
                    base: output as u32,
                    relative: None,
                }],
                lanes: [0, 1, 2, 3],
                mask: 7,
                modifier: 0,
                literal: [0; 4],
            },
            source.clone(),
        ],
        offset: [0; 3],
    }
}

/// The palette's instruction range and the three captures, each at its instruction.
type Anchors = (std::ops::RangeInclusive<usize>, [(usize, Instruction); 3]);

/// Where a stored deformation's results are read: the palette's instruction range, and the
/// three captures, the position before its matrix rows and the normal and tangent before
/// their basis products.
fn anchors(code: &Code) -> Option<Anchors> {
    let palette_start = code.instructions.iter().position(|i| i.code == 67)?;
    let camera = code
        .instructions
        .iter()
        .enumerate()
        .skip(palette_start)
        .find(|(_, i)| {
            i.operands
                .iter()
                .any(|s| s.kind == 8 && s.indices[0].base == 12)
        })?
        .0;
    let palette_end = code.instructions[..camera]
        .iter()
        .rposition(|i| i.code == 21)?;
    if palette_end <= palette_start {
        return None;
    }
    // Three matrix rows consume one model-space position. Capture the position before
    // those rows, then the normal and tangent before the corresponding basis products.
    // This preserves translations after palette selection and avoids stale temp values.
    let rows: Vec<_> = code
        .instructions
        .iter()
        .enumerate()
        .skip(camera)
        .filter(|(_, i)| i.code == 17)
        .take(3)
        .collect();
    if !matrix_rows(&rows) {
        return None;
    }
    let basis: Vec<_> = code
        .instructions
        .iter()
        .enumerate()
        .skip(rows[2].0 + 1)
        .filter(|(_, i)| i.code == 16 && same_register(&i.operands[1], &rows[0].1.operands[1]))
        .take(2)
        .collect();
    if basis.len() != 2 {
        return None;
    }
    Some((
        palette_start..=palette_end,
        [
            (rows[0].0, capture(0, &rows[0].1.operands[2])),
            (basis[0].0, capture(1, &basis[0].1.operands[2])),
            (basis[1].0, capture(2, &basis[1].1.operands[2])),
        ],
    ))
}

/// Whether `rows` are the three matrix rows of one position: three distinct row registers,
/// each applied to the same temporary source with the same lanes and modifier.
fn matrix_rows(rows: &[(usize, &Instruction)]) -> bool {
    let [first, second, third] = rows else {
        return false;
    };
    let (a, b, c) = (
        &first.1.operands[1],
        &second.1.operands[1],
        &third.1.operands[1],
    );
    let source = &first.1.operands[2];
    rows.iter()
        .all(|(_, i)| i.operands[1].kind == 0 && i.operands[2].kind == 0)
        && !same_register(a, b)
        && !same_register(a, c)
        && !same_register(b, c)
        && rows.iter().all(|(_, i)| {
            same_register(&i.operands[2], source)
                && i.operands[2].lanes == source.lanes
                && i.operands[2].modifier == source.modifier
        })
}

/// Takes `i` into the slice when it writes lanes still needed, and marks the lanes its sources
/// read. Returns whether it was taken, or nothing when the slice cannot carry it.
fn take(code: &Code, i: &Instruction, in_palette: bool, needed: &mut [u8; 32]) -> Option<bool> {
    let n = destinations(i);
    let mut lanes = 0;
    for d in i.operands.iter().take(n).filter(|d| d.kind == 0) {
        lanes |= needed[d.indices[0].base as usize] & d.mask;
    }
    if lanes == 0 {
        return Some(false);
    }
    // Only numeric, bounded equations can supply a stored geometry deformation.
    if in_palette || !matches!(i.code,0|1|14..=17|25|29|43|47|49|50|51|52|54|55|56|64..=68|75|77|86)
    {
        return None;
    }
    for d in i.operands.iter().take(n).filter(|d| d.kind == 0) {
        needed[d.indices[0].base as usize] &= !d.mask;
    }
    for s in i.operands.iter().skip(n) {
        read_source(code, i, s, lanes, needed)?;
    }
    Some(true)
}

/// Marks the lanes source `s` of `i` reads as needed. Refuses a relative index, an input other
/// than position, normal or tangent, and a constant other than the material's own or buffer
/// 11's slot 5.
fn read_source(
    code: &Code,
    i: &Instruction,
    s: &Operand,
    lanes: u8,
    needed: &mut [u8; 32],
) -> Option<()> {
    if s.indices.iter().any(|v| v.relative.is_some()) {
        return None;
    }
    match s.kind {
        0 => {
            for lane in 0..4 {
                let read = if (15..=17).contains(&i.code) {
                    lane < usize::from(i.code - 13)
                } else {
                    lanes & (1 << lane) != 0
                };
                if read {
                    needed[s.indices[0].base as usize] |= 1 << s.lanes[lane];
                }
            }
            Some(())
        }
        1 => code
            .inputs
            .iter()
            .any(|input| {
                input.register == s.indices[0].base as usize
                    && matches!(input.name.as_str(), "POSITION" | "NORMAL" | "TANGENT")
            })
            .then_some(()),
        4 => Some(()),
        8 => matches!(
            (s.indices[0].base, s.indices[1].base),
            (0, 0..=127) | (11, 5)
        )
        .then_some(()),
        _ => None,
    }
}

fn slice(mut code: Code) -> Option<Code> {
    if !stored(&code) {
        return None;
    }
    let (palette, captures) = anchors(&code)?;
    let mut needed = [0u8; 32];
    let mut selected = Vec::new();
    for at in (0..=captures[2].0).rev() {
        // A capture precedes its matrix product, so visit it after that product while
        // walking backwards. The original product is never an output of this slice.
        let i = &code.instructions[at];
        if take(&code, i, palette.contains(&at), &mut needed)? {
            selected.push(i.clone());
        }
        if let Some((_, output)) = captures.iter().find(|(capture, _)| *capture == at) {
            let source = &output.operands[1];
            if source.kind != 0 {
                return None;
            }
            for &lane in &source.lanes[..3] {
                needed[source.indices[0].base as usize] |= 1 << lane;
            }
            selected.push(output.clone());
        }
        if !palette.contains(&at) && matches!(i.code, 13 | 18 | 21 | 31 | 62) {
            return None;
        }
    }
    if needed.iter().any(|v| *v != 0) {
        return None;
    }
    selected.reverse();
    if !selected
        .iter()
        .flat_map(|i| &i.operands)
        .any(|s| s.kind == 8 && s.indices[0].base == 0)
    {
        return None;
    }
    code.instructions = selected;
    Some(code)
}
pub(in crate::model_preview) fn load(
    manager: &PackageManager,
    tag: u32,
    objects: ObjectInputs<'_>,
) -> Result<Option<Motion>, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    let vertex = u32_at(&bytes, 0x48)?;
    if matches!(vertex, 0 | u32::MAX) {
        return Ok(None);
    }
    let shader = super::super::read::shader_bytes(manager, vertex, 1)?;
    let Ok(code) = Code::read_stored(&shader) else {
        return Ok(None);
    };
    let Some(code) = slice(code) else {
        return Ok(None);
    };
    let constants = super::super::read::stage_constants(manager, &bytes, 0x48)?;
    check_constants(&code, &constants)?;
    let globals = crate::dyes::material::global_channels(manager);
    let expression = Program::material(&bytes, 0x48, &globals, objects, 0, constants.len())?;
    if let Some(program) = &expression {
        program.run_shader_into(&mut constants.clone(), 0.0)?;
    }
    Ok(Some(Motion {
        vertices: 0..0,
        code,
        constants,
        expression,
    }))
}
