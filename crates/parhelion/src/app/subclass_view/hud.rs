use super::*;

/// The color the game gives each damage type, from its elemental palette (`80BC7198`), which the
/// inventory subclass tree reads. The stock Supers' HUD rows hold the same colors.
const DAMAGE_TYPE_COLORS: [(&str, [u8; 3]); 3] = [
    ("Arc", [133, 197, 236]),
    ("Solar", [243, 111, 33]),
    ("Void", [178, 131, 204]),
];

/// What a chosen color reads as: a damage type's name for its color, else Custom Color.
fn color_name(rgb: [u8; 3]) -> &'static str {
    DAMAGE_TYPE_COLORS
        .iter()
        .find(|(_, color)| *color == rgb)
        .map_or("Custom Color", |(name, _)| name)
}

/// The damage types' colors, then Custom Color, which keeps `kept` or starts from white.
fn color_choices(ui: &mut egui::Ui, color: &mut Option<[u8; 3]>, kept: Option<[u8; 3]>) {
    for (name, rgb) in DAMAGE_TYPE_COLORS {
        ui.selectable_value(color, Some(rgb), name);
    }
    let custom = color.is_some_and(|rgb| color_name(rgb) == "Custom Color");
    if ui.selectable_label(custom, "Custom Color").clicked() {
        *color = Some(kept.unwrap_or([255; 3]));
    }
}

/// A color's hex code in a field, so a code can be typed or pasted. A complete `#RRGGBB`, or
/// `#RGB`, with or without the `#`, sets the color. Away from the field it reads the color's code.
fn hex_field(ui: &mut egui::Ui, salt: &str, name: &str, rgb: &mut [u8; 3]) {
    let id = ui.make_persistent_id(salt);
    let code = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    let mut text = ui.data(|data| data.get_temp::<String>(id)).unwrap_or(code);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap("#DDDDDD".to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    });
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(id.with("field"))
            .font(egui::TextStyle::Monospace)
            .desired_width((width + 10.0).ceil()),
    );
    let response = style::named_control(response, format!("{name} Hex Code"));
    if response.changed()
        && let Some(parsed) = parse_hex(&text)
    {
        *rgb = parsed;
    }
    // Asked before the data lock is held: `has_focus` locks the context itself.
    let focused = response.has_focus();
    ui.data_mut(|data| {
        if focused {
            data.insert_temp(id, text);
        } else {
            data.remove::<String>(id);
        }
    });
}

/// The color a hex code names: six digits, or three that each stand for two.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let digits = match digits.len() {
        6 => digits.to_owned(),
        3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
        _ => return None,
    };
    let [_, red, green, blue] = u32::from_str_radix(&digits, 16).ok()?.to_be_bytes();
    Some([red, green, blue])
}

impl PackageAuthoringApp {
    pub(super) fn draw_subclass_hud(&mut self, ui: &mut egui::Ui) {
        let current = self
            .recipe
            .overrides
            .subclass_abilities
            .as_ref()
            .and_then(|abilities| abilities.hud_color);
        let (label, reset) = style::stock_field_name(
            ui,
            "Subclass Color",
            "Colors the tree nodes, ability tiles and charge bars. Each ability can override it.",
            current.map(|_| "Donor Colors"),
        );
        let mut color = if reset { None } else { current };
        egui::ComboBox::from_id_salt("subclass_hud_color")
            .selected_text(color.map_or("Donor Colors", color_name))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                workbench_style(ui);
                ui.selectable_value(&mut color, None, "Donor Colors");
                color_choices(ui, &mut color, current);
            })
            .response
            .labelled_by(label.id);
        if let Some(rgb) = &mut color {
            ui.horizontal(|ui| {
                let response = egui::color_picker::color_edit_button_srgb(ui, rgb);
                style::named_control(response, "Subclass Color");
                hex_field(ui, "subclass_hud_hex", "Subclass Color", rgb);
            });
        }
        if color != current {
            let mut abilities = self
                .recipe
                .overrides
                .subclass_abilities
                .take()
                .unwrap_or_default();
            abilities.hud_color = color;
            self.recipe.overrides.subclass_abilities = (!abilities.is_empty()).then_some(abilities);
        }
    }

    /// The per-entry choice keeps inherited and explicit colors distinct, including when
    /// an explicit color happens to match the current subclass theme.
    pub(super) fn draw_ability_color(
        &self,
        ui: &mut egui::Ui,
        current: Option<[u8; 3]>,
    ) -> Option<Option<[u8; 3]>> {
        self.draw_hud_color(
            ui,
            current,
            "Color",
            "Ability Color",
            "Colors its tree node and its HUD tile where the game provides one.",
        )
    }

    pub(super) fn draw_attached_color(
        &self,
        ui: &mut egui::Ui,
        current: Option<[u8; 3]>,
    ) -> Option<Option<[u8; 3]>> {
        self.draw_hud_color(
            ui,
            current,
            "HUD Color",
            "Attached Ability HUD Color",
            "Colors the attached ability's HUD tile.",
        )
    }

    fn draw_hud_color(
        &self,
        ui: &mut egui::Ui,
        current: Option<[u8; 3]>,
        label: &str,
        name: &str,
        hint: &str,
    ) -> Option<Option<[u8; 3]>> {
        let inherited = self
            .recipe
            .overrides
            .subclass_abilities
            .as_ref()
            .and_then(|abilities| abilities.hud_color);
        let mut color = current;
        let (_, reset) = detail::field(ui, label, current.is_some(), |ui| {
            ui.horizontal_wrapped(|ui| {
                let response = egui::ComboBox::from_id_salt("ability_color_source")
                    .selected_text(color.map_or("Subclass Color", color_name))
                    .show_ui(ui, |ui| {
                        workbench_style(ui);
                        ui.selectable_value(&mut color, None, "Subclass Color");
                        color_choices(ui, &mut color, current.or(inherited));
                    });
                style::named_control(response.response, format!("{name} Source"));
                if let Some(rgb) = &mut color {
                    let response = egui::color_picker::color_edit_button_srgb(ui, rgb);
                    style::named_control(response, name);
                    hex_field(ui, "ability_color_hex", name, rgb);
                } else if let Some(mut rgb) = inherited {
                    ui.add_enabled_ui(false, |ui| {
                        egui::color_picker::color_edit_button_srgb(ui, &mut rgb);
                    });
                    ui.label(quiet(
                        ui,
                        format!("#{:02X}{:02X}{:02X} · Inherited", rgb[0], rgb[1], rgb[2]),
                    ));
                }
            });
            ui.label(quiet(ui, hint));
        });
        if reset {
            color = None;
        }
        (color != current).then_some(color)
    }
}
