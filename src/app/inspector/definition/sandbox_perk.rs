use super::*;

pub(super) fn draw_hash_sandbox_perk_definition(
    ui: &mut egui::Ui,
    definition: &SandboxPerkDefinition,
) {
    let runtime_data = definition.runtime_data.as_deref();
    ui.add_space(8.0);
    hash_metadata_section(ui, "Sandbox perk definition", true, |ui| {
        ui.label(
            "Exact hash-and-index alignment between the package definition and runtime tables.",
        );
        ui.add_space(6.0);
        if hash_inspector_uses_wide_summary(ui.available_width()) {
            ui.columns(2, |columns| {
                draw_sandbox_perk_definition_summary(&mut columns[0], definition);
                draw_sandbox_perk_runtime_summary(&mut columns[1], definition, runtime_data);
            });
        } else {
            draw_sandbox_perk_definition_summary(ui, definition);
            ui.add_space(8.0);
            draw_sandbox_perk_runtime_summary(ui, definition, runtime_data);
        }
        let Some(runtime_data) = runtime_data else {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "The runtime-table pointer is null, so the package provides no runtime resource for this definition.",
                )
                .weak(),
            );
            return;
        };
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let copied_id = egui::Id::new(("sandbox_runtime_hex_copied", definition.hash));
            let copied = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(copied_id).unwrap_or(false));
            let copy = ui
                .small_button(if copied { "Copied" } else { "Copy hex" })
                .on_hover_text(if copied {
                    "Copy the stored runtime data bytes again"
                } else {
                    "Copy all stored runtime data bytes"
                });
            if copy.clicked() {
                ui.ctx().copy_text(sandbox_runtime_hex(runtime_data));
                ui.ctx()
                    .data_mut(|data| data.insert_temp(copied_id, true));
            }
            ui.label(
                egui::RichText::new(
                    "Exact stored bytes after the 16-byte resource header. The span may include alignment padding; unknown meanings are not inferred.",
                )
                .weak(),
            );
        });
        egui::CollapsingHeader::new(format!("Raw bytes ({})", runtime_data.len()))
            .id_salt(("hash_sandbox_perk_runtime_bytes", definition.hash))
            .default_open(runtime_data.len() <= 128)
            .show(ui, |ui| {
                let bytes_per_row = sandbox_runtime_bytes_per_row(ui.available_width());
                egui::ScrollArea::horizontal()
                    .id_salt(("hash_sandbox_perk_runtime_bytes_scroll", definition.hash))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let runtime_offset = definition.runtime_offset.unwrap_or_default() as usize;
                        egui::Grid::new(("hash_sandbox_perk_runtime_bytes_grid", definition.hash))
                            .num_columns(4)
                            .spacing([16.0, 3.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Data offset");
                                ui.strong("Tag offset");
                                ui.strong("Hex bytes");
                                ui.strong("ASCII");
                                ui.end_row();
                                for (line_offset, bytes) in
                                    runtime_data.chunks(bytes_per_row).enumerate()
                                {
                                    let data_offset = line_offset * bytes_per_row;
                                    ui.monospace(format!("+0x{data_offset:04X}"));
                                    ui.monospace(format!(
                                        "0x{:08X}",
                                        runtime_offset + 16 + data_offset
                                    ));
                                    ui.monospace(
                                        bytes
                                            .iter()
                                            .map(|byte| format!("{byte:02X}"))
                                            .collect::<Vec<_>>()
                                            .join(" "),
                                    );
                                    ui.monospace(
                                        bytes
                                            .iter()
                                            .map(|byte| {
                                                if byte.is_ascii_graphic() || *byte == b' ' {
                                                    char::from(*byte)
                                                } else {
                                                    '·'
                                                }
                                            })
                                            .collect::<String>(),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            });
        egui::CollapsingHeader::new(format!(
            "32-bit interpretations ({} words)",
            runtime_data.len().div_ceil(4)
        ))
        .id_salt(("hash_sandbox_perk_runtime_words", definition.hash))
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(("hash_sandbox_perk_runtime_words_scroll", definition.hash))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new(("hash_sandbox_perk_runtime_words", definition.hash))
                        .num_columns(5)
                        .spacing([16.0, 3.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Offset");
                            ui.strong("Bytes");
                            ui.strong("Unsigned");
                            ui.strong("Signed");
                            ui.strong("Float");
                            ui.end_row();
                            for (word_index, bytes) in runtime_data.chunks(4).enumerate() {
                                ui.monospace(format!("+0x{:04X}", word_index * 4));
                                ui.monospace(
                                    bytes
                                        .iter()
                                        .map(|byte| format!("{byte:02X}"))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                );
                                if let Ok(bytes) = <[u8; 4]>::try_from(bytes) {
                                    ui.monospace(u32::from_le_bytes(bytes).to_string());
                                    ui.monospace(i32::from_le_bytes(bytes).to_string());
                                    ui.monospace(format!("{:.7e}", f32::from_le_bytes(bytes)));
                                } else {
                                    ui.label(egui::RichText::new("-").weak());
                                    ui.label(egui::RichText::new("-").weak());
                                    ui.label(egui::RichText::new("-").weak());
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    });
}

const HASH_INSPECTOR_WIDE_SUMMARY_WIDTH: f32 = 720.0;

pub(super) fn hash_inspector_uses_wide_summary(available_width: f32) -> bool {
    available_width >= HASH_INSPECTOR_WIDE_SUMMARY_WIDTH
}

fn draw_sandbox_perk_definition_summary(ui: &mut egui::Ui, definition: &SandboxPerkDefinition) {
    metadata_subsection(ui, "Definition", |ui| {
        egui::Grid::new(("hash_sandbox_perk_definition_summary", definition.hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Name").weak());
                ui.label(egui::RichText::new("<not resolved>").weak().italics())
                    .on_hover_text(SANDBOX_PERK_NAME_UNRESOLVED_HELP);
                ui.end_row();
                hash_detail_field(
                    ui,
                    "Definition hash",
                    format_hash_hex_and_decimal(definition.hash),
                    true,
                );
                hash_detail_field(ui, "Index", definition.definition_index.to_string(), true);
                hash_detail_field(
                    ui,
                    "Category",
                    format!("0x{:016X} · {}", definition.category, definition.category),
                    true,
                );
                hash_detail_field(ui, "Source", "root[88] · row class 0x8080748C", true);
            });
    });
}

fn draw_sandbox_perk_runtime_summary(
    ui: &mut egui::Ui,
    definition: &SandboxPerkDefinition,
    runtime_data: Option<&[u8]>,
) {
    metadata_subsection(ui, "Runtime resource", |ui| {
        egui::Grid::new(("hash_sandbox_perk_runtime_summary", definition.hash))
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                hash_detail_field(ui, "Source", "globals[53] · table class 0x80805961", true);
                hash_detail_field(
                    ui,
                    "Status",
                    if runtime_data.is_some() {
                        "Resource present"
                    } else {
                        "Definition only"
                    },
                    false,
                );
                hash_detail_field(
                    ui,
                    "Category",
                    format!(
                        "0x{:016X} · {}{}",
                        definition.runtime_category,
                        definition.runtime_category,
                        if definition.runtime_category == definition.category {
                            " (same)"
                        } else {
                            " (variant)"
                        }
                    ),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Offset",
                    definition.runtime_offset.map_or_else(
                        || "<not present>".to_owned(),
                        |offset| format!("0x{offset:08X}"),
                    ),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Class",
                    definition.runtime_class.map_or_else(
                        || "<not present>".to_owned(),
                        |class| format!("0x{class:08X}"),
                    ),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Stored span",
                    runtime_data
                        .map(|data| (data.len() + 16).to_string())
                        .unwrap_or_else(|| "0".to_owned()),
                    true,
                );
                hash_detail_field(
                    ui,
                    "Data bytes",
                    runtime_data
                        .map(|data| data.len().to_string())
                        .unwrap_or_else(|| "0".to_owned()),
                    true,
                );
            });
    });
}

pub(super) fn sandbox_runtime_bytes_per_row(available_width: f32) -> usize {
    if available_width >= 1_020.0 {
        32
    } else if available_width >= 760.0 {
        24
    } else {
        16
    }
}

pub(super) fn sandbox_runtime_hex(data: &[u8]) -> String {
    data.iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_hex_is_copy_ready() {
        assert_eq!(
            sandbox_runtime_hex(&[0x00, 0x7F, 0x80, 0xFF]),
            "00 7F 80 FF"
        );
    }

    #[test]
    fn summaries_use_wide_space_without_squeezing_narrow_views() {
        assert!(!hash_inspector_uses_wide_summary(719.0));
        assert!(hash_inspector_uses_wide_summary(720.0));
        assert_eq!(sandbox_runtime_bytes_per_row(759.0), 16);
        assert_eq!(sandbox_runtime_bytes_per_row(760.0), 24);
        assert_eq!(sandbox_runtime_bytes_per_row(1_020.0), 32);
    }
}
