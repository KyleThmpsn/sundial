//! Source per-pixel dye-bank reflection, with verified native bindings.
use super::*;
use crate::d2_mot::native::shader::replace_once;

pub(super) struct Dyes {
    pub values: Vec<u8>,
    pub scopes: Vec<(u8, Vec<u8>, Vec<u8>)>,
}
pub(super) fn dyes(prepared: &Path) -> Result<Dyes> {
    let root = prepared.join("modern-dyes");
    let report = load(&root.join("dyes.json"))?;
    let mut banks = vec![];
    let mut scopes = vec![];
    for channel in 4..=6u8 {
        let entries = report
            .as_array()
            .context("source dyes")?
            .iter()
            .filter(|v| v["channel"] == channel)
            .collect::<Vec<_>>();
        ensure!(entries.len() == 1, "missing or ambiguous source dye bank");
        let found = entries[0]["found"]
            .as_array()
            .context("source dye resources")?;
        ensure!(found.len() == 1, "ambiguous source dye scope");
        let entry = &found[0];
        let scope = Payload(fs::read(root.join(format!(
            "raw/{}.bin",
            entry["scope"].as_str().context("source dye scope")?
        )))?);
        let inline = array_bytes(&scope, 0x90, 16)?;
        let values = if inline.is_empty() {
            vectors(&entry["constants"])?
        } else {
            inline
        };
        ensure!(
            values.len() == 21 * 16,
            "source dye bank must contain 21 vectors"
        );
        banks.push(values);
        let code = array_bytes(&scope, 0x60, 1)?;
        if !code.is_empty() {
            scopes.push((channel, code, array_bytes(&scope, 0x70, 16)?));
        }
    }
    let values = banks
        .iter()
        .flat_map(|v| v[..48].iter().copied())
        .chain(banks.iter().flat_map(|v| v[48..].iter().copied()))
        .collect();
    Ok(Dyes { values, scopes })
}

fn adapt(text: &str, rect: [usize; 4], size: [usize; 2]) -> Result<String> {
    let [x, y, w, h] = rect;
    let [aw, ah] = size;
    let mut text = replace_once(text, "float4 cb0[22];", "float4 cb0[85];")?;
    text = replace_once(
        &text,
        "cbuffer cb7 : register(b7)\n{\n  float4 cb7[63];\n}",
        "",
    )?;
    text = text.replace("cb7[", "cb0[22 + ");
    text = replace_once(&text, "float4 cb12[15];", "float4 cb12[13];")?;
    text = replace_once(&text, "cb12[14].xyz", "cb12[7].xyz")?;
    text = replace_once(&text, "float4 v2 : TEXCOORD2", "float3 v2 : TEXCOORD2")?;
    ensure!(
        !text.contains("v2.w"),
        "source reflection requires missing vertex component"
    );
    // Slots 0..2 retain the native plated binding. Fixed textures occupy 3..12.
    // Advanced fog uses the native scope's demonstrated slots 16 and 17.
    for (source, native) in [(26, 12), (21, 17), (20, 16)] {
        text = text.replace(
            &format!("t{source} : register(t{source})"),
            &format!("t{native} : register(t{native})"),
        );
        text = text.replace(&format!("t{source}."), &format!("t{native}."));
    }
    for (texture, sampler) in [(0, 2), (1, 3), (2, 4)] {
        text = replace_once(
            &text,
            &format!("t{texture}.Sample(s{sampler}_s, v3.xy)"),
            &format!("t{texture}.SampleGrad(s{sampler}_s, plate_uv, ddx(v3.xy), ddy(v3.xy))"),
        )?;
    }
    text = replace_once(
        &text,
        "t3.Sample(s5_s, v3.xy)",
        "t3.SampleGrad(s5_s, source_uv, ddx(source_uv), ddy(source_uv))",
    )?;
    // The dither pattern is evaluated in source plate coordinates.
    let marker = "r0.w = cmp(0.000000 != cb0[21].x);";
    ensure!(
        text.matches(marker).count() == 1,
        "source reflection dither contract differs"
    );
    let at = text.find(marker).unwrap();
    text = format!(
        "{}{}",
        &text[..at],
        text[at..].replace("v3.xy", "source_uv")
    );
    replace_once(
        &text,
        "uint4 bitmask, uiDest;",
        &format!(
            "uint4 bitmask, uiDest;\n  float2 source_uv = (v3.xy * float2({aw},{ah}) - float2({x},{y})) / float2({w},{h});\n  float2 plate_uv = (saturate(source_uv) * float2({w},{h}) + float2({x},{y})) / float2({aw},{ah});"
        ),
    )
}

pub(super) fn texture(c: &mut Effect, model: usize) -> Result<String> {
    let plates = c.source.report["models"][model]["texture_plates"]["dyemap"]
        .as_array()
        .context("source dye-map plate")?;
    let plate = crate::d2_mot::plates::source_plate(&c.source.root, &c.source.manifest, plates)?;
    // Fixed shader textures need the resident detail texture's header and
    // package storage metadata. A gear plate donor stays unbound here.
    let mut header = c.graph.read("dye-4-texture-1")?.0;
    put(
        &mut header,
        0,
        &u32::try_from(plate.data.len())?.to_le_bytes(),
    )?;
    put(&mut header, 4, &plate.format.to_le_bytes())?;
    for at in [14, 16] {
        put(&mut header, at, &u16::try_from(plate.side)?.to_le_bytes())?;
    }
    header[23] = u8::try_from(plate.mips)?;
    crate::d2_mot::texture::resident(&mut header, plate.data.len())?;
    let name = format!("dyemap-reflection-{model}-texture");
    let data_name = format!("{name}-data");
    if c.graph.node(&name).is_ok() {
        ensure!(
            c.graph.read(&name)?.0 == header && c.graph.read(&data_name)?.0 == plate.data,
            "existing source dye map differs"
        );
        return Ok(name);
    }
    let ht = c.graph.node("dye-4-texture-1")?["template"]
        .as_u64()
        .context("texture template")?;
    let dt = c.graph.node("dye-4-texture-1-data")?["template"]
        .as_u64()
        .context("texture buffer template")?;
    c.graph.add(&name, ht, &header, Some(&data_name), vec![])?;
    c.graph
        .add(&data_name, dt, &plate.data, Some(&name), vec![])?;
    Ok(name)
}

pub(super) fn build(c: &mut Effect, prepared: &Path) -> Result<()> {
    let binding = c.bindings["80CF5F87"].clone();
    let native = load(&c.refs.join("native-transparent-bindings/bindings.json"))?;
    let advanced = load(
        &c.refs
            .join("native-transparent-advanced-bindings/bindings.json"),
    )?;
    let ns = &native["80EC271F"]["samplers"];
    let fog = &advanced["80C1DFB0"]["samplers"][6];
    let samplers = binding["samplers"]
        .as_array()
        .context("source dye-map reflection samplers")?;
    ensure!(
        samplers.len() == 8,
        "source dye-map reflection resource count differs"
    );
    let mut sampler_rows = vec![];
    for (index, n) in [0, 2, 2, 2, 5, 5].into_iter().enumerate() {
        ensure!(
            samplers[index]["data"]
                .as_str()
                .context("source sampler bytes")?
                == ns[n]["data"].as_str().context("native sampler bytes")?,
            "dye-map reflection sampler differs"
        );
        sampler_rows.extend(tag(&ns[n]["tag"])?.to_le_bytes());
        sampler_rows.extend([0; 12]);
    }
    ensure!(
        samplers[6]["data"].as_str().context("source fog sampler")?
            == fog["data"].as_str().context("native fog sampler")?,
        "dye-map fog sampler differs"
    );
    sampler_rows.extend(tag(&fog["tag"])?.to_le_bytes());
    sampler_rows.extend([0; 12]);
    let cube = &native["80EC271F"]["textures"][0];
    // The source cube must be the exact family validated by the native carrier builder.
    let inspected = load(&c.refs.join("source-transparent-bindings/bindings.json"))?;
    ensure!(
        binding["textures"] == inspected["80CF6F4A"]["textures"]
            && samplers[7]["tag"] == binding["textures"][0]["tag"],
        "dye-map reflection cube differs from inspected source"
    );
    sampler_rows.extend(tag(&cube["tag"])?.to_le_bytes());
    sampler_rows.extend([0; 12]);
    let shell = reflection::carrier(c)?;
    let source_dyes = dyes(prepared)?;
    let source_text =
        fs::read_to_string(c.refs.join("surface-carriers-01/80CF5F86.hlsl"))?.replace("\r\n", "\n");
    let native_model = Payload(fs::read(prepared.join("native/raw/80EC2722.bin"))?);
    let rows = native_model.array(352, 32, None)?;
    let range = native_model.u16(382)? as usize..native_model.u16(384)? as usize;
    let row = rows
        .get(range)
        .context("native reflection draw range")?
        .iter()
        .find(|r| native_model.u32(**r).is_ok_and(|v| v == 0x80EC271F))
        .context("native reflection draw")?;
    let donor = native_model.0[*row..*row + 32].to_vec();
    let mut created = BTreeMap::new();
    let mut evidence = vec![];
    for draw in c.source.draws(7)? {
        if draw.material != "80CF5F87" {
            continue;
        }
        let material = c.source.raw(&draw.material)?;
        ensure!(
            material.u32(0x2B0)? == 0x80CF5F86 && material.u8(48)? & 127 == 8,
            "source dye-map reflection contract differs"
        );
        if let std::collections::btree_map::Entry::Vacant(entry) = created.entry(draw.model) {
            let name = format!("dyemap-reflection-{}", draw.model);
            let (rect, size) = c.atlas(draw.model)?;
            let text = adapt(&source_text, rect, size)?;
            c.graph.program(
                &name,
                &text,
                &c.refs.join("surface-carriers-01/81529206.hlsl"),
                false,
                &c.out,
            )?;
            let texture = texture(c, draw.model)?;
            let mut values = array_bytes(&material, 0x300, 16)?;
            ensure!(
                values.len() == 22 * 16,
                "source reflection constant count differs"
            );
            values.extend(&source_dyes.values);
            let code = array_bytes(&material, 0x2D0, 1)?;
            let source_prefix = (0..7)
                .flat_map(|i| [0x5B, i, 0x58, 0x21 + i])
                .collect::<Vec<_>>();
            ensure!(
                code.starts_with(&source_prefix),
                "source dye-map reflection binding prefix differs"
            );
            let mut constants = array_bytes(&material, 0x2E0, 16)?;
            let mut b = Bindings {
                objects: c.objects.clone(),
                globals: c.globals.clone(),
                constant_count: constants.len() / 16,
                output_count: 85,
                sampler_count: 8,
                textures: BTreeMap::from([(7, 7)]),
                ..Default::default()
            };
            let mut lowered = program::lower(&code[source_prefix.len()..], &b)?;
            ensure!(
                lowered.samplers.is_empty(),
                "unexpected numeric reflection binding"
            );
            for (channel, code, extra) in &source_dyes.scopes {
                let bank = channel - 4;
                let map = (0..21u8)
                    .map(|i| {
                        (
                            i,
                            22 + if i < 3 {
                                bank * 3 + i
                            } else {
                                9 + bank * 18 + i - 3
                            },
                        )
                    })
                    .collect();
                let relocated = program::relocate(code, constants.len() / 16, &map)?;
                constants.extend(extra);
                b.constant_count = constants.len() / 16;
                let result = program::lower(&relocated, &b)?;
                ensure!(
                    result.samplers.is_empty(),
                    "source dye scope binds resources"
                );
                lowered.code.extend(result.code);
                lowered
                    .evidence
                    .extend(result.evidence.into_iter().map(|mut row| {
                        row["dye_channel"] = json!(channel);
                        row
                    }));
            }
            ensure!(
                lowered.evidence.iter().all(|e| e["translated"] == true),
                "unresolved dye-map reflection controls: {:?}",
                lowered.evidence
            );
            let mut mat = shell.0.clone();
            // Native plated, view and advanced fog scopes stay enabled. Dye values
            // and six detail textures are supplied explicitly for per-pixel selection.
            for at in [24, 28] {
                put(&mut mat, at, &(shell.u32(at)? & !0x1C000000).to_le_bytes())?;
            }
            let mut fixed = vec![];
            let mut symbolic = vec![(3, texture)];
            for channel in 4..=6 {
                for detail in 0..2 {
                    let symbol = format!("dye-{channel}-texture-{detail}");
                    c.graph.node(&symbol)?;
                    symbolic.push((4 + (channel - 4) * 2 + detail, symbol));
                }
            }
            for (slot, _) in &symbolic {
                fixed.extend((*slot as u32).to_le_bytes());
                fixed.extend(u32::MAX.to_le_bytes());
            }
            fixed.extend(12u32.to_le_bytes());
            fixed.extend(tag(&cube["tag"])?.to_le_bytes());
            let texture_class = shell.u64(shell.pointer(0x2D8)? + 8)?;
            ensure!(
                texture_class == 0x80807211,
                "native fixed-texture array class differs"
            );
            append_array(&mut mat, 0x2D0, texture_class, &fixed, 8)?;
            let rows = Payload(mat.clone()).array(0x2D0, 8, None)?;
            let mut patches = vec![
                json!({"offset":0x48,"symbol":"atlas-vertex-header"}),
                json!({"offset":0x2C8,"symbol":format!("{name}-shader")}),
            ];
            for (row, (_, symbol)) in rows.iter().zip(&symbolic) {
                patches.push(json!({"offset":row+4,"symbol":symbol}));
            }
            append_array(&mut mat, 0x308, 0x808073F3, &sampler_rows, 16)?;
            let mut code = (0..7)
                .flat_map(|i| [0x4C, i, 0x49, 0x21 + i])
                .collect::<Vec<_>>();
            code.extend(lowered.code);
            append_array(&mut mat, 0x2E8, 0x80800009, &code, 1)?;
            append_array(&mut mat, 0x2F8, 0x80800090, &constants, 16)?;
            append_array(&mut mat, 0x318, 0x80800090, &values, 16)?;
            c.graph.add(&name, 0x80EC271F, &mat, None, patches)?;
            evidence.push(json!({"model":draw.model_tag,"source_material":draw.material,"source_pixel_shader":"80CF5F86","source_dye_vectors":63,"runtime_outputs":lowered.evidence,"source_per_pixel_dye_selection":true,"source_sampler_bytes_matched":true,"native_cube_byte_identical":true,"native_fog_slots":[16,17],"source_camera_vector":14,"native_camera_vector":7}));
            entry.insert(name);
        }
        for (channel, faces) in &draw.groups {
            c.draws
                .add(7, &donor, &draw, *channel, faces, &created[&draw.model])?;
        }
    }
    c.draws.layout(7)?;
    c.graph.manifest["dyemap_reflection_adapter"] = json!({"materials":evidence,"source_shader_equations_retained":true,"gameplay_verified":false});
    Ok(())
}
