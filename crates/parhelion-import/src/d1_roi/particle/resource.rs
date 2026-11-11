//! Assemble one complete native particle system from a source wrapper and its three leaves.
use super::wrapper::Wrapper;
use crate::{
    d1_roi::{
        Source,
        dependency::{
            self,
            resource::{self as resources, Node},
        },
    },
    d2_mot::particles::mesh::{Kind, assets::Templates},
    presentation::Graph,
    tiger::reader::Reader,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub struct Resources {
    /// The `80806E28` system root. Its private edges name the three leaf roots.
    pub system: String,
    pub definition: String,
    pub material: String,
    pub geometry: String,
    pub nodes: Vec<Node>,
    pub evidence: Value,
}

fn allocation(native: &mut Reader, class: u32) -> Result<u32> {
    let tag = native
        .classes(class)
        .into_iter()
        .min()
        .with_context(|| format!("Native particle allocation class {class:08X} is absent"))?;
    native.tag(tag, Some(class))?;
    Ok(tag)
}

fn geometry_templates(native: &mut Reader) -> Result<Templates> {
    // The package index lists a class's tags in no stable order. The lowest qualifying container
    // gives every import of the same source the same templates.
    let mut containers = native.classes(0x80806E2E);
    containers.sort_unstable();
    for tag in containers {
        let container = native.tag(tag, Some(0x80806E2E))?;
        let models = container.array(16, 4, Some(0x808073A4))?;
        for row in models {
            let model = container.u32(row)?;
            if let Ok(templates) = Templates::read(native, model, tag) {
                return Ok(templates);
            }
        }
    }
    anyhow::bail!("Native particle float geometry allocation templates are absent")
}

impl Source {
    /// Prepare a native system and its definition, material and geometry from one source
    /// wrapper. Numeric input bindings and compiled identity belong to its containing owner,
    /// not the shared hash-like word in the source wrapper.
    pub fn particle_resources(
        &self,
        system: u32,
        native: &mut Reader,
        inputs: &BTreeMap<u32, u32>,
        pool_rows: usize,
        identity: u32,
        namespace: &str,
    ) -> Result<Resources> {
        dependency::symbol(namespace)?;
        ensure!(
            native.is_native(),
            "Particle resources require native packages"
        );
        // The checked wrapper reader establishes the bounded CPU wrapper envelope.
        let envelope = self.particle_wrapper(system)?;
        let wrapper = self.read(system)?;
        let definition_tag = wrapper.u32(0)?;
        let material_tag = wrapper.u32(0x10)?;
        let geometry_tag = wrapper.u32(0x14)?;
        let material = self.particle_material_for(material_tag, definition_tag, native)?;
        let definition =
            self.particle_definition(definition_tag, &material, inputs, pool_rows, identity)?;
        let material = material.assets(self, native, namespace)?;
        let container = self.particle_container(geometry_tag)?;
        let mut nodes = material.nodes;
        let mut models = BTreeMap::new();
        if !container.models().is_empty() {
            let templates = geometry_templates(native)?;
            for &tag in container.models() {
                if models.contains_key(&tag) {
                    continue;
                }
                let symbol = format!("{namespace}-model-{tag:08X}");
                let model = self.particle_geometry(tag)?;
                // Both original mode-one consumers bind the wrapper material for the draw.
                // Keep a valid private material edge in the model's otherwise unused slot.
                let materials = model
                    .references
                    .iter()
                    .filter(|r| r.kind == Kind::Material)
                    .map(|r| (r.source, material.material.clone()))
                    .collect();
                let mut geometry = model.assets(&symbol, &templates, &materials)?;
                geometry.validate_links()?;
                for node in geometry.nodes {
                    let file = node["file"]
                        .as_str()
                        .context("Particle geometry file is absent")?;
                    nodes.push(Node {
                        symbol: node["symbol"]
                            .as_str()
                            .context("Particle geometry symbol is absent")?
                            .into(),
                        template: u32::try_from(
                            node["template"]
                                .as_u64()
                                .context("Particle geometry template is absent")?,
                        )?,
                        payload: geometry
                            .files
                            .remove(file)
                            .context("Particle geometry bytes are absent")?,
                        reference: node["reference"].as_str().map(str::to_owned),
                        patches: node["patches"]
                            .as_array()
                            .context("Particle geometry patches are absent")?
                            .iter()
                            .map(|p| {
                                Ok((
                                    usize::try_from(
                                        p["offset"]
                                            .as_u64()
                                            .context("Particle geometry patch offset is absent")?,
                                    )?,
                                    p["symbol"]
                                        .as_str()
                                        .context("Particle geometry patch target is absent")?
                                        .to_owned(),
                                ))
                            })
                            .collect::<Result<_>>()?,
                    });
                }
                ensure!(
                    geometry.files.is_empty(),
                    "Particle geometry has unused files"
                );
                models.insert(tag, symbol);
            }
        }
        let geometry_symbol = format!("{namespace}-geometry");
        let container = container.write(&geometry_symbol, &models)?;
        nodes.push(Node {
            symbol: geometry_symbol.clone(),
            template: allocation(native, 0x80806E2E)?,
            payload: container.payload.0,
            reference: None,
            patches: container
                .patches
                .into_iter()
                .map(|p| (p.offset, p.symbol))
                .collect(),
        });
        let definition_symbol = format!("{namespace}-definition");
        nodes.push(Node {
            symbol: definition_symbol.clone(),
            template: allocation(native, 0x80806E2C)?,
            payload: definition.0,
            reference: None,
            patches: Vec::new(),
        });
        let system_symbol = format!("{namespace}-system");
        nodes.push(Node {
            symbol: system_symbol.clone(),
            template: allocation(native, 0x80806E28)?,
            payload: envelope.payload()?.0,
            reference: None,
            patches: Wrapper::EDGES
                .into_iter()
                .zip([&definition_symbol, &material.material, &geometry_symbol])
                .map(|(offset, symbol)| (offset, symbol.clone()))
                .collect(),
        });
        let resources = Resources {
            system: system_symbol,
            definition: definition_symbol,
            material: material.material,
            geometry: geometry_symbol,
            nodes,
            evidence: json!({"system":system,"definition":definition_tag,"material":material_tag,
                "geometry":geometry_tag,"identity":identity,"numeric_inputs":inputs,
                "numeric_pool_rows":pool_rows,"render_resources":material.evidence,
                "wrapper":{"identity":hex::encode(wrapper.bytes::<4>(0x18)?),
                    "stored":hex::encode(wrapper.bytes::<12>(0x18)?),"cost":[0,1]},
                "limits":["The stored footer words without a qualified native consumer are carried, not interpreted.",
                    "Natural lifecycle, renderer acceptance and gameplay remain separate."]}),
        };
        resources.validate()?;
        self.unchanged()?;
        Ok(resources)
    }
}

impl Resources {
    /// The system reaches every node, and its three edges name the distinct leaf roots.
    pub fn validate(&self) -> Result<()> {
        let roots = [
            &self.system,
            &self.definition,
            &self.material,
            &self.geometry,
        ];
        ensure!(
            roots
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == roots.len(),
            "Particle roots collide"
        );
        let system = self
            .nodes
            .iter()
            .find(|node| node.symbol == self.system)
            .context("Particle system node is absent")?;
        ensure!(
            system.payload.len() == 0x34
                && system
                    .patches
                    .iter()
                    .map(|(offset, symbol)| (*offset, symbol))
                    .eq(Wrapper::EDGES
                        .into_iter()
                        .zip(&roots[1..])
                        .map(|(o, s)| (o, *s))),
            "Particle system edges differ from its leaf roots"
        );
        resources::validate(&self.nodes, &[&self.system])
    }

    pub fn save(self, directory: &Path) -> Result<()> {
        self.validate()?;
        ensure!(
            !directory.exists(),
            "Particle resource output already exists"
        );
        std::fs::create_dir_all(directory)?;
        let mut graph = Graph::new(directory.to_owned(), Vec::new());
        resources::append(self.nodes, &mut graph)?;
        std::fs::write(
            directory.join("provenance.json"),
            serde_json::to_vec_pretty(&self.evidence)?,
        )?;
        // A standalone system has no owner that creates it. Sequence events provide that.
        std::fs::write(
            directory.join("asset-graph.json"),
            serde_json::to_vec_pretty(&json!({
                "installable":false,"roots":[self.system],
                "particle_roots":{"system":self.system,"definition":self.definition,
                    "material":self.material,"geometry":self.geometry},
                "nodes":graph.nodes
            }))?,
        )?;
        Ok(())
    }
}
