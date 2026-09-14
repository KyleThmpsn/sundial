//! Source-backed details shared by the ingredient preview.
use super::*;
use sundial::package_authoring::sandbox_perk::dependencies::{Behavior, DetailSection, Perk};

pub(super) fn identity(perk: &Perk) -> String {
    if let Some(behavior) = &perk.behavior {
        let names = behavior
            .effect_kinds
            .iter()
            .map(|&kind| sundial::package_authoring::sandbox_perk::nodes::effect_name(kind))
            .collect::<Vec<_>>();
        if !names.is_empty() {
            return format!("{} · Effect {}", names.join(" · "), perk.index);
        }
    }
    technical_identity(perk)
}

fn technical_identity(perk: &Perk) -> String {
    perk.action.map_or_else(
        || format!("Effect {} · No Standalone Action", perk.index),
        |tag| format!("Effect {} · Action 0x{tag:08X}", perk.index),
    )
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    behavior: &Behavior,
    data: Option<&discovery::Data>,
    index: u16,
    labels: &BTreeMap<u32, String>,
) {
    if let Some(perk) = data.and_then(|data| data.perks.perks.get(usize::from(index))) {
        ui.label(technical_identity(perk));
        ui.small(format!(
            "Perk 0x{:08X} · Runtime Key 0x{:08X}",
            perk.hash, perk.runtime_key
        ));
    }
    let mut last_group = "";
    for (section_index, section) in behavior.details.iter().enumerate() {
        if last_group != section.group {
            ui.separator();
            ui.strong(&section.group);
            last_group = &section.group;
        }
        ui.strong(section_title(&section.heading));
        for (line_index, line) in section.lines.iter().enumerate() {
            ui.push_id((section_index, line_index), |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(line.depth as f32 * 12.0);
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&line.kind).strong()).wrap());
                        draw_fields(ui, &line.fields);
                        if let Some(entry) = line.asset.and_then(|tag| {
                            data.and_then(|data| {
                                data.effects.entries.iter().find(|entry| entry.graph == tag)
                            })
                        }) {
                            ui.label(
                                labels.get(&entry.graph).cloned().unwrap_or_else(|| {
                                    entry.discovery_label_with(|_| None, |_| None)
                                }),
                            );
                            ui.small(assets::technical_name(entry));
                        }
                    });
                });
            });
        }
    }
    for note in &behavior.notes {
        if !note.starts_with("Conditions in one list are alternatives.")
            && !note.starts_with("This describes the compiled action.")
        {
            ui.label(note);
        }
    }
}

fn section_title(heading: &str) -> &str {
    match heading {
        "Starts When" => "Trigger",
        "Then" => "Actions",
        "Ends When" => "End Condition",
        "Ready Again When" => "Reactivation",
        other => other,
    }
}

fn draw_fields(ui: &mut egui::Ui, fields: &[String]) {
    let width = ui.available_width();
    egui::Grid::new("native-facts")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .striped(true)
        .show(ui, |ui| {
            for field in fields {
                let (name, value) = field.split_once(": ").unwrap_or(("", field));
                let label_width = (width * 0.43).min(185.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(label_width, 16.0),
                    egui::Layout::right_to_left(egui::Align::Min),
                    |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(name).weak()).wrap());
                    },
                );
                ui.allocate_ui(
                    egui::vec2((width - label_width - 12.0).max(60.0), 16.0),
                    |ui| {
                        ui.add(egui::Label::new(value).wrap());
                    },
                );
                ui.end_row();
            }
        });
    ui.add_space(6.0);
}

fn asset_text(text: &str, asset: Option<u32>, labels: &BTreeMap<u32, String>) -> String {
    asset
        .and_then(|tag| labels.get(&tag).map(|name| (tag, name)))
        .map_or_else(
            || text.to_owned(),
            |(tag, name)| text.replace(&format!("0x{tag:08X}"), name),
        )
}

/// Repeated action prose describes one operation, not identical parameter values.
/// Keep each native record in Technical Details and never combine trigger alternatives.
fn overview_lines(section: &DetailSection, labels: &BTreeMap<u32, String>) -> Vec<(usize, String)> {
    let actions = section.heading == "Then";
    let alternatives = !actions && section.lines.iter().filter(|line| line.depth == 0).count() > 1;
    let mut output = Vec::new();
    let mut index = 0;
    while let Some(line) = section.lines.get(index) {
        let mut count = 1;
        if actions && line.depth == 0 {
            count += section.lines[index + 1..]
                .iter()
                .take_while(|next| {
                    next.depth == 0
                        && next.kind == line.kind
                        && next.text == line.text
                        && next.asset == line.asset
                })
                .count();
        }
        let text = asset_text(&line.text, line.asset, labels);
        let text = if count > 1 {
            format!("{count} actions use this operation: {text}")
        } else if alternatives && index > 0 && line.depth == 0 {
            format!("Or {}", text.trim_end_matches('.'))
        } else {
            text
        };
        output.push((line.depth, text));
        index += count;
    }
    output
}

/// The decoded operation and activation belong above native field storage details.
pub(super) fn overview(ui: &mut egui::Ui, behavior: &Behavior, labels: &BTreeMap<u32, String>) {
    let mut last_group = "";
    let multiple_groups = behavior.details.iter().any(|section| {
        behavior
            .details
            .first()
            .is_some_and(|first| first.group != section.group)
    });
    for section in &behavior.details {
        if multiple_groups && section.group != last_group {
            ui.strong(&section.group);
            last_group = &section.group;
        }
        ui.strong(section_title(&section.heading));
        for (depth, text) in overview_lines(section, labels) {
            ui.horizontal_top(|ui| {
                ui.add_space(depth as f32 * 12.0);
                ui.add(egui::Label::new(text).wrap());
            });
        }
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_operations_keep_record_values_and_condition_alternatives() {
        use sundial::package_authoring::sandbox_perk::dependencies::DetailLine;
        let mut section = DetailSection {
            group: "Main Program".into(),
            heading: "Then".into(),
            lines: ["Value: 0.05", "Value: 0.1", "Value: 0.2"]
                .map(|field| DetailLine {
                    text: "Scale a component value.".into(),
                    kind: "Scale Component Value".into(),
                    fields: vec![field.into()],
                    depth: 0,
                    asset: None,
                })
                .to_vec(),
        };
        let original = section.clone();
        assert_eq!(
            overview_lines(&section, &BTreeMap::new()),
            vec![(
                0,
                "3 actions use this operation: Scale a component value.".into()
            )]
        );
        assert_eq!(
            section, original,
            "Technical Details must retain every record and value"
        );
        section.heading = "Starts When".into();
        let alternatives = overview_lines(&section, &BTreeMap::new());
        assert_eq!(alternatives.len(), 3);
        assert_eq!(alternatives[1].1, "Or Scale a component value");
        section.heading = "Then".into();
        section.lines[1].asset = Some(123);
        assert_eq!(
            overview_lines(&section, &BTreeMap::new()).len(),
            3,
            "Different assets must remain separate actions"
        );
    }

    #[test]
    fn named_references_keep_the_source_operation_and_unknown_tags() {
        let labels = BTreeMap::from([(0x81A6985B, "Chosen of the Warmind Emitter".into())]);
        assert_eq!(
            asset_text("Spawn 0x81A6985B once", Some(0x81A6985B), &labels),
            "Spawn Chosen of the Warmind Emitter once"
        );
        assert_eq!(
            asset_text("Spawn 0x12345678 once", Some(0x12345678), &labels),
            "Spawn 0x12345678 once"
        );
    }
}
