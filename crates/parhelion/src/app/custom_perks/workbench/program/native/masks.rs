//! Named bit choices preserve combinations and bits whose meanings remain unknown.
use super::*;

pub(super) fn draw(
    ui: &mut egui::Ui,
    field: &fields::Field,
    block: &mut native::Block,
    row: usize,
    contract: &fields::ValueContract,
) -> Result<(), String> {
    let bytes = field.bytes(block, row).ok_or("Truncated native mask.")?;
    let before = match bytes {
        [byte] => u32::from(*byte),
        [a, b, c, d] => u32::from_le_bytes([*a, *b, *c, *d]),
        _ => return Err("Invalid native mask width.".into()),
    };
    let mut value = before;
    column(ui, |ui| {
        egui::ComboBox::from_id_salt("native-mask")
            .width(COLUMN_WIDTH)
            .truncate()
            .selected_text(summary(value, contract.choices))
            .show_ui(ui, |ui| {
                for (bit, name) in contract.choices {
                    let bit = u32::from(*bit);
                    let mut on = value & bit != 0;
                    if ui.checkbox(&mut on, *name).changed() {
                        if on {
                            value |= bit;
                        } else {
                            value &= !bit;
                        }
                    }
                }
                egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                    for (observed, uses, perks) in contract.observed {
                        ui.selectable_value(
                            &mut value,
                            u32::from(*observed),
                            format!("0x{observed:02X}"),
                        )
                        .on_hover_text(format!("{uses} stock perk records. {perks}"));
                    }
                    let raw = hex_key(ui, "native-mask-hex", &mut value);
                    pickers::name_response(ui, &raw, "Mask as Hex");
                });
            })
            .response
            .on_hover_text(contract.description);
        pickers::name_combo(
            ui,
            "native-mask",
            plain_field_label(block.class, &field.label),
        );
    });
    if value != before {
        write(field, block, row, value)?;
    }
    Ok(())
}

fn write(
    field: &fields::Field,
    block: &mut native::Block,
    row: usize,
    value: u32,
) -> Result<(), String> {
    if field.width == 1 {
        let value = u8::try_from(value).map_err(|_| "This mask holds 8 bits.")?;
        field.write(block, row, &[value])
    } else {
        field.write(block, row, &value.to_le_bytes())
    }
}

fn summary(value: u32, choices: &[(u8, &str)]) -> String {
    if value == 0 {
        return "None".to_owned();
    }
    let mut remaining = value;
    let mut names = Vec::new();
    for (bit, name) in choices {
        let bit = u32::from(*bit);
        if value & bit != 0 {
            names.push((*name).to_owned());
            remaining &= !bit;
        }
    }
    if remaining != 0 {
        names.push(format!("0x{remaining:X}"));
    }
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_filters_preserve_unknown_bits_and_other_fields() {
        for (class, offset) in [
            (0x80803DFB, 9),
            (0x80803E01, 8),
            (0x80803DFD, 8),
            (0x80803E00, 8),
            (0x80803DCE, 0x38),
            (0x80803DCC, 0x38),
        ] {
            let node = nodes::CONDITIONS
                .iter()
                .find(|node| node.class == class)
                .unwrap();
            let mut graph =
                Graph::read(&native::template(true, node.kind).unwrap(), 0, class).unwrap();
            let field = fields::describe(class)
                .unwrap()
                .into_iter()
                .find(|field| field.offset == offset)
                .unwrap();
            let contract = fields::contract(class, &field);
            assert!(contract.bitmask);
            let first = u32::from(contract.choices[0].0);
            let second = u32::from(contract.choices[1].0);
            let unknown = (0..8)
                .map(|bit| 1u32 << bit)
                .find(|bit| {
                    contract
                        .choices
                        .iter()
                        .all(|(known, _)| u32::from(*known) & bit == 0)
                })
                .expect("this filter still has unidentified bits");
            let original = graph.blocks[0].bytes.clone();
            write(&field, &mut graph.blocks[0], 0, first | second | unknown).unwrap();
            assert!(
                summary(first | second | unknown, contract.choices)
                    .contains(&format!("0x{unknown:X}"))
            );
            write(
                &field,
                &mut graph.blocks[0],
                0,
                (first | second | unknown) & !first,
            )
            .unwrap();
            let read = Graph::read(&graph.emit().unwrap(), 0, class).unwrap();
            assert_eq!(
                field.bytes(&read.blocks[0], 0).unwrap()[0],
                (second | unknown) as u8
            );
            assert_eq!(&read.blocks[0].bytes[..offset], &original[..offset]);
            assert_eq!(
                &read.blocks[0].bytes[offset + field.width..],
                &original[offset + field.width..]
            );
            let before = graph.blocks[0].bytes.clone();
            if field.width == 1 {
                assert!(write(&field, &mut graph.blocks[0], 0, 256).is_err());
                assert_eq!(graph.blocks[0].bytes, before);
            }
        }
    }
}
