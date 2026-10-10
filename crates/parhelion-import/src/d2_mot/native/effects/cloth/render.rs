//! Keep simulation output and skinned fallback on the source float declaration.
use super::*;
use sha2::{Digest, Sha256};

fn compute(c: &Effect, model: &Payload, mesh: usize) -> Result<()> {
    let parts = model.array(mesh + 32, 36, Some(0x80806ECB))?;
    let rows = parts
        .get(
            usize::from(model.u16(mesh + 48 + 23 * 2)?)
                ..usize::from(model.u16(mesh + 48 + 24 * 2)?),
        )
        .context("Cloth compute draw range")?;
    let surface = parts
        .get(usize::from(model.u16(mesh + 48)?)..usize::from(model.u16(mesh + 50)?))
        .context("Cloth surface draw range")?;
    let manifest = load(&c.bindings_root.join("source-manifest.json"))?;
    let expected_code = hex::decode("4d560057a04f560159a04a560452004a56055201")?;
    for &at in rows {
        ensure!(
            surface.iter().any(
                |s| model.bytes::<12>(s + 6).ok() == model.bytes::<12>(at + 6).ok()
                    && model.bytes::<2>(s + 29).ok() == model.bytes::<2>(at + 29).ok()
            ),
            "Cloth compute draw has no corresponding render geometry"
        );
        let material = material(c, model.u32(at)?)?;
        ensure!(
            material.u32(8)? == 6 && array_bytes(&material, 0x360, 1)? == expected_code,
            "Cloth compute program differs from float upload"
        );
        for base in [0x70, 0x100, 0x190, 0x220, 0x2B0] {
            ensure!(
                material.u32(base)? == u32::MAX && material.u64(base + 0x20)? == 0,
                "Cloth copy material contains another shader stage"
            );
        }
        for (offset, stride) in [(0x348, 24), (0x370, 16), (0x380, 16)] {
            ensure!(
                array_bytes(&material, offset, stride)?.is_empty(),
                "Cloth copy has unexpected fixed resources"
            );
        }
        ensure!(
            array_bytes(&material, 0x390, 16)? == [0; 32] && material.u32(0x3B4)? == u32::MAX,
            "Cloth copy constants differ"
        );
        let shader = material.u32(0x340)?;
        let data = manifest["tags"][format!("{shader:08X}")]["reference"]
            .as_u64()
            .context("Cloth copy shader provenance")?;
        let bytes = fs::read(c.bindings_root.join(format!("raw/{data:08X}.bin")))?;
        // This inspected SM5 kernel copies one float per thread. The native
        // cloth buffer upload supplies that copy without a separate draw pass.
        ensure!(
            hex::encode(Sha256::digest(bytes))
                == "c91adcff14bedbf04d550e53cb37e0ea3e92f8689a44bbe4a479d562398a8372",
            "Cloth compute shader is not the validated float copy kernel"
        );
    }
    Ok(())
}

fn material(c: &Effect, tag: u32) -> Result<Payload> {
    Ok(Payload(fs::read(
        c.bindings_root.join(format!("raw/{tag:08X}.bin")),
    )?))
}

fn compatible(a: &Payload, b: &Payload) -> Result<()> {
    ensure!(
        a.u32(8)? == b.u32(8)?
            && a.bytes::<4>(48)? == b.bytes::<4>(48)?
            && a.u32(0x2B0)? == b.u32(0x2B0)?,
        "Cloth simulation changes the source pixel stage or render state"
    );
    for base in [0x70, 0x2B0] {
        for (at, stride) in [(8, 24), (0x20, 1), (0x30, 16), (0x40, 16), (0x50, 16)] {
            ensure!(
                array_bytes(a, base + at, stride)? == array_bytes(b, base + at, stride)?,
                "Cloth simulation changes a material program or resource table"
            );
        }
        ensure!(
            a.bytes::<32>(base + 0x60)? == b.bytes::<32>(base + 0x60)?,
            "Cloth simulation changes material binding metadata"
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn converted_material(
    c: &mut Effect,
    name: &str,
    model_index: usize,
    stage: usize,
    source_model: &Payload,
    source: u32,
    fallback: u32,
    simulated: bool,
) -> Result<String> {
    let symbol = format!("{name}-stage-{stage}-{source:08X}");
    if c.graph.node(&symbol).is_ok() {
        return Ok(symbol);
    }
    let source_material = material(c, source)?;
    let fallback_material = material(c, fallback)?;
    compatible(&source_material, &fallback_material)?;
    let previous = format!("source-stage-{stage}-{model_index}-{fallback:08X}");
    let mut bytes = c.graph.read(&previous)?.0;
    let mut refs = patches(&c.graph, &previous)?;
    let vs = source_material.u32(0x70)?;
    let original = fs::read_to_string(
        c.refs
            .join(format!("library-surfaces-01/source-shaders/{vs:08X}.hlsl")),
    )?
    .replace("\r\n", "\n");
    let bones = number(&c.graph.manifest["rig_mapping"]["native_bone_count"])?;
    let text = vertex::cloth(source_model, &original, bones, simulated)?;
    c.graph.program(
        &symbol,
        &text,
        &c.refs.join("shaders/vertex.hlsl"),
        true,
        &c.out,
    )?;
    put(&mut bytes, 0x48, &u32::MAX.to_le_bytes())?;
    patch(&mut refs, 0x48, &format!("{symbol}-shader"));
    let template = c.graph.node(&previous)?["template"]
        .as_u64()
        .context("Cloth material template")?;
    c.graph.add(&symbol, template, &bytes, None, refs)?;
    Ok(symbol)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    c: &mut Effect,
    reader: &mut Reader,
    name: &str,
    index: usize,
    entry: &Value,
    source: &Payload,
    mesh: usize,
    positions: &[u8],
    bones: &[u16],
    model_tag: u32,
    template: &Payload,
) -> Result<()> {
    compute(c, source, mesh)?;
    let native_meshes = template.array(16, 136, Some(0x80807378))?;
    ensure!(
        native_meshes.len() == 1,
        "Native cloth model template has multiple meshes"
    );
    let native_mesh = native_meshes[0];
    let count = positions.len() / 48;
    let uv = c.source.buffer(source.u32(mesh + 4)?)?.0;
    let mut skin = c.source.buffer(source.u32(mesh + 8)?)?.0;
    ensure!(
        uv.len() == count * 4 && skin.len() == count * 8,
        "Cloth streams have different vertex counts"
    );
    let mut palette = 0u32;
    for row in skin.chunks_exact_mut(8) {
        ensure!(
            row[..4].iter().map(|w| u16::from(*w)).sum::<u16>() == 255,
            "Cloth skin weights do not sum to one"
        );
        let valid = (0..4)
            .find(|i| row[*i] != 0)
            .context("Cloth vertex has no bone weight")?;
        let first = u8::try_from(
            *bones
                .get(usize::from(row[valid + 4]))
                .context("Cloth bone outside palette")?,
        )?;
        for i in 0..4 {
            row[i + 4] = if row[i] == 0 {
                first
            } else {
                u8::try_from(
                    *bones
                        .get(usize::from(row[i + 4]))
                        .context("Cloth bone outside palette")?,
                )?
            };
            palette = palette.max(u32::from(row[i + 4]) + 1);
        }
    }
    let index_tag = source.u32(mesh + 16)?;
    let source_index_header = c.source.raw(&format!("{index_tag:08X}"))?;
    let indices = c.source.buffer(index_tag)?;
    let (decoded, restart) = geometry::indices(&source_index_header, &indices)?;
    ensure!(
        restart == u16::MAX as u32 && decoded.iter().all(|i| (*i as usize) < count),
        "Cloth triangle indexes exceed their model or use a different width"
    );
    let mut model = template.0.clone();
    put(&mut model, 0x40, &palette.to_le_bytes())?;
    put(&mut model, 0x50, &source.bytes::<48>(0x50)?)?;
    let mut refs = Vec::new();
    for (offset, suffix, stride, data) in [
        (0, "positions", 48, positions),
        (4, "uv", 4, uv.as_slice()),
        (8, "skin", 8, skin.as_slice()),
        (16, "indices", 0, indices.0.as_slice()),
    ] {
        let header_tag = template.u32(native_mesh + offset)?;
        let mut header = reader.tag(header_tag, None)?.0.clone();
        let data_tag = reader.reference(header_tag)?;
        let hs = format!("{name}-{suffix}-header");
        let ds = format!("{name}-{suffix}-data");
        if stride == 0 {
            ensure!(
                header.len() == 24
                    && source_index_header.u32(0)? == u32::from_le_bytes(header[..4].try_into()?),
                "Cloth index header differs from native"
            );
            put(&mut header, 8, &u32::try_from(data.len())?.to_le_bytes())?;
        } else {
            ensure!(
                header.len() == 12 && Payload(header.clone()).u16(4)? == stride,
                "Native cloth vertex template differs"
            );
            put(&mut header, 0, &u32::try_from(data.len())?.to_le_bytes())?;
        }
        c.graph
            .add(&hs, u64::from(header_tag), &header, Some(&ds), vec![])?;
        c.graph
            .add(&ds, u64::from(data_tag), data, Some(&hs), vec![])?;
        put(&mut model, native_mesh + offset, &u32::MAX.to_le_bytes())?;
        refs.push(json!({"offset":native_mesh+offset,"symbol":hs}));
    }
    let parts = source.array(mesh + 32, 36, Some(0x80806ECB))?;
    let mut records = Vec::new();
    let mut materials = Vec::new();
    for stage in 0..23 {
        let rows = parts
            .get(
                usize::from(source.u16(mesh + 48 + stage * 2)?)
                    ..usize::from(source.u16(mesh + 50 + stage * 2)?),
            )
            .context("Cloth stage exceeds source parts")?;
        ensure!(
            rows.is_empty() || matches!(stage, 0 | 3),
            "Cloth render stage {stage} needs a separate physical stream contract"
        );
        put(
            &mut model,
            native_mesh + 40 + stage * 2,
            &u16::try_from(materials.len())?.to_le_bytes(),
        )?;
        put(
            &mut model,
            native_mesh + 88 + stage * 2,
            &(if rows.is_empty() { -1i16 } else { 18 }).to_le_bytes(),
        )?;
        for &at in rows {
            ensure!(
                source.u16(at + 6)? == 3 && source.u8(at + 31)? == 1,
                "Cloth requires independent triangle-list draw groups"
            );
            let start = source.u32(at + 8)? as usize;
            let length = source.u32(at + 12)? as usize;
            ensure!(
                length.is_multiple_of(3)
                    && start
                        .checked_add(length)
                        .is_some_and(|end| end <= decoded.len()),
                "Cloth draw exceeds its triangle index buffer"
            );
            let simulated = source.u8(at + 29)? == 3
                && source.u8(at + 30)? == 127
                && source.u32(at + 24)? & 8 != 0;
            let fallback = rows
                .iter()
                .copied()
                .filter(|other| {
                    source.u8(other + 29).ok() == Some(0)
                        && source.u8(other + 30).ok() == Some(0)
                        && source.bytes::<12>(other + 6).ok() == source.bytes::<12>(at + 6).ok()
                })
                .collect::<Vec<_>>();
            ensure!(
                fallback.len() == 1,
                "Cloth draw has no unique source skinned material"
            );
            let symbol = converted_material(
                c,
                name,
                index,
                stage,
                source,
                source.u32(at)?,
                source.u32(fallback[0])?,
                simulated,
            )?;
            let mut record = crate::d2_mot::mapping::draw_record(&source.0[at..at + 36])?;
            put(
                &mut record,
                22,
                &u16::try_from(materials.len())?.to_le_bytes(),
            )?;
            records.extend(record);
            materials.push(symbol);
        }
    }
    put(
        &mut model,
        native_mesh + 40 + 23 * 2,
        &u16::try_from(materials.len())?.to_le_bytes(),
    )?;
    append_array(&mut model, native_mesh + 24, 0x8080737E, &records, 32)?;
    let parts = Payload(model.clone()).array(native_mesh + 24, 32, Some(0x8080737E))?;
    for (at, symbol) in parts.into_iter().zip(materials) {
        refs.push(json!({"offset":at,"symbol":symbol}));
    }
    crate::d2_mot::audit::draws::declare_model_draw_indices(&mut model)?;
    c.graph.add(
        &format!("{name}-model"),
        u64::from(model_tag),
        &model,
        None,
        refs,
    )?;
    c.graph.node_mut(&format!("{name}-model"))?["model"] = json!(true);
    c.graph.node_mut(&format!("{name}-model"))?["source_model"] = entry["model"].clone();
    Ok(())
}
