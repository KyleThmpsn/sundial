//! The main page for subclasses. A subclass keeps its base's class and element. Each ability slot
//! and attunement can come from another stock subclass of any class, which the build writes into
//! a socket-entry list of the subclass's own. An attunement can also be built node by node, each
//! node from any stock node, with a name, a description and perks of its own.
//!
//! The page lists the subclass at the left in the game's order: its abilities by slot, then its
//! attunements with their nodes. Clicking any of them shows it at the right, with its own values
//! and every stock choice for it, a line to each stock subclass grouped by class.
use super::*;
use crate::app::style;
use crate::subclass::{AbilitySlot, AttunementPath, SubclassAbilities, SubclassPathNode, layout};
use sundial::investment::{DisplayTooltip, SubclassSummary, draw_display_tooltip};

/// The list's width beside the detail panel, and the narrowest pane that keeps the two side by
/// side.
const LIST_WIDTH: f32 = 360.0;
const SIDE_BY_SIDE_WIDTH: f32 = 860.0;
/// The list's rows, the column that names each row's slot or attunement, and the icons in rows,
/// in choices and in the detail panel's heading.
const ROW_HEIGHT: f32 = 28.0;
const GROUP_WIDTH: f32 = 96.0;
const ROW_ICON: f32 = 22.0;
const CHOICE_ICON: f32 = 18.0;
const HEADING_ICON: f32 = 48.0;
/// The detail panel's field labels, and the column that names each choice's subclass.
const LABEL_WIDTH: f32 = 110.0;

/// Attunements in the order the game shows them, the middle one between the others.
const DISPLAY_PATHS: [AttunementPath; 3] = [
    AttunementPath::Top,
    AttunementPath::Middle,
    AttunementPath::Bottom,
];

/// What the detail panel shows: an ability, an attunement, or one node of its path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubclassSelection {
    /// The ability in a socket entry.
    Ability(u8),
    Path(AttunementPath),
    Node(AttunementPath, u8),
}

impl Default for SubclassSelection {
    fn default() -> Self {
        Self::Ability(layout::CLASS_ABILITIES[0])
    }
}

/// One change the page makes to the recipe's abilities.
enum AbilityEdit {
    Choice(u8, Option<(u32, u8)>),
    /// Restores the base's own attunement, nodes and name.
    ResetAttunement(AttunementPath),
    PathSource(AttunementPath, (u32, AttunementPath)),
    PathName(AttunementPath, Option<String>),
    PathNode(AttunementPath, u8, Option<SubclassPathNode>),
    Restore,
}

/// The node at `position` of `path`: the recipe's own, or its source path's.
fn node_of(
    abilities: &SubclassAbilities,
    base: u32,
    path: AttunementPath,
    position: u8,
) -> SubclassPathNode {
    let (source, source_path) = attunement_of(abilities, base, path);
    abilities
        .attunement(path)
        .and_then(|attunement| attunement.node(position))
        .cloned()
        .unwrap_or_else(|| SubclassPathNode::stock(position, source, source_path, position))
}

/// The edit that sets `node`, or restores the path's own node when `node` is just that.
fn node_edit(
    abilities: &SubclassAbilities,
    base: u32,
    path: AttunementPath,
    node: SubclassPathNode,
) -> AbilityEdit {
    let (source, source_path) = attunement_of(abilities, base, path);
    let own = SubclassPathNode::stock(node.position, source, source_path, node.position);
    let position = node.position;
    AbilityEdit::PathNode(path, position, (node != own).then_some(node))
}

/// The ability in `entry` and the subclass it comes from.
fn choice_of(abilities: &SubclassAbilities, base: u32, entry: u8) -> (u32, u8) {
    abilities
        .choice(entry)
        .map_or((base, entry), |choice| (choice.source, choice.source_entry))
}

fn attunement_of(
    abilities: &SubclassAbilities,
    base: u32,
    path: AttunementPath,
) -> (u32, AttunementPath) {
    abilities
        .attunement(path)
        .map_or((base, path), |attunement| {
            (attunement.source, attunement.source_path)
        })
}

fn find_subclass(subclasses: &[SubclassSummary], hash: u32) -> Option<&SubclassSummary> {
    subclasses.iter().find(|subclass| subclass.hash == hash)
}

fn entry_name(subclass: &SubclassSummary, entry: u8) -> &str {
    subclass
        .entry_names
        .get(&entry)
        .map_or("Unknown Ability", String::as_str)
}

fn attunement_name(subclass: &SubclassSummary, path: AttunementPath) -> &str {
    subclass
        .attunement_names
        .get(path.index())
        .map_or(path.label(), String::as_str)
}

/// The stock node a path node is based on.
fn node_entry(node: &SubclassPathNode) -> u8 {
    node.source_path.entries()[usize::from(node.source_position)]
}

/// A slot's name for one of its entries: "Grenade 2", or "Super" for a slot of one.
fn slot_entry_label(slot: AbilitySlot, position: usize) -> String {
    if slot.entries().len() == 1 {
        slot.label().to_owned()
    } else {
        format!("{} {}", slot.label(), position + 1)
    }
}

/// Where something comes from, when that is another subclass than the base: its name and class.
fn from_label(subclass: Option<&SubclassSummary>, base: u32) -> Option<String> {
    let subclass = subclass.filter(|subclass| subclass.hash != base)?;
    Some(match gear_view::class_label(subclass.class_type) {
        Some(class) => format!("From {} · {class}", subclass.name),
        None => format!("From {}", subclass.name),
    })
}

/// A tooltip's line under a name: what it is, and the stock subclass it comes from.
fn from_subtitle(kind: &str, subclass: Option<&SubclassSummary>) -> String {
    match subclass {
        Some(subclass) => format!("{kind} · {}", subclass.name),
        None => kind.to_owned(),
    }
}

/// Stock subclasses grouped by class, the base's class first.
fn class_groups(
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

/// Text on one line, cut with an ellipsis where it would run past `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.fonts(|fonts| fonts.layout_job(job))
}

/// Small, quiet text: a section's name, a field's label, a subclass beside its choices.
fn quiet(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(12.0)
        .color(style::secondary(ui.visuals()))
}

/// The quiet icon that restores one value.
fn reset_icon(ui: &mut egui::Ui) -> bool {
    let hover = "Restore the original value";
    let button = ui.add(
        egui::Button::new(style::light_icon(
            ui,
            egui_phosphor::regular::ARROW_COUNTER_CLOCKWISE,
        ))
        .frame(false),
    );
    style::named_control(button, hover)
        .on_hover_text(hover)
        .clicked()
}

/// A row of the list: its slot or attunement's name when it starts one, its number in a path,
/// its name, the subclass it comes from when that is another, and a dot when it has edits of its
/// own. The selected row is outlined.
struct ListRow<'a> {
    group: &'a str,
    number: Option<u8>,
    /// Whether the row has an icon column, and its icon once loaded. An attunement has none.
    icon_column: bool,
    icon: Option<&'a egui::TextureHandle>,
    name: &'a str,
    detail: Option<&'a str>,
    edited: bool,
    selected: bool,
}

impl ListRow<'_> {
    fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::click(),
        );
        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals();
            let painter = ui.painter_at(rect);
            if self.selected {
                painter.rect(
                    rect,
                    3.0,
                    visuals.widgets.hovered.weak_bg_fill,
                    visuals.selection.stroke,
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
            }
            let secondary = style::secondary(visuals);
            let text = visuals.text_color();
            let middle = rect.center().y;
            let small = egui::FontId::proportional(12.0);
            if !self.group.is_empty() {
                let group = one_line(ui, self.group, small.clone(), secondary, GROUP_WIDTH - 12.0);
                let height = group.size().y;
                painter.galley(
                    egui::pos2(rect.left() + 8.0, middle - height / 2.0),
                    group,
                    secondary,
                );
            }
            let mut left = rect.left() + GROUP_WIDTH;
            if let Some(number) = self.number {
                painter.text(
                    egui::pos2(left, middle),
                    egui::Align2::LEFT_CENTER,
                    number.to_string(),
                    small.clone(),
                    secondary,
                );
                left += 16.0;
            }
            if self.icon_column {
                if let Some(icon) = self.icon {
                    painter.image(
                        icon.id(),
                        egui::Rect::from_min_size(
                            egui::pos2(left, middle - ROW_ICON / 2.0),
                            egui::Vec2::splat(ROW_ICON),
                        ),
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                left += ROW_ICON + 6.0;
            }
            let right = rect.right() - 8.0;
            let mut name_right = right;
            if let Some(detail) = self.detail {
                let detail = one_line(ui, detail, small, secondary, (right - left) * 0.4);
                let size = detail.size();
                painter.galley(
                    egui::pos2(right - size.x, middle - size.y / 2.0),
                    detail,
                    secondary,
                );
                name_right = right - size.x - 8.0;
            } else if self.edited {
                painter.circle_filled(egui::pos2(right - 3.0, middle), 3.0, text);
                name_right = right - 14.0;
            }
            let name = one_line(
                ui,
                self.name,
                egui::FontId::proportional(13.0),
                text,
                name_right - left,
            );
            let height = name.size().y;
            painter.galley(egui::pos2(left, middle - height / 2.0), name, text);
        }
        style::named_control(response, self.name)
    }
}

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

/// Every stock subclass's choices, grouped by class, a line to each subclass with its choices
/// beside its name. `search` keeps the subclasses whose names hold it, and elsewhere the choices
/// whose names do. Returns the choice clicked.
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

/// A perk as a chip: its icon and name in a button's frame, and a remove icon at its end. It is
/// measured before it is placed, so a full line wraps it whole. Returns the chip's response,
/// whether the remove icon was clicked, and whether the pointer is on it.
fn perk_chip(
    ui: &mut egui::Ui,
    icon: Option<&egui::TextureHandle>,
    name: &str,
) -> (egui::Response, bool, bool) {
    let text_color = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        egui::TextStyle::Button.resolve(ui.style()),
        text_color,
    );
    let padding = ui.spacing().button_padding;
    let icon_width = if icon.is_some() {
        CHOICE_ICON + 4.0
    } else {
        0.0
    };
    let remove_width = 20.0;
    let height = ui
        .spacing()
        .interact_size
        .y
        .max(galley.size().y + 2.0 * padding.y)
        .max(CHOICE_ICON + 2.0);
    let width = padding.x + icon_width + galley.size().x + 2.0 + remove_width;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let remove_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - remove_width, rect.top()),
        rect.max,
    );
    let remove = ui.interact(
        remove_rect,
        response.id.with("remove"),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let painter = ui.painter();
        painter.rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let mut left = rect.left() + padding.x;
        if let Some(icon) = icon {
            painter.image(
                icon.id(),
                egui::Rect::from_min_size(
                    egui::pos2(left, rect.center().y - CHOICE_ICON / 2.0),
                    egui::Vec2::splat(CHOICE_ICON),
                ),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            left += icon_width;
        }
        painter.galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
        let mark = if remove.hovered() {
            text_color
        } else {
            style::secondary(ui.visuals())
        };
        painter.text(
            remove_rect.center(),
            egui::Align2::CENTER_CENTER,
            egui_phosphor::regular::X,
            egui::FontId::proportional(12.0),
            mark,
        );
    }
    let on_remove = remove.hovered();
    let removed = style::named_control(remove, "Remove Perk")
        .on_hover_text("Remove Perk")
        .clicked();
    (response, removed, on_remove)
}

/// The icon a detail heading leads with: an ability's or a node's once it loads, or none for an
/// attunement.
#[derive(Clone, Copy)]
enum HeadingIcon<'a> {
    None,
    Node(Option<&'a egui::TextureHandle>),
}

/// The detail panel's heading: what it shows, small, then its name, then where it comes from and
/// what it does, with a button that restores the base's own at the right. Returns whether that
/// button was clicked.
fn detail_header(
    ui: &mut egui::Ui,
    (kind, name, icon): (&str, &str, HeadingIcon<'_>),
    lines: &[String],
    restore: Option<&str>,
) -> bool {
    let mut clicked = false;
    ui.horizontal_top(|ui| {
        if let HeadingIcon::Node(icon) = icon {
            let (rect, _) =
                ui.allocate_exact_size(egui::Vec2::splat(HEADING_ICON), egui::Sense::hover());
            if let Some(icon) = icon {
                ui.painter().image(
                    icon.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        let restore_width = if restore.is_some() { 220.0 } else { 0.0 };
        let width = (ui.available_width() - restore_width).max(120.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.label(quiet(ui, kind));
                ui.label(egui::RichText::new(name).size(18.0).strong());
                for line in lines {
                    ui.add(egui::Label::new(quiet(ui, line.as_str())).wrap());
                }
            },
        );
        if let Some(restore) = restore {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                clicked = ui.button(restore).clicked();
            });
        }
    });
    ui.add_space(6.0);
    clicked
}

/// A labeled field in the detail panel, with a quiet reset at its end once it differs. Returns
/// the control's result and whether the reset was clicked.
fn field<R>(
    ui: &mut egui::Ui,
    label: &str,
    modified: bool,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, bool) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                // A column of its own, so every field starts at one edge.
                ui.set_min_width(LABEL_WIDTH);
                let text = egui::RichText::new(label).size(12.0);
                let text = if modified {
                    text
                } else {
                    text.color(style::secondary(ui.visuals()))
                };
                ui.label(text);
            },
        );
        let width = (ui.available_width() - 32.0).max(80.0);
        let result = ui
            .allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    control(ui)
                },
            )
            .inner;
        (result, modified && reset_icon(ui))
    })
    .inner
}

/// The heading of the detail panel's choices, and their search. The original is always among the
/// choices, so picking it again restores it.
fn choices_heading(ui: &mut egui::Ui, name: &str, search: &mut String) {
    ui.add_space(8.0);
    ui.separator();
    ui.label(egui::RichText::new(name).strong());
    ui.add(
        egui::TextEdit::singleline(search)
            .hint_text(format!(
                "{} Search",
                egui_phosphor::regular::MAGNIFYING_GLASS
            ))
            .desired_width(f32::INFINITY),
    );
}

impl PackageAuthoringApp {
    pub(super) fn draw_subclass_editor(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        if let Some(base_width) = workbench_left_column_width(ui.available_width()) {
            let definition_width = ui.available_width() - base_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(base_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(base_width);
                        self.draw_gear_base(ui);
                        ui.add_space(8.0);
                        self.draw_icon_donor_picker(ui);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(definition_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(definition_width);
                        self.draw_item_text(ui, Some("Subclass"));
                    },
                );
            });
        } else {
            self.draw_gear_base(ui);
            ui.add_space(8.0);
            self.draw_icon_donor_picker(ui);
            ui.separator();
            self.draw_item_text(ui, Some("Subclass"));
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        self.draw_subclass_abilities(ui);
    }

    fn draw_subclass_abilities(&mut self, ui: &mut egui::Ui) {
        let Some(base) = self
            .recipe
            .donor
            .item_hash
            .parse_u32()
            .ok()
            .and_then(|hash| find_subclass(&self.subclasses, hash))
            .cloned()
        else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Base subclass not found in the catalog.",
            );
            return;
        };
        let abilities = self
            .recipe
            .overrides
            .subclass_abilities
            .clone()
            .unwrap_or_default();
        let mut search = std::mem::take(&mut self.subclass_search);
        let ((clicked, restore), edit) = if ui.available_width() >= SIDE_BY_SIDE_WIDTH {
            ui.horizontal_top(|ui| {
                let clicked = ui
                    .allocate_ui_with_layout(
                        egui::vec2(LIST_WIDTH, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(LIST_WIDTH);
                            self.draw_subclass_list(ui, &base, &abilities)
                        },
                    )
                    .inner;
                let width = ui.available_width();
                let edit = ui
                    .allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            self.draw_subclass_detail(ui, &base, &abilities, &mut search)
                        },
                    )
                    .inner;
                (clicked, edit)
            })
            .inner
        } else {
            let clicked = self.draw_subclass_list(ui, &base, &abilities);
            ui.add_space(8.0);
            let edit = self.draw_subclass_detail(ui, &base, &abilities, &mut search);
            (clicked, edit)
        };
        self.subclass_search = search;
        let edit = if restore {
            Some(AbilityEdit::Restore)
        } else {
            edit
        };
        if let Some(selection) = clicked
            && selection != self.subclass_selection
        {
            self.subclass_selection = selection;
            self.subclass_search.clear();
        }
        // The attunement tabs follow the selection.
        if let SubclassSelection::Path(path) | SubclassSelection::Node(path, _) =
            self.subclass_selection
        {
            self.subclass_path_tab = path;
        }
        if let Some(edit) = edit {
            let mut abilities = abilities;
            match edit {
                AbilityEdit::Choice(entry, source) => abilities.set_choice(entry, source),
                AbilityEdit::ResetAttunement(path) => abilities.set_attunement(path, None),
                AbilityEdit::PathSource(path, source) => {
                    abilities.set_path_source(path, base.hash, source);
                }
                AbilityEdit::PathName(path, name) => abilities.set_path_name(path, base.hash, name),
                AbilityEdit::PathNode(path, position, node) => {
                    abilities.set_path_node(path, base.hash, position, node);
                }
                AbilityEdit::Restore => abilities = SubclassAbilities::default(),
            }
            self.recipe.overrides.subclass_abilities = (!abilities.is_empty()).then_some(abilities);
        }
    }

    /// The subclass in the game's order: each ability slot's abilities, then each attunement and
    /// its nodes. Returns the row clicked, and whether Restore Base Abilities was.
    fn draw_subclass_list(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
    ) -> (Option<SubclassSelection>, bool) {
        let mut clicked = None;
        let mut restore = false;
        style::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(quiet(ui, "Abilities"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    restore = ui
                        .add_enabled(
                            !abilities.is_empty(),
                            egui::Button::new("Restore Base Abilities").small(),
                        )
                        .clicked();
                });
            });
            for (index, slot) in AbilitySlot::ALL.into_iter().enumerate() {
                if index > 0 {
                    ui.add_space(4.0);
                }
                for (position, &entry) in slot.entries().iter().enumerate() {
                    let (source, source_entry) = choice_of(abilities, base.hash, entry);
                    let summary = find_subclass(&self.subclasses, source);
                    let group = if position == 0 { slot.label() } else { "" };
                    let name = summary.map_or("Unknown Ability", |summary| {
                        entry_name(summary, source_entry)
                    });
                    let icon = self.entry_icon(ui.ctx(), summary, source_entry);
                    let row = ListRow {
                        group,
                        number: None,
                        icon_column: true,
                        icon: icon.as_ref(),
                        name,
                        detail: summary
                            .filter(|summary| summary.hash != base.hash)
                            .map(|summary| summary.name.as_str()),
                        edited: abilities.choice(entry).is_some(),
                        selected: self.subclass_selection == SubclassSelection::Ability(entry),
                    }
                    .show(ui)
                    .on_hover_ui(|ui| {
                        draw_display_tooltip(
                            ui,
                            DisplayTooltip {
                                icon: icon.as_ref(),
                                name,
                                subtitle: Some(&from_subtitle(slot.label(), summary)),
                                description: self
                                    .entry_description(summary, source_entry)
                                    .as_deref(),
                            },
                        );
                    });
                    if row.clicked() {
                        clicked = Some(SubclassSelection::Ability(entry));
                    }
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(quiet(ui, "Attunements"));
                draw_authoring_info_icon(
                    ui,
                    "Top and bottom attunements trade places. A middle one fills only the middle.",
                );
            });
            // One attunement at a time, as the game shows one tree, each tab marked once its
            // attunement has edits of its own.
            ui.horizontal(|ui| {
                for path in DISPLAY_PATHS {
                    let label = if abilities.attunement(path).is_some() {
                        format!("{} •", path.label())
                    } else {
                        path.label().to_owned()
                    };
                    if ui
                        .selectable_label(self.subclass_path_tab == path, label)
                        .clicked()
                    {
                        clicked = Some(SubclassSelection::Path(path));
                    }
                }
            });
            ui.add_space(4.0);
            if let Some(selection) =
                self.draw_path_rows(ui, base, abilities, self.subclass_path_tab)
            {
                clicked = Some(selection);
            }
        });
        (clicked, restore)
    }

    /// An attunement's row, then a row to each of its nodes. Returns the one clicked.
    fn draw_path_rows(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        path: AttunementPath,
    ) -> Option<SubclassSelection> {
        let (source, source_path) = attunement_of(abilities, base.hash, path);
        let summary = find_subclass(&self.subclasses, source);
        let own = abilities.attunement(path);
        let name = own
            .and_then(|attunement| attunement.name.as_deref())
            .unwrap_or_else(|| {
                summary.map_or(path.label(), |summary| {
                    attunement_name(summary, source_path)
                })
            });
        let mut clicked = None;
        let row = ListRow {
            group: "",
            number: None,
            icon_column: false,
            icon: None,
            name,
            detail: summary
                .filter(|summary| summary.hash != base.hash)
                .map(|summary| summary.name.as_str()),
            edited: (source, source_path) != (base.hash, path)
                || own.is_some_and(|attunement| attunement.name.is_some()),
            selected: self.subclass_selection == SubclassSelection::Path(path),
        }
        .show(ui)
        .on_hover_ui(|ui| {
            draw_display_tooltip(
                ui,
                DisplayTooltip {
                    icon: None,
                    name,
                    subtitle: Some(&from_subtitle(
                        &format!("{} Attunement", path.label()),
                        summary,
                    )),
                    description: Some(&self.path_nodes(base, abilities, path)),
                },
            );
        });
        if row.clicked() {
            clicked = Some(SubclassSelection::Path(path));
        }
        for position in 0..layout::PATH_NODES {
            let node = node_of(abilities, base.hash, path, position);
            let node_source = find_subclass(&self.subclasses, node.source);
            let stock_name = node_source.map_or("Unknown Ability", |node_source| {
                entry_name(node_source, node_entry(&node))
            });
            let node_name = node.name.as_deref().unwrap_or(stock_name);
            let node_icon = self.entry_icon(ui.ctx(), node_source, node_entry(&node));
            let description = node
                .description
                .clone()
                .or_else(|| self.entry_description(node_source, node_entry(&node)));
            let row = ListRow {
                group: "",
                number: Some(position + 1),
                icon_column: true,
                icon: node_icon.as_ref(),
                name: node_name,
                detail: node_source
                    .filter(|node_source| node_source.hash != source)
                    .map(|node_source| node_source.name.as_str()),
                edited: own.is_some_and(|attunement| attunement.node(position).is_some()),
                selected: self.subclass_selection == SubclassSelection::Node(path, position),
            }
            .show(ui)
            .on_hover_ui(|ui| {
                draw_display_tooltip(
                    ui,
                    DisplayTooltip {
                        icon: node_icon.as_ref(),
                        name: node_name,
                        subtitle: Some(&format!("{name} · Node {}", position + 1)),
                        description: description.as_deref(),
                    },
                );
            });
            if row.clicked() {
                clicked = Some(SubclassSelection::Node(path, position));
            }
        }
        ui.add_space(4.0);
        clicked
    }

    /// The selected ability, attunement or node.
    fn draw_subclass_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        search: &mut String,
    ) -> Option<AbilityEdit> {
        style::card(ui, |ui| match self.subclass_selection {
            SubclassSelection::Ability(entry) => {
                self.draw_ability_detail(ui, base, abilities, (entry, search))
            }
            SubclassSelection::Path(path) => {
                self.draw_attunement_detail(ui, base, abilities, (path, search))
            }
            SubclassSelection::Node(path, position) => {
                self.draw_node_detail(ui, base, abilities, (path, position, search))
            }
        })
    }

    /// An ability: which it is and where it comes from, then every stock ability for its slot.
    fn draw_ability_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        (entry, search): (u8, &mut String),
    ) -> Option<AbilityEdit> {
        let slot = AbilitySlot::of_entry(entry)?;
        let entries = slot.entries();
        let position = entries.iter().position(|each| *each == entry).unwrap_or(0);
        let (source, source_entry) = choice_of(abilities, base.hash, entry);
        let summary = find_subclass(&self.subclasses, source);
        let name = summary.map_or("Unknown Ability", |summary| {
            entry_name(summary, source_entry)
        });
        let lines = from_label(summary, base.hash)
            .into_iter()
            .chain(self.entry_description(summary, source_entry))
            .collect::<Vec<_>>();
        let restore = abilities
            .choice(entry)
            .is_some()
            .then(|| format!("Restore {}", entry_name(base, entry)));
        let icon = self.entry_icon(ui.ctx(), summary, source_entry);
        let mut edit = None;
        if detail_header(
            ui,
            (
                &slot_entry_label(slot, position),
                name,
                HeadingIcon::Node(icon.as_ref()),
            ),
            &lines,
            restore.as_deref(),
        ) {
            edit = Some(AbilityEdit::Choice(entry, None));
        }
        // Choosing one ability twice in a slot would offer it twice.
        let chosen = entries
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != position)
            .filter_map(|(_, &other)| {
                let (source, source_entry) = choice_of(abilities, base.hash, other);
                find_subclass(&self.subclasses, source)
                    .map(|source| entry_name(source, source_entry).to_owned())
            })
            .collect::<Vec<_>>();
        choices_heading(ui, "Replace With", search);
        let ctx = ui.ctx().clone();
        let picked = draw_choices(
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
                            current: (option.hash, option_entry) == (source, source_entry),
                            taken: chosen.contains(&label),
                            starts_group: false,
                            label,
                        }
                    })
                    .collect()
            },
        );
        if let Some(pick) = picked {
            let own = (base.hash, entry);
            edit = Some(AbilityEdit::Choice(entry, (pick != own).then_some(pick)));
        }
        edit
    }

    /// An attunement: its name, then every stock attunement that fits its place.
    fn draw_attunement_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        (path, search): (AttunementPath, &mut String),
    ) -> Option<AbilityEdit> {
        let (source, source_path) = attunement_of(abilities, base.hash, path);
        let summary = find_subclass(&self.subclasses, source);
        let stock_name = summary.map_or(path.label(), |summary| {
            attunement_name(summary, source_path)
        });
        let own = abilities.attunement(path);
        let name = own.and_then(|attunement| attunement.name.clone());
        let lines = from_label(summary, base.hash)
            .into_iter()
            .collect::<Vec<_>>();
        let restore = own
            .is_some()
            .then(|| format!("Restore {}", attunement_name(base, path)));
        let mut edit = None;
        if detail_header(
            ui,
            (
                &format!("{} Attunement", path.label()),
                name.as_deref().unwrap_or(stock_name),
                HeadingIcon::None,
            ),
            &lines,
            restore.as_deref(),
        ) {
            edit = Some(AbilityEdit::ResetAttunement(path));
        }
        let (named, reset) = field(ui, "Name", name.is_some(), |ui| {
            let mut text = name.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(stock_name)
                    .desired_width(f32::INFINITY),
            )
            .changed()
            .then_some(text)
        });
        if let Some(text) = named {
            let text = (!text.trim().is_empty()).then_some(text);
            edit = Some(AbilityEdit::PathName(path, text));
        } else if reset {
            edit = Some(AbilityEdit::PathName(path, None));
        }
        choices_heading(ui, "Replace With", search);
        let chosen = AttunementPath::ALL
            .into_iter()
            .filter(|other| *other != path)
            .map(|other| attunement_of(abilities, base.hash, other))
            .collect::<Vec<_>>();
        let picked = draw_choices(
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
                        current: (option.hash, option_path) == (source, source_path),
                        taken: chosen.contains(&(option.hash, option_path)),
                        starts_group: false,
                    })
                    .collect()
            },
        );
        if let Some(pick) = picked {
            edit = Some(AbilityEdit::PathSource(path, pick));
        }
        edit
    }

    /// A path node: its name, description and perks, then every stock node it can be based on.
    fn draw_node_detail(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        (path, position, search): (AttunementPath, u8, &mut String),
    ) -> Option<AbilityEdit> {
        let node = node_of(abilities, base.hash, path, position);
        let (source, source_path) = attunement_of(abilities, base.hash, path);
        let stock = SubclassPathNode::stock(position, source, source_path, position);
        let summary = find_subclass(&self.subclasses, node.source);
        let entry = node_entry(&node);
        let stock_name = summary.map_or("Unknown Ability", |summary| entry_name(summary, entry));
        let own = abilities
            .attunement(path)
            .is_some_and(|attunement| attunement.node(position).is_some());
        let mut lines = Vec::new();
        if node.name.is_some()
            || (node.source, node.source_path, node.source_position)
                != (stock.source, stock.source_path, stock.source_position)
        {
            let basis = summary.map_or_else(
                || stock_name.to_owned(),
                |summary| format!("Based on {stock_name} · {}", summary.name),
            );
            lines.push(basis);
        }
        let mut edit = None;
        let restore_name = find_subclass(&self.subclasses, stock.source)
            .map_or("Node", |stock_source| {
                entry_name(stock_source, node_entry(&stock))
            });
        let restore = own.then(|| format!("Restore {restore_name}"));
        let icon = self.entry_icon(ui.ctx(), summary, entry);
        if detail_header(
            ui,
            (
                &format!("{} Path · Node {}", path.label(), position + 1),
                node.name.as_deref().unwrap_or(stock_name),
                HeadingIcon::Node(icon.as_ref()),
            ),
            &lines,
            restore.as_deref(),
        ) {
            edit = Some(AbilityEdit::PathNode(path, position, None));
        }
        let mut changed = None;
        let (named, reset) = field(ui, "Name", node.name.is_some(), |ui| {
            let mut text = node.name.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(stock_name)
                    .desired_width(f32::INFINITY),
            )
            .changed()
            .then_some(text)
        });
        if let Some(text) = named {
            let name = (!text.trim().is_empty()).then_some(text);
            changed = Some(SubclassPathNode {
                name,
                ..node.clone()
            });
        } else if reset {
            changed = Some(SubclassPathNode {
                name: None,
                ..node.clone()
            });
        }
        let (described, reset) = field(ui, "Description", node.description.is_some(), |ui| {
            let mut text = node.description.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            )
            .changed()
            .then_some(text)
        });
        if let Some(text) = described {
            let description = (!text.trim().is_empty()).then_some(text);
            changed = Some(SubclassPathNode {
                description,
                ..node.clone()
            });
        } else if reset {
            changed = Some(SubclassPathNode {
                description: None,
                ..node.clone()
            });
        }
        let stock_perks = summary
            .and_then(|summary| summary.entry_perks.get(&entry))
            .cloned()
            .unwrap_or_default();
        let perks_edited = !node.added_perks.is_empty() || !node.removed_perks.is_empty();
        let (perked, reset) = field(ui, "Perks", perks_edited, |ui| {
            self.draw_node_perks(ui, base, &node, &stock_perks)
        });
        if perked.is_some() {
            changed = perked;
        } else if reset {
            changed = Some(SubclassPathNode {
                added_perks: Vec::new(),
                removed_perks: Vec::new(),
                ..node.clone()
            });
        }
        choices_heading(ui, "Based On", search);
        let lead = position == layout::LEAD_NODE;
        // The first node a path offers here, which starts its group.
        let first = if lead {
            layout::LEAD_NODE
        } else {
            layout::LEAD_NODE + 1
        };
        let ctx = ui.ctx().clone();
        let picked = draw_choices(
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
        );
        if let Some((option, option_path, option_position)) = picked {
            changed = Some(SubclassPathNode {
                source: option,
                source_path: option_path,
                source_position: option_position,
                ..node.clone()
            });
        }
        edit.or_else(|| changed.map(|node| node_edit(abilities, base.hash, path, node)))
    }

    /// A node's perks: its source's, less the ones it removes, and the ones it adds, each with a
    /// remove button, then a menu of every stock node's perks.
    fn draw_node_perks(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        node: &SubclassPathNode,
        stock_perks: &[u16],
    ) -> Option<SubclassPathNode> {
        let mut changed = None;
        let perks = stock_perks
            .iter()
            .filter(|perk| !node.removed_perks.contains(perk))
            .chain(&node.added_perks)
            .copied()
            .collect::<Vec<_>>();
        ui.horizontal_wrapped(|ui| {
            for &perk in &perks {
                let label = self.perk_label(perk);
                let source = self.perk_source(perk);
                let icon = source.and_then(|(subclass, entry, _)| {
                    self.entry_icon(ui.ctx(), Some(subclass), entry)
                });
                let (chip, removed, on_remove) = perk_chip(ui, icon.as_ref(), &label);
                if !on_remove {
                    chip.on_hover_ui(|ui| {
                        draw_display_tooltip(
                            ui,
                            DisplayTooltip {
                                icon: icon.as_ref(),
                                name: &label,
                                subtitle: source.map(|(subclass, _, _)| subclass.name.as_str()),
                                description: Some(&self.perk_description(perk)),
                            },
                        );
                    });
                }
                if removed {
                    let mut edited = node.clone();
                    if edited.added_perks.contains(&perk) {
                        edited.added_perks.retain(|added| *added != perk);
                    } else {
                        edited.removed_perks.push(perk);
                    }
                    changed = Some(edited);
                }
            }
            ui.menu_button("Add Perk", |ui| {
                workbench_style(ui);
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        if let Some(perk) = self.draw_perk_menu(ui, base, &perks) {
                            let mut edited = node.clone();
                            if edited.removed_perks.contains(&perk) {
                                edited.removed_perks.retain(|removed| *removed != perk);
                            } else {
                                edited.added_perks.push(perk);
                            }
                            changed = Some(edited);
                            ui.close_menu();
                        }
                    });
            });
        });
        changed
    }

    /// Every stock node's perks that a node does not have yet, grouped by class. Returns the one
    /// clicked.
    fn draw_perk_menu(
        &self,
        ui: &mut egui::Ui,
        base: &SubclassSummary,
        perks: &[u16],
    ) -> Option<u16> {
        let mut picked = None;
        for (class_type, members) in class_groups(&self.subclasses, base.class_type) {
            ui.label(quiet(
                ui,
                gear_view::class_label(class_type).unwrap_or("Other"),
            ));
            for option in members {
                for (entry, entry_perks) in &option.entry_perks {
                    for (ordinal, perk) in entry_perks.iter().enumerate() {
                        if perks.contains(perk) {
                            continue;
                        }
                        let mut label = format!("{} · {}", entry_name(option, *entry), option.name);
                        if entry_perks.len() > 1 {
                            label = format!("{label} {}", ordinal + 1);
                        }
                        if ui
                            .selectable_label(false, label)
                            .on_hover_text(self.perk_description(*perk))
                            .clicked()
                        {
                            picked = Some(*perk);
                        }
                    }
                }
            }
            ui.separator();
        }
        picked
    }

    /// What a stock subclass's entry does: its node's own description, or else the first of its
    /// perks that says.
    fn entry_description(&self, subclass: Option<&SubclassSummary>, entry: u8) -> Option<String> {
        let subclass = subclass?;
        if let Some(description) = subclass.entry_descriptions.get(&entry) {
            return Some(description.clone());
        }
        let catalog = self.catalog.as_ref()?;
        subclass
            .entry_perks
            .get(&entry)?
            .iter()
            .filter_map(|perk| catalog.perk_component_description(*perk))
            .find(|text| !text.trim().is_empty())
            .map(str::to_owned)
    }

    /// The icon a stock subclass's entry shows, once it has loaded.
    fn entry_icon(
        &self,
        ctx: &egui::Context,
        subclass: Option<&SubclassSummary>,
        entry: u8,
    ) -> Option<egui::TextureHandle> {
        let container = *subclass?.entry_icons.get(&entry)?;
        self.catalog.as_ref()?.subclass_icon(ctx, container)
    }

    /// An attunement's nodes by name, a line each, as its tooltip lists them.
    fn path_nodes(
        &self,
        base: &SubclassSummary,
        abilities: &SubclassAbilities,
        path: AttunementPath,
    ) -> String {
        (0..layout::PATH_NODES)
            .map(|position| {
                let node = node_of(abilities, base.hash, path, position);
                node.name.clone().unwrap_or_else(|| {
                    find_subclass(&self.subclasses, node.source).map_or_else(
                        || "Unknown Ability".to_owned(),
                        |source| entry_name(source, node_entry(&node)).to_owned(),
                    )
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The first stock node that grants a sandbox perk: its subclass, its entry, and the perk's
    /// number among that node's perks when it grants several.
    fn perk_source(&self, perk: u16) -> Option<(&SubclassSummary, u8, Option<usize>)> {
        self.subclasses.iter().find_map(|subclass| {
            subclass.entry_perks.iter().find_map(|(entry, perks)| {
                let ordinal = perks.iter().position(|each| *each == perk)?;
                Some((subclass, *entry, (perks.len() > 1).then_some(ordinal + 1)))
            })
        })
    }

    /// A sandbox perk by the first stock node that grants it, numbered as the Add Perk list
    /// numbers it when that node grants several, or by its number.
    fn perk_label(&self, perk: u16) -> String {
        self.perk_source(perk).map_or_else(
            || format!("Perk {perk}"),
            |(subclass, entry, number)| {
                let name = entry_name(subclass, entry);
                number.map_or_else(|| name.to_owned(), |number| format!("{name} {number}"))
            },
        )
    }

    fn perk_description(&self, perk: u16) -> String {
        self.catalog
            .as_ref()
            .and_then(|catalog| catalog.perk_component_description(perk))
            .filter(|text| !text.trim().is_empty())
            .map_or_else(|| format!("Perk {perk}"), str::to_owned)
    }
}
