//! Offers to turn a stock action into an editable program.
//!
//! The reading of the action itself is drawn by the program canvas in
//! `workbench/canvas.rs`, so a stock effect and an authored program share one layout.
use sundial::package_authoring::sandbox_perk::program::{Program, decompile::Difference};

/// Offers to turn the stock action into an editable program. Returns the program on request.
///
/// Program authoring is an experimental control, so the button follows that preference. The
/// explanation of whether a conversion would be exact is shown either way.
pub(super) fn draw_conversion(
    ui: &mut egui::Ui,
    program: Option<&Result<Program, String>>,
    fidelity: &Result<Vec<Difference>, String>,
    name: &str,
    experimental: bool,
) -> Option<Program> {
    let program = program?;
    let mut requested = None;
    egui::CollapsingHeader::new("Edit as a Program")
        .id_salt("private-perk-conversion")
        .default_open(true)
        .show(ui, |ui| match program {
            Err(reason) => {
                ui.weak(format!("Not convertible yet. {reason}"));
                ui.small("Parameter edits below still apply to the stock action.");
            }
            Ok(program) => {
                requested = draw_convertible(ui, program, fidelity, name, experimental);
            }
        });
    requested
}

fn draw_convertible(
    ui: &mut egui::Ui,
    program: &Program,
    fidelity: &Result<Vec<Difference>, String>,
    name: &str,
    experimental: bool,
) -> Option<Program> {
    let label = match fidelity {
        Ok(differences) if differences.is_empty() => {
            ui.label("This perk fits the program model. The native comparison found no differences in the checked settings.");
            "Convert to Editable Program"
        }
        Ok(differences) => {
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
            "Convert Anyway"
        }
        Err(error) => {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("The program model fits, but it could not be compiled: {error}"),
            );
            return None;
        }
    };
    ui.small(
        "A converted effect is compiled fresh. Stock parameter edits and projectile swaps become part of the program.",
    );
    let button = ui.add_enabled(experimental, egui::Button::new(label));
    if button
        .on_disabled_hover_text("Turn on Experimental Features in Preferences to edit programs.")
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
