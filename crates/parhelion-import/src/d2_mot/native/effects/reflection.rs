use super::*;
use std::collections::BTreeSet;

pub(super) fn carrier(c: &Effect) -> Result<Payload> {
    let source = load(&c.refs.join("source-transparent-bindings/bindings.json"))?;
    let native = load(&c.refs.join("native-transparent-bindings/bindings.json"))?;
    let advanced = load(
        &c.refs
            .join("native-transparent-advanced-bindings/bindings.json"),
    )?;
    let sb = &source["80CF6F4A"];
    let nb = &native["80EC271F"];
    let ab = &advanced["80C1DFB0"];
    for (src, dst) in [(0, 0), (0, 1), (1, 2), (2, 3), (3, 4), (4, 5)] {
        let s = sb["samplers"][src]["data"]
            .as_str()
            .context("source reflection sampler")?;
        let n = nb["samplers"][dst]["data"]
            .as_str()
            .context("native reflection sampler")?;
        ensure!(s == n, "reflection samplers differ");
    }
    ensure!(
        sb["samplers"][5]["data"]
            .as_str()
            .context("source fog sampler")?
            == ab["samplers"][6]["data"]
                .as_str()
                .context("native fog sampler")?,
        "fog sampler differs"
    );
    let st = &sb["textures"][0];
    let nt = &nb["textures"][0];
    let sh = Payload(hex::decode(
        st["header"].as_str().context("source cubemap header")?,
    )?);
    let nh = Payload(hex::decode(
        nt["header"].as_str().context("native cubemap header")?,
    )?);
    ensure!(
        sh.bytes::<8>(0)? == nh.bytes::<8>(0)?
            && sh.bytes::<8>(34)? == nh.bytes::<8>(14)?
            && sh.bytes::<2>(44)? == nh.bytes::<2>(22)?,
        "cubemap headers differ"
    );
    for (a, b) in [
        (
            c.refs.join(format!(
                "source-transparent-bindings/raw/{}.bin",
                st["buffer"].as_str().context("source cube buffer")?
            )),
            c.refs.join(format!(
                "native-transparent-bindings/raw/{}.bin",
                nt["buffer"].as_str().context("native cube buffer")?
            )),
        ),
        (
            c.refs.join(format!(
                "source-transparent-bindings/raw/{:08X}.bin",
                sh.u32(60)?
            )),
            c.refs
                .join(format!("native-cube-large/raw/{:08X}.bin", nh.u32(36)?)),
        ),
    ] {
        ensure!(
            fs::read(a)? == fs::read(b)?,
            "native reflection cubemap pixels differ from source"
        );
    }
    ensure!(
        st["slot"] == 26 && nt["slot"] == 5 && sb["samplers"][6]["tag"] == st["tag"],
        "cubemap resource identity differs"
    );
    let original = Payload(fs::read(
        c.refs.join("native-transparent-bindings/raw/80EC271F.bin"),
    )?);
    let mut mat = original.0.clone();
    for at in [24, 28] {
        put(&mut mat, at, &(original.u32(at)? | 0x4000).to_le_bytes())?;
    }
    for at in [0x48, 0x2C8] {
        put(&mut mat, at, &u32::MAX.to_le_bytes())?;
    }
    let mut samplers = array_bytes(&original, 0x308, 16)?;
    ensure!(
        samplers.len() == 6 * 16,
        "native reflection sampler resources differ"
    );
    for value in [&ab["samplers"][6]["tag"], &nt["tag"]] {
        samplers.extend(tag(value)?.to_le_bytes());
        samplers.extend([0; 12]);
    }
    append_array(&mut mat, 0x308, 0x808073F3, &samplers, 16)?;
    let source = Payload(fs::read(
        c.refs.join("source-transparent-bindings/raw/80CF6F4A.bin"),
    )?);
    let code = array_bytes(&source, 0x2D0, 1)?;
    let prefix = (0..6)
        .flat_map(|i| [0x5B, i, 0x58, 0x21 + i])
        .collect::<Vec<_>>();
    ensure!(
        code.starts_with(&prefix),
        "source reflection binding prefix differs"
    );
    let constants = array_bytes(&source, 0x2E0, 16)?;
    let lowered = c.lower(&code[prefix.len()..], constants.len() / 16, 22, 8)?;
    let mut code = (0..7)
        .flat_map(|i| [0x4C, i, 0x49, 0x21 + i])
        .collect::<Vec<_>>();
    code.extend(lowered.code);
    append_array(&mut mat, 0x2E8, 0x80800009, &code, 1)?;
    append_array(&mut mat, 0x2F8, 0x80800090, &constants, 16)?;
    append_array(&mut mat, 0x318, 0x80800090, &vectors(&sb["constants"])?, 16)?;
    put(
        &mut mat,
        0x338,
        &u32::try_from(c.objects.len())?.to_le_bytes(),
    )?;
    put(&mut mat, 0x348, &0u32.to_le_bytes())?;
    put(&mut mat, 0x34C, &u32::MAX.to_le_bytes())?;
    Ok(Payload(mat))
}

pub(super) fn build(c: &mut Effect, native: &Path) -> Result<()> {
    ensure!(
        c.draws.records[7].is_empty(),
        "input graph already contains transparent draws"
    );
    let inspected = load(&c.refs.join("source-transparent-bindings/bindings.json"))?;
    ensure!(
        c.bindings["80CF6F4A"] == inspected["80CF6F4A"],
        "reflection family binding differs from inspected reference"
    );
    let shell = carrier(c)?;
    let native_model = Payload(fs::read(native.join("raw/80EC2722.bin"))?);
    let rows = native_model.array(328 + 24, 32, None)?;
    let range =
        native_model.u16(328 + 40 + 14)? as usize..native_model.u16(328 + 40 + 16)? as usize;
    let row = rows
        .get(range)
        .context("native reflection draw range")?
        .iter()
        .find(|r| native_model.u32(**r).is_ok_and(|tag| tag == 0x80EC271F))
        .context("native reflection carrier draw")?;
    let donor = &native_model.0[*row..*row + 32];
    let reference = c.refs.join("surface-carriers-01/81529206.hlsl");
    let template = fs::read_to_string(&reference)?.replace("\r\n", "\n");
    let mut created = BTreeMap::new();
    let mut evidence = vec![];
    let mut unsupported = BTreeSet::new();
    for draw in c.source.draws(7)? {
        if draw.material != "80CF6F4A" {
            unsupported.insert(draw.material);
            continue;
        }
        let material = c.source.raw(&draw.material)?;
        ensure!(
            material.u32(0x2B0)? == 0x80CF6F49,
            "reflection shader family changed"
        );
        for (channel, faces) in &draw.groups {
            ensure!(*channel <= 5, "source dye selector outside native bank");
            let key = (draw.model, *channel);
            if let std::collections::btree_map::Entry::Vacant(entry) = created.entry(key) {
                let name = format!("family-reflection-{}-{channel}", draw.model);
                let (rect, size) = c.atlas(draw.model)?;
                let text = shader::reflection(&template, *channel, rect, size)?;
                c.graph.program(&name, &text, &reference, false, &c.out)?;
                let code = array_bytes(&material, 0x2D0, 1)?;
                let prefix = (0..6)
                    .flat_map(|i| [0x5B, i, 0x58, 0x21 + i])
                    .collect::<Vec<_>>();
                ensure!(
                    code.starts_with(&prefix),
                    "reflection source binding prefix differs"
                );
                let constants = array_bytes(&material, 0x2E0, 16)?;
                let lowered = c.lower(&code[prefix.len()..], constants.len() / 16, 22, 8)?;
                let mut mat = shell.0.clone();
                for at in [24, 28] {
                    put(
                        &mut mat,
                        at,
                        &((shell.u32(at)? & !0x1C000000) | (1 << (26 + channel / 2))).to_le_bytes(),
                    )?;
                }
                let mut code = (0..7)
                    .flat_map(|i| [0x4C, i, 0x49, 0x21 + i])
                    .collect::<Vec<_>>();
                code.extend(lowered.code);
                append_array(&mut mat, 0x2E8, 0x80800009, &code, 1)?;
                append_array(&mut mat, 0x2F8, 0x80800090, &constants, 16)?;
                put(
                    &mut mat,
                    0x338,
                    &u32::try_from(c.objects.len())?.to_le_bytes(),
                )?;
                c.graph.add(
                    &name,
                    0x80EC271F,
                    &mat,
                    None,
                    vec![
                        json!({"offset":0x48,"symbol":"atlas-vertex-header"}),
                        json!({"offset":0x2C8,"symbol":format!("{name}-shader")}),
                    ],
                )?;
                evidence.push(json!({"model":draw.model_tag,"channel":channel,"runtime_outputs":lowered.evidence}));
                entry.insert(name);
            }
            c.draws
                .add(7, donor, &draw, *channel, faces, &created[&key])?;
        }
    }
    c.draws.layout(7)?;
    c.graph.manifest["reflection_adapter"] = json!({"materials":evidence,"unsupported_materials":unsupported,"cubemap_byte_identical":true,"source_sampler_bytes_matched":true,"native_dye_shading_approximation":true,"gameplay_verified":false});
    Ok(())
}
