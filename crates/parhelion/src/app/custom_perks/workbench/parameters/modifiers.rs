//! Change Weapon Properties: the modifier rows of an attachment, every one started neutral,
//! and the rows an author sets. Both are written as the card's rows write them, so an author's
//! later edit of a row replaces its neutral value rather than overlapping it.
use super::native::{carrier, write_value};
use super::*;
use sundial::package_authoring::sandbox_perk::entity::modifiers::{self, Modifier};

/// The modifier rows of every graph in `loaded`, each with the graph it is on.
pub(crate) fn rows(loaded: &PrivatePerkRuntimeGraph) -> Vec<(u32, Modifier)> {
    loaded
        .graphs
        .iter()
        .flat_map(|(tag, graph)| {
            modifiers::discover(graph)
                .into_iter()
                .map(move |modifier| (*tag, modifier))
        })
        .collect()
}

/// Overrides that start every row of `loaded` neutral: nothing added, or multiplied by one.
pub(crate) fn neutral(
    loaded: &PrivatePerkRuntimeGraph,
) -> Result<Vec<WeaponRuntimeValueOverride>, String> {
    let mut draft = Vec::new();
    for (graph, modifier) in rows(loaded) {
        if modifier.is_neutral() {
            continue;
        }
        let amount = WeaponRuntimeValue::Float32Bits(modifier.neutral().to_bits());
        write(
            loaded,
            graph,
            &modifier,
            &modifier.amount,
            amount,
            &mut draft,
        )?;
    }
    Ok(draft)
}

/// Writes one field of a row into `draft`, as the card's rows do.
fn write(
    loaded: &PrivatePerkRuntimeGraph,
    graph: u32,
    modifier: &Modifier,
    field: &WeaponRuntimeField,
    value: WeaponRuntimeValue,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
) -> Result<(), String> {
    let graph = loaded
        .graphs
        .iter()
        .find(|(tag, _)| *tag == graph)
        .map(|(_, graph)| graph)
        .ok_or_else(|| format!("Graph 0x{graph:08X} is not loaded."))?;
    write_value(
        loaded,
        field,
        carrier(graph, modifier.owner_tag, field),
        draft,
        &value,
    )
}

/// One field of a row.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Part {
    Amount,
    Operation,
    Input,
    Component,
}

#[cfg(test)]
fn field(modifier: &Modifier, part: Part) -> &WeaponRuntimeField {
    match part {
        Part::Amount => &modifier.amount,
        Part::Operation => &modifier.operation,
        Part::Input => &modifier.input,
        Part::Component => &modifier.component,
    }
}

/// Change Weapon Properties' rows for one entity, read from the packages: the loaded graph,
/// its rows and the overrides that start every row neutral. The package tests author rows
/// through it as the card does.
#[cfg(test)]
pub(crate) struct WeaponProperties {
    pub(crate) loaded: PrivatePerkRuntimeGraph,
    pub(crate) rows: Vec<(u32, Modifier)>,
    pub(crate) neutral: Vec<WeaponRuntimeValueOverride>,
}

#[cfg(test)]
pub(crate) fn weapon_properties(packages: &Path, graph: u32) -> Result<WeaponProperties, String> {
    let loaded = load_entity_parameters(packages, graph)?;
    let rows = rows(&loaded);
    let neutral = neutral(&loaded)?;
    Ok(WeaponProperties {
        loaded,
        rows,
        neutral,
    })
}

#[cfg(test)]
impl WeaponProperties {
    /// The entity's components, one line each: every bound resource by its label and class,
    /// then every shared owner root by its schema, named where the native type is known.
    pub(crate) fn components(&self) -> Vec<String> {
        use sundial::package_authoring::runtime::native_type_name;
        let mut lines = std::collections::BTreeSet::new();
        for (_, graph) in &self.loaded.graphs {
            for resource in &graph.resources {
                lines.insert(format!(
                    "{} 0x{:08X} {}",
                    resource.binding_label,
                    resource.concrete_class,
                    native_type_name(resource.concrete_class).unwrap_or("?")
                ));
            }
            for owner in &graph.owners {
                for root in &owner.roots {
                    lines.insert(format!(
                        "owner 0x{:08X} root 0x{:08X} {}",
                        owner.owner_tag,
                        root.schema,
                        native_type_name(root.schema).unwrap_or("?")
                    ));
                }
            }
        }
        lines.into_iter().collect()
    }

    /// Writes one field of row `index` into `draft`.
    pub(crate) fn set(
        &self,
        index: usize,
        part: Part,
        value: WeaponRuntimeValue,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
    ) -> Result<(), String> {
        let (graph, modifier) = self.rows.get(index).ok_or("No such row.")?;
        write(
            &self.loaded,
            *graph,
            modifier,
            field(modifier, part),
            value,
            draft,
        )
    }

    /// One field of row `index` as the card shows it with `draft` applied.
    pub(crate) fn value(
        &self,
        index: usize,
        part: Part,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<WeaponRuntimeValue, String> {
        let (graph, modifier) = self.rows.get(index).ok_or("No such row.")?;
        let field = field(modifier, part);
        let loaded = self
            .loaded
            .graphs
            .iter()
            .find(|(tag, _)| tag == graph)
            .map(|(_, graph)| graph)
            .ok_or_else(|| format!("Graph 0x{graph:08X} is not loaded."))?;
        super::native::current_value(
            &self.loaded,
            field,
            carrier(loaded, modifier.owner_tag, field),
            draft,
        )
    }
}
