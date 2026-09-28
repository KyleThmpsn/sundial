//! Values grouped by the native record that holds them. A component can hold several records
//! of one type, and a component's instance can keep its own copy of settings its
//! configuration also holds. Run together as tiles, their values repeated the same names
//! across lines and read as duplicates, so records that share names read as a table with one
//! record to a row. A record type with a reading order of its own is a table even alone, so a
//! property adjustment always reads as what it changes, then how.
use super::*;
use sundial::package_authoring::weapon_runtime::modifiers;

/// The narrowest a table column gets before each record reads as its own tiles instead.
const MIN_COLUMN: f32 = 80.0;
/// The widest a table column gets, so a wide pane does not spread a row's values apart.
const MAX_COLUMN: f32 = 180.0;
/// Room for a long row label, such as a component name.
const MAX_LABEL: f32 = 180.0;

type Choices = &'static [(i64, &'static str)];

/// How one value draws.
#[derive(Clone, Copy)]
pub(super) enum Layout {
    /// The complete native field, labelled beside its control.
    Row,
    /// A tile, its name over the control, with names for its values when they depend on
    /// another value of its record.
    Tile(f32, Option<Choices>),
    /// A cell under its column's name. `reset` restores the original value.
    Cell {
        width: f32,
        reset: bool,
        choices: Option<Choices>,
    },
    /// Nothing is drawn. The value is only read.
    Peek,
}

/// What a value reports once drawn or read: whether it differs from the original, and its
/// current number when it is one.
#[derive(Clone, Copy, Default)]
pub(super) struct Drawn {
    pub modified: bool,
    pub number: Option<i64>,
}

impl Drawn {
    pub(super) fn of(current: Option<WeaponRuntimeValue>, original: &WeaponRuntimeValue) -> Self {
        let number = match &current {
            Some(WeaponRuntimeValue::Signed(value)) => Some(*value),
            Some(WeaponRuntimeValue::Unsigned(value)) => i64::try_from(*value).ok(),
            _ => None,
        };
        Self {
            modified: current.as_ref().is_some_and(|current| current != original),
            number,
        }
    }
}

/// Where a group's values live, which labels the rows of records from different groups.
pub(super) struct Source {
    pub component: String,
    pub root: &'static str,
}

/// One value and the component group it belongs to.
#[derive(Clone, Copy)]
pub(super) struct Entry<'f> {
    pub group: usize,
    pub field: &'f WeaponRuntimeField,
}

/// Values whose names appear in one record, by group, and a table for each set of records
/// that share names.
pub(super) struct Arranged<'f> {
    singles: Vec<(usize, Vec<Entry<'f>>)>,
    tables: Vec<Table<'f>>,
    /// A heading for each group of single values, when they come from more than one.
    captions: BTreeMap<usize, String>,
}

struct Table<'f> {
    /// The first record's type, which keeps the table's controls apart from another's.
    schema: u32,
    columns: Vec<&'f str>,
    rows: Vec<Row<'f>>,
}

struct Row<'f> {
    label: String,
    cells: Vec<Option<Entry<'f>>>,
}

/// Whether a record of `schema` with `fields` joins the table `other` already belongs to.
fn joins_record(other: &[Entry<'_>], schema: u32, fields: &[Entry<'_>]) -> bool {
    let theirs = other[0].field.locator.type_handle;
    theirs == schema
        || modifiers::column_order(theirs).is_none()
            && modifiers::column_order(schema).is_none()
            && shares_names(other, fields)
}

/// A table's column names. A type with a reading order of its own is read that way. Others
/// keep the order the names first appear in.
fn columns<'f>(
    set: &[(usize, Vec<Entry<'f>>)],
    schema: u32,
    order: Option<&[u32]>,
) -> Vec<&'f str> {
    let mut columns: Vec<(&'f str, u32)> = Vec::new();
    for entry in set.iter().flat_map(|(_, fields)| fields) {
        if !columns.iter().any(|(name, _)| *name == entry.field.name) {
            columns.push((&entry.field.name, entry.field.locator.value_offset));
        }
    }
    if let Some(order) = order
        && set
            .iter()
            .flat_map(|(_, fields)| fields)
            .all(|entry| entry.field.locator.type_handle == schema)
    {
        columns.sort_by_key(|(_, offset)| {
            order
                .iter()
                .position(|candidate| candidate == offset)
                .unwrap_or(order.len())
        });
    }
    columns.into_iter().map(|(name, _)| name).collect()
}

/// Sorts values into records, and records that share names into tables. `source` says
/// where a group's values live.
pub(super) fn arrange<'f>(entries: &[Entry<'f>], source: impl Fn(usize) -> Source) -> Arranged<'f> {
    // A record is one type at one start in one group. A reflected array measures every
    // element from its root, so its elements share a start and a repeated name begins the
    // next one.
    let mut records: Vec<((usize, u32, u32), Vec<Entry<'f>>)> = Vec::new();
    for entry in entries {
        let locator = &entry.field.locator;
        let key = (
            entry.group,
            locator.type_handle,
            entry.field.owner_offset.wrapping_sub(locator.value_offset),
        );
        match records.iter_mut().rev().find(|(found, _)| *found == key) {
            Some((_, fields))
                if !fields
                    .iter()
                    .any(|other| other.field.name == entry.field.name) =>
            {
                fields.push(*entry);
            }
            _ => records.push((key, vec![*entry])),
        }
    }
    // Records join a table when they are one type, or when they share most of their names,
    // as an instance's copy of a configuration does. One shared name alone does not join them.
    // A type with its own column order is a known record, so only its own type joins it: a
    // property adjustment shares Amount and Operation with other records without being one.
    let mut sets: Vec<Vec<(usize, Vec<Entry<'f>>)>> = Vec::new();
    for ((group, schema, _), fields) in records {
        let joins = |set: &Vec<(usize, Vec<Entry<'f>>)>| {
            set.iter()
                .any(|(_, other)| joins_record(other, schema, &fields))
        };
        match sets.iter_mut().find(|set| joins(set)) {
            Some(set) => set.push((group, fields)),
            None => sets.push(vec![(group, fields)]),
        }
    }
    let mut arranged = Arranged {
        singles: Vec::new(),
        tables: Vec::new(),
        captions: BTreeMap::new(),
    };
    for set in sets {
        let schema = set[0].1[0].field.locator.type_handle;
        let order = modifiers::column_order(schema);
        if set.len() < 2 && order.is_none() {
            for (group, fields) in set {
                match arranged
                    .singles
                    .iter_mut()
                    .find(|(found, _)| *found == group)
                {
                    Some((_, entries)) => entries.extend(fields),
                    None => arranged.singles.push((group, fields)),
                }
            }
            continue;
        }
        let columns = columns(&set, schema, order);
        let labels = row_labels(&set, &source);
        let rows = set
            .iter()
            .zip(labels)
            .map(|((_, fields), label)| Row {
                label,
                cells: columns
                    .iter()
                    .map(|name| {
                        fields
                            .iter()
                            .find(|entry| entry.field.name == *name)
                            .copied()
                    })
                    .collect(),
            })
            .collect();
        arranged.tables.push(Table {
            schema,
            columns,
            rows,
        });
    }
    if arranged.singles.len() > 1 {
        let sources = arranged
            .singles
            .iter()
            .map(|(group, _)| (*group, source(*group)))
            .collect::<Vec<_>>();
        for (group, found) in &sources {
            let repeated = sources
                .iter()
                .filter(|(_, other)| other.component == found.component)
                .count()
                > 1;
            let caption = if repeated {
                format!("{} · {}", found.component, found.root)
            } else {
                found.component.clone()
            };
            arranged.captions.insert(*group, caption);
        }
    }
    arranged
}

/// Whether two records share at least two names and half of the smaller one's.
fn shares_names(a: &[Entry<'_>], b: &[Entry<'_>]) -> bool {
    let shared = a
        .iter()
        .filter(|entry| b.iter().any(|other| other.field.name == entry.field.name))
        .count();
    shared >= 2 && shared * 2 >= a.len().min(b.len())
}

/// A lone record needs no label. Records of one group are numbered. Records from different
/// groups carry where they live: the root alone when that tells them apart or they share a
/// component, numbered only where one repeats.
fn row_labels(set: &[(usize, Vec<Entry<'_>>)], source: &impl Fn(usize) -> Source) -> Vec<String> {
    if set.len() == 1 {
        return vec![String::new()];
    }
    if set.iter().all(|(group, _)| *group == set[0].0) {
        return (1..=set.len()).map(|number| number.to_string()).collect();
    }
    let sources = set
        .iter()
        .map(|(group, _)| source(*group))
        .collect::<Vec<_>>();
    let distinct = sources.iter().enumerate().all(|(index, found)| {
        sources[..index]
            .iter()
            .all(|other| other.root != found.root)
    });
    let shared = distinct
        || sources
            .iter()
            .all(|found| found.component == sources[0].component);
    let names = sources
        .iter()
        .map(|found| {
            if shared {
                found.root.to_owned()
            } else {
                format!("{} · {}", found.component, found.root)
            }
        })
        .collect::<Vec<_>>();
    if names.iter().all(|name| *name == names[0]) {
        return (1..=set.len()).map(|number| number.to_string()).collect();
    }
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if names.iter().filter(|other| *other == name).count() < 2 {
                return name.clone();
            }
            let number = names[..=index]
                .iter()
                .filter(|other| *other == name)
                .count();
            format!("{name} {number}")
        })
        .collect()
}

/// Names for a value that depend on other values of its record, such as a property by the
/// component it changes.
fn row_choices(entry: Entry<'_>, row: &[(u32, i64)]) -> Option<Choices> {
    modifiers::row_choices(
        entry.field.locator.type_handle,
        entry.field.locator.value_offset,
        row,
    )
}

impl<'f> Arranged<'f> {
    /// Draws every value. `draw` draws or reads one in a layout and reports it.
    pub(super) fn draw(
        &self,
        ui: &mut egui::Ui,
        mut draw: impl FnMut(&mut egui::Ui, Entry<'f>, Layout) -> Drawn,
    ) {
        let secondary = crate::app::style::secondary(ui.visuals());
        for (group, entries) in &self.singles {
            if let Some(caption) = self.captions.get(group) {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(caption).color(secondary));
            }
            crate::app::style::tiles(ui, |ui, width| {
                for entry in entries {
                    draw(ui, *entry, Layout::Tile(width, None));
                }
            });
        }
        for table in &self.tables {
            table.draw(ui, &mut draw);
        }
    }
}

impl<'f> Table<'f> {
    fn draw(
        &self,
        ui: &mut egui::Ui,
        draw: &mut impl FnMut(&mut egui::Ui, Entry<'f>, Layout) -> Drawn,
    ) {
        // Every value is read first, so the table knows which columns any row uses and what
        // each row's names depend on.
        let numbers = self
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.and_then(|entry| draw(ui, entry, Layout::Peek).number))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let used = |column: usize| {
            self.rows.iter().zip(&numbers).any(|(row, numbers)| {
                row.cells[column].is_some_and(|entry| {
                    !numbers[column].is_some_and(|value| {
                        modifiers::unread(
                            entry.field.locator.type_handle,
                            entry.field.locator.value_offset,
                            value,
                        )
                    })
                })
            })
        };
        let visible = (0..self.columns.len())
            .filter(|column| used(*column))
            .collect::<Vec<_>>();
        let row_numbers = |row: usize| {
            self.rows[row]
                .cells
                .iter()
                .zip(&numbers[row])
                .filter_map(|(cell, number)| {
                    Some((cell.as_ref()?.field.locator.value_offset, (*number)?))
                })
                .collect::<Vec<_>>()
        };

        let secondary = crate::app::style::secondary(ui.visuals());
        let body = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let measure = |text: &str, font: &egui::FontId| {
            ui.fonts(|fonts| {
                fonts
                    .layout_no_wrap(text.to_owned(), font.clone(), secondary)
                    .size()
                    .x
            })
        };
        let labelled = self.rows.iter().any(|row| !row.label.is_empty());
        let label_width = self
            .rows
            .iter()
            .map(|row| measure(&row.label, &body))
            .fold(0.0, f32::max)
            .min(MAX_LABEL);
        let reset_width = measure("Reset", &small) + 8.0;
        let gap = 12.0;
        let count = visible.len().max(1) as f32;
        let fixed = if labelled { label_width + gap } else { 0.0 };
        let room = ui.available_width() - fixed - reset_width - gap * count;
        let width = (room / count).floor().min(MAX_COLUMN);
        if width < MIN_COLUMN {
            // Too narrow for columns: each record reads as its own tiles under its label.
            for (index, row) in self.rows.iter().enumerate() {
                if labelled {
                    ui.label(egui::RichText::new(&row.label).color(secondary));
                }
                let numbers = row_numbers(index);
                crate::app::style::tiles(ui, |ui, width| {
                    for column in &visible {
                        if let Some(entry) = row.cells[*column] {
                            draw(ui, entry, Layout::Tile(width, row_choices(entry, &numbers)));
                        }
                    }
                });
            }
            return;
        }
        let height = ui.spacing().interact_size.y;
        let cell = |ui: &mut egui::Ui, width: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    add(ui);
                },
            );
        };
        // The grid's own column minimum would widen the label and Reset columns past the
        // widths measured above and push the table off the pane.
        egui::Grid::new(("record-table", self.schema))
            .num_columns(visible.len() + 1 + usize::from(labelled))
            .min_col_width(0.0)
            .spacing([gap, 6.0])
            .show(ui, |ui| {
                if labelled {
                    cell(ui, label_width, &mut |_| {});
                }
                for column in &visible {
                    let name = self.columns[*column];
                    cell(ui, width, &mut |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(name).small().color(secondary))
                                .truncate(),
                        );
                    });
                }
                ui.end_row();
                for (index, row) in self.rows.iter().enumerate() {
                    // Reset takes effect on the next frame, since the row's values are drawn
                    // before its button.
                    let id = ui.make_persistent_id(("record-reset", self.schema, index));
                    let reset = ui
                        .data_mut(|data| data.remove_temp::<bool>(id))
                        .unwrap_or(false);
                    if labelled {
                        cell(ui, label_width, &mut |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&row.label).color(secondary))
                                    .truncate(),
                            );
                        });
                    }
                    let numbers = row_numbers(index);
                    let mut modified = false;
                    for column in &visible {
                        match row.cells[*column] {
                            Some(entry) => {
                                let choices = row_choices(entry, &numbers);
                                modified |= draw(
                                    ui,
                                    entry,
                                    Layout::Cell {
                                        width,
                                        reset,
                                        choices,
                                    },
                                )
                                .modified;
                            }
                            None => cell(ui, width, &mut |_| {}),
                        }
                    }
                    cell(ui, reset_width, &mut |ui| {
                        if modified
                            && ui
                                .scope(|ui| {
                                    crate::app::style::quiet(ui);
                                    ui.add(egui::Button::new(egui::RichText::new("Reset").small()))
                                        .on_hover_text("Restores this row's original values.")
                                        .clicked()
                                })
                                .inner
                        {
                            ui.data_mut(|data| data.insert_temp(id, true));
                            ui.ctx().request_repaint();
                        }
                    });
                    ui.end_row();
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value named `name` at `offset` in a record of type `schema` that starts at `start`.
    fn value(name: &str, schema: u32, start: u32, offset: u32) -> WeaponRuntimeField {
        let loaded = crate::app::custom_perks::workbench::parameters::tests::fixture();
        let mut field = loaded.graphs[0].1.owners[0].roots[0].fields[0].clone();
        field.name = name.into();
        field.locator.type_handle = schema;
        field.locator.value_offset = offset;
        field.owner_offset = start + offset;
        field
    }

    /// Records that repeat a type are numbered rows. An instance's copy of its configuration
    /// shares a table with it, each row named by its root. A property adjustment is a table
    /// even alone, read as what it changes and then how. A record that shares no names stays
    /// a tile under its group's heading.
    #[test]
    fn records_read_as_tables_in_their_own_order_and_the_rest_as_tiles() {
        let instance = [
            value("Amount", 1, 0x100, 0x28),
            value("Operation", 1, 0x100, 0x2C),
            value("Amount", 1, 0x158, 0x28),
            value("Operation", 1, 0x158, 0x2C),
            value("Speed Curve Multiplier", 2, 0x200, 0x50),
            value("Speed Curve Endpoint", 2, 0x200, 0x5C),
            value("Curve End Distance", 2, 0x200, 0x68),
            value("Invert Source Property", 3, 0x300, 0xE4),
            value("Amount", 0x8080_3B06, 0x400, 0x28),
            value("Operation", 0x8080_3B06, 0x400, 0x2C),
            value("Property", 0x8080_3B06, 0x400, 0x4A),
            value("Component", 0x8080_3B06, 0x400, 0x4C),
        ];
        let configuration = [
            value("Speed Curve Endpoint", 4, 0x40, 0),
            value("Curve End Distance", 4, 0x40, 12),
            value("Default Health Capacity", 5, 0x80, 0xB4),
        ];
        let entries = instance
            .iter()
            .map(|field| Entry { group: 0, field })
            .chain(configuration.iter().map(|field| Entry { group: 1, field }))
            .collect::<Vec<_>>();
        let arranged = arrange(&entries, |group| Source {
            component: if group == 0 {
                "Projectile Movement".into()
            } else {
                "Health and Shields".into()
            },
            root: if group == 0 {
                "Initial Values"
            } else {
                "Configuration"
            },
        });
        let tables = arranged
            .tables
            .iter()
            .map(|table| {
                let rows = table
                    .rows
                    .iter()
                    .map(|row| (row.label.as_str(), row.cells.iter().flatten().count()))
                    .collect::<Vec<_>>();
                (table.columns.clone(), rows)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            tables,
            [
                (vec!["Amount", "Operation"], vec![("1", 2), ("2", 2)]),
                (
                    vec![
                        "Speed Curve Multiplier",
                        "Speed Curve Endpoint",
                        "Curve End Distance"
                    ],
                    vec![("Initial Values", 3), ("Configuration", 2)]
                ),
                (
                    vec!["Component", "Property", "Operation", "Amount"],
                    vec![("", 4)]
                ),
            ]
        );
        let singles = arranged
            .singles
            .iter()
            .map(|(group, entries)| {
                let names = entries
                    .iter()
                    .map(|entry| entry.field.name.as_str())
                    .collect::<Vec<_>>();
                (arranged.captions.get(group).cloned(), names)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            singles,
            [
                (
                    Some("Projectile Movement".to_owned()),
                    vec!["Invert Source Property"]
                ),
                (
                    Some("Health and Shields".to_owned()),
                    vec!["Default Health Capacity"]
                ),
            ]
        );
    }
}
