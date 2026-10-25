//! A perk action read in plain words: its headline, then its trigger, actions, ending and
//! reactivation with their native values. Shared by every view that shows a stock perk.
use crate::sandbox_perk::{dependencies, program::Program};
use eframe::egui;

/// The headline, the reading under it and the notes that change how it reads.
pub fn draw(ui: &mut egui::Ui, behavior: &dependencies::Behavior) {
    ui.label(&behavior.headline);
    if behavior.details.is_empty() {
        if let Some(program) = &behavior.program {
            program_lines(ui, program);
        }
    } else {
        detail_sections(ui, &behavior.details);
    }
    for note in &behavior.notes {
        if !note.starts_with("Conditions in one list are alternatives.")
            && !note.starts_with("This describes the compiled action.")
        {
            ui.weak(note);
        }
    }
}

/// The native reading: trigger, actions, end and rearm conditions with their values.
fn detail_sections(ui: &mut egui::Ui, sections: &[dependencies::DetailSection]) {
    let groups = sections
        .iter()
        .map(|section| section.group.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut last_group = "";
    for (section_index, section) in sections.iter().enumerate() {
        if groups.len() > 1 && last_group != section.group {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&section.group).strong().small());
            last_group = &section.group;
        }
        let title = match section.heading.as_str() {
            "Starts When" => "Trigger",
            "Then" => "Actions",
            "Ends When" => "End Condition",
            "Ready Again When" => "Reactivation",
            other => other,
        };
        ui.add_space(2.0);
        ui.weak(title);
        for (line_index, line) in section.lines.iter().enumerate() {
            ui.push_id((section_index, line_index), |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(12.0 + line.depth as f32 * 12.0);
                    ui.vertical(|ui| {
                        let name = if line.kind.is_empty() {
                            line.text.as_str()
                        } else {
                            line.kind.as_str()
                        };
                        ui.add(egui::Label::new(name).wrap());
                        for field in &line.fields {
                            ui.add(egui::Label::new(egui::RichText::new(field).weak()).wrap());
                        }
                    });
                });
            });
        }
    }
}

/// Trigger, timing and the recovered actions, one line each. Used when the index predates
/// the native reading.
fn program_lines(ui: &mut egui::Ui, program: &Program) {
    let seconds = |ms: u32| {
        let seconds = f64::from(ms) / 1000.0;
        if seconds.fract() == 0.0 {
            format!("{seconds:.0} s")
        } else {
            format!("{seconds:.1} s")
        }
    };
    let mut timing = vec![program.trigger.label().to_owned()];
    if program.duration_ms > 0 {
        timing.push(seconds(program.duration_ms));
    }
    if program.cooldown_ms > 0 {
        timing.push(format!("{} cooldown", seconds(program.cooldown_ms)));
    }
    if program.chance_permyriad > 0 && program.chance_permyriad < 10_000 {
        timing.push(format!(
            "{}% chance",
            f64::from(program.chance_permyriad) / 100.0
        ));
    }
    ui.label(timing.join(" · "));
    for action in &program.actions {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(action.label());
        });
    }
}
