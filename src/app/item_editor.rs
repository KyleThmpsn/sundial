use std::hash::Hash;

use eframe::egui;

use crate::{catalog::Catalog, hash::format_hash};

const POWER_PER_LEVEL: i64 = 10;
const MINIMUM_POWERED_ITEM_POWER: i64 = 750;
const MAXIMUM_POWERED_ITEM_POWER: i64 = 1060;
const ITEM_HEADER_TITLE_SIZE_DELTA: f32 = 2.0;
const ITEM_HEADER_ICON_SIZE: f32 = 48.0;
const ITEM_HEADER_ROW_HEIGHT: f32 = 48.0;
const ITEM_HEADER_WITH_METADATA_ROW_HEIGHT: f32 = 54.0;

#[derive(Clone, Copy)]
enum FeatherActionIcon {
    Trash,
    Lock,
    Unlock,
}

/// Draws a compact, neutral delete button using Feather's Trash 2 icon.
pub(crate) fn draw_trash_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
) -> egui::Response {
    draw_feather_action_button(ui, enabled, accessible_label, FeatherActionIcon::Trash)
}

/// Matching lock-state icons are ready for a future compact lock control.
#[allow(dead_code)]
pub(crate) fn draw_lock_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
) -> egui::Response {
    draw_feather_action_button(ui, enabled, accessible_label, FeatherActionIcon::Lock)
}

#[allow(dead_code)]
pub(crate) fn draw_unlock_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
) -> egui::Response {
    draw_feather_action_button(ui, enabled, accessible_label, FeatherActionIcon::Unlock)
}

/// Feather action icons are MIT-licensed; see THIRD_PARTY_NOTICES.md.
fn draw_feather_action_button(
    ui: &mut egui::Ui,
    enabled: bool,
    accessible_label: &str,
    icon: FeatherActionIcon,
) -> egui::Response {
    let side = (ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().button_padding.y)
        .max(20.0);
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("")
            .small()
            .min_size(egui::vec2(side, side)),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, accessible_label)
    });

    if ui.is_rect_visible(response.rect) {
        let base_icon_size = (response.rect.height() - 5.0).clamp(12.0, 16.0);
        let icon_size = if matches!(icon, FeatherActionIcon::Trash) {
            base_icon_size - 1.0
        } else {
            base_icon_size
        };
        let icon_rect =
            egui::Rect::from_center_size(response.rect.center(), egui::vec2(icon_size, icon_size));
        let point = |x: f32, y: f32| {
            egui::pos2(
                icon_rect.left() + x / 24.0 * icon_rect.width(),
                icon_rect.top() + y / 24.0 * icon_rect.height(),
            )
        };
        let color = ui.style().interact(&response).fg_stroke.color;
        let stroke = egui::Stroke::new((icon_size / 12.0).max(1.0), color);
        let painter = ui.painter();

        match icon {
            FeatherActionIcon::Trash => {
                painter.add(egui::Shape::line(
                    vec![point(3.0, 6.0), point(5.0, 6.0), point(21.0, 6.0)],
                    stroke,
                ));
                painter.add(egui::Shape::line(
                    vec![
                        point(19.0, 6.0),
                        point(19.0, 20.0),
                        point(18.8, 20.8),
                        point(18.2, 21.5),
                        point(17.0, 22.0),
                        point(7.0, 22.0),
                        point(5.8, 21.5),
                        point(5.2, 20.8),
                        point(5.0, 20.0),
                        point(5.0, 6.0),
                    ],
                    stroke,
                ));
                painter.add(egui::Shape::line(
                    vec![
                        point(8.0, 6.0),
                        point(8.0, 4.0),
                        point(8.2, 3.2),
                        point(8.8, 2.5),
                        point(10.0, 2.0),
                        point(14.0, 2.0),
                        point(15.2, 2.5),
                        point(15.8, 3.2),
                        point(16.0, 4.0),
                        point(16.0, 6.0),
                    ],
                    stroke,
                ));
                painter.line_segment([point(10.0, 11.0), point(10.0, 17.0)], stroke);
                painter.line_segment([point(14.0, 11.0), point(14.0, 17.0)], stroke);
            }
            FeatherActionIcon::Lock | FeatherActionIcon::Unlock => {
                painter.rect_stroke(
                    egui::Rect::from_min_max(point(3.0, 11.0), point(21.0, 22.0)),
                    egui::CornerRadius::same(2),
                    stroke,
                    egui::StrokeKind::Middle,
                );
                let mut shackle = vec![
                    point(7.0, 11.0),
                    point(7.0, 7.0),
                    point(7.3, 5.3),
                    point(8.2, 3.8),
                    point(9.7, 2.7),
                    point(11.2, 2.1),
                    point(12.0, 2.0),
                ];
                if matches!(icon, FeatherActionIcon::Lock) {
                    shackle.extend([
                        point(12.8, 2.1),
                        point(14.3, 2.7),
                        point(15.8, 3.8),
                        point(16.7, 5.3),
                        point(17.0, 7.0),
                        point(17.0, 11.0),
                    ]);
                } else {
                    shackle.extend([
                        point(13.7, 2.2),
                        point(15.2, 3.0),
                        point(16.2, 4.3),
                        point(16.9, 6.0),
                    ]);
                }
                painter.add(egui::Shape::line(shackle, stroke));
            }
        }
    }

    response
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativePlugDefault {
    Plug(u64),
    Empty,
}

impl NativePlugDefault {
    pub(crate) const fn value(self) -> Option<u64> {
        match self {
            Self::Plug(hash) => Some(hash),
            Self::Empty => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemEditorAction {
    SetDefinition {
        hash: u64,
    },
    EquipInventoryItem {
        item_index: usize,
    },
    ClearDefinition,
    SetLevel {
        level: i64,
    },
    SetQuantity {
        quantity: i64,
    },
    SetPlug {
        socket_index: usize,
        hash: Option<u64>,
    },
}

pub(crate) enum DefinitionSummary<'a> {
    Empty,
    Known {
        name: &'a str,
        hash: &'a str,
        type_name: &'a str,
    },
    Unknown {
        hash: &'a str,
    },
}

pub(crate) struct ItemHeader<'a> {
    pub label: Option<&'a str>,
    pub soid: Option<&'a str>,
    pub definition: DefinitionSummary<'a>,
    pub icon: Option<egui::TextureHandle>,
    pub fill: egui::Color32,
    pub valid: bool,
    pub invalid_message: &'a str,
}

pub(crate) fn muted_item_header_fill(ui: &egui::Ui) -> egui::Color32 {
    let [red, green, blue, _] = ui.visuals().panel_fill.to_srgba_unmultiplied();
    egui::Color32::from_rgb(
        red.saturating_add(14),
        green.saturating_add(14),
        blue.saturating_add(14),
    )
}

pub(crate) fn draw_item_header_with_trailing(
    ui: &mut egui::Ui,
    header: ItemHeader<'_>,
    trailing: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let fill = header.fill;
    egui::Frame::NONE
        .fill(fill)
        .inner_margin(egui::Margin {
            left: 0,
            right: 4,
            top: 1,
            bottom: 1,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            draw_item_header_contents(ui, header, true, trailing)
        })
        .inner
}

pub(crate) fn draw_catalog_item_header_with_trailing(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    hash: Option<u64>,
    mut header: ItemHeader<'_>,
    trailing: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    header.icon = hash.and_then(|hash| catalog.icon_texture(ui.ctx(), hash));
    let response = draw_item_header_with_trailing(ui, header, trailing);
    finish_catalog_item_header(catalog, hash, response)
}

fn finish_catalog_item_header(
    catalog: &Catalog,
    hash: Option<u64>,
    response: egui::Response,
) -> egui::Response {
    let Some(hash) = hash else {
        return response;
    };
    catalog_item_tooltip(response, catalog, hash)
}

fn draw_item_header_contents(
    ui: &mut egui::Ui,
    header: ItemHeader<'_>,
    has_trailing: bool,
    trailing: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let body_font = egui::TextStyle::Body.resolve(ui.style());
    let monospace_font = egui::TextStyle::Monospace.resolve(ui.style());
    let mut title_font = body_font.clone();
    title_font.size += ITEM_HEADER_TITLE_SIZE_DELTA;
    let mut title_monospace_font = monospace_font.clone();
    title_monospace_font.size += ITEM_HEADER_TITLE_SIZE_DELTA;
    let metadata_monospace_font = monospace_font;
    let text_color = ui.visuals().text_color();
    let strong_color = ui.visuals().strong_text_color();
    let weak_color = ui.visuals().weak_text_color();
    let error_color = ui.visuals().error_fg_color;
    let item_spacing = ui.spacing().item_spacing.x;
    let mut title_job = egui::text::LayoutJob::default();
    let mut title_hash_job = egui::text::LayoutJob::default();
    let mut subtitle_job = egui::text::LayoutJob::default();
    let mut metadata_job = egui::text::LayoutJob::default();
    let mut title_text = String::new();
    let mut title_hash_text = String::new();
    let mut subtitle_text = String::new();

    let body = egui::TextFormat {
        font_id: body_font.clone(),
        color: text_color,
        ..Default::default()
    };
    let strong = egui::TextFormat {
        font_id: title_font.clone(),
        color: strong_color,
        ..Default::default()
    };
    let title_weak = egui::TextFormat {
        font_id: title_font.clone(),
        color: weak_color,
        ..Default::default()
    };
    let title_error = egui::TextFormat {
        font_id: title_font,
        color: error_color,
        ..Default::default()
    };
    let weak = egui::TextFormat {
        font_id: body_font,
        color: text_color,
        ..Default::default()
    };
    let title_hash_weak = egui::TextFormat {
        font_id: title_monospace_font.clone(),
        color: text_color,
        ..Default::default()
    };
    let title_hash_error = egui::TextFormat {
        font_id: title_monospace_font,
        color: error_color,
        ..Default::default()
    };
    let error = egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        color: error_color,
        ..Default::default()
    };
    let mut type_name = None;
    match header.definition {
        DefinitionSummary::Empty => {
            append_header_text(&mut title_job, &mut title_text, "Empty", 0.0, title_weak);
        }
        DefinitionSummary::Known {
            name,
            hash,
            type_name: definition_type_name,
        } => {
            append_header_text(&mut title_job, &mut title_text, name, 0.0, strong);
            append_header_text(
                &mut title_hash_job,
                &mut title_hash_text,
                hash,
                0.0,
                title_hash_weak,
            );
            type_name = (!definition_type_name.trim().is_empty()).then_some(definition_type_name);
        }
        DefinitionSummary::Unknown { hash } => {
            append_header_text(
                &mut title_job,
                &mut title_text,
                "Unknown item",
                0.0,
                title_error,
            );
            append_header_text(
                &mut title_hash_job,
                &mut title_hash_text,
                hash,
                0.0,
                title_hash_error,
            );
        }
    }
    if let Some(type_name) = type_name {
        append_header_text(
            &mut subtitle_job,
            &mut subtitle_text,
            type_name,
            0.0,
            weak.clone(),
        );
    }
    if let Some(label) = header.label {
        let label_spacing = if subtitle_text.is_empty() {
            0.0
        } else {
            item_spacing
        };
        if !subtitle_text.is_empty() {
            append_header_text(
                &mut subtitle_job,
                &mut subtitle_text,
                "|",
                item_spacing,
                weak.clone(),
            );
        }
        append_header_text(
            &mut subtitle_job,
            &mut subtitle_text,
            label,
            label_spacing,
            body,
        );
    }
    if !header.valid {
        let invalid_spacing = if subtitle_text.is_empty() {
            0.0
        } else {
            item_spacing
        };
        append_header_text(
            &mut subtitle_job,
            &mut subtitle_text,
            header.invalid_message,
            invalid_spacing,
            error,
        );
    }
    if let Some(soid) = header.soid {
        let metadata_monospace = egui::TextFormat {
            font_id: metadata_monospace_font,
            color: text_color,
            ..Default::default()
        };
        let mut metadata_text = String::new();
        append_header_text(
            &mut metadata_job,
            &mut metadata_text,
            soid,
            0.0,
            metadata_monospace,
        );
    }
    let row_height = item_header_row_height(ui, header.soid.is_some() || has_trailing);
    let trailing_width = if has_trailing {
        layout_job_width(ui, &title_hash_job).max(64.0)
    } else {
        0.0
    };
    let width = ui.available_width().max(0.0);
    let mut header_response = None;
    let header_area = ui.allocate_ui_with_layout(
        egui::vec2(width, row_height),
        if has_trailing {
            egui::Layout::right_to_left(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        },
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if has_trailing {
                let trailing_width = trailing_width.min(ui.available_width());
                ui.allocate_ui_with_layout(
                    egui::vec2(trailing_width, row_height),
                    egui::Layout::top_down(egui::Align::Max),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if !title_hash_job.text.is_empty() {
                            ui.add(
                                egui::Label::new(title_hash_job)
                                    .truncate()
                                    .halign(egui::Align::RIGHT),
                            );
                        }
                        trailing(ui);
                    },
                );
                let main_width = ui.available_width().max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(main_width, row_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        header_response = Some(draw_item_header_main(
                            ui,
                            row_height,
                            header.icon.as_ref(),
                            title_job,
                            egui::text::LayoutJob::default(),
                            subtitle_job,
                            metadata_job,
                        ));
                    },
                );
            } else {
                header_response = Some(draw_item_header_main(
                    ui,
                    row_height,
                    header.icon.as_ref(),
                    title_job,
                    title_hash_job,
                    subtitle_job,
                    metadata_job,
                ));
            }
        },
    );
    (header_response.expect("an item header always draws its main content") | header_area.response)
        .interact(egui::Sense::click())
}

fn draw_item_header_main(
    ui: &mut egui::Ui,
    row_height: f32,
    icon: Option<&egui::TextureHandle>,
    title: egui::text::LayoutJob,
    title_hash: egui::text::LayoutJob,
    subtitle: egui::text::LayoutJob,
    metadata: egui::text::LayoutJob,
) -> egui::Response {
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut response = None;
    if let Some(icon) = icon {
        merge_response(
            &mut response,
            ui.add(
                egui::Image::new(icon)
                    .fit_to_exact_size(egui::vec2(ITEM_HEADER_ICON_SIZE, ITEM_HEADER_ICON_SIZE))
                    .maintain_aspect_ratio(true),
            ),
        );
    }
    merge_response(
        &mut response,
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| draw_item_header_text(ui, title, title_hash, subtitle, metadata),
        )
        .inner,
    );
    response.expect("an item header always draws text")
}

fn merge_response(target: &mut Option<egui::Response>, response: egui::Response) {
    *target = Some(match target.take() {
        Some(current) => current | response,
        None => response,
    });
}

fn draw_item_header_text(
    ui: &mut egui::Ui,
    title: egui::text::LayoutJob,
    title_hash: egui::text::LayoutJob,
    subtitle: egui::text::LayoutJob,
    metadata: egui::text::LayoutJob,
) -> egui::Response {
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut response = draw_item_header_title(ui, title, title_hash);
    if !subtitle.text.is_empty() {
        response |= ui.add(
            egui::Label::new(subtitle)
                .truncate()
                .halign(egui::Align::LEFT),
        );
    }
    if !metadata.text.is_empty() {
        response |= ui.add(
            egui::Label::new(metadata)
                .truncate()
                .halign(egui::Align::LEFT),
        );
    }
    response
}

fn draw_item_header_title(
    ui: &mut egui::Ui,
    mut title: egui::text::LayoutJob,
    hash: egui::text::LayoutJob,
) -> egui::Response {
    if hash.text.is_empty() {
        return ui.add(egui::Label::new(title).truncate().halign(egui::Align::LEFT));
    }

    let available_width = ui.available_width().max(0.0);
    let spacing = ui.spacing().item_spacing.x;
    let hash_galley = ui.fonts(|fonts| fonts.layout_job(hash));
    let title_width = (available_width - hash_galley.size().x - spacing).max(0.0);
    title.wrap.max_width = title_width;
    title.wrap.max_rows = 1;
    title.wrap.break_anywhere = true;
    let title_galley = ui.fonts(|fonts| fonts.layout_job(title));
    let row_height = title_galley.size().y.max(hash_galley.size().y);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(available_width, row_height),
        egui::Sense::hover(),
    );
    ui.painter().galley(
        rect.left_top(),
        title_galley,
        ui.visuals().strong_text_color(),
    );
    ui.painter().galley(
        egui::pos2(rect.right() - hash_galley.size().x, rect.top()),
        hash_galley,
        ui.visuals().text_color(),
    );
    response
}

fn layout_job_width(ui: &egui::Ui, job: &egui::text::LayoutJob) -> f32 {
    ui.fonts(|fonts| fonts.layout_job(job.clone()).size().x)
}

fn item_header_row_height(ui: &egui::Ui, has_metadata: bool) -> f32 {
    let minimum = if has_metadata {
        ITEM_HEADER_WITH_METADATA_ROW_HEIGHT
    } else {
        ITEM_HEADER_ROW_HEIGHT
    };
    ui.spacing().interact_size.y.max(minimum)
}

fn append_header_text(
    job: &mut egui::text::LayoutJob,
    full_text: &mut String,
    text: &str,
    leading_space: f32,
    format: egui::TextFormat,
) {
    if !full_text.is_empty() {
        full_text.push_str("  ");
    }
    full_text.push_str(text);
    job.append(text, leading_space, format);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DefinitionChoice {
    pub hash: u64,
    pub name: String,
    pub type_name: String,
    /// Optional browse grouping. Callers keep equal groups adjacent.
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExistingInventoryChoice {
    pub item_index: usize,
    pub hash: u64,
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ClearDefinitionChoice {
    pub label: String,
    pub tooltip: String,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DefinitionPickerChoices {
    pub definitions: Vec<DefinitionChoice>,
    pub existing_inventory: Vec<ExistingInventoryChoice>,
    pub clear: Option<ClearDefinitionChoice>,
    pub empty_message: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PickerHeight {
    pub min: f32,
    pub max: f32,
}

pub(crate) fn draw_definition_picker_with_open_request(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    height: PickerHeight,
    trigger: (Option<&egui::Response>, bool),
    choices_for_query: impl FnOnce(&str) -> DefinitionPickerChoices,
) -> Option<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let (anchor, open_requested) = trigger;
        let picker_response = anchor.cloned().unwrap_or_else(|| {
            ui.add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                egui::Button::new("Choose an item…"),
            )
        });
        let popup_id = ui.make_persistent_id("definition-browser");
        let just_opened = ui.is_enabled() && (open_requested || picker_response.clicked());
        if just_opened {
            ui.memory_mut(|memory| memory.open_popup(popup_id));
        }
        if !ui.memory(|memory| memory.is_popup_open(popup_id)) {
            return None;
        }

        let row_height = ui.spacing().interact_size.y.max(44.0);
        let popup_direction = popup_direction(ui.ctx().screen_rect(), picker_response.rect);
        let mut action = None;
        egui::popup::popup_above_or_below_widget(
            ui,
            popup_id,
            &picker_response,
            popup_direction,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(picker_response.rect.width().max(360.0));
                let search_response = ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text("Search item name, description, type, or hex hash…")
                        .desired_width(ui.available_width()),
                );
                if just_opened {
                    search_response.request_focus();
                }
                ui.separator();
                let choices = choices_for_query(query);
                if choices.definitions.is_empty()
                    && choices.existing_inventory.is_empty()
                    && choices.clear.is_none()
                {
                    ui.label(egui::RichText::new(&choices.empty_message).weak());
                    return;
                }
                if let Some(clear) = &choices.clear {
                    if ui
                        .selectable_label(clear.selected, &clear.label)
                        .on_hover_text(&clear.tooltip)
                        .clicked()
                    {
                        action = Some(ItemEditorAction::ClearDefinition);
                        ui.memory_mut(egui::Memory::close_popup);
                    }
                    ui.separator();
                }

                let rows = definition_picker_rows(&choices.definitions);
                let scroll_row_count = rows.len()
                    + choices.existing_inventory.len()
                    + usize::from(!choices.existing_inventory.is_empty());
                if scroll_row_count == 0 {
                    return;
                }
                let picker_height =
                    picker_list_height(scroll_row_count, row_height, height.min, height.max);
                egui::ScrollArea::vertical()
                    .min_scrolled_height(picker_height)
                    .max_height(picker_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if !choices.existing_inventory.is_empty() {
                            ui.label(egui::RichText::new("Existing inventory item").strong());
                            for existing in &choices.existing_inventory {
                                let label =
                                    format!("{}  ({})", existing.name, format_hash(existing.hash));
                                let response = draw_catalog_picker_row(
                                    ui,
                                    catalog,
                                    CatalogPickerRow {
                                        hash: existing.hash,
                                        primary: &label,
                                        secondary: (!existing.type_name.trim().is_empty())
                                            .then_some(existing.type_name.as_str()),
                                        icon_size: 36.0,
                                        row_height,
                                        selected: false,
                                    },
                                );
                                let response =
                                    catalog_item_tooltip(response, catalog, existing.hash);
                                if response.clicked() {
                                    action = Some(ItemEditorAction::EquipInventoryItem {
                                        item_index: existing.item_index,
                                    });
                                    ui.memory_mut(egui::Memory::close_popup);
                                }
                            }
                            ui.separator();
                        }

                        for row in rows {
                            match row {
                                DefinitionPickerRow::Group(group) => {
                                    ui.add_sized(
                                        [ui.available_width(), row_height],
                                        egui::Label::new(egui::RichText::new(group).strong())
                                            .halign(egui::Align::LEFT),
                                    );
                                }
                                DefinitionPickerRow::Definition(definition) => {
                                    let label = format!(
                                        "{}  ({})",
                                        definition.name,
                                        format_hash(definition.hash)
                                    );
                                    let response = draw_catalog_picker_row(
                                        ui,
                                        catalog,
                                        CatalogPickerRow {
                                            hash: definition.hash,
                                            primary: &label,
                                            secondary: (!definition.type_name.trim().is_empty())
                                                .then_some(definition.type_name.as_str()),
                                            icon_size: 36.0,
                                            row_height,
                                            selected: false,
                                        },
                                    );
                                    let response =
                                        catalog_item_tooltip(response, catalog, definition.hash);
                                    let clicked = response.clicked();
                                    if clicked {
                                        action = Some(ItemEditorAction::SetDefinition {
                                            hash: definition.hash,
                                        });
                                        ui.memory_mut(egui::Memory::close_popup);
                                    }
                                }
                            }
                        }
                    });
            },
        );
        action
    })
    .inner
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NumericItemFields {
    pub level: Option<i64>,
    pub quantity: Option<i64>,
    pub quantity_max: Option<i64>,
}

pub(crate) fn draw_level_and_quantity(
    ui: &mut egui::Ui,
    scope: impl Hash,
    fields: NumericItemFields,
) -> Vec<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let mut actions = Vec::new();
        if let Some(level) = fields.level {
            let mut power = displayed_item_power(level);
            ui.label("Power");
            let response = ui.add(
                egui::DragValue::new(&mut power)
                    .speed(POWER_PER_LEVEL as f64)
                    .range(item_power_input_range())
                    .clamp_existing_to_range(false),
            );
            if response.changed()
                && let Some(authored_level) = authored_item_level(power)
                && authored_level != level
            {
                actions.push(ItemEditorAction::SetLevel {
                    level: authored_level,
                });
            }
            response.on_hover_text(
                "Sunrise stores one-tenth of the in-game power. Values snap down to a multiple of 10.",
            );
        }
        if fields.level.is_some() && fields.quantity.is_some() {
            ui.add_space(8.0);
        }
        if let Some(mut quantity) = fields.quantity {
            let quantity_max = fields
                .quantity_max
                .unwrap_or_else(|| i64::from(i32::MAX))
                .max(1);
            ui.label("Quantity");
            if ui
                .add(egui::DragValue::new(&mut quantity).range(1..=quantity_max))
                .changed()
            {
                actions.push(ItemEditorAction::SetQuantity { quantity });
            }
        }
        actions
    })
    .inner
}

pub(crate) fn displayed_item_power(authored_level: i64) -> i64 {
    if authored_level <= 0 {
        0
    } else {
        authored_level
            .saturating_mul(POWER_PER_LEVEL)
            .max(MINIMUM_POWERED_ITEM_POWER)
    }
}

fn item_power_input_range() -> std::ops::RangeInclusive<i64> {
    0..=MAXIMUM_POWERED_ITEM_POWER
}

pub(crate) fn authored_item_level(display_power: i64) -> Option<i64> {
    if display_power < 0 {
        None
    } else if display_power == 0 {
        Some(0)
    } else {
        Some(display_power.max(MINIMUM_POWERED_ITEM_POWER) / POWER_PER_LEVEL)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefinitionPickerRow<'a> {
    Group(&'a str),
    Definition(&'a DefinitionChoice),
}

fn definition_picker_rows(definitions: &[DefinitionChoice]) -> Vec<DefinitionPickerRow<'_>> {
    let mut first_group = None;
    let mut has_multiple_groups = false;
    for group in definitions
        .iter()
        .filter_map(|definition| definition.group.as_deref())
    {
        match first_group {
            Some(first_group) if first_group != group => {
                has_multiple_groups = true;
                break;
            }
            Some(_) => {}
            None => first_group = Some(group),
        }
    }

    let mut rows = Vec::with_capacity(definitions.len());
    let mut displayed_group = None::<&str>;
    for definition in definitions {
        if definition.group.as_deref() != displayed_group {
            if has_multiple_groups && let Some(group) = definition.group.as_deref() {
                rows.push(DefinitionPickerRow::Group(group));
            }
            displayed_group = definition.group.as_deref();
        }
        rows.push(DefinitionPickerRow::Definition(definition));
    }
    rows
}

#[derive(Clone, Debug)]
pub(crate) struct PlugChoice {
    pub hash: u64,
    pub label: String,
    pub type_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PlugPickerSnapshot {
    pub socket_index: usize,
    pub socket_label: String,
    pub current_hash: Option<u64>,
    pub current_label: String,
    pub native_default: Option<NativePlugDefault>,
    pub native_default_label: Option<String>,
    pub choices: Vec<PlugChoice>,
    pub show_types: bool,
}

pub(crate) fn draw_plug_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash,
    query: &mut String,
    snapshot: &PlugPickerSnapshot,
    height: PickerHeight,
) -> Option<ItemEditorAction> {
    ui.push_id(scope, |ui| {
        let searchable = snapshot.choices.len() > 12;
        if !searchable {
            query.clear();
        }
        let mut selection = None::<Option<u64>>;
        ui.horizontal(|ui| {
            let row_height = ui.spacing().interact_size.y;
            let spacing = ui.spacing().item_spacing.x;
            let available_width = ui.available_width();
            let socket_label_width = (available_width * 0.28).clamp(76.0, 104.0);
            let reset_button_width = 48.0;
            let plug_width =
                (available_width - socket_label_width - reset_button_width - spacing * 2.0)
                    .max(110.0);
            let screen = ui.ctx().screen_rect();
            let popup_width = (plug_width + 140.0)
                .clamp(440.0, 680.0)
                .min((screen.width() - 24.0).max(320.0));

            ui.allocate_ui_with_layout(
                egui::vec2(socket_label_width, row_height),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let mut socket_font = egui::TextStyle::Body.resolve(ui.style());
                    socket_font.size = (socket_font.size - 1.0).max(1.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&snapshot.socket_label).font(socket_font),
                        )
                        .truncate(),
                    )
                },
            )
            .inner
            .on_hover_text(&snapshot.socket_label);
            let popup_id = ui.make_persistent_id("plug-browser");
            let button = ui
                .allocate_ui_with_layout(
                    egui::vec2(plug_width, row_height),
                    egui::Layout::left_to_right(egui::Align::Center)
                        .with_main_align(egui::Align::Min),
                    |ui| {
                        let button = snapshot.current_hash.map_or_else(
                            || egui::Button::new(&snapshot.current_label),
                            |hash| {
                                catalog_button(
                                    ui,
                                    catalog,
                                    hash,
                                    &snapshot.current_label,
                                    (row_height - 6.0).max(16.0),
                                )
                            },
                        );
                        ui.add(
                            button
                                .truncate()
                                .min_size(egui::vec2(plug_width, row_height)),
                        )
                    },
                )
                .inner;
            let button = if let Some(hash) = snapshot.current_hash {
                catalog_item_tooltip(button, catalog, hash)
            } else {
                button
            };
            if button.clicked() {
                ui.memory_mut(|memory| memory.toggle_popup(popup_id));
            }
            let popup_direction = popup_direction(screen, button.rect);
            egui::popup::popup_above_or_below_widget(
                ui,
                popup_id,
                &button,
                popup_direction,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    ui.set_min_width(popup_width);
                    if searchable {
                        ui.add(
                            egui::TextEdit::singleline(query)
                                .hint_text("Search plug name or hex hash…")
                                .desired_width(popup_width - 20.0),
                        );
                        ui.separator();
                    }
                    if ui
                        .selectable_label(snapshot.current_hash.is_none(), "None")
                        .clicked()
                    {
                        selection = Some(None);
                    }
                    let current_is_choice = snapshot.current_hash.is_some_and(|current| {
                        snapshot.choices.iter().any(|choice| choice.hash == current)
                    });
                    if let Some(hash) = snapshot.current_hash
                        && !current_is_choice
                        && ui
                            .selectable_label(
                                true,
                                format!("{}  (custom/current)", catalog.plug_label(hash, true)),
                            )
                            .clicked()
                    {
                        selection = Some(Some(hash));
                    }
                    ui.separator();

                    let needle = query.trim().to_lowercase();
                    let visible = snapshot
                        .choices
                        .iter()
                        .filter(|choice| {
                            needle.is_empty()
                                || choice.label.to_lowercase().contains(&needle)
                                || format_hash(choice.hash).to_lowercase().contains(&needle)
                                || snapshot.show_types
                                    && choice.type_name.to_lowercase().contains(&needle)
                                || catalog.description(choice.hash).is_some_and(|description| {
                                    description.to_lowercase().contains(&needle)
                                })
                        })
                        .collect::<Vec<_>>();
                    if visible.is_empty() {
                        ui.label(
                            egui::RichText::new(if searchable {
                                "No matching plugs found"
                            } else {
                                "No plugs available"
                            })
                            .weak(),
                        );
                    } else {
                        let option_row_height = row_height.max(40.0);
                        let picker_height = picker_list_height(
                            visible.len(),
                            option_row_height,
                            height.min,
                            height.max,
                        );
                        egui::ScrollArea::vertical()
                            .min_scrolled_height(picker_height)
                            .max_height(picker_height)
                            .auto_shrink([false, false])
                            .show_rows(ui, option_row_height, visible.len(), |ui, rows| {
                                for index in rows {
                                    let choice = visible[index];
                                    let type_name = if snapshot.show_types {
                                        choice.type_name.as_str()
                                    } else {
                                        ""
                                    };
                                    let description = catalog
                                        .description(choice.hash)
                                        .map(single_line_text)
                                        .unwrap_or_default();
                                    let secondary = picker_secondary_text(type_name, &description);
                                    let response = draw_catalog_picker_row(
                                        ui,
                                        catalog,
                                        CatalogPickerRow {
                                            hash: choice.hash,
                                            primary: &choice.label,
                                            secondary: (!secondary.is_empty())
                                                .then_some(secondary.as_str()),
                                            icon_size: 28.0,
                                            row_height: option_row_height,
                                            selected: snapshot.current_hash == Some(choice.hash),
                                        },
                                    );
                                    let clicked =
                                        catalog_item_tooltip(response, catalog, choice.hash)
                                            .clicked();
                                    if clicked {
                                        selection = Some(Some(choice.hash));
                                    }
                                }
                            });
                    }
                },
            );
            if selection.is_some() {
                ui.memory_mut(egui::Memory::close_popup);
            }

            let reset_enabled = snapshot
                .native_default
                .is_some_and(|default| snapshot.current_hash != default.value());
            let reset = ui.add_enabled(
                reset_enabled,
                egui::Button::new("Reset").min_size(egui::vec2(reset_button_width, row_height)),
            );
            let reset_tooltip = match snapshot.native_default {
                Some(NativePlugDefault::Plug(hash)) => format!(
                    "Restore this socket's native default: {}",
                    snapshot
                        .native_default_label
                        .as_deref()
                        .map_or_else(|| format!("0x{hash:08X}"), str::to_owned)
                ),
                Some(NativePlugDefault::Empty) => {
                    "Restore this socket's native default: None".to_owned()
                }
                None => "No native default is available for this socket".to_owned(),
            };
            let reset = if reset_enabled {
                reset.on_hover_text(reset_tooltip)
            } else {
                reset.on_disabled_hover_text(reset_tooltip)
            };
            if reset.clicked() {
                selection = snapshot.native_default.map(NativePlugDefault::value);
                ui.memory_mut(egui::Memory::close_popup);
            }
        });
        selection.map(|hash| ItemEditorAction::SetPlug {
            socket_index: snapshot.socket_index,
            hash,
        })
    })
    .inner
}

fn catalog_button<'a>(
    ui: &egui::Ui,
    catalog: &Catalog,
    hash: u64,
    label: &'a str,
    icon_size: f32,
) -> egui::Button<'a> {
    catalog.icon_texture(ui.ctx(), hash).map_or_else(
        || egui::Button::new(label),
        |texture| {
            egui::Button::image_and_text((texture.id(), egui::vec2(icon_size, icon_size)), label)
        },
    )
}

struct CatalogPickerRow<'a> {
    hash: u64,
    primary: &'a str,
    secondary: Option<&'a str>,
    icon_size: f32,
    row_height: f32,
    selected: bool,
}

fn draw_catalog_picker_row(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    row: CatalogPickerRow<'_>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row.row_height),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.style().interact_selectable(&response, row.selected);
    if row.selected || response.hovered() || response.has_focus() {
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }

    const PADDING: f32 = 4.0;
    let icon_size = row.icon_size.min((row.row_height - PADDING * 2.0).max(0.0));
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + PADDING, rect.center().y - icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    if let Some(texture) = catalog.icon_texture(ui.ctx(), row.hash) {
        ui.painter().image(
            texture.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    let text_left = icon_rect.right() + ui.spacing().icon_spacing;
    let text_width = (rect.right() - PADDING - text_left).max(0.0);
    let primary_font = egui::TextStyle::Button.resolve(ui.style());
    let secondary_font = egui::TextStyle::Body.resolve(ui.style());
    let primary_galley = single_line_galley(
        ui,
        row.primary,
        primary_font,
        visuals.text_color(),
        text_width,
    );
    let secondary_galley = row.secondary.map(|secondary| {
        single_line_galley(
            ui,
            secondary,
            secondary_font,
            visuals.text_color(),
            text_width,
        )
    });
    let content_height = primary_galley.size().y
        + secondary_galley
            .as_ref()
            .map_or(0.0, |galley| 1.0 + galley.size().y);
    let mut text_top = rect.center().y - content_height / 2.0;
    ui.painter().galley(
        egui::pos2(text_left, text_top),
        primary_galley,
        visuals.text_color(),
    );
    if let Some(secondary_galley) = secondary_galley {
        text_top += content_height - secondary_galley.size().y;
        ui.painter().galley(
            egui::pos2(text_left, text_top),
            secondary_galley,
            visuals.text_color(),
        );
    }
    response
}

fn single_line_galley(
    ui: &egui::Ui,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts(|fonts| fonts.layout_job(job))
}

fn single_line_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn picker_secondary_text(type_name: &str, description: &str) -> String {
    match (type_name.trim(), description.trim()) {
        ("", description) => description.to_owned(),
        (type_name, "") => type_name.to_owned(),
        (type_name, description) => format!("{type_name} · {description}"),
    }
}

pub(crate) fn catalog_item_tooltip(
    response: egui::Response,
    catalog: &Catalog,
    hash: u64,
) -> egui::Response {
    let name = catalog.display_name(hash);
    let type_name = catalog
        .plug_type_name(hash)
        .filter(|name| !name.trim().is_empty());
    let description = catalog
        .description(hash)
        .filter(|description| !description.trim().is_empty());
    let icon_diagnostic = catalog.icon_diagnostic(hash);
    if name.is_none() && type_name.is_none() && description.is_none() && icon_diagnostic.is_none() {
        response
    } else {
        response.on_hover_ui(|ui| {
            ui.set_max_width(320.0);
            let icon = catalog.icon_texture(ui.ctx(), hash);
            ui.horizontal_top(|ui| {
                if let Some(icon) = icon {
                    ui.add(egui::Image::new(&icon));
                }
                ui.vertical(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if let Some(name) = name {
                            ui.label(egui::RichText::new(name).strong());
                        }
                        ui.horizontal_wrapped(|ui| {
                            if let Some(type_name) = type_name {
                                ui.label(type_name);
                                ui.label(egui::RichText::new("·").small().weak());
                            }
                            ui.label(
                                egui::RichText::new(format_hash(hash))
                                    .small()
                                    .monospace()
                                    .weak(),
                            );
                        });
                    });
                    if let Some(description) = description {
                        ui.separator();
                        ui.label(description);
                    }
                    if let Some(diagnostic) = icon_diagnostic {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("Icon: {diagnostic}"))
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        );
                    }
                });
            });
        })
    }
}

pub(crate) fn draw_responsive_item_cards<T>(
    ui: &mut egui::Ui,
    items: &[T],
    minimum_card_width: f32,
    maximum_card_width: f32,
    mut draw: impl FnMut(&mut egui::Ui, &T),
) {
    let Some((column_count, grid_width)) = responsive_item_card_layout(
        ui.available_width(),
        ui.spacing().item_spacing.x,
        items.len(),
        minimum_card_width,
        maximum_card_width,
    ) else {
        return;
    };
    ui.scope(|ui| {
        ui.set_width(grid_width);
        ui.columns(column_count, |columns| {
            let mut counts = vec![0_usize; column_count];
            for (index, item) in items.iter().enumerate() {
                let column = index % column_count;
                if counts[column] != 0 {
                    columns[column].add_space(3.0);
                }
                draw(&mut columns[column], item);
                counts[column] += 1;
            }
        });
    });
}

fn responsive_item_card_layout(
    available_width: f32,
    spacing: f32,
    item_count: usize,
    minimum_card_width: f32,
    maximum_card_width: f32,
) -> Option<(usize, f32)> {
    if item_count == 0 {
        return None;
    }

    let available_width = available_width.max(0.0);
    let minimum_card_width = minimum_card_width.max(1.0);
    let maximum_card_width = maximum_card_width.max(minimum_card_width);
    let column_count =
        (((available_width + spacing) / (minimum_card_width + spacing)).floor() as usize).max(1);
    let total_spacing = spacing * column_count.saturating_sub(1) as f32;
    let card_width =
        ((available_width - total_spacing) / column_count as f32).clamp(0.0, maximum_card_width);
    Some((
        column_count,
        card_width * column_count as f32 + total_spacing,
    ))
}

pub(crate) fn picker_list_height(
    row_count: usize,
    row_height: f32,
    min_height: f32,
    max_height: f32,
) -> f32 {
    let content_height = row_count as f32 * row_height;
    if content_height < min_height {
        content_height.max(row_height)
    } else {
        content_height.min(max_height)
    }
}

fn popup_direction(screen: egui::Rect, anchor: egui::Rect) -> egui::AboveOrBelow {
    let room_above = (anchor.top() - screen.top()).max(0.0);
    let room_below = (screen.bottom() - anchor.bottom()).max(0.0);
    if room_below >= room_above {
        egui::AboveOrBelow::Below
    } else {
        egui::AboveOrBelow::Above
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_header_text_is_anchored_to_the_left_edge() {
        egui::__run_test_ui(|ui| {
            ui.set_width(320.0);
            let expected_left = ui.cursor().left();
            let expected_right = ui.max_rect().right();
            let mut title = egui::text::LayoutJob::default();
            title.append("Battle Scar", 0.0, egui::TextFormat::default());
            let mut title_hash = egui::text::LayoutJob::default();
            title_hash.append("0x45ABCDEF", 0.0, egui::TextFormat::default());
            let mut subtitle = egui::text::LayoutJob::default();
            subtitle.append(
                "Pulse Rifle | Equipped: Kinetic Slot",
                0.0,
                egui::TextFormat::default(),
            );
            let mut metadata = egui::text::LayoutJob::default();
            metadata.append("0x4000000000000001", 0.0, egui::TextFormat::default());

            let response = ui
                .allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ITEM_HEADER_WITH_METADATA_ROW_HEIGHT),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| draw_item_header_text(ui, title, title_hash, subtitle, metadata),
                )
                .inner;

            assert!((response.rect.left() - expected_left).abs() < 0.5);
            assert!((response.rect.right() - expected_right).abs() < 0.5);
        });
    }

    #[test]
    fn item_header_title_uses_its_natural_text_height() {
        egui::__run_test_ui(|ui| {
            ui.set_width(320.0);
            let mut font_id = egui::TextStyle::Body.resolve(ui.style());
            font_id.size += ITEM_HEADER_TITLE_SIZE_DELTA;
            let format = egui::TextFormat {
                font_id,
                ..Default::default()
            };
            let mut title = egui::text::LayoutJob::default();
            title.append("Battle Scar", 0.0, format.clone());
            let mut hash = egui::text::LayoutJob::default();
            hash.append("0x45ABCDEF", 0.0, format);
            let expected_height = ui.fonts(|fonts| {
                fonts
                    .layout_job(title.clone())
                    .size()
                    .y
                    .max(fonts.layout_job(hash.clone()).size().y)
            });

            let response = draw_item_header_title(ui, title, hash);

            assert!((response.rect.height() - expected_height).abs() < 0.5);
        });
    }

    #[test]
    fn responsive_item_cards_share_the_same_bounded_width_rules() {
        const STANDARD_MIN: f32 = 335.0;
        const STANDARD_MAX: f32 = 390.0;
        assert_eq!(
            responsive_item_card_layout(900.0, 8.0, 0, STANDARD_MIN, STANDARD_MAX),
            None
        );

        let (narrow_columns, narrow_width) =
            responsive_item_card_layout(660.0, 8.0, 3, STANDARD_MIN, STANDARD_MAX).unwrap();
        assert_eq!(narrow_columns, 1);
        assert!((narrow_width - STANDARD_MAX).abs() < 0.5);

        let (default_columns, default_width) =
            responsive_item_card_layout(710.0, 8.0, 8, STANDARD_MIN, STANDARD_MAX).unwrap();
        assert_eq!(default_columns, 2);
        let default_card_width = (default_width - 8.0) / 2.0;
        assert!((STANDARD_MIN..=STANDARD_MAX).contains(&default_card_width));

        let (wide_columns, wide_width) =
            responsive_item_card_layout(1_400.0, 8.0, 8, STANDARD_MIN, STANDARD_MAX).unwrap();
        assert_eq!(wide_columns, 4);
        let card_width = (wide_width - 24.0) / 4.0;
        assert!((STANDARD_MIN..=STANDARD_MAX).contains(&card_width));

        let (compact_columns, compact_width) =
            responsive_item_card_layout(950.0, 8.0, 8, 285.0, 315.0).unwrap();
        let (standard_columns, standard_width) =
            responsive_item_card_layout(950.0, 8.0, 8, STANDARD_MIN, STANDARD_MAX).unwrap();
        let (wide_columns, wide_width) =
            responsive_item_card_layout(950.0, 8.0, 8, 430.0, 520.0).unwrap();
        assert_eq!(compact_columns, 3);
        assert_eq!(standard_columns, 2);
        assert_eq!(wide_columns, 2);
        let compact_card_width = (compact_width - 16.0) / 3.0;
        let standard_card_width = (standard_width - 8.0) / 2.0;
        let wide_card_width = (wide_width - 8.0) / 2.0;
        assert!(compact_card_width < standard_card_width);
        assert!(standard_card_width < wide_card_width);
    }

    #[test]
    fn picker_secondary_text_is_compact_and_single_line() {
        assert_eq!(
            single_line_text("First line\n  second\tline"),
            "First line second line"
        );
        assert_eq!(
            picker_secondary_text("Trait", "A short description"),
            "Trait · A short description"
        );
        assert_eq!(
            picker_secondary_text("", "A short description"),
            "A short description"
        );

        egui::__run_test_ui(|ui| {
            let galley = single_line_galley(
                ui,
                "A deliberately long description that cannot fit in the picker row",
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().text_color(),
                80.0,
            );
            assert_eq!(galley.rows.len(), 1);
            assert!(galley.size().x <= 80.5);
        });
    }

    #[test]
    fn definition_picker_omits_a_redundant_single_group_heading() {
        let definitions = [
            DefinitionChoice {
                hash: 1,
                name: "First".into(),
                type_name: "Pulse Rifle".into(),
                group: Some("Kinetic weapons".into()),
            },
            DefinitionChoice {
                hash: 2,
                name: "Second".into(),
                type_name: "Hand Cannon".into(),
                group: Some("Kinetic weapons".into()),
            },
        ];

        assert_eq!(
            definition_picker_rows(&definitions),
            vec![
                DefinitionPickerRow::Definition(&definitions[0]),
                DefinitionPickerRow::Definition(&definitions[1]),
            ]
        );

        let mut multiple_groups = definitions.to_vec();
        multiple_groups[1].group = Some("Energy weapons".into());
        assert!(matches!(
            definition_picker_rows(&multiple_groups).first(),
            Some(DefinitionPickerRow::Group("Kinetic weapons"))
        ));
    }

    #[test]
    fn feather_action_icons_use_matching_compact_buttons() {
        egui::__run_test_ui(|ui| {
            ui.horizontal(|ui| {
                for response in [
                    draw_trash_button(ui, true, "Delete item"),
                    draw_lock_button(ui, true, "Lock item"),
                    draw_unlock_button(ui, true, "Unlock item"),
                ] {
                    assert!(
                        (response.rect.width() - response.rect.height()).abs() < 0.5,
                        "icon button was {} by {}",
                        response.rect.width(),
                        response.rect.height()
                    );
                    assert!(response.rect.height() >= 12.0);
                }
            });
        });
    }

    #[test]
    fn long_item_header_keeps_trailing_action_inside_available_width() {
        egui::__run_test_ui(|ui| {
            ui.set_width(320.0);
            let expected_right = ui.max_rect().right();
            let mut trailing_rect = None;
            let mut trailing_right = None;
            let response = draw_item_header_with_trailing(
                ui,
                ItemHeader {
                    label: Some("Equipped · Class item"),
                    soid: Some("0x4000000000000001"),
                    definition: DefinitionSummary::Known {
                        name: "An intentionally very long installed item definition name",
                        hash: "0x12345678",
                        type_name: "Class item",
                    },
                    icon: None,
                    fill: muted_item_header_fill(ui),
                    valid: false,
                    invalid_message: "not valid for this character inventory",
                },
                |ui| {
                    trailing_right = Some(ui.max_rect().right());
                    trailing_rect = Some(ui.button("Remove").rect);
                },
            );

            let trailing_rect = trailing_rect.expect("trailing action should be drawn");
            let trailing_right = trailing_right.expect("trailing column should be drawn");
            assert!((trailing_rect.right() - trailing_right).abs() < 0.5);
            assert!(trailing_rect.right() >= expected_right - 4.5);
            assert!(trailing_rect.right() <= expected_right + 0.5);
            assert!(response.rect.contains(trailing_rect.center()));
            assert!(ui.min_rect().right() <= expected_right + 0.5);
        });
    }

    #[test]
    fn authored_levels_display_as_in_game_power() {
        assert_eq!(displayed_item_power(0), 0);
        assert_eq!(displayed_item_power(1), 750);
        assert_eq!(displayed_item_power(74), 750);
        assert_eq!(displayed_item_power(75), 750);
        assert_eq!(displayed_item_power(106), 1_060);
        assert_eq!(
            displayed_item_power(i64::from(i32::MAX)),
            i64::from(i32::MAX) * 10
        );
    }

    #[test]
    fn entered_power_snaps_down_and_converts_to_authored_level() {
        assert_eq!(authored_item_level(-1), None);
        assert_eq!(authored_item_level(0), Some(0));
        assert_eq!(authored_item_level(1), Some(75));
        assert_eq!(authored_item_level(749), Some(75));
        assert_eq!(authored_item_level(750), Some(75));
        assert_eq!(authored_item_level(759), Some(75));
        assert_eq!(authored_item_level(760), Some(76));
        assert_eq!(authored_item_level(1_060), Some(106));
    }

    #[test]
    fn power_input_range_keeps_unpowered_items_editable() {
        let range = item_power_input_range();
        assert!(range.contains(&0));
        assert!(range.contains(&MINIMUM_POWERED_ITEM_POWER));
        assert!(range.contains(&MAXIMUM_POWERED_ITEM_POWER));
        assert!(!range.contains(&-1));
        assert!(!range.contains(&(MAXIMUM_POWERED_ITEM_POWER + 1)));
    }

    #[test]
    fn drawing_power_does_not_rewrite_existing_out_of_range_values() {
        egui::__run_test_ui(|ui| {
            let actions = draw_level_and_quantity(
                ui,
                "out-of-range-power",
                NumericItemFields {
                    level: Some(200),
                    quantity: None,
                    quantity_max: None,
                },
            );
            assert!(actions.is_empty());
        });
    }
}
