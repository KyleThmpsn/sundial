use super::*;
use eframe::egui;

pub(crate) fn draw(ui: &mut egui::Ui, json: &mut Value, runtime: &mut Runtime) -> bool {
    ui.heading("Dawn");
    ui.label("Runtime configuration for the detected Dawn installation.");
    ui.label(
        "Dawn reads these controls from Dawn/settings.json at startup. Player preferences and key bindings are stored in player-state.db and appear on the other Game Settings tabs.",
    );
    ui.label(format!("DLL: {}", runtime.dll_path.display()));
    if super::super::schema_version(json) != Some(6) {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Dawn runtime configuration requires JSON schema v6. Load compatible configuration before editing these controls.",
        );
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
    egui::CollapsingHeader::new("Advanced Runtime Configuration").show(ui, |ui| {
        ui.label(
            "These controls are separate from the executor choice. Missing values use Dawn's compiled defaults.",
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
        changed |= draw_spawn_hold(ui, json);
    });
    for error in settings_issues(json) {
        ui.add_space(8.0);
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    changed
}

fn draw_flag(
    ui: &mut egui::Ui,
    json: &mut Value,
    group: &str,
    (key, label, default): (&str, &str, bool),
) -> bool {
    let path = format!("{group}/{key}");
    let invalid = json.pointer(&path).is_some_and(|v| !v.is_boolean());
    let mut enabled = json
        .pointer(&path)
        .and_then(Value::as_bool)
        .unwrap_or(default);
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
                    if ui.button("Use Default").clicked() {
                        changed |= set_flag(json, group, key, default);
                    }
                });
            }
        });
    });
    changed
}

fn draw_spawn_hold(ui: &mut egui::Ui, json: &mut Value) -> bool {
    let invalid = json.pointer(CLIENT_SPAWN_HOLD_MS).is_some_and(|value| {
        value
            .as_u64()
            .is_none_or(|value| value == 0 || value > MAXIMUM_SPAWN_HOLD_MS)
    });
    let mut milliseconds = json
        .pointer(CLIENT_SPAWN_HOLD_MS)
        .and_then(Value::as_u64)
        .filter(|value| (1..=MAXIMUM_SPAWN_HOLD_MS).contains(value))
        .unwrap_or(DEFAULT_SPAWN_HOLD_MS);
    let mut changed = false;
    ui.push_id(CLIENT_SPAWN_HOLD_MS, |ui| {
        ui.add_enabled_ui(editable_group(json, CLIENT), |ui| {
            ui.horizontal(|ui| {
                ui.label("Spawn Hold (Milliseconds)");
                if ui
                    .add(egui::DragValue::new(&mut milliseconds).range(1..=MAXIMUM_SPAWN_HOLD_MS))
                    .on_hover_text(dotted(CLIENT_SPAWN_HOLD_MS))
                    .changed()
                {
                    changed |= set_unsigned(json, CLIENT, "spawn_hold_ms", milliseconds);
                }
            });
            if invalid {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Must be from 1 to {MAXIMUM_SPAWN_HOLD_MS}."),
                    );
                    if ui.button("Use Default").clicked() {
                        changed |=
                            set_unsigned(json, CLIENT, "spawn_hold_ms", DEFAULT_SPAWN_HOLD_MS);
                    }
                });
            }
        });
    });
    changed
}
