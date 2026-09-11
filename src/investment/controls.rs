//! Shared authoring controls. UI adaptation stays separate from catalog queries.
use super::{InvestmentCatalog, PlugSelectionMode, WeaponDonorSummary};
use crate::app::authoring_bridge;
use eframe::egui;
use std::{hash::Hash, path::Path};

/// Shared tooltip title typography for Sundial and Parhelion.
pub fn tooltip_title(ui: &mut egui::Ui, title: impl Into<String>) -> egui::Response {
    crate::ui_help::tooltip_title(ui, title)
}

/// Native asset choices use the same row layout as investment choices.
pub fn draw_asset_choice_row(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    selected: bool,
) -> egui::Response {
    authoring_bridge::draw_asset_choice_row(ui, name, detail, selected)
}

/// Consistent loading and build progress appearance across both applications.
pub fn progress_bar(fraction: f32) -> egui::ProgressBar {
    egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).corner_radius(egui::CornerRadius::same(3))
}

/// Content rendered by Sundial's shared full-window catalog loading surface.
#[derive(Clone, Copy, Debug)]
pub struct CatalogLoadingView<'a> {
    pub product_name: &'a str,
    pub version: &'a str,
    pub message: &'a str,
    pub completed: usize,
    pub total: usize,
    pub source_path: Option<&'a Path>,
}

/// Draws the centered loading surface used while Sundial-backed catalogs are unavailable.
#[allow(clippy::cast_precision_loss)]
pub fn draw_catalog_loading_view(
    ctx: &egui::Context,
    logo: &egui::TextureHandle,
    view: CatalogLoadingView<'_>,
) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let top_space = ((ui.available_height() - 440.0) / 2.0).max(16.0);
        ui.add_space(top_space);
        ui.vertical_centered(|ui| {
            egui::Frame::group(ui.style())
                .inner_margin(28.0)
                .show(ui, |ui| {
                    ui.set_width(500.0_f32.min(ui.available_width()));
                    ui.vertical_centered(|ui| {
                        ui.image((logo.id(), egui::vec2(72.0, 72.0)));
                        ui.heading(view.product_name);
                        ui.label(egui::RichText::new(view.version).weak());
                        ui.add_space(18.0);
                        ui.spinner();
                        ui.strong(view.message);
                        ui.add_space(10.0);
                        let fraction = if view.total == 0 {
                            0.0
                        } else {
                            view.completed as f32 / view.total as f32
                        };
                        let mut bar = progress_bar(fraction).desired_width(400.0);
                        if view.total > 0 {
                            bar = bar.show_percentage();
                        } else {
                            bar = bar.animate(true);
                        }
                        ui.add(bar);
                        if let Some(path) = view.source_path {
                            ui.add_space(10.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(path.display().to_string())
                                        .text_style(egui::TextStyle::Body),
                                )
                                .wrap(),
                            );
                        }
                    });
                });
        });
    });
}

/// Optional empty choice rendered inside Sundial's native weapon browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponDonorPickerClearChoice<'a> {
    pub label: &'a str,
    pub tooltip: &'a str,
    pub selected: bool,
}

/// Current selection and optional empty state for Sundial's shared weapon browser.
#[derive(Clone, Copy)]
pub struct WeaponDonorPickerOptions<'a> {
    pub selected_hash: Option<u32>,
    pub selected_label: &'a str,
    pub header_label: Option<&'a str>,
    pub action_label: &'a str,
    /// Optional authored preview replacing the catalog icon on the selected donor card.
    pub selected_icon_override: Option<&'a egui::TextureHandle>,
    /// Optional compact action rendered on the selected donor card without opening the picker.
    pub secondary_action_label: Option<&'a str>,
    pub clear: Option<WeaponDonorPickerClearChoice<'a>>,
}

/// A selection made through Sundial's native icon-backed weapon browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponDonorPickerAction {
    Select(u32),
    Clear,
    Secondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlugSelection {
    pub socket_index: usize,
    pub hash: Option<u32>,
}

/// Compact trigger content for an additional authored socket-column choice.
///
/// The text is caller-owned (for example `+ Add choice`, `2`, `3`, or a plug name). When
/// `icon_hash` is present, Sundial renders the corresponding catalog icon and tooltip beside the
/// text while retaining its native plug browser popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlugChoicePickerButton<'a> {
    pub text: &'a str,
    pub icon_hash: Option<u32>,
    /// Effective authored text; omitted for stock choices. Never changes the stock catalog.
    pub tooltip: Option<&'a str>,
    /// Exact compact trigger width in logical pixels. Zero keeps the natural button width.
    pub width: u16,
}

/// Named selection context and trigger presentation for an authored socket choice.
#[derive(Clone, Copy, Debug)]
pub struct PlugChoicePickerOptions<'a> {
    pub donor_hash: u32,
    pub socket_index: usize,
    pub socket_type_override: Option<u16>,
    pub choice_index: usize,
    pub current_hash: Option<u32>,
    pub button: PlugChoicePickerButton<'a>,
    pub mode: PlugSelectionMode,
}

/// Width of the native Sundial Reset control used by an authoring socket row.
pub const AUTHORING_SOCKET_RESET_WIDTH: f32 = authoring_bridge::AUTHORING_SOCKET_RESET_WIDTH;

/// Measured for the active font and padding, shared with the native Reset renderer.
pub fn authoring_socket_reset_width(ui: &egui::Ui) -> f32 {
    authoring_bridge::authoring_socket_reset_width(ui)
}

pub fn authoring_button_width(ui: &egui::Ui, label: &str) -> f32 {
    authoring_bridge::authoring_button_width(ui, label)
}

/// Fixed height shared with the native icon and description picker row.
pub fn authoring_choice_row_height(ui: &egui::Ui) -> f32 {
    (ui.text_style_height(&egui::TextStyle::Button)
        + ui.text_style_height(&egui::TextStyle::Body)
        + 13.0)
        .max(48.0)
}

/// Returns the responsive right-aligned label width used by Sundial's plug rows.
#[must_use]
pub fn authoring_socket_label_width(available_width: f32) -> f32 {
    authoring_bridge::authoring_socket_label_width(available_width)
}

/// Renders the compact, right-aligned socket label used by Sundial's plug rows.
pub fn draw_authoring_socket_label(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    authoring_bridge::draw_authoring_socket_label(ui, label, width)
}

/// Renders the native Sundial Reset control used by an authoring socket row.
pub fn draw_authoring_socket_reset(
    ui: &mut egui::Ui,
    enabled: bool,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    authoring_bridge::draw_authoring_socket_reset(ui, enabled, tooltip)
}

/// Renders Sundial's native inline plug-safety selector.
pub fn draw_plug_safety_selector(
    ui: &mut egui::Ui,
    scope: impl Hash,
    mode: &mut PlugSelectionMode,
) -> bool {
    authoring_bridge::draw_plug_safety_selector(ui, scope, mode)
}

/// Renders Sundial's matching risk warning for a selected plug-safety mode.
pub fn draw_plug_safety_warning(ui: &mut egui::Ui, mode: PlugSelectionMode) {
    authoring_bridge::draw_authoring_plug_safety_warning(ui, mode);
}

/// Renders Sundial's compact information glyph with hover help.
pub fn draw_authoring_info_icon(
    ui: &mut egui::Ui,
    tooltip: impl Into<egui::WidgetText>,
) -> egui::Response {
    authoring_bridge::draw_authoring_info_icon(ui, tooltip)
}

/// Draws Sundial's standard compact action toolbar for companion authoring utilities.
pub fn draw_authoring_toolbar<R>(
    ui: &mut egui::Ui,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    authoring_bridge::draw_authoring_toolbar(ui, contents)
}

/// Returns whether Sundial's saved preferences allow plug-selection safety warnings.
#[must_use]
pub fn show_plug_safety_warnings() -> bool {
    authoring_bridge::show_plug_safety_warnings()
}

/// Returns Sundial's saved default scope for plug selection.
#[must_use]
pub fn default_plug_selection_mode() -> PlugSelectionMode {
    authoring_bridge::default_plug_selection_mode()
}

/// Installs Sundial's text/symbol font families for an external investment-authoring window.
/// The fallback family is registered even when optional game font files cannot be read.
pub fn configure_authoring_fonts(
    ctx: &egui::Context,
    install_directory: &Path,
) -> Result<(), String> {
    authoring_bridge::configure_fonts(ctx, install_directory)
}

impl InvestmentCatalog {
    /// Reuses the same icon, description, focus and tooltip renderer as Sundial's plug browser.
    pub fn draw_authoring_choice_row(
        &self,
        ui: &mut egui::Ui,
        hash: Option<u32>,
        name: &str,
        description: Option<&str>,
        selected: bool,
    ) -> egui::Response {
        authoring_bridge::draw_authoring_choice_row(
            ui,
            &self.catalog,
            hash,
            name,
            description,
            selected,
        )
    }

    /// Renders a compact trigger anchored to Sundial's native compatible-plug browser.
    ///
    /// `choice_index` is part of the persistent egui identity, so multiple ordered choices for
    /// one socket can keep independent popups and search state. The caller controls only compact
    /// trigger content and an optional footer. Return true from the footer when its action
    /// should close the popup. Compatibility filtering and picker rows remain shared with Sundial.
    pub fn draw_supported_plug_choice_picker(
        &self,
        ui: &mut egui::Ui,
        query: &mut String,
        options: PlugChoicePickerOptions<'_>,
        footer: impl FnOnce(&mut egui::Ui) -> bool,
    ) -> Result<Option<PlugSelection>, String> {
        let PlugChoicePickerOptions {
            donor_hash,
            socket_index,
            socket_type_override,
            ..
        } = options;
        let item = self
            .catalog
            .item(u64::from(donor_hash))
            .ok_or_else(|| format!("Unknown donor weapon 0x{donor_hash:08X}"))?;
        if socket_index >= super::MAX_WEAPON_SOCKETS
            || (item.sockets.get(socket_index).is_none()
                && socket_type_override.is_none_or(|socket_type| socket_type == u16::MAX))
        {
            return Err(format!(
                "Donor weapon 0x{donor_hash:08X} has no socket {socket_index}"
            ));
        }
        let action = authoring_bridge::draw_supported_plug_choice_picker(
            ui,
            &self.catalog,
            item,
            query,
            options,
            footer,
        );
        match action {
            Some((socket_index, hash)) => Ok(Some(PlugSelection {
                socket_index,
                hash: hash
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|_| "Selected plug hash does not fit 32 bits".to_owned())?,
            })),
            None => Ok(None),
        }
    }

    /// Renders Sundial's native item header as the trigger for the searchable donor browser.
    /// Each browser owns its filters so one selection cannot silently hide another's candidates.
    pub fn draw_weapon_donor_header_picker<'a>(
        &self,
        ui: &mut egui::Ui,
        scope: impl Hash,
        query: &mut String,
        candidates: impl IntoIterator<Item = &'a WeaponDonorSummary>,
        options: WeaponDonorPickerOptions<'_>,
    ) -> Option<WeaponDonorPickerAction> {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        let action = authoring_bridge::draw_weapon_donor_header_picker(
            ui,
            &self.catalog,
            scope,
            query,
            &candidates,
            options,
        );
        action.and_then(|action| match action {
            authoring_bridge::InvestmentWeaponPickerAction::Select(hash) => u32::try_from(hash)
                .ok()
                .map(WeaponDonorPickerAction::Select),
            authoring_bridge::InvestmentWeaponPickerAction::Clear => {
                Some(WeaponDonorPickerAction::Clear)
            }
            authoring_bridge::InvestmentWeaponPickerAction::Secondary => {
                Some(WeaponDonorPickerAction::Secondary)
            }
        })
    }
}
