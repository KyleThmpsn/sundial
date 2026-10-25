//! Mode-specific sequence inputs retain their actual component occurrences and reciprocal pairs.
use crate::{
    d1_roi::{Source, connections, sequence::input::Input},
    tiger::{entity::links, payload::Payload},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
struct Binding {
    mode: usize,
    selector: usize,
    input: Input,
    connection: connections::Connection,
    native_runtime: usize,
    native_definition: usize,
}

#[derive(Debug, Serialize)]
pub struct Sequences {
    source_sha256: String,
    providers: BTreeMap<u32, Provider>,
    components: Vec<u32>,
    occurrence: usize,
    bindings: Vec<Binding>,
}

#[derive(Debug, Serialize)]
struct Provider {
    owner_sha256: String,
    metadata: u32,
    metadata_sha256: String,
    animations: Vec<crate::d1_roi::sequence::animation::Animation>,
}

pub struct Written {
    /// Consumer and provider mappings for the complete occurrence-aware graph linker.
    pub endpoints: BTreeMap<connections::Endpoint, links::Endpoint>,
    pub patches: Vec<(usize, String)>,
    /// The nine ordinary connection rows, to join with all other translated graph rows.
    pub connections: links::NativeRows,
}

impl Sequences {
    pub fn actions(&self) -> std::collections::BTreeSet<(u32, bool, bool)> {
        self.providers
            .values()
            .flat_map(|provider| provider.animations.iter())
            .map(|animation| (animation.action(), animation.owner_mode(), animation.lane()))
            .collect()
    }

    pub fn read(
        source: &Source,
        owner: &Payload,
        graph: &connections::Graph,
        occurrence: usize,
    ) -> Result<Self> {
        let runtime = owner.pointer(16)?;
        let definition = owner.pointer(24)?;
        ensure!(
            runtime >= 4
                && definition >= 4
                && owner.u32(runtime - 4)? == 0x808025B8
                && owner.u32(definition - 4)? == 0x80802A4A
                && owner.u64(runtime + 8)? == definition as u64
                && owner.u64(definition + 8)? == runtime as u64
                && owner.u64(runtime + 16)? == 0x80802A4A
                && owner.u64(definition + 16)? == 0x808025B8
                && owner.u64(runtime)? == owner.u64(definition)?
                && graph.components.get(occurrence) == Some(&owner.u32(runtime)?),
            "Source Radiance sequence input owner differs"
        );
        owner.bytes::<0x1170>(runtime)?;
        owner.bytes::<0xCA0>(definition)?;
        let mut bindings = Vec::new();
        let mut providers = BTreeMap::new();
        // Source 110A580/610/6A0 and native F6BFC0/B9E0/BCD0 choose these runtime
        // triples. The definition order is established separately by reciprocal pairs.
        for (mode, (si, sd, ni, nd)) in [
            (0x4F0, 0x1A8, 0x550, 0x208),
            (0xC00, 0x608, 0xAE0, 0x6C0),
            (0xCF0, 0x560, 0xBD0, 0x630),
        ]
        .into_iter()
        .enumerate()
        {
            for selector in 0..3 {
                let input = Input::read(owner, runtime + si + selector * 0x50, runtime)?;
                ensure!(
                    input.definition() == definition + sd + selector * 0x38,
                    "Source Radiance helper definition order differs"
                );
                ensure!(
                    graph
                        .named_connections
                        .iter()
                        .all(|row| row.consumer.owner != input.owner()
                            || row.consumer.offset != input.definition() as u64
                            || row.consumer.selector != occurrence as u64),
                    "Source Radiance helper has an unqualified named connection"
                );
                let matches = graph
                    .connections
                    .iter()
                    .filter(|row| {
                        row.consumer.owner == input.owner()
                            && row.consumer.offset == input.definition() as u64
                            && row.consumer.selector == occurrence as u64
                    })
                    .collect::<Vec<_>>();
                let [connection] = matches.as_slice() else {
                    anyhow::bail!("Source Radiance sequence input has no unique connection");
                };
                ensure!(
                    connection.consumer.namespace == 0x811C9DC5
                        && connection.consumer.class == 0x80800456
                        && connection.provider.namespace == 0x811C9DC5
                        && connection.provider.class == 0x80800988
                        && connection.provider.selector < 0xFFFF
                        && graph.components.get(connection.provider.selector as usize)
                            == Some(&connection.provider.owner)
                        && connection.channel == 0,
                    "Source Radiance sequence connection has another dispatch contract"
                );
                providers.insert(
                    connection.provider.owner,
                    provider(source, connection.provider)?,
                );
                bindings.push(Binding {
                    mode,
                    selector,
                    input,
                    connection: (**connection).clone(),
                    native_runtime: ni + selector * 0x50,
                    native_definition: nd + selector * 0x30,
                });
            }
        }
        Ok(Self {
            source_sha256: hex::encode(Sha256::digest(&owner.0)),
            providers,
            components: graph.components.clone(),
            occurrence,
            bindings,
        })
    }

    pub fn connections(&self) -> impl Iterator<Item = &connections::Connection> {
        self.bindings.iter().map(|binding| &binding.connection)
    }

    /// Construct and connect all nine consumers atomically. Every provider must already have
    /// a native owner and checked dispatch metadata. The containing graph must retain its other
    /// connections and finish every controller field and event before sealing or publication.
    pub fn write(
        &self,
        target: &mut Payload,
        symbol: &str,
        occurrences: &[u32],
        owners: &BTreeMap<u32, Payload>,
        metadata: &BTreeMap<u32, Payload>,
    ) -> Result<Written> {
        let runtime = target.pointer(16)?;
        let definition = usize::try_from(target.u64(runtime + 8)?)?;
        let boundary = target.pointer(24)?;
        let owner = target.u32(runtime)?;
        ensure!(
            runtime >= 4
                && definition >= 4
                && runtime + 0xF40 <= definition
                && definition == boundary
                && target.u64(0)? == target.0.len() as u64
                && target.u32(runtime - 4)? == 0x80804176
                && target.u32(definition - 4)? == 0x80804177
                && target.u32(runtime + 4)? == 0x80804177
                && target.u32(definition + 4)? == 0x80804176
                && target.u32(definition)? == owner
                && target.u64(definition + 8)? == runtime as u64,
            "Native retained controller pair differs"
        );
        ensure!(
            occurrences.get(self.occurrence) == Some(&owner),
            "Native Radiance controller occurrence differs"
        );
        target.bytes::<0xB30>(definition)?;
        let mut output = target.clone();
        let mut endpoints = BTreeMap::new();
        let mut patches = Vec::new();
        for binding in &self.bindings {
            let written = binding.input.write(
                &mut output,
                runtime + binding.native_runtime,
                definition + binding.native_definition,
                runtime,
                symbol,
            )?;
            patches.extend(written.patches);
            ensure!(
                endpoints
                    .insert(
                        binding.connection.consumer,
                        links::Endpoint {
                            namespace: 0x811C9DC5,
                            object: Some(links::Object {
                                owner,
                                class: 0x808092F4,
                                offset: written.definition as u64,
                            }),
                            selector: binding.connection.consumer.selector,
                        }
                    )
                    .is_none(),
                "Radiance sequence consumers collide"
            );
        }
        for binding in &self.bindings {
            let original = binding.connection.provider;
            let tag = *occurrences
                .get(original.selector as usize)
                .context("Native sequence provider occurrence is absent")?;
            let payload = owners
                .get(&tag)
                .context("Native sequence provider was not emitted")?;
            let instance = payload.pointer(16)?;
            ensure!(
                crate::tiger::entity::lifecycle::read(payload)?
                    == vec![crate::tiger::entity::lifecycle::Method {
                        target: instance,
                        implementation: 0x808084D7,
                        constructor: 0,
                        cleanup: 2,
                    }],
                "Native sequence provider initialization or cleanup differs"
            );
            let settings = usize::try_from(payload.u64(instance + 8)?)?;
            ensure!(
                instance >= 4
                    && settings >= 4
                    && payload.u32(instance - 4)? == 0x808084D7
                    && payload.u32(settings - 4)? == 0x808084E9
                    && payload.u32(instance)? == tag
                    && payload.u32(settings)? == tag
                    && payload.u32(instance + 4)? == 0x808084E9
                    && payload.u32(settings + 4)? == 0x808084D7
                    && payload.u64(settings + 8)? == instance as u64,
                "Native sequence provider pair differs"
            );
            let object = links::Object {
                owner: tag,
                class: 0x808092F8,
                offset: (settings + 0x90) as u64,
            };
            let contract = links::Interface::read(
                payload,
                object,
                metadata
                    .get(&payload.u32(settings + 0x98)?)
                    .context("Native sequence dispatch metadata was not supplied")?,
                false,
            )?;
            ensure!(
                contract.definition_offset == settings
                    && contract.instance_class == 0x808092F7
                    && contract.methods.len() == 1
                    && contract.methods[0].implementation_class == 0x808084D7
                    && contract.methods[0].index == 3
                    && contract.methods[0].arguments == [0, 0]
                    && payload.u64(settings + 0xA0)? == 0,
                "Native sequence provider dispatch differs"
            );
            let endpoint = links::Endpoint {
                namespace: 0x811C9DC5,
                object: Some(object),
                selector: original.selector,
            };
            if let Some(previous) = endpoints.insert(original, endpoint.clone()) {
                ensure!(
                    previous == endpoint,
                    "Native sequence provider mappings conflict"
                );
            }
        }
        let subset = connections::Graph {
            components: self.components.clone(),
            connections: self.connections().cloned().collect(),
            named_connections: Vec::new(),
        };
        let mut native_owners = owners.clone();
        native_owners.insert(owner, output.clone());
        let connections = subset.native_rows(
            occurrences,
            &native_owners,
            |endpoint| {
                endpoints
                    .get(&endpoint)
                    .cloned()
                    .context("Missing sequence endpoint")
            },
            |_, channel| {
                ensure!(channel == 0, "Sequence provider channel differs");
                Ok(0)
            },
        )?;
        *target = output;
        Ok(Written {
            endpoints,
            patches,
            connections,
        })
    }
}

fn provider(source: &Source, endpoint: connections::Endpoint) -> Result<Provider> {
    ensure!(
        source.reference(endpoint.owner)? == 0x80800861,
        "Source sequence provider is not a component owner"
    );
    let owner = source.read(endpoint.owner)?;
    let runtime = owner.pointer(16)?;
    let definition = owner.pointer(24)?;
    let at = usize::try_from(endpoint.offset)?;
    ensure!(
        runtime >= 4
            && definition >= 4
            && owner.u64(0)? == owner.0.len() as u64
            && owner.u32(runtime - 4)? == 0x8080079A
            && owner.u32(definition - 4)? == 0x80800610
            && owner.u64(runtime)? == u64::from(endpoint.owner)
            && owner.u64(definition)? == u64::from(endpoint.owner)
            && owner.u64(runtime + 8)? == definition as u64
            && owner.u64(definition + 8)? == runtime as u64
            && owner.u64(runtime + 16)? == 0x80800610
            && owner.u64(definition + 16)? == 0x8080079A
            && at == definition + 0xA8
            && owner.pointer(at)? == definition
            && owner.u32(at + 12)? == 0
            && owner.u64(at + 16)? == 0,
        "Source sequence provider has another owner or selector"
    );
    let tag = owner.u32(at + 8)?;
    ensure!(
        source.reference(tag)? == 0x8080058A,
        "Source sequence provider metadata class differs"
    );
    let metadata = source.read(tag)?;
    ensure!(
        metadata.u64(0)? == metadata.0.len() as u64,
        "Source sequence metadata size differs"
    );
    let methods = metadata.array(16, 24, Some(0x808005C0))?;
    let [method] = methods.as_slice() else {
        anyhow::bail!("Source sequence provider method count differs");
    };
    ensure!(
        metadata.u32(8)? == 0x80800988
            && metadata.u32(12)? == 0x808003E3
            && metadata.u32(*method)? == 0x8080079A
            && metadata.u32(*method + 4)? == 4
            && metadata.bytes::<16>(*method + 8)? == [0; 16],
        "Source sequence provider dispatch differs"
    );
    let mut animations = Vec::new();
    for row in owner.array(definition + 0x180, 8, Some(0x80800629))? {
        let event = owner.pointer(row)?;
        if owner.u32(
            event
                .checked_sub(4)
                .context("Source sequence event marker")?,
        )? == 0x808025C1
        {
            animations.push(crate::d1_roi::sequence::animation::Animation::read(
                &owner, event,
            )?);
        }
    }
    Ok(Provider {
        owner_sha256: hex::encode(Sha256::digest(&owner.0)),
        metadata: tag,
        metadata_sha256: hex::encode(Sha256::digest(&metadata.0)),
        animations,
    })
}
