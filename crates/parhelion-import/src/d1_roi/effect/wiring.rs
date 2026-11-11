//! Native connection rows of a translated effect graph. Source rows name owners by ordinal and
//! are rebuilt by role. Template rows supply the native consumer and provider definitions of
//! retained-shape owners, and the unbound requests no source row names.
use super::component::{Inputs, Translated};
use super::*;

/// Rebuild every live source row by role, in source order. The returned set holds the template
/// rows that source rows stand for, which the template row stage leaves out.
pub(super) fn source_rows(
    graph: &connections::Graph,
    owners: &[intake::Owner],
    template: &Template,
    translated: &Translated,
) -> Result<(Vec<links::Connection>, BTreeSet<usize>)> {
    let roles = Roles {
        owners,
        template,
        translated,
    };
    let mut rows = Vec::new();
    let mut used = BTreeSet::new();
    for row in &graph.connections {
        if let Some(mapped) = roles.map(row, &mut used)? {
            rows.push(mapped);
        }
    }
    Ok((rows, used))
}

/// What a source row's native role reads.
struct Roles<'a> {
    owners: &'a [intake::Owner],
    template: &'a Template,
    translated: &'a Translated,
}

impl Roles<'_> {
    fn component(&self, ordinal: u64) -> Result<&Component> {
        self.translated
            .components
            .iter()
            .find(|component| component.ordinal as u64 == ordinal)
            .context("Source connection names an omitted owner")
    }

    fn owner(&self, ordinal: usize, missing: &'static str) -> Result<&intake::Owner> {
        self.owners
            .iter()
            .find(|owner| owner.0 == ordinal)
            .context(missing)
    }

    /// One source row as a native row, or none when a template row stands in for it.
    fn map(
        &self,
        row: &connections::Connection,
        used: &mut BTreeSet<usize>,
    ) -> Result<Option<links::Connection>> {
        ensure!(
            row.consumer.namespace == 0x811C9DC5 && row.provider.namespace == 0x811C9DC5,
            "Source effect connection has a named namespace"
        );
        let consumer = self.component(row.consumer.selector)?;
        if self.unbound(row, consumer) {
            return Ok(None);
        }
        ensure!(
            row.provider.selector < 0xFFFF,
            "Source effect connection has an unbound or external provider"
        );
        let provider = self.component(row.provider.selector)?;
        let channel = u32::try_from(row.channel)?;
        let roles = (
            consumer.family,
            row.consumer.class,
            provider.family,
            row.provider.class,
        );
        Ok(Some(match roles {
            (Family::Status, 0x80800456, Family::Sequence, 0x80800988) => {
                self.status_sequence(used, row, consumer, provider, channel)?
            }
            (Family::Status, 0x80800927, Family::Channel, 0x808005AB) => {
                self.status_channel(used, row, consumer, provider, channel)?
            }
            (Family::Duration, 0x80800927, Family::Channel, 0x808005AB) => {
                self.duration_channel(used, row, consumer, provider, channel)?
            }
            (Family::IncomingDamage, 0x80800927, Family::Channel, 0x808005AB) => {
                self.incoming_channel(used, row, consumer, provider, channel)?
            }
            (Family::Sequence, 0x80800927, Family::Channel, 0x808005AB) => {
                self.sequence_channel(row, consumer, provider, channel)?
            }
            (
                Family::Channel,
                0x80800668,
                Family::Status | Family::Receiver | Family::Duration,
                0x8080053B,
            ) => self.getter(used, row, consumer, provider, channel)?,
            _ => anyhow::bail!(
                "Source effect connection {:?} has no native role",
                (
                    roles.0.name(),
                    roles.1,
                    roles.2.name(),
                    roles.3,
                    row.channel
                )
            ),
        }))
    }

    /// Source requests whose native counterpart is an unbound template request, which the
    /// template row stage keeps.
    fn unbound(&self, row: &connections::Connection, consumer: &Component) -> bool {
        if row.provider.selector == 0xFFFF && consumer.family == Family::Category {
            // The retained category's unbound request comes from the template row.
            return true;
        }
        if consumer.family == Family::Damage
            && row.consumer.class == 0x8080073D
            && row.provider.selector == 0x20000
        {
            // The damage component's gather, garbage and routing marker inputs read the owner's
            // markers through an external adapter. Every stock overshield leaves its three
            // `80809BD7` requests unbound, and the template's rows stand in for them.
            return true;
        }
        // A status without a lifetime sequence leaves its lifetime request unbound. The
        // template's unbound status requests stand in for it, as `81A699C6` holds them.
        row.provider.selector == 0xFFFF
            && consumer.family == Family::Status
            && row.consumer.class == 0x80800456
            && self
                .translated
                .status
                .as_ref()
                .is_some_and(|fields| row.consumer.offset == fields.provider_input as u64)
    }

    fn status_sequence(
        &self,
        used: &mut BTreeSet<usize>,
        row: &connections::Connection,
        consumer: &Component,
        provider: &Component,
        channel: u32,
    ) -> Result<links::Connection> {
        let status_fields = self
            .translated
            .status
            .as_ref()
            .context("Status row without a status")?;
        ensure!(
            row.consumer.offset == status_fields.provider_input as u64 && channel == 0,
            "Source status names another lifetime sequence receiver"
        );
        let (index, request, _, native_channel) = self
            .template
            .row((Family::Status, 0x808092F4), (Family::Sequence, 0x808092F8))?;
        ensure!(
            used.insert(index),
            "Source status has two lifetime sequences"
        );
        let (_, definition) = pair(&provider.payload)?;
        Ok(links::Connection {
            consumer: endpoint(consumer.id, 0x808092F4, request)?,
            provider: endpoint(provider.id, 0x808092F8, definition + 0x90)?,
            channel: native_channel,
            flags: 0,
        })
    }

    fn status_channel(
        &self,
        used: &mut BTreeSet<usize>,
        row: &connections::Connection,
        consumer: &Component,
        provider: &Component,
        channel: u32,
    ) -> Result<links::Connection> {
        let status_fields = self
            .translated
            .status
            .as_ref()
            .context("Status row without a status")?;
        ensure!(
            Some(row.consumer.offset) == status_fields.lifetime_input.map(|at| at as u64),
            "Source status channel input is not its lifetime input"
        );
        let (index, request, bank, native_channel) = self
            .template
            .row((Family::Status, 0x80809789), (Family::Channel, 0x808097C2))?;
        // The bank keeps its declaration order, so ordinals name the same channels.
        ensure!(
            used.insert(index) && native_channel == channel,
            "Native lifetime input reads another channel ordinal"
        );
        Ok(links::Connection {
            consumer: endpoint(consumer.id, 0x80809789, request)?,
            provider: endpoint(provider.id, 0x808097C2, bank)?,
            channel,
            flags: 0,
        })
    }

    fn duration_channel(
        &self,
        used: &mut BTreeSet<usize>,
        row: &connections::Connection,
        consumer: &Component,
        provider: &Component,
        channel: u32,
    ) -> Result<links::Connection> {
        let fields = self
            .translated
            .host
            .as_ref()
            .context("Duration row without a Duration host")?;
        ensure!(
            Some(row.consumer.offset) == fields.lifetime_input.map(|at| at as u64),
            "Source Duration channel input is not its lifetime input"
        );
        let (index, request, _, _) = self.template.row(
            (Family::Duration, 0x80809789),
            (Family::Channel, 0x808097C2),
        )?;
        ensure!(
            used.insert(index)
                && consumer.template_tag == self.template.owners[&Family::Duration].1,
            "Source Duration host has another lifetime input"
        );
        // The bank keeps the source declaration order, so the source ordinal carries.
        let (_, bank) = pair(&provider.payload)?;
        Ok(links::Connection {
            consumer: endpoint(consumer.id, 0x80809789, request)?,
            provider: endpoint(provider.id, 0x808097C2, bank + 0x60)?,
            channel,
            flags: 0,
        })
    }

    /// The base program's one named input. The template copy keeps its input row, and the bank
    /// keeps the source declaration order, so the source ordinal carries.
    fn incoming_channel(
        &self,
        used: &mut BTreeSet<usize>,
        row: &connections::Connection,
        consumer: &Component,
        provider: &Component,
        channel: u32,
    ) -> Result<links::Connection> {
        let (_, _, source_owner, _) = self.owner(
            consumer.ordinal,
            "Incoming damage row without its source owner",
        )?;
        let (_, _, input) = pattern::program(source_owner, source_owner.pointer(24)? + 0x98)?;
        ensure!(
            input.map(|(_, at)| at as u64) == Some(row.consumer.offset),
            "Source incoming damage channel input is not its program input"
        );
        let (index, request, _, _) = self.template.row(
            (Family::IncomingDamage, 0x80809789),
            (Family::Channel, 0x808097C2),
        )?;
        ensure!(
            used.insert(index),
            "Source incoming damage has another program input"
        );
        let (_, bank) = pair(&provider.payload)?;
        Ok(links::Connection {
            consumer: endpoint(consumer.id, 0x80809789, request)?,
            provider: endpoint(provider.id, 0x808097C2, bank + 0x60)?,
            channel,
            flags: 0,
        })
    }

    fn sequence_channel(
        &self,
        row: &connections::Connection,
        consumer: &Component,
        provider: &Component,
        channel: u32,
    ) -> Result<links::Connection> {
        let definition = *self
            .translated
            .inputs
            .get(&consumer.ordinal)
            .and_then(|map| map.get(&usize::try_from(row.consumer.offset).ok()?))
            .context("Source sequence connection names an unknown input")?;
        let (_, bank) = pair(&provider.payload)?;
        Ok(links::Connection {
            consumer: endpoint(consumer.id, 0x80809789, definition)?,
            provider: endpoint(provider.id, 0x808097C2, bank + 0x60)?,
            channel,
            flags: 0,
        })
    }

    /// A bank input getter reading a status, receiver or Duration host output. The template's own
    /// row between these roles, when it has one, stands for this row. A receiver's getter needs
    /// none, since its place follows the source row. The template's own bank keeps its row's
    /// request.
    fn getter(
        &self,
        used: &mut BTreeSet<usize>,
        row: &connections::Connection,
        consumer: &Component,
        provider: &Component,
        channel: u32,
    ) -> Result<links::Connection> {
        let provider_family = provider.family;
        ensure!(channel == 0, "Channel input getter reads another output");
        let template_row = self
            .template
            .row((Family::Channel, 0x808097A8), (provider_family, 0x80809AE1))
            .ok();
        if let Some((index, _, _, native_channel)) = template_row {
            ensure!(
                used.insert(index) && native_channel == 0,
                "Channel input getter reads another output"
            );
        }
        let template_bank = self
            .template
            .owners
            .get(&Family::Channel)
            .is_some_and(|owner| consumer.template_tag == owner.1);
        let request = match template_row {
            Some((_, request, _, _)) if template_bank => request,
            _ => self.bank_request(row, consumer)?,
        };
        let getter = if provider_family == Family::Receiver {
            self.receiver_getter(row, provider)?
        } else {
            let (_, _, getter, _) = template_row.with_context(|| {
                format!(
                    "Native effect template has no {} getter row",
                    provider_family.name()
                )
            })?;
            self.provider_getter(provider, getter)?
        };
        Ok(links::Connection {
            consumer: endpoint(consumer.id, 0x808097A8, request)?,
            provider: endpoint(provider.id, 0x80809AE1, getter)?,
            channel: 0,
            flags: 0,
        })
    }

    /// A bank names its getter requests through typed references. They follow its incoming
    /// declarations in order from definition `+168`, as the source bank's follow its own, so the
    /// source request's place among them names the native one.
    fn bank_request(&self, row: &connections::Connection, consumer: &Component) -> Result<usize> {
        let (_, tag, source_bank, _) = self.owner(
            consumer.ordinal,
            "Channel getter row without its source bank",
        )?;
        let requests = getters(source_bank, *tag)?;
        let place = requests
            .iter()
            .position(|&at| at as u64 == row.consumer.offset)
            .context("Source channel getter request is not one of its bank's")?;
        let found = typed(&consumer.payload, consumer.id, 0x808097A8)?;
        ensure!(
            found.len() == requests.len(),
            "Channel bank has another getter request count"
        );
        Ok(found[place])
    }

    /// A receiver publishes one getter in each named row, at source row `+18` and native row
    /// `+10`. Its writer writes the source rows in order, so the getter follows its row wherever
    /// the rows were written.
    fn receiver_getter(
        &self,
        row: &connections::Connection,
        provider: &Component,
    ) -> Result<usize> {
        let (_, _, source_receiver, _) = self.owner(
            provider.ordinal,
            "Receiver getter row without its source receiver",
        )?;
        let place = source_receiver
            .array(source_receiver.pointer(24)? + 0x90, 0x50, Some(0x80800884))?
            .iter()
            .position(|&at| (at + 0x18) as u64 == row.provider.offset)
            .context("Source receiver getter is not one of its rows'")?;
        let (_, definition) = pair(&provider.payload)?;
        Ok(provider
            .payload
            .array(definition + 0x78, 0x40, Some(0x80808BFA))?
            .get(place)
            .context("Written receiver lacks the getter's row")?
            + 0x10)
    }

    /// Native Duration hosts publish their getter in the empty-state object at definition
    /// `+1D0`, `30` bytes in, as template host `80BC32A1` does at `+428` and `80FD758F` at
    /// `+3A8`. A replacement host's getter is there too. The template's own owner keeps the
    /// template row's getter.
    fn provider_getter(&self, provider: &Component, getter: usize) -> Result<usize> {
        let provider_family = provider.family;
        if self
            .template
            .owners
            .get(&provider_family)
            .is_some_and(|owner| provider.template_tag == owner.1)
        {
            Ok(getter)
        } else {
            let (_, definition) = pair(&self.template.owners[&provider_family].2)?;
            ensure!(
                provider_family == Family::Duration && getter == definition + 0x200,
                "Replacement {} owner has no qualified channel getter",
                provider_family.name()
            );
            Ok(pair(&provider.payload)?.1 + 0x200)
        }
    }
}

/// Template rows without a source counterpart must be unbound requests of a retained owner.
/// Rows of the template sequence are replaced by the translated sequences' own inputs.
pub(super) fn template_rows(
    template: &Template,
    used: &BTreeSet<usize>,
    components: &[Component],
    rows: &mut Vec<links::Connection>,
) -> Result<()> {
    for (index, row) in template.links.connections.iter().enumerate() {
        if used.contains(&index) {
            continue;
        }
        let consumer = row
            .consumer
            .object
            .context("Native template row has no consumer")?;
        if template.dropped.contains(&consumer.owner) {
            continue;
        }
        let family = template
            .family(consumer.owner)
            .context("Native template row names another owner")?;
        if family == Family::Sequence {
            continue;
        }
        // A request a template-only owner feeds stays unbound, as the source's is.
        let provider_endpoint = match row.provider.object {
            Some(object) if template.dropped.contains(&object.owner) => {
                ensure!(
                    family == Family::Category,
                    "Native template-only owner feeds a {} request",
                    family.name()
                );
                links::Endpoint {
                    namespace: row.provider.namespace,
                    object: None,
                    selector: 0xFFFF,
                }
            }
            _ => row.provider.clone(),
        };
        // Rows of a template owner the source does not have leave with that owner.
        let provider_family = row
            .provider
            .object
            .and_then(|object| template.family(object.owner));
        if !present(components, family)
            || provider_family.is_some_and(|family| !present(components, family))
        {
            continue;
        }
        // A replacement channel bank or Duration host brings its own requests, and the source
        // rows supply its connections. The template owner's rows leave with that owner.
        if replaced(components, family, consumer.owner)
            || row
                .provider
                .object
                .zip(provider_family)
                .is_some_and(|(object, family)| replaced(components, family, object.owner))
        {
            continue;
        }
        ensure!(
            provider_endpoint.object.is_none(),
            "Native effect template has a live {} row without a source counterpart",
            family.name()
        );
        let component = components
            .iter()
            .find(|component| component.family == family)
            .context("Native unbound request names an absent family")?;
        rows.push(links::Connection {
            consumer: endpoint(
                component.id,
                consumer.class,
                usize::try_from(consumer.offset)?,
            )?,
            provider: provider_endpoint,
            channel: row.channel,
            flags: row.flags,
        });
    }
    Ok(())
}

fn present(components: &[Component], family: Family) -> bool {
    components
        .iter()
        .any(|component| component.family == family)
}

/// Whether a channel bank, Duration host or status of this family replaced template owner
/// `owner` with a stock owner of the source's form.
fn replaced(components: &[Component], family: Family, owner: u32) -> bool {
    matches!(family, Family::Channel | Family::Duration | Family::Status)
        && components
            .iter()
            .any(|component| component.family == family && component.template_tag != owner)
}

/// A replacement Duration host or status keeps the unbound requests its own stock graph leaves
/// open, such as a host's empty state or an optional status's lifetime requests.
pub(super) fn stock_rows(
    template: &Template,
    components: &[Component],
    rows: &mut Vec<links::Connection>,
) -> Result<()> {
    for component in components.iter().filter(|c| {
        matches!(c.family, Family::Duration | Family::Status)
            && template
                .owners
                .get(&c.family)
                .is_none_or(|owner| c.template_tag != owner.1)
    }) {
        let graph = links::Graph::read(&component.entity, false)?;
        for row in &graph.connections {
            let Some(consumer) = row.consumer.object else {
                continue;
            };
            if consumer.owner != component.template_tag {
                continue;
            }
            ensure!(
                row.provider.object.is_none(),
                "Replacement Duration host has a live row in its stock graph"
            );
            rows.push(links::Connection {
                consumer: endpoint(
                    component.id,
                    consumer.class,
                    usize::try_from(consumer.offset)?,
                )?,
                provider: row.provider.clone(),
                channel: row.channel,
                flags: row.flags,
            });
        }
    }
    Ok(())
}

/// A native kind-zero control condition follows its input's provider unchecked, so every
/// translated sequence input needs the provider the source connects to it.
pub(super) fn check_inputs(
    components: &[Component],
    inputs: &Inputs,
    rows: &[links::Connection],
) -> Result<()> {
    for component in components {
        for &definition in inputs
            .get(&component.ordinal)
            .into_iter()
            .flat_map(BTreeMap::values)
        {
            ensure!(
                rows.iter()
                    .any(|row| row.consumer.object.is_some_and(|object| {
                        object.owner == component.id
                            && object.class == 0x80809789
                            && object.offset == definition as u64
                    })),
                "Source sequence occurrence {} leaves an input unconnected",
                component.ordinal
            );
        }
    }
    Ok(())
}
