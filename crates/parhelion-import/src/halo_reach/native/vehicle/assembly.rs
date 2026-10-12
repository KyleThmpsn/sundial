//! Private component closure, typed connections, registrations and replication.
mod audio;
mod boost;
mod channel;
mod fire;
mod wheel;
use super::{ROOT, contact::Contacts, motion};
use crate::{
    presentation::{Graph, put},
    tiger::{
        entity::{compiled, links},
        payload::Payload,
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) use boost::Trails;
pub(super) use channel::Channel;

pub(super) struct Assembly {
    entity: Payload,
    links: links::Graph,
    pub payloads: BTreeMap<u32, Payload>,
    symbols: BTreeMap<u32, String>,
    allocations: BTreeMap<u32, String>,
    resources: BTreeMap<u32, BTreeMap<usize, String>>,
}

fn patches(graph: &Graph, symbol: &str) -> Result<Vec<Value>> {
    Ok(graph.node(symbol)?["patches"]
        .as_array()
        .context("Vehicle node patches")?
        .clone())
}

/// Restore only declared graph references for the checked assembly pass. These
/// temporary identities never leave the graph linker as package references.
fn restored(graph: &Graph, symbol: &str) -> Result<Payload> {
    let mut payload = graph.read(symbol)?;
    for patch in patches(graph, symbol)? {
        let offset = usize::try_from(patch["offset"].as_u64().context("Vehicle patch offset")?)?;
        let target = patch["symbol"].as_str().context("Vehicle patch target")?;
        let tag = u32::try_from(
            graph.node(target)?["template"]
                .as_u64()
                .context("Vehicle template")?,
        )?;
        ensure!(
            payload.u32(offset)? == u32::MAX,
            "Vehicle patch is not symbolic"
        );
        put(&mut payload.0, offset, &tag.to_le_bytes())?;
    }
    Ok(payload)
}

impl Assembly {
    pub fn read(native: &mut Reader, graph: &mut Graph) -> Result<Self> {
        let entity = restored(graph, "entity")?;
        ensure!(entity.u16(0x96)? == 15, "Prepared runtime is not a vehicle");
        let links = links::Graph::read(&entity, false)?;
        let mut payloads = BTreeMap::new();
        let mut symbols = BTreeMap::new();
        let entity_patches = patches(graph, "entity")?;
        for row in entity.array(16, 12, Some(0x80809c04))? {
            let tag = entity.u32(row)?;
            let prepared = entity_patches.iter().find(|patch| patch["offset"] == row);
            let (symbol, payload) = if let Some(patch) = prepared {
                let symbol = patch["symbol"]
                    .as_str()
                    .context("Vehicle component symbol")?;
                (symbol.to_owned(), restored(graph, symbol)?)
            } else {
                let symbol = format!("vehicle-component-{tag:08X}");
                let payload = (*native.tag(tag, Some(0x80809c36))?).clone();
                graph.add(&symbol, tag, &payload.0, None, Vec::new())?;
                (symbol, payload)
            };
            ensure!(
                payload.u32(payload.pointer(16)?)? == tag,
                "Vehicle component identity differs"
            );
            symbols.insert(tag, symbol);
            payloads.insert(tag, payload);
        }
        links.validate_owners(&payloads)?;
        Ok(Self {
            entity,
            links,
            payloads,
            symbols,
            allocations: BTreeMap::new(),
            resources: BTreeMap::new(),
        })
    }

    pub fn owner(&self, class: u32) -> Result<u32> {
        let owners = self
            .payloads
            .iter()
            .filter_map(|(&tag, payload)| {
                payload
                    .pointer(16)
                    .ok()
                    .and_then(|at| payload.u32(at - 4).ok())
                    .filter(|&found| found == class)
                    .map(|_| tag)
            })
            .collect::<Vec<_>>();
        ensure!(
            owners.len() == 1,
            "Vehicle component {class:08X} is absent or ambiguous"
        );
        Ok(owners[0])
    }

    pub fn resource(&mut self, owner: u32, offset: usize, symbol: String) -> Result<()> {
        self.payloads[&owner].u32(offset)?;
        ensure!(
            self.resources
                .entry(owner)
                .or_default()
                .insert(offset, symbol)
                .is_none(),
            "Vehicle resource was assigned twice"
        );
        Ok(())
    }

    pub fn allocation(
        &self,
        native: &mut Reader,
        graph: &Graph,
        owner: u32,
    ) -> Result<(u32, Payload)> {
        let tag = self.payloads[&owner].u32(0x44)?;
        let payload = if let Some(symbol) = self.allocations.get(&owner) {
            graph.read(symbol)?
        } else {
            (*native.tag(tag, Some(0x80809bbb))?).clone()
        };
        Ok((tag, payload))
    }

    pub fn set_allocation(
        &mut self,
        graph: &mut Graph,
        owner: u32,
        tag: u32,
        payload: &Payload,
    ) -> Result<()> {
        if let Some(symbol) = self.allocations.get(&owner) {
            graph.replace(symbol, &payload.0, Vec::new())?;
        } else {
            let symbol = format!("vehicle-allocation-{owner:08X}");
            graph.add(&symbol, tag, &payload.0, None, Vec::new())?;
            self.allocations.insert(owner, symbol);
        }
        Ok(())
    }

    /// Remove carrier lighting from the summoned assembly. A mixed effect flow
    /// remains intact, and a light supplying another component cannot be removed.
    pub fn remove_lights(&mut self, native: &mut Reader) -> Result<Value> {
        let mut removed = BTreeSet::new();
        let mut resources = Vec::new();
        for &owner in &self.links.components {
            let payload = &self.payloads[&owner];
            let definition = payload.pointer(24)?;
            let lights = match payload.u32(definition - 4)? {
                0x80807134 => vec![payload.u32(definition + 0xf8)?],
                0x808084e9 => {
                    let nodes = payload.array(definition + 0x168, 24, Some(0x808093e6))?;
                    let mut lights = Vec::new();
                    for row in &nodes {
                        let node = payload.pointer(row + 16)?;
                        if payload.u32(node - 4)? != 0x80806f49 {
                            break;
                        }
                        lights.push(payload.u32(node + 0x40)?);
                    }
                    if nodes.is_empty() || lights.len() != nodes.len() {
                        continue;
                    }
                    lights
                }
                _ => continue,
            };
            for &light in &lights {
                native.tag(light, Some(0x80807140))?;
            }
            removed.insert(owner);
            resources.push(json!({"owner":format!("{owner:08X}"),"lights":lights}));
        }
        for row in self
            .links
            .connections
            .iter()
            .chain(&self.links.named_connections)
        {
            if row
                .provider
                .object
                .is_some_and(|p| removed.contains(&p.owner))
            {
                ensure!(
                    row.consumer
                        .object
                        .is_some_and(|c| removed.contains(&c.owner)),
                    "Carrier light supplies a retained vehicle component"
                );
            }
        }
        let keep = |row: &links::Connection| {
            !row.consumer
                .object
                .is_some_and(|c| removed.contains(&c.owner))
        };
        self.links.connections.retain(keep);
        self.links.named_connections.retain(keep);
        self.links
            .components
            .retain(|owner| !removed.contains(owner));
        Ok(json!({"removed_components":resources,"scope":"Summoned private vehicle"}))
    }

    pub fn motion_priority(&self, hover: u32) -> Result<i16> {
        let maps = self.entity.array(88, 40, Some(0x80809c22))?;
        let descriptors = self.entity.array(104, 24, Some(0x80809c20))?;
        let bindings = self
            .entity
            .array(72, 8, Some(0x80809c25))?
            .into_iter()
            .filter(|&row| self.entity.u32(row).ok() == Some(0x95a60f29))
            .collect::<Vec<_>>();
        ensure!(bindings.len() == 1, "Vehicle activation namespace differs");
        let span = self.entity.u32(bindings[0] + 4)?;
        let first = (span & 0xffff) as usize;
        let count = ((span >> 16) & 0x7fff) as usize;
        ensure!(
            maps.len() == descriptors.len() && first + count <= maps.len(),
            "Vehicle activation span differs"
        );
        let mut donor = None;
        let mut priorities = Vec::new();
        for index in first..first + count {
            let map = maps[index];
            let priority = self.entity.u32(map + 8)? as i32;
            priorities.push(priority);
            if self.entity.u32(descriptors[index])? == hover {
                ensure!(
                    donor.is_none()
                        && self.entity.u32(descriptors[index] + 4)? == 0x80803cde
                        && self.entity.u16(map + 2)? == 0,
                    "Vehicle motion activation is ambiguous or conditional"
                );
                donor = Some(priority);
            }
        }
        let donor = donor.context("Vehicle motion activation is missing")?;
        let priority = donor
            .checked_add(1)
            .context("Vehicle activation priority overflow")?;
        // The original motion runs after carrier setup. Enter immediately before
        // that motion without overtaking any of its existing prerequisites.
        ensure!(
            priorities
                .iter()
                .all(|&value| value <= donor || value > priority),
            "Vehicle activation has no priority gap before its donor motion"
        );
        Ok(i16::try_from(priority)?)
    }

    pub fn contacts(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        owner: u32,
        contacts: &Contacts,
    ) -> Result<()> {
        let body = self
            .links
            .connections
            .iter()
            .filter(|row| {
                row.consumer
                    .object
                    .is_some_and(|object| object.owner == owner && object.class == 0x80809bd7)
            })
            .collect::<Vec<_>>();
        let prototype = (**body.first().context("Vehicle contact body provider")?).clone();
        ensure!(
            body.iter().all(|row| row.provider == prototype.provider
                && row.channel == prototype.channel
                && row.flags == prototype.flags),
            "Vehicle contacts use different body providers"
        );
        // Contact markers and their channel inputs belong to the replaced
        // simulation. Outbound scalar notifications still feed the retained
        // presentation bank, which dereferences every connected input.
        for row in self
            .links
            .connections
            .iter()
            .chain(&self.links.named_connections)
        {
            if let Some(consumer) = row.consumer.object.filter(|object| object.owner == owner) {
                ensure!(
                    matches!(consumer.class, 0x80809bd7 | 0x80809789),
                    "Vehicle contact replacement encountered another input contract"
                );
            }
            if let Some(provider) = row.provider.object.filter(|object| object.owner == owner) {
                ensure!(
                    provider.class == 0x80809ae1
                        && row
                            .consumer
                            .object
                            .is_none_or(|object| object.owner != owner),
                    "Vehicle contact replacement encountered another output contract"
                );
                let offset = usize::try_from(provider.offset)?;
                let metadata = native.tag(contacts.payload.u32(offset + 8)?, Some(0x80809c54))?;
                let interface =
                    links::Interface::read(&contacts.payload, provider, &metadata, false)?;
                let definition = interface.definition_offset;
                let instance = usize::try_from(contacts.payload.u64(definition + 8)?)?;
                ensure!(
                    contacts.payload.u32(definition)? == owner
                        && contacts.payload.u32(definition + 4)? == 0x80803d69
                        && contacts.payload.u32(instance)? == owner
                        && contacts.payload.u32(instance + 4)? == 0x80803d6a
                        && contacts.payload.u64(instance + 8)? == definition as u64
                        && instance + 0x40 <= contacts.payload.pointer(24)?
                        && interface.instance_class == 0x80809ae2
                        && interface.methods.len() == 1
                        && interface.methods[0].implementation_class == 0x80803d69
                        && interface.methods[0].index == 1,
                    "Retained vehicle scalar notification is not a loaded local-value provider"
                );
            }
        }
        let keep = |row: &links::Connection| {
            row.consumer
                .object
                .is_none_or(|object| object.owner != owner)
        };
        self.links.connections.retain(keep);
        self.links.named_connections.retain(keep);
        let ordinal = self
            .links
            .components
            .iter()
            .position(|&tag| tag == owner)
            .context("Physical component ordinal")?;
        for &offset in &contacts.markers {
            let mut row = prototype.clone();
            row.consumer.object = Some(links::Object {
                owner,
                class: 0x80809bd7,
                offset: offset as u64,
            });
            row.consumer.selector = ordinal as u64;
            self.links.connections.push(row);
        }
        self.payloads.insert(owner, contacts.payload.clone());
        let symbol = "vehicle-contact-allocation";
        // Read the template class even when no old allocation is otherwise needed.
        native.tag(contacts.allocation_template, Some(0x80809bbb))?;
        graph.add(
            symbol,
            contacts.allocation_template,
            &contacts.allocation.0,
            None,
            Vec::new(),
        )?;
        self.allocations.insert(owner, symbol.to_owned());
        Ok(())
    }

    pub fn add_motion(&mut self, payload: Payload) {
        self.links.components.push(motion::OWNER);
        self.payloads.insert(motion::OWNER, payload);
        self.symbols
            .insert(motion::OWNER, "vehicle-motion".to_owned());
        self.allocations
            .insert(motion::OWNER, "vehicle-motion-allocation".to_owned());
    }

    pub fn motion_inputs(
        &mut self,
        graph: &mut Graph,
        motion: &mut motion::Motion,
        channels: &[Channel],
    ) -> Result<()> {
        ensure!(
            motion.inputs.len() == channels.len(),
            "Vehicle control input count differs"
        );
        if channels.is_empty() {
            return Ok(());
        }
        let mut prepared = graph.read("vehicle-motion")?;
        let mut report = Vec::new();
        for (input, channel) in motion.inputs.iter().zip(channels) {
            for payload in [&mut motion.payload, &mut prepared] {
                put(
                    &mut payload.0,
                    input.offset + 32,
                    &channel.name.to_le_bytes(),
                )?;
            }
            self.links.connections.push(links::Connection {
                consumer: links::Endpoint {
                    namespace: 0x811c9dc5,
                    object: Some(links::Object {
                        owner: motion::OWNER,
                        class: 0x80809789,
                        offset: input.offset as u64,
                    }),
                    selector: self.links.components.len() as u64,
                },
                provider: channel.provider.clone(),
                channel: channel.index,
                flags: 0,
            });
            report.push(json!({"definition":input.offset,"unit_method":input.method,
                "bank_channel":channel.index,"name":format!("{:08X}",channel.name)}));
        }
        let declared = patches(graph, "vehicle-motion")?;
        graph.replace("vehicle-motion", &prepared.0, declared)?;
        motion.report["input_bindings"] = json!(report);
        Ok(())
    }

    pub fn surface_inputs(
        &mut self,
        owner: u32,
        contacts: &Contacts,
        channels: &[Channel],
    ) -> Result<()> {
        let ordinal = self
            .links
            .components
            .iter()
            .position(|&tag| tag == owner)
            .context("Flight surface owner")?;
        for &(offset, index) in &contacts.inputs {
            let channel = channels.get(index).context("Flight maneuver channel")?;
            ensure!(
                contacts.payload.u32(offset + 32)? == channel.name,
                "Flight surface channel name differs"
            );
            self.links.connections.push(links::Connection {
                consumer: links::Endpoint {
                    namespace: 0x811c9dc5,
                    object: Some(links::Object {
                        owner,
                        class: 0x80809789,
                        offset: offset as u64,
                    }),
                    selector: ordinal as u64,
                },
                provider: channel.provider.clone(),
                channel: channel.index,
                flags: 0,
            });
        }
        Ok(())
    }

    fn allocate(&mut self, native: &mut Reader, graph: &mut Graph) -> Result<()> {
        for (&owner, payload) in &self.payloads {
            if self.allocations.contains_key(&owner) {
                continue;
            }
            let tag = payload.u32(0x44)?;
            let symbol = format!("vehicle-allocation-{owner:08X}");
            let allocation = native.tag(tag, Some(0x80809bbb))?;
            graph.add(&symbol, tag, &allocation.0, None, Vec::new())?;
            self.allocations.insert(owner, symbol);
        }
        Ok(())
    }

    fn write_owners(&self, graph: &mut Graph) -> Result<()> {
        for (&owner, payload) in &self.payloads {
            if owner == motion::OWNER {
                continue;
            }
            let symbol = &self.symbols[&owner];
            let mut declared = patches(graph, symbol)?
                .into_iter()
                .map(|patch| {
                    Ok((
                        usize::try_from(
                            patch["offset"].as_u64().context("Component patch offset")?,
                        )?,
                        patch["symbol"]
                            .as_str()
                            .context("Component patch target")?
                            .to_owned(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            declared.insert(0x44, self.allocations[&owner].clone());
            if let Some(resources) = self.resources.get(&owner) {
                declared.extend(
                    resources
                        .iter()
                        .map(|(&offset, symbol)| (offset, symbol.clone())),
                );
            }
            // Component identities are resource triples. A matching scalar is
            // not a tag and must not be retargeted as one.
            for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
                let tag = payload.u32(at)?;
                let Some(target) = self.symbols.get(&tag) else {
                    continue;
                };
                let class = payload.u32(at + 4)?;
                let reference = payload.u64(at + 8)?;
                if class & 0xffff0000 != 0x80800000
                    && !crate::tiger::entity::root_reference(
                        &self.payloads[&tag],
                        class,
                        reference,
                    )?
                {
                    continue;
                }
                let offset = usize::try_from(reference)?;
                ensure!(
                    offset
                        .checked_add(16)
                        .is_some_and(|end| end <= self.payloads[&tag].0.len()),
                    "Vehicle component resource exceeds its owner"
                );
                declared.insert(at, target.clone());
            }
            let mut bytes = payload.0.clone();
            let mut output = Vec::new();
            for (offset, target) in declared {
                put(&mut bytes, offset, &u32::MAX.to_le_bytes())?;
                output.push(json!({"offset":offset,"symbol":target}));
            }
            graph.replace(symbol, &bytes, output)?;
        }
        Ok(())
    }

    fn presentation_template(&self, hover: u32, retain_update: bool) -> Result<Payload> {
        let mut template = self.entity.clone();
        let maps = self.entity.array(88, 40, Some(0x80809c22))?;
        let descriptors = self.entity.array(104, 24, Some(0x80809c20))?;
        for binding in self.entity.array(72, 8, Some(0x80809c25))? {
            if retain_update && self.entity.u32(binding)? == 0xab076dc7 {
                continue;
            }
            if ![0xab076dc7, 0x18dba5e9, 0xecc9dfcb].contains(&self.entity.u32(binding)?) {
                continue;
            }
            let span = self.entity.u32(binding + 4)?;
            let first = (span & 0xffff) as usize;
            let count = ((span >> 16) & 0x7fff) as usize;
            ensure!(
                first + count <= maps.len() && maps.len() == descriptors.len(),
                "Vehicle callback span"
            );
            for &descriptor in &descriptors[first..first + count] {
                if self.entity.u32(descriptor)? == hover {
                    put(&mut template.0, descriptor, &u32::MAX.to_le_bytes())?;
                }
            }
        }
        Ok(template)
    }

    pub fn emit(
        mut self,
        native: &mut Reader,
        graph: &mut Graph,
        runtime: u32,
        hover: u32,
        motion: Option<&motion::Motion>,
    ) -> Result<()> {
        self.allocate(native, graph)?;
        self.write_owners(graph)?;
        let template_entity = match motion {
            Some(motion) => self.presentation_template(hover, motion.retain_hover_update)?,
            None => self.entity.clone(),
        };
        // Relocated rigs already changed entity offsets. Use that same checked
        // layout as each registration template, with its reciprocal definition.
        let templates = self
            .payloads
            .iter()
            .map(|(&owner, payload)| {
                let mut template = payload.clone();
                let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
                put(&mut template.0, 24, &(definition as i64 - 24).to_le_bytes())?;
                Ok((owner, template))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let components = self
            .links
            .components
            .iter()
            .map(|owner| compiled::Component {
                owner: *owner,
                payload: &self.payloads[owner],
                template: &templates[owner],
                entity: &template_entity,
            })
            .collect::<Vec<_>>();
        let connections = self.links.native_rows(
            &self.links.components,
            &self.payloads,
            Ok,
            |_, channel| Ok(channel),
            |_, selector| Ok(selector),
        )?;
        let providers = motion
            .map(|motion| BTreeMap::from([(motion::OWNER, motion.providers())]))
            .unwrap_or_default();
        let mut entity =
            compiled::emit_authored(&self.entity, &components, &connections, &providers)?;
        let mut replication = compiled::replication::emit(runtime, &components)?;
        let mut entity_patches = Vec::new();
        for (field, stride, class, offsets) in [
            (16, 12, 0x80809c04, &[0][..]),
            (32, 72, 0x80809bc9, &[8, 40][..]),
            (48, 72, 0x80809bc9, &[8, 40][..]),
            (88, 40, 0x80809c22, &[16][..]),
            (104, 24, 0x80809c20, &[0][..]),
        ] {
            for row in entity.array(field, stride, Some(class))? {
                for &offset in offsets {
                    let at = row + offset;
                    if let Some(symbol) = self.symbols.get(&entity.u32(at)?) {
                        entity_patches.push(json!({"offset":at,"symbol":symbol}));
                    }
                }
            }
        }
        let motion_patches = if motion.is_some() {
            patches(graph, "vehicle-motion")?
        } else {
            Vec::new()
        };
        for row in entity.array(88, 40, Some(0x80809c22))? {
            if entity.u32(row + 16)? == motion::OWNER {
                let provider = entity.u64(row + 24)?;
                let target = motion_patches
                    .iter()
                    .find(|patch| patch["offset"] == provider + 8)
                    .context("Authored motion interface symbol")?;
                entity_patches.push(json!({"offset":row+32,"symbol":target["symbol"]}));
            }
        }
        let replication_symbol = "vehicle-replication";
        entity_patches.push(json!({"offset":0x88,"symbol":replication_symbol}));
        for patch in &entity_patches {
            put(
                &mut entity.0,
                patch["offset"].as_u64().unwrap() as usize,
                &u32::MAX.to_le_bytes(),
            )?;
        }
        graph.add(ROOT, runtime, &entity.0, None, entity_patches)?;
        let mut replication_patches = vec![json!({"offset":8,"symbol":ROOT})];
        for (&owner, row) in
            self.links
                .components
                .iter()
                .zip(replication.array(0x30, 32, Some(0x80809bb8))?)
        {
            replication_patches.push(json!({"offset":row+4,"symbol":self.allocations[&owner]}));
        }
        for patch in &replication_patches {
            put(
                &mut replication.0,
                patch["offset"].as_u64().unwrap() as usize,
                &u32::MAX.to_le_bytes(),
            )?;
        }
        let template = self.entity.u32(0x88)?;
        native.tag(template, Some(0x80809bb6))?;
        graph.add(
            replication_symbol,
            template,
            &replication.0,
            None,
            replication_patches,
        )
    }
}
