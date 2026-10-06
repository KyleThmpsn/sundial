//! Visual building blocks shared by every inspector page: the definition header card, kind
//! chips, counted sections and property grids that leave out empty values.

use std::hash::Hash;

use eframe::egui;

use crate::{
    catalog::Catalog,
    hash::{format_hash_hex, format_hash_hex_and_decimal},
};

use super::metadata::{draw_catalog_hash_link, draw_named_catalog_hash_link};

/// Width of the label column in every property grid, so stacked grids line up.
pub(in crate::app) const LABEL_WIDTH: f32 = 176.0;
const ICON_SIZE: f32 = 64.0;
const KIND_CHIP_PADDING: egui::Vec2 = egui::vec2(5.0, 1.0);

/// One step of a header breadcrumb. Steps with a hash open that definition.
pub(in crate::app) struct Crumb {
    pub(in crate::app) hash: Option<u64>,
    pub(in crate::app) name: String,
}

/// What the header card says about the inspected definition.
pub(in crate::app) struct Header<'a> {
    pub(in crate::app) title: &'a str,
    pub(in crate::app) kind: &'a str,
    pub(in crate::app) hash: u64,
    /// The item whose icon leads the card, the page's own or a collectible's item.
    pub(in crate::app) icon: u64,
    pub(in crate::app) subtitle: Option<&'a str>,
    pub(in crate::app) path: Vec<Crumb>,
}

/// The card at the top of every page: icon, breadcrumb, name, kind, hash and facts.
pub(in crate::app) fn header(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    header: &Header<'_>,
    facts: impl FnOnce(&mut egui::Ui),
) {
    header_card(ui, |ui| header_body(ui, catalog, header, facts));
}

/// A [`header`] with buttons at its top right.
pub(in crate::app) fn header_with_actions(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    header: &Header<'_>,
    facts: impl FnOnce(&mut egui::Ui),
    actions: impl FnOnce(&mut egui::Ui),
) {
    header_card(ui, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            actions(ui);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                header_body(ui, catalog, header, facts);
            });
        });
    });
}

/// The presentation-node breadcrumb above a node, collectible or record.
pub(in crate::app) fn presentation_crumbs(catalog: &Catalog, hash: u64) -> Vec<Crumb> {
    catalog
        .presentation_path(hash)
        .into_iter()
        .map(|node| Crumb {
            hash: Some(node.hash),
            name: node.name.clone(),
        })
        .collect()
}

fn header_card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

fn header_body(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    header: &Header<'_>,
    facts: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        if let Some(icon) = catalog.icon_texture(ui.ctx(), header.icon) {
            ui.add(
                egui::Image::new(&icon)
                    .fit_to_exact_size(egui::Vec2::splat(ICON_SIZE))
                    .maintain_aspect_ratio(true)
                    .corner_radius(3),
            );
            ui.add_space(12.0);
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if !header.path.is_empty() {
                breadcrumb(ui, &header.path);
            }
            crate::app::item_editor::catalog_item_tooltip(
                ui.label(
                    crate::app::ui::destiny_text(ui, header.title)
                        .strong()
                        .size(21.0)
                        .color(ui.visuals().strong_text_color()),
                ),
                catalog,
                header.hash,
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                kind_chip(ui, header.kind);
                if let Some(subtitle) = header.subtitle.filter(|text| !text.is_empty()) {
                    ui.label(egui::RichText::new(subtitle).color(muted(ui)));
                }
                hash_with_copy(ui, header.hash);
            });
            ui.add_space(2.0);
            // A page with no facts leaves no empty row behind.
            let row_height = ui.spacing().interact_size.y;
            ui.spacing_mut().interact_size.y = 0.0;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().interact_size.y = row_height;
                ui.spacing_mut().item_spacing = egui::vec2(14.0, 4.0);
                facts(ui);
            });
        });
    });
}

/// egui's `Small` style is 9 points, too small to read the inspector's captions, counts and
/// column headings in, so the inspector sets it a step under body text.
pub(in crate::app) fn readable_small_text(ui: &mut egui::Ui) {
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    ui.style_mut().text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(body - 1.0),
    );
}

fn breadcrumb(ui: &mut egui::Ui, path: &[Crumb]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (index, crumb) in path.iter().enumerate() {
            if index > 0 {
                ui.label(egui::RichText::new("›").color(muted(ui)));
            }
            let text = egui::RichText::new(&crumb.name);
            match crumb.hash {
                Some(hash) => {
                    let response = ui.add(
                        egui::Button::new(text.color(ui.visuals().hyperlink_color)).frame(false),
                    );
                    if response.clicked() {
                        super::request_definition(ui.ctx(), hash);
                    }
                    response.on_hover_text(format!("Open {}", format_hash_hex(hash)));
                }
                None => {
                    ui.label(text.color(muted(ui)));
                }
            }
        }
    });
}

/// A badge naming what kind of definition a hash is, in the app's badge style
/// (`draw_item_badge`). Painted at the text's own size, so a tall row cannot stretch it.
pub(in crate::app) fn kind_chip(ui: &mut egui::Ui, kind: &str) -> egui::Response {
    let visuals = ui.visuals();
    let (fill, stroke, color) = (
        visuals.faint_bg_color,
        visuals.widgets.noninteractive.bg_stroke,
        visuals.text_color(),
    );
    let galley = kind_chip_galley(ui, kind);
    let padding = KIND_CHIP_PADDING;
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + 2.0 * padding, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect(
            rect,
            egui::CornerRadius::same(3),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + padding, galley, color);
    }
    response
}

/// The width a [`kind_chip`] takes for this kind.
pub(in crate::app) fn kind_chip_width(ui: &egui::Ui, kind: &str) -> f32 {
    kind_chip_galley(ui, kind).size().x + 2.0 * KIND_CHIP_PADDING.x
}

fn kind_chip_galley(ui: &egui::Ui, kind: &str) -> std::sync::Arc<egui::Galley> {
    egui::WidgetText::from(egui::RichText::new(kind).size(11.0)).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Small,
    )
}

/// A kind chip inside a fixed-width slot, so names after it line up down a list.
pub(in crate::app) fn kind_chip_column(ui: &mut egui::Ui, kind: &str, width: f32) {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            kind_chip(ui, kind);
        },
    );
}

/// The monospace hash with a copy button beside it.
pub(in crate::app) fn hash_with_copy(ui: &mut egui::Ui, hash: u64) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.label(
            egui::RichText::new(format_hash_hex(hash))
                .monospace()
                .color(muted(ui)),
        )
        .on_hover_text(format_hash_hex_and_decimal(hash));
        copy_button(ui, format_hash_hex(hash), "Copy Hash");
    });
}

/// A frameless copy icon.
pub(in crate::app) fn copy_button(ui: &mut egui::Ui, text: String, label: &str) {
    let response = ui
        .add(
            egui::Button::new(egui::RichText::new(egui_phosphor::regular::COPY).color(muted(ui)))
                .frame(false)
                .small(),
        )
        .on_hover_text(label);
    if response.clicked() {
        ui.ctx().copy_text(text);
    }
}

/// A label and value pair for the header's fact row. Empty values are left out.
pub(in crate::app) fn fact(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    let value = value.into();
    if is_empty_value(&value) {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(egui::RichText::new(label).small().color(muted(ui)));
        ui.label(egui::RichText::new(value).color(ui.visuals().strong_text_color()));
    });
}

/// A header fact whose value opens another definition.
pub(in crate::app) fn fact_link(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    label: &str,
    hash: u64,
    value: impl Into<String>,
) {
    if hash == 0 || hash == u64::from(u32::MAX) {
        fact(ui, label, value);
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(egui::RichText::new(label).small().color(muted(ui)));
        draw_named_catalog_hash_link(ui, catalog, hash, value);
    });
}

/// A collapsible section with its item count beside the title.
pub(in crate::app) fn section<R>(
    ui: &mut egui::Ui,
    id_salt: impl Hash,
    title: &str,
    count: Option<usize>,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    ui.add_space(10.0);
    let mut job = egui::text::LayoutJob::default();
    let font = egui::TextStyle::Body.resolve(ui.style());
    job.append(
        title,
        0.0,
        egui::TextFormat::simple(font.clone(), ui.visuals().strong_text_color()),
    );
    if let Some(count) = count {
        job.append(
            &count.to_string(),
            8.0,
            egui::TextFormat::simple(font, muted(ui)),
        );
    }
    egui::CollapsingHeader::new(job)
        .id_salt(("inspector_section", id_salt))
        .default_open(default_open)
        .show(ui, |ui| {
            ui.add_space(2.0);
            add_contents(ui)
        })
        .body_returned
}

/// A row of tabs in one recessed frame. Tabs are `SelectableLabel`s added by the caller.
pub(in crate::app) fn tab_bar(ui: &mut egui::Ui, add_tabs: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.spacing_mut().button_padding = egui::vec2(12.0, 4.0);
                add_tabs(ui);
            });
        });
}

/// A label with an optional count muted after it, for tabs and list choices.
pub(in crate::app) fn tab_text(
    ui: &egui::Ui,
    label: &str,
    count: Option<usize>,
) -> egui::WidgetText {
    let Some(count) = count else {
        return label.into();
    };
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        label,
        0.0,
        egui::TextFormat::simple(font.clone(), egui::Color32::PLACEHOLDER),
    );
    job.append(
        &count.to_string(),
        6.0,
        egui::TextFormat::simple(font, muted(ui)),
    );
    job.into()
}

/// A plain heading inside a section, for grouping properties without another fold.
pub(in crate::app) fn subheading(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).small().strong().color(muted(ui)));
    ui.add_space(2.0);
}

/// A two-column grid of labels and values. Rows with an empty value are left out.
pub(in crate::app) fn properties<R>(
    ui: &mut egui::Ui,
    id_salt: impl Hash,
    add_rows: impl FnOnce(&mut Properties<'_>) -> R,
) -> R {
    egui::Grid::new(("inspector_properties", id_salt))
        .num_columns(2)
        .spacing([16.0, 5.0])
        .show(ui, |ui| add_rows(&mut Properties { ui }))
        .inner
}

/// Rows of a [`properties`] grid.
pub(in crate::app) struct Properties<'a> {
    pub(in crate::app) ui: &'a mut egui::Ui,
}

impl Properties<'_> {
    fn label(&mut self, label: &str) {
        let height = self.ui.spacing().interact_size.y;
        self.ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.add(egui::Label::new(egui::RichText::new(label).color(muted(ui))).truncate());
            },
        );
    }

    /// A text value. Empty values and `<placeholder>` text are left out.
    pub(in crate::app) fn text(&mut self, label: &str, value: impl Into<String>) {
        let value = value.into();
        if is_empty_value(&value) {
            return;
        }
        self.label(label);
        self.ui.add(egui::Label::new(value).wrap());
        self.ui.end_row();
    }

    /// A monospace value, for numbers, codes and indices.
    pub(in crate::app) fn mono(&mut self, label: &str, value: impl Into<String>) {
        let value = value.into();
        if is_empty_value(&value) {
            return;
        }
        self.label(label);
        self.ui.label(egui::RichText::new(value).monospace());
        self.ui.end_row();
    }

    /// A named link to another definition. Left out when the hash is absent.
    pub(in crate::app) fn link(
        &mut self,
        label: &str,
        catalog: &Catalog,
        hash: u64,
        name: impl Into<String>,
    ) {
        if hash == 0 || hash == u64::from(u32::MAX) {
            return;
        }
        self.label(label);
        draw_named_catalog_hash_link(self.ui, catalog, hash, name);
        self.ui.end_row();
    }

    /// A hash shown as hex and decimal that opens its definition.
    pub(in crate::app) fn hash(&mut self, label: &str, catalog: &Catalog, hash: u64) {
        if hash == 0 || hash == u64::from(u32::MAX) {
            return;
        }
        self.label(label);
        draw_catalog_hash_link(self.ui, catalog, hash, format_hash_hex_and_decimal(hash));
        self.ui.end_row();
    }

    /// Any value drawn by the caller.
    pub(in crate::app) fn custom<R>(
        &mut self,
        label: &str,
        add_value: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.label(label);
        let value = add_value(self.ui);
        self.ui.end_row();
        value
    }
}

/// A muted one-line empty state.
pub(in crate::app) fn empty_state(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).italics().color(muted(ui)));
}

/// Secondary text colour used for labels, counts and hashes.
pub(in crate::app) fn muted(ui: &egui::Ui) -> egui::Color32 {
    crate::app::ui::secondary_text_color(ui)
}

fn is_empty_value(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || (value.starts_with('<') && value.ends_with('>'))
}
