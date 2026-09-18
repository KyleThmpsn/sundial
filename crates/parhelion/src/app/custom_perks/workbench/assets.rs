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

/// How the asset results are ordered. Order changes presentation only. It never hides a
/// result and never changes which assets an action accepts.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Order {
    /// Query matches first, then assets the packages name directly.
    #[default]
    BestMatch,
    Name,
    Kind,
    Source,
}

impl Order {
    const ALL: [Self; 4] = [Self::BestMatch, Self::Name, Self::Kind, Self::Source];

    fn label(self) -> &'static str {
        match self {
            Self::BestMatch => "Best Match",
            Self::Name => "Name",
            Self::Kind => "Type",
            Self::Source => "Package",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::BestMatch => "Search matches first, then assets the packages name directly.",
            Self::Name => "Every result by name, including unnamed assets by tag.",
            Self::Kind => "Grouped by object type, then by name.",
            Self::Source => "Grouped by the installed package the asset lives in.",
        }
    }

    fn from_index(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }
}

/// The comparable key for one result. Every order ends with the name, so results never
/// reshuffle between frames.
fn asset_sort_key(
    order: Order,
    entry: &projectile::catalog::Entry,
    name: &str,
    direct_match: bool,
) -> (bool, u8, String, String) {
    let name = name.to_lowercase();
    match order {
        Order::BestMatch => (!direct_match, entry.label_rank(), String::new(), name),
        Order::Name => (false, 0, String::new(), name),
        Order::Kind => (false, 0, entry.kind_label().to_lowercase(), name),
        Order::Source => (false, 0, entry.source_label().to_lowercase(), name),
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
    /// A shared attachment's ancestry is not its identity or gameplay role in this effect.
    pub(super) fn program_asset_labels(
        &self,
        program: &sundial::package_authoring::sandbox_perk::program::NativeProgram,
        name: &str,
    ) -> BTreeMap<u32, String> {
        let attachments: BTreeSet<_> = program
            .graph
            .blocks
            .iter()
            .filter(|block| matches!(block.class, 0x80803E45 | 0x80803E44))
            .filter_map(|block| block.bytes.get(16..20))
            .filter_map(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes)
            .collect();
        let context = name.split(" · Effect ").next().unwrap_or(name).trim();
        program
            .assets
            .iter()
            .enumerate()
            .map(|(index, asset)| {
                let label = self
                    .asset_labels
                    .get(&asset.graph)
                    .cloned()
                    .unwrap_or_else(|| format!("Asset 0x{:08X}", asset.graph));
                let shared_attachment = attachments.contains(&asset.graph)
                    && self.discovery.data.as_ref().is_some_and(|data| {
                        data.effects
                            .entries
                            .iter()
                            .find(|entry| entry.graph == asset.graph)
                            .is_some_and(|entry| {
                                entry.kind == projectile::Kind::Entity
                                    && entry.direct_name().is_none()
                            })
                    });
                (
                    asset.graph,
                    if shared_attachment {
                        format!("{context} Attachment {}", index + 1)
                    } else {
                        label
                    },
                )
            })
            .collect()
    }

    pub(super) fn draw_asset_picker(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        asset: &mut Asset,
        scope: AssetScope,
    ) {
        self.refresh_asset_labels();
        let (empty_label, title, use_label) = scope.picker_labels();
        let label = asset_label(asset, &self.asset_labels, empty_label);
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

/// A saved native path remains useful while discovery is loading or lacks ancestry.
pub(super) fn asset_label(asset: &Asset, labels: &BTreeMap<u32, String>, empty: &str) -> String {
    labels
        .get(&asset.graph)
        .filter(|name| !name.starts_with("Unidentified "))
        .cloned()
        .or_else(|| {
            (!asset.path.is_empty())
                .then(|| sundial::package_authoring::tft::asset_label(&asset.path))
        })
        .unwrap_or_else(|| {
            if asset.graph == 0 {
                empty.into()
            } else {
                labels
                    .get(&asset.graph)
                    .cloned()
                    .unwrap_or_else(|| format!("Asset 0x{:08X}", asset.graph))
            }
        })
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
        let order_id = ui.make_persistent_id("asset-order");
        let mut order =
            Order::from_index(ui.data(|state| state.get_temp::<u8>(order_id).unwrap_or_default()));
        let before = (filter, order);
        let mut visibility = (false, false);
        let mut search_changed = reset;
        let count_rect = ui
            // The type, sort and visibility controls do not fit one line beside the search
            // box in a narrow window. Wrapping keeps every control usable.
            .horizontal_wrapped(|ui| {
                // A combo takes the width of its selected text unless something bounds it,
                // so each one below is drawn inside an allocation of its own width and
                // truncates to that, with the full reading on hover. The search box takes
                // what those widths leave rather than a hand-added total that goes stale
                // the moment one of them changes.
                const TYPE_WIDTH: f32 = 150.0;
                const ORDER_WIDTH: f32 = 150.0;
                const SHOW_ALL_WIDTH: f32 = 90.0;
                const COUNT_WIDTH: f32 = 86.0;
                let gap = ui.spacing().item_spacing.x;
                // Sort, Show All and the result count always follow the search box. The
                // type filter joins them everywhere but the projectile picker.
                let reserved = if scope == AssetScope::Projectiles {
                    ORDER_WIDTH + SHOW_ALL_WIDTH + COUNT_WIDTH + gap * 3.0
                } else {
                    ORDER_WIDTH + SHOW_ALL_WIDTH + COUNT_WIDTH + TYPE_WIDTH + gap * 4.0
                };
                let width = (ui.available_width() - reserved).max(160.0);
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
                    let chosen = types
                        .iter()
                        .find(|(value, _)| *value == filter)
                        .map_or("All Types", |(_, label)| *label);
                    controls::sized(ui, TYPE_WIDTH, |ui| {
                        egui::ComboBox::from_id_salt("asset-type-filter")
                            .width(TYPE_WIDTH)
                            .truncate()
                            .selected_text(chosen)
                            .show_ui(ui, |ui| {
                                for (value, name) in types {
                                    if value != 3 || scope == AssetScope::Any {
                                        ui.selectable_value(&mut filter, value, name);
                                    }
                                }
                            })
                            .response
                            .on_hover_text(format!("Asset Type: {chosen}"));
                        pickers::name_combo(ui, "asset-type-filter", "Asset Type");
                    });
                }
                let sort = format!("Sort: {}", order.label());
                controls::sized(ui, ORDER_WIDTH, |ui| {
                    egui::ComboBox::from_id_salt("asset-order")
                        .width(ORDER_WIDTH)
                        .truncate()
                        .selected_text(sort.clone())
                        .show_ui(ui, |ui| {
                            for choice in Order::ALL {
                                ui.selectable_value(&mut order, choice, choice.label())
                                    .on_hover_text(choice.hint());
                            }
                        })
                        .response
                        .on_hover_text(format!(
                            "{sort}\nOrder the results. Sorting never hides a result."
                        ));
                    pickers::name_combo(ui, "asset-order", "Sort Order");
                });
                visibility = pickers::show_all(ui);
                ui.allocate_exact_size(
                    egui::vec2(COUNT_WIDTH, ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                )
                .0
            })
            .inner;
        ui.data_mut(|state| {
            state.insert_temp(filter_id, filter);
            state.insert_temp(
                order_id,
                Order::ALL
                    .iter()
                    .position(|choice| *choice == order)
                    .unwrap_or(0) as u8,
            );
        });
        let normalized_query = query.trim().to_lowercase();
        let exact_graph = exact_asset_tag(&normalized_query);
        let words = normalized_query
            .split_whitespace()
            .map(|word| word.strip_prefix("0x").unwrap_or(word))
            .collect::<Vec<_>>();
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
            let entry = &data.effects.entries[row.index];
            let name = self
                .asset_labels
                .get(&entry.graph)
                .map(|name| name.to_lowercase())
                .unwrap_or_default();
            let direct_match = !normalized_query.is_empty()
                && (exact_graph == Some(entry.graph)
                    || normalized_query
                        .split_whitespace()
                        .all(|word| name.contains(word)));
            asset_sort_key(order, entry, &name, direct_match)
        });
        let keys = choices
            .iter()
            .map(|row| u64::from(data.effects.entries[row.index].graph))
            .collect::<Vec<_>>();
        pickers::BrowserList {
            keys: &keys,
            height: (ui.available_height() - 4.0).max(110.0),
            reset: search_changed || visibility.1 || (filter, order) != before,
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
                let mut detail = technical_name(entry);
                if let Some(reason) = self.match_reason(entry, &name, &words) {
                    detail = format!("{detail} · {reason}");
                }
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
                if let Some(reason) = self.match_reason(entry, &name, &words) {
                    ui.small(format!("Matched through its source. {reason}."))
                        .on_hover_text("The search words are not in this asset's own name.");
                }
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

impl Browser<'_> {
    /// Why an entry answers a search when its own name does not: the perk, weapon, source
    /// context or behavior the words matched instead. Nothing when the name itself holds
    /// every word, so an ordinary match stays unadorned.
    fn match_reason(
        &self,
        entry: &projectile::catalog::Entry,
        name: &str,
        words: &[&str],
    ) -> Option<String> {
        let name = name.to_lowercase();
        let missing = words
            .iter()
            .copied()
            .filter(|word| !name.contains(word))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return None;
        }
        let holds = |text: &str| {
            let text = text.to_lowercase();
            missing.iter().all(|word| text.contains(word))
        };
        let perks = entry
            .perk_indices
            .iter()
            .copied()
            .chain(entry.contexts.iter().filter_map(|context| context.perk))
            .collect::<BTreeSet<_>>();
        if let Some(perk) = perks
            .iter()
            .filter_map(|index| self.perk_names.get(index))
            .find(|perk| holds(perk))
        {
            return Some(format!("Referenced by {perk}"));
        }
        if let Some(item) = entry
            .contexts
            .iter()
            .filter_map(|context| context.item)
            .filter_map(|item| self.item_names.get(&item))
            .find(|item| holds(&item.name))
        {
            return Some(format!("Fired by {}", item.name));
        }
        if let Some(hint) = entry.source_hint.as_deref().filter(|hint| holds(hint)) {
            return Some(format!("Source context: {hint}"));
        }
        if let Some(perk) = perks
            .iter()
            .filter(|index| {
                self.discovery
                    .behavior(**index)
                    .is_some_and(|behavior| holds(&guidance::behavior_search(behavior)))
            })
            .filter_map(|index| self.perk_names.get(index))
            .next()
        {
            return Some(format!(
                "The behavior of {perk} mentions {}",
                missing.join(" ")
            ));
        }
        Some(format!(
            "Its native path or type mentions {}",
            missing.join(" ")
        ))
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
    egui::CollapsingHeader::new("Advanced")
        .id_salt("attach-technical-fields")
        .show(ui, |ui| {
            properties::field(ui, "Attachment Mode", "Byte +0x02 of the Create Entity node. Stock actions store 0 through 3.", |ui| {
                ui.add(egui::DragValue::new(mode).range(0..=255));
            });
                    for (index, label) in ["First Key", "Second Key"].into_iter().enumerate() {
                        let hint = format!(
                            "Key at +0x{:X} of the Create Entity node, written as a 32-bit hash. The empty hash is 0x{EMPTY_KEY:08X}.",
                            0x18 + index * 4
                        );
                        properties::field(ui, label, &hint, |ui| {
                            controls::hex_key(ui, ("attach", index), &mut keys[index]);
                        });
                    }
                    for (index, label) in ["First Float", "Second Float", "Third Float", "Fourth Float"]
                        .into_iter()
                        .enumerate()
                    {
                        let hint = format!(
                            "Float at +0x{:X} of the Create Entity node. Stock actions store 0 or 1.",
                            0x20 + index * 4
                        );
                        properties::field(ui, label, &hint, |ui| {
                            controls::float_field(ui, &mut float_bits[index]);
                        });
                    }
            if !default && ui.small_button("Reset Technical Fields").clicked() {
                *mode = 1;
                *keys = [EMPTY_KEY; 2];
                *float_bits = [0; 4];
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{Order, asset_sort_key, exact_asset_tag};
    use sundial::package_authoring::sandbox_perk::projectile::{self, catalog::Entry};

    fn entry(graph: u32, kind: projectile::Kind, package: &str, path: Option<&str>) -> Entry {
        Entry {
            graph,
            kind,
            object_type: 18,
            owners: Vec::new(),
            package: package.into(),
            native_name: None,
            native_paths: path.map(|path| vec![path.to_owned()]).unwrap_or_default(),
            contexts: Vec::new(),
            perk_indices: Vec::new(),
            source_hint: None,
        }
    }

    #[test]
    fn every_asset_order_is_total_and_keeps_the_same_result_set() {
        let named = entry(
            0x80B1_0001,
            projectile::Kind::Emitter,
            "sandbox",
            Some("content/sandbox/effects/zebra.pattern.tft"),
        );
        let unnamed = entry(
            0x80B1_0002,
            projectile::Kind::Projectile,
            "activities",
            None,
        );
        let mut rows = [(&named, "zebra emitter"), (&unnamed, "alpha projectile")];
        fn order_by<'a>(order: Order, rows: &mut [(&Entry, &'a str)]) -> Vec<&'a str> {
            rows.sort_by_cached_key(|(entry, name)| asset_sort_key(order, entry, name, false));
            rows.iter().map(|(_, name)| *name).collect()
        }
        // Best Match ranks an asset the packages name above one they do not.
        assert_eq!(
            order_by(Order::BestMatch, &mut rows),
            ["zebra emitter", "alpha projectile"]
        );
        assert_eq!(
            order_by(Order::Name, &mut rows),
            ["alpha projectile", "zebra emitter"]
        );
        // Emitter sorts before Projectile, and activities before sandbox.
        assert_eq!(
            order_by(Order::Kind, &mut rows),
            ["zebra emitter", "alpha projectile"]
        );
        assert_eq!(
            order_by(Order::Source, &mut rows),
            ["alpha projectile", "zebra emitter"]
        );
    }

    #[test]
    fn a_query_match_outranks_a_named_asset_only_under_best_match() {
        let named = entry(
            0x80B1_0001,
            projectile::Kind::Projectile,
            "sandbox",
            Some("content/sandbox/effects/named.pattern.tft"),
        );
        let matched = entry(0x80B1_0002, projectile::Kind::Projectile, "sandbox", None);
        assert!(
            asset_sort_key(Order::BestMatch, &matched, "hit", true)
                < asset_sort_key(Order::BestMatch, &named, "aaa", false)
        );
        assert!(
            asset_sort_key(Order::Name, &matched, "hit", true)
                > asset_sort_key(Order::Name, &named, "aaa", false)
        );
    }

    #[test]
    fn only_complete_asset_tags_bypass_discovery_visibility() {
        assert_eq!(exact_asset_tag("0x80c1d182"), Some(0x80C1D182));
        assert_eq!(exact_asset_tag("80c1d182"), Some(0x80C1D182));
        for query in ["frog", "80c1", "80c1d182 frog", "123456789", "zzzzzzzz"] {
            assert_eq!(exact_asset_tag(query), None);
        }
    }
}
