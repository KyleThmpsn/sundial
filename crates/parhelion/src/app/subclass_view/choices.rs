//! Every stock choice for the selected ability, node or attunement: a line to each stock
//! subclass, its choices beside its name, the subclasses grouped by class with the base's first.
use super::*;

/// One stock choice in the detail panel.
struct Choice<K> {
    key: K,
    label: String,
    icon: Option<egui::TextureHandle>,
    /// Its tooltip's line under its name, and its description.
    subtitle: String,
    description: Option<String>,
    current: bool,
    /// Another place already has it.
    taken: bool,
    /// It starts a group of its subclass's choices, such as one path's nodes.
    starts_group: bool,
}

/// Stock subclasses grouped by class, the base's class first.
pub(super) fn class_groups(
    subclasses: &[SubclassSummary],
    class_type: u8,
) -> Vec<(u8, Vec<&SubclassSummary>)> {
    let mut options = subclasses.iter().collect::<Vec<_>>();
    options.sort_by_key(|option| (option.class_type != class_type, option.class_type));
    let mut groups: Vec<(u8, Vec<&SubclassSummary>)> = Vec::new();
    for subclass in options {
        match groups.last_mut() {
            Some((class, members)) if *class == subclass.class_type => members.push(subclass),
            _ => groups.push((subclass.class_type, vec![subclass])),
        }
    }
    groups
}

/// Every stock subclass's choices, a line to each subclass with its choices beside its name.
/// `search` keeps the subclasses whose names hold it, and elsewhere the choices whose names do.
/// Returns the choice clicked.
fn draw_choices<K: Copy>(
    ui: &mut egui::Ui,
    groups: &[(u8, Vec<&SubclassSummary>)],
    search: &str,
    choices: impl Fn(&SubclassSummary) -> Vec<Choice<K>>,
) -> Option<K> {
    let needle = search.trim().to_lowercase();
    let mut picked = None;
    let mut shown = false;
    for (class_type, members) in groups {
        let lines = members
            .iter()
            .filter_map(|subclass| {
                let whole = needle.is_empty() || subclass.name.to_lowercase().contains(&needle);
                let kept = choices(subclass)
                    .into_iter()
                    .filter(|choice| whole || choice.label.to_lowercase().contains(&needle))
                    .collect::<Vec<_>>();
                (!kept.is_empty()).then_some((*subclass, kept))
            })
            .collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }
        shown = true;
        ui.add_space(6.0);
        ui.label(quiet(
            ui,
            gear_view::class_label(*class_type).unwrap_or("Other"),
        ));
        for (subclass, choices) in lines {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(LABEL_WIDTH, 22.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        // A column of its own, so every subclass's choices start at one edge.
                        ui.set_min_width(LABEL_WIDTH);
                        ui.add(egui::Label::new(subclass.name.as_str()).truncate())
                    },
                );
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (index, choice) in choices.into_iter().enumerate() {
                        if choice.starts_group && index > 0 {
                            ui.add_space(10.0);
                        }
                        let button = match &choice.icon {
                            Some(icon) => egui::Button::image_and_text(
                                egui::Image::new(icon)
                                    .fit_to_exact_size(egui::Vec2::splat(CHOICE_ICON)),
                                choice.label.as_str(),
                            ),
                            None => egui::Button::new(choice.label.as_str()),
                        };
                        let response =
                            ui.add_enabled(!choice.taken, button.selected(choice.current));
                        let response = if choice.taken {
                            response.on_disabled_hover_text("Already Chosen")
                        } else {
                            response.on_hover_ui(|ui| {
                                draw_display_tooltip(
                                    ui,
                                    DisplayTooltip {
                                        icon: choice.icon.as_ref(),
                                        name: &choice.label,
                                        subtitle: Some(&choice.subtitle),
                                        description: choice.description.as_deref(),
                                    },
                                );
                            })
                        };
                        if response.clicked() && !choice.current {
                            picked = Some(choice.key);
                        }
                    }
                });
            });
        }
    }
    if !shown {
        ui.label(quiet(ui, "No Matches"));
    }
    picked
}

impl PackageAuthoringApp {
    /// Every stock ability for the slot of `entry`. Returns the one picked.
    pub(super) fn draw_ability_choices(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        entry: u8,
        search: &str,
    ) -> Option<(u32, u8)> {
        let slot = AbilitySlot::of_entry(entry)?;
        let entries = slot.entries();
        let current = source_of(abilities, base.hash, Place::Ability(entry));
        // A stock ability offered twice in one slot would show twice, unless either is authored.
        let authored = |other: u8| !abilities.edits(base.hash, Place::Ability(other)).is_empty();
        let chosen = if authored(entry) {
            Vec::new()
        } else {
            entries
                .iter()
                .filter(|&&other| other != entry && !authored(other))
                .filter_map(|&other| {
                    let (source, source_entry) =
                        source_of(abilities, base.hash, Place::Ability(other));
                    find_subclass(&self.subclasses, source)
                        .map(|source| entry_name(source, source_entry).to_owned())
                })
                .collect::<Vec<_>>()
        };
        let ctx = ui.ctx().clone();
        draw_choices(
            ui,
            &class_groups(&self.subclasses, base.class_type),
            search,
            |option| {
                entries
                    .iter()
                    .map(|&option_entry| {
                        let label = entry_name(option, option_entry).to_owned();
                        Choice {
                            key: (option.hash, option_entry),
                            icon: self.entry_icon(&ctx, Some(option), option_entry),
                            subtitle: format!("{} · {}", slot.label(), option.name),
                            description: self.entry_description(Some(option), option_entry),
                            current: (option.hash, option_entry) == current,
                            taken: chosen.contains(&label),
                            starts_group: false,
                            label,
                        }
                    })
                    .collect()
            },
        )
    }

    /// Every stock node that fits `position` of `path`: another path's lead for the lead, any
    /// other node for the rest. Returns the one picked.
    pub(super) fn draw_node_choices(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        (path, position): (AttunementPath, u8),
        search: &str,
    ) -> Option<(u32, AttunementPath, u8)> {
        let node = abilities.node(base.hash, path, position);
        let lead = position == layout::LEAD_NODE;
        // The first node a path offers here, which starts its group.
        let first = if lead {
            layout::LEAD_NODE
        } else {
            layout::LEAD_NODE + 1
        };
        let ctx = ui.ctx().clone();
        draw_choices(
            ui,
            &class_groups(&self.subclasses, base.class_type),
            search,
            |option| {
                DISPLAY_PATHS
                    .into_iter()
                    .filter(|option_path| !lead || option_path.fits(path))
                    .flat_map(|option_path| {
                        (0..layout::PATH_NODES)
                            .filter(move |option_position| {
                                (*option_position == layout::LEAD_NODE) == lead
                            })
                            .map(move |option_position| (option_path, option_position))
                    })
                    .map(|(option_path, option_position)| {
                        let option_entry = option_path.entries()[usize::from(option_position)];
                        Choice {
                            key: (option.hash, option_path, option_position),
                            label: entry_name(option, option_entry).to_owned(),
                            icon: self.entry_icon(&ctx, Some(option), option_entry),
                            subtitle: format!(
                                "{}, Node {} · {}",
                                attunement_name(option, option_path),
                                option_position + 1,
                                option.name
                            ),
                            description: self.entry_description(Some(option), option_entry),
                            current: (option.hash, option_path, option_position)
                                == (node.source, node.source_path, node.source_position),
                            taken: false,
                            // Each path's nodes read as one group.
                            starts_group: option_position == first,
                        }
                    })
                    .collect()
            },
        )
    }

    /// Every stock attunement that fits `path`'s place. Returns the one picked.
    pub(super) fn draw_attunement_choices(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        path: AttunementPath,
        search: &str,
    ) -> Option<(u32, AttunementPath)> {
        let current = abilities.attunement_source(base.hash, path);
        let chosen = AttunementPath::ALL
            .into_iter()
            .filter(|other| *other != path)
            .map(|other| abilities.attunement_source(base.hash, other))
            .collect::<Vec<_>>();
        draw_choices(
            ui,
            &class_groups(&self.subclasses, base.class_type),
            search,
            |option| {
                DISPLAY_PATHS
                    .into_iter()
                    .filter(|option_path| option_path.fits(path))
                    .map(|option_path| Choice {
                        key: (option.hash, option_path),
                        label: attunement_name(option, option_path).to_owned(),
                        icon: None,
                        subtitle: format!("{} Attunement · {}", option_path.label(), option.name),
                        description: Some(
                            option_path
                                .entries()
                                .iter()
                                .map(|&entry| entry_name(option, entry))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        ),
                        current: (option.hash, option_path) == current,
                        taken: chosen.contains(&(option.hash, option_path)),
                        starts_group: false,
                    })
                    .collect()
            },
        )
    }
}
