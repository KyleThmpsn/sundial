//! Form fields and labels the editor pages share: identity rows, paths, socket contexts,
//! section labels and choice labels.
use super::*;

pub(super) const SOCKET_CHOICE_PAGE_SIZE: usize = 12;
pub(super) const LIVE_SOCKET_DIAGNOSTIC_CHOICE_LIMIT: usize = 512;

pub(super) struct SocketPickerContext<'a> {
    pub(super) catalog: &'a InvestmentCatalog,
    pub(super) recipe_library: Option<&'a RecipeLibrary>,
    pub(super) recipe: &'a mut WeaponRecipe,
    pub(super) queries: &'a mut Vec<BTreeMap<usize, String>>,
    pub(super) pages: &'a mut Vec<usize>,
    pub(super) plug_selection_mode: &'a mut PlugSelectionMode,
    pub(super) show_plug_safety_warnings: bool,
    pub(super) show_experimental_options: bool,
    pub(super) show_technical_rows: &'a mut bool,
    pub(super) perk_request: &'a mut Option<crate::app::custom_perks::workbench::Request>,
    pub(super) donor: &'a WeaponDonor,
    pub(super) log: &'a mut ActivityLog,
}

/// The spacing and rule between two stacked workbench sections.
pub(super) fn draw_stacked_section_break(ui: &mut egui::Ui) {
    ui.add_space(3.0);
    ui.separator();
    ui.add_space(3.0);
}

/// Gameplay and Appearance sit side by side when there is room, and stack when there is not.
pub(super) fn donor_section_column_count(available_width: f32) -> usize {
    if available_width >= 780.0 { 2 } else { 1 }
}

pub(super) struct SocketTechnicalFields<'a> {
    pub(super) catalog: &'a InvestmentCatalog,
    pub(super) recipe: &'a mut WeaponRecipe,
    pub(super) donor: &'a WeaponDonor,
    pub(super) socket_index: usize,
    pub(super) is_added: bool,
    pub(super) inherited: &'a [u32],
    pub(super) page: &'a mut usize,
    pub(super) queries: &'a mut BTreeMap<usize, String>,
    pub(super) scroll_to_header: bool,
}

pub(super) fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> bool {
    let mut changed = false;
    ui.strong(label).on_hover_text(hint);
    ui.horizontal(|ui| {
        let field_width = (ui.available_width() - 86.0).max(160.0);
        changed |= ui
            .add_sized(
                [field_width, ui.spacing().interact_size.y],
                egui::TextEdit::singleline(value),
            )
            .on_hover_text(hint)
            .changed();
        if ui.button("Browse…").clicked()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            *value = folder.display().to_string();
            changed = true;
        }
    });
    changed
}

pub(super) struct IdentityField {
    pub(super) label: String,
    pub(super) value: String,
    pub(super) copyable: bool,
    pub(super) help: Option<&'static str>,
}

impl IdentityField {
    pub(super) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            copyable: true,
            help: None,
        }
    }

    pub(super) fn assigned_during_build(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: "Assigned during build".into(),
            copyable: false,
            help: None,
        }
    }
}

pub(super) fn draw_identity_group<'a>(
    ui: &mut egui::Ui,
    title: &str,
    fields: impl Iterator<Item = &'a IdentityField>,
) {
    ui.strong(title);
    egui::Grid::new(title)
        .striped(true)
        .spacing([12.0, 5.0])
        .show(ui, |ui| {
            for field in fields {
                ui.horizontal(|ui| {
                    ui.label(&field.label);
                    if let Some(help) = field.help {
                        draw_authoring_info_icon(ui, help);
                    }
                });
                // A value still to come reads as a quiet note, with nothing to copy yet.
                if field.copyable {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&field.value).monospace())
                            .selectable(true),
                    );
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(field.value.clone());
                    }
                } else {
                    ui.weak(&field.value);
                    ui.label("");
                }
                ui.end_row();
            }
        });
}

pub(super) fn draw_donor_section_label(ui: &mut egui::Ui, label: &str, tooltip: Option<&str>) {
    draw_donor_section_label_with_warning(ui, label, tooltip, None);
}

pub(super) fn draw_donor_section_label_with_warning(
    ui: &mut egui::Ui,
    label: &str,
    tooltip: Option<&str>,
    warning: Option<&str>,
) {
    ui.horizontal(|ui| {
        // What the section holds is on the label's hover, as a tile's hint is on its name.
        let response = ui.strong(label);
        if let Some(tooltip) = tooltip {
            response.on_hover_text(tooltip);
        }
        if let Some(warning) = warning {
            draw_authoring_warning_icon(ui, warning);
        }
    });
}

pub(super) fn sandbox_perk_choice_label(perk: u16, choices: &[WeaponSandboxPerkChoice]) -> String {
    let known = match perk {
        449 => Some("Arc damage"),
        450 => Some("Solar damage"),
        451 => Some("Void damage"),
        _ => None,
    };
    if let Some(label) = known {
        return format!("{perk} · {label}");
    }
    choices
        .iter()
        .find(|choice| choice.perk_index == perk)
        .map_or_else(
            || format!("Effect {perk} · name unavailable"),
            |choice| {
                format!(
                    "{perk} · {} · {}",
                    choice.representative_name, choice.representative_type_name
                )
            },
        )
}

pub(super) fn trait_choice_label(trait_index: u16, choices: &[WeaponTraitChoice]) -> String {
    choices
        .iter()
        .find(|choice| choice.trait_index == trait_index)
        .map_or_else(
            || format!("{trait_index} · not installed"),
            |choice| {
                let name = choice.name.trim();
                if name.is_empty() {
                    format!("{trait_index} · 0x{:08X}", choice.hash)
                } else {
                    format!("{trait_index} · {name} · 0x{:08X}", choice.hash)
                }
            },
        )
}
