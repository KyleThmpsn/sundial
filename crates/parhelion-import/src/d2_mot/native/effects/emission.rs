//! Inspected source emission and light-shaft-occlusion families.
use super::*;
use crate::d2_mot::native::shader::replace_once;

pub(super) fn stage(shader: u32) -> Option<usize> {
    match shader {
        0x80D0307E | 0x80D01AF7 | 0x80CF957F => Some(7),
        0x80D03080 | 0x80D01AF9 | 0x80A6D52B => Some(9),
        _ => None,
    }
}

pub(super) fn texture(c: &mut Effect, binding: &Value) -> Result<String> {
    let source_tag = binding["tag"].as_str().context("source effect texture")?;
    let name = format!("effect-texture-{source_tag}");
    if c.graph.node(&name).is_ok() {
        return Ok(name);
    }
    let source = Payload(hex::decode(
        binding["header"]
            .as_str()
            .context("source texture header")?,
    )?);
    let (width, height, format, mips) = (
        source.u16(34)?,
        source.u16(36)?,
        source.u32(4)?,
        source.u8(45)?,
    );
    let layers = source.u16(40)? as usize;
    ensure!(
        source.u16(38)? == 1 && matches!(layers, 1 | 6),
        "effect texture must be 2D or cubemap"
    );
    ensure!(
        width > 0 && height > 0 && width <= 16384 && height <= 16384 && (1..=15).contains(&mips),
        "invalid effect texture dimensions"
    );
    let (block, tile) = match format {
        28 | 29 | 35 => (4usize, 1usize),
        71 | 72 | 80 | 81 => (8, 4),
        74 | 75 | 77 | 78 | 83 | 84 | 95 | 96 | 98 | 99 => (16, 4),
        _ => anyhow::bail!("uninspected effect texture format {format}"),
    };
    let mut data = vec![];
    let large = source.u32(60)?;
    if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
        data.extend(fs::read(
            c.bindings_root.join(format!("raw/{large:08X}.bin")),
        )?);
    }
    data.extend(fs::read(c.bindings_root.join(format!(
        "raw/{}.bin",
        binding["buffer"].as_str().context("texture buffer")?
    )))?);
    let expected = (0..mips)
        .map(|m| {
            ((width as usize >> m).max(1)).div_ceil(tile)
                * ((height as usize >> m).max(1)).div_ceil(tile)
                * block
                * layers
        })
        .sum::<usize>();
    ensure!(
        data.len() == expected && data.len() == source.u32(0)? as usize,
        "effect texture mip chain is incomplete"
    );
    let (mut header, ht, dt) = if layers == 6 {
        let native;
        let texture = if let Some(contracts) = &c.contracts {
            &contracts.cube
        } else {
            native = load(&c.refs.join("native-transparent-bindings/bindings.json"))?;
            &native["80EC271F"]["textures"][0]
        };
        (
            hex::decode(texture["header"].as_str().context("native cube header")?)?,
            u64::from(tag(&texture["tag"])?),
            u64::from(tag(&texture["buffer"])?),
        )
    } else {
        let donor = if matches!(format, 29 | 72 | 75 | 78 | 99) {
            "dye-4-texture-0"
        } else {
            "dye-4-texture-1"
        };
        (
            c.graph.read(donor)?.0,
            c.graph.node(donor)?["template"]
                .as_u64()
                .context("fixed texture template")?,
            c.graph.node(&format!("{donor}-data"))?["template"]
                .as_u64()
                .context("fixed texture data template")?,
        )
    };
    put(&mut header, 0, &u32::try_from(data.len())?.to_le_bytes())?;
    put(&mut header, 4, &format.to_le_bytes())?;
    put(&mut header, 14, &width.to_le_bytes())?;
    put(&mut header, 16, &height.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &(layers as u16).to_le_bytes())?;
    header[22] = source.u8(44)?;
    header[23] = mips;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::d2_mot::texture::resident(&mut header, data.len())?;
    let data_name = format!("{name}-data");
    c.graph.add(&name, ht, &header, Some(&data_name), vec![])?;
    c.graph.add(&data_name, dt, &data, Some(&name), vec![])?;
    Ok(name)
}

fn adapt(text: &str, count: usize, rect: [usize; 4], size: [usize; 2]) -> Result<String> {
    ensure!(
        text.matches(&format!("float4 cb0[{count}];")).count() == 1,
        "source emission constants differ"
    );
    ensure!(
        !text.contains("cb1[")
            && !text.contains("cb7[")
            && !text.contains("v2.w")
            && !text.contains("v3.zw")
            && !text.contains("v4.w"),
        "source emission requires additional vertex or dye inputs"
    );
    for semantic in 5..=9 {
        ensure!(
            !text.contains(&format!(": TEXCOORD{semantic}")),
            "source emission requires extra vertex inputs"
        );
    }
    let mut text = replace_once(text, "float4 v2 : TEXCOORD2", "float3 v2 : TEXCOORD2")?;
    if text.contains("float4 cb12[15];") {
        text = replace_once(&text, "float4 cb12[15];", "float4 cb12[13];")?;
    }
    text = text.replace("cb12[14].xyz", "cb12[7].xyz");
    ensure!(
        !text.contains("cb12[14]") && !text.contains("cb12[15]"),
        "unmapped source view input"
    );
    for (source, native) in [(20, 16), (21, 17)] {
        text = text.replace(
            &format!("t{source} : register(t{source})"),
            &format!("t{native} : register(t{native})"),
        );
        text = text.replace(&format!("t{source}."), &format!("t{native}."));
    }
    text = text.replace("v3.xy", "source_uv");
    let [x, y, w, h] = rect;
    let [aw, ah] = size;
    replace_once(
        &text,
        "uint4 bitmask, uiDest;",
        &format!(
            "uint4 bitmask, uiDest;\n  float2 source_uv = (v3.xy * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});"
        ),
    )
}

pub(super) fn build(c: &mut Effect) -> Result<()> {
    let mut pool = vec![];
    for file in [
        "native-transparent-bindings/bindings.json",
        "native-transparent-advanced-bindings/bindings.json",
        "native-stage-09-bindings/bindings.json",
    ] {
        for material in load(&c.refs.join(file))?
            .as_object()
            .context("native binding pool")?
            .values()
        {
            pool.extend(
                material["samplers"]
                    .as_array()
                    .context("native sampler pool")?
                    .iter()
                    .filter(|s| s["direct_sampler"] == true)
                    .cloned(),
            );
        }
    }
    let usage = load(&c.refs.join("emission-native-usage/usage.json"))?;
    let shell = Payload(fs::read(
        c.refs.join("native-stage-09-bindings/raw/80BFB12B.bin"),
    )?);
    ensure!(
        shell.u32(24)? == 0x2083 && shell.u32(32)? == 0x88,
        "native emission state contract differs"
    );
    let mut evidence = vec![];
    let mut created = BTreeMap::new();
    for render_stage in [7, 9] {
        let native_material = if render_stage == 7 {
            "80EC271F"
        } else {
            "80BFB12B"
        };
        let donor = usage["matches"]
            .as_array()
            .context("native effect draws")?
            .iter()
            .find(|v| v["material"] == native_material)
            .context("inspected native effect draw")?;
        ensure!(
            donor["stage"] == render_stage && donor["layout"] == 139,
            "native effect stage or vertex layout differs"
        );
        let donor = hex::decode(donor["record"].as_str().context("native effect record")?)?;
        for draw in c.source.draws(render_stage)? {
            let material = c.source.raw(&draw.material)?;
            let ps = material.u32(0x2B0)?;
            if stage(ps) != Some(render_stage) {
                continue;
            }
            ensure!(material.u8(48)? & 127 == 8, "source emission blend differs");
            let key = (draw.model, draw.material.clone());
            if !created.contains_key(&key) {
                let name = format!("source-emission-{}-{}", draw.model, draw.material);
                let binding = c
                    .bindings
                    .get(&draw.material)
                    .context("source emission bindings")?
                    .clone();
                let constants = array_bytes(&material, 0x2E0, 16)?;
                let values = vectors(&binding["constants"])?;
                let samplers = binding["samplers"]
                    .as_array()
                    .context("source emission samplers")?;
                let mut sampler_rows = vec![];
                for sampler in samplers {
                    ensure!(
                        sampler["direct_sampler"] == true,
                        "source emission uses uninspected resource lookup"
                    );
                    let native = pool.iter().find(|s| s["data"] == sampler["data"]).context(
                        "source emission sampler lacks a byte-identical native descriptor",
                    )?;
                    sampler_rows.extend(tag(&native["tag"])?.to_le_bytes());
                    sampler_rows.extend([0; 12]);
                }
                let lowered = program::lower(
                    &array_bytes(&material, 0x2D0, 1)?,
                    &Bindings {
                        objects: c.objects.clone(),
                        globals: c.globals.clone(),
                        constant_count: constants.len() / 16,
                        output_count: values.len() / 16,
                        sampler_count: samplers.len(),
                        ..Default::default()
                    },
                )?;
                let expected = (0..samplers.len())
                    .map(|i| Ok((u8::try_from(i + 1)?, u8::try_from(i)?)))
                    .collect::<Result<BTreeMap<_, _>>>()?;
                ensure!(
                    lowered.samplers == expected
                        && lowered.evidence.iter().all(|e| e["translated"] == true),
                    "unresolved source emission controls: {:?}",
                    lowered.evidence
                );
                let (rect, size) = c.atlas(draw.model)?;
                let source_text = fs::read_to_string(c.refs.join(format!(
                    "library-surfaces-01/remaining-shaders/{ps:08X}.hlsl"
                )))?
                .replace("\r\n", "\n");
                let text = adapt(&source_text, values.len() / 16, rect, size)?;
                c.graph.program(
                    &name,
                    &text,
                    &c.refs.join("surface-carriers-01/81529206.hlsl"),
                    false,
                    &c.out,
                )?;
                let mut fixed = vec![];
                let mut symbols = vec![];
                for tex in binding["textures"]
                    .as_array()
                    .context("source emission textures")?
                {
                    let slot = number(&tex["slot"])?;
                    ensure!(
                        (4..=6).contains(&slot),
                        "source effect fixed texture overlaps native atmospheric slots"
                    );
                    symbols.push(texture(c, tex)?);
                    fixed.extend(u32::try_from(slot)?.to_le_bytes());
                    fixed.extend(u32::MAX.to_le_bytes());
                }
                let mut mat = shell.0.clone();
                let texture_class = shell.u64(shell.pointer(0x2D8)? + 8)?;
                ensure!(
                    texture_class == 0x80807211,
                    "native effect texture array class differs"
                );
                append_array(&mut mat, 0x2D0, texture_class, &fixed, 8)?;
                let rows = Payload(mat.clone()).array(0x2D0, 8, None)?;
                let mut patches = vec![
                    json!({"offset":0x48,"symbol":"atlas-vertex-header"}),
                    json!({"offset":0x2C8,"symbol":format!("{name}-shader")}),
                ];
                for (row, symbol) in rows.iter().zip(symbols) {
                    patches.push(json!({"offset":row+4,"symbol":symbol}));
                }
                for at in [0x48, 0x2C8] {
                    put(&mut mat, at, &u32::MAX.to_le_bytes())?;
                }
                append_array(&mut mat, 0x308, 0x808073F3, &sampler_rows, 16)?;
                append_array(&mut mat, 0x2E8, 0x80800009, &lowered.code, 1)?;
                append_array(&mut mat, 0x2F8, 0x80800090, &constants, 16)?;
                append_array(&mut mat, 0x318, 0x80800090, &values, 16)?;
                put(
                    &mut mat,
                    0x338,
                    &u32::try_from(c.objects.len())?.to_le_bytes(),
                )?;
                put(&mut mat, 0x348, &0u32.to_le_bytes())?;
                put(&mut mat, 0x34C, &u32::MAX.to_le_bytes())?;
                c.graph.add(&name, 0x80BFB12B, &mat, None, patches)?;
                evidence.push(json!({"model":draw.model_tag,"material":draw.material,"pixel_shader":format!("{ps:08X}"),"stage":render_stage,"runtime_outputs":lowered.evidence,"source_fixed_texture_bytes_retained":true,"source_sampler_bytes_matched":true,"native_atmosphere_slot":15,"native_fog_slots":[16,17]}));
                created.insert(key.clone(), name);
            }
            for (channel, faces) in &draw.groups {
                c.draws
                    .add(render_stage, &donor, &draw, *channel, faces, &created[&key])?;
            }
        }
        c.draws.layout(render_stage)?;
    }
    c.graph.manifest["emission_adapter"] = json!({"materials":evidence,"source_shader_equations_retained":true,"gameplay_verified":false});
    Ok(())
}
