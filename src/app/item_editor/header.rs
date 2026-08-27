use super::*;

const ITEM_HEADER_TITLE_SIZE_DELTA: f32 = 2.0;
const ITEM_HEADER_ICON_SIZE: f32 = 48.0;
const ITEM_HEADER_ROW_HEIGHT: f32 = 48.0;
const ITEM_HEADER_WITH_METADATA_ROW_HEIGHT: f32 = 54.0;

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
    inspection_context: Option<DefinitionInspectionContext>,
    mut header: ItemHeader<'_>,
    trailing: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let header_rect = egui::Rect::from_min_size(
        ui.next_widget_position(),
        egui::vec2(ui.available_width(), ITEM_HEADER_WITH_METADATA_ROW_HEIGHT),
    );
    header.icon = hash
        .filter(|_| ui.is_rect_visible(header_rect))
        .and_then(|hash| catalog.icon_texture(ui.ctx(), hash));
    let response = draw_item_header_with_trailing(ui, header, trailing);
    let Some(hash) = hash else {
        return response;
    };
    let card_response = ui.interact(
        response.rect,
        response.id.with(("item_card", hash)),
        egui::Sense::click(),
    );
    let tooltip_response = catalog_item_tooltip(card_response.clone(), catalog, hash);

    let mut font = egui::TextStyle::Monospace.resolve(ui.style());
    font.size += ITEM_HEADER_TITLE_SIZE_DELTA;
    let hash_width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(format_hash_hex(hash), font, ui.visuals().text_color())
            .size()
            .x
    });
    let hash_rect = egui::Rect::from_min_max(
        egui::pos2(
            (response.rect.right() - hash_width - 6.0).max(response.rect.left()),
            response.rect.top(),
        ),
        egui::pos2(response.rect.right(), response.rect.top() + 24.0),
    );
    let hash_response = ui
        .interact(
            hash_rect.expand2(egui::vec2(4.0, 2.0)),
            response.id.with(("definition_hash", hash)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Inspect definition");
    if hash_response.clicked() {
        if let Some(context) = inspection_context {
            request_hash_inspection_with_context(ui.ctx(), hash, context);
        } else {
            request_hash_inspection(ui.ctx(), hash);
        }
    }

    response | card_response | tooltip_response
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
    let mut title_hash_display_text = String::new();
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
        color: weak_color,
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
            hash_display_text,
            type_name: definition_type_name,
        } => {
            append_header_text(&mut title_job, &mut title_text, name, 0.0, strong);
            append_header_text(
                &mut title_hash_job,
                &mut title_hash_display_text,
                hash_display_text,
                0.0,
                title_hash_weak,
            );
            type_name = (!definition_type_name.trim().is_empty()).then_some(definition_type_name);
        }
        DefinitionSummary::Unknown { hash_display_text } => {
            append_header_text(
                &mut title_job,
                &mut title_text,
                "Unknown item",
                0.0,
                title_error,
            );
            append_header_text(
                &mut title_hash_job,
                &mut title_hash_display_text,
                hash_display_text,
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
