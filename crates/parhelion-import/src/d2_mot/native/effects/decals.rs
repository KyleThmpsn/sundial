use super::*;

fn numeric_tfx(code: &[u8]) -> Result<Vec<u8>> {
    let instructions = program::parse(code)?;
    let mut result = vec![];
    let mut at = 0;
    while let Some(i) = instructions.get(at) {
        if matches!(i.op, 0x5B | 0x4D) {
            let next = instructions
                .get(at + 1)
                .context("unterminated source binding")?;
            if i.op == 0x5B {
                ensure!(
                    next.op == 0x58 && i.args[0] <= 3 && next.args == [0x21 + i.args[0]],
                    "unexpected source decal sampler binding"
                );
            } else {
                ensure!(
                    i.args == [0x2D, 1] && next.op == 0x56 && next.args == [0x2A],
                    "unverified source framebuffer binding"
                );
            }
            at += 2;
        } else {
            result.push(i.op);
            result.extend_from_slice(i.args);
            at += 1;
        }
    }
    Ok(result)
}

#[expect(
    clippy::cognitive_complexity,
    reason = "Preserve the audited converter while integrating the legacy rendering pipeline."
)]
pub(super) fn build(c: &mut Effect) -> Result<()> {
    ensure!(
        c.draws.records[1].is_empty(),
        "input already contains decal draws"
    );
    let nb = load(&c.refs.join("native-decal-bindings-26/bindings.json"))?;
    let nb = &nb["80BA7713"];
    let native_samplers = nb["samplers"].as_array().context("native decal samplers")?;
    let native_mask = &nb["textures"][0];
    let usage = load(&c.refs.join("native-decal-usage/usage.json"))?;
    let mut carriers = BTreeMap::new();
    for tag in ["80BA7713", "80BF87BD"] {
        let row = usage["matches"]
            .as_array()
            .context("carrier usage")?
            .iter()
            .find(|v| v["material"] == tag)
            .context("missing carrier usage")?;
        ensure!(
            row["stage"] == 1 && row["layout"] == 139,
            "native carrier does not demonstrate plated decal draws"
        );
        carriers.insert(
            tag,
            hex::decode(row["record"].as_str().context("carrier record")?)?,
        );
    }
    let carrier = Payload(fs::read(
        c.refs.join("native-decal-bindings-26/raw/80BA7713.bin"),
    )?);
    let prefix = hex::decode("4c0049214c0149224c0249234c0349243f2c014725")?;
    ensure!(
        array_bytes(&carrier, 0x2E8, 1)?.starts_with(&prefix),
        "native texture/sampler/framebuffer binding differs"
    );
    let mut created = BTreeMap::new();
    let mut evidence = vec![];
    for draw in c.source.draws(1)? {
        let material = c.source.raw(&draw.material)?;
        let ps = material.u32(0x2B0)?;
        let blend = material.u8(48)? & 127;
        ensure!(
            matches!(
                (blend, ps),
                (29, 0x80D01FF2) | (26, 0x80D02013) | (29, 0x80CF9AB9) | (26, 0x80CF5EB7)
            ),
            "source decal program is not inspected"
        );
        for (channel, faces) in &draw.groups {
            ensure!(*channel <= 5, "source dye selector outside native bank");
            let key = (draw.model, draw.material.clone(), *channel);
            if !created.contains_key(&key) {
                let expected = if ps == 0x80CF9AB9 {
                    native_samplers.get(1..3)
                } else if blend == 29 {
                    native_samplers.get(1..)
                } else {
                    Some(native_samplers.as_slice())
                }
                .context("native sampler table")?;
                let binding = c
                    .bindings
                    .get(&draw.material)
                    .context("source decal binding")?;
                ensure!(
                    binding["samplers"]
                        .as_array()
                        .context("source samplers")?
                        .iter()
                        .map(|s| &s["data"])
                        .eq(expected.iter().map(|s| &s["data"])),
                    "source sampler behavior differs from native carriers"
                );
                let name = format!("decal-{}-{}-{channel}", draw.model, draw.material);
                let native_ps = if blend == 29 { "80C1CB1C" } else { "815B95EB" };
                let reference = c.refs.join(format!("surface-carriers-01/{native_ps}.hlsl"));
                let (rect, size) = c.atlas(draw.model)?;
                let text = shader::decal(
                    &fs::read_to_string(&reference)?.replace("\r\n", "\n"),
                    blend,
                    *channel,
                    rect,
                    size,
                    ps,
                )?;
                c.graph.program(&name, &text, &reference, false, &c.out)?;
                let values = vectors(&binding["constants"])?;
                ensure!(
                    values.len() / 16
                        == if matches!(ps, 0x80CF9AB9 | 0x80CF5EB7) {
                            6
                        } else {
                            8
                        },
                    "source decal constant layout differs"
                );
                let code = numeric_tfx(&array_bytes(&material, 0x2D0, 1)?)?;
                let constants = array_bytes(&material, 0x2E0, 16)?;
                let lowered = c.lower(&code, constants.len() / 16, values.len() / 16, 0)?;
                let masks = binding["textures"]
                    .as_array()
                    .context("source mask textures")?;
                if ps == 0x80CF9AB9 {
                    ensure!(
                        masks.is_empty(),
                        "simple decal unexpectedly has fixed textures"
                    );
                } else {
                    ensure!(
                        masks.len() == 1 && masks[0]["slot"] == if blend == 29 { 10 } else { 11 },
                        "source decal mask bindings differ"
                    );
                }
                let mask = masks
                    .first()
                    .unwrap_or(&c.bindings["80D01FF6"]["textures"][0]);
                let mh = Payload(hex::decode(
                    mask["header"].as_str().context("source mask header")?,
                )?);
                let nh = Payload(hex::decode(
                    native_mask["header"]
                        .as_str()
                        .context("native mask header")?,
                )?);
                ensure!(
                    mh.bytes::<8>(0)? == nh.bytes::<8>(0)?
                        && mh.bytes::<8>(34)? == nh.bytes::<8>(14)?
                        && mh.bytes::<2>(44)? == nh.bytes::<2>(22)?,
                    "native decal mask header differs from source"
                );
                for (a, b) in [
                    (
                        c.bindings_root.join(format!(
                            "raw/{}.bin",
                            mask["buffer"].as_str().context("source mask buffer")?
                        )),
                        c.refs.join(format!(
                            "native-decal-bindings-26/raw/{}.bin",
                            native_mask["buffer"]
                                .as_str()
                                .context("native mask buffer")?
                        )),
                    ),
                    (
                        c.bindings_root.join(format!("raw/{:08X}.bin", mh.u32(60)?)),
                        c.refs.join(format!(
                            "native-decal-mask-large/raw/{:08X}.bin",
                            nh.u32(36)?
                        )),
                    ),
                ] {
                    ensure!(
                        fs::read(a)? == fs::read(b)?,
                        "native decal mask pixels differ from source"
                    );
                }
                let mut mat = carrier.0.clone();
                for at in [24, 28] {
                    put(
                        &mut mat,
                        at,
                        &((carrier.u32(at)? & !0x1C000000) | (1 << (26 + channel / 2)))
                            .to_le_bytes(),
                    )?;
                }
                put(&mut mat, 32, &(0x80u32 | blend as u32).to_le_bytes())?;
                for at in [0x48, 0x2C8] {
                    put(&mut mat, at, &u32::MAX.to_le_bytes())?;
                }
                let fixed = carrier.array(0x2D0, 8, None)?;
                ensure!(
                    fixed.len() == 1
                        && carrier.u32(fixed[0])? == 6
                        && carrier.u32(fixed[0] + 4)? == tag(&native_mask["tag"])?,
                    "native mask slot differs"
                );
                let mut code = prefix.clone();
                code.extend(lowered.code);
                append_array(&mut mat, 0x2E8, 0x80800009, &code, 1)?;
                append_array(&mut mat, 0x2F8, 0x80800090, &constants, 16)?;
                append_array(&mut mat, 0x318, 0x80800090, &values, 16)?;
                put(
                    &mut mat,
                    0x338,
                    &u32::try_from(c.objects.len())?.to_le_bytes(),
                )?;
                put(&mut mat, 0x348, &0u32.to_le_bytes())?;
                put(&mut mat, 0x34C, &u32::MAX.to_le_bytes())?;
                c.graph.add(
                    &name,
                    0x80BA7713,
                    &mat,
                    None,
                    vec![
                        json!({"offset":0x48,"symbol":"atlas-vertex-header"}),
                        json!({"offset":0x2C8,"symbol":format!("{name}-shader")}),
                    ],
                )?;
                evidence.push(json!({"source_model":draw.model_tag,"source_material":draw.material,"channel":channel,"native_blend":blend,"source_runtime_outputs":lowered.evidence,"native_framebuffer_slot":5,"mask_slot":6,"source_sampler_bytes_matched":true,"native_mask_byte_identical":true}));
                created.insert(key.clone(), name);
            }
            c.draws.add(
                1,
                &carriers[if blend == 29 { "80BF87BD" } else { "80BA7713" }],
                &draw,
                *channel,
                faces,
                &created[&key],
            )?;
        }
    }
    c.draws.layout(1)?;
    let lod0 = c.draws.records[1]
        .iter()
        .filter(|(r, _)| r[27] == 0)
        .map(|(r, _)| u32::from_le_bytes(r[16..20].try_into().unwrap()) as u64)
        .sum::<u64>();
    c.graph.manifest["appearance"] =
        json!("Rigid source models and decal masks using native rendering scopes");
    c.graph.manifest["decal_adapter"] = json!({"materials":evidence,"lod0_triangles":lod0,"native_layout":139,"transparent_finish_retained":false,"gameplay_verified":false,"source_color_and_mask_controls":true});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_scope_only_removes_validated_bindings() {
        let code = hex::decode("5b0058214d2d01562a42005200").unwrap();
        assert_eq!(hex::encode(numeric_tfx(&code).unwrap()), "42005200");
        for code in ["5b00", "5b045825", "5b005822", "4d2d02562a"] {
            assert!(numeric_tfx(&hex::decode(code).unwrap()).is_err());
        }
    }
}
