//! Properties: the values of an ability and of the graphs it spawns whose meaning is
//! established. The ability's own join its Ability card, and each graph it spawns has a card, in
//! the order the ability spawns them. A projectile's
//! card picks the projectile it fires and how it flies. Every card holds the graph's timers that
//! run out, the changes its effects make to the player and weapons, each named by the input it
//! changes, its invisibility, and a movement ability's traced controller values, such as a
//! Blink's distance and a jump's airborne jumps. Graphs that hold the same values share a card, and an edit
//! there changes each of them. A value change is the same override Raw Values writes,
//! and a projectile another stock ability's, so the build changes copies only this
//! ability uses.
use super::tuning::SPAWN_DEPTH;
use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::ability_damage;
use sundial::package_authoring::ability_movement::{self, MovementValue, RowLane, Unit};
use sundial::package_authoring::ability_settings::{
    self, Kind as SettingKind, Setting, Unit as SettingUnit,
};
use sundial::package_authoring::ability_spawns::{describe, spawns, table_graphs, tables};
use sundial::package_authoring::runtime::modifiers;
use sundial::package_authoring::runtime::{WeaponRuntimeField, WeaponRuntimeValue};
use sundial::package_authoring::sandbox_perk::entity::effect_length::{
    self, EffectLength, UNLIMITED,
};
use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{
    self, Curve, CurveKind, Kind, Parameter,
};

mod property;
mod readings;
mod search;
mod swap;
mod tree;
use property::*;
use readings::*;
pub(super) use search::{matches, search_bar};
use swap::*;
use tree::*;
pub(super) use tree::{Properties, Tree};

/// Where an entity graph keeps the client's object type, and the type of a projectile.
const OBJECT_TYPE: usize = 0x96;
const PROJECTILE: u8 = 18;
/// Component Modifiers: records that add to or multiply an input of a component of the player
/// or a weapon.
const MODIFIERS: u32 = 0x8080_3B00;
/// Invisibility Attachment, whose named fields set how strong the invisibility is and how it
/// fades.
const INVISIBILITY: u32 = 0x8080_43DF;
/// The settings record's ability slot, read when the record changes the Abilities component.
const ABILITY_OFFSET: u32 = 0x48;
const ABILITIES: i64 = 9;

/// A closed section of a card: `title`, with what it holds on hover, and its tiles once opened. It
/// is marked once one of them holds an edit.
fn fold(
    ui: &mut egui::Ui,
    (salt, title, hover): (impl std::hash::Hash + std::fmt::Debug, &str, &str),
    edited: bool,
    tiles: impl FnOnce(&mut egui::Ui, f32),
) {
    let marked = format!("{title} •");
    let section = egui::CollapsingHeader::new(if edited { marked.as_str() } else { title })
        .id_salt(salt)
        .default_open(false)
        .show(ui, |ui| style::tiles(ui, tiles));
    let header = section.header_response.on_hover_text(hover);
    // A screen reader hears the edit dot as "Changed".
    if edited {
        style::named_control(header, format!("{title}, Changed"));
    }
}

/// What a card's Unconfirmed section says it holds.
const UNCONFIRMED: &str = "Values whose role is not yet traced";

/// A card's More section, for the values most authors leave alone, in the same groups as the
/// card's own tiles. The offsets and scaling the game adds to its timers wait in a closed
/// Adjustments section at its end.
fn more_fold(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    (properties, lanes): (&[Property], &[&RowLane]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let tucked = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| {
            (more(property) || idle(property, properties, values)) && !shadowed(lanes, property)
        })
        .collect::<Vec<_>>();
    if tucked.is_empty() {
        return;
    }
    let edited = tucked
        .iter()
        .any(|(_, property)| property.is_modified(values));
    let (adjustments, rest): (Vec<_>, Vec<_>) = tucked
        .into_iter()
        .partition(|(_, property)| Group::of(property) == Group::Adjustments);
    let title = if edited { "More •" } else { "More" };
    let section = egui::CollapsingHeader::new(title)
        .id_salt((salt, "more"))
        .default_open(false)
        .show(ui, |ui| {
            grouped_tiles(ui, &rest, properties, values);
            if adjustments.is_empty() {
                return;
            }
            let edited = adjustments
                .iter()
                .any(|(_, property)| property.is_modified(values));
            let name = (
                (salt, "adjustments"),
                Group::Adjustments.title(),
                Group::Adjustments.hint(),
            );
            fold(ui, name, edited, |ui, width| {
                for (index, property) in adjustments {
                    let inactive = idle(property, properties, values);
                    paired_tile(ui, width, index, (property, properties), values, inactive);
                }
            });
        });
    let header = section
        .header_response
        .on_hover_text("Values most abilities keep as they are");
    // A screen reader hears the edit dot as "Changed".
    if edited {
        style::named_control(header, "More, Changed");
    }
}

/// A card's lead tiles: the values it shows, in groups.
fn lead_tiles(
    ui: &mut egui::Ui,
    properties: &[Property],
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let members = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| shown(property, properties, &[], values))
        .collect::<Vec<_>>();
    grouped_tiles(ui, &members, properties, values);
}

/// One group's values: its single values, and each value a numbered series repeats, with its
/// members by number.
struct Section<'a> {
    group: Group,
    singles: Vec<(usize, &'a Property)>,
    series: Vec<(&'a str, Vec<Member<'a>>)>,
}

/// One member of a numbered series: its number, its index among the members, and the value.
type Member<'a> = (usize, usize, &'a Property);

/// `members` by group. A group's single values sit in a block under its name, the blocks side by
/// side in one wrapping flow so a card stays dense. A group that has a numbered series, such as
/// Conditional Damage 1 to 7 or the four bounce records, takes the card's width instead: its
/// single values, then the series as a table with a row for each value and a column for each
/// number, which lines the records up for comparison.
fn grouped_tiles(
    ui: &mut egui::Ui,
    members: &[(usize, &Property)],
    properties: &[Property],
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    if members.is_empty() {
        return;
    }
    let sections = sections(members);
    // One group needs no name of its own: the card's title already says what its values are.
    let captioned = sections.len() > 1;
    let (flowing, tabled): (Vec<_>, Vec<_>) = sections
        .into_iter()
        .partition(|section| section.series.is_empty());
    if !flowing.is_empty() {
        style::tiles(ui, |ui, width| {
            let gap = ui.spacing().item_spacing.x;
            let line = ui.available_width();
            let per_line = (((line + gap) / (width + gap)) as usize).max(1);
            for section in &flowing {
                let count = section.singles.len().clamp(1, per_line);
                let span = width * count as f32 + gap * (count - 1) as f32;
                ui.allocate_ui_with_layout(
                    egui::vec2(span, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(span);
                        if captioned {
                            group_caption(ui, section.group.title(), section.group.hint());
                        }
                        let wrap =
                            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
                        ui.with_layout(wrap, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(gap, 8.0);
                            for &(index, property) in &section.singles {
                                group_tile(ui, width, (index, property), properties, values);
                            }
                        });
                    },
                );
            }
        });
    }
    for section in tabled {
        ui.add_space(4.0);
        if captioned {
            group_caption(ui, section.group.title(), section.group.hint());
        }
        if !section.singles.is_empty() {
            style::tiles(ui, |ui, width| {
                for &(index, property) in &section.singles {
                    group_tile(ui, width, (index, property), properties, values);
                }
            });
        }
        let columns = section
            .series
            .iter()
            .flat_map(|(_, entries)| entries.iter().map(|(number, ..)| *number))
            .max()
            .unwrap_or(1);
        let headings = (1..=columns).map(|n| n.to_string()).collect::<Vec<_>>();
        let rows = section
            .series
            .iter()
            .map(|(base, entries)| TableRow {
                label: (*base).to_owned(),
                hint: entries[0].2.hint.as_str(),
                cells: (1..=columns)
                    .map(|number| {
                        entries
                            .iter()
                            .find(|(each, ..)| *each == number)
                            .map(|&(_, index, property)| (index, property, properties))
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        value_table(ui, ("series", section.group), &headings, rows, values);
    }
}

/// A card's title, above its groups' 13-point names.
fn card_title(title: &str) -> egui::RichText {
    egui::RichText::new(title).size(15.0).strong()
}

/// A group's name over its values: above their 12-point names and below the card's title.
fn group_caption(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add(egui::Label::new(egui::RichText::new(title).size(13.0).strong()).truncate())
        .on_hover_text(hint);
}

/// One value's tile in a group, keeping a pair such as a minimum and maximum in order.
fn group_tile(
    ui: &mut egui::Ui,
    width: f32,
    (index, property): (usize, &Property),
    properties: &[Property],
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let inactive = idle(property, properties, values);
    let before = property.current(values);
    property_tile_named(
        ui,
        width,
        index,
        (property, &property.label),
        values,
        inactive,
    );
    keep_ordered(property, before, properties, values);
}

/// `members` by group in order: each group's single values and its numbered series, a series
/// being a value that repeats under numbers, as "Surface Placement 2" follows "Surface Placement".
fn sections<'a>(members: &[(usize, &'a Property)]) -> Vec<Section<'a>> {
    let mut out = Vec::new();
    for group in Group::ALL {
        let mut series = Vec::<(&str, Vec<Member>)>::new();
        for &(index, property) in members {
            if Group::of(property) != group {
                continue;
            }
            let (base, number) = numbered(&property.label);
            match series.iter_mut().find(|(each, _)| *each == base) {
                Some((_, entries)) => entries.push((number, index, property)),
                None => series.push((base, vec![(number, index, property)])),
            }
        }
        if series.is_empty() {
            continue;
        }
        let singles = series
            .iter()
            .filter(|(_, entries)| entries.len() == 1)
            .map(|(_, entries)| (entries[0].1, entries[0].2))
            .collect();
        let mut repeated = series
            .into_iter()
            .filter(|(_, entries)| entries.len() > 1)
            .collect::<Vec<_>>();
        for (_, entries) in &mut repeated {
            entries.sort_by_key(|(number, ..)| *number);
        }
        out.push(Section {
            group,
            singles,
            series: repeated,
        });
    }
    out
}

/// One row of a value table: its name, what it is on hover, and in each column the property it
/// edits there, with the card's properties around it, or none. A row without cells names the
/// group the rows under it belong to.
struct TableRow<'a> {
    label: String,
    hint: &'a str,
    cells: Vec<Option<(usize, &'a Property, &'a [Property])>>,
}

/// The table's narrowest and widest value column.
const CELL_MIN_WIDTH: f32 = 72.0;
const CELL_MAX_WIDTH: f32 = 160.0;
/// The room a changed cell's way back takes beside its field.
const CELL_RESTORE: f32 = 18.0;

/// Values that compare across columns, such as the records of a series or the variants of a part:
/// a name column, then a column under each of `headings`. A row's name brightens once one of its
/// values changes, and reads Inactive beside it when another setting turns every one of them off.
fn value_table(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    headings: &[String],
    rows: Vec<TableRow<'_>>,
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let gap = 12.0;
    let name_width = rows
        .iter()
        .map(|row| label_width(ui, &row.label) + label_width(ui, " Inactive"))
        .fold(60.0, f32::max)
        .min(260.0)
        .ceil();
    let columns = headings.len().max(1);
    let room = ui.available_width() - name_width - gap * columns as f32;
    let width = (room / columns as f32)
        .clamp(CELL_MIN_WIDTH, CELL_MAX_WIDTH)
        .floor();
    let line = ui.spacing().interact_size.y;
    egui::Grid::new(ui.id().with(salt))
        .spacing([gap, 6.0])
        .show(ui, |ui| {
            ui.allocate_space(egui::vec2(name_width, line));
            for heading in headings {
                let text = egui::RichText::new(heading)
                    .size(12.0)
                    .color(style::secondary(ui.visuals()));
                ui.add_sized(
                    [width, line],
                    egui::Label::new(text).truncate().halign(egui::Align::Center),
                )
                .on_hover_text(heading);
            }
            ui.end_row();
            for (row_index, row) in rows.into_iter().enumerate() {
                if row.cells.is_empty() {
                    group_caption(ui, &row.label, row.hint);
                    ui.end_row();
                    continue;
                }
                let present = row.cells.iter().flatten().collect::<Vec<_>>();
                let modified = present
                    .iter()
                    .any(|(_, property, _)| property.is_modified(values));
                let inactive = !present.is_empty()
                    && present
                        .iter()
                        .all(|(_, property, siblings)| idle(property, siblings, values));
                ui.allocate_ui_with_layout(
                    egui::vec2(name_width, line),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(name_width);
                        let text = egui::RichText::new(&row.label).size(12.0);
                        let text = if modified {
                            text
                        } else {
                            text.color(style::secondary(ui.visuals()))
                        };
                        let hover = if row.hint.is_empty() {
                            row.label.clone()
                        } else {
                            format!("{}\n{}", row.label, row.hint)
                        };
                        ui.add(egui::Label::new(text).truncate())
                            .on_hover_text(hover);
                        if inactive {
                            let text = egui::RichText::new("Inactive")
                                .size(12.0)
                                .color(style::muted_warning(ui.visuals()));
                            ui.label(text).on_hover_text(
                                "Another setting disables this value. You can still edit or reset it.",
                            );
                        }
                    },
                );
                for (column, cell) in row.cells.into_iter().enumerate() {
                    match cell {
                        Some((index, property, siblings)) => {
                            ui.push_id((row_index, column), |ui| {
                                property_cell(ui, (index, width), (property, siblings), values);
                            });
                        }
                        None => {
                            ui.allocate_space(egui::vec2(width, line));
                        }
                    }
                }
                ui.end_row();
            }
        });
}

/// A property's field in a table cell, which its row names, with a way back beside it once the
/// value changes.
fn property_cell(
    ui: &mut egui::Ui,
    (index, width): (usize, f32),
    (property, siblings): (&Property, &[Property]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    property.keep_unlimited(values);
    let current = property.current(values);
    let modified = property.is_modified(values);
    let (edited, reset) = ui
        .allocate_ui_with_layout(
            egui::vec2(width, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let field = (width - CELL_RESTORE).max(CELL_MIN_WIDTH - CELL_RESTORE);
                let edited = property_control(ui, (index, field), property, current);
                let reset = modified && {
                    let stock = property.reading(f64::from(property.stock));
                    let icon = egui::RichText::new(egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE)
                        .size(12.0);
                    let response = ui
                        .add(egui::Button::new(icon).frame(false).small())
                        .on_hover_text(format!("Restore {stock}"));
                    style::named_control(response, format!("Restore {} to {stock}", property.label))
                        .clicked()
                };
                (edited, reset)
            },
        )
        .inner;
    let before = current;
    if let Some(value) = edited {
        property.set(values, value);
    } else if reset {
        property.reset(values);
    }
    keep_ordered(property, before, siblings, values);
}

/// Sibling cards that compare as variants, each group leader first: cards of one kind under the
/// same card, with the same alternative standing, neither a projectile, which has its swap, nor
/// skipped, sharing at least half their value names. Towering Barricade's four barricade objects
/// are one such group, as are two effects an object attaches that hold the same kinds of values.
fn variant_groups(cards: &[Card], skip: impl Fn(&Card) -> bool) -> Vec<Vec<usize>> {
    // What a card changes, without the timing, conflict and adjustment values nearly every part
    // carries, so two effects alike only in those, such as Move Speed and Shield Piercing, stay
    // apart. Two cards with nothing else compare by those.
    let names = |card: &Card| {
        let mut all = BTreeSet::new();
        let mut changes = BTreeSet::new();
        for property in &card.properties {
            let name = numbered(&property.label).0.to_owned();
            if !matches!(
                Group::of(property),
                Group::Timing | Group::Conflicts | Group::Adjustments
            ) {
                changes.insert(name.clone());
            }
            all.insert(name);
        }
        (all, changes)
    };
    let eligible = |card: &Card| !card.projectile && !card.properties.is_empty() && !skip(card);
    let mut taken = BTreeSet::new();
    let mut groups = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        if taken.contains(&index) || !eligible(card) {
            continue;
        }
        let own = names(card);
        let mut group = vec![index];
        for (other_index, other) in cards.iter().enumerate().skip(index + 1) {
            if taken.contains(&other_index)
                || !eligible(other)
                || other.above != card.above
                || other.kind != card.kind
                || other.alternative != card.alternative
            {
                continue;
            }
            let theirs = names(other);
            let (mine, yours) = if own.1.is_empty() && theirs.1.is_empty() {
                (&own.0, &theirs.0)
            } else {
                (&own.1, &theirs.1)
            };
            let shared = mine.intersection(yours).count();
            if shared > 0 && shared * 2 >= mine.union(yours).count() {
                group.push(other_index);
            }
        }
        if group.len() > 1 {
            taken.extend(group.iter().copied());
            groups.push(group);
        }
    }
    groups
}

/// The rows a variant table holds for the values `keep` selects, group by group: a caption row
/// where the table spans several groups, then a row for each value name in the order the
/// variants list them, with each variant's property of that name.
fn variant_rows<'a>(
    cards: &[&'a Card],
    keep: impl Fn(&Property, &[Property]) -> bool,
) -> Vec<TableRow<'a>> {
    let mut sections = Vec::<(Group, Vec<&'a str>)>::new();
    for group in Group::ALL {
        let mut labels = Vec::<&str>::new();
        for card in cards {
            for property in &card.properties {
                if Group::of(property) == group
                    && keep(property, &card.properties)
                    && !labels.contains(&property.label.as_str())
                {
                    labels.push(property.label.as_str());
                }
            }
        }
        if !labels.is_empty() {
            sections.push((group, labels));
        }
    }
    let captioned = sections.len() > 1;
    let mut rows = Vec::new();
    for (group, labels) in sections {
        if captioned {
            rows.push(TableRow {
                label: group.title().to_owned(),
                hint: group.hint(),
                cells: Vec::new(),
            });
        }
        for &label in &labels {
            // A series' first row reads 1 beside the rows numbered after it.
            let shown = if labels.iter().any(|other| numbered(other) == (label, 2)) {
                format!("{label} 1")
            } else {
                label.to_owned()
            };
            let cells = cards
                .iter()
                .map(|card| {
                    card.properties
                        .iter()
                        .enumerate()
                        .find(|(_, property)| {
                            property.label == label && keep(property, &card.properties)
                        })
                        .map(|(index, property)| (index, property, card.properties.as_slice()))
                })
                .collect::<Vec<_>>();
            let hint = cells
                .iter()
                .flatten()
                .next()
                .map_or("", |(_, property, _)| property.hint.as_str());
            rows.push(TableRow {
                label: shown,
                hint,
                cells,
            });
        }
    }
    rows
}

/// One card for variants that compare: their shared name, or their kind where names differ, and
/// a table with a column for each variant and a row for each value, the values most abilities
/// keep as they are in its closed More section.
fn variant_card(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    cards: &[&Card],
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let first = cards[0];
    let title = if cards.iter().all(|card| card.name == first.name) {
        first.name.clone()
    } else {
        first.kind.clone()
    };
    let tags = cards
        .iter()
        .flat_map(|card| card.graphs.iter().map(|(tag, _)| format!("0x{tag:08X}")))
        .collect::<Vec<_>>()
        .join(", ");
    ui.horizontal(|ui| {
        ui.label(card_title(&title)).on_hover_text(tags);
        ui.weak(format!("{} Parts", cards.len()))
            .on_hover_text("Parts of the same kind side by side. Each column edits its own part.");
        if first.alternative {
            ui.label(quiet(ui, "Alternative"))
                .on_hover_text("Spawns only when a perk or node selects it");
        }
    });
    // The component a title names after its kind is the same for every variant, so a heading is
    // the part before it, such as Object 1.
    let headings = cards
        .iter()
        .map(|card| {
            card.title
                .split_once(" · ")
                .map_or_else(|| card.title.clone(), |(own, _)| own.to_owned())
        })
        .collect::<Vec<_>>();
    let reading: &[WeaponRuntimeValueOverride] = values;
    let lead = variant_rows(cards, |property, properties| {
        shown(property, properties, &[], reading)
    });
    let tucked = variant_rows(cards, |property, properties| {
        confirmed(property) && (more(property) || idle(property, properties, reading))
    });
    let unconfirmed = variant_rows(cards, |property, _| !confirmed(property));
    if !lead.is_empty() {
        value_table(ui, (salt, "lead"), &headings, lead, values);
    }
    for (rows, title, hover) in [
        (tucked, "More", "Values most abilities keep as they are"),
        (unconfirmed, "Unconfirmed", UNCONFIRMED),
    ] {
        if rows.is_empty() {
            continue;
        }
        let edited = rows
            .iter()
            .flat_map(|row| row.cells.iter().flatten())
            .any(|(_, property, _)| property.is_modified(values));
        let marked = format!("{title} •");
        let section = egui::CollapsingHeader::new(if edited { marked.as_str() } else { title })
            .id_salt((salt, title))
            .default_open(false)
            .show(ui, |ui| {
                value_table(ui, (salt, title, 1), &headings, rows, values)
            });
        let header = section.header_response.on_hover_text(hover);
        if edited {
            style::named_control(header, format!("{title}, Changed"));
        }
    }
}

/// How wide `text` reads at a tile name's size.
fn label_width(ui: &egui::Ui, text: &str) -> f32 {
    egui::WidgetText::from(egui::RichText::new(text).size(12.0))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

/// A label's name without the number a repeated value takes, and that number, 1 for a first
/// that has none, as "Surface Placement 2" follows "Surface Placement".
fn numbered(label: &str) -> (&str, usize) {
    label
        .rsplit_once(' ')
        .and_then(|(base, number)| {
            number
                .parse::<usize>()
                .ok()
                .filter(|number| (1..=9).contains(number))
                .map(|number| (base, number))
        })
        .unwrap_or((label, 1))
}

/// What a value is for, so a card reads in groups rather than as one wall of tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Group {
    Flight,
    Lifetime,
    Collision,
    Bouncing,
    Contact,
    Health,
    DamageTaken,
    Invisibility,
    Targeting,
    Tracking,
    Spawning,
    Changes,
    Movement,
    Energy,
    Timing,
    Conflicts,
    Adjustments,
}

impl Group {
    const ALL: [Self; 17] = [
        Self::Flight,
        Self::Lifetime,
        Self::Collision,
        Self::Bouncing,
        Self::Contact,
        Self::Health,
        Self::DamageTaken,
        Self::Invisibility,
        Self::Targeting,
        Self::Tracking,
        Self::Spawning,
        Self::Changes,
        Self::Movement,
        Self::Energy,
        Self::Timing,
        Self::Conflicts,
        Self::Adjustments,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Flight => "Flight",
            Self::Lifetime => "Lifetime",
            Self::Collision => "Collision",
            Self::Bouncing => "Bouncing",
            Self::Contact => "Contact",
            Self::Health => "Health",
            Self::DamageTaken => "Damage Taken",
            Self::Invisibility => "Invisibility",
            Self::Targeting => "Targeting",
            Self::Tracking => "Tracking",
            Self::Spawning => "Spawning",
            Self::Changes => "Changes",
            Self::Movement => "Movement",
            Self::Energy => "Energy",
            Self::Timing => "Timing",
            Self::Conflicts => "Conflicts",
            Self::Adjustments => "Adjustments",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Flight => "How it flies",
            Self::Lifetime => "When it finishes and expires",
            Self::Collision => "What it hits and passes through",
            Self::Bouncing => "How it bounces",
            Self::Contact => "What happens when it touches a surface",
            Self::Health => "Its health and how it recovers",
            Self::DamageTaken => "Damage its wearer takes",
            Self::Invisibility => "How strong it is and what breaks it",
            Self::Targeting => "How it looks for targets",
            Self::Tracking => "How it steers toward a target",
            Self::Spawning => "What it creates and how many",
            Self::Changes => "What it changes on the player and weapons",
            Self::Movement => "How the player moves",
            Self::Energy => "Ability energy",
            Self::Timing => "Delays, cycles and lengths",
            Self::Conflicts => "Which effect wins when effects of a group overlap",
            Self::Adjustments => "Offsets and scaling the game adds to the values above",
        }
    }

    fn of(property: &Property) -> Self {
        use sundial::package_authoring::ability_settings::NativeProperty as N;
        let settings = match &property.value {
            Value::Flight(_) | Value::Curve(_) => return Self::Flight,
            // A timer's offset and scaling are its length, as a Self-Destruct Timer's are, so they
            // lead with the other lengths.
            Value::Duration(_) | Value::Timer { .. } => return Self::Timing,
            Value::Amount { .. } => return Self::Changes,
            // Named component fields come from invisibility attachments.
            Value::Field(_) => return Self::Invisibility,
            Value::Movement(_) => return Self::Movement,
            Value::Setting(settings) => settings,
        };
        match settings[0].kind {
            SettingKind::ImpulseHoldTime
            | SettingKind::ImpulseRampTime
            | SettingKind::ImpulseFadeTime
            | SettingKind::VerticalBias => Self::Flight,
            SettingKind::ExpirationTime
            | SettingKind::MinimumFinishTime
            | SettingKind::MaximumFinishTime
            | SettingKind::FinishTravelTime
            | SettingKind::ExpirationUpdates
            | SettingKind::ExpirationResponse
            | SettingKind::FallSpeedThreshold => Self::Lifetime,
            SettingKind::PierceLimit
            | SettingKind::CollisionRadius
            | SettingKind::CollisionMode
            | SettingKind::Native(N::CollisionLimit | N::LaunchSpeedWeight) => Self::Collision,
            SettingKind::BounceLimit
            | SettingKind::Native(
                N::BounceCountIncrement
                | N::BounceAngleVariation
                | N::BounceSpeedVariation
                | N::BounceSurfaceRadius,
            ) => Self::Bouncing,
            SettingKind::Native(
                N::CleanupOnContact
                | N::SurfacePlacement
                | N::ContactSpeed
                | N::ContactFinalSpeed
                | N::ContactFinalGravity
                | N::ContactCurveStart
                | N::ContactCurveEnd
                | N::ContactCleanupDelay
                | N::ContactTriggerDelay
                | N::ContactCleanupVariation,
            ) => Self::Contact,
            SettingKind::HealthScale
            | SettingKind::RecoveryDelay
            | SettingKind::DepletedRecoveryDelay
            | SettingKind::RecoveryTime
            | SettingKind::Native(N::StartingHealth) => Self::Health,
            SettingKind::OwnerDamageOnly
            | SettingKind::SourceFilter
            | SettingKind::InvertSourceFilter
            | SettingKind::IncomingDamage
            | SettingKind::ConditionalDamage => Self::DamageTaken,
            SettingKind::DamageBreakThreshold
            | SettingKind::SuppressionTime
            | SettingKind::MovementThreshold
            | SettingKind::IgnoreMovement
            | SettingKind::RetireOnRemoval
            | SettingKind::RetirementDelay
            | SettingKind::DisruptionGracePeriod
            | SettingKind::DamageBreakResponse
            | SettingKind::StrengthLoss => Self::Invisibility,
            SettingKind::QueryScale
            | SettingKind::SearchDelay
            | SettingKind::SearchRange
            | SettingKind::IncludeSelf
            | SettingKind::Native(N::TargetingRange) => Self::Targeting,
            SettingKind::TrackingStrength
            | SettingKind::DistanceTracking
            | SettingKind::BounceTracking
            | SettingKind::TrackingVariation
            | SettingKind::TrackingSpeed
            | SettingKind::FastThrowTracking
            | SettingKind::TurnRate
            | SettingKind::LeadTimeLimit
            | SettingKind::LeadDistanceLimit
            | SettingKind::SteeringAxisThreshold
            | SettingKind::TargetLead
            | SettingKind::ProximityRange
            | SettingKind::MinimumProximityTime
            | SettingKind::MaximumProximityTime
            | SettingKind::Native(N::TrackingSpeedChange) => Self::Tracking,
            SettingKind::AcquisitionScale
            | SettingKind::TargetLimit
            | SettingKind::SpawnChance
            | SettingKind::SpawnCount
            | SettingKind::GenerationLimit
            | SettingKind::SpawnLimit
            | SettingKind::Native(N::MinimumSpawnDelay | N::MaximumSpawnDelay) => Self::Spawning,
            SettingKind::EnergyCost
            | SettingKind::RequiredEnergy
            | SettingKind::ActivationEnergy
            | SettingKind::ActiveEnergyRate
            | SettingKind::EndingEnergyCost
            | SettingKind::RechargeDelay
            | SettingKind::MinimumActivationEnergy
            | SettingKind::EnergyFloor
            | SettingKind::Native(
                N::RechargeScale
                | N::ActivationCostScale
                | N::ActiveEnergyScale
                | N::ActivationLockout,
            ) => Self::Energy,
            SettingKind::TurnRateOffset
            | SettingKind::AcquisitionScaleOffset
            | SettingKind::SpawnCountOffset
            | SettingKind::GenerationLimitOffset
            | SettingKind::SpawnLimitOffset
            | SettingKind::Native(
                N::ContactCleanupBaseOffset
                | N::ContactTriggerBaseOffset
                | N::ContactCleanupOffset
                | N::ContactTriggerOffset
                | N::ContactCleanupScaling
                | N::ContactTriggerScaling
                | N::ContactCleanupScalingOffset
                | N::ContactTriggerScalingOffset
                | N::TriggeredDurationScaling
                | N::QueryScaleReduction,
            ) => Self::Adjustments,
            SettingKind::Native(N::EffectPriority | N::EffectGroup) => Self::Conflicts,
            SettingKind::TriggeredDuration
            | SettingKind::Native(
                N::MinimumActivationDelay
                | N::MaximumActivationDelay
                | N::MinimumCycleDuration
                | N::MaximumCycleDuration
                | N::RepeatCount
                | N::MinimumPartDuration
                | N::MaximumPartDuration,
            ) => Self::Timing,
        }
    }
}

/// A card's folds after its tiles: More for the values most authors leave alone, then
/// Unconfirmed for the values whose role no native consumer establishes.
fn card_folds(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    (properties, lanes): (&[Property], &[&RowLane]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    more_fold(ui, (salt, "more"), (properties, lanes), values);
    let unconfirmed = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| !confirmed(property))
        .collect::<Vec<_>>();
    if unconfirmed.is_empty() {
        return;
    }
    let edited = unconfirmed
        .iter()
        .any(|(_, property)| property.is_modified(values));
    let name = ((salt, "unconfirmed"), "Unconfirmed", UNCONFIRMED);
    fold(ui, name, edited, |ui, width| {
        for (index, property) in unconfirmed {
            property_tile(ui, width, index, property, values);
        }
    });
}

/// A movement value's bits as a number: a distance's float, or a count.
fn movement_value(movement: &MovementValue, bits: u32) -> f32 {
    movement.unit.value(bits)
}

/// An ability's loaded tree, with the keys its entry applies to its own row.
pub(super) struct Loaded {
    entity: u32,
    tree: Arc<Tree>,
    keys: Vec<u32>,
}

impl Loaded {
    /// The traced lanes of the bank rows the entry's own keys apply.
    fn lanes(&self) -> Vec<&RowLane> {
        self.tree
            .lanes
            .iter()
            .filter(|lane| self.keys.contains(&lane.key))
            .collect()
    }

    /// Whether a card stands for the ability's own graph.
    fn is_own(&self, card: &Card) -> bool {
        card.graphs.iter().any(|(tag, _)| *tag == self.entity)
    }

    /// The damage types its graphs' damage profiles deal, as the client encodes them.
    pub(super) fn damage_types(&self) -> &BTreeSet<u8> {
        &self.tree.damage
    }

    /// Whether the ability's own graph has values for its Ability card.
    pub(super) fn has_own_values(&self) -> bool {
        !self.lanes().is_empty()
            || self
                .tree
                .cards
                .iter()
                .any(|card| self.is_own(card) && !card.properties.is_empty())
    }
}

/// Whether a value a lane of the entry's keys sets stands for `property`, which then shows only
/// as the lane.
fn shadowed(lanes: &[&RowLane], property: &Property) -> bool {
    lanes.iter().any(|lane| {
        lane.label == property.label
            && matches!(
                lane.label,
                "Airborne Jumps"
                    | "Impulse Height Limit"
                    | "Active Energy Rate"
                    | "Velocity Blending"
            )
    })
}

/// The values of the ability's own graph.
fn own_properties(loaded: &Loaded) -> &[Property] {
    loaded
        .tree
        .cards
        .iter()
        .find(|card| loaded.is_own(card))
        .map_or(&[][..], |card| card.properties.as_slice())
}

/// The ability's own values as tiles of its Ability card: its graph's values, then the lanes of
/// the bank rows its entry's own keys apply. Those kept under More and those whose role is
/// unconfirmed follow the card's tiles in `own_folds`.
pub(super) fn own_tiles(ui: &mut egui::Ui, width: f32, loaded: &Loaded, edits: &mut EntryEdits) {
    let lanes = loaded.lanes();
    let properties = own_properties(loaded);
    for (index, property) in properties.iter().enumerate() {
        if shown(property, properties, &lanes, &edits.ability_values) {
            paired_tile(
                ui,
                width,
                index,
                (property, properties),
                &mut edits.ability_values,
                false,
            );
        }
    }
    for (index, lane) in lanes.iter().enumerate() {
        if lane.traced {
            lane_tile(ui, width, properties.len() + index, lane, edits);
        }
    }
}

/// The Ability card's closed sections after its tiles: More, then the values and bank lanes whose
/// role is unconfirmed.
pub(super) fn own_folds(ui: &mut egui::Ui, loaded: &Loaded, edits: &mut EntryEdits) {
    let lanes = loaded.lanes();
    let properties = own_properties(loaded);
    let salt = ("subclass-own", loaded.entity);
    more_fold(
        ui,
        (salt, "more"),
        (properties, &lanes),
        &mut edits.ability_values,
    );
    let hidden = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| !confirmed(property) && !shadowed(&lanes, property))
        .collect::<Vec<_>>();
    let hidden_lanes = lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| !lane.traced)
        .collect::<Vec<_>>();
    if hidden.is_empty() && hidden_lanes.is_empty() {
        return;
    }
    let edited = hidden
        .iter()
        .any(|(_, property)| property.is_modified(&edits.ability_values))
        || hidden_lanes
            .iter()
            .any(|(_, lane)| edits.bank_value(lane.key, lane.row, lane.lane).is_some());
    let name = ((salt, "unconfirmed"), "Unconfirmed", UNCONFIRMED);
    fold(ui, name, edited, |ui, width| {
        for (index, property) in hidden {
            property_tile(ui, width, index, property, &mut edits.ability_values);
        }
        for (index, lane) in hidden_lanes {
            lane_tile(ui, width, properties.len() + index, lane, edits);
        }
    });
}

impl PackageAuthoringApp {
    /// The ability's tree, starting its load on first use. `None` while it loads.
    pub(super) fn load_properties(
        &self,
        ctx: &egui::Context,
        (entity, keys): (u32, Vec<u32>),
        page: &mut PageState,
    ) -> Option<Result<Loaded, String>> {
        let tree = page.properties.poll(ctx, &self.packages, entity)?;
        Some(tree.map(|tree| Loaded { entity, tree, keys }))
    }

    /// The Gameplay section's parts: a card for each graph the ability spawns with values whose
    /// meaning is established. The ability's own values sit on its Ability card. Returns the
    /// entry's edits once one changes.
    pub(super) fn draw_properties(
        &self,
        ui: &mut egui::Ui,
        loaded: &Loaded,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let tree = &loaded.tree;
        let donors = if tree.cards.iter().any(|card| card.projectile) {
            page.properties
                .donors(ui.ctx(), &self.packages, || self.stock_abilities())
        } else {
            None
        };
        let workbench = &self.perk_workbench;
        let catalog_name = |graph: u32| {
            let name = workbench.asset_name(graph, "");
            (!name.is_empty()).then_some(name)
        };
        let sources = Sources {
            donors: donors.as_ref(),
            catalog: workbench.asset_catalog(),
            name: &catalog_name,
        };
        // A swapped projectile shows what it fires now, and the graphs below it, which nothing
        // spawns any more, show nothing.
        let replaced = |tag: u32| {
            tree.parents
                .get(&tag)
                .is_some_and(|parent| edits.swap(*parent, tag).is_some())
        };
        let gone = |tag: u32| {
            let mut at = tag;
            while let Some(&parent) = tree.parents.get(&at) {
                if replaced(parent) {
                    return true;
                }
                at = parent;
            }
            false
        };
        let own_graphs = tree_graphs(tree);
        let mut next = edits.clone();
        let show_alternatives = page.properties.show_alternatives;
        // Variants share the card of the first of them, a column each.
        let groups = variant_groups(&tree.cards, |card| loaded.is_own(card));
        let group_of = |index: usize| groups.iter().find(|group| group.contains(&index));
        for (index, card) in tree.cards.iter().enumerate() {
            if loaded.is_own(card) || card.graphs.iter().all(|(tag, _)| gone(*tag)) {
                continue;
            }
            let group = group_of(index);
            if group.is_some_and(|group| group[0] != index) {
                continue;
            }
            // An alternative shows on request, or once it holds an edit.
            let edited = group.map_or_else(
                || card_edited(card, edits),
                |group| {
                    group
                        .iter()
                        .any(|&each| card_edited(&tree.cards[each], edits))
                },
            );
            if card.alternative && !show_alternatives && !edited {
                continue;
            }
            if let Some(group) = group {
                let members = group
                    .iter()
                    .map(|&each| &tree.cards[each])
                    .collect::<Vec<_>>();
                ui.push_id(("subclass-variant-card", index), |ui| {
                    nested(ui, card.depth, |ui| {
                        style::card(ui, |ui| {
                            variant_card(
                                ui,
                                ("subclass-variants", index),
                                &members,
                                &mut next.ability_values,
                            );
                        });
                    });
                });
                continue;
            }
            let swapped = card.graphs.first().is_some_and(|(tag, _)| replaced(*tag));
            // The projectile swapped in shows its own tiles, which go to its private copy.
            let swapped_in = card
                .graphs
                .first()
                .and_then(|(tag, parent)| edits.swap((*parent)?, *tag))
                .map(|replacement| {
                    let tree = page.properties.poll(ui.ctx(), &self.packages, replacement);
                    (replacement, tree)
                });
            ui.push_id(("subclass-property-card", index), |ui| {
                nested(ui, card.depth, |ui| {
                    style::card(ui, |ui| {
                        card_header(ui, card);
                        if card.projectile {
                            swap_row(
                                ui,
                                (card, &own_graphs),
                                &sources,
                                &mut page.properties,
                                &mut next,
                            );
                        }
                        match &swapped_in {
                            Some((replacement, Some(Ok(tree)))) => {
                                if let Some(root) = root_card(tree, *replacement) {
                                    card_tiles(
                                        ui,
                                        ("swapped-in", index),
                                        root,
                                        &mut next.ability_values,
                                    );
                                }
                            }
                            Some((_, Some(Err(error)))) => {
                                ui.colored_label(
                                    ui.visuals().warn_fg_color,
                                    "Properties unavailable.",
                                )
                                .on_hover_text(error.as_str());
                            }
                            Some((_, None)) => {
                                ui.weak("Loading…");
                            }
                            None => {}
                        }
                        if !swapped && !card.properties.is_empty() {
                            let properties = &card.properties;
                            lead_tiles(ui, properties, &mut next.ability_values);
                            card_folds(
                                ui,
                                ("subclass-card", index),
                                (properties, &[]),
                                &mut next.ability_values,
                            );
                        }
                    })
                });
            });
            // What the projectile swapped in spawns follows its card, as the ability's own do.
            if let Some((replacement, Some(Ok(tree)))) = &swapped_in {
                draw_tree_cards(
                    ui,
                    ("subclass-swapped-in", index),
                    tree,
                    (card.depth, Some(*replacement)),
                    &mut next.ability_values,
                );
            }
        }
        let alternatives = tree.cards.iter().filter(|card| card.alternative).count();
        if alternatives > 0 {
            ui.checkbox(
                &mut page.properties.show_alternatives,
                format!("Show Alternatives ({alternatives})"),
            )
            .on_hover_text("Parts that spawn only when a perk or node selects them");
        }
        (next != *edits).then_some(next)
    }
}

/// How far a card nests under the card that spawns it, per level.
const NEST_INDENT: f32 = 18.0;
/// Levels past this one nest no further, so a deep tree keeps its cards wide.
const DEEPEST_NEST: usize = 3;

/// Draws `content` nested `depth` levels in.
fn nested<R>(ui: &mut egui::Ui, depth: usize, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if depth == 0 {
        return content(ui);
    }
    ui.horizontal_top(|ui| {
        ui.add_space(NEST_INDENT * depth.min(DEEPEST_NEST) as f32);
        ui.vertical(content).inner
    })
    .inner
}

/// Whether a card holds an edit: a changed value, or a projectile it fires instead.
fn card_edited(card: &Card, edits: &EntryEdits) -> bool {
    card.properties
        .iter()
        .any(|property| property.is_modified(&edits.ability_values))
        || card
            .graphs
            .iter()
            .any(|(tag, parent)| parent.is_some_and(|parent| edits.swap(parent, *tag).is_some()))
}

/// One lane of a bank row the entry's key applies: its name over its field, and the stock value
/// beside the name once it changes. The edit changes the ability's private copy of the bank.
fn lane_tile(ui: &mut egui::Ui, width: f32, index: usize, lane: &RowLane, edits: &mut EntryEdits) {
    let own = edits.bank_value(lane.key, lane.row, lane.lane);
    let unit = lane.unit;
    let stock = f64::from(unit.value(lane.stock));
    let stock_text = movement_text(unit, stock);
    let hint = if lane.traced {
        movement_hint(lane.label)
    } else {
        "Role unconfirmed"
    };
    let original = own.is_some().then_some(stock_text.as_str());
    let (edited, reset) =
        style::stock_tile(ui, (width, index), (lane.label, hint), original, |ui| {
            if unit == Unit::Mode {
                return movement_mode(
                    ui,
                    ("bank-mode", index),
                    width,
                    lane.label,
                    unit.value(own.unwrap_or(lane.stock)),
                )
                .map(|value| unit.bits(value));
            }
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            let mut value = f64::from(unit.value(own.unwrap_or(lane.stock)));
            let field = movement_field(&mut value, unit, lane.label);
            let response = style::named_control(ui.add_sized(size, field), lane.label);
            (response.changed() && value.is_finite()).then(|| unit.bits(value as f32))
        });
    if let Some(bits) = edited {
        edits.set_bank_value(
            lane.key,
            lane.row,
            lane.lane,
            (bits != lane.stock).then_some(bits),
        );
    } else if reset {
        edits.set_bank_value(lane.key, lane.row, lane.lane, None);
    }
}

/// A card's title, its graphs in the title's tooltip, and Alternative on a card that spawns only
/// when a perk or node selects it. Where it comes from shows by its nesting.
fn card_header(ui: &mut egui::Ui, card: &Card) {
    ui.horizontal(|ui| {
        let tags = card
            .graphs
            .iter()
            .map(|(tag, _)| format!("0x{tag:08X}"))
            .collect::<Vec<_>>()
            .join(", ");
        ui.label(card_title(&card.title)).on_hover_text(tags);
        if card.graphs.len() > 1 {
            ui.weak(format!("{} Matching Parts", card.graphs.len()))
                .on_hover_text(
                    "These controls change every matching part. Technical edits individual parts.",
                );
        }
        if card.alternative {
            ui.label(quiet(ui, "Alternative"))
                .on_hover_text("Spawns only when a perk or node selects it");
        }
    });
}

/// Every graph a tree's cards stand for.
fn tree_graphs(tree: &Tree) -> BTreeSet<u32> {
    tree.cards
        .iter()
        .flat_map(|card| card.graphs.iter().map(|(tag, _)| *tag))
        .collect()
}

/// The card that stands for `graph` in `tree`.
fn root_card(tree: &Tree, graph: u32) -> Option<&Card> {
    tree.cards
        .iter()
        .find(|card| card.graphs.iter().any(|(tag, _)| *tag == graph))
}

/// A card's tiles and closed sections, for a graph outside the ability's own tree.
fn card_tiles(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    card: &Card,
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let properties = &card.properties;
    lead_tiles(ui, properties, values);
    card_folds(ui, salt, (properties, &[]), values);
}

/// The cards of `tree`, a graph outside the ability's own tree, such as a projectile swapped in or
/// the one a weapon fires, nested `depth` levels past their own nesting, without `skip`'s card.
/// Alternatives and swaps stay with an ability's own cards.
fn draw_tree_cards(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    tree: &Tree,
    (depth, skip): (usize, Option<u32>),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let skipped =
        |card: &Card| skip.is_some_and(|graph| card.graphs.iter().any(|(tag, _)| *tag == graph));
    let groups = variant_groups(&tree.cards, |card| skipped(card) || card.alternative);
    for (index, card) in tree.cards.iter().enumerate() {
        if skipped(card) || card.alternative || card.properties.is_empty() {
            continue;
        }
        if let Some(group) = groups.iter().find(|group| group.contains(&index)) {
            if group[0] == index {
                let members = group
                    .iter()
                    .map(|&each| &tree.cards[each])
                    .collect::<Vec<_>>();
                ui.push_id((salt, index), |ui| {
                    nested(ui, depth + card.depth, |ui| {
                        style::card(ui, |ui| variant_card(ui, (salt, index), &members, values));
                    });
                });
            }
            continue;
        }
        ui.push_id((salt, index), |ui| {
            nested(ui, depth + card.depth, |ui| {
                style::card(ui, |ui| {
                    card_header(ui, card);
                    card_tiles(ui, (salt, index), card, values);
                });
            });
        });
    }
}

/// Property cards for a graph outside any ability, such as the projectile a weapon fires: its
/// tree, loaded once, with the tiles an ability's Gameplay cards show.
#[derive(Default)]
pub(in crate::app) struct GraphCards(Properties);

impl GraphCards {
    /// Whether `graph`'s tree holds a card with a tile, once it has loaded.
    pub(in crate::app) fn has_cards(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        graph: u32,
    ) -> Option<bool> {
        match self.0.poll(ctx, packages, graph)? {
            Ok(tree) => Some(
                tree.cards
                    .iter()
                    .any(|card| !card.alternative && !card.properties.is_empty()),
            ),
            Err(_) => Some(true),
        }
    }

    /// Draws `graph`'s cards, editing `values`.
    pub(in crate::app) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        packages: &Path,
        graph: u32,
        values: &mut Vec<WeaponRuntimeValueOverride>,
    ) {
        match self.0.poll(ui.ctx(), packages, graph) {
            Some(Ok(tree)) => {
                draw_tree_cards(ui, ("graph-cards", graph), &tree, (0, None), values);
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().warn_fg_color, "Properties unavailable.")
                    .on_hover_text(error);
            }
            None => {
                ui.weak("Loading…");
            }
        }
    }
}

/// One property's tile: its name over its field, and the stock value beside the name once it
/// changes.
fn property_tile(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    property: &Property,
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    property_tile_state(ui, width, index, property, values, false);
}

fn paired_tile(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    (property, siblings): (&Property, &[Property]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
    inactive: bool,
) {
    let before = property.current(values);
    property_tile_state(ui, width, index, property, values, inactive);
    keep_ordered(property, before, siblings, values);
}

fn property_tile_state(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    property: &Property,
    values: &mut Vec<WeaponRuntimeValueOverride>,
    inactive: bool,
) {
    property_tile_named(
        ui,
        width,
        index,
        (property, &property.label),
        values,
        inactive,
    );
}

/// A property's tile under `shown`, its name on the tile, such as a series' number. Its control
/// keeps the full name, which a screen reader hears.
fn property_tile_named(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    (property, shown): (&Property, &str),
    values: &mut Vec<WeaponRuntimeValueOverride>,
    inactive: bool,
) {
    property.keep_unlimited(values);
    let stock = property.reading(f64::from(property.stock));
    let current = property.current(values);
    let original = property.is_modified(values).then_some(stock.as_str());
    let name = (shown, property.hint.as_str());
    let status = inactive.then_some((
        "Inactive",
        "Another setting disables this value. You can still edit or reset it.",
    ));
    let (edited, reset) =
        style::stock_tile_marked(ui, (width, index), name, original, status, |ui| {
            property_state_hint(ui, property, values);
            property_control(ui, (index, width), property, current)
        });
    if let Some(value) = edited {
        property.set(values, value);
    } else if reset {
        property.reset(values);
    }
}

/// A property's field at `width`: its checkbox, choice or number, reading `current`. Returns the
/// value an edit set.
fn property_control(
    ui: &mut egui::Ui,
    (index, width): (usize, f32),
    property: &Property,
    current: f32,
) -> Option<f32> {
    if let Value::Movement(movement) = &property.value
        && movement[0].unit == Unit::Mode
    {
        return movement_mode(
            ui,
            ("movement-mode", index),
            width,
            &property.label,
            current,
        );
    }
    // A flag is a checkbox, on as one and off as zero.
    if let Value::Setting(settings) = &property.value
        && settings[0].kind.unit() == SettingUnit::Flag
    {
        let mut on = current >= 0.5;
        let label = if on { "On" } else { "Off" };
        let response = style::named_control(ui.checkbox(&mut on, label), &property.label);
        return response.changed().then_some(if on { 1.0 } else { 0.0 });
    }
    if let Value::Setting(settings) = &property.value {
        let names = match settings[0].kind {
            SettingKind::ExpirationResponse => Some(&EXPIRATION_RESPONSES),
            SettingKind::CollisionMode => Some(&COLLISION_MODES),
            _ => None,
        };
        if let Some(names) = names {
            return setting_choice(
                ui,
                (index, width),
                (settings[0].kind, &property.label),
                current,
                names,
            );
        }
    }
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    let mut value = f64::from(current);
    // Below zero is Unlimited, for a timer that can be.
    let unlimited = property.can_be_unlimited();
    let floor = if unlimited { -1.0 } else { 0.0 };
    let field = match &property.value {
        Value::Duration(_) => egui::DragValue::new(&mut value)
            .speed(0.1)
            .range(floor..=3600.0)
            .clamp_existing_to_range(false)
            .custom_formatter(move |value, _| duration_text(value))
            .custom_parser(parse_duration),
        Value::Timer { scaling: false, .. } => egui::DragValue::new(&mut value)
            .speed(0.1)
            .range(-60.0..=60.0)
            .clamp_existing_to_range(false)
            .custom_formatter(|value, _| format!("{} s", number(value)))
            .custom_parser(parse_number),
        Value::Timer { scaling: true, .. } => egui::DragValue::new(&mut value)
            .speed(0.05)
            .range(-10.0..=10.0)
            .clamp_existing_to_range(false)
            .custom_formatter(|value, _| number(value))
            .custom_parser(parse_number),
        Value::Flight(parameters) if parameters[0].kind == Kind::TravelDistance => {
            egui::DragValue::new(&mut value)
                .speed(1.0)
                .custom_formatter(|value, _| travel_text(value))
                .custom_parser(parse_travel)
        }
        Value::Flight(parameters) if parameters[0].kind == Kind::Speed => {
            egui::DragValue::new(&mut value)
                .speed(0.01)
                .custom_formatter(|value, _| speed_text(value))
                .custom_parser(parse_speed)
        }
        Value::Flight(_) | Value::Amount { multiply: true, .. } => egui::DragValue::new(&mut value)
            .speed(0.01)
            .custom_formatter(|value, _| format!("×{}", number(value)))
            .custom_parser(parse_amount),
        Value::Amount {
            multiply: false, ..
        } => egui::DragValue::new(&mut value)
            .speed(0.05)
            .custom_formatter(|value, _| signed(value))
            .custom_parser(parse_amount),
        Value::Field(_) => egui::DragValue::new(&mut value).speed(0.01).max_decimals(3),
        Value::Movement(movement) => {
            movement_field(&mut value, movement[0].unit, movement[0].label)
        }
        Value::Curve(curves) => {
            let (top, format): (f64, fn(f64) -> String) = match curves[0].kind {
                CurveKind::FinalSpeed => (200.0, number),
                CurveKind::FinalGravity => (10.0, |value| format!("×{}", number(value))),
                CurveKind::Start | CurveKind::End => {
                    (200.0, |value| format!("{} Units", number(value)))
                }
            };
            egui::DragValue::new(&mut value)
                .speed(0.05)
                .range(0.0..=top)
                .clamp_existing_to_range(false)
                .custom_formatter(move |value, _| format(value))
                .custom_parser(parse_number)
        }
        Value::Setting(settings) => {
            let kind = settings[0].kind;
            let (low, high) = kind.range();
            // A count steps by whole numbers at a usable drag speed. Other values cross their
            // range in a few hundred points, so a wide one such as Turn Rate stays draggable.
            let speed = if kind.unit() == SettingUnit::Count {
                0.05
            } else {
                (f64::from(high - low) / 400.0).max(0.01)
            };
            egui::DragValue::new(&mut value)
                .speed(speed)
                .range(f64::from(low)..=f64::from(high))
                .clamp_existing_to_range(false)
                .custom_formatter(move |value, _| setting_text(kind, value))
                .custom_parser(move |text| parse_setting(kind, text))
        }
    };
    let response = style::named_control(ui.add_sized(size, field), &property.label);
    let value = match property.value {
        Value::Duration(_) if value < 0.0 => UNLIMITED,
        Value::Duration(_) => value.max(0.0) as f32,
        _ => value as f32,
    };
    (response.changed() && value.is_finite()).then_some(value)
}

fn setting_choice(
    ui: &mut egui::Ui,
    (index, width): (usize, f32),
    (kind, label): (SettingKind, &str),
    current: f32,
    names: &[&str],
) -> Option<f32> {
    let mut value = current;
    let response = egui::ComboBox::from_id_salt(("setting-choice", index))
        .width(width)
        .selected_text(setting_text(kind, f64::from(value)))
        .show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                ui.selectable_value(&mut value, index as f32, *name);
            }
        });
    style::named_control(response.response, label);
    (value != current).then_some(value)
}

fn property_state_hint(
    ui: &mut egui::Ui,
    property: &Property,
    values: &[WeaponRuntimeValueOverride],
) {
    if property.mixed(values) {
        ui.weak("Mixed Values").on_hover_text(
            "Changing this control updates every matching part. Use Technical to edit individual parts.",
        );
    }
}
