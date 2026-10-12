//! Private secondary entities retain their complete component and allocation closure.
use crate::{
    presentation::{Graph, put},
    tiger::{
        entity::{self, compiled, links},
        payload::Payload,
        reader::Reader,
    },
};
use anyhow::{Result, ensure};
use serde_json::json;
use std::collections::BTreeMap;

pub(super) struct Bundle {
    tag: u32,
    entity: Payload,
    pub owners: BTreeMap<u32, Payload>,
    pub allocations: BTreeMap<u32, (u32, Payload)>,
    pub resources: BTreeMap<u32, BTreeMap<usize, String>>,
    added: Vec<(u32, Payload)>,
}

impl Bundle {
    pub fn read(native: &mut Reader, tag: u32) -> Result<Self> {
        let entity = (*native.tag(tag, Some(0x80809c0f))?).clone();
        if entity.u32(0x88)? != u32::MAX {
            native.tag(entity.u32(0x88)?, Some(0x80809bb6))?;
        }
        let mut owners = BTreeMap::new();
        let mut allocations = BTreeMap::new();
        for row in entity.array(16, 12, Some(0x80809c04))? {
            let owner = entity.u32(row)?;
            let payload = (*native.tag(owner, Some(0x80809c36))?).clone();
            let allocation_tag = payload.u32(0x44)?;
            let allocation = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
            ensure!(
                owners.insert(owner, payload).is_none(),
                "Duplicate vehicle secondary component"
            );
            allocations.insert(owner, (allocation_tag, allocation));
        }
        ensure!(!owners.is_empty(), "Vehicle secondary entity is empty");
        Ok(Self {
            tag,
            entity,
            owners,
            allocations,
            resources: BTreeMap::new(),
            added: Vec::new(),
        })
    }

    /// Add an intact native component whose host requires no connection rows.
    /// Its own registration template supplies activation and cleanup callbacks.
    pub fn add_unconnected(&mut self, native: &mut Reader, owner: u32, host: u32) -> Result<()> {
        let entity = (*native.tag(host, Some(0x80809c0f))?).clone();
        let links = links::Graph::read(&entity, false)?;
        ensure!(
            links.components.contains(&owner)
                && links.connections.is_empty()
                && links.named_connections.is_empty(),
            "Additional vehicle component needs a connected host"
        );
        ensure!(
            !self.owners.contains_key(&owner),
            "Additional vehicle component already exists"
        );
        let payload = (*native.tag(owner, Some(0x80809c36))?).clone();
        let allocation = payload.u32(0x44)?;
        self.allocations.insert(
            owner,
            (
                allocation,
                (*native.tag(allocation, Some(0x80809bbb))?).clone(),
            ),
        );
        self.owners.insert(owner, payload);
        self.added.push((owner, entity));
        Ok(())
    }

    pub fn emit(mut self, graph: &mut Graph, prefix: &str) -> Result<String> {
        let symbol = format!("{prefix}-{:08X}", self.tag);
        let original_entity = self.entity.clone();
        let mut order = original_entity
            .array(16, 12, Some(0x80809c04))?
            .into_iter()
            .map(|row| original_entity.u32(row))
            .collect::<Result<Vec<_>>>()?;
        order.extend(self.added.iter().map(|(owner, _)| *owner));
        let components = order
            .iter()
            .map(|&owner| compiled::Component {
                owner,
                payload: &self.owners[&owner],
                template: &self.owners[&owner],
                entity: self
                    .added
                    .iter()
                    .find(|(added, _)| *added == owner)
                    .map(|(_, entity)| entity)
                    .unwrap_or(&original_entity),
            })
            .collect::<Vec<_>>();
        if !self.added.is_empty() {
            let connections = links::Graph::read(&original_entity, false)?.native_rows(
                &order,
                &self.owners,
                Ok,
                |_, channel| Ok(channel),
                |_, selector| Ok(selector),
            )?;
            self.entity = compiled::emit(&original_entity, &components, &connections)?;
        }
        let replication = if original_entity.u32(0x88)? != u32::MAX {
            Some(compiled::replication::emit(self.tag, &components)?)
        } else {
            None
        };
        let symbols = self
            .owners
            .keys()
            .map(|&owner| (owner, format!("{symbol}-{owner:08X}")))
            .collect::<BTreeMap<_, _>>();
        let mut entity = self.entity.clone();
        let mut entity_patches = Vec::new();
        if let Some(mut replication) = replication {
            let replication_symbol = format!("{symbol}-replication");
            let mut patches = vec![json!({"offset":8,"symbol":symbol})];
            for (&owner, row) in order
                .iter()
                .zip(replication.array(0x30, 32, Some(0x80809bb8))?)
            {
                patches.push(
                    json!({"offset":row+4,"symbol":format!("{}-allocation",symbols[&owner])}),
                );
            }
            for patch in &patches {
                put(
                    &mut replication.0,
                    patch["offset"].as_u64().unwrap() as usize,
                    &u32::MAX.to_le_bytes(),
                )?;
            }
            graph.add(
                &replication_symbol,
                original_entity.u32(0x88)?,
                &replication.0,
                None,
                patches,
            )?;
            put(&mut entity.0, 0x88, &u32::MAX.to_le_bytes())?;
            entity_patches.push(json!({"offset":0x88,"symbol":replication_symbol}));
        }
        for (&owner, payload) in &self.owners {
            let target = &symbols[&owner];
            for offset in entity::owner_slots(&self.entity, payload, owner)? {
                put(&mut entity.0, offset, &u32::MAX.to_le_bytes())?;
                entity_patches.push(json!({"offset":offset,"symbol":target}));
            }
            let (allocation_tag, allocation) = &self.allocations[&owner];
            let allocation_symbol = format!("{target}-allocation");
            graph.add(
                &allocation_symbol,
                *allocation_tag,
                &allocation.0,
                None,
                Vec::new(),
            )?;
            let mut bytes = payload.0.clone();
            let mut patches = BTreeMap::from([(0x44, allocation_symbol)]);
            if let Some(resources) = self.resources.get(&owner) {
                for (&offset, symbol) in resources {
                    ensure!(
                        patches.insert(offset, symbol.clone()).is_none(),
                        "Vehicle secondary resource overlaps allocation"
                    );
                }
            }
            for offset in (0..payload.0.len().saturating_sub(15)).step_by(4) {
                let referenced_owner = payload.u32(offset)?;
                let Some(target) = symbols.get(&referenced_owner) else {
                    continue;
                };
                let class = payload.u32(offset + 4)?;
                let reference = payload.u64(offset + 8)?;
                if class & 0xffff0000 != 0x80800000
                    && !entity::root_reference(&self.owners[&referenced_owner], class, reference)?
                {
                    continue;
                }
                let referenced = usize::try_from(reference)?;
                ensure!(
                    referenced
                        .checked_add(16)
                        .is_some_and(|end| end <= self.owners[&referenced_owner].0.len()),
                    "Vehicle secondary resource exceeds its owner"
                );
                ensure!(
                    patches.insert(offset, target.clone()).is_none(),
                    "Vehicle secondary resource overlaps a typed owner"
                );
            }
            for &offset in patches.keys() {
                put(&mut bytes, offset, &u32::MAX.to_le_bytes())?;
            }
            graph.add(
                target,
                owner,
                &bytes,
                None,
                patches
                    .into_iter()
                    .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
                    .collect(),
            )?;
        }
        graph.add(&symbol, self.tag, &entity.0, None, entity_patches)?;
        Ok(symbol)
    }
}
