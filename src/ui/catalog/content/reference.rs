//! Explain native links without mistaking a contained path for its owner's name.
use super::*;

pub(super) fn resource_name(names: &BTreeMap<u32, Vec<String>>, tag: u32) -> Option<&str> {
    names
        .get(&tag)?
        .iter()
        .find(|name| !name.trim().is_empty())
        .map(String::as_str)
}

pub(super) fn type_name(class: u32) -> &'static str {
    crate::weapon_runtime::native_type_name(class).unwrap_or("Type Not Identified")
}

pub(super) fn source(
    ui: &mut egui::Ui,
    names: &BTreeMap<u32, Vec<String>>,
    tag: u32,
) -> Option<navigation::Destination> {
    ui.strong("Containing Resource");
    resource_link(
        ui,
        resource_name(names, tag).unwrap_or("Unnamed Resource"),
        tag,
    )
}

pub(super) fn resource_link(
    ui: &mut egui::Ui,
    name: &str,
    tag: u32,
) -> Option<navigation::Destination> {
    let title = if name == "Unnamed Resource" {
        format!("Unnamed Resource · 0x{tag:08X}")
    } else {
        name.to_owned()
    };
    let response = ui
        .link(egui::RichText::new(title).underline())
        .on_hover_text(format!("Open Resource · 0x{tag:08X}"));
    response.context_menu(|ui| {
        if name != "Unnamed Resource" && ui.button("Copy Path").clicked() {
            ui.ctx().copy_text(name.to_owned());
            ui.close_menu();
        }
        copy_tag(ui, "Copy Resource Tag", tag);
    });
    response
        .clicked()
        .then_some(navigation::Destination::Resource(tag))
}

pub(super) fn type_link(
    ui: &mut egui::Ui,
    label: &str,
    class: u32,
) -> Option<navigation::Destination> {
    let title = format!("{label}: {}", type_name(class));
    if class == 0 {
        ui.label(title);
        None
    } else {
        ui.link(egui::RichText::new(title).underline())
            .on_hover_text(format!(
                "Show resources with this engine data type · 0x{class:08X}"
            ))
            .clicked()
            .then_some(navigation::Destination::Class(class))
    }
}

pub(super) fn copy_tag(ui: &mut egui::Ui, label: &str, tag: u32) {
    if ui
        .button(label)
        .on_hover_text("Copy the resource's unique package identifier.")
        .clicked()
    {
        ui.ctx().copy_text(format!("0x{tag:08X}"));
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    reference: &tft::Reference,
    names: &BTreeMap<u32, Vec<String>>,
) -> Option<navigation::Destination> {
    ui.push_id((reference.source, reference.offset, reference.target), |ui| {
        let mut destination = None;
        ui.strong("Referenced Asset");
        destination = resource_link(ui, &reference.path, reference.target).or(destination);
        destination = type_link(ui, "Asset Type", reference.target_class).or(destination);
        ui.add_space(4.0);
        destination = source(ui, names, reference.source).or(destination);
        destination = type_link(ui, "Resource Type", reference.source_class).or(destination);
        ui.label("This resource contains a link to the asset above.");
        egui::CollapsingHeader::new("Technical Details").show(ui, |ui| {
            ui.label("Tags identify individual package resources. Class identifiers describe their data types.");
            for (label, tag) in [
                ("Source Tag", reference.source),
                ("Target Tag", reference.target),
                ("Source Class", reference.source_class),
                ("Target Class", reference.target_class),
            ] {
                ui.monospace(format!("{label}: 0x{tag:08X}"));
            }
            ui.label(format!("Link Location: {} bytes from the start of the containing resource (0x{:X})", reference.offset, reference.offset));
        });
        ui.horizontal_wrapped(|ui| {
            copy_tag(ui, "Copy Source Tag", reference.source);
            copy_tag(ui, "Copy Target Tag", reference.target);
        });
        ui.add_space(6.0);
        destination
    }).inner
}

/// A linked resource: the row itself opens it.
pub(super) fn resource_row(ui: &mut egui::Ui, name: &str, detail: &str) -> bool {
    crate::investment::draw_asset_choice_row_plain(ui, name, detail, false)
        .on_hover_text(format!("Open {name}"))
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containing_resource_names_come_from_incoming_links_not_contained_paths() {
        let reference = tft::Reference {
            source: 1,
            source_class: 0x8080_40B5,
            offset: 32,
            target: 2,
            target_class: 0x8080_3B73,
            path: "native/Projectile.tft".into(),
        };
        let mut index = tft::Index {
            references: vec![reference.clone()],
            paths: vec![tft::ContentPath {
                source: 1,
                offset: 24,
                path: reference.path.clone(),
            }],
            ..Default::default()
        };
        assert_eq!(resource_name(&index.names(), 1), None);
        assert_eq!(
            resource_name(&index.names(), 2),
            Some("native/Projectile.tft")
        );
        index.references.push(tft::Reference {
            source: 3,
            target: 1,
            path: "native/PerkAction.tft".into(),
            ..reference
        });
        assert_eq!(
            resource_name(&index.names(), 1),
            Some("native/PerkAction.tft")
        );
    }

    #[test]
    fn types_use_verified_roles_and_leave_unknown_classes_explicit() {
        assert_eq!(type_name(0x8080_40B5), "Perk Action");
        assert_eq!(type_name(0x8080_3B73), "Projectile Movement");
        assert_eq!(type_name(0), "Type Not Identified");
        assert_eq!(type_name(0xDEAD_BEEF), "Type Not Identified");
    }
}
