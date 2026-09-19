//! Sunrise settings organized into focused subtabs.

use super::{
    Capabilities,
    fields::{FIELDS, Field, Kind},
    optional_value, set_field, write_value,
};
use eframe::egui;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Profile,
    Presentation,
    Activities,
    Destinations,
    Networking,
    Logging,
    Characters,
    Entitlements,
    Advanced,
}

impl Tab {
    const ALL: [Self; 9] = [
        Self::Profile,
        Self::Presentation,
        Self::Activities,
        Self::Destinations,
        Self::Networking,
        Self::Logging,
        Self::Characters,
        Self::Entitlements,
        Self::Advanced,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Presentation => "Presentation",
            Self::Activities => "Activities",
            Self::Destinations => "Destinations",
            Self::Networking => "Networking",
            Self::Logging => "Logging",
            Self::Characters => "Characters",
            Self::Entitlements => "Entitlements",
            Self::Advanced => "Advanced",
        }
    }

    /// A tab whose every control the installed runtime retired is hidden, not shown empty.
    /// Destinations keeps its default-destination half on every runtime, and the account-backed
    /// tabs carry no runtime fields at all.
    fn available(self, capabilities: Capabilities) -> bool {
        match self {
            Self::Profile => group_populated("Profile & Catalysts", capabilities),
            Self::Presentation => group_populated("Presentation", capabilities),
            Self::Activities => {
                group_populated("Activities & Scripting", capabilities)
                    || group_populated("Server Activation", capabilities)
            }
            Self::Networking => {
                group_populated("External Server", capabilities)
                    || group_populated("Server Networking", capabilities)
            }
            Self::Logging => group_populated("Logging", capabilities),
            Self::Advanced => group_populated("Advanced Runtime", capabilities),
            Self::Destinations | Self::Characters | Self::Entitlements => true,
        }
    }
}

/// True while at least one control in the group survives on the installed runtime.
fn group_populated(group: &str, capabilities: Capabilities) -> bool {
    FIELDS
        .iter()
        .chain(super::services::FIELDS)
        .any(|field| field.group == group && capabilities.supports(field.path))
}

pub(in crate::game_settings) fn draw(
    ui: &mut egui::Ui,
    document: &mut Value,
    account_available: bool,
    capabilities: Capabilities,
) -> bool {
    if !super::available(document) {
        return false;
    }
    let id = ui.make_persistent_id("sunrise_settings_tab");
    let mut tab = ui
        .data_mut(|data| data.get_temp::<Tab>(id))
        .unwrap_or_default();
    if !tab.available(capabilities) {
        tab = Tab::ALL
            .into_iter()
            .find(|candidate| candidate.available(capabilities))
            .unwrap_or_default();
    }
    ui.horizontal_wrapped(|ui| {
        for candidate in Tab::ALL.into_iter().filter(|c| c.available(capabilities)) {
            ui.selectable_value(&mut tab, candidate, candidate.label());
        }
    });
    ui.data_mut(|data| data.insert_temp(id, tab));
    ui.separator();
    ui.add_space(8.0);
    let changed = match tab {
        Tab::Profile => draw_group(
            ui,
            document,
            "Profile & Catalysts",
            account_available,
            capabilities,
        ),
        Tab::Presentation => draw_group(
            ui,
            document,
            "Presentation",
            account_available,
            capabilities,
        ),
        Tab::Activities => {
            draw_group(
                ui,
                document,
                "Activities & Scripting",
                account_available,
                capabilities,
            ) | draw_group(
                ui,
                document,
                "Server Activation",
                account_available,
                capabilities,
            )
        }
        Tab::Destinations => super::activity_page::draw(ui, document, capabilities),
        Tab::Networking => {
            ui.strong("External Server");
            let mut changed = draw_group(
                ui,
                document,
                "External Server",
                account_available,
                capabilities,
            );
            ui.add_space(12.0);
            ui.strong("Server Networking");
            changed |= draw_group(
                ui,
                document,
                "Server Networking",
                account_available,
                capabilities,
            );
            changed
        }
        Tab::Logging => draw_group(ui, document, "Logging", account_available, capabilities),
        Tab::Characters => super::character_page::draw(ui, document, account_available),
        Tab::Entitlements if account_available => super::entitlements::draw(ui, document),
        Tab::Entitlements => {
            ui.label("The active account is unavailable.");
            false
        }
        Tab::Advanced => draw_group(
            ui,
            document,
            "Advanced Runtime",
            account_available,
            capabilities,
        ),
    };
    if let Err(error) = super::services::validate(document, account_available) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    changed
}

fn draw_group(
    ui: &mut egui::Ui,
    document: &mut Value,
    group: &str,
    account_available: bool,
    capabilities: Capabilities,
) -> bool {
    let mut changed = false;
    for &field in FIELDS
        .iter()
        .chain(super::services::FIELDS)
        .filter(|field| field.group == group && capabilities.supports(field.path))
    {
        ui.push_id(field.path, |ui| {
            ui.add_enabled_ui(account_available || !field.account_owned(), |ui| {
                changed |= draw_field(ui, document, field, account_available);
            })
            .response
            .on_disabled_hover_text("The active account is unavailable.");
        });
    }
    changed
}

pub(super) fn draw_field(
    ui: &mut egui::Ui,
    document: &mut Value,
    field: Field,
    account_available: bool,
) -> bool {
    let field = field.for_document(document);
    let mut value = match optional_value(document, field.path) {
        Ok(value) => value.cloned().unwrap_or_else(|| field.default_value()),
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            return false;
        }
    };
    let text_field = matches!(field.kind, Kind::Text(..) | Kind::Ipv4(_));
    if let Err(error) = field.validate(&value) {
        ui.colored_label(ui.visuals().error_fg_color, error);
        if !text_field || !value.is_string() {
            return false;
        }
    }
    let text_edit_id = ui.make_persistent_id(("runtime-field", field.path));
    let mut requested = false;
    ui.horizontal_wrapped(|ui| {
        match field.kind {
            Kind::Bool(_) => {
                let mut enabled = value.as_bool().unwrap_or(false);
                requested = ui
                    .checkbox(&mut enabled, field.label)
                    .on_hover_text(field.help)
                    .changed();
                if requested {
                    value = Value::Bool(enabled);
                }
            }
            Kind::UInt(_, min, max) => {
                ui.label(field.label);
                let mut number = value.as_u64().unwrap_or(min);
                requested = ui
                    .add(egui::DragValue::new(&mut number).range(min..=max))
                    .changed();
                value = Value::from(number);
            }
            Kind::Text(_, _, _) | Kind::Ipv4(_) => {
                ui.label(field.label);
                let mut text = value.as_str().unwrap_or_default().to_owned();
                requested = ui
                    .add(egui::TextEdit::singleline(&mut text).id(text_edit_id))
                    .changed();
                value = Value::from(text);
            }
            Kind::Choice(choices) => {
                ui.label(field.label).on_hover_text(field.help);
                let mut selected = value.as_str().unwrap_or(choices[0]).to_owned();
                egui::ComboBox::from_id_salt(field.path)
                    .selected_text(&selected)
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            requested |= ui
                                .selectable_value(&mut selected, (*choice).to_owned(), *choice)
                                .changed();
                        }
                    });
                if requested {
                    value = Value::from(selected);
                }
            }
        }
        crate::ui_help::info(ui, field.help);
        if document.pointer(field.path).is_none() {
            ui.label(egui::RichText::new("Using Default").small().weak())
                .on_hover_text("No value is saved for this setting, so Sunrise uses its built-in default. Change this control to save your own value.");
        }
    });
    let error_id = ui.make_persistent_id(("runtime-error", field.path));
    if requested {
        // Keep incomplete text in the document so dirty state, undo and reload see it.
        // The save boundary validates the complete value; no hidden draft can outlive a file.
        let result = if text_field {
            write_value(document, field.path, value).map(|()| true)
        } else {
            set_field(document, field.path, value, account_available)
        };
        match result {
            Ok(changed) => {
                ui.data_mut(|data| data.remove::<String>(error_id));
                return changed;
            }
            Err(error) => ui.data_mut(|data| data.insert_temp(error_id, error)),
        }
    }
    if let Some(error) = ui.data_mut(|data| data.get_temp::<String>(error_id)) {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
    false
}
