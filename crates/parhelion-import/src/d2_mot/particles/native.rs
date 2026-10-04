//! Native particle systems converted from source particle systems.
//!
//! Each source system becomes a native system, program and material, with its shaders and
//! textures. The assets are written as private graph nodes: payload files whose package
//! references name other nodes by symbol, for the build to allocate and link. An emitter mesh
//! becomes a native model container with its float geometry and buffers. Systems that need GPU
//! simulation passes are reported rather than converted.
use super::mesh::{self, Kind, assets};
use super::system::{self as particle_system, System};
use crate::d2_mot::{
    payload::Payload,
    reader::{Reader, write_json},
};
use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod dxbc;
mod material;
mod program;
mod shader;
mod texture;

pub struct Request<'a> {
    pub source_packages: &'a Path,
    pub native_packages: &'a Path,
    /// Render inputs exported by the importer, with `tfx-modern` and `tfx-native` contexts.
    pub render_inputs: &'a Path,
    /// The pinned shader decompiler, used for lit shaders only.
    pub decompiler: &'a Path,
    /// Graph folder receiving `particles/*.bin`.
    pub graph: &'a Path,
    /// Fresh folder for the checked source and native reads.
    pub work: &'a Path,
    pub systems: &'a [u32],
    /// Named source inputs the weapon fixes, such as its damage type.
    pub inputs: &'a BTreeMap<u32, f32>,
    /// A shipped native particle system whose program, material, shaders and first pixel
    /// texture lend their package entry metadata to the converted assets.
    pub template: u32,
}

pub(super) struct Node {
    pub symbol: String,
    pub template: u32,
    pub payload: Vec<u8>,
    pub reference: Option<String>,
    /// Payload words holding `u32::MAX` that receive another node's tag.
    pub patches: Vec<(usize, String)>,
}

#[derive(Default)]
pub(super) struct Nodes {
    list: Vec<Node>,
    symbols: BTreeSet<String>,
}

impl Nodes {
    pub fn contains(&self, symbol: &str) -> bool {
        self.symbols.contains(symbol)
    }

    pub fn add(&mut self, node: Node) -> Result<()> {
        ensure!(
            self.symbols.insert(node.symbol.clone()),
            "duplicate particle node {}",
            node.symbol
        );
        for (offset, _) in &node.patches {
            ensure!(
                node.payload.get(*offset..offset + 4) == Some(&u32::MAX.to_le_bytes()[..]),
                "particle node {} patch at {offset:X} is not a placeholder",
                node.symbol
            );
        }
        self.list.push(node);
        Ok(())
    }
}

pub(super) struct Shader {
    pub header_tag: u32,
    pub data_tag: u32,
    pub header: Vec<u8>,
}

pub(super) struct Texture {
    pub header_tag: u32,
    pub data_tag: u32,
    pub header: Vec<u8>,
}

pub(super) struct Templates {
    pub system: u32,
    pub emitter: u32,
    pub program: u32,
    pub material: u32,
    pub vertex: Shader,
    pub pixel: Shader,
    pub texture: Texture,
}

pub(super) struct Context<'a> {
    pub request: &'a Request<'a>,
    pub source: Reader,
    pub native: Reader,
    pub templates: Templates,
    pub nodes: Nodes,
    pub globals: BTreeMap<u8, u8>,
    pub work: PathBuf,
    samplers: Option<BTreeMap<Vec<u8>, u32>>,
    geometry: Option<(assets::Templates, u32)>,
}

/// Append a native array: a 16-aligned header after its class marker, then the rows.
/// Returns where the rows start, or zero for an empty array.
pub(super) fn array(
    out: &mut Vec<u8>,
    field: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> Result<usize> {
    ensure!(
        stride > 0 && rows.len().is_multiple_of(stride),
        "particle array stride differs"
    );
    if rows.is_empty() {
        out[field..field + 16].fill(0);
        return Ok(0);
    }
    let header = (out.len() + 4).next_multiple_of(16);
    out.resize(header, 0);
    out[header - 4..header].copy_from_slice(&0x80809FBDu32.to_le_bytes());
    let count = (rows.len() / stride) as u64;
    out.extend(count.to_le_bytes());
    out.extend(u64::from(class).to_le_bytes());
    out.extend_from_slice(rows);
    out[field..field + 8].copy_from_slice(&count.to_le_bytes());
    let delta = i64::try_from(header)? - i64::try_from(field + 8)?;
    out[field + 8..field + 16].copy_from_slice(&delta.to_le_bytes());
    Ok(header + 16)
}

/// Package entry type, subtype and reference.
fn entry(reader: &Reader, tag: u32) -> Result<(u32, u32, u32)> {
    let e = reader
        .manager
        .get_entry(tiger_pkg::TagHash(tag))
        .with_context(|| format!("missing native template {tag:08X}"))?;
    Ok((
        u32::from(e.file_type),
        u32::from(e.file_subtype),
        e.reference,
    ))
}

fn templates(native: &mut Reader, system: u32) -> Result<Templates> {
    let s = native.tag(system, Some(0x80806E28))?;
    ensure!(s.0.len() == 52, "native template system size differs");
    let (program, material, emitter) = (s.u32(0)?, s.u32(0x14)?, s.u32(0x18)?);
    native.tag(program, Some(0x80806E2C))?;
    native.tag(emitter, Some(0x80806E2E))?;
    let m = native.tag(material, Some(0x808071E8))?;
    let shader = |native: &mut Reader, tag: u32, subtype: u32| -> Result<Shader> {
        let (kind, sub, data) = entry(native, tag)?;
        ensure!(
            kind == 33 && sub == subtype,
            "native template shader {tag:08X} stage differs"
        );
        ensure!(
            entry(native, data)?.0 == 41,
            "native template shader data differs"
        );
        Ok(Shader {
            header_tag: tag,
            data_tag: data,
            header: native.tag(tag, None)?.0.clone(),
        })
    };
    let vertex = shader(native, m.u32(0x48)?, 1)?;
    let pixel = shader(native, m.u32(0x2C8)?, 0)?;
    let rows = m.array(0x2C8 + 8, 8, None)?;
    let first = *rows
        .first()
        .context("native template material has no pixel texture")?;
    let texture_tag = m.u32(first + 4)?;
    let (kind, _, data) = entry(native, texture_tag)?;
    ensure!(
        kind == 32 && entry(native, data)?.0 == 40,
        "native template texture differs"
    );
    let texture_header = native.tag(texture_tag, None)?.0.clone();
    ensure!(
        texture_header.len() == 40,
        "native template texture header differs"
    );
    Ok(Templates {
        system,
        emitter,
        program,
        material,
        vertex,
        pixel,
        texture: Texture {
            header_tag: texture_tag,
            data_tag: data,
            header: texture_header,
        },
    })
}

impl Context<'_> {
    /// The shipped native sampler whose descriptor equals the source sampler's.
    ///
    /// Equal descriptors repeat across packages, and only some of those packages stay loaded.
    /// The boot package holds copies that are gone in world, so a material naming one faults
    /// the first time it draws. Stock materials name the shared copies: 93% of the sampler
    /// rows in 3,000 sampled stock materials name the package holding the most samplers. That
    /// package's copy wins, then the lowest tag.
    pub fn native_sampler(&mut self, source: u32) -> Result<u32> {
        let header = self.source.tag(source, None)?;
        ensure!(
            header.0.len() == 8 && header.0.iter().all(|v| *v == 0),
            "particle sampler header differs"
        );
        let descriptor = self
            .source
            .tag(self.source.reference(source)?, None)?
            .0
            .clone();
        ensure!(
            descriptor.len() == 52,
            "particle sampler descriptor differs"
        );
        if self.samplers.is_none() {
            let mut index = BTreeMap::new();
            let mut candidates = Vec::new();
            for (&package, entries) in &self.native.manager.lookup.tag32_entries_by_pkg {
                let held = entries.iter().filter(|e| e.file_type == 34).count();
                for (at, e) in entries.iter().enumerate() {
                    if e.file_type == 34 {
                        candidates.push((
                            std::cmp::Reverse(held),
                            tiger_pkg::TagHash::new(package, u16::try_from(at)?).0,
                            e.reference,
                        ));
                    }
                }
            }
            candidates.sort_unstable();
            for (_, tag, data) in candidates {
                if let Ok(bytes) = self.native.manager.read_tag(tiger_pkg::TagHash(data))
                    && bytes.len() == 52
                {
                    index.entry(bytes).or_insert(tag);
                }
            }
            self.samplers = Some(index);
        }
        self.samplers
            .as_ref()
            .and_then(|index| index.get(&descriptor))
            .copied()
            .with_context(|| format!("no native sampler matches source sampler {source:08X}"))
    }
}

impl Context<'_> {
    /// The allocation templates converted emitter geometry takes its package entries from: the
    /// first shipped float geometry and the first shipped model container, in tag order, that
    /// the geometry writer accepts, and the material that geometry's first draw names.
    fn geometry_templates(&mut self) -> Result<(&assets::Templates, u32)> {
        if self.geometry.is_none() {
            let mut geometries = self.native.classes(0x808073A5);
            geometries.sort_unstable();
            let mut containers = self.native.classes(0x80806E2E);
            containers.sort_unstable();
            let container = containers
                .into_iter()
                .find(|&tag| {
                    self.native.tag(tag, None).is_ok_and(|p| {
                        p.array(16, 4, Some(0x808073A4))
                            .is_ok_and(|models| !models.is_empty())
                    })
                })
                .context("no shipped particle model container")?;
            let (templates, geometry) = geometries
                .into_iter()
                .find_map(|geometry| {
                    assets::Templates::read(&mut self.native, geometry, container)
                        .ok()
                        .map(|templates| (templates, geometry))
                })
                .context("no shipped float geometry the geometry writer accepts")?;
            let p = Payload(self.native.tag(geometry, Some(0x808073A5))?.0.clone());
            let mesh = *p
                .array(16, 136, Some(0x80807378))?
                .first()
                .context("template geometry has no mesh")?;
            let draw = *p
                .array(mesh + 24, 32, Some(0x8080737E))?
                .first()
                .context("template geometry has no draw")?;
            let material = p.u32(draw)?;
            self.native.tag(material, Some(0x808071E8))?;
            self.geometry = Some((templates, material));
        }
        let (templates, material) = self.geometry.as_ref().expect("set above");
        Ok((templates, *material))
    }

    /// A private copy of the shipped material emitter geometry draws name. Shipped emitter
    /// geometry draws name a few shared materials whatever the particle system's own material
    /// is (one container serves systems with different materials), so the draw material is not
    /// what the particles render with and the source mesh's own material is not converted.
    fn emitter_draw_material(&mut self) -> Result<String> {
        let (_, material) = self.geometry_templates()?;
        let symbol = format!("particle-emitter-material-{material:08X}");
        if !self.nodes.contains(&symbol) {
            let payload = self.native.tag(material, Some(0x808071E8))?.0.clone();
            self.nodes.add(Node {
                symbol: symbol.clone(),
                template: material,
                payload,
                reference: None,
                patches: Vec::new(),
            })?;
        }
        Ok(symbol)
    }

    fn add_assets(&mut self, assets: assets::Assets) -> Result<()> {
        for node in &assets.nodes {
            let symbol = node["symbol"].as_str().context("geometry node symbol")?;
            let file = node["file"].as_str().context("geometry node file")?;
            let payload = assets
                .files
                .get(file)
                .context("geometry node payload is missing")?
                .clone();
            let template = node["template"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .context("geometry node template")?;
            let patches = node["patches"]
                .as_array()
                .context("geometry node patches")?
                .iter()
                .map(|p| {
                    Ok((
                        usize::try_from(p["offset"].as_u64().context("geometry patch offset")?)?,
                        p["symbol"]
                            .as_str()
                            .context("geometry patch symbol")?
                            .to_owned(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            self.nodes.add(Node {
                symbol: symbol.to_owned(),
                template,
                payload,
                reference: node["reference"].as_str().map(str::to_owned),
                patches,
            })?;
        }
        Ok(())
    }

    /// The native model container for a source emitter mesh container, with each float
    /// geometry and its buffers.
    fn emitter(&mut self, container: u32) -> Result<String> {
        let symbol = format!("particle-emitter-{container:08X}");
        if self.nodes.contains(&symbol) {
            return Ok(symbol);
        }
        let bytes = self.source.tag(container, Some(0x80806929))?.0.clone();
        let models = particle_system::model_container_sources(&bytes)?;
        let mut geometries = BTreeMap::new();
        for model in models {
            let name = format!("particle-geometry-{model:08X}");
            if !geometries.contains_key(&model) && !self.nodes.contains(&name) {
                let geometry = mesh::convert(&mut self.source, model)
                    .with_context(|| format!("particle emitter mesh {model:08X}"))?;
                let draw_material = self.emitter_draw_material()?;
                let materials = geometry
                    .references
                    .iter()
                    .filter(|r| r.kind == Kind::Material)
                    .map(|r| (r.source, draw_material.clone()))
                    .collect();
                let (templates, _) = self.geometry_templates()?;
                let assets = geometry.assets(&name, templates, &materials)?;
                self.add_assets(assets)?;
            }
            geometries.insert(model, name);
        }
        let (templates, _) = self.geometry_templates()?;
        let assets = assets::container(&bytes, &symbol, templates, &geometries)?;
        self.add_assets(assets)?;
        Ok(symbol)
    }
}

fn system(c: &mut Context, source: u32) -> Result<(String, Value)> {
    let symbol = format!("particle-system-{source:08X}");
    let payload = c.source.tag(source, Some(0x80806920))?.0.clone();
    let lookup = c.source.manager.clone();
    let read = System::read(&payload, |hash| {
        lookup
            .lookup
            .tag64_entries
            .get(&hash)
            .map(|entry| entry.hash32.0)
            .context("particle emitter mesh reference is unresolved")
    })?;
    ensure!(
        read.compute.iter().all(Option::is_none),
        "GPU particle simulation passes require compute conversion"
    );
    ensure!(
        read.gpu_binding.is_none(),
        "particle compiled identity requires GPU conversion"
    );
    let program_symbol = format!("particle-program-{:08X}", read.program);
    if !c.nodes.contains(&program_symbol) {
        let program = c.source.tag(read.program, Some(0x80806927))?;
        let identity = Payload(program.0.clone()).u32(0x100)?;
        // Native identities share the 82 prefix and sit above the runtime package range, so
        // reference walks never read them as tags (stock ones run from 8242 to 824C). The
        // source identity's low bits keep them distinct, with bit 23 set clear of every stock one.
        let native_identity = 0x8280_0000 | (identity & 0x007F_FFFF);
        let bytes = program::native(&program.0, c.request.inputs, native_identity)
            .with_context(|| format!("particle program {:08X}", read.program))?;
        c.nodes.add(Node {
            symbol: program_symbol.clone(),
            template: c.templates.program,
            payload: bytes,
            reference: None,
            patches: Vec::new(),
        })?;
    }
    let (material_symbol, material) = if c
        .nodes
        .contains(&format!("particle-material-{:08X}", read.material))
    {
        (
            format!("particle-material-{:08X}", read.material),
            json!({"source":format!("{:08X}", read.material),"shared":true}),
        )
    } else {
        material::convert(c, read.material)
            .with_context(|| format!("particle material {:08X}", read.material))?
    };
    let emitter = read
        .model
        .map(|container| c.emitter(container))
        .transpose()?;
    let mut out = vec![0u8; 52];
    out[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    for at in [4, 8, 0x10, 0x14, 0x1C, 0x30] {
        out[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    out[0x0C..0x10].copy_from_slice(&read.render_layout.to_le_bytes());
    out[0x20..0x30].copy_from_slice(&read.draw_metadata);
    let mut patches = vec![(0, program_symbol.clone()), (0x14, material_symbol)];
    match &emitter {
        // A mesh emitter names its converted model container, a point emitter the template's.
        Some(container) => {
            out[0x18..0x1C].copy_from_slice(&u32::MAX.to_le_bytes());
            patches.push((0x18, container.clone()));
        }
        None => out[0x18..0x1C].copy_from_slice(&c.templates.emitter.to_le_bytes()),
    }
    c.nodes.add(Node {
        symbol: symbol.clone(),
        template: c.templates.system,
        payload: out,
        reference: None,
        patches,
    })?;
    Ok((
        symbol,
        json!({"source":format!("{source:08X}"),"program":program_symbol,"material":material,
            "emitter":emitter,
            "render_layout":format!("{:08X}", read.render_layout),"draw_metadata":hex::encode(read.draw_metadata)}),
    ))
}

/// Convert the requested systems and write their nodes beneath the graph folder. Returns the
/// graph's `particles` section: nodes, converted systems by source tag, and refusals.
pub fn convert(request: &Request) -> Result<Value> {
    ensure!(
        !request.work.exists(),
        "particle conversion work folder already exists"
    );
    let source = Reader::new(request.source_packages, &request.work.join("source"), true)?;
    let mut native = Reader::new(request.native_packages, &request.work.join("native"), false)?;
    let templates = templates(&mut native, request.template)?;
    let mut c = Context {
        request,
        source,
        native,
        templates,
        nodes: Nodes::default(),
        globals: material::globals(request.render_inputs)?,
        work: request.work.to_owned(),
        samplers: None,
        geometry: None,
    };
    let mut systems = serde_json::Map::new();
    let mut evidence = Vec::new();
    let mut refused = serde_json::Map::new();
    for &tag in request.systems {
        let checkpoint = c.nodes.list.len();
        let checkpoint_symbols = c.nodes.symbols.clone();
        match system(&mut c, tag) {
            Ok((symbol, record)) => {
                systems.insert(format!("{tag:08X}"), json!(symbol));
                evidence.push(record);
            }
            Err(error) => {
                // A refused system leaves none of its partial nodes behind.
                c.nodes.list.truncate(checkpoint);
                c.nodes.symbols = checkpoint_symbols;
                refused.insert(format!("{tag:08X}"), json!(format!("{error:#}")));
            }
        }
    }
    c.source.finish()?;
    c.native.finish()?;
    let folder = request.graph.join("particles");
    fs::create_dir_all(&folder)?;
    let mut nodes = Vec::with_capacity(c.nodes.list.len());
    for node in &c.nodes.list {
        let file = format!("particles/{}.bin", node.symbol);
        fs::write(request.graph.join(&file), &node.payload)?;
        nodes.push(json!({
            "symbol": node.symbol,
            "file": file,
            "template": node.template,
            "reference": node.reference,
            "patches": node.patches.iter().map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol})).collect::<Vec<_>>(),
        }));
    }
    let section = json!({"nodes":nodes,"systems":systems,"refused":refused,
        "inputs":request.inputs.iter().map(|(k, v)| (format!("{k:08X}"), json!(v))).collect::<serde_json::Map<_, _>>(),
        "template":format!("{:08X}", request.template),"installable":true,"gameplay_verified":false});
    write_json(
        &request.work.join("particles.json"),
        &json!({"section":section,"evidence":evidence}),
    )?;
    Ok(section)
}
