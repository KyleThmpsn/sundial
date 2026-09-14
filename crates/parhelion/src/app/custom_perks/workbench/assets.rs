//! The asset picker shared by spawn, attach and pattern actions, and the native fields an
//! attach node carries beside its asset.
use super::*;
use sundial::package_authoring::sandbox_perk::program::{Asset, EMPTY_KEY};

/// Which catalog entries an action may reference.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AssetScope {
    /// A pattern override needs a projectile.
    Projectiles,
    /// A spawn accepts a projectile, emitter, pickup or physical world object.
    Spawnable,
    /// An attach accepts any entity graph of a type stock perks attach.
    Any,
}

impl AssetScope {
    fn placement_hint(self, kind: projectile::Kind) -> &'static str {
        match self {
            Self::Projectiles => "Fired by the weapon in place of its current projectile.",
            Self::Spawnable if kind == projectile::Kind::Projectile => {
                "Starts at Spawn Location and uses the asset's native motion."
            }
            Self::Spawnable => "Created at Spawn Location.",
            Self::Any => "Attached to the weapon for the effect's duration.",
        }
    }

    fn picker_labels(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Projectiles => (
                "Choose Projectile…",
                "Choose a Projectile",
                "Use Projectile",
            ),
            Self::Spawnable => (
                "Choose Object or Effect…",
                "Choose an Object or Effect to Spawn",
                "Use for Spawn",
            ),
            Self::Any => (
                "Choose Attachment…",
                "Choose an Attachment",
                "Use Attachment",
            ),
        }
    }

    fn allows(self, kind: projectile::Kind) -> bool {
        match self {
            Self::Projectiles => kind == projectile::Kind::Projectile,
            Self::Spawnable => kind.spawnable(),
            Self::Any => true,
        }
    }
}

fn asset_usage(
    entry: &projectile::catalog::Entry,
    perk_names: &BTreeMap<u16, String>,
    discovery: &discovery::Discovery,
) -> String {
    let mut uses = BTreeSet::new();
    for index in entry
        .perk_indices
        .iter()
        .copied()
        .chain(entry.contexts.iter().filter_map(|context| context.perk))
    {
        if let Some(name) = perk_names.get(&index) {
            let behavior = discovery
                .behavior(index)
                .map(|behavior| behavior.headline.as_str())
                .unwrap_or_default();
            uses.insert(format!("Referenced by {name}: {behavior}"));
        }
    }
    if uses.len() > 1 {
        entry.source_hint.clone().unwrap_or_else(|| {
            "Shared by multiple perks. No common behavior has been established.".into()
        })
    } else {
        uses.into_iter().collect::<Vec<_>>().join("\n")
    }
}

impl Workbench {
    pub(super) fn draw_asset_picker(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        asset: &mut Asset,
        scope: AssetScope,
    ) {
        self.refresh_asset_labels();
        let (empty_label, title, use_label) = scope.picker_labels();
        let label = self
            .asset_labels
            .get(&asset.graph)
            .cloned()
            .unwrap_or_else(|| {
                if asset.graph == 0 {
                    empty_label.into()
                } else {
                    format!("Unidentified Effect · 0x{:08X}", asset.graph)
                }
            });
        let picked = pickers::browser_with_toolbar(
            ui,
            "program-asset",
            &label,
            title,
            &mut self.asset_query,
            |ui, query, reset, _height| {
                Browser {
                    catalog,
                    discovery: &self.discovery,
                    perk_names: &self.perk_names,
                    item_names: &self.item_names,
                    asset_labels: &self.asset_labels,
                }
                .draw(ui, scope, query, reset, Some(use_label))
            },
        );
        if let Some(picked) = picked
            && picked.graph != asset.graph
        {
            *asset = picked;
        }
    }
}

/// Shared asset list, filters and details for selection and Engine Catalog inspection.
pub(super) struct Browser<'a> {
    pub catalog: Option<&'a InvestmentCatalog>,
    pub discovery: &'a discovery::Discovery,
    pub perk_names: &'a BTreeMap<u16, String>,
    pub item_names: &'a BTreeMap<u32, projectile::catalog::ItemName>,
    pub asset_labels: &'a BTreeMap<u32, String>,
}

impl Browser<'_> {
    pub fn draw(
        &self,
        ui: &mut egui::Ui,
        scope: AssetScope,
        query: &mut String,
        reset: bool,
        use_label: Option<&str>,
    ) -> Option<Asset> {
        if let Some(error) = &self.discovery.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let Some(data) = &self.discovery.data else {
            if self.discovery.busy() {
                ui.spinner();
                ui.label("Reading Native Assets…");
                if let Some((current, total)) = self.discovery.progress {
                    ui.small(format!("{current} of {total} resources"));
                }
            }
            return None;
        };
        let filter_id = ui.make_persistent_id("asset-kind");
        let mut filter = ui.data(|state| state.get_temp::<u8>(filter_id).unwrap_or(0));
        let before = filter;
        let mut visibility = (false, false);
        let mut search_changed = reset;
        let count_rect = ui
            .horizontal(|ui| {
                let reserved = if scope == AssetScope::Projectiles {
                    240.0
                } else {
                    380.0
                };
                let width = (ui.available_width() - reserved).max(100.0);
                search_changed |= pickers::search(ui, query, reset, width);
                if scope != AssetScope::Projectiles {
                    let types = [
                        (0, "All Types"),
                        (1, "Projectiles"),
                        (2, "Emitters"),
                        (3, "Other Entities"),
                        (4, "Pickups"),
                        (5, "World Objects"),
                    ];
                    egui::ComboBox::from_id_salt("asset-type-filter")
                        .width(130.0)
                        .selected_text(
                            types
                                .iter()
                                .find(|(value, _)| *value == filter)
                                .map_or("All Types", |(_, label)| *label),
                        )
                        .show_ui(ui, |ui| {
                            for (value, name) in types {
                                if value != 3 || scope == AssetScope::Any {
                                    ui.selectable_value(&mut filter, value, name);
                                }
                            }
                        });
                }
                visibility = pickers::show_all(ui);
                ui.allocate_exact_size(
                    egui::vec2(86.0, ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                )
                .0
            })
            .inner;
        ui.data_mut(|state| state.insert_temp(filter_id, filter));
        let normalized_query = query.trim().to_lowercase();
        let exact_graph = exact_asset_tag(&normalized_query);
        let mut choices = data
            .asset_choices
            .iter()
            .filter(|row| {
                let entry = &data.effects.entries[row.index];
                scope.allows(entry.kind)
                    && (visibility.0
                        || exact_graph == Some(entry.graph)
                        || entry.has_discovery_identity_with(
                            |index| self.perk_names.get(&index).cloned(),
                            |item| self.item_names.get(&item).cloned(),
                        ))
                    && (scope == AssetScope::Projectiles
                        || match filter {
                            1 => entry.kind == projectile::Kind::Projectile,
                            2 => entry.kind == projectile::Kind::Emitter,
                            3 => entry.kind == projectile::Kind::Entity,
                            4 => {
                                entry.kind == projectile::Kind::Pickup
                                    || entry.pickup_role().is_some()
                            }
                            5 => entry.kind == projectile::Kind::Object,
                            _ => true,
                        })
                    && normalized_query.split_whitespace().all(|word| {
                        let word = word.strip_prefix("0x").unwrap_or(word);
                        row.search.contains(word)
                            || self
                                .asset_labels
                                .get(&entry.graph)
                                .is_some_and(|name| name.to_lowercase().contains(word))
                            || entry
                                .source_hint
                                .as_ref()
                                .is_some_and(|hint| hint.to_lowercase().contains(word))
                            || entry
                                .perk_indices
                                .iter()
                                .copied()
                                .chain(entry.contexts.iter().filter_map(|context| context.perk))
                                .any(|index| {
                                    self.perk_names
                                        .get(&index)
                                        .is_some_and(|name| name.to_lowercase().contains(word))
                                        || self.discovery.behavior(index).is_some_and(|behavior| {
                                            guidance::behavior_search(behavior)
                                                .to_lowercase()
                                                .contains(word)
                                        })
                                })
                            || entry
                                .contexts
                                .iter()
                                .filter_map(|context| context.item)
                                .filter_map(|item| self.item_names.get(&item))
                                .any(|item| item.name.to_lowercase().contains(word))
                    })
            })
            .collect::<Vec<_>>();
        ui.put(
            count_rect,
            egui::Label::new(if choices.len() == 1 {
                "1 Result".into()
            } else {
                format!("{} Results", choices.len())
            }),
        );
        ui.separator();
        choices.sort_by_cached_key(|row| {
            (
                data.effects.entries[row.index].label_rank(),
                self.asset_labels
                    .get(&data.effects.entries[row.index].graph)
                    .map(|name| name.to_lowercase())
                    .unwrap_or_default(),
            )
        });
        let keys = choices
            .iter()
            .map(|row| u64::from(data.effects.entries[row.index].graph))
            .collect::<Vec<_>>();
        pickers::BrowserList {
            keys: &keys,
            height: (ui.available_height() - 4.0).max(110.0),
            reset: search_changed || visibility.1 || filter != before,
            row_height: sundial::investment::authoring_choice_row_height(ui),
        }
        .draw_body(
            ui,
            |ui, index, selected| {
                let entry = &data.effects.entries[choices[index].index];
                let name = self
                    .asset_labels
                    .get(&entry.graph)
                    .cloned()
                    .unwrap_or_else(|| entry.discovery_label_with(|_| None, |_| None));
                let detail = technical_name(entry);
                if let Some(catalog) = self.catalog {
                    catalog.draw_authoring_choice_row(
                        ui,
                        None,
                        &name,
                        Some(&detail),
                        selected,
                    )
                } else {
                    sundial::investment::draw_asset_choice_row(ui, &name, &detail, selected)
                }
            },
            |ui, index| {
                let entry = &data.effects.entries[choices[index].index];
                let name = self
                    .asset_labels
                    .get(&entry.graph)
                    .cloned()
                    .unwrap_or_else(|| entry.discovery_label_with(|_| None, |_| None));
                ui.heading(name);
                ui.label(
                    entry
                        .pickup_role()
                        .map_or_else(|| entry.kind.label().to_owned(), str::to_owned),
                );
                if use_label.is_some() {
                    ui.label(scope.placement_hint(entry.kind))
                    .on_hover_text("This describes the selected action's placement. The asset controls its own behavior after creation.");
                }
                if let Some(use_label) = use_label
                    && ui.add(crate::app::style::primary(ui, use_label)).clicked() {
                    return Some(Asset {
                        graph: entry.graph,
                        path: entry.native_paths.first().cloned().unwrap_or_default(),
                        values: Vec::new(),
                    });
                }
                asset_details(
                    ui,
                    entry,
                    &data.effects,
                    &asset_usage(entry, self.perk_names, self.discovery),
                );
                None
            },
        )
    }
}

/// A complete native tag is an explicit lookup, including entries hidden by Show All.
fn exact_asset_tag(query: &str) -> Option<u32> {
    let digits = query.trim().strip_prefix("0x").unwrap_or(query.trim());
    (digits.len() == 8)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

pub(in crate::app::custom_perks) use sundial::investment::native_content::technical_name;
/// Shared detail panel for authored actions and stock projectile replacements.
pub(in crate::app::custom_perks) use sundial::ui::catalog::assets::asset_details;

/// The native Create Entity fields the compiler writes verbatim. Their roles are not mapped,
/// so these are technical controls with no gameplay claim attached.
pub(super) fn draw_attach_technical_fields(
    ui: &mut egui::Ui,
    mode: &mut u8,
    keys: &mut [u32; 2],
    float_bits: &mut [u32; 4],
) {
    let default = *mode == 1 && *keys == [EMPTY_KEY; 2] && *float_bits == [0; 4];
    egui::CollapsingHeader::new(egui::RichText::new("Technical Fields").small())
        .id_salt("attach-technical-fields")
        .show(ui, |ui| {
            ui.weak("Native bytes of the attach node. Their gameplay roles are not mapped.");
            egui::Grid::new("attach-technical-grid")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Attachment Mode")
                        .on_hover_text("Byte +0x02 of the Create Entity node. Stock actions store 0 through 3.");
                    ui.add(egui::DragValue::new(mode).range(0..=255));
                    ui.end_row();
                    for (index, label) in ["First Key", "Second Key"].into_iter().enumerate() {
                        ui.label(label).on_hover_text(format!(
                            "Key at +0x{:X} of the Create Entity node, written as a 32-bit hash. The empty hash is 0x{EMPTY_KEY:08X}.",
                            0x18 + index * 4
                        ));
                        controls::hex_key(ui, ("attach", index), &mut keys[index]);
                        ui.end_row();
                    }
                    for (index, label) in ["First Float", "Second Float", "Third Float", "Fourth Float"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.label(label).on_hover_text(format!(
                            "Float at +0x{:X} of the Create Entity node. Stock actions store 0 or 1.",
                            0x20 + index * 4
                        ));
                        controls::float_field(ui, &mut float_bits[index]);
                        ui.end_row();
                    }
                });
            if !default && ui.small_button("Reset Technical Fields").clicked() {
                *mode = 1;
                *keys = [EMPTY_KEY; 2];
                *float_bits = [0; 4];
            }
        });
}

#[cfg(test)]
mod tests {
    use super::exact_asset_tag;

    #[test]
    fn only_complete_asset_tags_bypass_discovery_visibility() {
        assert_eq!(exact_asset_tag("0x80c1d182"), Some(0x80C1D182));
        assert_eq!(exact_asset_tag("80c1d182"), Some(0x80C1D182));
        for query in ["frog", "80c1", "80c1d182 frog", "123456789", "zzzzzzzz"] {
            assert_eq!(exact_asset_tag(query), None);
        }
    }
}
