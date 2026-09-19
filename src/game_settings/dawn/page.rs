use super::*;
use eframe::egui;

pub(crate) fn draw(ui: &mut egui::Ui, json: &mut Value, runtime: &mut Runtime) -> bool {
    ui.heading("Dawn");
    ui.label("Settings for the detected Dawn runtime.");
    ui.label(format!("DLL: {}", runtime.dll_path.display()));
    if super::super::schema_version(json) != Some(6) {
        ui.colored_label(ui.visuals().error_fg_color, "This runtime requires schema v6. Load compatible settings before editing these controls.");
        return false;
    }
    ui.add_space(10.0);
    let mut changed = draw_flag(ui, json, OMEGA, OMEGA_FLAGS[0]);
    ui.label(if executor_enabled(json) {
        "Omega uses its Lua mission executor on the next mission run."
    } else {
        "Omega uses its legacy executor. A missing setting also means off."
    });
    ui.label("Executor changes apply on the next mission run. Restart Destiny 2 after editing the script.");
    ui.add_space(6.0);
    ui.label(format!("Omega Script: {}", runtime.script_path.display()));
    if ui.button("Recheck Script").clicked() {
        *runtime = Runtime::inspect(&runtime.dll_path);
    }
    if let Some(problem) = &runtime.script_problem {
        ui.colored_label(ui.visuals().warn_fg_color, problem);
        ui.label(if executor_enabled(json) {
            "Restore omega.lua at this path or turn off Omega Lua Executor before saving."
        } else {
            "Restore omega.lua at this path before enabling Omega Lua Executor."
        });
    } else {
        ui.label(
            "Script is readable and nonempty. Dawn validates the Lua script when Omega starts.",
        );
    }
    ui.add_space(10.0);
    egui::CollapsingHeader::new("Advanced Experiments").show(ui, |ui| {
        ui.label(
            "These flags are separate from the executor choice. Missing flags default to off.",
        );
        ui.strong("Omega");
        for flag in &OMEGA_FLAGS[1..] {
            changed |= draw_flag(ui, json, OMEGA, *flag);
        }
        ui.add_space(8.0);
        ui.strong("Client");
        for flag in CLIENT_FLAGS {
            changed |= draw_flag(ui, json, CLIENT, flag);
        }
    });
    for error in settings_issues(json) {
        ui.add_space(8.0);
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    changed
}

fn draw_flag(ui: &mut egui::Ui, json: &mut Value, group: &str, (key, label): (&str, &str)) -> bool {
    let path = format!("{group}/{key}");
    let invalid = json.pointer(&path).is_some_and(|v| !v.is_boolean());
    let mut enabled = json
        .pointer(&path)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut changed = false;
    ui.push_id(&path, |ui| {
        ui.add_enabled_ui(editable_group(json, group), |ui| {
            if ui
                .checkbox(&mut enabled, label)
                .on_hover_text(dotted(&path))
                .changed()
            {
                changed |= set_flag(json, group, key, enabled);
            }
            if invalid {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, "Must be true or false.");
                    if ui.button("Set Off").clicked() {
                        changed |= set_flag(json, group, key, false);
                    }
                });
            }
        });
    });
    changed
}
