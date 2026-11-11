//! Complete private GPU resource edges for one paired material. No system wrapper is implied.
use super::{Kind, Material};
use crate::{
    d1_roi::{
        Source,
        dependency::{
            self,
            resource::{Node, append, validate},
        },
    },
    presentation::Graph,
    tiger::{
        reader::Reader,
        shader::{
            identity::Stage,
            program::{Program, Template},
        },
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tiger_pkg::TagHash;

pub struct Prepared {
    pub material: String,
    pub nodes: Vec<Node>,
    pub evidence: Value,
}

impl Prepared {
    /// Follow both payload and package-reference edges before exposing the complete closure.
    pub fn validate(&self) -> Result<()> {
        validate(&self.nodes, &[self.material.as_str()])
    }

    pub(crate) fn append(self, graph: &mut Graph) -> Result<String> {
        self.validate()?;
        append(self.nodes, graph)?;
        Ok(self.material)
    }

    /// Save an inspectable material fragment. It has no effect wrapper or installable ability root.
    pub fn save(self, directory: &Path) -> Result<()> {
        self.validate()?;
        ensure!(
            !directory.exists(),
            "Particle material output already exists"
        );
        std::fs::create_dir_all(directory)?;
        let evidence = self.evidence.clone();
        let mut graph = Graph::new(directory.to_owned(), Vec::new());
        let root = self.append(&mut graph)?;
        std::fs::write(
            directory.join("provenance.json"),
            serde_json::to_vec_pretty(&evidence)?,
        )?;
        std::fs::write(
            directory.join("asset-graph.json"),
            serde_json::to_vec_pretty(&json!({
                "installable":false,"roots":[root],"nodes":graph.nodes
            }))?,
        )?;
        Ok(())
    }
}

fn template(native: &Reader, kind: u8, subtype: u8) -> Result<u32> {
    native
        .manager
        .lookup
        .tag32_entries_by_pkg
        .iter()
        .flat_map(|(&package, entries)| {
            entries
                .iter()
                .enumerate()
                .filter_map(move |(index, entry)| {
                    (entry.file_type == kind && entry.file_subtype == subtype)
                        .then(|| {
                            u16::try_from(index)
                                .ok()
                                .map(|index| TagHash::new(package, index).0)
                        })
                        .flatten()
                })
        })
        .min()
        .with_context(|| format!("No native particle resource template for {kind}/{subtype}"))
}

fn pair(native: &Reader, tag: u32, kind: u8, subtype: u8) -> Result<u32> {
    let entry = native
        .manager
        .get_entry(TagHash(tag))
        .context("Particle resource template is absent")?;
    let data = native
        .manager
        .get_entry(TagHash(entry.reference))
        .context("Particle resource data template is absent")?;
    ensure!(
        entry.file_type == kind
            && entry.file_subtype == subtype
            && data.file_type == kind + 8
            && data.file_subtype == subtype
            && data.reference == tag,
        "Particle resource templates do not form the expected reciprocal pair"
    );
    Ok(entry.reference)
}

fn resource(
    nodes: &mut Vec<Node>,
    symbol: String,
    header_template: u32,
    data_template: u32,
    header: Vec<u8>,
    data: Vec<u8>,
) {
    let backing = format!("{symbol}-data");
    nodes.push(Node {
        symbol: backing.clone(),
        template: data_template,
        payload: data,
        reference: Some(symbol.clone()),
        patches: Vec::new(),
    });
    nodes.push(Node {
        symbol,
        template: header_template,
        payload: header,
        reference: Some(backing),
        patches: Vec::new(),
    });
}

impl Material {
    /// Resolve every shader, texture and sampler obligation into private native resources.
    /// Paired material variants retain separate shader identities even when their source tag agrees.
    pub fn assets(self, source: &Source, native: &mut Reader, namespace: &str) -> Result<Prepared> {
        dependency::symbol(namespace)?;
        ensure!(
            native.is_native(),
            "Particle render allocation requires native packages"
        );
        let definition = self
            .definition
            .context("Particle render material is not paired")?;
        let root = format!("{namespace}-material");
        let mut nodes = Vec::new();
        let mut dependencies = BTreeMap::new();
        let mut patches = Vec::new();
        let mut evidence = Vec::new();
        let mut shaders = BTreeSet::new();
        for reference in &self.references {
            let kind = match reference.kind {
                Kind::VertexShader => 0,
                Kind::PixelShader => 1,
                Kind::Texture => 2,
                Kind::Sampler => 3,
            };
            let key = (kind, reference.source);
            if let std::collections::btree_map::Entry::Vacant(entry) = dependencies.entry(key) {
                let suffix = match reference.kind {
                    Kind::VertexShader => "vertex",
                    Kind::PixelShader => "pixel",
                    Kind::Texture => "texture",
                    Kind::Sampler => "sampler",
                };
                let symbol = format!("{namespace}-{suffix}-{:08X}", reference.source);
                match reference.kind {
                    Kind::VertexShader | Kind::PixelShader => {
                        let stage = if reference.kind == Kind::VertexShader {
                            Stage::Vertex
                        } else {
                            Stage::Pixel
                        };
                        let candidates = self
                            .shaders
                            .iter()
                            .filter(|shader| {
                                shader.source == reference.source && shader.stage == stage
                            })
                            .collect::<Vec<_>>();
                        let [shader] = candidates.as_slice() else {
                            anyhow::bail!("Particle shader obligation is absent or repeated");
                        };
                        shaders.insert((reference.source, stage));
                        let header = template(native, 33, stage.subtype())?;
                        let allocation = Template::read(native, header)?;
                        let program = Program::new(stage, shader.bytecode.clone())?;
                        let assets = program.assets(&symbol, &allocation)?;
                        resource(
                            &mut nodes,
                            symbol.clone(),
                            assets.header_template,
                            assets.bytecode_template,
                            assets.header,
                            assets.bytecode,
                        );
                        evidence.push(json!({"kind":suffix,"source":reference.source,"symbol":symbol,"program":program.receipt()}));
                    }
                    Kind::Texture => {
                        let texture = source.linear_texture(reference.source, None)?;
                        let header = template(native, 32, texture.subtype())?;
                        let data = pair(native, header, 32, texture.subtype())?;
                        let volume = (texture.depth > 1)
                            .then(|| native.tag(header, None))
                            .transpose()?;
                        let payload = texture.native_header(volume.as_deref())?;
                        evidence.push(
                            json!({"kind":suffix,"source":reference.source,"symbol":symbol,
                            "format":texture.format,"mips":texture.mips,"layers":texture.layers,
                            "width":texture.width,"height":texture.height,"depth":texture.depth}),
                        );
                        resource(
                            &mut nodes,
                            symbol.clone(),
                            header,
                            data,
                            payload,
                            texture.bytes,
                        );
                    }
                    Kind::Sampler => {
                        let header = source.read(reference.source)?;
                        let descriptor = source.read(source.reference(reference.source)?)?;
                        ensure!(
                            header.0 == [0; 8]
                                && descriptor.0.len() == 60
                                && descriptor.0[52..] == [255; 8],
                            "Source particle sampler layout differs"
                        );
                        let header = template(native, 34, 1)?;
                        let data = pair(native, header, 34, 1)?;
                        let payload = native.tag(header, None)?;
                        ensure!(
                            payload.0 == [0; 8] && native.tag(data, None)?.0.len() == 52,
                            "Native particle sampler layout differs"
                        );
                        resource(
                            &mut nodes,
                            symbol.clone(),
                            header,
                            data,
                            payload.0.clone(),
                            descriptor.0[..52].to_vec(),
                        );
                        evidence
                            .push(json!({"kind":suffix,"source":reference.source,"symbol":symbol}));
                    }
                }
                entry.insert(symbol);
            }
            patches.push((reference.offset, dependencies[&key].clone()));
        }
        ensure!(
            shaders.len() == self.shaders.len(),
            "Particle material includes unused shader programs"
        );
        let template = native
            .classes(0x808071E8)
            .into_iter()
            .min()
            .context("Native material allocation template is absent")?;
        native.tag(template, Some(0x808071E8))?;
        nodes.push(Node {
            symbol: root.clone(),
            template,
            payload: self.payload.0,
            reference: None,
            patches,
        });
        let prepared = Prepared {
            material: root,
            nodes,
            evidence: json!({"definition":definition,
            "resources":evidence,"ribbon":self.ribbon,"appearance_rows":self.appearance_rows,
            "limits":["Private material resource closure only. Particle wrapper, lifecycle and gameplay require separate verification."]}),
        };
        prepared.validate()?;
        source.unchanged()?;
        Ok(prepared)
    }
}
