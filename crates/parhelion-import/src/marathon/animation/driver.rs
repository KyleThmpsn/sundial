//! The source animation driver's local component bindings. Choosing a component by class alone
//! does not establish that this driver consumes it. Dispatch identities qualify the serialized
//! binding and are never emitted as Shadowkeep method ordinals.
use super::*;
use crate::tiger::entity::links::{Graph, Interface};

pub(in crate::marathon) const CLASS: u32 = 0x8080_3BE5;

pub(in crate::marathon) struct Driver {
    pub lookup: Component,
    pub pose: Option<Component>,
    pub controls: Component,
    pub skeleton: Component,
    pub report: Value,
}

impl Driver {
    pub fn read(reader: &mut Reader, entity: u32, owners: &[Component]) -> Result<Self> {
        let payload = reader.tag(entity, Some(0x8080_BAAD))?;
        let graph = Graph::read_marathon(&payload)?;
        ensure!(
            graph
                .components
                .iter()
                .copied()
                .eq(owners.iter().map(|owner| owner.0)),
            "Source animation component occurrences changed"
        );
        let drivers = owners
            .iter()
            .enumerate()
            .filter(|(_, owner)| owner.3 == CLASS)
            .collect::<Vec<_>>();
        let [(index, driver)] = drivers.as_slice() else {
            anyhow::bail!("Source entity {entity:08X} has no single animation driver");
        };
        let mut input = |field: usize,
                         class,
                         interface,
                         implementation,
                         method|
         -> Result<(Component, Value)> {
            let offset = u64::try_from(
                driver
                    .2
                    .checked_add(field)
                    .context("Driver input offset overflow")?,
            )?;
            let connections = graph
                .connections
                .iter()
                .chain(&graph.named_connections)
                .filter(|row| {
                    row.consumer.selector == *index as u64
                        && row
                            .consumer
                            .object
                            .is_some_and(|object| object.offset == offset)
                })
                .collect::<Vec<_>>();
            let [connection] = connections.as_slice() else {
                anyhow::bail!("Animation driver input is absent or ambiguous");
            };
            let consumer = connection
                .consumer
                .object
                .context("Animation driver consumer is absent")?;
            ensure!(
                consumer.owner == driver.0
                    && consumer.class == 0x8080_BA71
                    && connection.consumer.namespace == controller::UNSET
                    && connection.provider.namespace == controller::UNSET
                    && connection.flags == 0,
                "Animation driver input has untranslated connection semantics"
            );
            ensure!(
                connection.provider.selector < 0xFFFF,
                "Animation driver input is not a local component"
            );
            let provider = connection
                .provider
                .object
                .context("Animation driver provider is absent")?;
            let component = usize::try_from(connection.provider.selector)?;
            let owner = owners
                .get(component)
                .context("Animation driver provider occurrence is absent")?;
            ensure!(
                owner.0 == provider.owner && owner.3 == class && provider.class == interface,
                "Animation driver provider has a different component or interface class"
            );
            let metadata_tag = owner.1.u32(
                usize::try_from(provider.offset)?
                    .checked_add(8)
                    .context("Provider metadata offset overflow")?,
            )?;
            let metadata = reader.tag(metadata_tag, Some(0x8080_BAF8))?;
            let dispatch = Interface::read_marathon(&owner.1, provider, &metadata)?;
            ensure!(
                dispatch.definition_offset == owner.2,
                "Animation provider addresses a different definition"
            );
            let selected = dispatch
                .methods
                .get(usize::try_from(connection.channel)?)
                .context("Animation driver provider channel has no method")?;
            ensure!(
                selected.implementation_class == implementation
                    && selected.index == method
                    && selected.arguments == [0, 0],
                "Animation driver provider dispatch is untranslated"
            );
            Ok((
                owner.clone(),
                json!({"field":field,"component":component,"owner":owner.0,
                "definition":owner.2,"definition_class":owner.3,"interface":provider.class,
                "offset":provider.offset,"metadata":metadata_tag,"channel":connection.channel,
                "method":selected}),
            ))
        };
        let mut bound = |field, class, interface, implementation, method| {
            input(field, class, interface, implementation, method).with_context(|| {
                format!(
                    "Source entity {entity:08X} animation driver {:08X} input +{field:X}",
                    driver.0
                )
            })
        };
        let (lookup, lookup_report) =
            bound(0x358, controller::LOOKUP, 0x8080_3953, 0x8080_3951, 0)?;
        let has_pose = graph
            .connections
            .iter()
            .chain(&graph.named_connections)
            .any(|row| {
                row.consumer.selector == *index as u64
                    && row
                        .consumer
                        .object
                        .is_some_and(|object| object.offset == (driver.2 + 0x318) as u64)
                    && row
                        .provider
                        .object
                        .is_some_and(|object| object.class == 0x8080_3982)
            });
        let pose = has_pose
            .then(|| bound(0x318, 0x8080_397B, 0x8080_3982, 0x8080_3980, 0))
            .transpose()?;
        let (controls, controls_report) =
            bound(0x378, source::CONTROLS, 0x8080_AD87, 0x8080_AD85, 0)?;
        let (skeleton, skeleton_report) =
            bound(0x398, source::SKELETON, 0x8080_AF37, 0x8080_9FB6, 2)?;
        Ok(Self {
            lookup,
            pose: pose.as_ref().map(|(component, _)| component.clone()),
            controls,
            skeleton,
            report: json!({"owner":driver.0,"component":index,
            "lookup":lookup_report,"pose":pose.map(|(_, report)| report),"controls":controls_report,"skeleton":skeleton_report}),
        })
    }
}
