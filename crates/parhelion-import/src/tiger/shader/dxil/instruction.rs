use super::*;

pub(super) struct Emitter<'a> {
    pub program: &'a Program,
    pub options: &'a Options,
    pub declarations: BTreeMap<u32, &'static str>,
    pub handles: BTreeMap<u32, (u32, u32)>,
    /// Resource kind and register -> DXIL resource kind and byte size/properties.
    pub resources: BTreeMap<(u32, u32), (u32, u32)>,
}

fn call(code: &str) -> Result<(&str, &str, Vec<&str>)> {
    let (typ, rest) = code
        .strip_prefix("call ")
        .context("Expected DXIL call")?
        .split_once(" @dx.op.")
        .context("Unsupported DXIL call target")?;
    let (name, rest) = rest.split_once('(').context("DXIL call arguments")?;
    let close = rest.rfind(')').context("Unterminated DXIL call")?;
    let tail = rest[close + 1..].trim();
    ensure!(
        tail.is_empty()
            || tail
                .strip_prefix('#')
                .is_some_and(|s| s.parse::<u32>().is_ok()),
        "Unsupported DXIL call suffix"
    );
    Ok((
        typ,
        name.split('.').next().context("DXIL intrinsic")?,
        arguments(&rest[..close])?,
    ))
}

fn literals(text: &str) -> Result<Vec<u32>> {
    if text.ends_with("zeroinitializer") {
        return Ok(vec![0; 4]);
    }
    let start = text.find('{').context("DXIL resource literal")?;
    let end = text.rfind('}').context("DXIL resource literal end")?;
    arguments(&text[start + 1..end])?
        .into_iter()
        .map(number)
        .collect()
}

impl Emitter<'_> {
    fn remap_value(
        &self,
        operand: &str,
        mapping: &BTreeMap<u32, u32>,
        depth: usize,
    ) -> Result<String> {
        ensure!(depth < 16, "DXIL output encoding exceeds budget");
        let token = operand
            .split_whitespace()
            .last()
            .context("Missing output value")?;
        if token.starts_with('%') {
            let ident = id(token)?;
            let definition = self
                .program
                .blocks
                .values()
                .flatten()
                .find(|i| i.id == Some(ident))
                .context("Missing output encoding definition")?;
            let rest = definition
                .code
                .strip_prefix("select ")
                .context("Output encoding is not a literal selection")?;
            let args = arguments(rest)?;
            ensure!(args.len() == 3, "Output encoding select arity");
            return Ok(format!(
                "({} ? {} : {})",
                value(args[0])?,
                self.remap_value(args[1], mapping, depth + 1)?,
                self.remap_value(args[2], mapping, depth + 1)?
            ));
        }
        let bits = if let Some(hex) = token.strip_prefix("0x") {
            ensure!(hex.len() == 16, "Unsupported output floating literal");
            (f64::from_bits(u64::from_str_radix(hex, 16)?) as f32).to_bits()
        } else {
            token.parse::<f32>()?.to_bits()
        };
        let native = mapping
            .get(&bits)
            .context("Unknown source output encoding")?;
        Ok(format!("asfloat(0x{native:08X}u)"))
    }

    fn handle(&self, text: &str, kind: u32) -> Result<u32> {
        let &(found, slot) = self
            .handles
            .get(&id(text)?)
            .context("Unresolved DXIL handle")?;
        ensure!(kind == found, "DXIL resource kind differs");
        Ok(slot)
    }

    #[allow(
        clippy::cognitive_complexity,
        reason = "Keep scalar opcode validation and emission together in the checked instruction table."
    )]
    pub fn expression(
        &mut self,
        ident: Option<u32>,
        code: &str,
    ) -> Result<(Option<&'static str>, Option<String>)> {
        let (op, rest) = code.split_once(' ').context("Malformed DXIL instruction")?;
        let result = match op {
            "call" => return self.intrinsic(ident, code),
            "extractvalue" => {
                let a = arguments(rest)?;
                ensure!(
                    a.len() == 2 && number(a[1])? < 4,
                    "Unsupported DXIL aggregate extraction"
                );
                (
                    "float",
                    format!(
                        "{}.{}",
                        value(a[0])?,
                        "xyzw".as_bytes()[number(a[1])? as usize] as char
                    ),
                )
            }
            "fmul" | "fadd" | "fsub" | "fdiv" | "and" | "or" | "xor" => {
                let a = arguments(rest)?;
                ensure!(a.len() == 2, "DXIL binary arity");
                let boolean = a[0].contains("i1 ");
                let typ = if op.starts_with('f') {
                    "float"
                } else if boolean {
                    "bool"
                } else {
                    "uint"
                };
                let symbol = match op {
                    "fmul" => "*",
                    "fadd" => "+",
                    "fsub" => "-",
                    "fdiv" => "/",
                    "and" if boolean => "&&",
                    "or" if boolean => "||",
                    "and" => "&",
                    "or" => "|",
                    "xor" => "^",
                    _ => unreachable!(),
                };
                (typ, format!("({} {symbol} {})", value(a[0])?, value(a[1])?))
            }
            "fcmp" | "icmp" => {
                let a = arguments(rest)?;
                ensure!(a.len() == 2, "DXIL comparison arity");
                let first = a[0].strip_prefix("fast ").unwrap_or(a[0]);
                let pred = first
                    .split_whitespace()
                    .next()
                    .context("DXIL comparison predicate")?;
                let (symbol, invert) = match pred {
                    "olt" | "ult" if op == "icmp" || pred == "olt" => ("<", false),
                    "ole" | "ule" if op == "icmp" || pred == "ole" => ("<=", false),
                    "ogt" | "ugt" if op == "icmp" || pred == "ogt" => (">", false),
                    "oge" | "uge" if op == "icmp" || pred == "oge" => (">=", false),
                    "ult" => (">=", true),
                    "ule" => (">", true),
                    "ugt" => ("<=", true),
                    "uge" => ("<", true),
                    "oeq" | "eq" => ("==", false),
                    "une" | "ne" => ("!=", false),
                    _ => bail!("Unsupported DXIL comparison {pred}"),
                };
                (
                    "bool",
                    format!(
                        "{}({} {symbol} {})",
                        if invert { "!" } else { "" },
                        value(a[0])?,
                        value(a[1])?
                    ),
                )
            }
            "select" => {
                let a = arguments(rest)?;
                ensure!(a.len() == 3, "DXIL select arity");
                (
                    scalar_type(a[1].split_whitespace().next().context("DXIL select type")?)?,
                    format!("({} ? {} : {})", value(a[0])?, value(a[1])?, value(a[2])?),
                )
            }
            "uitofp" | "sitofp" | "fptosi" | "fptoui" | "bitcast" => {
                let (source, target) = rest.split_once(" to ").context("DXIL cast type")?;
                let from = source
                    .split_whitespace()
                    .next()
                    .context("DXIL cast source")?;
                let (typ, function) = match (op, from, target) {
                    ("bitcast", "float", "i32") => ("uint", "asuint"),
                    ("bitcast", "i32", "float") => ("float", "asfloat"),
                    ("uitofp", "i32" | "i1", "float") => ("float", "float"),
                    ("fptosi", "float", "i32") => ("int", "int"),
                    ("fptoui", "float", "i32") => ("uint", "uint"),
                    _ => bail!("Unsupported DXIL conversion {code}"),
                };
                (typ, format!("{function}({})", value(source)?))
            }
            "phi" => {
                return Ok((
                    Some(scalar_type(
                        rest.split_whitespace().next().context("DXIL phi type")?,
                    )?),
                    None,
                ));
            }
            "br" | "ret" => return Ok((None, None)),
            _ => bail!("Unsupported DXIL instruction {code}"),
        };
        Ok((Some(result.0), Some(result.1)))
    }

    #[allow(
        clippy::cognitive_complexity,
        reason = "Keep intrinsic arity, operand validation and emission together for each DXIL opcode."
    )]
    fn intrinsic(
        &mut self,
        ident: Option<u32>,
        code: &str,
    ) -> Result<(Option<&'static str>, Option<String>)> {
        let (typ, name, a) = call(code)?;
        let opcode = number(a.first().context("Missing DXIL intrinsic opcode")?)?;
        let result = match name {
            "createHandleFromBinding" => {
                ensure!(opcode == 217 && a.len() == 4, "DXIL resource binding arity");
                let binding = literals(a[1])?;
                ensure!(
                    binding.len() == 4
                        && binding[0] == binding[1]
                        && binding[2] == 0
                        && number(a[2])? == binding[0]
                        && a[3] == "i1 false",
                    "Dynamic DXIL resource arrays are unsupported"
                );
                ensure!(
                    matches!(binding[3], 0 | 2 | 3),
                    "Unsupported DXIL resource class"
                );
                self.handles.insert(
                    ident.context("DXIL handle result")?,
                    (binding[3], binding[0]),
                );
                return Ok((None, None));
            }
            "annotateHandle" => {
                ensure!(
                    opcode == 216 && a.len() == 3,
                    "DXIL resource annotation arity"
                );
                let handle = *self
                    .handles
                    .get(&id(a[1])?)
                    .context("Unresolved DXIL resource annotation")?;
                let property = literals(a[2])?;
                ensure!(
                    property.len() == 2
                        && matches!((handle.0, property[0]), (0, 2 | 4) | (2, 13) | (3, 14)),
                    "Unsupported DXIL resource properties"
                );
                if let Some(old) = self.resources.insert(handle, (property[0], property[1])) {
                    ensure!(
                        old == (property[0], property[1]),
                        "Conflicting DXIL resource declarations"
                    );
                }
                self.handles
                    .insert(ident.context("DXIL annotation result")?, handle);
                return Ok((None, None));
            }
            "loadInput" => {
                ensure!(
                    opcode == 4 && a.len() == 5 && number(a[2])? == 0,
                    "Unsupported DXIL input load"
                );
                let index = number(a[1])? as usize;
                let field = self
                    .program
                    .inputs
                    .get(index)
                    .context("DXIL input outside signature")?;
                let col = number(a[3])? as usize;
                ensure!(col < field.width, "DXIL input lane outside signature");
                let suffix = if field.width > 1 {
                    format!(".{}", "xyzw".as_bytes()[col] as char)
                } else {
                    String::new()
                };
                (scalar_type(typ)?, format!("i{index}{suffix}"))
            }
            "storeOutput" => {
                ensure!(
                    opcode == 5 && a.len() == 5 && number(a[2])? == 0,
                    "Unsupported DXIL output store"
                );
                let index = number(a[1])? as usize;
                let field = self
                    .program
                    .outputs
                    .get(index)
                    .context("DXIL output outside signature")?;
                let col = number(a[3])? as usize;
                ensure!(col < field.width, "DXIL output lane outside signature");
                if !self.options.targets.contains(&field.index) {
                    return Ok((None, None));
                }
                let output =
                    if let Some(mapping) = self.options.output_values.get(&(field.index, col)) {
                        self.remap_value(a[4], mapping, 0)?
                    } else {
                        value(a[4])?
                    };
                (
                    "void",
                    format!("o{index}.{} = {output};", "xyzw".as_bytes()[col] as char),
                )
            }
            "cbufferLoadLegacy" => {
                ensure!(
                    opcode == 59 && a.len() == 3 && typ == "%dx.types.CBufRet.f32",
                    "Unsupported DXIL constant-buffer load"
                );
                let slot = self.handle(a[1], 2)?;
                let replacement = number(a[2])
                    .ok()
                    .and_then(|index| self.options.constants.get(&(slot, index)))
                    .cloned();
                (
                    "float4",
                    replacement.unwrap_or(format!("cb{slot}[{}]", value(a[2])?)),
                )
            }
            "sample" | "sampleLevel" | "sampleBias" => {
                ensure!(
                    matches!(
                        (name, opcode),
                        ("sample", 60) | ("sampleBias", 61) | ("sampleLevel", 62)
                    ),
                    "DXIL sampling opcode differs"
                );
                ensure!(
                    (name != "sampleBias" && a.len() == 11)
                        || (name == "sampleBias" && a.len() == 12),
                    "DXIL sample arity"
                );
                let slot = self.handle(a[1], 0)?;
                let dimension = if self
                    .resources
                    .get(&(0, slot))
                    .context("DXIL sampled resource properties")?
                    .0
                    == 4
                {
                    3
                } else {
                    2
                };
                ensure!(
                    a[3 + dimension..7].iter().all(|v| *v == "float undef"),
                    "Unexpected DXIL sample coordinate"
                );
                let coords = a[3..3 + dimension]
                    .iter()
                    .map(|s| value(s))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                let offsets = a[7..7 + dimension]
                    .iter()
                    .map(|s| value(s))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                ensure!(
                    a[7 + dimension..10].iter().all(|v| *v == "i32 undef"),
                    "Unexpected DXIL sample offset"
                );
                let sampler = self.handle(a[2], 3)?;
                let (method, extra) = match name {
                    "sampleLevel" => ("SampleLevel", format!(",{}", value(a[10])?)),
                    "sampleBias" => {
                        ensure!(a[11] == "float undef", "DXIL sample clamp is unsupported");
                        ("SampleBias", format!(",{}", value(a[10])?))
                    }
                    _ => {
                        ensure!(a[10] == "float undef", "DXIL sample clamp is unsupported");
                        ("Sample", String::new())
                    }
                };
                (
                    "float4",
                    format!(
                        "t{slot}.{method}(s{sampler},float{dimension}({coords}){extra},int{dimension}({offsets}))"
                    ),
                )
            }
            "textureLoad" => {
                ensure!(
                    opcode == 66 && a.len() == 9 && a[5..].iter().all(|s| *s == "i32 undef"),
                    "Unsupported DXIL texture load"
                );
                (
                    "float4",
                    format!(
                        "t{}.Load(int3({},{},{}))",
                        self.handle(a[1], 0)?,
                        value(a[3])?,
                        value(a[4])?,
                        value(a[2])?
                    ),
                )
            }
            "discard" => {
                ensure!(opcode == 82 && a.len() == 2, "DXIL discard arity");
                ("void", format!("if ({}) discard;", value(a[1])?))
            }
            "unary" => {
                ensure!(a.len() == 2, "DXIL unary arity");
                let function = match opcode {
                    6 => "abs",
                    7 => "saturate",
                    12 => "cos",
                    13 => "sin",
                    21 => "exp2",
                    22 => "frac",
                    23 => "log2",
                    24 => "sqrt",
                    25 => "rsqrt",
                    26 => "round",
                    27 => "floor",
                    28 => "ceil",
                    83 => "ddx_coarse",
                    84 => "ddy_coarse",
                    _ => bail!("Unsupported DXIL unary opcode {opcode}"),
                };
                ("float", format!("{function}({})", value(a[1])?))
            }
            "binary" | "tertiary" | "dot2" | "dot3" | "dot4" => {
                let args = a[1..]
                    .iter()
                    .map(|v| value(v))
                    .collect::<Result<Vec<_>>>()?;
                let text = match (name, opcode, args.len()) {
                    ("binary", 35, 2) => format!("max({})", args.join(",")),
                    ("binary", 36, 2) => format!("min({})", args.join(",")),
                    ("tertiary", 46, 3) => format!("mad({})", args.join(",")),
                    ("dot2", 54, 4) | ("dot3", 55, 6) | ("dot4", 56, 8) => {
                        let n = args.len() / 2;
                        format!(
                            "dot(float{n}({}),float{n}({}))",
                            args[..n].join(","),
                            args[n..].join(",")
                        )
                    }
                    _ => bail!("Unsupported DXIL intrinsic {name}/{opcode}"),
                };
                ("float", text)
            }
            _ => bail!("Unsupported DXIL intrinsic {name}/{opcode}"),
        };
        Ok((Some(result.0), Some(result.1)))
    }
}
