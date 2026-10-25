//! Private gameplay resources are reserved before any recursive reference is emitted.
use super::*;
use crate::d2_mot::native::{
    categories,
    effects::resources::{self, selectors},
};

pub(super) struct Plan {
    pub tags: BTreeMap<u32, u32>,
    pub classes: BTreeMap<u32, u32>,
    allocations: BTreeMap<u32, u32>,
    pending: BTreeSet<u32>,
    silent: BTreeSet<u32>,
}

fn class(source: u32) -> Option<u32> {
    Some(match source {
        categories::MODERN_CLASS => categories::NATIVE_CLASS,
        selectors::MODERN_CLASS => selectors::NATIVE_CLASS,
        resources::MODERN_CLASS => resources::NATIVE_CLASS,
        resources::slot::MODERN_CLASS => resources::slot::NATIVE_CLASS,
        controller::health::policy::SOURCE_CLASS => controller::health::policy::NATIVE_CLASS,
        0x808031DE => 0x80803EB0,
        0x808031D8 => 0x80803EAA,
        0x808031C2 => 0x80803E94,
        0x808031D3 => 0x80803EA5,
        0x808031C7 => 0x80803E99,
        _ => return None,
    })
}

impl Plan {
    pub fn reserve(
        native: &Reader,
        closure: &closure::Closure,
        assets: &mut Assets,
        graphs: &BTreeMap<u32, graph::Plan>,
    ) -> Result<Self> {
        let mut output = Self {
            tags: graphs
                .iter()
                .map(|(&tag, graph)| (tag, graph.target))
                .collect(),
            classes: graphs.keys().map(|&tag| (tag, 0x80809C0F)).collect(),
            allocations: BTreeMap::new(),
            pending: BTreeSet::new(),
            silent: BTreeSet::new(),
        };
        for (&tag, resource) in &closure.resources {
            if let Some(class) = class(resource.class) {
                output.reserve_one(native, assets, tag, class)?;
            }
        }
        for (&tag, owner) in &closure.owners {
            if owner.u32(owner.pointer(24)? - 4)? == 0x80803F70 {
                output.reserve_one(native, assets, tag, 0x80809C36)?;
                output.allocations.insert(
                    tag,
                    assets.reserve(native, format!("attachment-response-allocation-{tag:08X}"))?,
                );
            }
        }
        Ok(output)
    }

    fn reserve_one(
        &mut self,
        native: &Reader,
        assets: &mut Assets,
        tag: u32,
        class: u32,
    ) -> Result<()> {
        ensure!(
            !self.tags.contains_key(&tag),
            "attachment resource is already reserved"
        );
        self.tags.insert(
            tag,
            assets.reserve(native, format!("attachment-resource-{tag:08X}"))?,
        );
        self.classes.insert(tag, class);
        self.pending.insert(tag);
        Ok(())
    }

    /// Presentation conversion supplies its checked result through this same
    /// correspondence. It cannot replace a reserved gameplay dependency.
    pub fn bind(&mut self, source: u32, target: u32, class: u32) -> Result<()> {
        ensure!(
            !self.tags.contains_key(&source),
            "attachment dependency was bound twice"
        );
        self.tags.insert(source, target);
        self.classes.insert(source, class);
        Ok(())
    }

    pub fn omit_sounds(&mut self, skipped: &BTreeMap<u32, u32>) {
        self.silent.extend(skipped.keys());
    }

    fn table(
        &self,
        tag: u32,
        result: resources::Table,
        closure: &closure::Closure,
        report: &mut Vec<Value>,
    ) -> Result<Payload> {
        for reference in result.references {
            if reference.target.is_some() {
                continue;
            }
            ensure!(
                self.silent.contains(&reference.source)
                    && closure
                        .resources
                        .get(&reference.source)
                        .is_some_and(|resource| resource.class == 0x80809738)
                    && result.payload.u32(reference.offset)? == u32::MAX,
                "attachment response table {tag:08X} has an unbound leaf {:08X} at {:X}",
                reference.source,
                reference.offset
            );
            report.push(json!({"source":tag,"native_offset":reference.offset,
                "cue":reference.source,"unsupported_hirc_type":22,
                "difference":"A material response sound is omitted because its bank contains an unsupported audio object."}));
        }
        Ok(result.payload)
    }

    pub fn bindings(
        &self,
        closure: &closure::Closure,
    ) -> Result<BTreeMap<u32, BTreeMap<usize, sequence::Resource>>> {
        let mut result = BTreeMap::new();
        for (&owner, fields) in &closure.references {
            let mut bindings = BTreeMap::new();
            for (&field, &source) in fields {
                let Some(&target) = self.tags.get(&source) else {
                    continue;
                };
                let source_class = if closure.graphs.contains_key(&source) {
                    0x80809AD8
                } else if closure.owners.contains_key(&source) {
                    0x80809B06
                } else if closure.actions.contains_key(&source) {
                    0x8080B835
                } else {
                    closure
                        .resources
                        .get(&source)
                        .context("undeclared attachment dependency")?
                        .class
                };
                bindings.insert(
                    field,
                    sequence::Resource {
                        source_class,
                        native_class: self.classes[&source],
                        tag: target,
                    },
                );
            }
            result.insert(owner, bindings);
        }
        Ok(result)
    }

    pub fn emit(
        &self,
        native: &mut Reader,
        closure: &closure::Closure,
        assets: &mut Assets,
        dictionary: u32,
        namespace: &categories::Namespace,
        selectors: &sequence::Selectors,
    ) -> Result<Vec<Value>> {
        let mut report = Vec::new();
        for &tag in &self.pending {
            let target = self.tags[&tag];
            let class = self.classes[&tag];
            if let Some(source) = closure.owners.get(&tag) {
                let profile = native.tag(0x8152949F, None)?;
                let template_tag = profile.u32(profile.pointer(24)? + 0x118)?;
                let template = native.tag(template_tag, None)?;
                let allocation_tag = template.u32(0x44)?;
                let allocation = native.tag(allocation_tag, None)?;
                let result = controller::response::emit_with_bindings(
                    source,
                    &template,
                    &allocation,
                    &controller::response::Resources {
                        tags: self.tags.clone(),
                        hashes: selectors.resolver.hashes.clone(),
                        categories: namespace.source_indices.clone(),
                        names: namespace.source_names.clone(),
                    },
                    target,
                    self.allocations[&tag],
                    &selectors.groups,
                    &selectors.resolver,
                )?;
                ensure!(
                    result.gates.is_empty()
                        && result.references.iter().all(|r| r.target.is_some())
                        && result
                            .named_conditions
                            .iter()
                            .all(|name| selectors.accepts_name(name)),
                    "attachment response {tag:08X} has unresolved semantics or dependencies"
                );
                assets.push(
                    self.allocations[&tag],
                    allocation_tag,
                    result.allocation,
                    None,
                )?;
                assets.push(
                    target,
                    template_tag,
                    result.owner,
                    (native.reference(template_tag)? == allocation_tag)
                        .then_some(self.allocations[&tag]),
                )?;
                report.push(json!({"source":tag,"target":target,"class":class}));
                continue;
            }
            let source = &closure.resources[&tag];
            let payload = match source.class {
                categories::MODERN_CLASS => {
                    ensure!(tag == dictionary, "attachment namespace identity changed");
                    namespace.payload.clone()
                }
                selectors::MODERN_CLASS => {
                    let result = selectors::emit_with_bindings(
                        &source.payload,
                        &selectors.resources,
                        &selectors.resolver,
                        &selectors.groups,
                    )?;
                    ensure!(
                        result.gates.is_empty()
                            && result.references.iter().all(|r| r.target.is_some())
                            && result
                                .named_conditions
                                .iter()
                                .all(|name| selectors.accepts_name(name)),
                        "attachment selector {tag:08X} is unbound"
                    );
                    result.payload
                }
                controller::health::policy::SOURCE_CLASS => {
                    controller::health::policy::emit(&source.payload, &selectors.resources)?
                }
                resources::MODERN_CLASS | resources::slot::MODERN_CLASS => {
                    let result = if source.class == resources::MODERN_CLASS {
                        resources::emit(&source.payload, &self.tags, &selectors.resolver.hashes)?
                    } else {
                        let mut slot = source.payload.clone();
                        let scalar = slot.f32(0x2C)?;
                        ensure!(
                            scalar.is_finite(),
                            "material response audio scalar is nonfinite"
                        );
                        if slot.u32(0x2C)? != 0 {
                            report.push(json!({"source":tag,"source_scalar_bits":slot.u32(0x2C)?,
                                "difference":"The additional modern material-response audio scalar has no native field and is omitted."}));
                            put(&mut slot, 0x2C, &[0; 4])?;
                        }
                        resources::slot::emit(&slot, &selectors.resolver.hashes, &self.tags)?
                    };
                    self.table(tag, result, closure, &mut report)?
                }
                _ => {
                    let result =
                        resources::programs::emit(&source.payload, source.class, &self.tags)?;
                    ensure!(
                        result.class == class
                            && result.references.iter().all(|r| r.target.is_some()),
                        "attachment program {tag:08X} has an unbound reference or class"
                    );
                    result.payload
                }
            };
            let template = native
                .classes(class)
                .into_iter()
                .min()
                .with_context(|| format!("no native resource template for {class:08X}"))?;
            assets.push(target, template, payload, None)?;
            report.push(json!({"source":tag,"target":target,"class":class}));
        }
        Ok(report)
    }
}
