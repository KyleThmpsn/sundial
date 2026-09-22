//! Source material equations with native runtime and fixed surface bindings.
use super::super::contracts::Role;
use super::*;

fn surface(c: &mut Effect, kind: &str) -> Result<String> {
    let name = format!("source-surface-{kind}");
    if c.graph.node(&name).is_ok() {
        return Ok(name);
    }
    let mut header = c.graph.read(&format!("texture-{kind}-header"))?.0;
    let data = c.graph.read(&format!("texture-{kind}-data"))?.0;
    let format = Payload(header.clone()).u32(4)?;
    let donor = if matches!(format, 72 | 99) {
        "dye-4-texture-0"
    } else {
        "dye-4-texture-1"
    };
    let resident = c.graph.read(donor)?;
    ensure!(
        header.len() == 40 && resident.0.len() == 40,
        "surface header differs"
    );
    header[24..36].copy_from_slice(&resident.0[24..36]);
    crate::d2_mot::texture::resident(&mut header, data.len())?;
    let ht = c.graph.node(donor)?["template"]
        .as_u64()
        .context("surface header template")?;
    let dt = c.graph.node(&format!("{donor}-data"))?["template"]
        .as_u64()
        .context("surface data template")?;
    let body = format!("{name}-data");
    c.graph.add(&name, ht, &header, Some(&body), vec![])?;
    c.graph.add(&body, dt, &data, Some(&name), vec![])?;
    Ok(name)
}

struct Program {
    code: Vec<u8>,
    constants: Vec<u8>,
    values: Vec<u8>,
    evidence: Vec<Value>,
    samplers: BTreeMap<u8, u8>,
    textures: BTreeMap<u8, u8>,
}

fn texture_contract(material: &Payload, base: usize, bindings: &mut Bindings) -> Result<()> {
    // Verified hemisphere resources. Deferred specular mips moved from +0x98
    // to +0xD8, and the Atmosphere sky lookup from +0x90 to +0x100. Native and
    // source shaders use the same hemisphere projection for each resource.
    // Preserve the material's output slot, which varies between materials.
    let code = array_bytes(material, base + 0x20, 1)?;
    let instructions = program::parse(&code)?;
    for pair in instructions.windows(2) {
        let mapped = match pair[0].args {
            [3, 0x1B] => Some([0x3F, 3, 0x13]),
            [7, 0x20] => Some([0x3F, 7, 0x12]),
            _ => None,
        };
        if let Some(mapped) = mapped.filter(|_| pair[0].op == 0x4D && pair[1].op == 0x56) {
            let slot = pair[1].args[0];
            ensure!(
                slot >> 5 == 1,
                "sky texture has an unsupported shader stage"
            );
            bindings
                .external_textures
                .insert([0x4D, pair[0].args[0], pair[0].args[1]], mapped);
            bindings.texture_slots.insert(slot, slot);
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
    texture_contract(material, base, &mut bindings)?;
    program::lower(&code, &bindings).map_err(crate::d2_mot::source_limit)?;
    Ok(())
}

fn program(
    c: &Effect,
    material: &Payload,
    base: usize,
    text: &str,
    dyes: Option<&dyemap::Dyes>,
    textures: BTreeMap<u8, u8>,
    resource_bindings: Option<&Value>,
) -> Result<Program> {
    let external = material.u32(base + 0x74)?;
    let mut values = if [0, u32::MAX, 0x811C9DC5].contains(&external) {
        array_bytes(material, base + 0x50, 16)?
    } else {
        let manifest = load(&c.bindings_root.join("source-manifest.json"))?;
        let entry = &manifest["tags"][format!("{external:08X}")];
        let reference = entry["reference"]
            .as_u64()
            .context("external source constant buffer")?;
        let header = Payload(fs::read(
            c.bindings_root.join(format!("raw/{external:08X}.bin")),
        )?);
        let bytes = fs::read(c.bindings_root.join(format!("raw/{reference:08X}.bin")))?;
        ensure!(
            header.0.len() == 16
                && header.u32(0)? as usize == bytes.len()
                && bytes.len().is_multiple_of(16),
            "source external constant buffer differs"
        );
        bytes
    };
    let count = values.len() / 16;
    ensure!(
        inputs::cb_count(text, 0)?.unwrap_or(0) == count,
        "source constant table and shader disagree"
    );
    let mut constants = array_bytes(material, base + 0x30, 16)?;
    let resources = material.u64(base + 0x40)? as usize;
    let code = array_bytes(material, base + 0x20, 1)?;
    let mut bindings = Bindings {
        objects: c.objects.clone(),
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
        let value = if instruction.op == 0x61 {
            ensure!(
                (0..4).all(|i| header.f32(16 + i * 4).is_ok_and(f32::is_finite)),
                "nonfinite texture tiling"
            );
            header.bytes::<16>(16)?
        } else {
            let values = [
                f32::from(header.u16(42)?),
                f32::from(header.u16(40)?),
                0.,
                0.,
            ];
            values
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        };
        bindings
            .texture_metadata
            .insert(key, u8::try_from(constants.len() / 16)?);
        constants.extend(value);
    }
    bindings.constant_count = constants.len() / 16;
    texture_contract(material, base, &mut bindings)?;
    // This lowers the source material's own TFX program, so a translation gap
    // here is a converter limit no native donor can satisfy.
    let mut lowered = program::lower(&code, &bindings).map_err(crate::d2_mot::source_limit)?;
    let reads = inputs::reads(text, 0);
    for row in &mut lowered.evidence {
        if !global_fallbacks.is_empty() {
            row["source_global_defaults"] = json!(global_fallbacks);
        }
        row["required_by_shader"] = json!(
            reads
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
    resources: Resources,
    textures: BTreeMap<u32, String>,
}

fn vertex_resources(
    c: &mut Effect,
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
    let program = program(c, material, 0x70, &vertex.source, None, map, Some(binding))?;
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

fn decal_donor(c: &Effect, blend: u8) -> Result<Vec<u8>> {
    ensure!(supported_blend(1, blend), "source decal blend differs");
    let role = if blend == 29 {
        Role::AdditiveDecal
    } else {
        Role::Decal
    };
    Ok(c.contracts()?.carrier(role)?.record.clone())
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
/// converter does not emit, so it reuses the decal carrier record.
const DECAL_STATES: [u8; 6] = [26, 27, 29, 33, 36, 57];

/// Blend equations this converter implements for each retained source stage.
/// The value comes from the source material alone, so an unsupported blend
/// fails identically for every native donor.
pub(super) fn supported_blend(stage: usize, blend: u8) -> bool {
    if stage == 1 {
        DECAL_STATES.contains(&blend)
    } else {
        blend == if matches!(stage, 0 | 3 | 12) { 0 } else { 8 }
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
    let donor = carrier.record.clone();
    let shell = c.contracts()?.material(&c.refs, role)?;
    let pool = c.contracts()?.samplers.clone();
    let mut materials = BTreeMap::new();
    let mut records = vec![];
    let mut evidence = vec![];
    for draw in c.source.draws(stage)? {
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
            let vertex_bindings =
                vertex_resources(c, stage, &material, &vertex, binding.as_ref(), &pool)?;
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
            set_resources(&mut mat, 0x48, vertex_bindings.resources, &mut patches)?;
            append_array(&mut mat, 0x88, 0x808073F3, &[], 16)?;
            c.graph
                .add(&name, u64::from(material_tag), &mat, None, patches)?;
            evidence.push(json!({"model":draw.model_tag,"material":draw.material,"vertex_shader":format!("{:08X}",material.u32(0x70)?),"vertex_runtime_outputs":program.evidence,"source_vertex_equations_retained":true}));
            materials.insert(key.clone(), name);
        }
        records.push((draw, materials[&key].clone()));
    }
    for (draw, name) in records {
        for (channel, faces) in &draw.groups {
            c.draws.add(stage, &donor, &draw, *channel, faces, &name)?;
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

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
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
        3 => Role::Shadow,
        12 => Role::Depth,
        7 | 9 => Role::Emission,
        _ => anyhow::bail!("unsupported source draw stage"),
    };
    let shell = c.contracts()?.material(&c.refs, role)?;
    let template = u64::from(c.contracts()?.carrier(role)?.material);
    let donor = if stage == 0 {
        c.draws.records[0]
            .first()
            .context("native opaque draw")?
            .0
            .clone()
    } else if stage == 7 {
        c.contracts()?.carrier(Role::Transparent)?.record.clone()
    } else {
        c.contracts()?.carrier(role)?.record.clone()
    };
    let mut dyes = None;
    let mut created = BTreeMap::new();
    let mut evidence = vec![];
    let mut records = vec![];
    for draw in c.source.draws(stage)? {
        let key = (draw.model, draw.material.clone());
        if !created.contains_key(&key) {
            let material = c.source.raw(&draw.material)?;
            if matches!(stage, 3 | 12) && material.u32(0x2B0)? == u32::MAX {
                continue;
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
            let (source, native_lighting) =
                lighting::adapt(&source).map_err(crate::d2_mot::source_limit)?;
            let dye_inputs = inputs::dye_inputs(&source, &material, &c.refs)?;
            let has_dyes = !dye_inputs.is_empty();
            if has_dyes && dyes.is_none() {
                dyes = Some(
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
                &material,
                0x2B0,
                &source,
                dyes.as_ref().filter(|_| has_dyes),
                resource_map,
                Some(&binding),
            )
            .with_context(|| format!("stage {stage} material {} pixel program", draw.material))?;
            if native_lighting {
                pixel_program.code.extend(lighting::bindings(&c.refs)?);
                for slot in 27..=31 {
                    pixel_program.textures.insert(0x20 | slot, 0);
                }
                c.graph.manifest["native_forward_lighting"] = json!({"irradiance":"Native RGB spherical-harmonic atlases and shadow factor","reflection":"Native environment hemisphere","modern_local_probe_volumes":false,"source_material_equations_retained":true,"gameplay_verified":false});
            }
            let vertex = vertex::build(c, &draw, &material)
                .with_context(|| format!("material {} vertex shader", draw.material))?;
            let vertex_bindings =
                vertex_resources(c, stage, &material, &vertex, binding.get("vertex"), &pool)?;
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
            } = samplers(c, &pool, &binding)?;
            let mut textures = BTreeMap::new();

            let used = inputs::slots(&source, 't')?;
            if matches!(stage, 7 | 9) && used.contains(&22) {
                ensure!(
                    !used.contains(&18),
                    "native screen-refraction slot is occupied"
                );
                lighting::check_refraction(&c.refs)?;
            }
            for (slot, kind) in [(0, "albedo"), (1, "normal"), (2, "gstack")] {
                if used.contains(&slot) {
                    textures.insert(slot, surface(c, kind)?);
                }
            }
            if has_dyes {
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
                ensure!(slot >= 3, "source material overrides a native plate");
                if used.contains(&slot) {
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
            if used.contains(&3) && !textures.contains_key(&3) {
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
                    !(*slot < 3
                        || textures.contains_key(slot)
                        || pixel_program.textures.contains_key(&(0x20 | *slot as u8))
                        || (*slot == 26 && textures.contains_key(&12))
                        || (matches!(stage, 7 | 9) && [15, 20, 21, 22].contains(slot)))
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
            pixel = inputs::merge_dyes(
                &pixel,
                dyes.as_ref().map_or(0, |dyes| dyes.values.len() / 16),
                &dye_inputs,
            )?;
            let (rect, size) = c.atlas(draw.model)?;
            pixel = inputs::pixel(&pixel, rect, size)?;
            if matches!(stage, 7 | 9) {
                pixel = atmosphere::adapt(&pixel)?;
                for (from, to) in [(20, 16), (21, 17), (22, 18), (26, 12)] {
                    pixel = pixel.replace(
                        &format!("t{from} : register(t{from})"),
                        &format!("t{to} : register(t{to})"),
                    );
                    pixel = pixel.replace(&format!("t{from}."), &format!("t{to}."));
                }
                ensure!(
                    inputs::cb_count(&pixel, 13)?.is_none_or(|n| n == 2)
                        && inputs::cb_count(&pixel, 8)?.is_none_or(|n| n == 8),
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
            inputs::pixel_scopes(&pixel, stage)?;
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
                let mut scopes = shell.u32(at)? & !0x1E000000;
                if inputs::cb_count(&pixel, 8)?.is_some() {
                    scopes |= 1 << 14;
                }
                if matches!(stage, 7 | 9) && used.contains(&22) {
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
            set_resources(&mut mat, 0x48, vertex_bindings.resources, &mut patches)?;
            append_array(&mut mat, 0x308, 0x808073F3, &sampler_rows, 16)?;
            if !sampler_patches.is_empty() {
                let start = Payload(mat.clone()).pointer(0x310)? + 16;
                for (offset, symbol) in sampler_patches {
                    patches.push(json!({"offset":start+offset,"symbol":symbol}));
                }
            }
            c.graph.add(&name, template, &mat, None, patches)?;
            evidence.push(json!({"model":draw.model_tag,"material":draw.material,"pixel_shader":format!("{ps:08X}"),"vertex_shader":format!("{:08X}",material.u32(0x70)?),"source_vertex_equations_retained":true,"source_pixel_equations_retained":!native_lighting,"source_material_equations_retained":true,"native_forward_lighting":native_lighting,"null_glow_mask_specialized":null_glow,"native_plate_scope_retained":false,"fixed_surface_textures":true,"pixel_runtime_outputs":pixel_program.evidence,"vertex_runtime_outputs":vertex_program.evidence}));
            created.insert(key.clone(), name);
        }
        records.push((draw, created[&key].clone()));
    }
    c.draws.records[stage].clear();
    for (draw, symbol) in records {
        let donor = if stage == 1 {
            decal_donor(c, c.source.raw(&draw.material)?.u8(48)? & 127)?
        } else {
            donor.clone()
        };
        for (channel, faces) in &draw.groups {
            c.draws
                .add(stage, &donor, &draw, *channel, faces, &symbol)?;
        }
    }
    c.draws.layout(stage)?;
    c.graph.manifest[format!("source_stage_{stage}_adapter")] =
        json!({"materials":evidence,"implementation":"Rust","gameplay_verified":false});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decal_resource_mapping_follows_the_program_across_supported_blend_states() {
        for blend in DECAL_STATES {
            let code = [0x4D, 0x2D, 1, 0x56, 0x2A];
            let mut material = vec![0; 0x400];
            material[48] = blend | 0x80;
            append_array(&mut material, 0x2D0, 0x80800009, &code, 1).unwrap();
            let mut bindings = Bindings::default();
            texture_contract(&Payload(material), 0x2B0, &mut bindings).unwrap();
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
            texture_contract(&Payload(material), 0x2B0, &mut bindings).unwrap();
            let result = program::lower(&code, &bindings).unwrap();
            assert_eq!(result.code, [0x3F, 3, 0x13, 0x47, slot]);
            assert_eq!(result.textures, BTreeMap::from([(slot, slot)]));
            assert!(program::lower(&[0x4D, 3, 0x1A, 0x56, slot], &bindings).is_err());
        }
    }

    #[test]
    fn atmosphere_hemisphere_uses_verified_native_lookup() {
        let code = [0x4D, 7, 0x20, 0x56, 0x24];
        let mut material = vec![0; 0x400];
        append_array(&mut material, 0x2D0, 0x80800009, &code, 1).unwrap();
        let mut bindings = Bindings::default();
        texture_contract(&Payload(material), 0x2B0, &mut bindings).unwrap();
        let result = program::lower(&code, &bindings).unwrap();
        assert_eq!(result.code, [0x3F, 7, 0x12, 0x47, 0x24]);
        assert_eq!(result.textures, BTreeMap::from([(0x24, 0x24)]));
        assert!(program::lower(&[0x4D, 7, 0x21, 0x56, 0x24], &bindings).is_err());
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

    #[test]
    fn sampler_only_program_keeps_static_constant_storage() {
        let p = Program {
            code: vec![0x4C, 0, 0x49, 0x20],
            constants: vec![],
            values: vec![0; 16],
            evidence: vec![],
            samplers: BTreeMap::new(),
            textures: BTreeMap::new(),
        };
        let mut material = vec![0; 0x400];
        set_program(&mut material, 0x2C8, &p, 2).unwrap();
        assert_eq!(material[0x33C], 0);
    }
}
