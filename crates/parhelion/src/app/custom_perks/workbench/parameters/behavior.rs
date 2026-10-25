//! Offers to turn a stock action into an editable program when the conversion is not exact.
//! An exact one needs no offer, since the effect's card edits it in place.
//!
//! The reading of the action itself is drawn by the program canvas in
//! `workbench/canvas.rs`, so a stock effect and an authored program share one layout.
use sundial::package_authoring::sandbox_perk::program::{Program, decompile::Difference};

/// Offers to turn the stock action into an editable program, listing the native settings it
/// would leave behind. Returns the program on request.
///
/// The button is disabled while `blocked` holds a reason, which its hover text shows.
pub(super) fn draw_conversion(
    ui: &mut egui::Ui,
    program: &Program,
    fidelity: &Result<Vec<Difference>, String>,
    name: &str,
    blocked: Option<&str>,
) -> Option<Program> {
    let mut requested = None;
    egui::CollapsingHeader::new("Edit as a Program")
        .id_salt("private-perk-conversion")
        .default_open(true)
        .show(ui, |ui| {
            requested = draw_convertible(ui, program, fidelity, name, blocked);
        });
    requested
}

fn draw_convertible(
    ui: &mut egui::Ui,
    program: &Program,
    fidelity: &Result<Vec<Difference>, String>,
    name: &str,
    blocked: Option<&str>,
) -> Option<Program> {
    let differences = match fidelity {
        Ok(differences) => differences,
        Err(error) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Conversion could not be checked. {error}"),
            );
            return None;
        }
    };
    ui.label(format!(
        "The program model changes or omits {} checked native setting{}.",
        differences.len(),
        if differences.len() == 1 { "" } else { "s" }
    ));
    egui::CollapsingHeader::new(egui::RichText::new("Settings Left Behind").small())
        .id_salt("conversion-differences")
        .show(ui, |ui| {
            for difference in differences {
                ui.small(format!(
                    "{} +0x{:X}: {} becomes {}",
                    difference.node,
                    difference.offset,
                    hex(&difference.stock),
                    hex(&difference.compiled)
                ));
            }
        });
    let button = ui.add_enabled(blocked.is_none(), egui::Button::new("Convert Anyway"));
    if button
        .on_disabled_hover_text(blocked.unwrap_or_default())
        .clicked()
    {
        let mut program = program.clone();
        program.name = name.to_owned();
        return Some(program);
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
