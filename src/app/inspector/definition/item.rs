use super::*;
use crate::app::inspector::DefinitionInspectionContext;
use crate::app::item_editor::displayed_item_power;
use tiger_pkg::TagHash;

mod identity;
mod overview;
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
    Sockets,
    Runtime,
    Technical,
    Related,
}

impl ItemPage {
    pub(super) const ALL: [Self; 5] = [
        Self::Overview,
        Self::Sockets,
        Self::Runtime,
        Self::Technical,
        Self::Related,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
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

pub(super) fn draw_hash_item_matches(
    ui: &mut egui::Ui,
    content: ItemInspection<'_>,
    runtime: &mut super::runtime::RuntimeInspectionState,
    page: ItemPage,
) {
    let matches = content.matches;
    if matches.item_package_metadata.is_some() || matches.item.is_some() {
        ui.add_space(8.0);
        draw_hash_item_identity_summary(
            ui,
            content.catalog,
            content.hash,
            content.resolved_name,
            matches.item,
            matches.item_package_metadata,
            matches.inventory_metadata,
        );
        match page {
            ItemPage::Overview => draw_overview(ui, &content),
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
