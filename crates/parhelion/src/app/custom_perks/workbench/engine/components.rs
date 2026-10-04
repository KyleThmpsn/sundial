//! Component classes used by installed objects and perk entities.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use sundial::package_authoring::runtime::{self, NativeMember};
use sundial::ui::catalog::BrowserList;

struct Resource {
    tag: u32,
    name: String,
    package: String,
}

struct Row {
    class: u32,
    name: String,
    fields: Vec<String>,
    /// The members this class declares, and the members of other types that hold one.
    members: Vec<NativeMember>,
    holders: Vec<NativeMember>,
    bindings: BTreeSet<u32>,
    resources: Vec<Resource>,
    search: String,
}

#[derive(Default)]
pub(super) struct Browser {
    query: String,
    source: Option<(usize, usize, usize)>,
    rows: Vec<Row>,
    classes: BTreeSet<u32>,
    /// Every class each binding selects, named classes first.
    shared: BTreeMap<u32, Vec<u32>>,
    matches: Vec<usize>,
    filtered_query: Option<String>,
    /// A class a link chose, selected on the next frame.
    reveal: Option<u32>,
}

/// What the detail pane asked for.
enum Choice {
    Resource(u32),
    Class(u32),
}

impl Browser {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui, data: &discovery::Data) -> Option<u32> {
        let source = (
            Arc::as_ptr(&data.effects) as usize,
            Arc::as_ptr(&data.perks) as usize,
            Arc::as_ptr(&data.names) as usize,
        );
        if self.source != Some(source) {
            self.rows = rows(data);
            self.classes = self.rows.iter().map(|row| row.class).collect();
            self.shared = shared(&self.rows);
            self.source = Some(source);
            self.filtered_query = None;
        }
        // A link to a class the search hides clears the search, so the class can be shown.
        if let Some(class) = self.reveal
            && !self
                .matches
                .iter()
                .any(|&index| self.rows.get(index).is_some_and(|row| row.class == class))
        {
            self.query.clear();
        }
        let width = (ui.available_width() - 120.0).max(160.0);
        let changed = sundial::ui::catalog::search(
            ui,
            &mut self.query,
            false,
            width,
            "Search Component Types, Fields, or Resources",
        );
        let query = self.query.trim().to_ascii_lowercase();
        if self.filtered_query.as_ref() != Some(&query) {
            self.matches = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    query
                        .split_whitespace()
                        .all(|word| row.search.contains(word.strip_prefix("0x").unwrap_or(word)))
                })
                .map(|(index, _)| index)
                .collect();
            self.filtered_query = Some(query);
        }
        ui.weak(format!(
            "{} {}",
            self.matches.len(),
            if self.matches.len() == 1 {
                "Component Type"
            } else {
                "Component Types"
            }
        ));
        ui.separator();
        let keys = self
            .matches
            .iter()
            .map(|&index| u64::from(self.rows[index].class))
            .collect::<Vec<_>>();
        let choice = BrowserList {
            keys: &keys,
            height: ui.available_height().max(120.0),
            reset: changed,
            row_height: sundial::investment::authoring_choice_row_height(ui),
            select: self.reveal.take().map(u64::from),
        }
        .draw_body(
            ui,
            |ui, index, selected| {
                let row = &self.rows[self.matches[index]];
                sundial::investment::draw_asset_choice_row_plain(
                    ui,
                    &row.name,
                    &format!("{} Resources · 0x{:08X}", row.resources.len(), row.class),
                    selected,
                )
            },
            |ui, index| {
                draw_detail(
                    ui,
                    &self.rows[self.matches[index]],
                    &self.classes,
                    &self.shared,
                )
            },
        );
        match choice {
            Some(Choice::Resource(tag)) => Some(tag),
            Some(Choice::Class(class)) => {
                self.reveal = Some(class);
                ui.ctx().request_repaint();
                None
            }
            None => None,
        }
    }
}

fn draw_detail(
    ui: &mut egui::Ui,
    row: &Row,
    classes: &BTreeSet<u32>,
    shared: &BTreeMap<u32, Vec<u32>>,
) -> Option<Choice> {
    ui.heading(&row.name);
    ui.monospace(format!("Component Type 0x{:08X}", row.class));
    if ui.button("Copy Component Type").clicked() {
        ui.ctx().copy_text(format!("0x{:08X}", row.class));
    }
    if !row.fields.is_empty() {
        ui.strong("Known Fields");
        for field in &row.fields {
            ui.label(field);
        }
    }
    let mut choice = draw_structure(ui, row, classes).map(Choice::Class);
    draw_bindings(ui, row, shared);
    ui.strong(format!("Resources ({})", row.resources.len()));
    let height = sundial::investment::authoring_choice_row_height(ui);
    egui::ScrollArea::vertical()
        .id_salt(("component-resources", row.class))
        .max_height(240.0)
        .show_rows(ui, height, row.resources.len(), |ui, range| {
            for resource in &row.resources[range] {
                let label = if resource.name.starts_with("Resource 0x") {
                    resource.name.clone()
                } else {
                    format!("{} · 0x{:08X}", resource.name, resource.tag)
                };
                if ui.button(label).on_hover_text(&resource.package).clicked() {
                    choice = Some(Choice::Resource(resource.tag));
                }
            }
        });
    choice
}

/// What the class holds and what holds it, each linking to the component type it names.
fn draw_structure(ui: &mut egui::Ui, row: &Row, classes: &BTreeSet<u32>) -> Option<u32> {
    let mut chosen = None;
    if !row.members.is_empty() {
        ui.strong("Contains");
        for member in &row.members {
            ui.horizontal(|ui| {
                ui.monospace(format!("+0x{:X}", member.byte_offset));
                ui.label(member_label(member));
                if !member.scalar {
                    chosen = type_link(ui, member.type_handle, classes).or(chosen);
                }
            });
        }
    }
    if !row.holders.is_empty() {
        ui.strong("Part Of");
        for holder in &row.holders {
            ui.horizontal(|ui| {
                chosen = type_link(ui, holder.holder, classes).or(chosen);
                ui.weak(member_label(holder));
            });
        }
    }
    chosen
}

/// Each binding with the other component types that also register under it.
fn draw_bindings(ui: &mut egui::Ui, row: &Row, shared: &BTreeMap<u32, Vec<u32>>) {
    if row.bindings.is_empty() {
        return;
    }
    ui.strong("Bindings");
    for &binding in &row.bindings {
        let others = shared
            .get(&binding)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&class| class != row.class)
            .collect::<Vec<_>>();
        ui.horizontal(|ui| {
            ui.label(runtime::component_binding_label(binding))
                .on_hover_text(format!("Binding 0x{binding:08X}"));
            let note = ui.weak(sharing(&others));
            if !others.is_empty() {
                note.on_hover_text(sharing_list(&others));
            }
        });
    }
}

/// A type's name, or its hash when nothing names it.
fn type_label(class: u32, component: bool) -> String {
    runtime::native_type_name(class).map_or_else(
        || {
            if component {
                format!("Component Type 0x{class:08X}")
            } else {
                format!("Type 0x{class:08X}")
            }
        },
        str::to_owned,
    )
}

/// A link to a component type this browser lists, or plain text for any other type.
fn type_link(ui: &mut egui::Ui, class: u32, classes: &BTreeSet<u32>) -> Option<u32> {
    if classes.contains(&class) {
        ui.link(type_label(class, true)).clicked().then_some(class)
    } else {
        ui.weak(type_label(class, false));
        None
    }
}

fn member_label(member: &NativeMember) -> String {
    match &member.name {
        Some(name) if member.inferred => format!("{name} (inferred)"),
        Some(name) => name.clone(),
        None => format!("Member 0x{:08X}", member.name_hash),
    }
}

/// Which other component types a binding also selects, the most used named ones first.
fn sharing(others: &[u32]) -> String {
    if others.is_empty() {
        return "Only This Type".into();
    }
    let mut named = Vec::new();
    for name in others
        .iter()
        .filter_map(|&class| runtime::native_type_name(class))
    {
        if named.len() == 2 {
            break;
        }
        if !named.contains(&name) {
            named.push(name);
        }
    }
    if named.is_empty() {
        return match others.len() {
            1 => "Shared with 1 Type".into(),
            count => format!("Shared with {count} Types"),
        };
    }
    let listed = others
        .iter()
        .filter(|&&class| {
            runtime::native_type_name(class).is_some_and(|name| named.contains(&name))
        })
        .count();
    match others.len() - listed {
        0 => format!("Shared with {}", named.join(", ")),
        rest => format!("Shared with {} +{rest}", named.join(", ")),
    }
}

fn sharing_list(others: &[u32]) -> String {
    let mut lines = others
        .iter()
        .take(12)
        .map(|&class| type_label(class, true))
        .collect::<Vec<_>>();
    if others.len() > 12 {
        lines.push(format!("+{} More", others.len() - 12));
    }
    lines.join("\n")
}

/// Every class each binding selects, named and most used classes first, so a binding reads by
/// what it shares a role with.
fn shared(rows: &[Row]) -> BTreeMap<u32, Vec<u32>> {
    let mut shared = BTreeMap::<u32, Vec<u32>>::new();
    let mut uses = BTreeMap::new();
    for row in rows {
        uses.insert(row.class, row.resources.len());
        for &binding in &row.bindings {
            shared.entry(binding).or_default().push(row.class);
        }
    }
    for classes in shared.values_mut() {
        classes.sort_by_key(|&class| {
            (
                runtime::native_type_name(class).is_none(),
                std::cmp::Reverse(uses.get(&class).copied().unwrap_or(0)),
                class,
            )
        });
    }
    shared
}

fn rows(data: &discovery::Data) -> Vec<Row> {
    let mut classes: BTreeMap<u32, (BTreeSet<u32>, BTreeMap<u32, Resource>)> = BTreeMap::new();
    let mut add = |class: u32, binding: u32, tag: u32, package: &str, known: Option<&str>| {
        if class == 0 {
            return;
        }
        let name = known
            .map(sundial::package_authoring::tft::asset_label)
            .unwrap_or_else(|| format!("Resource 0x{tag:08X}"));
        let (bindings, resources) = classes.entry(class).or_default();
        bindings.insert(binding);
        resources.entry(tag).or_insert(Resource {
            tag,
            name,
            package: package.to_owned(),
        });
    };
    for (&tag, owner) in &data.effects.owners {
        for component in &owner.components {
            add(
                component.class,
                component.binding,
                tag,
                &owner.package,
                owner.names.first().map(String::as_str),
            );
        }
    }
    for entity in data
        .perks
        .patterns
        .iter()
        .filter_map(|pattern| pattern.entity.as_ref())
        .chain(data.perks.perks.iter().flat_map(|perk| &perk.graphs))
    {
        for component in &entity.components {
            let package = data
                .effects
                .owners
                .get(&component.owner)
                .map_or("", |owner| owner.package.as_str());
            add(
                component.class,
                component.binding,
                component.owner,
                package,
                None,
            );
        }
    }
    let wanted = classes
        .values()
        .flat_map(|(_, resources)| resources.keys().copied())
        .collect::<BTreeSet<_>>();
    let mut paths: BTreeMap<u32, &str> = BTreeMap::new();
    for (tag, path) in data
        .names
        .references
        .iter()
        .map(|reference| (reference.target, reference.path.as_str()))
        .chain(
            data.names
                .paths
                .iter()
                .map(|path| (path.source, path.path.as_str())),
        )
    {
        if wanted.contains(&tag) && paths.get(&tag).is_none_or(|kept| path.len() < kept.len()) {
            paths.insert(tag, path);
        }
    }
    for (_, resources) in classes.values_mut() {
        for resource in resources.values_mut() {
            if resource.name.starts_with("Resource 0x")
                && let Some(path) = paths.get(&resource.tag)
            {
                resource.name = sundial::package_authoring::tft::asset_label(path);
            }
        }
    }
    let mut rows = classes
        .into_iter()
        .map(|(class, (bindings, resources))| {
            let name = runtime::native_type_name(class)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Component Type 0x{class:08X}"));
            let fields = runtime::native_member_names(class);
            let members = runtime::native_members(class);
            let holders = runtime::native_holders(class);
            let mut resources = resources.into_values().collect::<Vec<_>>();
            resources
                .sort_by_cached_key(|resource| (resource.name.to_ascii_lowercase(), resource.tag));
            let search = format!(
                "{} {:08x} {} {} {} {}",
                name,
                class,
                fields.join(" "),
                members
                    .iter()
                    .chain(&holders)
                    .filter_map(|member| member.name.as_deref())
                    .collect::<Vec<_>>()
                    .join(" "),
                bindings
                    .iter()
                    .map(|binding| format!(
                        "{} {:08x}",
                        runtime::component_binding_label(*binding),
                        binding
                    ))
                    .collect::<Vec<_>>()
                    .join(" "),
                resources
                    .iter()
                    .map(|resource| format!(
                        "{} {} {:08x}",
                        resource.name, resource.package, resource.tag
                    ))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .to_ascii_lowercase();
            Row {
                class,
                name,
                fields,
                members,
                holders,
                bindings,
                resources,
                search,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by_cached_key(|row| {
        (
            row.name.starts_with("Component Type 0x"),
            row.name.to_ascii_lowercase(),
            row.class,
        )
    });
    rows
}
