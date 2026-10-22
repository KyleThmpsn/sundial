//! Marathon numeric expressions and native resource bindings.
use super::*;
use anyhow::Context as _;

#[derive(Clone, Default)]
struct Expr {
    code: Vec<u8>,
    missing: BTreeSet<String>,
    initial: BTreeSet<String>,
    resource: bool,
    authored: bool,
    viewport: bool,
}

fn missing(reason: String) -> Expr {
    Expr {
        missing: BTreeSet::from([reason]),
        ..Expr::default()
    }
}

pub(super) fn arithmetic(op: u8) -> Option<(u8, usize, usize)> {
    Some(match op {
        7 => (op, 0, 1),
        1..=15 => (op, 0, 2),
        0x13..=0x16 => (op - 3, 0, 3),
        0x18..=0x23 => (op - 3, 0, 1),
        0x28 => (0x21, 0, 1),
        0x29 => (0x22, 1, 1),
        0x2a => (0x23, 0, 1),
        0x2c => (0x25, 0, 1),
        0x2e..=0x32 => (op - 7, 0, 1),
        0x35 => (0x2e, 0, 5),
        0x3b => (0x0a, 0, 2),
        0x47 => (0x39, 1, 2),
        0x43..=0x49 => (op - 14, 1, 1),
        _ => return None,
    })
}

fn take<'a>(code: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8]> {
    let result = code
        .get(*at..*at + count)
        .context("Truncated Marathon expression")?;
    *at += count;
    Ok(result)
}

/// Rows written by a stage expression cannot be treated as immutable shader inputs.
pub(super) fn written(code: &[u8]) -> Result<BTreeSet<usize>> {
    let mut rows = BTreeSet::new();
    let mut at = 0;
    while at < code.len() {
        let op = take(code, &mut at, 1)?[0];
        let size = match op {
            0x42 | 0x51 | 0x53 | 0x57..=0x59 | 0x5d | 0x61 | 0x63 | 0x64 => 1,
            0x62 => 4,
            0x4a..=0x4f | 0x67..=0x69 => 2,
            _ => {
                arithmetic(op)
                    .context("Unsupported source stage expression")?
                    .1
            }
        };
        let operands = take(code, &mut at, size)?;
        if op == 0x53 {
            rows.insert(usize::from(operands[0]));
        }
    }
    Ok(rows)
}
fn pop(stack: &mut Vec<Expr>) -> Result<Expr> {
    stack.pop().context("Marathon expression stack underflow")
}

#[derive(Clone)]
pub(super) struct Context<'a> {
    pub constants: Vec<[u8; 16]>,
    pub local: &'a channel::Bank,
    pub parent: &'a channel::Bank,
    pub objects: &'a BTreeMap<String, u8>,
    pub globals: &'a channel::Globals,
    pub metadata: BTreeMap<(u8, u8), [u8; 16]>,
    pub samplers: BTreeSet<u8>,
    pub authored: BTreeMap<super::super::inputs::Input, u8>,
    pub full_viewport: bool,
}

pub(super) struct Lowered {
    pub code: Vec<u8>,
    pub resources: BTreeSet<u32>,
    pub samplers: BTreeSet<u32>,
    pub evidence: Vec<Value>,
    pub constants: BTreeMap<usize, [u8; 16]>,
    pub viewports: BTreeSet<u32>,
}

impl Context<'_> {
    fn constant(&mut self, value: [u8; 16]) -> Result<Expr> {
        let index = if let Some(i) = self.constants.iter().enumerate().find_map(|(i, v)| {
            (*v == value && !self.authored.values().any(|&slot| usize::from(slot) == i))
                .then_some(i)
        }) {
            i
        } else {
            self.constants.push(value);
            self.constants.len() - 1
        };
        Ok(Expr {
            code: vec![
                0x34,
                u8::try_from(index).context("Marathon expression constant capacity")?,
            ],
            ..Expr::default()
        })
    }

    fn authored(&mut self, input: super::super::inputs::Input) -> Result<Expr> {
        let index = if let Some(&index) = self.authored.get(&input) {
            index
        } else {
            let index = u8::try_from(self.constants.len())
                .context("Authored material constant capacity")?;
            self.constants.push(resources::vector_bytes(
                super::super::MaterialInputs::default().value(input)?,
            ));
            self.authored.insert(input, index);
            index
        };
        Ok(Expr {
            code: vec![0x34, index],
            authored: true,
            ..Expr::default()
        })
    }

    fn object(
        &mut self,
        name: u32,
        parent: bool,
        seen: &mut BTreeSet<(bool, u32)>,
    ) -> Result<Expr> {
        ensure!(
            seen.len() < 64 && seen.insert((parent, name)),
            "Cyclic Marathon channel expression"
        );
        let bank = if parent { self.parent } else { self.local };
        let declaration = bank.iter().find(|d| d.name == name).cloned();
        let result = if let Some(d) = declaration.as_ref().filter(|d| !d.code.is_empty()) {
            let mut at = 0;
            let mut stack = Vec::new();
            let mut output = None;
            while at < d.code.len() {
                let op = take(&d.code, &mut at, 1)?[0];
                match op {
                    0x42 => {
                        let i = take(&d.code, &mut at, 1)?[0] as usize;
                        stack.push(
                            self.constant(*d.constants.get(i).context("Channel constant index")?)?,
                        );
                    }
                    0x4a => {
                        let i = take(&d.code, &mut at, 1)?[0] as usize;
                        let selector = *d.inputs.get(i).context("Channel input index")?;
                        let dependency = usize::try_from(selector).ok().and_then(|i| bank.get(i));
                        stack.push(if let Some(dependency) = dependency {
                            self.object(dependency.name, parent, seen)?
                        } else {
                            missing(format!("Channel {name:08X} selector {selector:016X}"))
                        });
                    }
                    0x4c => {
                        ensure!(
                            take(&d.code, &mut at, 1)? == [0] && output.is_none(),
                            "Channel output differs"
                        );
                        output = Some(pop(&mut stack)?);
                    }
                    _ => {
                        let (native, size, arity) =
                            arithmetic(op).context("Unsupported Marathon channel arithmetic")?;
                        // Curves require relocating a contiguous source constant range.
                        ensure!(!(0x43..=0x49).contains(&op), "Unsupported channel curve");
                        let args = take(&d.code, &mut at, size)?;
                        combine(&mut stack, native, args, arity, op == 0x3b)?;
                    }
                }
            }
            ensure!(stack.is_empty(), "Unfinished source channel expression");
            output.context("Source channel has no output")?
        } else if !parent
            && self
                .parent
                .iter()
                .any(|d| d.name == name && !d.code.is_empty())
        {
            self.object(name, true, seen)?
        } else if let Some(index) = self.objects.get(&format!("{name:08X}")) {
            Expr {
                code: vec![0x4d, *index],
                ..Expr::default()
            }
        } else if let Some(d) = declaration {
            let mut e = self.constant(d.initial)?;
            e.initial.insert(format!("Object channel {name:08X}"));
            e
        } else {
            missing(format!("Object channel {name:08X}"))
        };
        seen.remove(&(parent, name));
        Ok(result)
    }

    #[allow(
        clippy::cognitive_complexity,
        reason = "Decode the material stack program with resource, temporary and output state in one checked pass."
    )]
    pub fn lower(
        &mut self,
        code: &[u8],
        values: &[[u8; 16]],
        live: &BTreeSet<u32>,
        strict: bool,
    ) -> Result<Lowered> {
        ensure!(
            self.constants.len() <= 256 && values.len() <= 256,
            "Marathon expression table capacity"
        );
        let source_constants = self.constants.len();
        let mut result = Lowered {
            code: vec![],
            resources: BTreeSet::new(),
            samplers: BTreeSet::new(),
            evidence: vec![],
            constants: BTreeMap::new(),
            viewports: BTreeSet::new(),
        };
        let mut stack = Vec::new();
        let mut temps = BTreeMap::<u8, Expr>::new();
        let mut outputs = BTreeMap::<u8, Expr>::new();
        let mut at = 0;
        while at < code.len() {
            let op = take(code, &mut at, 1)?[0];
            let e = match op {
                0x42 => {
                    let i = take(code, &mut at, 1)?[0];
                    ensure!((i as usize) < source_constants, "Source constant index");
                    Expr {
                        code: vec![0x34, i],
                        ..Expr::default()
                    }
                }
                0x62 => {
                    let name = u32::from_be_bytes(take(code, &mut at, 4)?.try_into()?);
                    self.object(name, false, &mut BTreeSet::new())?
                }
                0x63 => {
                    let i = take(code, &mut at, 1)?[0];
                    let (name, value) = *self
                        .globals
                        .source
                        .get(i as usize)
                        .context("Source global index")?;
                    if let Some(index) = self.globals.native.get(&name) {
                        Expr {
                            code: vec![0x4e, *index],
                            ..Expr::default()
                        }
                    } else {
                        result.evidence.push(json!({"global":format!("{name:08X}"),"runtime":"Source initial value","reason":"No native global with this name"}));
                        self.constant(value)?
                    }
                }
                0x64 => {
                    let input = take(code, &mut at, 1)?[0];
                    if self.globals.wetness == Some(input) {
                        self.authored(super::super::inputs::Input::Wetness)?
                    } else {
                        missing(format!("Source renderer input {input}"))
                    }
                }
                0x4a..=0x4f => {
                    let args = take(code, &mut at, 2)?;
                    if op == 0x4b && args[0] == 2 && (0x47..=0x4e).contains(&args[1]) {
                        self.authored(super::super::inputs::Input::Pattern(args[1] - 0x47))?
                    } else if op == 0x4a && matches!(args, [1, 0 | 4]) {
                        Expr {
                            code: vec![0x3c, 1, args[1]],
                            ..Expr::default()
                        }
                    } else if op == 0x4b && args == [3, 4] && self.full_viewport {
                        // The pixel adapter supplies native dimensions. Retain
                        // an unresolved dependency if arithmetic changes this row.
                        Expr {
                            viewport: true,
                            ..missing("Source depth viewport".into())
                        }
                    } else if op == 0x4b && args == [1, 30] {
                        // Both consumers select the same derivative-scaled
                        // stochastic coverage calculation from this frame flag.
                        Expr {
                            code: vec![0x3d, 1, 26],
                            ..Expr::default()
                        }
                    } else if op == 0x4b
                        && args == [1, 40]
                        && code.get(at..at + 2) == Some(&[0x29, 0xff])
                    {
                        self.authored(super::super::inputs::Input::SpecularAa)?
                    } else if op == 0x4d && args == [45, 1] {
                        Expr {
                            code: vec![0x3f, 44, 1],
                            resource: true,
                            ..Expr::default()
                        }
                    } else if op == 0x4d && args == [3, 31] {
                        // Both consumers sample the sky hemisphere with the
                        // normalized reflection's paraboloid coordinates and LOD.
                        Expr {
                            code: vec![0x3f, 3, 19],
                            resource: true,
                            ..Expr::default()
                        }
                    } else {
                        missing(format!("Source extern {op:02X}:{}", hex::encode(args)))
                    }
                }
                0x67..=0x69 => {
                    let args = take(code, &mut at, 2)?;
                    let data = *self
                        .metadata
                        .get(&(op, args[0]))
                        .context("Unresolved source texture metadata")?;
                    let mut e = self.constant(data)?;
                    e.code.extend([0x22, args[1]]);
                    e
                }
                0x61 => {
                    let i = take(code, &mut at, 1)?[0];
                    ensure!(
                        self.samplers.contains(&i),
                        "Source sampler index is not a sampler"
                    );
                    Expr {
                        code: vec![0x4c, i],
                        resource: true,
                        ..Expr::default()
                    }
                }
                0x51 => {
                    let i = take(code, &mut at, 1)?[0];
                    if let Some(e) = outputs.get(&i) {
                        e.clone()
                    } else {
                        self.constant(*values.get(i as usize).context("Initial output index")?)?
                    }
                }
                0x57 => temps
                    .get(&take(code, &mut at, 1)?[0])
                    .context("Source temporary read before write")?
                    .clone(),
                0x58 => {
                    let index = take(code, &mut at, 1)?[0];
                    // Inline the stored expression at each read. Reassigning a
                    // temporary cannot change expressions already read from it.
                    temps.insert(index, pop(&mut stack)?);
                    continue;
                }
                0x53 => {
                    let i = take(code, &mut at, 1)?[0];
                    ensure!((i as usize) < values.len(), "Source output index");
                    let e = pop(&mut stack)?;
                    ensure!(!e.resource, "Resource written to numeric output");
                    outputs.insert(i, e.clone());
                    result.constants.remove(&usize::from(i));
                    result.viewports.remove(&u32::from(i));
                    if e.viewport {
                        result.viewports.insert(u32::from(i));
                    }
                    if e.missing.is_empty() && !e.authored {
                        let bank: Vec<[f32; 4]> = self
                            .constants
                            .iter()
                            .map(|row| {
                                std::array::from_fn(|lane| {
                                    f32::from_le_bytes(
                                        row[lane * 4..lane * 4 + 4].try_into().unwrap(),
                                    )
                                })
                            })
                            .collect();
                        if let Some(value) =
                            sundial::package_authoring::constant_material_expression(&e.code, &bank)
                        {
                            result
                                .constants
                                .insert(usize::from(i), resources::vector_bytes(value));
                        }
                    }
                    if live.contains(&u32::from(i)) {
                        ensure!(
                            !strict || e.missing.is_empty(),
                            "Material output {i} needs unmapped source inputs {:?}",
                            e.missing
                        );
                        if e.missing.is_empty() {
                            result.code.extend(e.code);
                            result.code.extend([0x43, i]);
                        }
                        result.evidence.push(json!({"output":i,"translated":e.missing.is_empty(),"source_initial_inputs":e.initial,"unresolved":e.missing,"fallback":if e.missing.is_empty(){Value::Null}else{json!("Source initial output")}}));
                    }
                    continue;
                }
                0x59 | 0x5d => {
                    let slot = take(code, &mut at, 1)?[0];
                    let e = pop(&mut stack)?;
                    ensure!(
                        slot >> 5 == 1 && e.missing.is_empty() && e.resource,
                        "Unsupported source resource binding"
                    );
                    let register = u32::from(slot & 31);
                    if op == 0x5d {
                        ensure!(e.code.first() == Some(&0x4c), "Non-sampler binding");
                        result.samplers.insert(register);
                    } else {
                        ensure!(
                            e.code == [0x3f, 44, 1] || e.code == [0x3f, 3, 19],
                            "Unknown native scene texture"
                        );
                        result.resources.insert(register);
                    }
                    result.code.extend(e.code);
                    result
                        .code
                        .extend([if op == 0x5d { 0x49 } else { 0x47 }, slot]);
                    continue;
                }
                _ => {
                    let (native, size, arity) = arithmetic(op).with_context(|| {
                        format!("Unsupported Marathon expression opcode {op:02X}")
                    })?;
                    let args = take(code, &mut at, size)?;
                    if (0x43..=0x49).contains(&op) {
                        let width = [2, 2, 5, 10, 10, 6, 11][(op - 0x43) as usize];
                        ensure!(
                            args[0] as usize + width <= source_constants,
                            "Source curve constant extent"
                        );
                    }
                    combine(&mut stack, native, args, arity, op == 0x3b)?;
                    continue;
                }
            };
            stack.push(e);
            ensure!(stack.len() <= 64, "Source expression stack capacity");
        }
        ensure!(stack.is_empty(), "Unfinished Marathon material expression");
        Ok(result)
    }
}

fn combine(
    stack: &mut Vec<Expr>,
    op: u8,
    args: &[u8],
    arity: usize,
    strict_less: bool,
) -> Result<()> {
    ensure!(stack.len() >= arity, "Source arithmetic stack underflow");
    let mut e = Expr::default();
    let mut inputs: Vec<_> = stack.drain(stack.len() - arity..).collect();
    if strict_less {
        inputs.reverse();
    }
    for input in inputs {
        ensure!(!input.resource, "Resource consumed by arithmetic");
        e.code.extend(input.code);
        e.missing.extend(input.missing);
        e.initial.extend(input.initial);
        e.authored |= input.authored;
    }
    e.code.push(op);
    e.code.extend(args);
    if strict_less {
        e.code.push(0x07);
    }
    ensure!(e.code.len() <= 65536, "Expanded source expression capacity");
    stack.push(e);
    Ok(())
}
