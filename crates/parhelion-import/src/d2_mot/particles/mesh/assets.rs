//! Native allocation templates and graph nodes for complete float geometry.
use super::*;
use serde_json::{Value, json};
use tiger_pkg::TagHash;

#[cfg(test)]
mod tests;

pub struct Templates {
    geometry: u32,
    container: u32,
    vertex: (u32, u32),
    index: (u32, u32),
}

#[derive(Default)]
pub struct Assets {
    pub files: BTreeMap<String, Vec<u8>>,
    pub nodes: Vec<Value>,
    pub external_materials: BTreeSet<String>,
}

fn symbol(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "invalid geometry graph symbol"
    );
    Ok(())
}

fn allocation(reader: &mut Reader, tag: u32, class: u32) -> Result<Payload> {
    let entry = reader
        .manager
        .get_entry(TagHash(tag))
        .context("missing geometry allocation template")?;
    ensure!(
        entry.file_type == 8 && entry.file_subtype == 0 && entry.reference == class,
        "geometry allocation template type or class differs"
    );
    Ok((*reader.tag(tag, Some(class))?).clone())
}

fn slots(bytes: &[u8]) -> Result<Vec<(usize, Kind)>> {
    let p = Payload(bytes.to_vec());
    ensure!(
        bytes.len() >= 160 && p.u64(0)? == bytes.len() as u64,
        "native float geometry envelope differs"
    );
    ensure!(
        p.u64(8)? == 0
            && p.u64(0x30)? == 32
            && p.u64(0x38)? == 0x0000030000000001
            && p.u64(0x40)? == 0
            && p.u64(0x48)? == 0
            && p.u32(0x90)? == u32::MAX
            && p.u32(0x94)? == 0
            && p.u64(0x98)? == 0,
        "native float geometry header differs"
    );
    let meshes = p.array(16, 136, Some(0x80807378))?;
    ensure!(
        !meshes.is_empty() && meshes.len() <= 256,
        "native geometry mesh count differs"
    );
    let mut result = Vec::new();
    for mesh in meshes {
        ensure!(
            [4, 8, 12]
                .into_iter()
                .all(|at| p.u32(mesh + at).ok() == Some(u32::MAX)),
            "native geometry has unsupported auxiliary streams"
        );
        for i in 0..23 {
            ensure!(
                p.u16(mesh + 88 + i * 2)? == 13,
                "native float input layout differs"
            );
        }
        let parts = p.array(mesh + 24, 32, Some(0x8080737E))?;
        ensure!(
            !parts.is_empty() && parts.len() <= u16::MAX as usize,
            "native draw count differs"
        );
        let ranges = (0..24)
            .map(|i| p.u16(mesh + 40 + i * 2))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            ranges[0] == 0
                && usize::from(ranges[23]) == parts.len()
                && ranges.windows(2).all(|w| w[0] <= w[1]),
            "native geometry stage ranges differ"
        );
        result.extend([(mesh, Kind::VertexBuffer), (mesh + 16, Kind::IndexBuffer)]);
        result.extend(parts.into_iter().map(|at| (at, Kind::Material)));
    }
    Ok(result)
}

impl Templates {
    pub fn read(reader: &mut Reader, geometry: u32, container: u32) -> Result<Self> {
        ensure!(
            reader.is_native(),
            "geometry allocation templates require native packages"
        );
        let model = allocation(reader, geometry, 0x808073A5)?;
        let references = slots(&model.0)?;
        let vertex_tag = model.u32(
            references
                .iter()
                .find(|(_, kind)| *kind == Kind::VertexBuffer)
                .context("native vertex allocation control")?
                .0,
        )?;
        let index_tag = model.u32(
            references
                .iter()
                .find(|(_, kind)| *kind == Kind::IndexBuffer)
                .context("native index allocation control")?
                .0,
        )?;
        let vertex = read_buffer(reader, vertex_tag, Kind::VertexBuffer)?;
        let index = read_buffer(reader, index_tag, Kind::IndexBuffer)?;
        let wrapper = allocation(reader, container, 0x80806E2E)?;
        ensure!(
            wrapper.0.len() >= 32
                && wrapper.u64(0)? == wrapper.0.len() as u64
                && wrapper.u32(8)? == u32::MAX
                && wrapper.u32(12)? == 0,
            "native model container template envelope differs"
        );
        ensure!(
            wrapper.array(16, 4, Some(0x808073A4))?.len() <= 256,
            "native model container template exceeds capacity"
        );
        Ok(Self {
            geometry,
            container,
            vertex: (vertex_tag, vertex.data_source),
            index: (index_tag, index.data_source),
        })
    }
}

impl Assets {
    fn add(
        &mut self,
        name: &str,
        template: u32,
        bytes: Vec<u8>,
        reference: Option<&str>,
        patches: Vec<Value>,
    ) -> Result<()> {
        symbol(name)?;
        let file = format!("{name}.bin");
        ensure!(
            self.files.insert(file.clone(), bytes).is_none(),
            "duplicate geometry asset file"
        );
        self.nodes
            .push(json!({"symbol":name,"file":file,"template":template,
            "reference":reference,"patches":patches}));
        Ok(())
    }

    pub fn append(&mut self, other: Self) -> Result<()> {
        ensure!(
            other
                .files
                .keys()
                .all(|name| !self.files.contains_key(name)),
            "duplicate geometry asset file"
        );
        let existing = self
            .nodes
            .iter()
            .map(|n| n["symbol"].as_str().context("geometry node symbol"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            other
                .nodes
                .iter()
                .all(|n| n["symbol"].as_str().is_some_and(|s| !existing.contains(s))),
            "duplicate geometry graph symbol"
        );
        self.files.extend(other.files);
        self.nodes.extend(other.nodes);
        self.external_materials.extend(other.external_materials);
        Ok(())
    }

    /// Only declared material symbols may remain outside this geometry fragment.
    pub fn validate_links(&self) -> Result<()> {
        let names = self
            .nodes
            .iter()
            .map(|n| n["symbol"].as_str().context("geometry node symbol"))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            names.len() == self.nodes.len(),
            "duplicate geometry graph symbol"
        );
        ensure!(
            self.external_materials
                .iter()
                .all(|name| !names.contains(name.as_str())),
            "external material collides with a geometry node"
        );
        for node in &self.nodes {
            if let Some(reference) = node["reference"].as_str() {
                ensure!(
                    names.contains(reference),
                    "geometry package reference is unresolved"
                );
            }
            for patch in node["patches"].as_array().context("geometry patches")? {
                let target = patch["symbol"].as_str().context("geometry patch symbol")?;
                ensure!(
                    names.contains(target) || self.external_materials.contains(target),
                    "geometry graph dependency is unresolved"
                );
            }
        }
        Ok(())
    }
}

impl Geometry {
    pub fn assets(
        &self,
        name: &str,
        templates: &Templates,
        materials: &BTreeMap<u32, String>,
    ) -> Result<Assets> {
        symbol(name)?;
        let expected = slots(&self.bytes)?;
        ensure!(
            expected.len() == self.references.len(),
            "geometry relocation count differs"
        );
        let mut seen = BTreeSet::new();
        let mut buffers = BTreeMap::new();
        let mut assets = Assets::default();
        for buffer in &self.buffers {
            buffer.validate()?;
            ensure!(
                buffers.insert(buffer.source, buffer).is_none(),
                "duplicate geometry buffer identity"
            );
            let header = format!("{name}-buffer-{:08X}", buffer.source);
            let data = format!("{header}-data");
            let pair = match buffer.kind {
                Kind::VertexBuffer => templates.vertex,
                Kind::IndexBuffer => templates.index,
                Kind::Material => bail!("material used as buffer"),
            };
            assets.add(&header, pair.0, buffer.header.clone(), Some(&data), vec![])?;
            assets.add(&data, pair.1, buffer.data.clone(), Some(&header), vec![])?;
        }
        let p = Payload(self.bytes.clone());
        for mesh in p.array(16, 136, Some(0x80807378))? {
            let buffer = |at: usize, kind: Kind| -> Result<&Buffer> {
                let reference = self
                    .references
                    .iter()
                    .find(|r| r.offset == at && r.kind == kind)
                    .context("geometry stream relocation is missing")?;
                buffers
                    .get(&reference.source)
                    .copied()
                    .context("geometry stream data is missing")
            };
            let vertex = buffer(mesh, Kind::VertexBuffer)?;
            let index = buffer(mesh + 16, Kind::IndexBuffer)?;
            ensure!(
                vertex.kind == Kind::VertexBuffer && index.kind == Kind::IndexBuffer,
                "geometry stream roles differ"
            );
            let vertices = vertex.data.len() / 32;
            for part in p.array(mesh + 24, 32, Some(0x8080737E))? {
                ensure!(
                    p.u16(part + 4)? == u16::MAX
                        && p.u16(part + 6)? == 3
                        && p.u16(part + 24)? == 0
                        && p.u16(part + 30)? == 0,
                    "native geometry draw contract differs"
                );
                let first = usize::try_from(p.u32(part + 8)?)?;
                let count = usize::try_from(p.u32(part + 12)?)?;
                ensure!(
                    count > 0 && count.is_multiple_of(3),
                    "geometry triangle count differs"
                );
                let indices = index
                    .data
                    .get(first * 2..(first + count) * 2)
                    .context("geometry draw exceeds index buffer")?;
                ensure!(
                    indices
                        .chunks_exact(2)
                        .all(|v| usize::from(u16::from_le_bytes([v[0], v[1]])) < vertices),
                    "geometry draw addresses a missing vertex"
                );
            }
        }
        let mut patches = Vec::new();
        let mut used = BTreeSet::new();
        for reference in &self.references {
            ensure!(
                seen.insert(reference.offset)
                    && expected.contains(&(reference.offset, reference.kind))
                    && p.u32(reference.offset)? == u32::MAX,
                "invalid or linked geometry relocation"
            );
            let target = match reference.kind {
                Kind::Material => {
                    let target = materials
                        .get(&reference.source)
                        .context("geometry material symbol is unresolved")?;
                    symbol(target)?;
                    assets.external_materials.insert(target.clone());
                    target.clone()
                }
                kind => {
                    let buffer = buffers
                        .get(&reference.source)
                        .context("geometry buffer is missing")?;
                    ensure!(buffer.kind == kind, "geometry buffer role differs");
                    used.insert(reference.source);
                    format!("{name}-buffer-{:08X}", reference.source)
                }
            };
            patches.push(json!({"offset":reference.offset,"symbol":target}));
        }
        ensure!(
            used.len() == buffers.len(),
            "geometry contains an unreferenced buffer"
        );
        assets.add(name, templates.geometry, self.bytes.clone(), None, patches)?;
        assets.validate_links()?;
        Ok(assets)
    }
}

pub fn container(
    bytes: &[u8],
    name: &str,
    templates: &Templates,
    geometries: &BTreeMap<u32, String>,
) -> Result<Assets> {
    symbol(name)?;
    let models = super::super::system::model_container_sources(bytes)?;
    let mut patches = Vec::new();
    for (i, model) in models.iter().enumerate() {
        let target = geometries
            .get(model)
            .context("particle container geometry symbol is unresolved")?;
        symbol(target)?;
        ensure!(target != name, "particle container refers to itself");
        patches.push(json!({"offset":64+i*4,"symbol":target}));
    }
    let mut assets = Assets::default();
    assets.add(
        name,
        templates.container,
        super::super::system::model_container_payload(&models),
        None,
        patches,
    )?;
    Ok(assets)
}
