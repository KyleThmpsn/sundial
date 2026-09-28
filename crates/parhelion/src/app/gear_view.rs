//! The main page for armor, Sparrows, Ships and Ghost Shells.
//!
//! Gear keeps its base item's slot, class, look and sockets. This page edits what the build
//! compiles for gear: name, text and lore, icon, rarity, armor energy, stats and each socket's
//! choices, which work as a weapon's do and can hold custom perks from the Custom Perk Workbench.
use super::*;
use sundial::investment::{WeaponSocket, WeaponSupportedPlugSet};

/// Armor Energy Upgrade sockets. Their plug sets the energy type and capacity.
const ENERGY_SOCKET_TYPES: [u16; 2] = [678, 679];
/// The general armor mod socket, which only Armor 2.0 carries. Sundial's equipment view tells the
/// two generations apart by it too.
const ARMOR_2_MOD_SOCKET_TYPE: u16 = 643;
const ARMOR_STATS: [&str; 6] = [
    "Mobility",
    "Resilience",
    "Recovery",
    "Discipline",
    "Intellect",
    "Strength",
];
/// The Sparrow Engine plug carries Speed. The Sparrow itself carries Boost and Durability. These are
/// tooltip stats: a Sparrow with Speed 300 drove no faster in game (2026-09-25). The engine's perk
/// sets a named property per drive (Standard, Tuned, Custom) that picks the speed tier.
const SPARROW_STATS: [&str; 3] = ["Speed", "Boost", "Durability"];
const ENERGY_TYPES: [&str; 3] = ["Arc", "Solar", "Void"];

type PlugSets = Result<Vec<WeaponSupportedPlugSet>, String>;

#[cfg(test)]
mod tests;

pub(super) const fn class_label(class_type: u8) -> Option<&'static str> {
    match class_type {
        0 => Some("Titan"),
        1 => Some("Hunter"),
        2 => Some("Warlock"),
        _ => None,
    }
}

/// Whether armor with these socket types is Armor 2.0. Everything else, perk-rolled armor and
/// masks alike, is Armor 1.0.
fn is_armor_2(socket_types: impl IntoIterator<Item = u16>) -> bool {
    socket_types
        .into_iter()
        .any(|socket_type| socket_type == ARMOR_2_MOD_SOCKET_TYPE)
}

const fn armor_generation(armor_2: bool) -> &'static str {
    if armor_2 { "Armor 2.0" } else { "Armor 1.0" }
}

/// "Solar Energy 7" as ("Solar", 7).
fn energy_plug(label: &str) -> Option<(&'static str, u8)> {
    let mut words = label.split_whitespace();
    let element = words.next()?;
    if words.next()? != "Energy" {
        return None;
    }
    let capacity = words.next()?.parse::<u8>().ok()?;
    if words.next().is_some() {
        return None;
    }
    let element = ENERGY_TYPES
        .into_iter()
        .find(|candidate| *candidate == element)?;
    Some((element, capacity))
}

/// Whether a gear socket is listed on this page and can take a custom perk. The energy sockets
/// belong to the Energy Type and Energy Capacity controls.
pub(super) fn perk_destination(socket: &WeaponSocket) -> bool {
    socket.socket_type != u16::MAX && !ENERGY_SOCKET_TYPES.contains(&socket.socket_type)
}

/// The custom perk in a socket's starting choice.
fn socket_variant(
    recipe: &WeaponRecipe,
    socket_index: usize,
) -> Option<&WeaponSocketPlugVariantRecipe> {
    recipe
        .overrides
        .socket_plug_variants
        .iter()
        .find(|variant| {
            usize::from(variant.socket_index) == socket_index && variant.choice_index == 0
        })
}

/// The plug a socket starts with: the recipe's first choice, or the base item's own.
fn current_plug(recipe: &WeaponRecipe, socket: &WeaponSocket) -> Option<u32> {
    recipe
        .overrides
        .socket_columns
        .get(socket.index)
        .and_then(Option::as_ref)
        .and_then(|column| column.choices.first())
        .and_then(|hash| hash.parse_u32().ok())
        .or(socket.native_default)
}

/// Makes `plug` the one plug of a socket, replacing any custom perk there, as the energy controls
/// do. The column has no random roll, which would replace the plug, and the build keeps the base
/// socket's plug set beside it, so energy upgrades still apply in game. The base item's own plug
/// restores the inherited column instead.
fn set_socket_plug(
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
    socket: &WeaponSocket,
    plug: u32,
) {
    if Some(plug) == socket.native_default {
        restore_socket(recipe, socket.index);
    } else {
        remove_socket_variants(recipe, socket.index);
        materialize_socket_column(recipe, donor.sockets.len(), socket.index, &[plug], false);
    }
}

fn restore_socket(recipe: &mut WeaponRecipe, socket_index: usize) {
    remove_socket_variants(recipe, socket_index);
    if let Some(column) = recipe.overrides.socket_columns.get_mut(socket_index) {
        *column = None;
    }
    if recipe.overrides.socket_columns.iter().all(Option::is_none) {
        recipe.overrides.socket_columns.clear();
    }
}

fn remove_socket_variants(recipe: &mut WeaponRecipe, socket_index: usize) {
    recipe
        .overrides
        .socket_plug_variants
        .retain(|variant| usize::from(variant.socket_index) != socket_index);
}

/// The base item's own label for a rarity: "Legendary (base armor)".
fn base_rarity_label(kind: ItemKind, rarity: WeaponRarity) -> String {
    format!("{} (base {})", rarity.label(), kind.noun())
}

struct EnergySocket {
    index: usize,
    /// Every stock plug for this socket by energy type and capacity.
    plugs: BTreeMap<(&'static str, u8), u32>,
    current: Option<(&'static str, u8)>,
    base: Option<(&'static str, u8)>,
}

fn energy_socket(
    catalog: &InvestmentCatalog,
    recipe: &WeaponRecipe,
    donor: &WeaponDonor,
    sets: &[WeaponSupportedPlugSet],
) -> Option<EnergySocket> {
    let socket = donor
        .sockets
        .iter()
        .find(|socket| ENERGY_SOCKET_TYPES.contains(&socket.socket_type))?;
    let set = sets.iter().find(|set| set.socket_index == socket.index)?;
    let mut plugs = BTreeMap::new();
    for &hash in &set.plug_hashes {
        if let Some(key) = energy_plug(&catalog.plug_label(hash, false)) {
            plugs.entry(key).or_insert(hash);
        }
    }
    if plugs.is_empty() {
        return None;
    }
    let read =
        |plug: Option<u32>| plug.and_then(|plug| energy_plug(&catalog.plug_label(plug, false)));
    Some(EnergySocket {
        index: socket.index,
        plugs,
        current: read(current_plug(recipe, socket)),
        base: read(socket.native_default),
    })
}

struct StatRow {
    name: String,
    definition_index: u16,
    /// The item's own value and the base item's.
    item: i32,
    base_item: i32,
    /// What the starting plugs add, now and on the base item.
    plugs: i32,
    base_plugs: i32,
    /// One of the kind's own stats, always listed. Others are the recipe's added stats.
    core: bool,
}

impl StatRow {
    const fn value(&self) -> i32 {
        self.item + self.plugs
    }

    const fn base(&self) -> i32 {
        self.base_item + self.base_plugs
    }
}

fn stat_rows(
    catalog: &InvestmentCatalog,
    recipe: &WeaponRecipe,
    donor: &WeaponDonor,
    names: &[&'static str],
) -> Vec<StatRow> {
    let definitions = catalog.perk_stat_choices();
    let plug_stats = |plug: Option<u32>| -> Vec<(u16, i32)> {
        plug.map(|plug| catalog.item_stat_contributions(plug))
            .unwrap_or_default()
            .into_iter()
            .map(|stat| (stat.definition_index, stat.value))
            .collect()
    };
    // A custom perk that replaces its effects carries its complete stat list.
    let current = donor
        .sockets
        .iter()
        .map(|socket| match socket_variant(recipe, socket.index) {
            Some(variant) if variant.replace_effects => variant
                .investment_stats
                .iter()
                .map(|stat| (stat.definition_index, stat.value))
                .collect(),
            _ => plug_stats(current_plug(recipe, socket)),
        })
        .collect::<Vec<_>>();
    let base = donor
        .sockets
        .iter()
        .map(|socket| plug_stats(socket.native_default))
        .collect::<Vec<_>>();
    let sum = |plugs: &[Vec<(u16, i32)>], index: u16| {
        plugs
            .iter()
            .flatten()
            .filter(|(stat, _)| *stat == index)
            .map(|(_, value)| value)
            .sum::<i32>()
    };
    let row = |definition_index: u16, name: String, core: bool| {
        let base_item = donor
            .investment_stats
            .iter()
            .find(|stat| stat.definition_index == definition_index)
            .map_or(0, |stat| stat.value);
        let item = recipe
            .overrides
            .investment_stats
            .iter()
            .find(|stat| stat.definition_index == definition_index)
            .map_or(base_item, |stat| stat.value);
        StatRow {
            name,
            definition_index,
            item,
            base_item,
            plugs: sum(&current, definition_index),
            base_plugs: sum(&base, definition_index),
            core,
        }
    };
    let mut rows = names
        .iter()
        .filter_map(|&name| {
            let definition_index = definitions
                .iter()
                .find(|stat| stat.name.trim() == name)?
                .definition_index;
            Some(row(definition_index, name.to_owned(), true))
        })
        .collect::<Vec<_>>();
    // Stats the recipe sets beyond the kind's own, the way the weapon stat editor adds them.
    for stat in &recipe.overrides.investment_stats {
        if rows
            .iter()
            .any(|row| row.definition_index == stat.definition_index)
        {
            continue;
        }
        let name = definitions
            .iter()
            .find(|choice| choice.definition_index == stat.definition_index)
            .map_or_else(
                || format!("Stat {}", stat.definition_index),
                |choice| choice.name.trim().to_owned(),
            );
        rows.push(row(stat.definition_index, name, false));
    }
    rows
}

fn set_item_stat(recipe: &mut WeaponRecipe, definition_index: u16, value: Option<i32>) {
    let stats = &mut recipe.overrides.investment_stats;
    stats.retain(|stat| stat.definition_index != definition_index);
    if let Some(value) = value {
        stats.push(WeaponStatOverride {
            definition_index,
            value,
        });
        stats.sort_by_key(|stat| stat.definition_index);
    }
}

impl PackageAuthoringApp {
    pub(super) fn current_gear_donor(&self) -> Option<WeaponDonor> {
        let hash = self.recipe.donor.item_hash.parse_u32().ok()?;
        self.catalog.as_ref()?.gear_donor(hash)
    }

    /// Compatible plugs for the base item's sockets, cached for as long as the base is the same.
    fn gear_plug_sets(&mut self, donor_hash: u32) -> PlugSets {
        if self
            .gear_plug_sets
            .as_ref()
            .is_none_or(|(hash, _)| *hash != donor_hash)
        {
            let sets = self
                .catalog
                .as_ref()
                .ok_or_else(|| "Catalog unavailable.".to_owned())
                .and_then(|catalog| catalog.gear_supported_plug_sets(donor_hash));
            self.gear_plug_sets = Some((donor_hash, sets));
        }
        self.gear_plug_sets
            .as_ref()
            .map_or_else(|| Ok(Vec::new()), |(_, sets)| sets.clone())
    }

    pub(super) fn draw_gear_editor(&mut self, ui: &mut egui::Ui) {
        match self.recipe.kind {
            ItemKind::Shader => return self.draw_shader_editor(ui),
            ItemKind::Subclass => return self.draw_subclass_editor(ui),
            _ => {}
        }
        ui.spacing_mut().item_spacing.y = 4.0;
        let donor = self.current_gear_donor();
        let plug_sets = donor
            .as_ref()
            .map(|donor| self.gear_plug_sets(donor.summary.hash));
        if let Some(base_width) = workbench_left_column_width(ui.available_width()) {
            let definition_width = ui.available_width() - base_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(base_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(base_width);
                        self.draw_gear_base(ui);
                        ui.add_space(8.0);
                        self.draw_icon_donor_picker(ui);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(definition_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(definition_width);
                        self.draw_gear_definition(ui, donor.as_ref(), plug_sets.as_ref());
                        ui.add_space(8.0);
                        self.draw_gear_lore(ui);
                    },
                );
            });
        } else {
            self.draw_gear_base(ui);
            ui.add_space(8.0);
            self.draw_icon_donor_picker(ui);
            ui.separator();
            self.draw_gear_definition(ui, donor.as_ref(), plug_sets.as_ref());
            ui.add_space(8.0);
            self.draw_gear_lore(ui);
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        let (Some(donor), Some(plug_sets)) = (donor, plug_sets) else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Base item not found in the catalog.",
            );
            return;
        };
        let stats: &[&'static str] = match self.recipe.kind {
            ItemKind::Armor => &ARMOR_STATS,
            ItemKind::Sparrow => &SPARROW_STATS,
            _ => &[],
        };
        let stats_width =
            workbench_left_column_width(ui.available_width()).filter(|_| !stats.is_empty());
        if let Some(stats_width) = stats_width {
            let sockets_width = ui.available_width() - stats_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(stats_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(stats_width);
                        self.draw_gear_stats(ui, &donor, stats);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(sockets_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(sockets_width);
                        self.draw_gear_sockets(ui, &donor, &plug_sets);
                    },
                );
            });
        } else {
            if !stats.is_empty() {
                self.draw_gear_stats(ui, &donor, stats);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
            }
            self.draw_gear_sockets(ui, &donor, &plug_sets);
        }
    }

    pub(super) fn draw_gear_base(&mut self, ui: &mut egui::Ui) {
        let kind = self.recipe.kind;
        draw_donor_section_label(
            ui,
            &format!("Base {}", kind.label()),
            Some(match kind {
                ItemKind::Shader => "Starting dyes.",
                ItemKind::Subclass => "Class, element and starting abilities.",
                _ => "Slot, class, look and starting sockets.",
            }),
        );
        let current_hash = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .filter(|hash| *hash != 0);
        let candidates = self.gear_donors.get(&kind).map_or(&[][..], Vec::as_slice);
        let selected_text = current_hash
            .and_then(|hash| candidates.iter().find(|donor| donor.hash == hash))
            .map_or_else(
                || {
                    self.recipe
                        .donor
                        .expected_name
                        .clone()
                        .unwrap_or_else(|| self.recipe.donor.item_hash.to_string())
                },
                |donor| {
                    format!(
                        "{} · {} · 0x{:08X}",
                        donor.name, donor.type_name, donor.hash
                    )
                },
            );
        let selection = self.catalog.as_ref().and_then(|catalog| {
            let class_detail = |hash: u32| {
                catalog
                    .item_class_type(hash)
                    .and_then(class_label)
                    .map(str::to_owned)
            };
            // An armor row names its slot, class and generation, which is what tells two pieces
            // of one set apart. The slot names are looked up only while the list is open.
            let slots = std::cell::OnceCell::new();
            let armor_detail = |hash: u32| {
                let slots = slots.get_or_init(|| {
                    candidates
                        .iter()
                        .map(|donor| (donor.hash, donor.type_name.as_str()))
                        .collect::<BTreeMap<_, _>>()
                });
                let generation = armor_generation(is_armor_2(catalog.item_socket_types(hash)));
                let parts = [
                    slots.get(&hash).copied(),
                    catalog.item_class_type(hash).and_then(class_label),
                    Some(generation),
                ];
                Some(parts.into_iter().flatten().collect::<Vec<_>>().join(" · "))
            };
            let row_detail: Option<&dyn Fn(u32) -> Option<String>> = match kind {
                ItemKind::Armor => Some(&armor_detail),
                ItemKind::Subclass => Some(&class_detail),
                _ => None,
            };
            catalog.draw_weapon_donor_header_picker(
                ui,
                ("gear-base", kind),
                &mut self.donor_query,
                candidates.iter(),
                WeaponDonorPickerOptions {
                    selected_hash: current_hash,
                    selected_label: &selected_text,
                    header_label: None,
                    action_label: "Change Base",
                    selected_icon_override: None,
                    secondary_action_label: None,
                    row_detail,
                    clear: None,
                },
            )
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(selected_text));
        }
        // Armor names its class and generation under the base. A subclass's type already names
        // its class, as "Hunter Subclass".
        if kind == ItemKind::Armor
            && let Some((catalog, hash)) = self.catalog.as_ref().zip(current_hash)
        {
            ui.horizontal(|ui| {
                if let Some(class) = catalog.item_class_type(hash).and_then(class_label) {
                    ui.weak(class);
                }
                crate::app::style::badge(
                    ui,
                    armor_generation(is_armor_2(catalog.item_socket_types(hash))),
                );
            });
        }
        if let Some(WeaponDonorPickerAction::Select(hash)) = selection
            && current_hash != Some(hash)
            && let Some(name) = self
                .gear_donors
                .get(&kind)
                .and_then(|donors| donors.iter().find(|donor| donor.hash == hash))
                .map(|donor| donor.name.clone())
        {
            self.recipe.set_donor(hash, name);
            self.clear_dependent_picker_queries();
        }
    }

    fn draw_gear_definition(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
        plug_sets: Option<&PlugSets>,
    ) {
        let kind = self.recipe.kind;
        let branding = self.presentation_editor.branding();
        self.draw_item_text(ui, donor.map(|donor| donor.summary.type_name.as_str()));
        ui.add_space(4.0);
        let energy = match (self.catalog.as_ref(), donor, plug_sets) {
            (Some(catalog), Some(donor), Some(Ok(sets))) => {
                energy_socket(catalog, &self.recipe, donor, sets)
            }
            _ => None,
        };
        let fields: &[usize] = if energy.is_some() { &[0, 1, 2] } else { &[0] };
        let column_count = core_profile_column_count(ui.available_width());
        for fields in fields.chunks(column_count) {
            ui.columns(column_count, |columns| {
                for (&field, column) in fields.iter().zip(columns) {
                    match field {
                        0 => draw_gear_rarity(
                            column,
                            &mut self.recipe.overrides,
                            donor.map_or(WeaponRarity::Unknown, |donor| donor.summary.rarity),
                            kind,
                            branding,
                        ),
                        1 => {
                            if let (Some(energy), Some(donor)) = (&energy, donor) {
                                draw_energy_type(column, &mut self.recipe, donor, energy);
                            }
                        }
                        _ => {
                            if let (Some(energy), Some(donor)) = (&energy, donor) {
                                draw_energy_capacity(column, &mut self.recipe, donor, energy);
                            }
                        }
                    }
                }
            });
        }
    }

    fn draw_gear_stats(&mut self, ui: &mut egui::Ui, donor: &WeaponDonor, names: &[&'static str]) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let kind = self.recipe.kind;
        // Armor 1.0 carries all six stats too, through its intrinsic plug.
        let rows = stat_rows(catalog, &self.recipe, donor, names);
        // The base's stat group caps what each stat can show. Every armor group in the
        // Shadowkeep packages caps them at 42, Armor 1.0 included.
        let cap = catalog.item_stat_maximum(donor.summary.hash);
        ui.horizontal_wrapped(|ui| {
            ui.heading(format!("{} Stats", kind.label()));
            let hint = match kind {
                ItemKind::Sparrow => {
                    "Tooltip stats. The Sparrow Engine's perk sets how fast it drives."
                }
                _ => "Totals include the starting plugs. A change here adds to the armor itself.",
            };
            draw_authoring_info_icon(
                ui,
                cap.map_or_else(
                    || hint.to_owned(),
                    |cap| format!("{hint} Each stat tops out at {cap}."),
                ),
            );
            if ui
                .add_enabled(
                    !self.recipe.overrides.investment_stats.is_empty(),
                    egui::Button::new("Reset Stats"),
                )
                .clicked()
            {
                self.recipe.overrides.investment_stats.clear();
            }
        });
        ui.add_space(4.0);
        let mut edits = Vec::new();
        egui::Grid::new(("gear-stats", kind))
            .num_columns(4)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Stat");
                ui.label("Value");
                ui.label("Base");
                ui.label("");
                ui.end_row();
                for row in &rows {
                    ui.label(row.name.as_str());
                    let mut value = row.value();
                    // A base already past the cap keeps its own value.
                    let upper = cap.map_or(999, |cap| cap.max(row.base()));
                    // Plugs can carry a total past the cap. The value is shown as it is: a
                    // ranged DragValue otherwise clamps it on draw and reports a change, which
                    // would rewrite the item's own stat with no edit made.
                    if ui
                        .add(
                            egui::DragValue::new(&mut value)
                                .range(0..=upper)
                                .clamp_existing_to_range(false),
                        )
                        .changed()
                    {
                        edits.push((row.definition_index, Some(value - row.plugs)));
                    }
                    ui.weak(row.base().to_string());
                    if !row.core {
                        if ui
                            .add(egui::Button::new("×").small())
                            .on_hover_text("Remove this added stat")
                            .clicked()
                        {
                            edits.push((row.definition_index, None));
                        }
                    } else if row.item != row.base_item
                        && ui
                            .add(egui::Button::new("Reset").small())
                            .on_hover_text("Restore the base value")
                            .clicked()
                    {
                        edits.push((row.definition_index, None));
                    } else if row.item == row.base_item {
                        ui.label("");
                    }
                    ui.end_row();
                }
                let core = rows.iter().filter(|row| row.core);
                if kind == ItemKind::Armor && core.clone().next().is_some() {
                    ui.strong("Total");
                    ui.strong(core.clone().map(StatRow::value).sum::<i32>().to_string());
                    ui.weak(core.map(StatRow::base).sum::<i32>().to_string());
                    ui.label("");
                    ui.end_row();
                }
            });
        // Any stat the base carries or the catalog allows, as on weapons.
        let shown = rows
            .iter()
            .map(|row| row.definition_index)
            .collect::<BTreeSet<_>>();
        let mut choices = donor
            .investment_stats
            .iter()
            .chain(&donor.addable_investment_stats)
            .filter(|stat| !shown.contains(&stat.definition_index))
            .filter(|stat| {
                self.show_internal_stats || !is_internal_weapon_stat(stat.definition_index)
            })
            .map(|stat| (stat.definition_index, stat.name.clone(), stat.value))
            .collect::<Vec<_>>();
        choices.sort_by_key(|(definition_index, _, _)| *definition_index);
        choices.dedup_by_key(|(definition_index, _, _)| *definition_index);
        ui.add_space(3.0);
        ui.add_enabled_ui(!choices.is_empty(), |ui| {
            ui.menu_button("+ Add Stat", |ui| {
                ui.set_min_width(220.0);
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (definition_index, name, value) in &choices {
                            if ui.button(format!("{definition_index}  {name}")).clicked() {
                                edits.push((*definition_index, Some(*value)));
                                ui.close_menu();
                            }
                        }
                    });
            });
        });
        for (definition_index, value) in edits {
            set_item_stat(&mut self.recipe, definition_index, value);
        }
    }

    /// The base's sockets, edited the way a weapon's are. Gear keeps its base's sockets, so the
    /// shared editor names each one where a weapon offers a role, and adds or removes none.
    fn draw_gear_sockets(&mut self, ui: &mut egui::Ui, donor: &WeaponDonor, plug_sets: &PlugSets) {
        let has_authored_columns = !self.recipe.overrides.socket_columns.is_empty()
            || !self.recipe.overrides.socket_plug_variants.is_empty();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Perks & Sockets");
            draw_authoring_info_icon(
                ui,
                "The first choice starts equipped. Right-click a choice to make it the default.",
            );
            if ui
                .button("Custom Perks…")
                .on_hover_text("Open the Custom Perk Workbench.")
                .clicked()
            {
                self.perk_workbench.open = true;
            }
            self.draw_socket_options(ui, has_authored_columns);
        });
        if self.show_plug_safety_warnings {
            draw_plug_safety_warning(ui, self.plug_selection_mode);
        }
        if let Err(error) = plug_sets {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return;
        }
        let Self {
            catalog,
            recipe,
            plug_queries,
            socket_choice_pages,
            recipe_library,
            plug_selection_mode,
            show_plug_safety_warnings,
            show_technical_socket_rows,
            perk_request,
            log,
            ..
        } = self;
        if let Some(catalog) = catalog.as_ref() {
            draw_socket_pickers(
                ui,
                SocketPickerContext {
                    catalog,
                    recipe_library: recipe_library.as_ref(),
                    recipe,
                    queries: plug_queries,
                    pages: socket_choice_pages,
                    plug_selection_mode,
                    show_plug_safety_warnings: *show_plug_safety_warnings,
                    show_experimental_options: false,
                    show_technical_rows: show_technical_socket_rows,
                    perk_request,
                    donor,
                    log,
                },
            );
        }
    }

    /// The item's lore tab: its base's, none, or a story of its own.
    fn draw_gear_lore(&mut self, ui: &mut egui::Ui) {
        self.presentation_editor.draw_lore(
            ui,
            &mut self.recipe.overrides,
            &self.packages,
            (
                self.recipe.donor.item_hash.parse_u32().ok(),
                self.recipe.kind,
            ),
        );
    }
}

pub(super) fn draw_gear_rarity(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    inherited: WeaponRarity,
    kind: ItemKind,
    branding: crate::branding::Branding,
) {
    ui.horizontal(|ui| {
        ui.label("Rarity");
        draw_authoring_info_icon(
            ui,
            if crate::collection::GearPage::for_kind(kind).is_some() {
                format!(
                    "Exotic needs an Exotic base. Any rarity appears on the {} page under {}.",
                    branding.name(),
                    kind.plural()
                )
            } else {
                "Exotic needs an Exotic base. Exotic armor appears under Exotics beside its base and equips one piece at a time.".to_owned()
            },
        );
    });
    let base_exotic = inherited == WeaponRarity::Exotic;
    let selected_text = overrides.rarity.map_or_else(
        || base_rarity_label(kind, inherited),
        |rarity| recipe_rarity_label(rarity).to_owned(),
    );
    egui::ComboBox::from_id_salt("gear_rarity")
        .selected_text(selected_text)
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            if ui
                .selectable_label(
                    overrides.rarity.is_none(),
                    base_rarity_label(kind, inherited),
                )
                .clicked()
            {
                overrides.rarity = None;
            }
            for rarity in [
                RecipeRarity::Common,
                RecipeRarity::Uncommon,
                RecipeRarity::Rare,
                RecipeRarity::Legendary,
                RecipeRarity::Exotic,
            ] {
                // Exotic stays with Exotic bases.
                let allowed = (rarity == RecipeRarity::Exotic) == base_exotic;
                if ui
                    .add_enabled(
                        allowed,
                        egui::SelectableLabel::new(
                            overrides.rarity == Some(rarity),
                            recipe_rarity_label(rarity),
                        ),
                    )
                    .clicked()
                {
                    overrides.rarity = Some(rarity);
                }
            }
        });
}

fn draw_energy_type(
    ui: &mut egui::Ui,
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
    energy: &EnergySocket,
) {
    ui.horizontal(|ui| {
        ui.label("Energy Type");
        draw_authoring_info_icon(ui, "Arc, Solar or Void Energy for mods.");
    });
    let current = energy.current.map(|(element, _)| element);
    let base = energy.base.map(|(element, _)| element);
    let label = |element: &str| {
        if Some(element) == base {
            format!("{element} (base armor)")
        } else {
            element.to_owned()
        }
    };
    let mut chosen = None;
    egui::ComboBox::from_id_salt("gear_energy_type")
        .selected_text(current.map_or_else(String::new, label))
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            for element in ENERGY_TYPES {
                let available = energy
                    .plugs
                    .keys()
                    .any(|(candidate, _)| *candidate == element);
                if ui
                    .add_enabled(
                        available,
                        egui::SelectableLabel::new(current == Some(element), label(element)),
                    )
                    .clicked()
                {
                    chosen = Some(element);
                }
            }
        });
    if let Some(element) = chosen {
        let capacity = energy.current.map_or(1, |(_, capacity)| capacity);
        // Keep the capacity when the new type has it, otherwise the nearest one it has.
        let plug = energy
            .plugs
            .iter()
            .filter(|((candidate, _), _)| *candidate == element)
            .min_by_key(|((_, candidate), _)| candidate.abs_diff(capacity))
            .map(|(_, plug)| *plug);
        if let (Some(plug), Some(socket)) = (plug, donor.sockets.get(energy.index)) {
            set_socket_plug(recipe, donor, socket, plug);
        }
    }
}

fn draw_energy_capacity(
    ui: &mut egui::Ui,
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
    energy: &EnergySocket,
) {
    ui.horizontal(|ui| {
        ui.label("Energy Capacity");
        draw_authoring_info_icon(
            ui,
            "Stock armor tops out at 10. Capacity 10 adds 2 to every stat.",
        );
    });
    let Some((element, current)) = energy.current else {
        ui.add_enabled(false, egui::Button::new("Unknown"));
        return;
    };
    let label = |capacity: u8| {
        if energy.base == Some((element, capacity)) {
            format!("{capacity} (base armor)")
        } else {
            capacity.to_string()
        }
    };
    let mut chosen = None;
    egui::ComboBox::from_id_salt("gear_energy_capacity")
        .selected_text(label(current))
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            for (&(candidate, capacity), &plug) in &energy.plugs {
                if candidate == element
                    && ui
                        .selectable_label(capacity == current, label(capacity))
                        .clicked()
                {
                    chosen = Some(plug);
                }
            }
        });
    if let (Some(plug), Some(socket)) = (chosen, donor.sockets.get(energy.index)) {
        set_socket_plug(recipe, donor, socket, plug);
    }
}
