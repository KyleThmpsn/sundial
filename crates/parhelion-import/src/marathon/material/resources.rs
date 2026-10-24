//! Native envelopes for translated material resources.
use super::*;

/// Immutable stage buffers take precedence over the inline initial-output array.
pub(super) fn values(
    source: &mut Reader,
    material: &Payload,
    stage: usize,
) -> Result<Vec<[u8; 16]>> {
    let tag = material.u32(stage + 0x6c)?;
    if matches!(tag, 0 | u32::MAX | 0x811c9dc5) {
        return vectors(material, stage + 0x50);
    }
    let buffer = shader::buffer::Buffer::from_package(source, tag)?;
    Ok(buffer
        .data()
        .chunks_exact(16)
        .map(|row| row.try_into().unwrap())
        .collect())
}

/// Follow named render globals in both package sets before adapting screen-space color.
pub(super) fn view_scalars(source: &mut Reader, native: &mut Reader) -> Result<()> {
    // Both view producers combine the same scalar pair with the same reciprocal
    // coefficient chain. Their vector output row and opcode dialect differ.
    scope_binding(
        source,
        true,
        b"view",
        &hex::decode("4a02004a02010c42004a02004a02010c040d5310")?,
    )?;
    scope_binding(
        native,
        false,
        b"view",
        &hex::decode("3c02003c02010c34003c02003c02010c040d430c")?,
    )
}

/// Follow named render globals in both package sets before adapting screen-space color.
pub(super) fn scene_depth(source: &mut Reader, native: &mut Reader) -> Result<()> {
    for (reader, modern, texture, coefficients) in [
        (
            source,
            true,
            [0x4d, 3, 19, 0x59, 0x2a],
            [0x4b, 3, 0, 0x53, 0],
        ),
        (
            native,
            false,
            [0x3f, 3, 7, 0x47, 0x2a],
            [0x3d, 3, 0, 0x43, 0],
        ),
    ] {
        scope_binding(reader, modern, b"transparent", &texture)?;
        scope_binding(reader, modern, b"transparent", &coefficients)?;
    }
    Ok(())
}

pub(super) fn scene_color(source: &mut Reader, native: &mut Reader) -> Result<()> {
    for (reader, modern, expected) in [
        (source, true, [0x4d, 0x28, 12, 0x59, 0x37]),
        (native, false, [0x3f, 0x27, 7, 0x47, 0x32]),
    ] {
        scope_binding(reader, modern, b"transparent", &expected)?;
    }
    Ok(())
}

pub(super) fn scope_binding(
    reader: &mut Reader,
    modern: bool,
    scope_name: &[u8],
    expected: &[u8],
) -> Result<()> {
    let name = if modern {
        "render_globals"
    } else {
        "client_bootstrap_patchable"
    };
    let tags: BTreeSet<_> = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .filter(|tag| tag.name == name)
        .map(|tag| tag.hash.0)
        .collect();
    ensure!(!tags.is_empty(), "Missing renderer globals for scene color");
    for tag in tags {
        let bootstrap = reader.tag(tag, None)?;
        let globals = reader.tag(bootstrap.u32(if modern { 0x48 } else { 0x4c })?, None)?;
        let mut programs = Vec::new();
        for row in globals.array(0x10, 16, None)? {
            let start = globals.pointer(row)?;
            let tail = &globals.0[start..];
            let end = tail
                .iter()
                .position(|byte| *byte == 0)
                .context("Renderer scope name is unterminated")?;
            ensure!(end <= 128, "Renderer scope name exceeds limit");
            if &tail[..end] != scope_name {
                continue;
            }
            let scope = reader.tag(globals.u32(row + 12)?, None)?;
            ensure!(
                scope.u64(0)? as usize == scope.0.len(),
                "Renderer scope extent differs"
            );
            let program = scope
                .array(if modern { 0x60 } else { 0x58 }, 1, Some(0x80800009))?
                .into_iter()
                .map(|at| scope.u8(at))
                .collect::<Result<Vec<_>>>()?;
            programs.push(program);
        }
        ensure!(
            programs.len() == 1
                && programs[0]
                    .windows(expected.len())
                    .filter(|row| *row == expected)
                    .count()
                    == 1,
            "Renderer scope binding differs"
        );
    }
    Ok(())
}

pub(super) struct Bindings {
    pub textures: BTreeMap<u32, String>,
    pub metadata: BTreeMap<(u8, u8), [u8; 16]>,
    pub sampler_indices: BTreeSet<u8>,
    table: Vec<String>,
}

pub(super) fn vector_bytes(values: [f32; 4]) -> [u8; 16] {
    let bytes = values.map(f32::to_le_bytes);
    std::array::from_fn(|i| bytes[i / 4][i % 4])
}

/// A material's resource inputs: each metadata vector by its resource and row, and the resources
/// that are samplers.
pub(super) type Inputs = (BTreeMap<(u8, u8), [u8; 16]>, BTreeSet<u8>);

pub(super) fn inputs(source: &mut Reader, material: &Payload) -> Result<Inputs> {
    let mut metadata = BTreeMap::new();
    let mut samplers = BTreeSet::new();
    for (index, row) in material.array(0x2b8, 16, None)?.into_iter().enumerate() {
        let index = u8::try_from(index)?;
        let tag = source.ref64(material, row)?;
        let header = source.tag(tag, None)?;
        if header.0.len() == 8 {
            samplers.insert(index);
        } else {
            ensure!(
                header.0.len() == 64 && header.u16(32)? == 0xcafe,
                "Source resource table contains an unsupported object"
            );
            metadata.insert((0x68, index), header.bytes::<16>(16)?);
            metadata.insert(
                (0x67, index),
                vector_bytes([
                    f32::from(header.u16(34)?),
                    f32::from(header.u16(36)?),
                    f32::from(header.u16(38)?),
                    f32::from(header.u16(40)?),
                ]),
            );
            metadata.insert(
                (0x69, index),
                vector_bytes([
                    f32::from(header.u16(42)?),
                    f32::from(header.u16(40)?),
                    0.,
                    0.,
                ]),
            );
        }
    }
    Ok((metadata, samplers))
}

pub(super) fn bindings(
    source: &mut Reader,
    native: &mut Reader,
    graph: &mut Graph,
    material: &Payload,
    inspection: &shader::inputs::Inputs,
) -> Result<Bindings> {
    let mut result = Bindings {
        textures: BTreeMap::new(),
        metadata: BTreeMap::new(),
        sampler_indices: BTreeSet::new(),
        table: Vec::new(),
    };
    let mut slots = BTreeSet::new();
    for row in material.array(0x280, 24, None)? {
        let slot = material.u32(row)?;
        ensure!(
            slot < 128 && slots.insert(slot),
            "Duplicate source texture register"
        );
        if inspection.resources.contains_key(&slot) {
            let tag = source.ref64(material, row + 8)?;
            result
                .textures
                .insert(slot, texture::build(source, native, graph, tag)?);
        }
    }
    let (metadata, samplers) = inputs(source, material)?;
    result.metadata = metadata;
    result.sampler_indices = samplers;
    for (index, row) in material.array(0x2b8, 16, None)?.into_iter().enumerate() {
        let tag = source.ref64(material, row)?;
        let symbol = if result.sampler_indices.contains(&u8::try_from(index)?) {
            sampler(source, native, graph, tag)?
        } else {
            texture::build(source, native, graph, tag)?
        };
        result.table.push(symbol);
    }
    Ok(result)
}

impl Bindings {
    pub fn append(
        &self,
        native: &mut Reader,
        bytes: &mut Vec<u8>,
        patches: &mut Vec<Value>,
    ) -> Result<()> {
        let mut fixed = Vec::new();
        for slot in self.textures.keys() {
            fixed.extend(slot.to_le_bytes());
            fixed.extend(u32::MAX.to_le_bytes());
        }
        append(bytes, 0x2d0, 0x80807211, &fixed, 8)?;
        for (row, symbol) in Payload(bytes.clone())
            .array(0x2d0, 8, None)?
            .into_iter()
            .zip(self.textures.values())
        {
            patches.push(json!({"offset":row+4,"symbol":symbol}));
        }
        let template = native.tag(0x80ec270d, Some(0x808071e8))?;
        let rows = template.array(0x308, 16, None)?;
        let class = template.u32(
            rows.first()
                .context("Native sampler table is empty")?
                .checked_sub(8)
                .context("Native sampler table header")?,
        )?;
        let mut table = Vec::new();
        for _ in &self.table {
            table.extend(u32::MAX.to_le_bytes());
            table.extend(1u32.to_le_bytes());
            table.extend(0u64.to_le_bytes());
        }
        append(bytes, 0x308, u64::from(class), &table, 16)?;
        for (row, symbol) in Payload(bytes.clone())
            .array(0x308, 16, None)?
            .into_iter()
            .zip(&self.table)
        {
            patches.push(json!({"offset":row,"symbol":symbol}));
        }
        Ok(())
    }
}

pub(super) fn vectors(payload: &Payload, at: usize) -> Result<Vec<[u8; 16]>> {
    payload
        .array(at, 16, None)?
        .into_iter()
        .map(|row| payload.bytes(row))
        .collect()
}

pub(super) fn program(
    native: &mut Reader,
    graph: &mut Graph,
    symbol: &str,
    template: u32,
    text: &str,
    vertex: bool,
) -> Result<Vec<u8>> {
    let (code, warnings) = shader::compile(text, vertex)?;
    fs::write(graph.root.join(format!("{symbol}.hlsl")), text)?;
    fs::write(graph.root.join(format!("{symbol}.log")), warnings)?;
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 8, &u32::try_from(code.len())?.to_le_bytes())?;
    let raw = format!("{symbol}-data");
    graph.add(symbol, template, &header, Some(&raw), vec![])?;
    graph.add(
        &raw,
        native.reference(template)?,
        &code,
        Some(symbol),
        vec![],
    )?;
    Ok(code)
}

pub(super) fn sampler(
    source: &mut Reader,
    native: &mut Reader,
    graph: &mut Graph,
    tag: u32,
) -> Result<String> {
    let name = format!("sampler-{tag:08X}");
    if graph.nodes.iter().any(|n| n["symbol"] == name) {
        return Ok(name);
    }
    let header = source.tag(tag, None)?;
    let bytes = source.tag(source.reference(tag)?, None)?;
    ensure!(
        header.0 == [0; 8] && bytes.0.len() == 52,
        "Unsupported source sampler descriptor"
    );
    let filter = bytes.u32(0)?;
    ensure!(
        matches!(filter, 0 | 0x14 | 0x15 | 0x55)
            && (1..=16).contains(&bytes.u32(20)?)
            && (1..=8).contains(&bytes.u32(24)?),
        "Unsupported source sampler filtering"
    );
    for offset in [4, 8, 12] {
        ensure!(
            (1..=5).contains(&bytes.u32(offset)?),
            "Unsupported source sampler address mode"
        );
    }
    let template = 0x80c70b8e;
    let native_header = native.tag(template, None)?;
    let native_data = native.reference(template)?;
    ensure!(
        native_header.0 == [0; 8] && native.tag(native_data, None)?.0.len() == 52,
        "Native sampler envelope differs"
    );
    let raw = format!("{name}-data");
    graph.add(&name, template, &native_header.0, Some(&raw), vec![])?;
    graph.add(&raw, native_data, &bytes.0, Some(&name), vec![])?;
    Ok(name)
}

pub(super) fn colors(
    native: &mut Reader,
    graph: &mut Graph,
    mesh: &geometry::Mesh,
) -> Result<String> {
    let name = "vertex-colors";
    if graph.nodes.iter().any(|n| n["symbol"] == name) {
        return Ok(name.into());
    }
    ensure!(
        mesh.colors.len() == mesh.positions.len(),
        "Source vertex-color count differs"
    );
    let height = mesh.colors.len().div_ceil(256);
    let mut bytes = mesh.colors.iter().flatten().copied().collect::<Vec<_>>();
    bytes.resize(256 * height * 4, 0);
    let template = 0x80ec31bc;
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
    put(&mut header, 4, &28u32.to_le_bytes())?;
    put(&mut header, 14, &256u16.to_le_bytes())?;
    put(&mut header, 16, &u16::try_from(height)?.to_le_bytes())?;
    put(&mut header, 18, &1u16.to_le_bytes())?;
    put(&mut header, 20, &1u16.to_le_bytes())?;
    header[22] = 0;
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, bytes.len())?;
    graph.add(name, template, &header, Some("vertex-colors-data"), vec![])?;
    graph.add(
        "vertex-colors-data",
        native.reference(template)?,
        &bytes,
        Some(name),
        vec![],
    )?;
    Ok(name.into())
}

pub(super) fn fresh(shell: &Payload, source: &Payload, stage: u8) -> Result<Vec<u8>> {
    let state = source.u32(0x30)?;
    ensure!(
        matches!(
            (stage, state),
            (0, 0 | 0x810000) | (2, 0x9a) | (4 | 13, 0) | (8 | 17, 0x88)
        ),
        "Unsupported Marathon render state {state:08X} in stage {stage}"
    );
    envelope(shell, state)
}

pub(super) fn envelope(shell: &Payload, state: u32) -> Result<Vec<u8>> {
    let mut bytes = shell.0.clone();
    for offset in [24, 28] {
        put(
            &mut bytes,
            offset,
            &(shell.u32(offset)? & !0x1e000000).to_le_bytes(),
        )?;
    }
    put(&mut bytes, 32, &state.to_le_bytes())?;
    for base in [0x48, 0xe8, 0x188, 0x228, 0x2c8, 0x368] {
        bytes[base..base + 0xa0].fill(0);
        for offset in [0, 0x80, 0x84] {
            put(&mut bytes, base + offset, &u32::MAX.to_le_bytes())?;
        }
    }
    Ok(bytes)
}
