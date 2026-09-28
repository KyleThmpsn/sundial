//! Values kept per ammo type read as a table: a row for each of Primary, Special and Heavy and
//! a column for each value. As nine loose tiles, three rows of the same three numbers never
//! read as one choice between ammo types.
use super::*;

/// Drop Ammo by Weighted Chance. Its triples are a weight and the range the drop samples.
pub(super) const DROP: u32 = 0x8080_3E47;
/// Change Ammo Drop Chance. Its triples are added to the host while the effect lasts.
const FINDER: u32 = 0x8080_3E35;

const WEIGHT_HINT: &str = "How often this ammo type drops, against the others' weights.";
const CHANCE_HINT: &str = "The share of drops this weight gives.";
const RANGE_HINT: &str = "The drop samples a value between Minimum and Maximum.";
const VALUE_HINT: &str =
    "Added while the effect lasts. Which of the three values does what is not established.";
const CELL: f32 = 72.0;

struct Table {
    class: u32,
    /// Where each ammo type's triple starts, for Primary, Special and Heavy.
    rows: [usize; 3],
    /// Each column's name, offset within its triple and hint.
    columns: [(&'static str, usize, &'static str); 3],
    /// The weight column, followed by the share of drops it gives.
    weight: Option<usize>,
}

const TABLES: [Table; 2] = [
    Table {
        class: DROP,
        rows: [0x18, 0x24, 0x30],
        columns: [
            ("Weight", 8, WEIGHT_HINT),
            ("Minimum", 0, RANGE_HINT),
            ("Maximum", 4, RANGE_HINT),
        ],
        weight: Some(8),
    },
    // The Finder mods each write only their own ammo type's triple, which is what makes
    // the rows ammo types.
    Table {
        class: FINDER,
        rows: [0x0C, 0x18, 0x24],
        columns: [
            ("Value 1", 0, VALUE_HINT),
            ("Value 2", 4, VALUE_HINT),
            ("Value 3", 8, VALUE_HINT),
        ],
        weight: None,
    },
];
const TYPES: [&str; 3] = ["Primary", "Special", "Heavy"];

fn table(class: u32) -> Option<&'static Table> {
    TABLES.iter().find(|table| table.class == class)
}

/// Whether the node leads with an ammo table.
pub(super) fn leads(class: u32) -> bool {
    table(class).is_some()
}

/// Whether a table draws this field, so the node's tiles leave it out.
pub(super) fn owns(class: u32, offset: usize) -> bool {
    table(class).is_some_and(|table| {
        table
            .rows
            .iter()
            .any(|row| (*row..row + 12).contains(&offset))
    })
}

fn number(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The table for a node that keeps values per ammo type. Any other node draws nothing here.
pub(super) fn draw(ui: &mut egui::Ui, graph: &mut Graph, index: usize) {
    let block = &mut graph.blocks[index];
    let Some(table) = table(block.class) else {
        return;
    };
    if block.bytes.len() < table.rows[2] + 12 {
        return;
    }
    let total = table.weight.map_or(0.0, |weight| {
        table
            .rows
            .iter()
            .map(|row| number(&block.bytes, row + weight).max(0.0))
            .sum::<f32>()
    });
    let secondary = crate::app::style::secondary(ui.visuals());
    let height = ui.spacing().interact_size.y;
    let header = |ui: &mut egui::Ui, name: &str, hint: &str| {
        ui.label(egui::RichText::new(name).small().color(secondary))
            .on_hover_text(format!("{name}\n{hint}"));
    };
    let columns = 4 + usize::from(table.weight.is_some());
    // The cells share what the card leaves after the ammo type column, so a narrow card
    // squeezes them rather than growing past its edge.
    let cell = ((ui.available_width() - 64.0 - 12.0 * columns as f32) / (columns - 1) as f32)
        .clamp(40.0, CELL);
    egui::Grid::new(("ammo-table", table.class))
        .num_columns(columns)
        .min_col_width(0.0)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("");
            for (name, offset, hint) in table.columns {
                header(ui, name, hint);
                if table.weight == Some(offset) {
                    header(ui, "Chance", CHANCE_HINT);
                }
            }
            ui.end_row();
            for (name, row) in TYPES.into_iter().zip(table.rows) {
                ui.label(egui::RichText::new(name).color(secondary));
                for (column, offset, _) in table.columns {
                    let at = row + offset;
                    let mut value = number(&block.bytes, at);
                    let mut drag = egui::DragValue::new(&mut value).speed(0.01).max_decimals(3);
                    if table.weight.is_some() {
                        drag = drag.range(0.0..=f32::MAX).clamp_existing_to_range(false);
                    }
                    let response = ui.add_sized([cell, height], drag);
                    pickers::name_response(ui, &response, &format!("{name} Ammo {column}"));
                    if response.changed() {
                        block.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    if table.weight == Some(offset) {
                        let weight = number(&block.bytes, at).max(0.0);
                        let chance = if total > 0.0 {
                            weight / total * 100.0
                        } else {
                            0.0
                        };
                        ui.add_sized(
                            [cell, height],
                            egui::Label::new(
                                egui::RichText::new(format!("{chance:.0}%")).color(secondary),
                            ),
                        );
                    }
                }
                ui.end_row();
            }
        });
}
