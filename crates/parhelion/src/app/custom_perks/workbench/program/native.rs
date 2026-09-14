//! Complete native records in the private-perk editor.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{
    self, Graph,
    fields::{self, Format},
    schema,
};

pub(in crate::app::custom_perks::workbench) fn draw_complete(
    ui: &mut egui::Ui,
    program: &mut sundial::package_authoring::sandbox_perk::program::NativeProgram,
    editing: bool,
) -> Option<usize> {
    ui.strong("Complete Program");
    ui.small("Edit the condition lists, effects, groups and execution settings below.");
    let mut changed = program.clone();
    let result = ui
        .add_enabled_ui(editing, |ui| {
            allocation(ui, &mut changed.graph, 0, &mut Vec::new())?;
            changed.sync_assets()?;
            changed.validate()
        })
        .inner;
    match result {
        Ok(()) => *program = changed,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    let mut edit = None;
    for (index, asset) in program.assets.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(if asset.path.is_empty() {
                format!("Asset 0x{:08X}", asset.graph)
            } else {
                sundial::package_authoring::tft::asset_label(&asset.path)
            });
            if ui
                .add_enabled_ui(editing, |ui| {
                    super::super::properties::edit_object(ui, asset)
                })
                .inner
            {
                edit = Some(index);
            }
            if !asset.values.is_empty() {
                ui.weak(format!("{} Changes", asset.values.len()));
            }
        });
    }
    edit
}

pub(super) fn draw(ui: &mut egui::Ui, id: &str, node: &mut NativeNode, family: NativeFamily) {
    let Some(entry) = family.catalog(node.kind) else {
        return;
    };
    ui.small(entry.summary);
    let result = Graph::read(&node.bytes, 0, entry.class).and_then(|mut graph| {
        ui.push_id(id, |ui| allocation(ui, &mut graph, 0, &mut Vec::new()))
            .inner?;
        graph.validate_node(family == NativeFamily::Condition, node.kind)?;
        graph.emit()
    });
    match result {
        Ok(bytes) => node.bytes = bytes,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}

fn allocation(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    ancestors: &mut Vec<usize>,
) -> Result<(), String> {
    if ancestors.contains(&index) || ancestors.len() > 32 {
        return Err("The native structure contains a cycle or nests too deeply.".into());
    }
    ancestors.push(index);
    let class = graph.blocks[index].class;
    if class == 0 {
        let bytes = &mut graph.blocks[index].bytes;
        let mut path = String::from_utf8(bytes[..bytes.len() - 1].to_vec())
            .map_err(|_| "Invalid native path.")?;
        if ui.text_edit_singleline(&mut path).changed() && !path.contains('\0') {
            *bytes = path.into_bytes();
            bytes.push(0);
        }
    } else if matches!(class, 0x808093F5 | 0x808093F6 | 0x8080407B) {
        ui.weak("Compiled from this program when it is built.");
    } else {
        let count = graph.blocks[index].count;
        if let Some(count) = count {
            ui.horizontal(|ui| {
                ui.label(format!("{count} Entries"));
                if ui
                    .add_enabled(count < 256, egui::Button::new("Add Entry"))
                    .clicked()
                {
                    graph.resize_array(index, count + 1)?;
                }
                if ui
                    .add_enabled(count > 0, egui::Button::new("Remove Last Entry"))
                    .clicked()
                {
                    graph.resize_array(index, count.saturating_sub(1))?;
                }
                Ok::<_, String>(())
            })
            .inner?;
        }
        let stride = schema::record(class)?.size;
        for row in 0..graph.blocks[index].count.unwrap_or(1) {
            if count.is_some() {
                egui::CollapsingHeader::new(format!("{} {}", fields::name(class), row + 1))
                    .id_salt((index, row))
                    .show(ui, |ui| record(ui, graph, index, row, stride, ancestors))
                    .body_returned
                    .transpose()?;
            } else {
                record(ui, graph, index, row, stride, ancestors)?;
            }
        }
    }
    egui::CollapsingHeader::new("Native Bytes")
        .id_salt((index, "native-bytes"))
        .show(ui, |ui| {
            for (row, bytes) in graph.blocks[index].bytes.chunks(16).enumerate() {
                ui.monospace(format!("{:04X}  {}", row * 16, hex(bytes)));
            }
        });
    ancestors.pop();
    Ok(())
}

fn record(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    row: usize,
    stride: usize,
    ancestors: &mut Vec<usize>,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    let views = schema::inline(class)?;
    for (at, child, _) in &views {
        if *child == native::value::CLASS {
            egui::CollapsingHeader::new(format!("Value Program +0x{at:02X}"))
                .id_salt((index, row, at, "value"))
                .show(ui, |ui| value(ui, graph, index, row * stride + at))
                .body_returned
                .transpose()?;
        }
    }
    for field in fields::describe(class)? {
        let in_program = views.iter().any(|(at, child, _)| {
            *child == native::value::CLASS && (field.offset >= *at && field.offset < at + 48)
        });
        let generated_predicate = views.iter().any(|(at, child, _)| {
            *child == native::labels::PREDICATE_CLASS
                && field.offset >= *at
                && field.offset < at + 16
        });
        let added_mask = matches!(class, 0x80803E1A) && (0xA8..0xD0).contains(&field.offset)
            || matches!(class, 0x8080281C) && (0x78..0xA0).contains(&field.offset);
        if in_program || generated_predicate || added_mask {
            continue;
        }
        let at = row * stride + field.offset;
        ui.push_id((index, row, field.offset), |ui| {
            if field.format == Format::Pointer {
                pointer(ui, graph, index, at, field.offset, ancestors)
            } else {
                let component_target = field.editable
                    && class == 0x80803E4D
                    && field.offset == 2
                    && graph.blocks[index]
                        .bytes
                        .get(row * stride + 3..row * stride + 5)
                        == Some(&[0, 0]);
                super::super::properties::field(
                    ui,
                    &field.label,
                    &format!("Native byte +0x{:02X}", field.offset),
                    |ui| {
                        if component_target {
                            let target = graph.blocks[index]
                                .bytes
                                .get_mut(at)
                                .ok_or("Ability target extends past the component record")?;
                            ability_target(ui, target);
                            return Ok(());
                        }
                        scalar(ui, &field, &mut graph.blocks[index], row)
                    },
                )
            }
        })
        .inner?;
    }
    Ok(())
}

fn ability_target(ui: &mut egui::Ui, target: &mut u8) {
    use sundial::package_authoring::sandbox_perk::action::component_target;
    egui::ComboBox::from_id_salt("ability-target")
        .selected_text(component_target(*target, 0, 0).unwrap_or("Unmapped Target"))
        .show_ui(ui, |ui| {
            for selector in [0, 2, 7] {
                ui.selectable_value(target, selector, component_target(selector, 0, 0).unwrap());
            }
        }).response.on_hover_text(
            "Source-derived from Innervation and Bomber, Invigoration and Outreach, Insulation and Perpetuation. The numeric selector remains editable."
        );
    ui.add(egui::DragValue::new(target).range(0..=255))
        .on_hover_text("Native target selector. Other values have no identified ability role.");
}

fn pointer(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    at: usize,
    field: usize,
    ancestors: &mut Vec<usize>,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    if class == 0x808040B5 && field == 0xB0 {
        ui.weak("Group routing is compiled from the condition lists.");
        return Ok(());
    }
    let target = graph.blocks[index].links.get(&at).copied();
    let mut name = target.map_or_else(
        || "Reference".into(),
        |i| fields::name(graph.blocks[i].class),
    );
    let group_field = if class == 0x808040B5 && (0x20..0x68).contains(&field) {
        Some(field - 0x20)
    } else if class == 0x8080407D {
        Some(field)
    } else {
        None
    };
    if let Some(label) = match group_field {
        Some(0x08) => Some("Starts When"),
        Some(0x20) => Some("Effects"),
        Some(0x30) => Some("Ends When"),
        Some(0x40) => Some("Ready Again When"),
        _ => None,
    } {
        name = label.into();
    }
    if class == 0x808040B5 {
        match field {
            0x18 => name = "Auxiliary Records".into(),
            0x70 => name = "Additional Groups".into(),
            0x78 => name = "Execution Policy Settings".into(),
            _ => {}
        }
    }
    for (source, kind, _) in schema::inline(class)? {
        if kind == native::labels::SOURCE_CLASS
            && field >= source + 8
            && (field - source - 8) % 16 == 0
            && field < source + 64
        {
            name = [
                "Any Labels",
                "All Labels",
                "Excluded Labels",
                "Not All Labels",
            ][(field - source - 8) / 16]
                .into();
        }
    }
    egui::CollapsingHeader::new(format!("{name} +0x{field:02X}"))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut choices = schema::choices(class, field);
                if let Some((_, code)) = schema::record(class)?
                    .fields
                    .iter()
                    .find(|(offset, _)| *offset == field)
                {
                    if matches!(code, 1 | 2) {
                        choices = vec![(0, false)];
                    }
                }
                ui.menu_button("Choose Reference", |ui| {
                    for (class, array) in choices {
                        if ui.button(fields::name(class)).clicked() {
                            graph.create_target(index, at, class, array)?;
                            ui.close_menu();
                        }
                    }
                    Ok::<_, String>(())
                })
                .inner
                .transpose()?;
                if let Some(target) = target
                    && ui.button("Clear Reference").clicked()
                {
                    if graph.blocks[target].count.is_some() {
                        graph.blocks[index].bytes[at - 8..at].fill(0);
                    }
                    graph.blocks[index].links.remove(&at);
                }
                Ok::<_, String>(())
            })
            .inner?;
            if let Some(target) = graph.blocks[index].links.get(&at).copied() {
                allocation(ui, graph, target, ancestors)?;
            } else {
                ui.weak("No reference selected.");
            }
            Ok::<_, String>(())
        })
        .body_returned
        .transpose()?;
    Ok(())
}

fn scalar(
    ui: &mut egui::Ui,
    field: &fields::Field,
    block: &mut native::Block,
    row: usize,
) -> Result<(), String> {
    let bytes = field
        .bytes(block, row)
        .ok_or("Truncated native field.")?
        .to_vec();
    if !field.editable {
        ui.weak(hex(&bytes));
        return Ok(());
    }
    let mapped = nodes::CONDITIONS
        .iter()
        .find(|entry| entry.class == block.class)
        .and_then(|entry| layout::condition_layout(entry.kind))
        .or_else(|| {
            nodes::EFFECTS
                .iter()
                .find(|entry| entry.class == block.class)
                .and_then(|entry| layout::effect_layout(entry.kind))
        })
        .and_then(|layout| {
            layout.fields.iter().find(|mapped| {
                mapped.offset == field.offset && mapped.format.width() == field.width
            })
        });
    if let Some(mapped) = mapped {
        let start = row * schema::record(block.class)?.size;
        super::draw_native_field(ui, mapped, &mut block.bytes[start..]);
        return Ok(());
    }
    let changed = match field.format {
        Format::Flag => {
            let mut v = bytes[0] != 0;
            ui.checkbox(&mut v, "").changed().then(|| vec![u8::from(v)])
        }
        Format::Float => {
            let mut v = f32::from_le_bytes(bytes.try_into().map_err(|_| "Invalid float field.")?);
            ui.add(egui::DragValue::new(&mut v).speed(0.01))
                .changed()
                .then(|| v.to_le_bytes().to_vec())
        }
        Format::Key | Format::Tag => {
            let mut value = u32::from_le_bytes(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid native key width.")?,
            );
            let before = value;
            hex_key(ui, "native-key", &mut value);
            if block.class == 0x808094B3 && field.offset == 0 {
                if let Some(name) =
                    sundial::package_authoring::sandbox_perk::action::label_name(value)
                {
                    ui.weak(name);
                }
            }
            (value != before).then(|| value.to_le_bytes().to_vec())
        }
        Format::Byte => {
            let mut v = bytes[0];
            if block.class == 0x80803E42 && field.offset == 2 {
                egui::ComboBox::from_id_salt("orb-position")
                    .selected_text(if v == 1 {
                        "Event Position"
                    } else {
                        "Owner Position"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut v, 0, "Owner Position");
                        ui.selectable_value(&mut v, 1, "Event Position");
                    });
                (v != bytes[0]).then(|| vec![v])
            } else {
                ui.add(egui::DragValue::new(&mut v))
                    .changed()
                    .then(|| vec![v])
            }
        }
        Format::Integer => {
            let mut value =
                i32::from_le_bytes(bytes.try_into().map_err(|_| "Invalid integer field.")?);
            ui.add(egui::DragValue::new(&mut value))
                .changed()
                .then(|| value.to_le_bytes().to_vec())
        }
        _ => hex_input(ui, &bytes, false),
    };
    if let Some(bytes) = changed {
        field.write(block, row, &bytes)?;
    }
    Ok(())
}

fn value(ui: &mut egui::Ui, graph: &mut Graph, index: usize, at: usize) -> Result<(), String> {
    let mut program = native::value::Program::read(graph, index, at)?;
    let original = program.clone();
    let mut fast_path = program.fast_path == 1;
    if ui
        .checkbox(&mut fast_path, "Polynomial Fast Path")
        .on_hover_text("This mode uses constant vector zero to evaluate a clamped cubic.")
        .changed()
    {
        program.fast_path = u32::from(fast_path);
    }
    for (row, constant) in program.constants.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("Constant {row}"));
            for lane in constant {
                let mut v = f32::from_bits(*lane);
                if ui.add(egui::DragValue::new(&mut v).speed(0.01)).changed() && v.is_finite() {
                    *lane = v.to_bits();
                }
            }
        });
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                program.constants.len() < 256,
                egui::Button::new("Add Constant"),
            )
            .clicked()
        {
            program.constants.push([0; 4]);
        }
        if ui
            .add_enabled(
                !program.constants.is_empty(),
                egui::Button::new("Remove Last Constant"),
            )
            .clicked()
        {
            program.constants.pop();
        }
    });
    ui.small("Instructions use hexadecimal opcode and operand bytes.");
    let code = program
        .instructions
        .iter()
        .flat_map(|i| std::iter::once(i.opcode).chain(i.operand))
        .collect::<Vec<_>>();
    if let Some(bytes) = hex_input(ui, &code, true) {
        let mut instructions = Vec::new();
        let mut p = 0;
        while p < bytes.len() {
            let opcode = bytes[p];
            p += 1;
            let operand = if opcode == 34 || opcode >= 52 {
                let operand = *bytes.get(p).ok_or("Instruction needs an operand.")?;
                p += 1;
                Some(operand)
            } else {
                None
            };
            instructions.push(native::value::Instruction { opcode, operand });
        }
        program.instructions = instructions;
        program.fast_path = 0;
    }
    for instruction in &program.instructions {
        ui.weak(native::value::instruction_name(instruction.opcode));
    }
    if program != original {
        program.write(graph, index, at)?;
    }
    Ok(())
}

fn hex_input(ui: &mut egui::Ui, bytes: &[u8], multiline: bool) -> Option<Vec<u8>> {
    let id = ui.id().with("native-hex-input");
    let mut state = ui
        .data(|data| data.get_temp::<(Vec<u8>, String)>(id))
        .filter(|state| state.0 == bytes)
        .unwrap_or_else(|| (bytes.to_vec(), hex(bytes)));
    if multiline {
        ui.text_edit_multiline(&mut state.1);
    } else {
        ui.add(egui::TextEdit::singleline(&mut state.1).desired_width(120.0));
    }
    let mut result = None;
    if ui.button("Apply Bytes").clicked() {
        match parse_hex(&state.1) {
            Ok(value) => {
                result = Some(value);
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    result
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.split_whitespace().collect::<String>();
    if !digits.is_ascii() || digits.len() % 2 != 0 {
        return Err("Enter complete hexadecimal bytes.".into());
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| "Enter hexadecimal bytes.".into())
        })
        .collect()
}
