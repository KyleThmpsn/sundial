//! The main page for armor, Sparrows, Ships and Ghost Shells.
//!
//! Gear keeps its base item's slot and look. Armor can select another equip class. This page edits what the build compiles for
//! gear: name, text and lore, icon, rarity, armor energy, stats and sockets, which work as a
//! weapon's do. Sockets can be added, removed and given another role, and hold custom perks from
//! the Custom Perk Workbench. An emblem has no lore tab, stats, sockets or model, and shows its
//! nameplate instead (`emblem_view`).
use super::*;
use crate::item::ENERGY_SOCKET_TYPES;
use sundial::investment::{WeaponSocket, WeaponSupportedPlugSet};
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
/// contributes a continuous input to ordinary motion. Fixed-speed profiles ignore that input.
/// `vehicle` authors the actual forward and reverse motion programs independently of the tooltip.
const SPARROW_STATS: [&str; 3] = ["Speed", "Boost", "Durability"];
const ENERGY_TYPES: [&str; 3] = ["Arc", "Solar", "Void"];
type PlugSets = Result<Vec<WeaponSupportedPlugSet>, String>;
mod vehicle;

#[cfg(feature = "d2-model-importer")]
mod source;
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

/// An armor piece's slot, class and generation, which tell two pieces of one set apart.
fn armor_detail(catalog: &InvestmentCatalog, hash: u32, slot: Option<&str>) -> String {
    [
        slot,
        catalog.item_class_type(hash).and_then(class_label),
        Some(armor_generation(is_armor_2(
            catalog.item_socket_types(hash),
        ))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
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

/// The plug a socket starts with: the recipe's first choice, or the base item's own. A removed
/// socket holds none.
fn current_plug(recipe: &WeaponRecipe, socket: &WeaponSocket) -> Option<u32> {
    let column = recipe
        .overrides
        .socket_columns
        .get(socket.index)
        .and_then(Option::as_ref);
    if column.is_some_and(|column| column.socket_type == Some(u16::MAX)) {
        return None;
    }
    column
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

/// The list entry that gives a field back to the base item, which then follows the base.
pub(super) fn follow_base(kind: ItemKind) -> String {
    format!("Follow Base {}", kind.label())
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
    // A custom perk that replaces its effects carries its complete stat list. Sockets the recipe
    // adds count too.
    let sockets = super::socket_editor::socket_editor_donor(donor, recipe);
    let current = sockets
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

/// The stats' heading with what they are on hover, and its reset once a stat holds an edit.
/// Returns whether the reset was clicked.
fn stats_header(ui: &mut egui::Ui, kind: ItemKind, cap: Option<i32>, changed: bool) -> bool {
    let title = format!("{} Stats", kind.label());
    ui.horizontal(|ui| {
        let hint = match kind {
            ItemKind::Sparrow => {
                "Tooltip stats. Use Driving Speed to change motion. The Engine also affects \
                 ordinary Sparrow motion"
            }
            _ => "Totals include the starting plugs. A change here adds to the armor itself",
        };
        let hint = cap.map_or_else(
            || hint.to_owned(),
            |cap| format!("{hint}. Each stat tops out at {cap}"),
        );
        let heading = style::heading(ui, &title, changed);
        // A Sparrow's stats only label its tooltip, which nobody would guess, so that one keeps
        // its icon.
        if kind == ItemKind::Sparrow {
            draw_authoring_info_icon(ui, format!("{hint}."));
        } else {
            heading.on_hover_text(hint);
        }
        changed && style::reset_icon(ui, &format!("Reset {title}"))
    })
    .inner
}

/// One stat as a Destiny stat row: its name, its bar, its value, and the base's value to restore
/// once the item's own differs, or Remove for a stat the recipe added. Returns the edit made: the
/// item's own value, or none to restore the base's.
fn draw_stat_row(
    ui: &mut egui::Ui,
    columns: &StatColumns,
    row: &StatRow,
    (cap, scale): (Option<i32>, i32),
) -> Option<(u16, Option<i32>)> {
    ui.push_id(("gear-stat", row.definition_index), |ui| {
        // Grey until the item's own value differs from the base's, as a tile's name is.
        let modified = row.item != row.base_item;
        let cells = columns.row(ui, &row.name, modified);
        style::stat_bar(ui, cells.bar, (row.base(), row.value()), scale);
        // The bar's change segment says it by colour, so the base's value is also on its hover
        // and in the field's name.
        let name = if row.value() == row.base() {
            row.name.clone()
        } else {
            ui.interact(cells.bar, ui.id().with("bar"), egui::Sense::hover())
                .on_hover_text(format!("Base {}", row.base()));
            format!("{}, base {}", row.name, row.base())
        };
        let mut value = row.value();
        // A base already past the cap keeps its own value.
        let upper = cap.map_or(999, |cap| cap.max(row.base()));
        // Plugs can carry a total past the cap. The value is shown as it is: a ranged DragValue
        // otherwise clamps it on draw and reports a change, which would rewrite the item's own
        // stat with no edit made.
        let field = ui.put(
            cells.value,
            egui::DragValue::new(&mut value)
                .range(0..=upper)
                .clamp_existing_to_range(false),
        );
        let edited = style::named_control(field, name)
            .changed()
            .then_some(Some(value - row.plugs));
        let mut actions = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cells.action)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let cleared = if row.core {
            // The value a restore leaves: the base's own with the plugs the item has now.
            let restored = (row.base_item + row.plugs).to_string();
            modified && style::restore(&mut actions, &row.name, &restored)
        } else {
            let remove = actions
                .add(egui::Button::new("×").frame(false).small())
                .on_hover_text("Remove this added stat");
            style::focus_ring(&actions, &remove);
            style::named_control(remove, format!("Remove {}", row.name)).clicked()
        };
        edited
            .or(cleared.then_some(None))
            .map(|value| (row.definition_index, value))
    })
    .inner
}

/// Armor's total under its stats, with what the edits and plugs add against the base's total,
/// as Destiny marks a change.
fn draw_stat_total(ui: &mut egui::Ui, columns: &StatColumns, (value, base): (i32, i32)) {
    let cells = columns.row(ui, "Total", true);
    let total = ui.put(
        cells.value,
        egui::Label::new(egui::RichText::new(value.to_string()).strong()),
    );
    // The name is painted, so the number carries it for a screen reader.
    style::named_control(total, format!("Total {value}"));
    if value == base {
        return;
    }
    let (text, color) = if value > base {
        (
            format!("+{}", value - base),
            style::success_color(ui.visuals()),
        )
    } else {
        ((value - base).to_string(), ui.visuals().error_fg_color)
    };
    ui.put(
        cells.action,
        egui::Label::new(egui::RichText::new(text).size(11.0).color(color)),
    )
    .on_hover_text(format!("Base total {base}"));
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
                .and_then(|catalog| catalog.supported_plug_sets(donor_hash, &[]));
            self.gear_plug_sets = Some((donor_hash, sets));
        }
        self.gear_plug_sets
            .as_ref()
            .map_or_else(|| Ok(Vec::new()), |(_, sets)| sets.clone())
    }

    /// The page in two bands. Above, the base and its icon, the item's text and rarity, and the
    /// item as the game draws it. Below, the stats with the sockets beside them, as a weapon's
    /// are. Where there are no stats the sockets start at the page's edge at the same width, since
    /// wider rows only stretch their choices. An emblem has no model and shows its nameplate below
    /// instead.
    pub(super) fn draw_gear_editor(&mut self, ui: &mut egui::Ui) {
        match self.recipe.kind {
            ItemKind::Shader => return self.draw_shader_editor(ui),
            ItemKind::Subclass => return self.draw_subclass_editor(ui),
            ItemKind::Mod => return self.draw_mod_editor(ui),
            _ => {}
        }
        ui.spacing_mut().item_spacing.y = 4.0;
        let donor = self.current_gear_donor();
        let plug_sets = donor
            .as_ref()
            .map(|donor| self.gear_plug_sets(donor.summary.hash));
        let model = self.recipe.kind != ItemKind::Emblem && donor.is_some();
        let column = egui::Layout::top_down(egui::Align::Min);
        if let Some(base_width) = workbench_left_column_width(ui.available_width()) {
            let spacing = ui.spacing().item_spacing.x;
            let beside = ui.available_width() - base_width - spacing;
            let preview_width = model
                .then(|| preview_column_width(beside, spacing))
                .flatten();
            let definition_width = beside - preview_width.map_or(0.0, |width| width + spacing);
            ui.horizontal_top(|ui| {
                let base = ui
                    .allocate_ui_with_layout(egui::vec2(base_width, 0.0), column, |ui| {
                        ui.set_width(base_width);
                        self.draw_gear_base(ui);
                        ui.add_space(8.0);
                        self.draw_icon_donor_picker(ui);
                    })
                    .response
                    .rect;
                ui.allocate_ui_with_layout(egui::vec2(definition_width, 0.0), column, |ui| {
                    ui.set_width(definition_width);
                    self.draw_gear_definition(ui, donor.as_ref(), plug_sets.as_ref());
                    // Without the room for a column of its own, the preview goes under the text.
                    if model && preview_width.is_none() {
                        ui.add_space(8.0);
                        self.draw_gear_preview(ui, None);
                    }
                });
                // The preview ends with the base column, which keeps its height while Text
                // Presentation opens and closes.
                if let Some(width) = preview_width {
                    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), column, |ui| {
                        ui.set_width(width);
                        self.draw_gear_preview(ui, Some(base.height()));
                    });
                }
            });
        } else {
            self.draw_gear_base(ui);
            ui.add_space(8.0);
            self.draw_icon_donor_picker(ui);
            ui.separator();
            self.draw_gear_definition(ui, donor.as_ref(), plug_sets.as_ref());
            if model {
                ui.add_space(8.0);
                self.draw_gear_preview(ui, None);
            }
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        if self.recipe.kind == ItemKind::Sparrow {
            self.draw_vehicle_controls(ui);
        }
        let (Some(donor), Some(plug_sets)) = (donor, plug_sets) else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Base item not found in the catalog.",
            );
            return;
        };
        if self.recipe.kind == ItemKind::Emblem {
            self.draw_emblem_trackers(ui);
            ui.add_space(8.0);
            self.draw_emblem_nameplate(ui);
            return;
        }
        let stats: &[&'static str] = match self.recipe.kind {
            ItemKind::Armor => &ARMOR_STATS,
            ItemKind::Sparrow => &SPARROW_STATS,
            _ => &[],
        };
        match workbench_left_column_width(ui.available_width()) {
            Some(stats_width) => {
                let sockets_width =
                    ui.available_width() - stats_width - ui.spacing().item_spacing.x;
                if stats.is_empty() {
                    ui.allocate_ui_with_layout(egui::vec2(sockets_width, 0.0), column, |ui| {
                        ui.set_width(sockets_width);
                        self.draw_gear_sockets(ui, &donor, &plug_sets);
                    });
                } else {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(stats_width, 0.0), column, |ui| {
                            ui.set_width(stats_width);
                            self.draw_gear_stats(ui, &donor, stats);
                        });
                        ui.allocate_ui_with_layout(egui::vec2(sockets_width, 0.0), column, |ui| {
                            ui.set_width(sockets_width);
                            self.draw_gear_sockets(ui, &donor, &plug_sets);
                        });
                    });
                }
            }
            None => {
                if !stats.is_empty() {
                    self.draw_gear_stats(ui, &donor, stats);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                }
                self.draw_gear_sockets(ui, &donor, &plug_sets);
            }
        }
    }

    /// The selected appearance, which drags to turn as the Shader page's preview does.
    /// Beside the item's text it ends where the base column ends, `band`, within its bounds.
    /// Its corner opens it in the model viewer, which has the full tools.
    fn draw_gear_preview(&mut self, ui: &mut egui::Ui, band: Option<f32>) {
        let id = egui::Id::new("gear-preview");
        let width = ui.available_width();
        let height = preview_height(width, band);
        // A Sparrow that summons another vehicle shows that vehicle.
        let summoned = self.recipe.overrides.sparrow.as_ref().and_then(|sparrow| {
            let tag = sparrow.summon.entity().ok().flatten()?;
            Some((tag, sparrow.summon.label()))
        });
        if let Some((tag, name)) = summoned {
            // The Sparrow's icon is not the vehicle's.
            sundial::ui::model_preview::still::placeholder(ui.ctx(), id, None);
            let response = sundial::ui::model_preview::still::show_object(
                ui,
                id,
                &self.packages,
                Some(tag),
                egui::vec2(width, height),
            );
            sundial::ui::model_preview::pop_out_object(
                ui,
                id,
                response.rect,
                &self.packages,
                (tag, name),
            );
            return;
        }
        #[cfg(feature = "d2-model-importer")]
        if let Some(reference) = &self.recipe.overrides.imported_graph {
            let appearance = crate::imported::preview::appearance(reference, self.recipe.kind);
            sundial::ui::model_preview::still::placeholder(ui.ctx(), id, None);
            let response = sundial::ui::model_preview::still::show_local(
                ui,
                id,
                &self.packages,
                appearance.clone(),
                egui::vec2(width, height),
            );
            sundial::ui::model_preview::pop_out_local(
                ui,
                id,
                response.rect,
                &self.packages,
                (appearance, &self.recipe.name),
            );
            return;
        }
        let (Some(catalog), Ok(hash)) = (
            self.catalog.as_ref(),
            self.recipe.donor.item_hash.parse_u32(),
        ) else {
            return;
        };
        // No shader rows leave the item's own dyes.
        let appearance =
            catalog.shader_preview_appearance(hash, &[Vec::new(), Vec::new(), Vec::new()]);
        // The item's icon stands in until its model is read.
        sundial::ui::model_preview::still::placeholder(
            ui.ctx(),
            id,
            catalog.item_icon(ui.ctx(), hash),
        );
        let response = sundial::ui::model_preview::still::show(
            ui,
            id,
            &self.packages,
            appearance.clone(),
            &[],
            egui::vec2(width, height),
        );
        if let Some(appearance) = appearance {
            let name = catalog.item_display_name(hash).unwrap_or("Base Item");
            sundial::ui::model_preview::pop_out(
                ui,
                id,
                response.rect,
                &self.packages,
                (appearance, name),
            );
        }
    }

    pub(super) fn draw_gear_base(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "d2-model-importer")]
        if source::show(self, ui) {
            return;
        }
        let kind = self.recipe.kind;
        draw_donor_section_label(
            ui,
            &format!("Base {}", kind.label()),
            Some(match kind {
                ItemKind::Shader => "Starting dyes.",
                ItemKind::Subclass => "Class, element and starting abilities.",
                ItemKind::Emblem => "Starting icon and nameplate.",
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
        let selected =
            current_hash.and_then(|hash| candidates.iter().find(|donor| donor.hash == hash));
        let selected_text = selected.map_or_else(
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
            let armor_row = |hash: u32| {
                let slots = slots.get_or_init(|| {
                    candidates
                        .iter()
                        .map(|donor| (donor.hash, donor.type_name.as_str()))
                        .collect::<BTreeMap<_, _>>()
                });
                Some(armor_detail(catalog, hash, slots.get(&hash).copied()))
            };
            let row_detail: Option<&dyn Fn(u32) -> Option<String>> = match kind {
                ItemKind::Armor => Some(&armor_row),
                ItemKind::Subclass => Some(&class_detail),
                _ => None,
            };
            // The base's own card names them too. A subclass's type already names its class, as
            // "Hunter Subclass".
            let selected_detail = selected
                .filter(|_| kind == ItemKind::Armor)
                .map(|donor| armor_detail(catalog, donor.hash, Some(donor.type_name.as_str())));
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
                    selected_detail: selected_detail.as_deref(),
                },
            )
        });
        if self.catalog.is_none() {
            ui.add_enabled(false, egui::Button::new(selected_text));
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
        let inherited_class =
            donor.and_then(|donor| self.catalog.as_ref()?.item_class_type(donor.summary.hash));
        let fields: &[usize] = if kind == ItemKind::Armor {
            if energy.is_some() {
                &[0, 3, 1, 2]
            } else {
                &[0, 3]
            }
        } else if energy.is_some() {
            &[0, 1, 2]
        } else if kind == ItemKind::Sparrow {
            &[0, 4]
        } else {
            &[0]
        };
        style::tiles(ui, |ui, width| {
            for &field in fields {
                style::tile_column(ui, (width, ("gear-field", field)), |column| match field {
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
                    3 => draw_armor_class(column, &mut self.recipe.overrides, inherited_class),
                    4 => vehicle::draw_summon(column, self),
                    _ => {
                        if let (Some(energy), Some(donor)) = (&energy, donor) {
                            draw_energy_capacity(column, &mut self.recipe, donor, energy);
                        }
                    }
                });
            }
        });
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
        let changed = !self.recipe.overrides.investment_stats.is_empty();
        if stats_header(ui, kind, cap, changed) {
            self.recipe.overrides.investment_stats.clear();
        }
        ui.add_space(4.0);
        let core = rows.iter().filter(|row| row.core).collect::<Vec<_>>();
        let total = (kind == ItemKind::Armor && !core.is_empty()).then(|| {
            (
                core.iter().map(|row| row.value()).sum::<i32>(),
                core.iter().map(|row| row.base()).sum::<i32>(),
            )
        });
        let columns = StatColumns::new(
            ui,
            rows.iter()
                .map(|row| row.name.as_str())
                .chain(total.map(|_| "Total")),
            (0.0, 0.0),
        );
        // A full bar is the cap, or the largest value where the group sets none.
        let scale = cap.unwrap_or_else(|| {
            rows.iter()
                .map(|row| row.value().max(row.base()))
                .max()
                .unwrap_or(0)
                .max(100)
        });
        let mut edits = rows
            .iter()
            .filter_map(|row| draw_stat_row(ui, &columns, row, (cap, scale)))
            .collect::<Vec<_>>();
        if let Some(total) = total {
            draw_stat_total(ui, &columns, total);
        }
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
                                ui.close();
                            }
                        }
                    });
            });
        });
        for (definition_index, value) in edits {
            set_item_stat(&mut self.recipe, definition_index, value);
        }
    }

    /// The item's sockets, edited the way a weapon's are: added, removed or given another role.
    /// The energy sockets keep theirs, since the energy controls own them.
    fn draw_gear_sockets(&mut self, ui: &mut egui::Ui, donor: &WeaponDonor, plug_sets: &PlugSets) {
        let has_authored_columns = !self.recipe.overrides.socket_columns.is_empty()
            || !self.recipe.overrides.socket_plug_variants.is_empty();
        ui.horizontal_wrapped(|ui| {
            style::heading(ui, "Perks & Sockets", has_authored_columns);
            draw_authoring_info_icon(
                ui,
                "The first choice starts equipped. Right-click a choice to make it the default.",
            );
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
}

fn draw_armor_class(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    inherited: Option<u8>,
) {
    let base = inherited.map_or("Base Class", |class| {
        class_label(class).unwrap_or("Any Class")
    });
    let (label, reset) = style::stock_field_name(
        ui,
        "Class",
        "Which class can equip it. Any Class removes the restriction. Collections and badges \
         follow it",
        overrides.armor_class.map(|_| base),
    );
    if reset {
        overrides.armor_class = None;
    }
    egui::ComboBox::from_id_salt("armor_class")
        .selected_text(overrides.armor_class.map_or(base, |class| class.label()))
        .width(ui.available_width())
        .truncate()
        .show_ui(ui, |ui| {
            workbench_style(ui);
            ui.selectable_value(
                &mut overrides.armor_class,
                None,
                follow_base(ItemKind::Armor),
            );
            for class in crate::ArmorClass::ALL {
                ui.selectable_value(&mut overrides.armor_class, Some(class), class.label());
            }
        })
        .response
        .labelled_by(label.id);
}

pub(super) fn draw_gear_rarity(
    ui: &mut egui::Ui,
    overrides: &mut WeaponRecipeOverrides,
    inherited: WeaponRarity,
    kind: ItemKind,
    branding: crate::branding::Branding,
) {
    let imported = {
        #[cfg(feature = "d2-model-importer")]
        {
            overrides.imported_graph.is_some()
        }
        #[cfg(not(feature = "d2-model-importer"))]
        {
            false
        }
    };
    // An imported item has no base whose rarity it could follow.
    let (default_label, follow) = if imported {
        ("Default Rarity", "Default Rarity".to_owned())
    } else {
        (inherited.label(), follow_base(kind))
    };
    let hint = if imported {
        if kind == ItemKind::Armor {
            "Exotic armor appears under Exotics for its class and equips one piece at a time"
                .to_owned()
        } else {
            "Its rarity in Collections and on its inventory icon".to_owned()
        }
    } else if crate::collection::GearPage::for_kind(kind).is_some() {
        format!(
            "Any rarity appears on the {} page under {}",
            branding.name(),
            kind.plural()
        )
    } else {
        "Exotic needs an Exotic base. Exotic armor appears under Exotics beside its base and \
         equips one piece at a time"
            .to_owned()
    };
    let (label, reset) =
        style::stock_field_name(ui, "Rarity", &hint, overrides.rarity.map(|_| default_label));
    if reset {
        overrides.rarity = None;
    }
    let base_exotic = inherited == WeaponRarity::Exotic;
    let selected_text = overrides.rarity.map_or(default_label, recipe_rarity_label);
    egui::ComboBox::from_id_salt("gear_rarity")
        .selected_text(selected_text)
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            if ui
                .selectable_label(overrides.rarity.is_none(), follow)
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
                // Exotic armor stays with Exotic bases, which decide its Collections page.
                let allowed = imported
                    || kind != ItemKind::Armor
                    || (rarity == RecipeRarity::Exotic) == base_exotic;
                if ui
                    .add_enabled(
                        allowed,
                        egui::Button::selectable(
                            overrides.rarity == Some(rarity),
                            recipe_rarity_label(rarity),
                        ),
                    )
                    .clicked()
                {
                    overrides.rarity = Some(rarity);
                }
            }
        })
        .response
        .labelled_by(label.id);
}

fn draw_energy_type(
    ui: &mut egui::Ui,
    recipe: &mut WeaponRecipe,
    donor: &WeaponDonor,
    energy: &EnergySocket,
) {
    const HINT: &str = "Arc, Solar or Void Energy for its mods";
    let current = energy.current.map(|(element, _)| element);
    let base = energy.base.map(|(element, _)| element);
    let modified = current != base;
    let (name, reset) = match base {
        Some(base) => style::stock_field_name(ui, "Energy Type", HINT, modified.then_some(base)),
        None => style::field_name(ui, "Energy Type", HINT, modified),
    };
    // The base's own type, at the capacity the armor has now where that type offers it.
    let mut chosen = None;
    if reset {
        match base {
            Some(element) => chosen = Some(element),
            None => restore_socket(recipe, energy.index),
        }
    }
    let combo = egui::ComboBox::from_id_salt("gear_energy_type")
        .selected_text(current.unwrap_or_default())
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
                        egui::Button::selectable(current == Some(element), element),
                    )
                    .clicked()
                {
                    chosen = Some(element);
                }
            }
        });
    combo.response.labelled_by(name.id);
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
    const HINT: &str = "Stock armor tops out at 10. Capacity 10 adds 2 to every stat";
    let base_capacity = energy.base.map(|(_, capacity)| capacity);
    let modified = energy.current.map(|(_, capacity)| capacity) != base_capacity;
    let (name, reset) = match base_capacity.map(|capacity| capacity.to_string()) {
        Some(base) => style::stock_field_name(
            ui,
            "Energy Capacity",
            HINT,
            modified.then_some(base.as_str()),
        ),
        None => style::field_name(ui, "Energy Capacity", HINT, modified),
    };
    let Some((element, current)) = energy.current else {
        ui.add_enabled(false, egui::Button::new("Unknown"));
        return;
    };
    // The base's own capacity in the type the armor has now, or the base's own plug where that
    // type does not offer it.
    let mut chosen = None;
    if reset {
        chosen = base_capacity.and_then(|capacity| energy.plugs.get(&(element, capacity)).copied());
        if chosen.is_none() {
            restore_socket(recipe, energy.index);
        }
    }
    let combo = egui::ComboBox::from_id_salt("gear_energy_capacity")
        .selected_text(current.to_string())
        .truncate()
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            workbench_style(ui);
            for (&(candidate, capacity), &plug) in &energy.plugs {
                if candidate == element
                    && ui
                        .selectable_label(capacity == current, capacity.to_string())
                        .clicked()
                {
                    chosen = Some(plug);
                }
            }
        });
    combo.response.labelled_by(name.id);
    if let (Some(plug), Some(socket)) = (chosen, donor.sockets.get(energy.index)) {
        set_socket_plug(recipe, donor, socket, plug);
    }
}
