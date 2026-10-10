//! The damage an Event Numeric Modifier writes, which is what the action is for: the rows that
//! set or multiply a damage field of the event, and the overall damage bonus. They are nested
//! records, so without this the card led with its filters and never said how much it changed
//! damage.
use super::*;

pub(super) const CLASS: u32 = 0x8080_2F16;
/// A 12-byte row: the damage field at +0, a literal at +4 and a stat selector at +8.
const ROW_CLASS: u32 = 0x8080_3E22;
const ROW_SIZE: usize = 12;
/// The overall bonus as a literal pair, or as a program the runtime evaluates instead.
const PAIR_CLASS: u32 = 0x8080_2F1A;
const PROGRAM_CLASS: u32 = 0x8080_2F18;
/// The program's Value Source, one of the shared value inputs.
const INPUT_SOURCE: usize = 0x38;
/// Value Source 0, the number the action keeps, which stock perks use as their stacks.
const STACKS: u8 = 0;
/// The most stacks the card samples, past every stock perk's cap.
const MOST_STACKS: usize = 16;
/// A curve over this many stacks or fewer reads stack by stack.
const LISTED_STACKS: usize = 6;
/// Pointers to the assignment rows, the multiplication rows and the overall bonus.
const ASSIGN: usize = 0x128;
const MULTIPLY: usize = 0x138;
const BONUS: usize = 0x140;
/// The stat selector that uses the row's literal instead of a native stat.
const LITERAL: u8 = 0xFF;

const BONUS_HINT: &str =
    "Scales damage that passes the filters. 0% leaves it unchanged and -100% removes it.";
const STACK_HINT: &str = "Scales damage that passes the filters by the perk's stacks. Show Properties holds the formula.";
const FORMULA_HINT: &str = "A formula sets this from its Value Source. Show Properties holds it.";
const ASSIGN_HINT: &str = "Replaces this value on damage that passes the filters.";
const MULTIPLY_HINT: &str = "Multiplies this value on damage that passes the filters.";
const STAT_HINT: &str = "Reads a native stat. The menu can set a number instead.";

/// A damage field by the row's first byte, named as the row's contract names it. A lane no
/// stock perk has established reads by its number.
fn field_name(field: u8) -> String {
    fields::describe(ROW_CLASS)
        .ok()
        .and_then(|described| {
            described
                .into_iter()
                .find(|described| described.offset == 0)
        })
        .and_then(|described| {
            fields::contract(ROW_CLASS, &described)
                .choices
                .iter()
                .find(|(value, _)| *value == field)
                .map(|(_, name)| (*name).to_owned())
        })
        .unwrap_or_else(|| format!("Damage Value {field}"))
}

fn row_label(field: u8, multiply: bool) -> String {
    let name = field_name(field);
    if multiply {
        format!("{name} Multiplier")
    } else {
        name
    }
}

/// The overall bonus, as a number or as the formula's reading.
enum Bonus {
    Literal(f32),
    Formula(Reading, u8),
    Unread,
}

/// A formula's bonus over the stacks it reads, sampled from the formula itself.
#[derive(Debug, PartialEq)]
enum Reading {
    /// The same whatever its input.
    Fixed(f32),
    /// The bonus at each stack from the first until it stops changing.
    Steps(Vec<f32>),
    /// The same bonus for every stack, up to a last stack or without one.
    PerStack(f32, Option<usize>),
    /// Anything the card cannot put in a few words.
    Varies,
}

fn bonus(graph: &Graph, block: usize) -> Bonus {
    let record = &graph.blocks[block];
    match record.class {
        PAIR_CLASS => record.bytes.get(..4).map_or(Bonus::Unread, |bytes| {
            Bonus::Literal(f32::from_le_bytes(bytes.try_into().unwrap()))
        }),
        PROGRAM_CLASS => {
            let source = record.bytes.get(INPUT_SOURCE).copied().unwrap_or(STACKS);
            let program = schema::inline(PROGRAM_CLASS)
                .ok()
                .and_then(|inline| {
                    inline
                        .into_iter()
                        .find(|(_, child, _)| *child == native::value::CLASS)
                })
                .and_then(|(offset, _, _)| native::value::Program::read(graph, block, offset).ok());
            let reading = program.map_or(Reading::Varies, |program| {
                reading(&program, source == STACKS)
            });
            Bonus::Formula(reading, source)
        }
        _ => Bonus::Unread,
    }
}

/// Reads a formula by sampling it. Only a stack input reads stack by stack, since other
/// inputs such as ammunition fractions are not counts.
fn reading(program: &native::value::Program, stacks: bool) -> Reading {
    let Some(values) = (0..=MOST_STACKS)
        .map(|count| program.evaluate(count as f32))
        .collect::<Option<Vec<_>>>()
    else {
        return Reading::Varies;
    };
    let same = |a: f32, b: f32| (a - b).abs() < 1e-4;
    let Some(last) = (1..=MOST_STACKS)
        .rev()
        .find(|&count| !same(values[count], values[count - 1]))
    else {
        return Reading::Fixed(values[0]);
    };
    if !stacks || !same(values[0], 0.0) {
        return Reading::Varies;
    }
    let step = values[1];
    if last > 1 && (1..=last).all(|count| same(values[count], step * count as f32)) {
        Reading::PerStack(step, (last < MOST_STACKS).then_some(last))
    } else if last <= LISTED_STACKS {
        Reading::Steps(values[1..=last].to_vec())
    } else {
        Reading::Varies
    }
}

/// A bonus as a signed percentage with at most one decimal.
fn percent(value: f32) -> String {
    let text = format!("{:+.1}", value * 100.0);
    format!("{}%", text.strip_suffix(".0").unwrap_or(&text))
}

/// The Value Source a formula reads, by its choice name.
fn source_name(source: u8) -> String {
    fields::describe(PROGRAM_CLASS)
        .ok()
        .and_then(|fields| {
            fields
                .into_iter()
                .find(|field| field.offset == INPUT_SOURCE)
        })
        .and_then(|field| {
            fields::contract(PROGRAM_CLASS, &field)
                .choices
                .iter()
                .find(|(value, _)| *value == source)
                .map(|(_, name)| (*name).to_owned())
        })
        .unwrap_or_else(|| format!("Value Source {source}"))
}

/// A formula's reading in its tile, level with the controls beside it.
fn formula(ui: &mut egui::Ui, reading: &Reading, source: u8) {
    let secondary = crate::app::style::secondary(ui.visuals());
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| match reading {
            Reading::Fixed(value) => {
                ui.label(percent(*value));
            }
            Reading::Steps(values) => {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (count, value) in values.iter().enumerate() {
                    if count > 0 {
                        ui.add_space(8.0);
                    }
                    ui.label(egui::RichText::new(format!("x{}", count + 1)).color(secondary));
                    ui.label(percent(*value));
                }
            }
            Reading::PerStack(step, last) => {
                ui.label(format!("{} per Stack", percent(*step)));
                if let Some(last) = last {
                    ui.label(egui::RichText::new(format!("up to x{last}")).color(secondary));
                }
            }
            Reading::Varies => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("Varies with {}", source_name(source)))
                            .color(secondary),
                    )
                    .truncate(),
                );
            }
        },
    );
}

/// One row as its list stores it.
struct Row {
    list: usize,
    number: usize,
    field: u8,
    literal: f32,
    selector: u8,
}

fn rows(graph: &Graph, index: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for list in [ASSIGN, MULTIPLY] {
        let Some(&block) = graph.blocks[index].links.get(&list) else {
            continue;
        };
        let block = &graph.blocks[block];
        for number in 0..block.count.unwrap_or(0) {
            let Some(bytes) = block.bytes.get(number * ROW_SIZE..(number + 1) * ROW_SIZE) else {
                continue;
            };
            rows.push(Row {
                list,
                number,
                field: bytes[0],
                literal: f32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                selector: bytes[8],
            });
        }
    }
    rows
}

/// A change the card asked for, applied after drawing so the rows stay put while they draw.
enum Change {
    Literal(usize, usize, f32),
    Bonus(f32),
    Add(usize, u8),
    Remove(usize, usize),
    AddBonus,
    /// Drops the overall bonus, literal or formula.
    RemoveBonus,
    /// Replaces a formula bonus with a literal the card can edit.
    LiteralBonus,
    /// Reads a row's literal instead of a native stat.
    UseLiteral(usize, usize),
}

/// The damage tiles and the command that adds or removes a change, for a damage modifier.
/// Any other node draws nothing here.
pub(super) fn draw(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    if graph.blocks[index].class != CLASS {
        return Ok(());
    }
    let rows = rows(graph, index);
    let bonus = graph.blocks[index]
        .links
        .get(&BONUS)
        .map(|&block| bonus(graph, block));
    let mut change = None;
    // An empty line of tiles still takes a line's height.
    let tiled = !rows.is_empty() || matches!(bonus, Some(Bonus::Literal(_) | Bonus::Formula(..)));
    if tiled {
        crate::app::style::tiles(ui, |ui, width| {
            match &bonus {
                Some(Bonus::Literal(literal)) => {
                    let mut value = literal * 100.0;
                    crate::app::style::tile(
                        ui,
                        width,
                        "damage-bonus",
                        "Damage Bonus",
                        BONUS_HINT,
                        false,
                        |ui| {
                            let response = ui.add_sized(
                                [ui.available_width(), ui.spacing().interact_size.y],
                                egui::DragValue::new(&mut value)
                                    .speed(1.0)
                                    .range(-100.0..=10_000.0)
                                    .clamp_existing_to_range(false)
                                    .custom_formatter(|value, _| format!("{value:+.0}"))
                                    .suffix("%"),
                            );
                            pickers::name_response(ui, &response, "Damage Bonus");
                            if response.changed() {
                                change = Some(Change::Bonus(value / 100.0));
                            }
                        },
                    );
                }
                // A formula computes the bonus instead of a literal. The card reads it by stack,
                // and Show Properties holds the formula itself.
                Some(Bonus::Formula(reading, source)) => {
                    let (wide, hint) = match reading {
                        Reading::Steps(values) => (values.len() > 3, STACK_HINT),
                        Reading::PerStack(..) => (false, STACK_HINT),
                        Reading::Fixed(_) => (false, BONUS_HINT),
                        Reading::Varies => (false, FORMULA_HINT),
                    };
                    let gap = ui.spacing().item_spacing.x;
                    let width = if wide { width * 2.0 + gap } else { width };
                    crate::app::style::tile(
                        ui,
                        width,
                        "damage-bonus",
                        "Damage Bonus",
                        hint,
                        false,
                        |ui| formula(ui, reading, *source),
                    );
                }
                Some(Bonus::Unread) | None => {}
            }
            for row in &rows {
                let multiply = row.list == MULTIPLY;
                let label = row_label(row.field, multiply);
                let hint = if row.selector != LITERAL {
                    STAT_HINT
                } else if multiply {
                    MULTIPLY_HINT
                } else {
                    ASSIGN_HINT
                };
                crate::app::style::tile(
                    ui,
                    width,
                    ("damage-row", row.list, row.number),
                    &label,
                    hint,
                    false,
                    |ui| {
                        if row.selector != LITERAL {
                            ui.label(
                                egui::RichText::new(format!("Reads Stat {}", row.selector))
                                    .color(crate::app::style::secondary(ui.visuals())),
                            );
                            return;
                        }
                        let mut value = row.literal;
                        let response = ui.add_sized(
                            [ui.available_width(), ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut value)
                                .speed(0.01)
                                .max_decimals(3)
                                .suffix(if multiply { " ×" } else { "" }),
                        );
                        pickers::name_response(ui, &response, &label);
                        if response.changed() {
                            change = Some(Change::Literal(row.list, row.number, value));
                        }
                    },
                );
            }
        });
    }
    ui.scope(|ui| {
        crate::app::style::quiet(ui);
        ui.menu_button("Add Damage Change…", |ui| {
            for (label, list, field) in [
                ("Multiply Base Damage Scale", MULTIPLY, 0),
                ("Multiply Precision Bonus", MULTIPLY, 1),
                ("Set Base Damage Scale", ASSIGN, 0),
                ("Set Precision Bonus", ASSIGN, 1),
            ] {
                if ui.button(label).clicked() {
                    change = Some(Change::Add(list, field));
                    ui.close();
                }
            }
            match &bonus {
                None => {
                    if ui.button("Add Damage Bonus").clicked() {
                        change = Some(Change::AddBonus);
                        ui.close();
                    }
                }
                Some(Bonus::Literal(_)) => {}
                // A formula's bonus reads by stack. Making it a number the card edits drops
                // the formula.
                Some(Bonus::Formula(..) | Bonus::Unread) => {
                    if ui.button("Set Damage Bonus to a Number").clicked() {
                        change = Some(Change::LiteralBonus);
                        ui.close();
                    }
                }
            }
            let stat_rows = rows
                .iter()
                .filter(|row| row.selector != LITERAL)
                .collect::<Vec<_>>();
            if !stat_rows.is_empty() {
                ui.separator();
                for row in stat_rows {
                    let label = format!(
                        "Use a Number for {}",
                        row_label(row.field, row.list == MULTIPLY)
                    );
                    if ui.button(label).clicked() {
                        change = Some(Change::UseLiteral(row.list, row.number));
                        ui.close();
                    }
                }
            }
            if !rows.is_empty() || bonus.is_some() {
                ui.separator();
                for row in &rows {
                    let label = format!("Remove {}", row_label(row.field, row.list == MULTIPLY));
                    if ui.button(label).clicked() {
                        change = Some(Change::Remove(row.list, row.number));
                        ui.close();
                    }
                }
                if bonus.is_some() && ui.button("Remove Damage Bonus").clicked() {
                    change = Some(Change::RemoveBonus);
                    ui.close();
                }
            }
        });
    });
    match change {
        Some(change) => apply(graph, index, change),
        None => Ok(()),
    }
}

fn apply(graph: &mut Graph, index: usize, change: Change) -> Result<(), String> {
    let mut changed = graph.clone();
    match change {
        Change::Literal(list, number, value) => {
            let block = changed.make_unique(index, list)?;
            let at = number * ROW_SIZE + 4;
            changed.blocks[block]
                .bytes
                .get_mut(at..at + 4)
                .ok_or("Missing damage row.")?
                .copy_from_slice(&value.to_le_bytes());
        }
        Change::Bonus(value) => {
            let block = changed.make_unique(index, BONUS)?;
            changed.blocks[block]
                .bytes
                .get_mut(..4)
                .ok_or("Missing damage bonus.")?
                .copy_from_slice(&value.to_le_bytes());
        }
        Change::Add(list, field) => {
            if !changed.blocks[index].links.contains_key(&list) {
                changed.create_target(index, list, ROW_CLASS, true)?;
            }
            let block = changed.make_unique(index, list)?;
            let count = changed.blocks[block].count.unwrap_or(0);
            changed.resize_array(block, count + 1)?;
            // A new row reads its literal, and 1 leaves a multiplied field as it was.
            let mut row = [0; ROW_SIZE];
            row[0] = field;
            row[4..8].copy_from_slice(&1.0f32.to_le_bytes());
            row[8] = LITERAL;
            changed.blocks[block].bytes[count * ROW_SIZE..(count + 1) * ROW_SIZE]
                .copy_from_slice(&row);
        }
        Change::Remove(list, number) => {
            let block = changed.make_unique(index, list)?;
            let count = changed.blocks[block].count.unwrap_or(0);
            if count <= 1 {
                // Stock nodes with no rows carry a zero descriptor and no pointer.
                changed.blocks[index].links.remove(&list);
                // Emitting walks from the root, so the dropped rows are left for the program's
                // own compaction when the edit commits.
                changed.blocks[index].bytes[list - 8..list].fill(0);
            } else {
                changed.blocks[block]
                    .bytes
                    .drain(number * ROW_SIZE..(number + 1) * ROW_SIZE);
                changed.blocks[block].count = Some(count - 1);
                changed.synchronize_counts()?;
            }
        }
        Change::AddBonus => changed.create_target(index, BONUS, PAIR_CLASS, false)?,
        // Emitting walks from the root, so the dropped record is left for the program's own
        // compaction when the edit commits.
        Change::RemoveBonus => {
            changed.blocks[index].links.remove(&BONUS);
        }
        Change::LiteralBonus => {
            changed.blocks[index].links.remove(&BONUS);
            changed.create_target(index, BONUS, PAIR_CLASS, false)?;
        }
        Change::UseLiteral(list, number) => {
            let block = changed.make_unique(index, list)?;
            let at = number * ROW_SIZE;
            let row = changed.blocks[block]
                .bytes
                .get_mut(at..at + ROW_SIZE)
                .ok_or("Missing damage row.")?;
            // The literal starts neutral, as a row added from the card does.
            row[4..8].copy_from_slice(&1.0f32.to_le_bytes());
            row[8] = LITERAL;
        }
    }
    changed.validate()?;
    *graph = changed;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sundial::package_authoring::sandbox_perk::action::native::NodeKind as NativeNodeKind;

    fn modifier() -> Graph {
        let bytes = native::template(NativeNodeKind::Effect(40)).unwrap();
        Graph::read(&bytes, 0, CLASS).unwrap()
    }

    /// Rows added from the card use their literal and leave damage as it was, and removing
    /// them returns the node to the stock shape with no rows.
    #[test]
    fn damage_changes_add_neutral_literal_rows_and_remove_to_the_stock_shape() {
        let mut graph = modifier();
        let stock = graph.emit().unwrap();
        assert!(rows(&graph, 0).is_empty());
        apply(&mut graph, 0, Change::Add(MULTIPLY, 0)).unwrap();
        apply(&mut graph, 0, Change::Add(MULTIPLY, 1)).unwrap();
        apply(&mut graph, 0, Change::Add(ASSIGN, 0)).unwrap();
        let added = rows(&graph, 0)
            .iter()
            .map(|row| (row.list, row.field, row.literal, row.selector))
            .collect::<Vec<_>>();
        assert_eq!(
            added,
            [
                (ASSIGN, 0, 1.0, LITERAL),
                (MULTIPLY, 0, 1.0, LITERAL),
                (MULTIPLY, 1, 1.0, LITERAL),
            ]
        );
        apply(&mut graph, 0, Change::Literal(MULTIPLY, 1, 1.5)).unwrap();
        assert_eq!(
            rows(&graph, 0)
                .iter()
                .find(|row| row.list == MULTIPLY && row.field == 1)
                .unwrap()
                .literal,
            1.5
        );
        graph.validate_node(NativeNodeKind::Effect(40)).unwrap();
        apply(&mut graph, 0, Change::Remove(MULTIPLY, 0)).unwrap();
        assert_eq!(rows(&graph, 0).len(), 2);
        apply(&mut graph, 0, Change::Remove(MULTIPLY, 0)).unwrap();
        apply(&mut graph, 0, Change::Remove(ASSIGN, 0)).unwrap();
        assert!(rows(&graph, 0).is_empty());
        assert_eq!(graph.emit().unwrap(), stock);
    }

    fn program(code: &[u8], rows: &[[f32; 4]]) -> native::value::Program {
        let mut instructions = Vec::new();
        let mut at = 0;
        while at < code.len() {
            let opcode = code[at];
            let operand = (opcode == 34 || opcode >= 52).then(|| code[at + 1]);
            at += 1 + usize::from(operand.is_some());
            instructions.push(native::value::Instruction { opcode, operand });
        }
        native::value::Program {
            instructions,
            constants: rows.iter().map(|row| row.map(f32::to_bits)).collect(),
            fast_path: 0,
        }
    }

    /// A formula reads as its bonus per stack when every stack adds the same, stack by stack
    /// when a short curve does not, and as varying when its input is not a count.
    #[test]
    fn damage_formulas_read_by_stack() {
        // Swashbuckler's shape: a third more damage at five stacks, a fifth of that per stack.
        let ramp = program(
            &[
                52, 0, 60, 0, 34, 0, 52, 1, 3, 35, 35, 34, 0, 52, 2, 15, 35, 34, 0, 3, 62, 0,
            ],
            &[[1.0 / 3.0; 4], [0.2, 1.0, 1.0, 1.0], [0.0, 0.0, 1.0, 0.0]],
        );
        let Reading::PerStack(step, last) = reading(&ramp, true) else {
            panic!("{:?}", reading(&ramp, true));
        };
        assert!((step - 1.0 / 15.0).abs() < 1e-6);
        assert_eq!(last, Some(5));
        assert_eq!(percent(step), "+6.7%");
        // The square of a third of the stacks, capped at three: a curve.
        let curve = program(
            &[60, 0, 34, 0, 52, 0, 3, 35, 34, 0, 52, 1, 15, 62, 0],
            &[[1.0 / 3.0; 4], [0.0, 1.0, 0.0, 0.0]],
        );
        let Reading::Steps(values) = reading(&curve, true) else {
            panic!("{:?}", reading(&curve, true));
        };
        assert_eq!(
            values
                .iter()
                .map(|value| percent(*value))
                .collect::<Vec<_>>(),
            ["+11.1%", "+44.4%", "+100%"]
        );
        // Ammunition inputs are fractions, not stacks, so the same curve only varies.
        assert_eq!(reading(&curve, false), Reading::Varies);
        let constant = program(&[52, 0, 62, 0], &[[-0.25; 4]]);
        assert_eq!(reading(&constant, false), Reading::Fixed(-0.25));
    }

    /// The stock template's overall bonus is a literal 0.5, which the card now shows.
    #[test]
    fn the_damage_bonus_reads_and_writes_the_literal_pair() {
        let mut graph = modifier();
        apply(&mut graph, 0, Change::Bonus(0.25)).unwrap();
        let pair = graph.blocks[0].links[&BONUS];
        assert_eq!(graph.blocks[pair].bytes[..4], 0.25f32.to_le_bytes());
    }
}
