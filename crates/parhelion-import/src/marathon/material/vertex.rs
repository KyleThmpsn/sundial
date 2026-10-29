//! Native weighted motion with checked source-local material varyings.
use super::*;
use crate::tiger::shader::dxil::{arguments, id, number};
mod flow;

fn call(code: &str) -> Result<(&str, Vec<&str>)> {
    let (_, name, args) = dxil::call(code)?;
    let (opcode, arity) = match name {
        "loadInput" => (4, 5),
        "storeOutput" => (5, 5),
        "binary" => (40, 3),
        "cbufferLoadLegacy" => (59, 3),
        "bufferLoad" => (68, 4),
        "annotateHandle" => (216, 3),
        "createHandleFromBinding" => (217, 4),
        _ => anyhow::bail!("Unsupported source vertex intrinsic {name}"),
    };
    ensure!(
        args.len() == arity && number(args[0])? == opcode,
        "Source vertex intrinsic {name} has unexpected operands"
    );
    Ok((name, args))
}

struct Source<'a> {
    definitions: BTreeMap<u32, &'a str>,
    inputs: Vec<dxil::Field>,
    surface: &'a geometry::Surface,
    constants: &'a [[u8; 16]],
    dynamic: &'a BTreeSet<usize>,
    flow: flow::Flow,
}

impl Source<'_> {
    fn literal_position(
        &self,
        outputs: &[dxil::Field],
        writes: &BTreeMap<(u32, u32), &str>,
    ) -> Result<Option<String>> {
        let Some((ordinal, field)) = outputs
            .iter()
            .enumerate()
            .find(|(_, f)| f.semantic.eq_ignore_ascii_case("SV_Position"))
        else {
            return Ok(None);
        };
        ensure!(field.width == 4, "Source projected position width");
        self.literal_output(writes, ordinal, 4)
    }

    fn selected<'a>(&'a self, value: &'a str, depth: usize) -> Result<&'a str> {
        ensure!(depth < 32, "Source varying phi exceeds budget");
        if !value
            .split_whitespace()
            .last()
            .is_some_and(|s| s.starts_with('%'))
        {
            return Ok(value);
        }
        let Some(rest) = self.definition(value)?.strip_prefix("phi float ") else {
            return Ok(value);
        };
        let predecessor = self
            .flow
            .predecessor(id(value)?, |value| self.condition(value))?;
        let mut selected = None;
        for pair in arguments(rest)? {
            let pair = pair
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .context("Source varying phi pair")?;
            let parts = arguments(pair)?;
            ensure!(parts.len() == 2, "Source varying phi arity");
            if id(parts[1])? == predecessor {
                ensure!(
                    selected.is_none(),
                    "Repeated source varying phi predecessor"
                );
                selected = Some(parts[0]);
            }
        }
        self.selected(
            selected.context("Source varying phi has no active predecessor")?,
            depth + 1,
        )
    }

    fn literal_output(
        &self,
        writes: &BTreeMap<(u32, u32), &str>,
        ordinal: usize,
        width: usize,
    ) -> Result<Option<String>> {
        let mut values = Vec::new();
        for lane in 0..width {
            let value = self.selected(
                writes
                    .get(&(ordinal as u32, lane as u32))
                    .context("Source output lane")?,
                0,
            )?;
            let token = value
                .split_whitespace()
                .last()
                .context("Source output value")?;
            if token.starts_with('%') {
                return Ok(None);
            }
            values.push(float(literal(token)?));
        }
        Ok(Some(format!("float{width}({})", values.join(","))))
    }

    fn constant(&self, value: &str, depth: usize) -> Result<f32> {
        ensure!(depth < 32, "Source vertex condition exceeds budget");
        let token = value
            .split_whitespace()
            .last()
            .context("Source condition operand")?;
        if !token.starts_with('%') {
            return literal(token);
        }
        let code = self.definition(value)?;
        if let Some(rest) = code.strip_prefix("extractvalue ") {
            let parts = arguments(rest)?;
            ensure!(parts.len() == 2, "Source condition extraction arity");
            let (name, load) = call(self.definition(parts[0])?)?;
            ensure!(
                name == "cbufferLoadLegacy" && self.binding(load[1])? == (2, 0),
                "Source vertex condition requires an immutable material input"
            );
            let row = number(load[2])? as usize;
            let lane = number(parts[1])? as usize;
            ensure!(lane < 4, "Source condition lane");
            ensure!(
                !self.dynamic.contains(&row),
                "Source vertex condition has a dynamic material producer"
            );
            let data = self
                .constants
                .get(row)
                .context("Source vertex constant buffer is truncated")?;
            let value = f32::from_le_bytes(data[lane * 4..lane * 4 + 4].try_into().unwrap());
            ensure!(value.is_finite(), "Nonfinite source vertex condition");
            return Ok(value);
        }
        anyhow::bail!("Unsupported source vertex condition value {code}")
    }

    fn condition(&self, value: &str) -> Result<bool> {
        let code = self.definition(value)?;
        let rest = code
            .strip_prefix("fcmp ")
            .context("Unsupported source vertex condition")?;
        let (comparison, operands) = rest.split_once(' ').context("Source vertex comparison")?;
        let operands = arguments(operands)?;
        ensure!(operands.len() == 2, "Source comparison arity");
        let a = self.constant(operands[0], 0)?;
        let b = self.constant(operands[1], 0)?;
        Ok(match comparison {
            "olt" => a < b,
            "ole" => a <= b,
            "ogt" => a > b,
            "oge" => a >= b,
            "oeq" => a == b,
            "one" => a != b,
            _ => anyhow::bail!("Unsupported source vertex comparison {comparison}"),
        })
    }
    fn definition(&self, value: &str) -> Result<&str> {
        self.definitions
            .get(&id(value)?)
            .copied()
            .context("Missing source vertex definition")
    }
    fn binding(&self, value: &str) -> Result<(u32, u32)> {
        let (name, args) = call(self.definition(value)?)?;
        if name == "annotateHandle" {
            return self.binding(args[1]);
        }
        ensure!(
            name == "createHandleFromBinding" && args.len() == 4 && args[3] == "i1 false",
            "Unsupported source vertex handle"
        );
        if args[1].ends_with("zeroinitializer") {
            ensure!(number(args[2])? == 0, "Source vertex resource array");
            return Ok((0, 0));
        }
        let row = args[1]
            .split_once('{')
            .context("Source binding fields")?
            .1
            .trim_end_matches('}')
            .trim();
        let row = arguments(row)?;
        ensure!(
            row.len() == 4
                && number(row[0])? == number(row[1])?
                && number(row[2])? == 0
                && number(args[2])? == number(row[0])?,
            "Source vertex resource array"
        );
        Ok((number(row[3])?, number(row[0])?))
    }
    fn scalar(&self, value: &str, depth: usize) -> Result<String> {
        ensure!(depth < 32, "Source varying expression exceeds budget");
        let value = self.selected(value, 0)?;
        let token = value
            .split_whitespace()
            .last()
            .context("Missing source varying value")?;
        if !token.starts_with('%') {
            return Ok(float(literal(token)?));
        }
        let code = self.definition(value)?;
        if code.starts_with("call ") {
            let (name, args) = call(code)?;
            ensure!(
                name == "loadInput" && number(args[2])? == 0,
                "Unsupported source varying call"
            );
            let field = self
                .inputs
                .get(number(args[1])? as usize)
                .context("Source input signature")?;
            let lane = number(args[3])? as usize;
            ensure!(lane < 3 && field.index == 0, "Source varying input lane");
            let variable = match field.semantic.as_str() {
                "POSITION" => "sourcePosition",
                "NORMAL" => "sourceNormal",
                _ => anyhow::bail!("Unsupported source varying input {}", field.semantic),
            };
            return Ok(format!("{variable}.{}", "xyz".as_bytes()[lane] as char));
        }
        if let Some(rest) = code.strip_prefix("extractvalue ") {
            let args = arguments(rest)?;
            ensure!(args.len() == 2, "Source vector extraction arity");
            let lane = number(args[1])? as usize;
            ensure!(lane < 4, "Source vector lane");
            let (name, read) = call(self.definition(args[0])?)?;
            if name == "cbufferLoadLegacy" {
                ensure!(
                    self.binding(read[1])? == (2, 1) && lane < 3,
                    "Unmapped source varying constant"
                );
                let value = match number(read[2])? {
                    4 => self.surface.scale[lane],
                    5 => self.surface.offset[lane],
                    _ => anyhow::bail!("Unmapped source position decode row"),
                };
                return Ok(float(value));
            }
            if name == "bufferLoad" {
                ensure!(
                    self.surface.has_colors && self.binding(read[1])? == (0, 0),
                    "Missing source vertex colors"
                );
                // Verify the source reads vertex ID clamped to the color count.
                let (operation, index) = call(self.definition(read[2])?)?;
                ensure!(
                    operation == "binary" && number(index[0])? == 40 && index.len() == 3,
                    "Source color address is not a clamped vertex ID"
                );
                let mut vertex = false;
                let mut count = false;
                for operand in &index[1..] {
                    let value = self.definition(operand)?;
                    if value.contains("@dx.op.loadInput.i32") {
                        let (_, load) = call(value)?;
                        vertex = self
                            .inputs
                            .get(number(load[1])? as usize)
                            .is_some_and(|f| f.semantic == "SV_VertexID");
                    } else if let Some(cast) = value.strip_prefix("bitcast ") {
                        let from = cast
                            .split_once(" to ")
                            .context("Source color bound cast")?
                            .0;
                        let extract = self
                            .definition(from)?
                            .strip_prefix("extractvalue ")
                            .context("Source color bound lane")?;
                        let parts = arguments(extract)?;
                        ensure!(parts.len() == 2, "Source color bound extraction arity");
                        let (operation, load) = call(self.definition(parts[0])?)?;
                        // The rigid model's decode-scale row stores the color
                        // index bound in W as integer bits, alongside XYZ scale.
                        count = operation == "cbufferLoadLegacy"
                            && number(parts[1])? == 3
                            && self.binding(load[1])? == (2, 1)
                            && number(load[2])? == 4;
                    }
                }
                ensure!(vertex && count, "Source color count producer differs");
                return Ok(format!("vertexColor.{}", "xyzw".as_bytes()[lane] as char));
            }
        }
        if let Some((op, rest)) = code.split_once(' ')
            && matches!(op, "fmul" | "fadd")
        {
            let args = arguments(rest)?;
            ensure!(args.len() == 2, "Source varying arithmetic");
            return Ok(format!(
                "({} {} {})",
                self.scalar(args[0], depth + 1)?,
                if op == "fmul" { "*" } else { "+" },
                self.scalar(args[1], depth + 1)?
            ));
        }
        anyhow::bail!("Unsupported source varying expression {code}")
    }
}

fn literal(token: &str) -> Result<f32> {
    let value = if let Some(bits) = token.strip_prefix("0x") {
        ensure!(bits.len() == 16, "Unsupported source floating literal");
        f64::from_bits(u64::from_str_radix(bits, 16)?) as f32
    } else {
        token
            .parse::<f32>()
            .context("Unsupported source varying literal")?
    };
    ensure!(value.is_finite(), "Nonfinite source varying literal");
    Ok(value)
}

pub(super) fn float(value: f32) -> String {
    format!("asfloat(0x{:08X}u)", value.to_bits())
}

fn inverse(m: &[f32; 16]) -> Result<[[f32; 4]; 3]> {
    let (a, b, c, d, e, f, g, h, i) = (m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]);
    let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    ensure!(
        determinant.is_finite() && determinant.abs() > 1e-12 && m[12..] == [0., 0., 0., 1.],
        "Source assembly transform is not invertible affine"
    );
    let mut result = [
        [e * i - f * h, c * h - b * i, b * f - c * e, 0.],
        [f * g - d * i, a * i - c * g, c * d - a * f, 0.],
        [d * h - e * g, b * g - a * h, a * e - b * d, 0.],
    ];
    for row in &mut result {
        for value in row.iter_mut().take(3) {
            *value /= determinant;
        }
        row[3] = -(row[0] * m[3] + row[1] * m[7] + row[2] * m[11]);
    }
    Ok(result)
}

fn validate_projection(source: &Source<'_>, projection: Option<reticle::Projection>) -> Result<()> {
    let Some(projection) = projection else {
        return Ok(());
    };
    let mut rows = BTreeSet::new();
    for definition in source
        .definitions
        .values()
        .filter(|definition| definition.contains("@dx.op.cbufferLoadLegacy.f32("))
    {
        let (_, args) = call(definition)?;
        if source.binding(args[1])? == (2, 0) {
            rows.insert(number(args[2])?);
        }
    }
    ensure!(
        rows == (u32::from(projection.matrix)..=u32::from(projection.depth)).collect(),
        "Reticle vertex reads a different projection layout"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn hlsl(
    listing: &str,
    pixel: &[u8],
    surface: &geometry::Surface,
    constants: &[[u8; 16]],
    dynamic: &BTreeSet<usize>,
    motion: (&str, u32),
    projection: Option<reticle::Projection>,
    aimed: Option<reticle::Projection>,
) -> Result<(String, bool)> {
    let inputs = dxil::signature(listing, true)?;
    let outputs = dxil::signature(listing, false)?;
    let mut source = Source {
        definitions: BTreeMap::new(),
        inputs,
        surface,
        constants,
        dynamic,
        flow: flow::Flow::default(),
    };
    let writes = read_entry(listing, &mut source)?;
    validate_projection(&source, projection)?;
    // Read/write masks come from FXC's optimized input signature. Velocity
    // inputs removed with their render target need no native producer.
    let range =
        crate::tiger::shader::dxbc::chunk(pixel, b"ISGN")?.context("Pixel input signature")?;
    let signature = &pixel[range];
    let pixel_fields = crate::tiger::shader::dxbc::signature(pixel, b"ISGN")?;
    let mut parameters = Vec::new();
    let mut assignments = Vec::new();
    let mut colors = false;
    let mut position = false;
    if let Some(value) = source.literal_position(&outputs, &writes)? {
        assignments.push(format!("projected={value};"));
    }
    // A scope cutout adds its aperture point above every source varying, so it is the highest
    // pixel varying.
    let aperture = surface.cutout.as_ref().and_then(|_| {
        pixel_fields
            .iter()
            .filter(|(semantic, ..)| semantic == "TEXCOORD")
            .map(|(_, index, ..)| *index)
            .max()
    });
    for (pixel_ordinal, (semantic, index, _, mask)) in pixel_fields.iter().enumerate() {
        if semantic.eq_ignore_ascii_case("SV_Position") {
            parameters.push("out float4 projected:SV_POSITION0".into());
            position = true;
            continue;
        }
        if semantic.eq_ignore_ascii_case("SV_IsFrontFace") {
            continue;
        }
        ensure!(
            semantic == "TEXCOORD" && matches!(*mask, 1 | 3 | 7 | 15),
            "Unsupported pixel varying signature {semantic}{index} with mask {mask:X}"
        );
        let width = mask.count_ones() as usize;
        if aperture == Some(*index)
            && let Some(lens) = &surface.cutout
        {
            ensure!(width == 4, "Scope aperture varying width differs");
            let vector = |v: &[f32; 3]| v.iter().map(|v| float(*v)).collect::<Vec<_>>().join(",");
            parameters.push(format!("out float4 aperturePoint:TEXCOORD{index}"));
            assignments.push(format!(
                "float3 apertureLocal=p.xyz-float3({});\naperturePoint=float4(dot(apertureLocal,float3({})),dot(apertureLocal,float3({})),0,0);",
                vector(&lens.center), vector(&lens.axes[0]), vector(&lens.axes[1])));
            continue;
        }
        let (ordinal, field) = outputs
            .iter()
            .enumerate()
            .find(|(_, f)| f.semantic == *semantic && f.index == *index)
            .context("Pixel varying has no source vertex output")?;
        ensure!(
            width <= field.width,
            "Pixel varying exceeds the source vertex output"
        );
        let active = signature
            .get(8 + pixel_ordinal * 24 + 21)
            .is_some_and(|v| *v != 0);
        let expression = if !active {
            "0".to_owned()
        } else if let Some(value) = source.literal_output(&writes, ordinal, width)? {
            value
        } else if matches!(surface.stage, 4 | 13) {
            ensure!(
                *index == 0 && width == 2,
                "Unsupported source depth varying"
            );
            "tex".to_owned()
        } else {
            match field.index {
                0 => "float4(n,saturate(objectData[7].w+saturate(objectData[7].z*n.z)))".into(),
                1 => "float4(t,p.z)".into(),
                2 => "cross(n,t)*tangent.w".into(),
                3 => "tex.xyxy".into(),
                4 if surface.stage == 17 => "reticleView".into(),
                4 | 7 if surface.stage == 17 => "world".into(),
                4 => "world".into(),
                _ => {
                    let components = (0..width)
                        .map(|lane| {
                            source
                                .scalar(
                                    writes
                                        .get(&(ordinal as u32, lane as u32))
                                        .context("Missing source varying lane")?,
                                    0,
                                )
                                .with_context(|| {
                                    format!("Source vertex varying TEXCOORD{index} lane {lane}")
                                })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    colors |= components.iter().any(|s| s.contains("vertexColor"));
                    format!("float{width}({})", components.join(","))
                }
            }
        };
        parameters.push(format!("out float{width} o{index}:TEXCOORD{index}"));
        assignments.push(format!("o{}={expression};", field.index));
    }
    if !position {
        parameters.push("out float4 projected:SV_POSITION0".into());
    }
    Ok((
        program(
            surface,
            &parameters,
            &assignments,
            colors,
            motion,
            projection,
            aimed,
        )?,
        colors,
    ))
}

fn read_entry<'a>(
    listing: &'a str,
    source: &mut Source<'a>,
) -> Result<BTreeMap<(u32, u32), &'a str>> {
    let mut writes = BTreeMap::new();
    // DXC also emits intrinsic declarations. Only the executable body owns
    // SSA definitions and output writes, and each function has its own SSA IDs.
    let body = dxil::entry(listing)?;
    for line in body.lines().map(str::trim) {
        let line = line.split(';').next().unwrap().trim();
        if let Some((name, code)) = line.strip_prefix('%').and_then(|s| s.split_once(" = ")) {
            if name.bytes().all(|b| b.is_ascii_digit()) {
                source.definitions.insert(name.parse()?, code);
            }
        } else if line.starts_with("call void @dx.op.storeOutput.f32(") {
            let (_, args) = call(line)?;
            ensure!(number(args[2])? == 0, "Source vertex output row");
            writes.insert((number(args[1])?, number(args[3])?), args[4]);
        }
    }
    source.flow = flow::Flow::read(body)?;
    Ok(writes)
}

fn program(
    surface: &geometry::Surface,
    parameters: &[String],
    assignments: &[String],
    colors: bool,
    motion: (&str, u32),
    projection: Option<reticle::Projection>,
    aimed: Option<reticle::Projection>,
) -> Result<String> {
    let inv = inverse(&surface.transform)?;
    let vector = |row: &[f32]| row.iter().map(|v| float(*v)).collect::<Vec<_>>().join(",");
    // Glass ahead of an enclosed sight's source eye draws in the reticle pass and always takes the
    // source camera. Its pixel stage draws it only once aiming completes, so it fills the same
    // window as the framed reticle. Its offset from the eye over its distance along the sight axis
    // is a view tangent, divided by the tangent of half the source view. Like a reticle, it takes
    // the native reticle rows' aspect (`M11 / |M00|`), screen offset and focal-plane depth, with a
    // clip W of one. The glass's own native depth depends on where the aimed camera sits, and
    // glass projected with the view rows in transparent or reticle passes did not show in game.
    let glass_framing = match (&surface.framed, aimed) {
        (Some(framing), Some(aimed)) => format!(
            "float3 framedOffset=local-float3({origin});\nfloat framedForward=dot(framedOffset,float3({forward}));\nfloat2 framedLateral=float2(dot(framedOffset,float3({left})),dot(framedOffset,float3({up})));\nfloat4 framedDepth=reticleData[{m2}]*-reticleData[{depth}].x+reticleData[{m3}];\nprojected=float4(float2(-framedLateral.x,framedLateral.y*abs(reticleData[{m1}].y/reticleData[{m}].x))/({tangent}*framedForward)+reticleData[{offset}].xy,framedDepth.z/framedDepth.w,1);",
            origin = vector(&framing.origin),
            forward = vector(&framing.forward),
            left = vector(&framing.axes[0]),
            up = vector(&framing.axes[1]),
            tangent = float(framing.tangent),
            m = aimed.matrix,
            m1 = aimed.matrix + 1,
            m2 = aimed.matrix + 2,
            m3 = aimed.matrix + 3,
            depth = aimed.depth,
            offset = aimed.offset
        ),
        (Some(_), None) => anyhow::bail!("Aimed glass has no native reticle projection"),
        (None, _) => String::new(),
    };
    Ok(format!(
        r#"
cbuffer Object:register(b11) {{float4 objectData[{object_rows}];}};
cbuffer View:register(b12) {{float4 viewData[14];}};
{reticle_buffer}
{color_texture}
{motion}
void main(float4 position:POSITION0,float4 weight:BLENDWEIGHT0,uint4 bone:BLENDINDICES0,float2 uv:TEXCOORD0,float4 normal:NORMAL0,float4 tangent:TANGENT0,uint vertexId:SV_VertexID,
{parameters}) {{
float4 p=float4(position.xyz*objectData[5].w+objectData[5].xyz,1);
float3 world,n,t;
deform(p,normal.xyz,tangent.xyz,weight,bone,world,n,t);
float3 relative=world-viewData[7].xyz;
projected=viewData[0]*relative.x+viewData[1]*relative.y+viewData[2]*relative.z+viewData[13];
float2 tex=uv*objectData[6].xy+objectData[6].zw;
float4 inverse0=float4({inverse0}),inverse1=float4({inverse1}),inverse2=float4({inverse2});
float3 local=float3(dot(inverse0,p),dot(inverse1,p),dot(inverse2,p));
{reticle_projection}
float3 sourcePosition=(local-float3({offset}))/float3({scale});
float3 sourceNormal=float3(dot(inverse0.xyz,normal.xyz),dot(inverse1.xyz,normal.xyz),dot(inverse2.xyz,normal.xyz));
{color_load}
{assignments}
{glass_framing}
}}
"#,
        object_rows = 8 + motion.1 * 3,
        motion = motion.0,
        color_texture = if colors {
            "Texture2D<float4> colors:register(t0);"
        } else {
            ""
        },
        reticle_buffer = if let Some(projection) = projection.or(aimed) {
            format!(
                "cbuffer Reticle:register(b0){{float4 reticleData[{}];}};",
                projection.vectors
            )
        } else {
            String::new()
        },
        reticle_projection = if let Some(projection) = projection {
            // Reticle vertices are camera-relative artwork, unlike the physical lens.
            // Undo attachment placement before the native scope projection consumes them.
            // A framed sight's artwork spans the source aimed view at its source depth. The
            // native matrix still supplies axis signs, aspect and depth.
            let screen = match projection.framing {
                Some(tangent) => format!(
                    "float2(reticlePosition.x*reticleData[{m}].x,reticlePosition.y*reticleData[{m1}].y)/(abs(reticleData[{m}].x)*reticleData[{depth}].x*{tangent})",
                    depth = projection.depth,
                    m = projection.matrix,
                    m1 = projection.matrix + 1,
                    tangent = float(tangent)
                ),
                None => "q.xy/q.w".into(),
            };
            format!(
                "float3 reticlePosition=local-float3(0,0,reticleData[{depth}].x);\nfloat4 q=reticleData[{m}]*reticlePosition.x+reticleData[{m1}]*reticlePosition.y+reticleData[{m2}]*reticlePosition.z+reticleData[{m3}];\nprojected=float4({screen}+reticleData[{offset}].xy,q.z/q.w,1);\nfloat3 reticleView=viewData[4].xyz*reticlePosition.x+viewData[5].xyz*reticlePosition.y+viewData[6].xyz*reticlePosition.z+viewData[7].xyz;",
                depth = projection.depth,
                m = projection.matrix,
                m1 = projection.matrix + 1,
                m2 = projection.matrix + 2,
                m3 = projection.matrix + 3,
                offset = projection.offset
            )
        } else {
            String::new()
        },
        parameters = parameters.join(","),
        inverse0 = vector(&inv[0]),
        inverse1 = vector(&inv[1]),
        inverse2 = vector(&inv[2]),
        offset = vector(&surface.offset),
        scale = vector(&surface.scale),
        color_load = if colors {
            "float4 vertexColor=colors.Load(int3(vertexId&255,vertexId>>8,0));"
        } else {
            ""
        },
        assignments = assignments.join("\n")
    ))
}
