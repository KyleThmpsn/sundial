//! Source material equations with native runtime and fixed surface bindings.
use super::super::contracts::Role;
use super::*;
use std::collections::BTreeSet;

fn surface(c: &mut Effect, kind: &str) -> Result<String> {
    let name = format!("source-surface-{kind}");
    if c.graph.node(&name).is_ok() {
        return Ok(name);
    }
    let mut header = c.graph.read(&format!("texture-{kind}-header"))?.0;
    let data = c.graph.read(&format!("texture-{kind}-data"))?.0;
    let format = Payload(header.clone()).u32(4)?;
    let (resident, ht, dt) = c.texture_template(matches!(format, 29 | 72 | 75 | 78 | 99))?;
    ensure!(
        header.len() == 40 && resident.len() == 40,
        "surface header differs"
    );
    header[24..36].copy_from_slice(&resident[24..36]);
    crate::d2_mot::texture::resident(&mut header, data.len())?;
    let body = format!("{name}-data");
    c.graph.add(&name, ht, &header, Some(&body), vec![])?;
    c.graph.add(&body, dt, &data, Some(&name), vec![])?;
    Ok(name)
}

#[derive(Clone)]
struct Program {
    code: Vec<u8>,
    constants: Vec<u8>,
    values: Vec<u8>,
    evidence: Vec<Value>,
    samplers: BTreeMap<u8, u8>,
    textures: BTreeMap<u8, u8>,
}

/// `reticle_viewport` maps the source reticle viewport extern onto native extern `0x49`. Only the
/// reticle stage may read it: the client leaves that extern null in other passes, and a read
/// there faults in the material interpreter.
fn texture_contract(
    material: &Payload,
    base: usize,
    bindings: &mut Bindings,
    reticle_viewport: bool,
) -> Result<()> {
    // Verified Deferred and Atmosphere resources. The integer screen-space
    // texture moved from +0x78 to +0xB8, Deferred specular mips from +0x98
    // to +0xD8, and the Atmosphere sky lookup from +0x90 to +0x100.
    // Preserve the material's output slot, which varies between materials.
    let code = array_bytes(material, base + 0x20, 1)?;
    let instructions = program::parse(&code)?;
    for pair in instructions.windows(2) {
        let mapped = match pair[0].args {
            [3, 0x17] => Some([0x3F, 3, 0x0F]),
            [3, 0x1B] => Some([0x3F, 3, 0x13]),
            [7, 0x20] => Some([0x3F, 7, 0x12]),
            // Reticle mask, verified against the native integer stencil lookup.
            [0x4A, 0x0C] => Some([0x3F, 0x49, 0x0C]),
            _ => None,
        };
        if let Some(mapped) = mapped.filter(|_| pair[0].op == 0x4D && pair[1].op == 0x56) {
            let slot = pair[1].args[0];
            ensure!(
                slot >> 5 == 1,
                "external texture has an unsupported shader stage"
            );
            bindings
                .external_textures
                .insert([0x4D, pair[0].args[0], pair[0].args[1]], mapped);
            bindings.texture_slots.insert(slot, slot);
        }
    }
    // The reticle viewport and display-color externs moved as whole structs.
    // Native reticle programs read the same viewport vector and five HDR
    // scalars. Keep their field offsets and the source shader's equations.
    for instruction in &instructions {
        let mapped = match (instruction.op, instruction.args) {
            (0x4B, [0x4A, field @ 4..=5]) if reticle_viewport => Some([0x3D, 0x49, *field]),
            (0x4C, [2, 8]) => Some([0x3E, 2, 6]),
            (0x4A, [0x5F, field @ 0..=4]) => Some([0x3C, 0x5C, *field]),
            _ => None,
        };
        if let Some(mapped) = mapped {
            bindings.external_textures.insert(
                [instruction.op, instruction.args[0], instruction.args[1]],
                mapped,
            );
        }
    }
    if base == 0x2B0
        && DECAL_STATES.contains(&(material.u8(48)? & 127))
        && instructions.windows(2).any(|pair| {
            pair[0].op == 0x4D
                && pair[0].args == [0x2D, 1]
                && pair[1].op == 0x56
                && pair[1].args == [0x2A]
        })
    {
        bindings
            .external_textures
            .insert([0x4D, 0x2D, 1], [0x3F, 0x2C, 1]);
        bindings.texture_slots.insert(0x2A, 0x25);
    }
    Ok(())
}

/// Validate donor-independent VM structure before extracting native assets.
/// Numeric channel bindings remain unresolved here and are verified against the
/// selected donor later. This output is never used as executable bytecode.
pub(super) fn preflight(material: &Payload, base: usize) -> Result<()> {
    let code = array_bytes(material, base + 0x20, 1)?;
    let constants = array_bytes(material, base + 0x30, 16)?.len() / 16;
    let external = material.u32(base + 0x74)?;
    let outputs = if [0, u32::MAX, 0x811C9DC5].contains(&external) {
        array_bytes(material, base + 0x50, 16)?.len() / 16
    } else {
        // External buffers are checked when exported. No guessed buffer size
        // should reject a source before that data has been read.
        256
    };
    let resources = usize::try_from(material.u64(base + 0x40)?)?;
    let mut bindings = Bindings {
        constant_count: constants,
        output_count: outputs,
        sampler_count: resources,
        sampler_stage: Some(if base == 0x70 { 2 } else { 1 }),
        textures: (0..resources)
            .map(|i| Ok((u8::try_from(i)?, u8::try_from(i)?)))
            .collect::<Result<BTreeMap<_, _>>>()?,
        ..Default::default()
    };
    for instruction in program::parse(&code)? {
        if matches!(instruction.op, 0x61 | 0x62) {
            let key = [instruction.op, instruction.args[0]];
            if let std::collections::btree_map::Entry::Vacant(entry) =
                bindings.texture_metadata.entry(key)
            {
                entry.insert(u8::try_from(bindings.constant_count)?);
                bindings.constant_count += 1;
            }
        }
    }
    texture_contract(material, base, &mut bindings, true)?;
    program::lower(&code, &bindings).map_err(crate::d2_mot::source_limit)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn program(
    c: &Effect,
    model: usize,
    material: &Payload,
    base: usize,
    text: &str,
    dyes: Option<&dyemap::Dyes>,
    textures: BTreeMap<u8, u8>,
    resource_bindings: Option<&Value>,
    reticle_viewport: bool,
) -> Result<Program> {
    let external = material.u32(base + 0x74)?;
    let mut values = if [0, u32::MAX, 0x811C9DC5].contains(&external) {
        array_bytes(material, base + 0x50, 16)?
    } else {
        let manifest = load(&c.bindings_root.join("source-manifest.json"))?;
        let entry = &manifest["tags"][format!("{external:08X}")];
        let reference = u32::try_from(
            entry["reference"]
                .as_u64()
                .context("external source constant buffer")?,
        )?;
        let data_entry = &manifest["tags"][format!("{reference:08X}")];
        ensure!(
            entry["type"] == 32
                && entry["subtype"] == 7
                && data_entry["type"] == 40
                && data_entry["subtype"] == 7
                && data_entry["reference"].as_u64() == Some(u64::from(external)),
            "source external constant buffer package pair differs"
        );
        let header = Payload(fs::read(
            c.bindings_root.join(format!("raw/{external:08X}.bin")),
        )?);
        let bytes = fs::read(c.bindings_root.join(format!("raw/{reference:08X}.bin")))?;
        super::super::MaterialBuffer::read(&header.0, bytes)?
            .data()
            .to_vec()
    };
    let count = values.len() / 16;
    ensure!(
        inputs::cb_count(text, 0)?.unwrap_or(0) == count,
        "source constant table and shader disagree"
    );
    let mut constants = array_bytes(material, base + 0x30, 16)?;
    let resources = material.u64(base + 0x40)? as usize;
    let objects = c.model_objects(model)?;
    let owner = c.source.raw(
        c.source.report["models"][model]["owner"]
            .as_str()
            .context("source material owner")?,
    )?;
    let (code, base_fallbacks) = channels::material_program(material, base, &owner, &objects)?;
    channels::validate_material_fallbacks(text, &base_fallbacks)?;
    let mut bindings = Bindings {
        objects,
        globals: c.globals.clone(),
        constant_count: constants.len() / 16,
        output_count: count,
        sampler_count: resources,
        sampler_stage: Some(if base == 0x70 { 2 } else { 1 }),
        textures,
        ..Default::default()
    };
    let mut global_fallbacks = Vec::new();
    for instruction in program::parse(&code)? {
        if instruction.op != 0x5D
            || bindings.globals.contains_key(&instruction.args[0])
            || bindings.global_defaults.contains_key(&instruction.args[0])
        {
            continue;
        }
        let row = c
            .global_defaults
            .as_array()
            .context("source global defaults")?
            .iter()
            .find(|r| r["index"].as_u64() == Some(u64::from(instruction.args[0])))
            .context("missing source global default")?;
        let values = row["value"]
            .as_array()
            .context("source global default vector")?;
        ensure!(
            values.len() == 4,
            "source global default vector size differs"
        );
        bindings
            .global_defaults
            .insert(instruction.args[0], u8::try_from(constants.len() / 16)?);
        for value in values {
            let value = value.as_f64().context("source global default component")? as f32;
            ensure!(value.is_finite(), "nonfinite source global default");
            constants.extend(value.to_le_bytes());
        }
        global_fallbacks.push(row.clone());
    }
    for instruction in program::parse(&code)? {
        if !matches!(instruction.op, 0x61 | 0x62) {
            continue;
        }
        let key = [instruction.op, instruction.args[0]];
        if bindings.texture_metadata.contains_key(&key) {
            continue;
        }
        let resource = resource_bindings.context("source texture metadata resources")?["samplers"]
            .as_array()
            .context("source resource table")?
            .get(instruction.args[0] as usize)
            .context("texture metadata resource index")?;
        ensure!(
            resource["direct_sampler"] == false,
            "texture metadata references a sampler"
        );
        let header = Payload(hex::decode(
            resource["header"]
                .as_str()
                .context("source texture header")?,
        )?);
        let value = crate::d2_mot::texture::metadata(&header, instruction.op)?;
        bindings
            .texture_metadata
            .insert(key, u8::try_from(constants.len() / 16)?);
        constants.extend(value);
    }
    bindings.constant_count = constants.len() / 16;
    texture_contract(material, base, &mut bindings, reticle_viewport)?;
    // This lowers the source material's own TFX program, so a translation gap
    // here is a converter limit no native donor can satisfy.
    let mut lowered = program::lower(&code, &bindings).map_err(crate::d2_mot::source_limit)?;
    let reads = inputs::reads(text, 0);
    for row in &mut lowered.evidence {
        let fallbacks = base_fallbacks
            .iter()
            .filter(|fallback| fallback["output"] == row["output"])
            .collect::<Vec<_>>();
        if !fallbacks.is_empty() {
            row["base_value_fallbacks"] = json!(fallbacks);
        }
        if !global_fallbacks.is_empty() {
            row["source_global_defaults"] = json!(global_fallbacks);
        }
        // Without the reticle viewport the output keeps its constant table value.
        let viewport_only = !reticle_viewport
            && row["unresolved"].as_array().is_some_and(|inputs| {
                !inputs.is_empty()
                    && inputs
                        .iter()
                        .all(|input| matches!(input.as_str(), Some("extern 4a04" | "extern 4a05")))
            });
        row["required_by_shader"] = json!(
            !viewport_only
                && reads
                    .as_ref()
                    .is_none_or(|v| v.contains(&(row["output"].as_u64().unwrap() as usize)))
        );
    }
    if let Some(dyes) = dyes {
        values.extend(&dyes.values);
        bindings.output_count = values.len() / 16;
        for (channel, code, extra) in &dyes.scopes {
            let bank = channel - 4;
            let map = (0..21u8)
                .map(|i| {
                    Ok((
                        i,
                        u8::try_from(count)?
                            + if i < 3 {
                                bank * 3 + i
                            } else {
                                9 + bank * 18 + i - 3
                            },
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let relocated = program::relocate(code, constants.len() / 16, &map)?;
            constants.extend(extra);
            bindings.constant_count = constants.len() / 16;
            let part = program::lower(&relocated, &bindings)?;
            ensure!(part.samplers.is_empty(), "source dye scope binds resources");
            lowered.code.extend(part.code);
            lowered
                .evidence
                .extend(part.evidence.into_iter().map(|mut row| {
                    row["dye_channel"] = json!(channel);
                    row["required_by_shader"] = json!(true);
                    row
                }));
        }
    }
    lowered.require_runtime_inputs()?;
    Ok(Program {
        code: lowered.code,
        constants,
        values,
        evidence: lowered.evidence,
        samplers: lowered.samplers,
        textures: lowered.textures,
    })
}

fn set_program(mat: &mut Vec<u8>, base: usize, p: &Program, objects: usize) -> Result<()> {
    append_array(mat, base + 0x20, 0x80800009, &p.code, 1)?;
    append_array(mat, base + 0x30, 0x80800090, &p.constants, 16)?;
    append_array(mat, base + 0x50, 0x80800090, &p.values, 16)?;
    if p.values.is_empty() {
        // The native renderer allocates whenever this byte is nonzero, then
        // binds the result even if the slot below is the absent-buffer sentinel.
        // A carrier's writable flag cannot survive removal of its output table.
        mat[base + 0x74] = 0;
    }
    if p.evidence.iter().any(|row| row["translated"] == true) {
        ensure!(
            !p.values.is_empty(),
            "runtime writes lack a constant buffer"
        );
        // A static donor can acquire runtime constant writes during conversion.
        // Native flag 0x10 requests writable storage before interpreting TFX.
        // Without it, the interpreter receives a null output pointer even when
        // the serialized constant array contains the required float4 slots.
        mat[base + 0x74] |= 0x10;
    }
    put(mat, base + 0x70, &u32::try_from(objects)?.to_le_bytes())?;
    put(
        mat,
        base + 0x80,
        &(if p.values.is_empty() { u32::MAX } else { 0 }).to_le_bytes(),
    )?;
    put(mat, base + 0x84, &u32::MAX.to_le_bytes())
}

fn fixed(
    mat: &mut Vec<u8>,
    base: usize,
    textures: &BTreeMap<u32, String>,
    patches: &mut Vec<Value>,
) -> Result<()> {
    let rows = textures
        .keys()
        .flat_map(|slot| slot.to_le_bytes().into_iter().chain(u32::MAX.to_le_bytes()))
        .collect::<Vec<_>>();
    append_array(mat, base + 8, 0x80807211, &rows, 8)?;
    for (at, symbol) in Payload(mat.clone())
        .array(base + 8, 8, None)?
        .iter()
        .zip(textures.values())
    {
        patches.push(json!({"offset":at+4,"symbol":symbol}));
    }
    Ok(())
}

#[derive(Clone)]
struct Resources {
    rows: Vec<u8>,
    patches: Vec<(usize, String)>,
}

fn samplers(c: &mut Effect, pool: &[Value], binding: &Value) -> Result<Resources> {
    let mut data = vec![];
    let mut symbols = vec![];
    for source in binding["samplers"]
        .as_array()
        .context("source sampler list")?
    {
        if source["direct_sampler"] != true {
            let symbol = emission::texture(c, source)?;
            symbols.push((data.len(), symbol));
            data.extend(u32::MAX.to_le_bytes());
        } else if let Some(native) = pool.iter().find(|n| n["data"] == source["data"]) {
            data.extend(tag(&native["tag"])?.to_le_bytes());
        } else {
            let name = format!(
                "source-sampler-{}",
                source["tag"].as_str().context("source sampler tag")?
            );
            if c.graph.node(&name).is_err() {
                let native = pool
                    .iter()
                    .find(|native| native["header"] == source["header"])
                    .context("No native sampler matches the source header format")?;
                let header =
                    hex::decode(source["header"].as_str().context("source sampler header")?)?;
                let body = hex::decode(source["data"].as_str().context("source sampler data")?)?;
                ensure!(
                    header.len() == 8 && body.len() == 52 && source["header"] == native["header"],
                    "source sampler envelope differs from native"
                );
                let body_name = format!("{name}-data");
                c.graph.add(
                    &name,
                    tag(&native["tag"])? as u64,
                    &header,
                    Some(&body_name),
                    vec![],
                )?;
                c.graph.add(
                    &body_name,
                    tag(&native["buffer"])? as u64,
                    &body,
                    Some(&name),
                    vec![],
                )?;
            }
            symbols.push((data.len(), name));
            data.extend(u32::MAX.to_le_bytes());
        }
        data.extend([0; 12]);
    }
    Ok(Resources {
        rows: data,
        patches: symbols,
    })
}

struct VertexResources {
    program: Program,
    /// For a stage-16 material, its program without the reticle viewport extern.
    lens: Option<Program>,
    resources: Resources,
    textures: BTreeMap<u32, String>,
}

fn vertex_resources(
    c: &mut Effect,
    model: usize,
    stage: usize,
    material: &Payload,
    vertex: &vertex::Vertex,
    binding: Option<&Value>,
    pool: &[Value],
) -> Result<VertexResources> {
    let empty = json!({"samplers":[],"textures":[]});
    let binding = binding.unwrap_or(&empty);
    let resources = binding["samplers"]
        .as_array()
        .context("vertex resource table")?;
    ensure!(
        resources.len() == usize::try_from(material.u64(0xB0)?)?,
        "vertex resource export is missing or stale"
    );
    let map = resources
        .iter()
        .enumerate()
        .filter(|(_, r)| r["direct_sampler"] != true)
        .map(|(i, _)| Ok((u8::try_from(i)?, u8::try_from(i)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    // A reticle's lens runs the same equations in a pass without the reticle viewport extern.
    let lens = (stage == 16)
        .then(|| {
            program(
                c,
                model,
                material,
                0x70,
                &vertex.source,
                None,
                map.clone(),
                Some(binding),
                false,
            )
        })
        .transpose()?;
    let program = program(
        c,
        model,
        material,
        0x70,
        &vertex.source,
        None,
        map,
        Some(binding),
        true,
    )?;
    for (&slot, &index) in &program.samplers {
        ensure!(
            resources
                .get(index as usize)
                .is_some_and(|r| r["direct_sampler"] == true)
                && slot < 16,
            "vertex sampler does not reference a sampler resource"
        );
    }
    ensure!(
        inputs::slots(&vertex.source, 's')?
            .iter()
            .all(|s| program.samplers.contains_key(&(*s as u8))),
        "source vertex sampler is unbound"
    );
    let mut textures = vertex.textures.iter().cloned().collect::<BTreeMap<_, _>>();
    let used = inputs::slots(&vertex.source, 't')?;
    for texture in binding["textures"]
        .as_array()
        .context("vertex fixed textures")?
    {
        let slot = u32::try_from(number(&texture["slot"])?)?;
        if used.contains(&slot) {
            ensure!(
                !textures.contains_key(&slot),
                "vertex texture conflicts with converted geometry"
            );
            textures.insert(slot, emission::texture(c, texture)?);
        }
    }
    let missing = used
        .iter()
        .filter(|slot| {
            !(textures.contains_key(slot)
                || program.textures.contains_key(&(0x40 | **slot as u8))
                || (**slot == 2 && shadow_viewport(stage, &vertex.source)))
        })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
            "source vertex textures are unbound: {missing:?}"
        )));
    }
    Ok(VertexResources {
        program,
        lens,
        resources: samplers(c, pool, binding)?,
        textures,
    })
}

fn shadow_viewport(stage: usize, source: &str) -> bool {
    // The shadow pass binds the atlas viewport at VS t2 directly, outside the
    // material resource table. Both runtime shaders load its sole float4 texel
    // before applying viewport scale and offset to clip-space XY.
    stage == 3
        && source.contains("Texture2D<float4> t2 : register(t2);")
        && source.matches("t2.").count() == 1
        && source.contains("t2.Load(float4(0,0,0,0)).xyzw")
}

fn set_resources(
    mat: &mut Vec<u8>,
    base: usize,
    resources: Resources,
    patches: &mut Vec<Value>,
) -> Result<()> {
    append_array(mat, base + 0x40, 0x808073F3, &resources.rows, 16)?;
    if !resources.patches.is_empty() {
        let start = Payload(mat.clone()).pointer(base + 0x48)? + 16;
        for (offset, symbol) in resources.patches {
            patches.push(json!({"offset":start+offset,"symbol":symbol}));
        }
    }
    Ok(())
}

/// Engine render states this converter emits for stage 1 decals. These are
/// engine constants, not asset identities: each one was confirmed to exist on
/// a native stage 1 material in the shipped packages, so the engine already
/// runs the equation the converted material asks for.
///
/// 26 and 29 are the states the discovered decal carriers declare. 57 is
/// alpha tested and keeps its own exact check. 27, 33 and 36 are additional
/// states the source arsenal uses; 27 and 36 also appear on native stage 1
/// draws directly, while 33 appears natively only at a vertex layout this
/// converter does not emit. The material state is retained explicitly.
const DECAL_STATES: [u8; 6] = [26, 27, 29, 33, 36, 57];

/// Blend equations this converter implements for each retained source stage.
/// The value comes from the source material alone, so an unsupported blend
/// fails identically for every native donor.
pub(super) fn supported_blend(stage: usize, blend: u8) -> bool {
    if stage == 1 {
        DECAL_STATES.contains(&blend)
    } else {
        blend
            == if matches!(stage, 0 | 3 | 12 | 14) {
                0
            } else {
                8
            }
    }
}

pub(super) fn auxiliary(c: &mut Effect, prepared: &Path, stage: usize) -> Result<()> {
    build(c, prepared, stage)?;
    if c.source.draws(stage)?.is_empty() {
        return Ok(());
    }
    let role = match stage {
        3 => Role::Shadow,
        12 => Role::Depth,
        _ => anyhow::bail!("unsupported auxiliary stage"),
    };
    let carrier = c.contracts()?.carrier(role)?;
    let material_tag = carrier.material;
    let shell = c.contracts()?.material(&c.refs, role)?;
    let pool = c.contracts()?.samplers.clone();
    let mut materials = BTreeMap::new();
    let mut records = vec![];
    let mut evidence = vec![];
    for draw in c.source.draws(stage)? {
        // build() already records this runtime-supplied draw as an omission.
        if draw.material == "FFFFFFFF" {
            continue;
        }
        let key = (draw.model, draw.material.clone());
        if !materials.contains_key(&key) {
            let material = c.source.raw(&draw.material)?;
            if material.u32(0x2B0)? != u32::MAX {
                continue;
            }
            ensure!(
                material.u8(48)? & 127 == 0,
                "source auxiliary blend differs"
            );
            let vertex = vertex::build(c, &draw, &material)?;
            let binding = c.bindings[&draw.material].get("vertex").cloned();
            let vertex_bindings = vertex_resources(
                c,
                draw.model,
                stage,
                &material,
                &vertex,
                binding.as_ref(),
                &pool,
            )?;
            let program = vertex_bindings.program;
            let name = format!("source-stage-{stage}-{}-{}", draw.model, draw.material);
            c.graph.program(
                &name,
                &vertex.text,
                &c.refs.join("shaders/vertex.hlsl"),
                true,
                &c.out,
            )?;
            let mut mat = shell.0.clone();
            put(&mut mat, 0x48, &u32::MAX.to_le_bytes())?;
            let mut patches = vec![json!({"offset":0x48,"symbol":format!("{name}-shader")})];
            fixed(&mut mat, 0x48, &vertex_bindings.textures, &mut patches)?;
            set_program(&mut mat, 0x48, &program, c.objects.len())?;
            // The vertex program reads its samplers from this table, so a shadow or
            // depth pass that loses it faults on the first vertex texture read.
            let samplers = vertex_bindings.resources.rows.len() / 16;
            set_resources(&mut mat, 0x48, vertex_bindings.resources, &mut patches)?;
            ensure!(
                Payload(mat.clone()).u64(0x88)? as usize == samplers,
                "auxiliary vertex sampler table differs from its program"
            );
            c.graph
                .add(&name, u64::from(material_tag), &mat, None, patches)?;
            evidence.push(json!({"model":draw.model_tag,"material":draw.material,"vertex_shader":format!("{:08X}",material.u32(0x70)?),"vertex_runtime_outputs":program.evidence,"source_vertex_equations_retained":true}));
            materials.insert(key.clone(), name);
        }
        records.push((draw, materials[&key].clone()));
    }
    for (draw, name) in records {
        for (channel, faces) in &draw.groups {
            c.draws.add(stage, &draw, *channel, faces, &name)?;
        }
    }
    c.draws.layout(stage)?;
    let mut all = c.graph.manifest[format!("source_stage_{stage}_adapter")]["materials"]
        .as_array()
        .context("source auxiliary pixel evidence")?
        .clone();
    all.extend(evidence);
    c.graph.manifest[format!("source_stage_{stage}_adapter")] =
        json!({"materials":all,"implementation":"Rust","gameplay_verified":false});
    Ok(())
}

/// A reticle material's vertex shader with its resources, so its lens can run the same vertex
/// equations. `lens` is the program without the reticle viewport extern.
struct VertexSide {
    shader: String,
    lens: Program,
    textures: BTreeMap<u32, String>,
    resources: Resources,
    objects: usize,
    flags: u8,
}

impl VertexSide {
    fn apply_lens(&self, mat: &mut Vec<u8>, patches: &mut Vec<Value>) -> Result<()> {
        put(mat, 0x48, &u32::MAX.to_le_bytes())?;
        patches.push(json!({"offset":0x48,"symbol":self.shader}));
        fixed(mat, 0x48, &self.textures, patches)?;
        set_program(mat, 0x48, &self.lens, self.objects)?;
        mat[0xBC] |= self.flags;
        set_resources(mat, 0x48, self.resources.clone(), patches)
    }
}

/// The native reticle vertex program. It loads the projection from extern 2 field 6 into
/// `cb0[6..9]` and the viewport offset from extern `0x49` field 4 into `cb0[10]`, and the
/// material keeps a z offset at `cb0[11].x`. Source reticle programs have the same equations.
const RETICLE_VERTEX_PROGRAM: [u8; 10] = [0x3E, 2, 6, 0x44, 6, 0x3D, 0x49, 4, 0x43, 0x0A];
/// Its projection alone. The client sets extern `0x49` only for the reticle stage, and the lens
/// stage draws in passes without it, where its viewport row keeps the material's value.
const RETICLE_LENS_PROGRAM: [u8; 5] = [0x3E, 2, 6, 0x44, 6];

/// A reticle draw whose material the source game supplies at runtime takes the native
/// reticle material: its pixel program, texture and vertex constants. Its vertex shader
/// runs the native reticle equations on the converted geometry and returns the draw's own
/// texture coordinates from the atlas.
fn runtime_reticle(
    c: &mut Effect,
    draw: &SourceDraw,
    shell: &Payload,
    template: u64,
) -> Result<(String, VertexSide, Value)> {
    ensure!(
        array_bytes(shell, 0x68, 1)? == RETICLE_VERTEX_PROGRAM
            && array_bytes(shell, 0x50, 8)?.is_empty()
            && array_bytes(shell, 0x88, 16)?.is_empty(),
        "native reticle vertex program differs"
    );
    let values = array_bytes(shell, 0x98, 16)?;
    ensure!(
        values.len() == 12 * 16,
        "native reticle vertex constants differ"
    );
    let name = format!("source-stage-16-{}-{}", draw.model, draw.material);
    let vertex_name = format!("{name}-vertex");
    let ([x, y, w, h], [aw, ah]) = c.atlas(draw.model)?;
    let native_model = c.graph.read("model")?;
    let uv = (0..4)
        .map(|i| native_model.f32(0x70 + i * 4))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        uv.iter().all(|v| v.is_finite()),
        "nonfinite native UV transform"
    );
    let text = format!(
        "cbuffer cb0 : register(b0)
{{
  float4 cb0[12];
}}

cbuffer cb11 : register(b11)
{{
  float4 cb11[6];
}}

cbuffer cb12 : register(b12)
{{
  float4 cb12[8];
}}

void main(
  float4 nativePosition : POSITION0,
  float2 nativeUv : TEXCOORD0,
  float3 nativeNormal : NORMAL0,
  float4 nativeTangent : TANGENT0,
  out float4 o0 : TEXCOORD0,
  out float4 o1 : TEXCOORD1,
  out float3 o2 : TEXCOORD2,
  out float4 o3 : TEXCOORD3,
  out float3 o4 : TEXCOORD4,
  out float4 o5 : SV_POSITION0)
{{
  float3x3 view = float3x3(cb12[4].xyz, cb12[5].xyz, cb12[6].xyz);
  float3 normal = mul(nativeNormal, view);
  float scale = rsqrt(dot(normal, normal));
  normal *= scale;
  float3 tangent = mul(nativeTangent.xyz, view) * scale;
  o0 = float4(normal, 1);
  o1 = float4(tangent, 0);
  o2 = cross(normal, tangent) * nativeTangent.w;
  float2 atlas = nativeUv * float2({:.9},{:.9}) + float2({:.9},{:.9});
  float2 uv = (atlas * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});
  o3 = uv.xyxy;
  float3 position = nativePosition.xyz * cb11[5].www + cb11[5].xyz;
  position.z -= cb0[11].x;
  o4 = mul(position, view) + cb12[7].xyz;
  float4 clip = position.x * cb0[6] + position.y * cb0[7] + position.z * cb0[8] + cb0[9];
  float inverse = 1 / clip.w;
  o5 = float4(clip.xy * inverse + cb0[10].xy, clip.zw * inverse);
}}
",
        uv[0], uv[1], uv[2], uv[3]
    );
    c.graph.program(
        &vertex_name,
        &text,
        &c.refs.join("shaders/vertex.hlsl"),
        true,
        &c.out,
    )?;
    let shader = format!("{vertex_name}-shader");
    let mut mat = shell.0.clone();
    put(&mut mat, 0x48, &u32::MAX.to_le_bytes())?;
    c.graph.add(
        &name,
        template,
        &mat,
        None,
        vec![json!({"offset":0x48,"symbol":shader})],
    )?;
    if let Some(rows) = c.graph.manifest["source_runtime_material_draws"].as_array_mut() {
        rows.retain(|row| !(row["model"] == draw.model_tag && row["stage"] == 16));
    }
    let side = VertexSide {
        shader,
        lens: Program {
            code: RETICLE_LENS_PROGRAM.to_vec(),
            constants: array_bytes(shell, 0x78, 16)?,
            values,
            evidence: vec![],
            samplers: BTreeMap::new(),
            textures: BTreeMap::new(),
        },
        textures: BTreeMap::new(),
        resources: Resources {
            rows: vec![],
            patches: vec![],
        },
        objects: usize::try_from(shell.u32(0xB8)?)?,
        flags: shell.u8(0xBC)?,
    };
    let evidence = json!({"model":draw.model_tag,"material":draw.material,"runtime_material":"native reticle","native_reticle_equations":true,"source_vertex_equations_retained":false,"source_pixel_equations_retained":false});
    Ok((name, side, evidence))
}

/// Native reticle pixel programs discard every pixel whose integer mask lacks bit 2, which
/// a native sight gets from the stage-14 lens drawn in front of it. When the source has no
/// stage-14 draw, each reticle gets a lens over its own faces from the native lens material
/// running the reticle's vertex shader, so the lens covers exactly the reticle's pixels.
fn lenses(
    c: &mut Effect,
    records: &[(SourceDraw, String)],
    sides: &BTreeMap<String, VertexSide>,
) -> Result<()> {
    if records.is_empty() || !c.source.draws(14)?.is_empty() {
        return Ok(());
    }
    let template = u64::from(c.contracts()?.carrier(Role::OpticStencil)?.material);
    let shell = c.contracts()?.material(&c.refs, Role::OpticStencil)?;
    let mut created = BTreeMap::new();
    let mut evidence = vec![];
    for (draw, reticle) in records {
        if !created.contains_key(reticle) {
            let side = sides
                .get(reticle)
                .context("reticle vertex shader for its lens")?;
            let name = format!("{reticle}-lens");
            let mut mat = shell.0.clone();
            let mut patches = vec![];
            side.apply_lens(&mut mat, &mut patches)?;
            c.graph.add(&name, template, &mat, None, patches)?;
            evidence.push(json!({"reticle":reticle,"lens":name,"native_lens_pixel_shader":format!("{:08X}",shell.u32(0x2C8)?)}));
            created.insert(reticle.clone(), name);
        }
        for (channel, faces) in &draw.groups {
            c.draws.add(14, draw, *channel, faces, &created[reticle])?;
            // Reticle draws carry the translucent-stage bit 0x10, and no native stage-14 draw
            // does (census of the stock packages, 2026-10-05).
            c.draws.records[14]
                .last_mut()
                .context("lens draw record")?
                .0[0x18] &= !0x10;
        }
    }
    c.draws.layout(14)?;
    c.graph.manifest["source_stage_14_adapter"]["reticle_lenses"] = json!(evidence);
    Ok(())
}

/// What every material of one source stage is built against: the native carrier material, its
/// template and sampler pool, and the dyes, lens sides and evidence rows the stage collects.
struct StageBuild<'a> {
    shell: &'a Payload,
    template: u64,
    pool: &'a Vec<serde_json::Value>,
    dyes: &'a mut Option<dyemap::Dyes>,
    sides: &'a mut BTreeMap<String, VertexSide>,
    evidence: &'a mut Vec<serde_json::Value>,
}

pub(super) fn build(c: &mut Effect, prepared: &Path, stage: usize) -> Result<()> {
    if c.source.draws(stage)?.is_empty() {
        c.draws.records[stage].clear();
        c.draws.layout(stage)?;
        c.graph.manifest[format!("source_stage_{stage}_adapter")] =
            json!({"materials":[],"implementation":"Rust","gameplay_verified":false});
        return Ok(());
    }
    let pool = c.contracts()?.samplers.clone();
    let role = match stage {
        0 => Role::Surface,
        1 => Role::Decal,
        3 => Role::ShadowPixel,
        12 => Role::DepthPixel,
        7 | 9 => Role::Emission,
        14 => Role::OpticStencil,
        16 => Role::Reticle,
        _ => anyhow::bail!("unsupported source draw stage"),
    };
    let shell = c.contracts()?.material(&c.refs, role)?;
    let template = u64::from(c.contracts()?.carrier(role)?.material);
    let mut dyes = None;
    let mut created = BTreeMap::new();
    let mut sides = BTreeMap::new();
    let mut evidence = vec![];
    let mut records = vec![];
    for draw in c.source.draws(stage)? {
        if draw.material == "FFFFFFFF" && stage == 16 {
            let key = (draw.model, draw.material.clone());
            if !created.contains_key(&key) {
                let (name, side, row) = runtime_reticle(c, &draw, &shell, template)?;
                evidence.push(row);
                sides.insert(name.clone(), side);
                created.insert(key.clone(), name);
            }
            records.push((draw, created[&key].clone()));
            continue;
        }
        if draw.material == "FFFFFFFF" {
            let omitted = &mut c.graph.manifest["source_runtime_material_draws"];
            if omitted.is_null() {
                *omitted = json!([]);
            }
            let record = json!({"model":draw.model_tag,"stage":stage,
                "reason":"draw requires a runtime material override absent from the source asset"});
            let records = omitted
                .as_array_mut()
                .context("runtime material omissions")?;
            if !records.contains(&record) {
                records.push(record);
            }
            continue;
        }
        let key = (draw.model, draw.material.clone());
        if !created.contains_key(&key) {
            let stage_build = StageBuild {
                shell: &shell,
                template,
                pool: &pool,
                dyes: &mut dyes,
                sides: &mut sides,
                evidence: &mut evidence,
            };
            let Some(name) = build_material(c, prepared, stage, &draw, stage_build)? else {
                continue;
            };
            created.insert(key.clone(), name);
        }
        records.push((draw, created[&key].clone()));
    }
    c.draws.records[stage].clear();
    for (draw, symbol) in &records {
        for (channel, faces) in &draw.groups {
            c.draws.add(stage, draw, *channel, faces, symbol)?;
        }
    }
    c.draws.layout(stage)?;
    c.graph.manifest[format!("source_stage_{stage}_adapter")] =
        json!({"materials":evidence,"implementation":"Rust","gameplay_verified":false});
    if stage == 16 {
        lenses(c, &records, &sides)?;
    }
    Ok(())
}

/// One source material of a stage as a native material: its pixel and vertex programs, textures,
/// samplers and scopes, named by model and material. `None` when a shadow or depth stage's material
/// has no pixel shader to translate.
#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
fn build_material(
    c: &mut Effect,
    prepared: &Path,
    stage: usize,
    draw: &SourceDraw,
    build: StageBuild<'_>,
) -> Result<Option<String>> {
    let StageBuild {
        shell,
        template,
        pool,
        dyes,
        sides,
        evidence,
    } = build;
    let plated = c.source.report["models"][draw.model]["has_texture_plates"] != false;
    let material = c.source.raw(&draw.material)?;
    if matches!(stage, 3 | 12) && material.u32(0x2B0)? == u32::MAX {
        return Ok(None);
    }
    let blend = material.u8(48)? & 127;
    if !supported_blend(stage, blend) {
        return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
            "source blend differs"
        )));
    }
    let ps = material.u32(0x2B0)?;
    let source_path = c
        .refs
        .join(format!("library-surfaces-01/source-shaders/{ps:08X}.hlsl"));
    let source = fs::read_to_string(&source_path)
        .with_context(|| format!("source shader {}", source_path.display()))?
        .replace("\r\n", "\n");
    let binding = c
        .bindings
        .get(&draw.material)
        .context("source material bindings")?
        .clone();
    let (source, null_glow) = null_glow::specialize(&source, &material, &binding)?;
    let (source, native_lighting) = lighting::adapt(&source)
        .with_context(|| format!("source shader {ps:08X} material {}", draw.material))
        .map_err(crate::d2_mot::source_limit)?;
    // Scene textures belong to the transparent scope even when the shader does
    // not consume its depth constants. Explicit material bindings remain owned
    // by the material and do not establish a renderer input.
    let mut bound_textures = binding["textures"]
        .as_array()
        .context("source fixed textures")?
        .iter()
        .map(|texture| Ok(u32::try_from(number(&texture["slot"])?)?))
        .collect::<Result<BTreeSet<_>>>()?;
    let pixel_code = array_bytes(&material, 0x2D0, 1)?;
    for instruction in program::parse(&pixel_code)? {
        if instruction.op == 0x56 && instruction.args[0] >> 5 == 1 {
            bound_textures.insert(u32::from(instruction.args[0] & 31));
        }
    }
    let scene_textures = inputs::slots(&source, 't')?
        .difference(&bound_textures)
        .copied()
        .filter(|slot| (10..=13).contains(slot))
        .collect::<BTreeSet<_>>();
    let scene_scope = matches!(stage, 7 | 9)
        && (inputs::cb_count(&source, 2)? == Some(1)
            || (material.u64(0x20)? & (1 << 13) != 0 && !scene_textures.is_empty()));
    let source = if scene_scope {
        super::super::shader::packed::Scopes::read(&c.refs)
            .and_then(|scopes| scopes.pixel(&source))
            .map_err(crate::d2_mot::source_limit)?
    } else {
        source
    };
    let dye_inputs = if binding["implicit_dyes"] == true {
        inputs::dye_inputs_with_palette(&source, &material, &c.refs, true)?
    } else {
        inputs::dye_inputs(&source, &material, &c.refs)?
    };
    let has_dyes = !dye_inputs.is_empty();
    let runtime_dyes = has_dyes && c.source.report["independent_art_entity"].is_string();
    if has_dyes && !runtime_dyes && dyes.is_none() {
        *dyes = Some(
            dyemap::dyes(prepared)
                .context("source dyes")
                .map_err(crate::d2_mot::source_limit)?,
        );
    }
    let resources = binding["samplers"].as_array().context("source resources")?;
    let resource_map = resources
        .iter()
        .enumerate()
        .filter(|(_, r)| r["direct_sampler"] != true)
        .map(|(i, _)| Ok((u8::try_from(i)?, u8::try_from(i)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut pixel_program = program(
        c,
        draw.model,
        &material,
        0x2B0,
        &source,
        dyes.as_ref().filter(|_| has_dyes),
        resource_map,
        Some(&binding),
        true,
    )
    .with_context(|| format!("stage {stage} material {} pixel program", draw.material))?;
    let base_fallbacks = pixel_program
        .evidence
        .iter()
        .flat_map(|row| row["base_value_fallbacks"].as_array().into_iter().flatten())
        .cloned()
        .collect::<Vec<_>>();
    if let Some(previous) = c.graph.manifest["material_base_fallbacks"].as_array_mut() {
        previous.retain(|row| {
            row["model"] != draw.model_tag
                || row["material"] != draw.material
                || row["stage"] != stage
        });
    }
    if !base_fallbacks.is_empty() {
        if c.graph.manifest["material_base_fallbacks"].is_null() {
            c.graph.manifest["material_base_fallbacks"] = json!([]);
        }
        c.graph.manifest["material_base_fallbacks"]
            .as_array_mut()
            .context("material base fallbacks")?
            .push(json!({"model":draw.model_tag,"material":draw.material,
                "stage":stage,"outputs":base_fallbacks}));
    }
    if runtime_dyes {
        ensure!(
            !pixel_program
                .textures
                .keys()
                .any(|slot| slot >> 5 == 1 && (3..=9).contains(&(slot & 31))),
            "Source material writes over the native runtime dye texture bindings"
        );
    }
    if native_lighting {
        pixel_program.code.extend(lighting::bindings(&c.refs)?);
        for slot in 27..=31 {
            pixel_program.textures.insert(0x20 | slot, 0);
        }
        c.graph.manifest["native_forward_lighting"] = json!({"irradiance":"Native RGB spherical-harmonic atlases and shadow factor","reflection":"Native environment hemisphere","modern_local_probe_volumes":false,"source_material_equations_retained":true,"gameplay_verified":false});
    }
    let vertex = vertex::build(c, draw, &material)
        .with_context(|| format!("material {} vertex shader", draw.material))?;
    let vertex_bindings = vertex_resources(
        c,
        draw.model,
        stage,
        &material,
        &vertex,
        binding.get("vertex"),
        pool,
    )?;
    let vertex_program = vertex_bindings.program;
    let binding = c
        .bindings
        .get(&draw.material)
        .context("source opaque bindings")?
        .clone();
    let expected = resources
        .iter()
        .enumerate()
        .filter(|(_, r)| r["direct_sampler"] == true)
        .map(|(i, _)| i)
        .map(|i| Ok((u8::try_from(i + 1)?, u8::try_from(i)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        pixel_program.samplers == expected,
        "source opaque sampler program differs"
    );
    let Resources {
        rows: sampler_rows,
        patches: sampler_patches,
    } = samplers(c, pool, &binding)?;
    let mut textures = BTreeMap::new();

    let used = inputs::slots(&source, 't')?;
    if matches!(stage, 7 | 9) {
        // Either source scene color input becomes native's one resolved scene color.
        let scene = [22u8, 23]
            .into_iter()
            .filter(|slot| used.contains(&u32::from(*slot)))
            .collect::<Vec<_>>();
        if let [slot] = scene[..] {
            ensure!(
                !used.contains(&18),
                "native screen-refraction slot is occupied"
            );
            lighting::check_refraction(&c.refs, slot)?;
        } else {
            ensure!(
                scene.is_empty(),
                "source reads both scene color inputs, which native binds as one"
            );
        }
    }
    for (slot, kind) in [(0, "albedo"), (1, "normal"), (2, "gstack")] {
        if plated && used.contains(&slot) {
            textures.insert(slot, surface(c, kind)?);
        }
    }
    if has_dyes && !runtime_dyes {
        for channel in 4..=6 {
            for detail in 0..2 {
                let slot = 4 + (channel - 4) * 2 + detail;
                if used.contains(&slot) {
                    let symbol = format!("dye-{channel}-texture-{detail}");
                    c.graph.node(&symbol)?;
                    textures.insert(slot, symbol);
                }
            }
        }
    }
    for tex in binding["textures"]
        .as_array()
        .context("source fixed textures")?
    {
        let slot = u32::try_from(number(&tex["slot"])?)?;
        ensure!(
            !plated || slot >= 3,
            "source material overrides a native plate"
        );
        if used.contains(&slot) {
            ensure!(
                !runtime_dyes || !(4..=9).contains(&slot),
                "Source fixed texture overrides a runtime dye texture"
            );
            if slot == 26 && matches!(stage, 7 | 9) {
                ensure!(
                    !used.contains(&12),
                    "Source reflection texture conflicts with the native cubemap slot"
                );
                textures.insert(12u32, emission::texture(c, tex)?);
            } else {
                textures.insert(slot, emission::texture(c, tex)?);
            }
        }
    }
    if plated
        && used.contains(&3)
        && !textures.contains_key(&3)
        && !pixel_program.textures.contains_key(&0x23)
    {
        textures.insert(3, dyemap::texture(c, draw.model)?);
    }
    // Runtime texture outputs own their destination slots. A dye or
    // fixed binding must not overwrite an explicitly translated input.
    for slot in pixel_program
        .textures
        .keys()
        .filter(|slot| **slot >> 5 == 1)
    {
        textures.remove(&u32::from(slot & 31));
    }
    let missing = used
        .iter()
        .copied()
        .filter(|slot| {
            !(textures.contains_key(slot)
                || (runtime_dyes && (4..=9).contains(slot))
                || pixel_program.textures.contains_key(&(0x20 | *slot as u8))
                || (*slot == 26 && textures.contains_key(&12))
                || (scene_scope && [10, 11, 12, 13].contains(slot))
                || (matches!(stage, 7 | 9) && [15, 20, 21, 22, 23].contains(slot)))
        })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        // These resources belong to the source renderer. Retrying an
        // animation donor cannot add a missing renderer conversion.
        return Err(crate::d2_mot::source_limit(anyhow::anyhow!(
            "source pixel stage {stage} has unmapped texture inputs {missing:?}"
        )));
    }
    let name = format!("source-stage-{stage}-{}-{}", draw.model, draw.material);
    let vertex_name = format!("{name}-vertex");
    c.graph.program(
        &vertex_name,
        &vertex.text,
        &c.refs.join("shaders/vertex.hlsl"),
        true,
        &c.out,
    )?;
    let scale = c.source.raw(&draw.model_tag)?.f32(0x6C)?;
    if inputs::cb_count(&source, 12)? == Some(15) {
        inputs::validate_pixel_view(&c.refs)?;
    }
    let mut pixel = inputs::model_constants(&source, scale)?;
    let mut dye_scope_mask = 0;
    let mut dye_slots = BTreeSet::new();
    if runtime_dyes {
        (pixel, dye_scope_mask, dye_slots) = inputs::runtime_dyes(&pixel, &dye_inputs, &c.refs)?;
    } else {
        pixel = inputs::merge_dyes(
            &pixel,
            dyes.as_ref().map_or(0, |dyes| dyes.values.len() / 16),
            &dye_inputs,
        )?;
    }
    let (rect, size) = c.atlas(draw.model)?;
    pixel = if plated {
        inputs::pixel(&pixel, rect, size)?
    } else {
        inputs::pixel_with_plates(&pixel, None)?
    };
    if runtime_dyes {
        pixel = inputs::dye_textures(&pixel);
        textures = textures
            .into_iter()
            .map(|(slot, value)| (inputs::dye_texture_slot(slot), value))
            .collect();
    }
    if matches!(stage, 7 | 9) {
        pixel = atmosphere::adapt(&pixel)?;
        for (from, to) in [(20, 16), (21, 17), (22, 18), (23, 18), (26, 12)] {
            pixel = pixel.replace(
                &format!("t{from} : register(t{from})"),
                &format!("t{to} : register(t{to})"),
            );
            pixel = pixel.replace(&format!("t{from}."), &format!("t{to}."));
        }
        ensure!(
            inputs::cb_count(&pixel, 13)?.is_none_or(|n| n == 2)
                && inputs::cb_count(&pixel, 8)?.is_none_or(|n| n == 8 || (scene_scope && n == 4)),
            "source atmospheric constant contract differs"
        );
    }
    if stage == 1 && pixel_program.textures.get(&0x2A) == Some(&0x25) {
        ensure!(
            !used.contains(&5) && !textures.contains_key(&5),
            "decal runtime texture conflicts with source texture 5"
        );
        pixel = pixel
            .replace("t10 : register(t10)", "t5 : register(t5)")
            .replace("t10.", "t5.");
    }
    let immutable = constants::specialize(&pixel, &pixel_program.values, &pixel_program.code)?;
    let static_pixel_constants = immutable.is_some();
    if let Some(specialized) = immutable {
        pixel = specialized;
    }
    if scene_scope {
        dye_slots.insert(2);
    }
    if dye_slots.is_empty() {
        inputs::pixel_scopes(&pixel, stage)?;
    } else {
        inputs::pixel_scopes_with_dyes(&pixel, stage, &dye_slots)?;
    }
    c.graph.program(
        &name,
        &pixel,
        &c.refs.join("shaders/pixel.hlsl"),
        false,
        &c.out,
    )?;
    let mut mat = shell.0.clone();
    if stage == 1 {
        if blend == 57 {
            // Alpha-tested three-target decals (including Abyss Defiant's
            // attachment) have a native 0xB9 state. Preserve the source
            // pixel discard equations and require the exact native state.
            let carrier = c.contracts()?.material(&c.refs, Role::AlphaDecal)?;
            ensure!(
                carrier.u32(32)? == 0xB9 && material.u32(48)? == 0xB9,
                "source alpha-tested decal state differs from native"
            );
        }
        put(&mut mat, 32, &(0x80u32 | blend as u32).to_le_bytes())?;
    }
    for at in [24, 28] {
        // These source atlases are fixed resident resources. The gear
        // scope otherwise replaces their explicit pixel bindings.
        let mut scopes = (shell.u32(at)? & !0x1E000000) | u32::try_from(dye_scope_mask)?;
        if inputs::cb_count(&pixel, 8)?.is_some() {
            scopes |= 1 << 14;
        }
        if scene_scope || (matches!(stage, 7 | 9) && used.contains(&22)) {
            scopes |= 1 << 13;
        }
        put(&mut mat, at, &scopes.to_le_bytes())?;
    }
    let mut patches = vec![
        json!({"offset":0x48,"symbol":format!("{vertex_name}-shader")}),
        json!({"offset":0x2C8,"symbol":format!("{name}-shader")}),
    ];
    for at in [0x48, 0x2C8] {
        put(&mut mat, at, &u32::MAX.to_le_bytes())?;
    }
    fixed(&mut mat, 0x48, &vertex_bindings.textures, &mut patches)?;
    fixed(&mut mat, 0x2C8, &textures, &mut patches)?;
    set_program(&mut mat, 0x48, &vertex_program, c.objects.len())?;
    set_program(&mut mat, 0x2C8, &pixel_program, c.objects.len())?;
    if stage == 16 {
        sides.insert(
            name.clone(),
            VertexSide {
                shader: format!("{vertex_name}-shader"),
                lens: vertex_bindings
                    .lens
                    .clone()
                    .context("reticle lens vertex program")?,
                textures: vertex_bindings.textures.clone(),
                resources: vertex_bindings.resources.clone(),
                objects: c.objects.len(),
                flags: 0,
            },
        );
    }
    set_resources(&mut mat, 0x48, vertex_bindings.resources, &mut patches)?;
    append_array(&mut mat, 0x308, 0x808073F3, &sampler_rows, 16)?;
    if !sampler_patches.is_empty() {
        let start = Payload(mat.clone()).pointer(0x310)? + 16;
        for (offset, symbol) in sampler_patches {
            patches.push(json!({"offset":start+offset,"symbol":symbol}));
        }
    }
    c.graph.add(&name, template, &mat, None, patches)?;
    // Recorded separately so refreshed graphs can verify specialization.
    if static_pixel_constants {
        c.graph.manifest["immutable_pixel_buffers"][&name] = json!(pixel_program.values.len() / 16);
    }
    evidence.push(json!({"model":draw.model_tag,"material":draw.material,"pixel_shader":format!("{ps:08X}"),"vertex_shader":format!("{:08X}",material.u32(0x70)?),"source_vertex_equations_retained":true,"source_pixel_equations_retained":!native_lighting,"source_material_equations_retained":base_fallbacks.is_empty(),"material_base_fallbacks":base_fallbacks,"native_transparent_scope":scene_scope,"scene_textures":scene_textures,"native_forward_lighting":native_lighting,"null_glow_mask_specialized":null_glow,"native_plate_scope_retained":false,"native_runtime_dyes":runtime_dyes,"fixed_surface_textures":true,"pixel_runtime_outputs":pixel_program.evidence,"vertex_runtime_outputs":vertex_program.evidence}));
    Ok(Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decal_resource_mapping_follows_the_program_across_supported_blend_states() {
        for blend in [26, 27, 29, 33, 36, 57] {
            let code = [0x4D, 0x2D, 1, 0x56, 0x2A];
            let mut material = vec![0; 0x400];
            material[48] = blend | 0x80;
            append_array(&mut material, 0x2D0, 0x80800009, &code, 1).unwrap();
            let mut bindings = Bindings::default();
            texture_contract(&Payload(material), 0x2B0, &mut bindings, true).unwrap();
            let result = program::lower(&code, &bindings).unwrap();
            assert_eq!(result.code, [0x3F, 0x2C, 1, 0x47, 0x25]);
            assert_eq!(result.textures, BTreeMap::from([(0x2A, 0x25)]));
            assert!(program::lower(&[0x4D, 0x2D, 2, 0x56, 0x2A], &bindings).is_err());
        }
    }

    #[test]
    fn sky_hemisphere_uses_native_extern_and_preserves_material_slot() {
        for slot in [0x24, 0x26, 0x34] {
            let code = [0x4D, 3, 0x1B, 0x56, slot];
            let mut material = vec![0; 0x400];
            append_array(&mut material, 0x2D0, 0x80800009, &code, 1).unwrap();
            let mut bindings = Bindings::default();
            texture_contract(&Payload(material), 0x2B0, &mut bindings, true).unwrap();
            let result = program::lower(&code, &bindings).unwrap();
            assert_eq!(result.code, [0x3F, 3, 0x13, 0x47, slot]);
            assert_eq!(result.textures, BTreeMap::from([(slot, slot)]));
            assert!(program::lower(&[0x4D, 3, 0x1A, 0x56, slot], &bindings).is_err());
        }
    }

    #[test]
    fn static_donor_gets_writable_storage_for_imported_vertex_outputs() {
        // Insidious 80D0B07C wrote output 1 through a null pointer because
        // its donor's writable-buffer flag remained zero.
        let p = Program {
            code: hex::decode("4d0422003c01013400031a23014301").unwrap(),
            constants: 0.0625f32.to_le_bytes().into_iter().chain([0; 12]).collect(),
            values: vec![0; 3 * 16],
            evidence: vec![json!({"output":1,"translated":true})],
            samplers: BTreeMap::new(),
            textures: BTreeMap::new(),
        };
        let mut material = vec![0; 0x400];
        set_program(&mut material, 0x48, &p, 7).unwrap();
        let material = Payload(material);
        assert_eq!(material.u8(0xBC).unwrap(), 0x10);
        assert_eq!(material.u64(0x98).unwrap(), 3);
        assert_eq!(array_bytes(&material, 0x68, 1).unwrap(), p.code);
        assert_eq!(array_bytes(&material, 0x98, 16).unwrap(), p.values);
    }
}
