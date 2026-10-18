//! Collect category requirements before allocating the bounded native dictionary.
use super::*;
use crate::d2_mot::native::{categories, effects::resources::selectors};

struct Inspection<'a> {
    names: &'a [u32],
    groups: &'a selectors::GroupBindings,
    resolver: &'a selectors::ResourceResolver,
    queries: categories::Queries,
    named: BTreeSet<u32>,
    optional: BTreeSet<u32>,
}

impl Inspection<'_> {
    fn include(&mut self, result: selectors::Inspection) {
        self.queries.required.extend(result.categories);
        self.queries.unions.extend(result.union_categories);
        self.queries.pinned.extend(result.pinned_categories);
        self.queries.groups.extend(result.category_groups);
        self.named.extend(result.named_inputs);
        self.optional.extend(result.optional_inputs);
    }

    fn selector(&mut self, p: &Payload, at: usize) -> Result<()> {
        let result = selectors::inspect_selector(p, at, self.names, self.groups, self.resolver)?;
        self.include(result);
        Ok(())
    }

    fn event_labels(&mut self, p: &Payload, fields: (usize, usize)) -> Result<()> {
        self.include(selectors::inspect_labels(
            p,
            fields,
            self.names,
            self.groups,
        )?);
        Ok(())
    }

    fn mask(&mut self, p: &Payload, at: usize) -> Result<()> {
        self.include(selectors::inspect_mask_usage(p, at, self.names)?);
        Ok(())
    }

    fn labels(&mut self, p: &Payload, at: usize) -> Result<()> {
        for index in 0..4 {
            let mut bank = BTreeSet::new();
            for row in p.array(at + index * 16, 32, Some(0x80809787))? {
                let name = p.u32(row)?;
                ensure!(
                    self.names.contains(&name),
                    "health label has no category identity"
                );
                bank.insert(name);
            }
            self.queries.required.extend(&bank);
            if index == 0 || index == 2 {
                self.queries.unions.push(bank);
            } else {
                self.queries.pinned.extend(bank);
            }
        }
        Ok(())
    }

    fn conditions(&mut self, p: &Payload, at: usize, depth: usize) -> Result<()> {
        ensure!(
            depth <= 32,
            "health category inspection exceeds condition depth"
        );
        let rows = p.array(at, 8, Some(0x80803F4F))?;
        ensure!(
            rows.len() <= 256,
            "health category inspection exceeds condition count"
        );
        for row in rows {
            let node = p.pointer(row)?;
            match p.u32(node - 4)? {
                0x80803F43 => {
                    self.labels(p, node + 40)?;
                    self.mask(p, node + 0x90)?;
                }
                0x80803F45 => {}
                0x80803F47 => self.conditions(p, node + 8, depth + 1)?,
                class => anyhow::bail!("health condition {class:08X} has no category inspection"),
            }
        }
        Ok(())
    }

    fn health(&mut self, p: &Payload, d: usize) -> Result<()> {
        for section in p.array(d + 0x210, 368, Some(0x80804091))? {
            for rule in p.array(section + 0x68, 40, Some(0x8080405C))? {
                self.conditions(p, rule + 8, 0)?;
            }
            for row in p.array(section + 0x78, 8, Some(0x80804074))? {
                let property = p.pointer(row)?;
                if p.u32(property - 4)? == 0x80804077 {
                    self.labels(p, property + 32)?;
                    self.mask(p, property + 136)?;
                }
            }
        }
        Ok(())
    }

    fn sequence(&mut self, p: &Payload, d: usize) -> Result<()> {
        for control in sequence::Sequence::read(p)?.controls {
            for condition in control.conditions.into_iter().flatten() {
                if let sequence::Condition::Category { offset } = condition {
                    self.mask(p, offset + 16)?;
                }
            }
        }
        for row in p.array(d + 0x1D8, 24, Some(0x808091F1))? {
            let event = p.pointer(row + 16)?;
            if p.u32(event - 4)? == 0x80803F24 {
                self.selector(p, event + 0x30)?;
            }
        }
        Ok(())
    }

    fn response(&mut self, p: &Payload, d: usize) -> Result<()> {
        for bit in 0..448 {
            if p.u8(d + 0x24 + bit / 8)? & (1 << (bit % 8)) != 0 {
                let name = *self
                    .names
                    .get(bit)
                    .context("response category exceeds dictionary")?;
                self.queries.required.insert(name);
                self.queries.pinned.insert(name);
            }
        }
        for row in p.array(d + 0x88, 32, Some(0x80809787))? {
            let name = p.u32(row)?;
            self.queries.required.insert(name);
            self.queries.pinned.insert(name);
        }
        self.selector(p, d + 0xC0)
    }

    fn owner(&mut self, p: &Payload) -> Result<()> {
        let d = p.pointer(24)?;
        match p.u32(d - 4)? {
            0x80803FFC => self.health(p, d),
            0x8080B11F => self.selector(p, d + 0x5F0),
            0x80803F70 => self.response(p, d),
            0x80808179 => self.sequence(p, d),
            _ => Ok(()),
        }
    }

    fn condition(
        &mut self,
        p: &Payload,
        c: &crate::d2_mot::gameplay::perks::controller::Condition,
    ) -> Result<()> {
        if c.class == 0x8080B7B3 {
            self.event_labels(p, (c.offset + 0x28, c.offset + 0x88))?;
            self.selector(p, c.offset + 0x138)?;
        } else if c.class == 0x8080B7B2 {
            self.event_labels(p, (c.offset + 8, c.offset + 0x60))?;
            self.event_labels(p, (c.offset + 0xE0, c.offset + 0x140))?;
        }
        for child in &c.children {
            self.condition(p, child)?;
        }
        Ok(())
    }

    fn action(&mut self, p: &Payload) -> Result<()> {
        for state in crate::d2_mot::gameplay::perks::controller::read(p)?.states {
            for transition in &state.transitions {
                for c in &transition.conditions {
                    self.condition(p, c)?;
                }
            }
            for effect in crate::d2_mot::gameplay::perks::translate::state_effects(p, &state)? {
                if p.u32(effect - 4)? == 0x8080B4E1 {
                    self.event_labels(p, (effect + 0x48, effect + 0xA8))?;
                    self.event_labels(p, (effect + 0xE0, effect + 0x138))?;
                }
            }
        }
        Ok(())
    }
}

pub(super) fn prepare(
    source: &Reader,
    native: &mut Reader,
    closure: &closure::Closure,
    tags: &BTreeMap<u32, u32>,
) -> Result<(
    u32,
    categories::Namespace,
    sequence::Selectors,
    BTreeSet<u32>,
)> {
    let dictionaries = closure
        .resources
        .iter()
        .filter(|(_, resource)| resource.class == categories::MODERN_CLASS)
        .collect::<Vec<_>>();
    ensure!(
        dictionaries.len() == 1,
        "attachment closure must use one category namespace"
    );
    let (&dictionary, resource) = dictionaries[0];
    let stock = native.tag(0x80C70CA1, Some(categories::NATIVE_CLASS))?;
    let initial = categories::emit_static(&resource.payload, &stock, &BTreeSet::new())?;
    let groups = selectors::GroupBindings::from_namespace(&initial);
    let resolver = selectors::ResourceResolver {
        hashes: source
            .manager
            .lookup
            .tag64_entries
            .iter()
            .map(|(&hash, value)| (hash, value.hash32.0))
            .collect(),
        source_classes: closure
            .resources
            .iter()
            .map(|(&tag, resource)| (tag, resource.class))
            .collect(),
        native_classes: closure
            .resources
            .iter()
            .filter_map(|(&tag, resource)| {
                let class = match resource.class {
                    selectors::MODERN_CLASS => selectors::NATIVE_CLASS,
                    categories::MODERN_CLASS => categories::NATIVE_CLASS,
                    _ => return None,
                };
                tags.get(&tag).map(|&target| (target, class))
            })
            .collect(),
    };
    let mut inspection = Inspection {
        names: &initial.source_names,
        groups: &groups,
        resolver: &resolver,
        queries: categories::Queries::default(),
        named: BTreeSet::new(),
        optional: BTreeSet::new(),
    };
    for resource in closure.resources.values() {
        let p = &resource.payload;
        match resource.class {
            selectors::MODERN_CLASS => inspection.selector(p, 8)?,
            controller::health::policy::SOURCE_CLASS => {
                for row in p.array(8, 40, Some(0x80804032))? {
                    inspection.mask(p, row + 16)?;
                }
            }
            _ => {}
        }
    }
    for p in closure.owners.values() {
        inspection.owner(p)?;
    }
    for p in closure.actions.values() {
        inspection.action(p)?;
    }
    let queries = inspection.queries;
    let named = inspection.named;
    let optional = inspection.optional;
    write_json(
        &source.output.join("category-requirements.json"),
        &json!({
            "dictionary":dictionary,"native_count":initial.names.len(),
            "source_count":initial.source_names.len(),"required":queries.required,
            "additional":queries.required.iter().filter(|name| !initial.names.contains(name)).collect::<Vec<_>>(),
            "capacity":320,"union_banks":queries.unions,"pinned":queries.pinned,"source_groups":queries.groups
        }),
    )?;
    let namespace = categories::emit_queries(&resource.payload, &stock, &queries)?;
    if !namespace.query_aliases.is_empty() {
        // The graph assembler admits only known component classes. Keep this
        // additional guard if category publishers are supported there later.
        for p in closure.owners.values() {
            ensure!(
                p.u32(p.pointer(24)? - 4)? != 0x80809775,
                "compacted query categories cannot have an imported publisher"
            );
        }
    }
    write_json(
        &source.output.join("category-namespace.json"),
        &json!({
            "native_atoms":namespace.names.len(),"added_atoms":namespace.added_static,
            "query_aliases":namespace.query_aliases,"group_aliases":namespace.group_aliases,
            "limit":"Read-only union predicates. No added native network or mutable category capacity."
        }),
    )?;
    let resources = selectors::Resources::from_namespace(&namespace, tags.clone());
    let groups = selectors::GroupBindings::from_namespace(&namespace);
    // Requirements are returned separately from publication. Assembly fills the
    // published set only after following the providers in each containing graph.
    ensure!(
        named.iter().all(|name| !matches!(*name, 0 | 0x811C9DC5)),
        "selector has an empty named input"
    );
    Ok((
        dictionary,
        namespace,
        sequence::Selectors {
            resources,
            groups,
            resolver,
            published_names: BTreeSet::new(),
            optional_names: optional,
        },
        named,
    ))
}
