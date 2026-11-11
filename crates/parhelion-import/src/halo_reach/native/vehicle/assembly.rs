//! Private component closure, typed connections, registrations and replication.
mod audio;
mod boost;
mod channel;
mod fire;
mod rotor;
mod wheel;
use super::{
    contact::{self, Contacts},
    motion,
};
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
pub(in crate::halo_reach) use fire::launch;

pub(super) struct Assembly {
    /// Symbol prefix of every node this assembly adds, so a vehicle and an entity it carries
    /// can each privately copy a shared stock component.
    prefix: String,
    entity: Payload,
    links: links::Graph,
    pub payloads: BTreeMap<u32, Payload>,
    symbols: BTreeMap<u32, String>,
    allocations: BTreeMap<u32, String>,
    resources: BTreeMap<u32, BTreeMap<usize, String>>,
    runtime_ends: BTreeMap<u32, usize>,
    registration_entities: BTreeMap<u32, Payload>,
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
        let prepared = patches(graph, "entity")?;
        Self::assemble(native, graph, entity, &prepared, "vehicle".to_owned())
    }

    /// A private copy of a stock entity graph of `object_type`, under `prefix`. Each copy a
    /// vehicle carries takes its own prefix, so several copies of one stock graph coexist.
    pub fn stock(
        native: &mut Reader,
        graph: &mut Graph,
        entity: u32,
        object_type: u16,
        prefix: String,
    ) -> Result<Self> {
        let entity = (*native.tag(entity, Some(0x80809c0f))?).clone();
        ensure!(
            entity.u16(0x96)? == object_type,
            "Stock entity object type differs from {object_type}"
        );
        Self::assemble(native, graph, entity, &[], prefix)
    }

    /// `entity_patches` name the components an earlier conversion already prepared.
    fn assemble(
        native: &mut Reader,
        graph: &mut Graph,
        entity: Payload,
        entity_patches: &[Value],
        prefix: String,
    ) -> Result<Self> {
        let links = links::Graph::read(&entity, false)?;
        let mut payloads = BTreeMap::new();
        let mut symbols = BTreeMap::new();
        for row in entity.array(16, 12, Some(0x80809c04))? {
            let tag = entity.u32(row)?;
            let prepared = entity_patches.iter().find(|patch| patch["offset"] == row);
            let (symbol, payload) = if let Some(patch) = prepared {
                let symbol = patch["symbol"]
                    .as_str()
                    .context("Vehicle component symbol")?;
                (symbol.to_owned(), restored(graph, symbol)?)
            } else {
                let symbol = format!("{prefix}-component-{tag:08X}");
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
            prefix,
            entity,
            links,
            payloads,
            symbols,
            allocations: BTreeMap::new(),
            resources: BTreeMap::new(),
            runtime_ends: BTreeMap::new(),
            registration_entities: BTreeMap::new(),
        })
    }

    /// The symbol prefix of every node this assembly adds.
    pub fn prefix(&self) -> &str {
        &self.prefix
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

    /// Retain appended mutable records in the loader's copied instance span.
    /// Sealing waits until every resource reference has been declared.
    pub fn runtime_end(&mut self, owner: u32, end: usize) -> Result<()> {
        ensure!(
            end <= self.payloads[&owner].0.len(),
            "Vehicle runtime span exceeds its owner"
        );
        self.runtime_ends
            .entry(owner)
            .and_modify(|value| *value = (*value).max(end))
            .or_insert(end);
        Ok(())
    }

    fn seal_owners(&mut self, graph: &Graph) -> Result<()> {
        for (&owner, &end) in &self.runtime_ends {
            let mut declared = patches(graph, &self.symbols[&owner])?;
            if let Some(resources) = self.resources.get(&owner) {
                for (&offset, symbol) in resources {
                    declared.retain(|patch| patch["offset"] != offset);
                    declared.push(json!({"offset":offset,"symbol":symbol}));
                }
            }
            crate::tiger::instance::seal_span(
                self.payloads.get_mut(&owner).unwrap(),
                &mut declared,
                end,
            )?;
            let resources = self.resources.entry(owner).or_default();
            for patch in declared {
                let offset = usize::try_from(
                    patch["offset"]
                        .as_u64()
                        .context("Sealed vehicle patch offset")?,
                )?;
                let symbol = patch["symbol"]
                    .as_str()
                    .context("Sealed vehicle patch symbol")?;
                resources.insert(offset, symbol.to_owned());
            }
        }
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
            let symbol = format!("{}-allocation-{owner:08X}", self.prefix);
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

    /// Disarm the driver: disconnect the unit's slots that reach the carrier's weapons, its vehicle
    /// weapon component and every component that names a projectile graph (object type 18). Stock
    /// unarmed vehicles leave those unit slots disconnected. The weapon components stay, because
    /// the vehicle's channel bank reads several of its declared inputs from the weapon controller,
    /// and an input left without its provider faults the bank when the vehicle spawns.
    pub fn disarm(&mut self, native: &mut Reader) -> Result<Value> {
        let mut weapons = BTreeMap::new();
        for &owner in &self.links.components {
            let payload = &self.payloads[&owner];
            let class = payload.u32(payload.pointer(16)? - 4)?;
            let mut projectiles = BTreeSet::new();
            for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
                let word = payload.u32(at)?;
                if !(0x8080_0000..0x8200_0000).contains(&word) || projectiles.contains(&word) {
                    continue;
                }
                if native
                    .tag(word, Some(0x80809c0f))
                    .is_ok_and(|entity| entity.u16(0x96).ok() == Some(18))
                {
                    projectiles.insert(word);
                }
            }
            if class == 0x80809425 || !projectiles.is_empty() {
                weapons.insert(owner, (class, projectiles));
            }
        }
        ensure!(
            weapons.values().any(|(class, _)| *class == 0x80809425),
            "Carrier has no vehicle weapon component"
        );
        let unit = self.owner(0x80803c23)?;
        let mut disconnected = Vec::new();
        for row in self
            .links
            .connections
            .iter_mut()
            .chain(self.links.named_connections.iter_mut())
        {
            let (Some(consumer), Some(provider)) = (row.consumer.object, row.provider.object)
            else {
                continue;
            };
            if consumer.owner == unit && weapons.contains_key(&provider.owner) {
                disconnected.push(json!({
                    "unit_slot":consumer.offset,"provider":format!("{:08X}", provider.owner),
                    "interface":format!("{:08X}", provider.class)}));
                row.provider.object = None;
                row.provider.selector = 0xFFFF;
            }
        }
        ensure!(
            !disconnected.is_empty(),
            "Carrier unit has no weapon slot to disconnect"
        );
        Ok(json!({
            "weapons":weapons.iter().map(|(owner, (class, projectiles))| json!({
                "owner":format!("{owner:08X}"),"class":format!("{class:08X}"),
                "projectiles":projectiles.iter().map(|tag| format!("{tag:08X}")).collect::<Vec<_>>()}))
                .collect::<Vec<_>>(),
            "disconnected_unit_slots":disconnected,
            "reason":"The source vehicle has no weapon barrel with a projectile"
        }))
    }

    /// Connect a new consumer of `owner` at `offset` to the provider that the owner's existing
    /// consumer of the same class uses. The stock tank's entity connects its turret row's marker
    /// input and its unit's own marker input to one skeleton provider this way. An unbound marker
    /// input leaves the row's forwarded reference `-1`, and the turret spawn (A1F640) resolves it
    /// without a check.
    pub fn connect_like(&mut self, owner: u32, class: u32, offset: u64) -> Result<Value> {
        let template = self
            .links
            .connections
            .iter()
            .find(|row| {
                row.consumer
                    .object
                    .is_some_and(|object| object.owner == owner && object.class == class)
                    && row.provider.object.is_some()
            })
            .cloned()
            .with_context(|| format!("{owner:08X} has no connected {class:08X} consumer"))?;
        ensure!(
            !self.links.connections.iter().any(|row| row
                .consumer
                .object
                .is_some_and(|object| object.owner == owner && object.offset == offset)),
            "{owner:08X} already has a consumer at {offset:#x}"
        );
        let provider = template.provider.object.context("Template provider")?;
        let mut row = template;
        row.consumer.object = Some(links::Object {
            owner,
            class,
            offset,
        });
        self.links.connections.push(row);
        Ok(json!({"consumer":offset,"class":format!("{class:08X}"),
            "provider":format!("{:08X}", provider.owner),"interface":format!("{:08X}", provider.class),
            "provider_offset":provider.offset}))
    }

    /// Leave the consumers of `owner` at `offsets` without a provider, as a stock unarmed unit
    /// leaves its weapon slots. Every offset must name an existing consumer.
    pub fn disconnect(&mut self, owner: u32, offsets: &[u64]) -> Result<Value> {
        let mut report = Vec::new();
        for &offset in offsets {
            let mut found = false;
            for row in self
                .links
                .connections
                .iter_mut()
                .chain(self.links.named_connections.iter_mut())
            {
                if row
                    .consumer
                    .object
                    .is_some_and(|object| object.owner == owner && object.offset == offset)
                {
                    if let Some(provider) = row.provider.object.take() {
                        report.push(
                            json!({"consumer":offset,"provider":format!("{:08X}", provider.owner),
                            "interface":format!("{:08X}", provider.class)}),
                        );
                    }
                    row.provider.selector = 0xFFFF;
                    found = true;
                }
            }
            ensure!(found, "{owner:08X} has no consumer at {offset:#x}");
        }
        Ok(json!(report))
    }

    /// Remove the connections whose consumer is `owner` at `offset`, for a consumer whose record
    /// the owner no longer reaches. Returns how many were removed.
    pub fn remove_consumer(&mut self, owner: u32, offset: u64) -> usize {
        let before = self.links.connections.len() + self.links.named_connections.len();
        let keep = |row: &links::Connection| {
            !row.consumer
                .object
                .is_some_and(|object| object.owner == owner && object.offset == offset)
        };
        self.links.connections.retain(keep);
        self.links.named_connections.retain(keep);
        before - self.links.connections.len() - self.links.named_connections.len()
    }

    /// Remove a component that only consumes. Its inputs go with it, and a component that
    /// supplies another one cannot be removed.
    pub fn remove_component(&mut self, owner: u32) -> Result<()> {
        ensure!(
            self.links.components.contains(&owner),
            "{owner:08X} is not a component of this assembly"
        );
        for row in self
            .links
            .connections
            .iter()
            .chain(&self.links.named_connections)
        {
            ensure!(
                row.provider.object.is_none_or(|p| p.owner != owner)
                    || row.consumer.object.is_some_and(|c| c.owner == owner),
                "{owner:08X} supplies another component"
            );
        }
        let keep = |row: &links::Connection| row.consumer.object.is_none_or(|c| c.owner != owner);
        self.links.connections.retain(keep);
        self.links.named_connections.retain(keep);
        self.links.components.retain(|&tag| tag != owner);
        Ok(())
    }

    /// Records of `owner` moved from `from` to `to`, `length` bytes each: their consumers and
    /// declared resources follow them.
    pub fn relocate(&mut self, owner: u32, from: usize, to: usize, length: usize) -> Result<()> {
        let range = from as u64..(from + length) as u64;
        for row in self
            .links
            .connections
            .iter_mut()
            .chain(self.links.named_connections.iter_mut())
        {
            if let Some(object) = &mut row.consumer.object
                && object.owner == owner
                && range.contains(&object.offset)
            {
                object.offset = object.offset - from as u64 + to as u64;
            }
        }
        if let Some(resources) = self.resources.get_mut(&owner) {
            let moved = resources
                .range(from..from + length)
                .map(|(&offset, symbol)| (offset, symbol.clone()))
                .collect::<Vec<_>>();
            for (offset, symbol) in moved {
                resources.remove(&offset);
                ensure!(
                    resources.insert(offset - from + to, symbol).is_none(),
                    "Relocated resource overlaps another"
                );
            }
        }
        Ok(())
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

    /// Add a second copy of component `source` under the stand-in owner `key`, as stock transports
    /// bind several owners of one component kind. The copy consumes what `source` consumes and
    /// registers where `source` does, through the entity with `source` renamed `key`. It supplies
    /// no other component. The graph linker gives `symbol` its own identity, so the stand-in never
    /// reaches a package. Returns the copy's payload for its own edits.
    pub fn duplicate(
        &mut self,
        graph: &mut Graph,
        source: u32,
        key: u32,
        symbol: String,
    ) -> Result<&mut Payload> {
        ensure!(
            self.links.components.contains(&source) && !self.payloads.contains_key(&key),
            "Vehicle component copy {key:08X} of {source:08X} is not distinct"
        );
        let mut payload = self.payloads[&source].clone();
        for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
            if payload.u32(at)? == source
                && payload.u32(at + 4)? & 0xffff_0000 == 0x8080_0000
                && payload.u64(at + 8)? < payload.0.len() as u64
            {
                put(&mut payload.0, at, &key.to_le_bytes())?;
            }
        }
        let instance = payload.pointer(16)?;
        ensure!(
            payload.u32(instance)? == key
                && payload.u32(usize::try_from(payload.u64(instance + 8)?)?)? == key,
            "Vehicle component copy keeps no reciprocal identity"
        );
        let mut registration = self.entity.clone();
        for (field, stride, class, offset) in [
            (16, 12, 0x80809c04, 0),
            (88, 40, 0x80809c22, 16),
            (104, 24, 0x80809c20, 0),
        ] {
            for row in self.entity.array(field, stride, Some(class))? {
                if self.entity.u32(row + offset)? == source {
                    put(&mut registration.0, row + offset, &key.to_le_bytes())?;
                }
            }
        }
        let consumed = |row: &links::Connection| {
            row.consumer
                .object
                .is_some_and(|object| object.owner == source)
        };
        let rename = |mut row: links::Connection| {
            for endpoint in [&mut row.consumer, &mut row.provider] {
                if let Some(object) = &mut endpoint.object
                    && object.owner == source
                {
                    object.owner = key;
                }
            }
            row
        };
        let connections = self
            .links
            .connections
            .iter()
            .filter(|row| consumed(row))
            .cloned()
            .map(rename)
            .collect::<Vec<_>>();
        let named = self
            .links
            .named_connections
            .iter()
            .filter(|row| consumed(row))
            .cloned()
            .map(rename)
            .collect::<Vec<_>>();
        self.links.connections.extend(connections);
        self.links.named_connections.extend(named);
        graph.add(&symbol, source, &payload.0, None, Vec::new())?;
        if let Some(&end) = self.runtime_ends.get(&source) {
            self.runtime_ends.insert(key, end);
        }
        self.links.components.push(key);
        self.symbols.insert(key, symbol);
        self.registration_entities.insert(key, registration);
        self.payloads.insert(key, payload);
        Ok(self.payloads.get_mut(&key).unwrap())
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
            let symbol = format!("{}-allocation-{owner:08X}", self.prefix);
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
        self.seal_owners(graph)?;
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
                entity: self
                    .registration_entities
                    .get(owner)
                    .unwrap_or(&template_entity),
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
        let root = format!("{}-runtime", self.prefix);
        let replication_symbol = format!("{}-replication", self.prefix);
        entity_patches.push(json!({"offset":0x88,"symbol":replication_symbol}));
        for patch in &entity_patches {
            put(
                &mut entity.0,
                patch["offset"].as_u64().unwrap() as usize,
                &u32::MAX.to_le_bytes(),
            )?;
        }
        graph.add(&root, runtime, &entity.0, None, entity_patches)?;
        let mut replication_patches = vec![json!({"offset":8,"symbol":root})];
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
            &replication_symbol,
            template,
            &replication.0,
            None,
            replication_patches,
        )
    }
}
