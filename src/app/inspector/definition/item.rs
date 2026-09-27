use super::*;
use crate::app::inspector::DefinitionInspectionContext;
use crate::app::item_editor::displayed_item_power;
use tiger_pkg::TagHash;

mod identity;
mod overview;
mod preview;
mod sockets;
mod technical;
use identity::*;
use overview::*;
use sockets::*;
use technical::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum ItemPage {
    #[default]
    Overview,
    Preview,
    Sockets,
    Runtime,
    Technical,
    Related,
}

impl ItemPage {
    pub(super) const ALL: [Self; 6] = [
        Self::Overview,
        Self::Preview,
        Self::Sockets,
        Self::Runtime,
        Self::Technical,
        Self::Related,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Preview => "Appearance",
            Self::Sockets => "Sockets",
            Self::Runtime => "Runtime",
            Self::Technical => "Technical",
            Self::Related => "Related Records",
        }
    }
}

pub(super) struct ItemInspection<'a> {
    pub(super) catalog: &'a Catalog,
    pub(super) hash: u64,
    pub(super) resolved_name: &'a Option<String>,
    pub(super) matches: &'a CatalogHashMatches<'a>,
    pub(super) source_context: Option<&'a DefinitionInspectionContext>,
}

/// The item's header card and description.
pub(super) fn draw_item_header(ui: &mut egui::Ui, content: &ItemInspection<'_>) {
    identity::draw_header(ui, content);
}

/// Tabs with something to show for this item, in order.
pub(super) fn available_pages(content: &ItemInspection<'_>, related_count: usize) -> Vec<ItemPage> {
    ItemPage::ALL
        .into_iter()
        .filter(|page| page_available(*page, content, related_count))
        .collect()
}

fn page_available(page: ItemPage, content: &ItemInspection<'_>, related_count: usize) -> bool {
    let metadata = content.matches.item_package_metadata;
    match page {
        ItemPage::Overview => true,
        ItemPage::Preview => metadata.is_some_and(|metadata| {
            metadata
                .art_arrangements
                .iter()
                .any(|row| row.arrangement != u16::MAX)
        }),
        ItemPage::Sockets => content
            .matches
            .item
            .is_some_and(|item| !item.sockets.is_empty() || !item.default_plugs.is_empty()),
        ItemPage::Runtime => metadata.is_some_and(|metadata| {
            metadata
                .weapon_pattern_index
                .is_some_and(|index| index != u16::MAX)
                || !metadata.sandbox_perks.is_empty()
        }),
        ItemPage::Technical => metadata.is_some(),
        ItemPage::Related => related_count > 0,
    }
}

/// A segmented row of tabs. The Related Records tab carries its count.
pub(super) fn draw_page_tabs(
    ui: &mut egui::Ui,
    pages: &[ItemPage],
    page: &mut ItemPage,
    related_count: usize,
) {
    egui::Frame::NONE
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.spacing_mut().button_padding = egui::vec2(12.0, 4.0);
                for candidate in pages {
                    let text = page_tab_text(ui, *candidate, related_count);
                    if ui
                        .add(egui::SelectableLabel::new(*page == *candidate, text))
                        .clicked()
                    {
                        *page = *candidate;
                    }
                }
            });
        });
}

fn page_tab_text(ui: &egui::Ui, page: ItemPage, related_count: usize) -> egui::WidgetText {
    if page != ItemPage::Related {
        return page.label().into();
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        page.label(),
        0.0,
        egui::TextFormat::simple(font.clone(), egui::Color32::PLACEHOLDER),
    );
    job.append(
        &related_count.to_string(),
        6.0,
        egui::TextFormat::simple(font, crate::app::inspector::look::muted(ui)),
    );
    job.into()
}

pub(super) fn draw_hash_item_matches(
    ui: &mut egui::Ui,
    content: ItemInspection<'_>,
    runtime: &mut super::runtime::RuntimeInspectionState,
    page: ItemPage,
) {
    let matches = content.matches;
    if matches.item_package_metadata.is_some() || matches.item.is_some() {
        ui.add_space(4.0);
        if page == ItemPage::Preview {
            preview::draw(ui, &content, true, false);
        }
        match page {
            ItemPage::Overview => draw_overview(ui, &content),
            ItemPage::Preview => {}
            ItemPage::Sockets => draw_sockets_page(ui, &content),
            ItemPage::Runtime => draw_runtime_page(ui, &content, runtime),
            ItemPage::Technical => draw_technical_page(ui, &content, runtime),
            ItemPage::Related => {}
        }
    } else {
        if let Some(context) = content.source_context {
            draw_hash_item_source_comparison(
                ui,
                content.catalog,
                content.hash,
                matches.item,
                context,
            );
        }
        draw_hash_item_stat_matches(ui, content.catalog, matches);
        if let Some(metadata) = matches.inventory_metadata {
            hash_metadata_section(ui, "Inventory Metadata", false, |ui| {
                draw_hash_inventory_placement_summary(ui, metadata);
                draw_hash_inventory_capacity_summary(ui, metadata);
            });
        }
        if !matches.bucket_items.is_empty() {
            draw_hash_inventory_bucket(ui, content.catalog, content.hash, &matches.bucket_items);
        }
    }
}

pub(super) fn draw_preview_button(ui: &mut egui::Ui, content: ItemInspection<'_>) {
    preview::draw(ui, &content, false, true);
}
