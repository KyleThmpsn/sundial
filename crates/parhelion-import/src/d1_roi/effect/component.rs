//! Each source owner as a native component. A shared framework owner keeps its stock owner. Every
//! other owner takes a private copy of its native counterpart that holds the source values.
use super::choice::Choice;
use super::*;

/// What every owner's translation reads.
pub(super) struct Scope<'a> {
    pub(super) source: &'a Source,
    pub(super) shared: &'a Shared,
    /// Native sequences whose scalar input rows a translated sequence may copy.
    pub(super) envelopes: &'a [Payload],
    /// The source entity graph, whose rows the network, status, modifier and host readers follow.
    pub(super) entity: &'a Payload,
    pub(super) choice: &'a Choice,
    pub(super) symbol: &'a str,
    /// The first temporary identity. Each owner takes `base` plus its source ordinal.
    pub(super) base: u32,
}

/// Source input definition to native definition, per translated sequence ordinal.
pub(super) type Inputs = BTreeMap<usize, BTreeMap<usize, usize>>;

/// Every translated component in source order, with what later stages read from the owners.
pub(super) struct Translated {
    pub(super) components: Vec<Component>,
    /// Source owners without a translation, each with its reason.
    pub(super) omitted: Vec<Value>,
    pub(super) dependencies: BTreeMap<String, dependency::Asset>,
    pub(super) inputs: Inputs,
    /// The source status's fields, which its lifetime rows check.
    pub(super) status: Option<pattern::Connected>,
    /// The source Duration host's fields, which its lifetime row checks.
    pub(super) host: Option<pattern::Host>,
    pub(super) hud: Vec<Value>,
    pub(super) private_hud: Vec<u32>,
}

/// The native owner a source family translates from, with the graph that registers it. A
/// borrowed owner has no template ordinal.
struct Native<'a> {
    ordinal: usize,
    tag: u32,
    owner: &'a Payload,
    entity: &'a Payload,
}

/// Translate every source owner in source order. Status graphs need their lifetime owner, and
/// victim and presentation graphs have none.
pub(super) fn translate(
    native: &mut Reader,
    scope: &Scope,
    owners: &[intake::Owner],
) -> Result<Translated> {
    let mut translated = Translated {
        components: Vec::new(),
        omitted: Vec::new(),
        dependencies: BTreeMap::new(),
        inputs: BTreeMap::new(),
        status: None,
        host: None,
        hud: Vec::new(),
        private_hud: Vec::new(),
    };
    for item in owners {
        let (ordinal, owner, payload, family) = item;
        let Some(family) = *family else {
            translated
                .omitted
                .push(omission(*ordinal, *owner, payload)?);
            continue;
        };
        // Only framework families are borrowed, and they keep their stock owner.
        let counterpart = scope.counterpart(family)?;
        if let Some(stock) = scope.retained(*owner, counterpart.tag) {
            let (component, inputs) = retain(scope, item, family, &counterpart, stock)?;
            if let Some(inputs) = inputs {
                translated.inputs.insert(*ordinal, inputs);
            }
            translated.components.push(component);
            continue;
        }
        let component = translated.private(native, scope, item, family, &counterpart)?;
        translated.components.push(component);
    }
    ensure!(
        translated.status.is_some() || !scope.choice.template.owners.contains_key(&Family::Status),
        "Source effect graph has no status"
    );
    Ok(translated)
}

impl Scope<'_> {
    fn counterpart(&self, family: Family) -> Result<Native<'_>> {
        let template = &self.choice.template;
        Ok(match template.owners.get(&family) {
            Some((ordinal, tag, owner)) => Native {
                ordinal: *ordinal,
                tag: *tag,
                owner,
                entity: &template.graph,
            },
            None => {
                let (tag, owner, graph) = self
                    .choice
                    .borrowed
                    .get(&family)
                    .context("Native effect template lacks a source owner family")?;
                Native {
                    ordinal: usize::MAX,
                    tag: *tag,
                    owner,
                    entity: graph,
                }
            }
        })
    }

    /// The stock owner a shared framework owner keeps. Status templates translate their network
    /// privately, unless the template names the framework owner's own stock counterpart, as
    /// `80FF06FD` does for Demotion's victim graph.
    fn retained(&self, owner: u32, native_tag: u32) -> Option<u32> {
        let template = &self.choice.template;
        let retains =
            |stock: u32| !template.owners.contains_key(&Family::Status) || native_tag == stock;
        FRAMEWORK
            .iter()
            .find(|(s, stock)| *s == owner && retains(*stock))
            .map(|&(_, stock)| stock)
    }
}

/// An omitted source owner and the reason its definition class has no translation.
fn omission(ordinal: usize, owner: u32, payload: &Payload) -> Result<Value> {
    let class = payload.u32(payload.pointer(24)? - 4)?;
    // A modifier bank leaves only when detached, to be attached beside the graph.
    let reason = UNTRANSLATED
        .iter()
        .chain(&UNCONNECTED)
        .chain(&[(
            0x808020DE,
            "Unconnected modifier bank, attached beside the graph as its own modifier graph",
        )])
        .find(|(c, _)| *c == class)
        .map(|(_, r)| *r);
    Ok(json!({"ordinal":ordinal,"owner":owner,"definition_class":class,"reason":reason}))
}

/// A nested controller keeps the template's controller, and with it the stock child action, when
/// that child is the native form of the source child. An imported group cannot carry a privately
/// compiled child: groups are authored before perk programs compile and their patches name only
/// their own nodes. The controller stays in the graph, so the status that retires the graph also
/// ends the child, as in the source.
fn nested(
    native: &mut Reader,
    scope: &Scope,
    component: &mut Component,
    item: &intake::Owner,
    counterpart: &Native,
) -> Result<()> {
    let (_, owner, payload, _) = item;
    let evidence = pattern::nested(
        scope.source,
        native,
        (*owner, payload),
        (counterpart.tag, counterpart.owner),
    )?;
    let template_tag = component.template_tag;
    retarget(&mut component.payload, template_tag, component.id)?;
    component.evidence = json!({"stock_child_equivalence":evidence});
    Ok(())
}

/// A temporary owner identity, `base` plus the source ordinal, which pending patches replace with
/// the owner's private symbol.
fn temporary(base: u32, ordinal: usize) -> Result<u32> {
    base.checked_add(u32::try_from(ordinal)?)
        .context("Temporary effect identity overflow")
}

/// A shared framework owner keeps the stock owner of the same role. A replicated graph owns one
/// allocation row for every component, which must name a private owner, so there it takes an
/// unchanged private copy that keeps the stock allocation. A retained sequence also returns its
/// source to stock input map.
fn retain(
    scope: &Scope,
    item: &intake::Owner,
    family: Family,
    counterpart: &Native,
    stock: u32,
) -> Result<(Component, Option<BTreeMap<usize, usize>>)> {
    let (ordinal, _, payload, _) = item;
    ensure!(
        counterpart.tag == stock,
        "Native template names another {} framework owner",
        family.name()
    );
    let (id, symbol, retained) = if scope.choice.template.graph.u32(0x88)? == u32::MAX {
        (stock, String::new(), counterpart.owner.clone())
    } else {
        let id = temporary(scope.base, *ordinal)?;
        let mut payload = counterpart.owner.clone();
        retarget(&mut payload, stock, id)?;
        let symbol = scope.symbol;
        (id, format!("{symbol}-{}-{ordinal}", family.name()), payload)
    };
    let inputs = if family == Family::Sequence {
        Some(stock_inputs(payload, counterpart.owner, stock)?)
    } else {
        None
    };
    let component = Component {
        id,
        ordinal: *ordinal,
        family,
        symbol,
        template_tag: stock,
        template: counterpart.owner.clone(),
        entity: counterpart.entity.clone(),
        payload: retained,
        patches: Vec::new(),
        allocation: None,
        evidence: json!({"retained_stock_owner":stock}),
    };
    Ok((component, inputs))
}

/// A retained stock sequence keeps its own input declarations. Source connections name source
/// inputs, so each maps to the stock input of the same name in its own list, main inputs at
/// source `+1A0` and native `+188`, auxiliary ones at `+2A8` and `+2B8`. A name can appear in
/// both lists, as `414428AF` does in `80FA302E`.
fn stock_inputs(
    payload: &Payload,
    native_owner: &Payload,
    stock: u32,
) -> Result<BTreeMap<usize, usize>> {
    let from = pair(payload)?.1;
    let to = pair(native_owner)?.1;
    let mut map = BTreeMap::new();
    for (field, stock_field) in [(0x1A0, 0x188), (0x2A8, 0x2B8)] {
        let stock_inputs = native_owner.array(to + stock_field, 40, Some(0x80809789))?;
        for row in payload.array(from + field, 48, Some(0x80800927))? {
            let name = payload.u32(row + 0x28)?;
            let matched = stock_inputs
                .iter()
                .copied()
                .find(|&at| native_owner.u32(at + 0x20).ok() == Some(name))
                .with_context(|| {
                    format!("Stock sequence {stock:08X} lacks source input {name:08X}")
                })?;
            map.insert(row, matched);
        }
    }
    Ok(map)
}

impl Translated {
    /// A private copy of the native counterpart that holds the source owner's values. Each family
    /// writer may replace the counterpart with a stock owner of the source's form.
    fn private(
        &mut self,
        native: &mut Reader,
        scope: &Scope,
        item: &intake::Owner,
        family: Family,
        counterpart: &Native,
    ) -> Result<Component> {
        let (ordinal, owner, payload, _) = item;
        ensure!(
            !matches!(
                family,
                Family::Context | Family::Category | Family::Invisibility
            ),
            "Source {} owner {owner:08X} is not the shared framework owner",
            family.name()
        );
        let id = temporary(scope.base, *ordinal)?;
        let symbol = scope.symbol;
        let mut component = Component {
            id,
            ordinal: *ordinal,
            family,
            symbol: format!("{symbol}-{}-{ordinal}", family.name()),
            template_tag: counterpart.tag,
            template: counterpart.owner.clone(),
            entity: scope.choice.template.graph.clone(),
            payload: counterpart.owner.clone(),
            patches: Vec::new(),
            allocation: None,
            evidence: Value::Null,
        };
        let target = &mut component;
        match family {
            Family::Network => network(native, scope, target, counterpart)?,
            Family::Status => self.status(native, scope, target, item, counterpart.owner)?,
            Family::Sequence => self.sequence(native, scope, target, *owner, counterpart)?,
            Family::Receiver => receiver(native, target, payload, counterpart.tag)?,
            Family::Modifier => modifier(native, scope, target, *owner, counterpart.owner)?,
            Family::Channel => channel(native, target, payload, counterpart.owner)?,
            Family::Duration => self.duration(native, scope, target, item, counterpart.owner)?,
            Family::Damage => damage(target, payload)?,
            Family::IncomingDamage => incoming_damage(native, scope, target, payload)?,
            Family::Volume => condition_volume(native, scope, target, payload)?,
            Family::Nested => nested(native, scope, target, item, counterpart)?,
            Family::Context | Family::Category | Family::Invisibility => {
                anyhow::bail!("Framework owners are retained before translation")
            }
        }
        allocate(native, &mut component)?;
        Ok(component)
    }

    fn status(
        &mut self,
        native: &mut Reader,
        scope: &Scope,
        component: &mut Component,
        item: &intake::Owner,
        native_owner: &Payload,
    ) -> Result<()> {
        let (ordinal, owner, payload, _) = item;
        let fields = if payload.u32(payload.pointer(24)? - 4)? == 0x80802698 {
            pattern::status(scope.entity, payload, *owner, *ordinal)?
        } else {
            pattern::icon(payload, *owner)?
        };
        // A status whose lifetime receiver policy the template status lacks takes a stock status
        // of that policy, as a host takes a stock host of its lifetime form.
        let policy = |p: &Payload| -> Result<u64> { p.u64(p.pointer(24)? + 0x70) };
        if policy(native_owner)? != fields.receiver_policy {
            replace(
                native,
                component,
                &STATUSES,
                |candidate| Ok(policy(candidate)? == fields.receiver_policy),
                "No stock status has the source status's lifetime receiver policy",
            )?;
        }
        let name = u32::from_le_bytes(fields.hud[..4].try_into()?);
        if crate::d1_roi::native::perk::hud::read(scope.source, native, name)?.is_some() {
            ensure!(
                scope.shared.private_hud,
                "Effect status {name:08X} needs a private HUD entry"
            );
            self.private_hud.push(name);
        }
        let template_tag = component.template_tag;
        retarget(&mut component.payload, template_tag, component.id)?;
        component.evidence = owner::status(&mut component.payload, &fields)?;
        if let Some(display) = crate::d1_roi::native::perk::hud::source(scope.source, native, name)?
        {
            self.hud
                .push(json!({"owner":component.symbol,"name":name,"status":display}));
        }
        self.status = Some(fields);
        Ok(())
    }

    /// A translated sequence takes the source events through the ability's shared resources, and
    /// its input declarations replace the counterpart's.
    fn sequence(
        &mut self,
        native: &mut Reader,
        scope: &Scope,
        component: &mut Component,
        owner: u32,
        counterpart: &Native,
    ) -> Result<()> {
        let native_owner = counterpart.owner;
        let (sequence, bindings) = scope.shared.bindings(owner)?;
        let allocation_tag = native_owner.u32(0x44)?;
        let allocation = (*native.tag(allocation_tag, Some(0x80809BBB))?).clone();
        let input_envelope = if has_inputs(native_owner) {
            native_owner
        } else {
            scope
                .envelopes
                .iter()
                .find(|envelope| has_inputs(envelope))
                .context("No native scalar input envelope")?
        };
        let mut all = scope.envelopes.to_vec();
        all.push(native_owner.clone());
        all.extend(scope.shared.witnesses.iter().cloned());
        let mut written =
            sequence.write(native_owner, &allocation, input_envelope, &all, &bindings)?;
        retarget(&mut written.owner, counterpart.tag, component.id)?;
        merge(
            &mut self.dependencies,
            std::mem::take(&mut written.prepared.dependencies),
        )?;
        let definitions = sequence.definitions()?;
        ensure!(
            definitions.len() == written.inputs.len(),
            "Translated sequence inputs differ from the source"
        );
        self.inputs.insert(
            component.ordinal,
            definitions
                .into_iter()
                .zip(written.inputs.iter().map(|(_, definition)| *definition))
                .collect(),
        );
        component.evidence = sequence.evidence();
        component.payload = written.owner;
        component.patches = written.patches;
        component.allocation = Some((allocation_tag, written.allocation));
        Ok(())
    }

    fn duration(
        &mut self,
        native: &mut Reader,
        scope: &Scope,
        component: &mut Component,
        item: &intake::Owner,
        native_owner: &Payload,
    ) -> Result<()> {
        let (ordinal, owner, payload, _) = item;
        let fields = pattern::host(scope.entity, payload, *owner, *ordinal)?;
        // A host whose lifetime form the template host lacks takes a stock host of that form, as
        // a bank takes a stock bank of its shape.
        if lifetime_inputs(native_owner)? != fields.lifetime_input.is_some() {
            replace(
                native,
                component,
                &DURATIONS,
                |candidate| Ok(lifetime_inputs(candidate)? == fields.lifetime_input.is_some()),
                "No stock Duration host has the source host's lifetime form",
            )?;
        }
        let template_tag = component.template_tag;
        retarget(&mut component.payload, template_tag, component.id)?;
        component.evidence = json!({"host":owner::host(&mut component.payload, &fields)?,
            "template":template_tag});
        self.host = Some(fields);
        Ok(())
    }
}

/// Each graph numbers its provider exclusion bits across all its owners, so the network's bit
/// depends on the owners that export before it.
fn network(
    native: &mut Reader,
    scope: &Scope,
    component: &mut Component,
    counterpart: &Native,
) -> Result<()> {
    pattern::network(
        scope.source,
        native,
        scope.entity,
        component.ordinal,
        scope.choice.template.tag,
        counterpart.ordinal,
        0,
    )?;
    retarget(&mut component.payload, counterpart.tag, component.id)
}

fn receiver(
    native: &mut Reader,
    component: &mut Component,
    payload: &Payload,
    native_tag: u32,
) -> Result<()> {
    retarget(&mut component.payload, native_tag, component.id)?;
    let (evidence, allocation) = owner::receiver(native, &mut component.payload, payload)?;
    component.evidence = evidence;
    component.allocation = allocation;
    Ok(())
}

fn modifier(
    native: &mut Reader,
    scope: &Scope,
    component: &mut Component,
    owner: u32,
    native_owner: &Payload,
) -> Result<()> {
    let rows = pattern::modifiers(scope.source, scope.entity, owner)?;
    let settings = native_owner.pointer(24)?;
    let native_rows = native_owner
        .array(settings + 88, 88, Some(0x80803B06))?
        .len();
    if native_rows != rows.len() {
        // A complete standalone bank with the source row count replaces the template's, with its
        // own allocation and registrations.
        let standalone = rows.envelope()?;
        let graph = (*native.tag(standalone, Some(0x80809C0F))?).clone();
        let bank_owners = graph.array(16, 12, Some(0x80809C04))?;
        ensure!(
            bank_owners.len() == 1,
            "Native standalone modifier graph has companions"
        );
        let bank = graph.u32(bank_owners[0])?;
        component.template_tag = bank;
        component.template = (*native.tag(bank, Some(0x80809C36))?).clone();
        component.payload = component.template.clone();
        component.entity = graph;
    }
    let (id, template_tag) = (component.id, component.template_tag);
    retarget(&mut component.payload, template_tag, id)?;
    component.evidence = json!({"rows":rows.write(&mut component.payload, id)?,
        "template":template_tag});
    Ok(())
}

/// A bank whose declaration roles differ from the template bank's takes a stock bank of its
/// shape, as a modifier bank takes a standalone bank.
fn channel(
    native: &mut Reader,
    component: &mut Component,
    payload: &Payload,
    native_owner: &Payload,
) -> Result<()> {
    let wanted = owner::shape(payload, true)?;
    if owner::shape(native_owner, false)? != wanted {
        replace(
            native,
            component,
            &BANKS,
            |candidate| Ok(owner::shape(candidate, false)? == wanted),
            "No stock channel bank has the source bank's declaration roles",
        )?;
    }
    let template_tag = component.template_tag;
    retarget(&mut component.payload, template_tag, component.id)?;
    component.evidence = owner::channel(&mut component.payload, payload)?;
    Ok(())
}

fn damage(component: &mut Component, payload: &Payload) -> Result<()> {
    let template_tag = component.template_tag;
    retarget(&mut component.payload, template_tag, component.id)?;
    component.evidence = json!({"damage":owner::damage(&mut component.payload, payload)?,
        "template":template_tag});
    Ok(())
}

fn incoming_damage(
    native: &mut Reader,
    scope: &Scope,
    component: &mut Component,
    payload: &Payload,
) -> Result<()> {
    let template_tag = component.template_tag;
    retarget(&mut component.payload, template_tag, component.id)?;
    component.evidence = json!({"incoming_damage":
        incoming::write(scope.source, native, &mut component.payload, payload)?,
        "template":template_tag});
    Ok(())
}

/// The recipient graph is translated with the graphs its closure creates, and the volume names
/// its symbol.
fn condition_volume(
    native: &mut Reader,
    scope: &Scope,
    component: &mut Component,
    payload: &Payload,
) -> Result<()> {
    let template_tag = component.template_tag;
    retarget(&mut component.payload, template_tag, component.id)?;
    let recipient = volume::graph(payload)?;
    let graph_symbol = scope
        .shared
        .graph(recipient)
        .with_context(|| {
            format!("Condition volume recipient graph {recipient:08X} has no translation")
        })?
        .to_owned();
    let (evidence, field) = volume::write(scope.source, native, &mut component.payload, payload)?;
    component.patches.push((field, graph_symbol.clone()));
    component.evidence =
        json!({"volume":evidence,"template":template_tag,"recipient":graph_symbol});
    Ok(())
}

/// Replace a component's template with the first listed stock owner that `fits`, and its
/// registering entity with that owner's graph. Each list entry pairs the graph with the owner.
fn replace(
    native: &mut Reader,
    component: &mut Component,
    stock: &[(u32, u32)],
    fits: impl Fn(&Payload) -> Result<bool>,
    missing: &'static str,
) -> Result<()> {
    let mut found = None;
    for &(graph, owner) in stock {
        let candidate = (*native.tag(owner, Some(0x80809C36))?).clone();
        if fits(&candidate)? {
            found = Some((graph, owner, candidate));
            break;
        }
    }
    let (graph, owner, candidate) = found.context(missing)?;
    component.template_tag = owner;
    component.template = candidate.clone();
    component.payload = candidate;
    component.entity = (*native.tag(graph, Some(0x80809C0F))?).clone();
    Ok(())
}

/// A stock allocation belongs to its template's exact owner layout. An owner that grew, such as a
/// status with a lifetime program, takes a private copy.
fn allocate(native: &mut Reader, component: &mut Component) -> Result<()> {
    if component.allocation.is_none()
        && (component.payload.0.len() != component.template.0.len()
            || component.payload.pointer(24)? != component.template.pointer(24)?)
    {
        let tag = component.payload.u32(0x44)?;
        component.allocation = Some((tag, (*native.tag(tag, Some(0x80809BBB))?).clone()));
    }
    Ok(())
}
