//! Private raw channels for vehicle controls without changing presentation inputs.
use super::*;
use crate::tiger::{channel, instance};

pub(in crate::halo_reach::native::vehicle) struct Channel {
    pub provider: links::Endpoint,
    pub index: u32,
    pub name: u32,
}

impl Assembly {
    fn flight_boost_release(&mut self) -> Result<()> {
        let owner = self.owner(0x80803cde)?;
        let hover = self.payloads.get_mut(&owner).unwrap();
        let definition = usize::try_from(hover.u64(hover.pointer(16)? + 8)?)?;
        ensure!(
            hover.u32(definition - 4)? == 0x80803171,
            "Flight carrier hover definition differs"
        );
        for profile in [0x6c0, 0x800] {
            let at = definition + profile;
            ensure!(
                hover.u32(at)? == owner
                    && hover.u32(at + 4)? == 0x80803cc5
                    && hover.f32(at + 0x78)?.is_finite(),
                "Flight carrier boost profile differs"
            );
            // EB3310 clears boost on release only after this minimum elapsed
            // time. EB7D60 owns that clock and is replaced by flight motion.
            // Keep boost activation and its firing gate, but release immediately.
            put(&mut hover.0, at + 0x78, &0f32.to_le_bytes())?;
        }
        hover.0[definition + 0x9a1] = 1;
        Ok(())
    }

    pub(super) fn numeric_provider(
        &self,
        native: &mut Reader,
        implementation: u32,
        method: u32,
    ) -> Result<links::Object> {
        let owner = self.owner(implementation)?;
        channel::inputs::provider(
            native,
            owner,
            &self.payloads[&owner],
            implementation,
            method,
        )
    }

    pub(super) fn channel_bridge(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        symbol: &str,
        sources: &[(links::Object, u16)],
    ) -> Result<Vec<Channel>> {
        let owner = self.owner(0x8080979f)?;
        let original = self.payloads[&owner].clone();
        let definition = usize::try_from(original.u64(original.pointer(16)? + 8)?)?;
        let inputs = original.array(definition + 0xc8, 32, Some(0x808097a8))?;
        let rows = original.array(definition + 0xd8, 112, Some(0x808097a1))?;
        ensure!(
            !sources.is_empty() && rows.len() >= inputs.len(),
            "Vehicle raw input dimensions differ"
        );
        let provider = self
            .links
            .connections
            .iter()
            .chain(&self.links.named_connections)
            .find(|row| {
                row.provider
                    .object
                    .is_some_and(|v| v.owner == owner && v.class == 0x808097c2)
            })
            .context("Vehicle vector channel provider")?
            .provider
            .clone();
        let object = provider.object.unwrap();
        let metadata = native.tag(original.u32(object.offset as usize + 8)?, Some(0x80809c54))?;
        let interface = links::Interface::read(&original, object, &metadata, false)?;
        ensure!(
            interface.definition_offset == definition && provider.namespace == 0x811c9dc5,
            "Vehicle raw input provider belongs to another bank"
        );
        let mut selected = Vec::new();
        let mut additions = Vec::new();
        let mut bindings = Vec::new();
        for (request, &(source, value_type)) in sources.iter().enumerate() {
            let existing = self
                .links
                .connections
                .iter()
                .chain(&self.links.named_connections)
                .filter(|row| row.provider.object == Some(source))
                .filter_map(|row| {
                    row.consumer
                        .object
                        .filter(|v| v.owner == owner && v.class == 0x808097a8)
                        .and_then(|v| inputs.iter().position(|&at| at as u64 == v.offset))
                })
                .collect::<Vec<_>>();
            ensure!(
                existing.len() <= 1,
                "Vehicle raw provider has duplicate notification consumers"
            );
            if let Some(&index) = existing.first() {
                let row = rows[index];
                ensure!(
                    original.u64(row + 8)? == 0
                        && original.u16(row + 74)? == value_type
                        && original.u64(row + 96)? == 0,
                    "Vehicle source is not an unmodified raw input"
                );
                selected.push((index, original.u32(row)?));
                continue;
            }
            let name = format!("{symbol}:{request}")
                .bytes()
                .fold(0x811c9dc5u32, |hash, byte| {
                    hash.wrapping_mul(0x01000193) ^ u32::from(byte)
                });
            let source_payload = self
                .payloads
                .get(&source.owner)
                .context("Vehicle raw provider owner")?;
            ensure!(
                source.class == 0x80809ae1,
                "Vehicle raw input requires a numeric provider"
            );
            additions.push(channel::inputs::Input {
                name,
                namespace: 0x811c9dc5,
                property: source_payload.u32(source.offset as usize + 24)?,
                value_type,
            });
            selected.push((inputs.len() + bindings.len(), name));
            bindings.push(source);
        }
        if !additions.is_empty() {
            let allocation_tag = original.u32(0x44)?;
            let allocation = if let Some(symbol) = self.allocations.get(&owner) {
                graph.read(symbol)?
            } else {
                (*native.tag(allocation_tag, Some(0x80809bbb))?).clone()
            };
            let extension = channel::inputs::extend(&original, &allocation, &additions)?;
            for row in self
                .links
                .connections
                .iter_mut()
                .chain(&mut self.links.named_connections)
            {
                if row
                    .provider
                    .object
                    .is_some_and(|v| v.owner == owner && v.class == 0x808097c2)
                {
                    row.channel = extension.channel(row.channel)?;
                }
                if let Some(consumer) = row
                    .consumer
                    .object
                    .as_mut()
                    .filter(|v| v.owner == owner && v.class == 0x808097a8)
                {
                    consumer.offset = *extension
                        .relocated
                        .get(&(consumer.offset as usize))
                        .context("Vehicle bank input is outside its declarations")?
                        as u64;
                }
            }
            let ordinal = self
                .links
                .components
                .iter()
                .position(|&tag| tag == owner)
                .context("Vehicle raw input bank ordinal")? as u64;
            for (&source, &declaration) in bindings.iter().zip(&extension.declarations) {
                let source_ordinal =
                    self.links
                        .components
                        .iter()
                        .position(|&tag| tag == source.owner)
                        .context("Vehicle raw input provider ordinal")? as u64;
                self.links.connections.push(links::Connection {
                    consumer: links::Endpoint {
                        namespace: 0x811c9dc5,
                        selector: ordinal,
                        object: Some(links::Object {
                            owner,
                            class: 0x808097a8,
                            offset: declaration as u64,
                        }),
                    },
                    provider: links::Endpoint {
                        namespace: 0x811c9dc5,
                        selector: source_ordinal,
                        object: Some(source),
                    },
                    channel: 0,
                    flags: 0,
                });
            }
            let allocation_symbol = format!("{symbol}-allocation");
            graph.add(
                &allocation_symbol,
                allocation_tag,
                &extension.allocation.0,
                None,
                Vec::new(),
            )?;
            self.allocations.insert(owner, allocation_symbol);
            let owner_symbol = &self.symbols[&owner];
            let mut declared = patches(graph, owner_symbol)?;
            let mut payload = extension.payload;
            let end = payload.0.len();
            instance::seal_span(&mut payload, &mut declared, end)?;
            graph.replace(owner_symbol, &payload.0, declared)?;
            self.payloads.insert(owner, payload);
        }
        Ok(selected
            .into_iter()
            .map(|(index, name)| Channel {
                provider: provider.clone(),
                index: index as u32,
                name,
            })
            .collect())
    }

    pub(in crate::halo_reach::native::vehicle) fn flight_channels(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
    ) -> Result<Vec<Channel>> {
        let hover = &self.payloads[&self.owner(0x80803cde)?];
        let definition = usize::try_from(hover.u64(hover.pointer(16)? + 8)?)?;
        let maneuvers = hover.u8(definition + 0x9b1)? != 0 && hover.u64(definition + 0x9b8)? != 0;
        let mut sources = vec![
            (self.numeric_provider(native, 0x80803c23, 55)?, 2),
            (self.numeric_provider(native, 0x80803c23, 60)?, 3),
            // The native primary-body speed input uses raw value type 4.
            (self.numeric_provider(native, 0x808092d8, 26)?, 4),
        ];
        if maneuvers {
            let action = hover.pointer(definition + 0x9b8)?;
            ensure!(
                hover.u32(action - 4)? == 0x80803cce,
                "Vehicle maneuver definition differs"
            );
            sources.extend([
                (self.numeric_provider(native, 0x80803cde, 19)?, 0),
                (self.numeric_provider(native, 0x80803cde, 20)?, 0),
            ]);
            self.flight_boost_release()?;
        }
        self.channel_bridge(native, graph, "vehicle-flight-channels", &sources)
    }
}
