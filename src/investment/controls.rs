//! Shared authoring controls. UI adaptation stays separate from catalog queries.
use super::{InvestmentCatalog, PlugSelectionMode, WeaponDonorSummary};
use crate::app::authoring_bridge;
use eframe::egui;
use std::{hash::Hash, path::Path};

pub use crate::ui::help::tooltip_title;
pub use authoring_bridge::{
    AUTHORING_SOCKET_RESET_WIDTH, AuthoringItemHeader, WeaponChoiceFilter, authoring_button_width,
    authoring_socket_label_width, authoring_socket_reset_width,
    configure_fonts as configure_authoring_fonts, default_plug_selection_mode,
    draw_asset_choice_row, draw_asset_choice_row_plain, draw_authoring_info_icon,
    draw_authoring_item_header, draw_authoring_plug_safety_warning as draw_plug_safety_warning,
    draw_authoring_socket_label, draw_authoring_socket_reset, draw_authoring_tile,
    draw_authoring_toolbar, draw_authoring_warning_icon, show_plug_safety_warnings,
};

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
    ui: &mut egui::Ui,
    logo: &egui::TextureHandle,
    view: CatalogLoadingView<'_>,
) {
    egui::CentralPanel::default().show(ui, |ui| {
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
                        ui.weak(view.version);
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
    /// Optional second line for a candidate row, keyed on its item hash. Used where the weapon
    /// name alone does not say what picking it brings, such as the perks a behavior carries.
    pub row_detail: Option<&'a dyn Fn(u32) -> Option<String>>,
    /// Optional line under the selected card's name in place of its item type, such as an armor
    /// piece's slot, class and generation.
    pub selected_detail: Option<&'a str>,
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

/// Authored presentation passed to the shared perk and plug tooltip renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlugTooltip<'a> {
    pub name: Option<&'a str>,
    /// `Some("")` clears the donor description.
    pub description: Option<&'a str>,
    pub classification_hash: Option<u32>,
}

/// A tooltip for something that is not an item, such as a subclass ability: its icon, its name, a
/// quiet line under the name and its description, in the item tooltips' own layout.
#[derive(Clone, Copy)]
pub struct DisplayTooltip<'a> {
    pub icon: Option<&'a egui::TextureHandle>,
    pub name: &'a str,
    pub subtitle: Option<&'a str>,
    pub description: Option<&'a str>,
}

/// Draws a [`DisplayTooltip`], as a hover's contents.
pub fn draw_display_tooltip(ui: &mut egui::Ui, tooltip: DisplayTooltip<'_>) {
    authoring_bridge::draw_display_tooltip(ui, tooltip);
}

/// A private recipe's icon must never fall back to a different donor while it loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconOverride {
    Pending,
    Texture(egui::TextureId),
}
impl IconOverride {
    pub(crate) fn texture(self) -> Option<egui::TextureId> {
        match self {
            Self::Pending => None,
            Self::Texture(id) => Some(id),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlugChoicePickerButton<'a> {
    pub text: &'a str,
    pub icon_hash: Option<u32>,
    pub icon_override: Option<IconOverride>,
    /// Effective authored text; omitted for stock choices. Never changes the stock catalog.
    pub tooltip: Option<PlugTooltip<'a>>,
    /// Exact compact trigger width in logical pixels. Zero keeps the natural button width.
    pub width: u16,
}

/// Named selection context and trigger presentation for an authored socket choice.
#[derive(Debug)]
pub struct PlugChoicePickerOptions<'a> {
    pub preview: Option<&'a crate::ui::model_preview::Loadout>,
    pub donor_hash: u32,
    pub socket_index: usize,
    pub socket_type_override: Option<u16>,
    pub choice_index: usize,
    pub current_hash: Option<u32>,
    pub button: PlugChoicePickerButton<'a>,
    /// The scope the picker offers plugs from, which its Plugs Offered dropdown changes.
    pub mode: &'a mut PlugSelectionMode,
}

/// Fixed height shared with the native icon and description picker row.
pub fn authoring_choice_row_height(ui: &egui::Ui) -> f32 {
    (ui.text_style_height(&egui::TextStyle::Button)
        + ui.text_style_height(&egui::TextStyle::Body)
        + 13.0)
        .max(48.0)
}

impl InvestmentCatalog {
    pub fn preview_loadout(&self, hash: u32) -> Option<crate::ui::model_preview::Loadout> {
        authoring_bridge::preview_loadout(&self.catalog, hash)
    }
    /// Resolves a read-only cosmetic candidate against installed plug metadata.
    pub fn preview_appearance(
        &self,
        loadout: &crate::ui::model_preview::Loadout,
    ) -> crate::ui::model_preview::Appearance {
        authoring_bridge::resolve_preview(&self.catalog, loadout)
    }
    /// An item wearing a shader's dye rows, for previewing the shader on any gear type.
    pub fn shader_preview_appearance(
        &self,
        item: u32,
        shader: &[Vec<(i8, u16)>; 3],
    ) -> Option<crate::ui::model_preview::Appearance> {
        authoring_bridge::shader_preview(&self.catalog, item, shader)
    }
    pub fn texture_icon(&self, ctx: &egui::Context, tag: u32) -> Option<egui::TextureHandle> {
        self.catalog.texture_icon(ctx, tag)
    }

    /// An item's inventory icon, read in the background on first use.
    pub fn item_icon(&self, ctx: &egui::Context, hash: u32) -> Option<egui::TextureHandle> {
        self.catalog.icon_texture(ctx, u64::from(hash))
    }

    /// What a plug scope offers on `item_hash`'s sockets, in Dawn's words for that item: for
    /// Atonement Tau, Atonement Tau Plugs, Chest Armor Plugs, Same Socket Type, Chest Armor, All
    /// Armor and All. Without one socket to name, the socket-type scope reads as each socket's own
    /// type. An item the catalog lacks keeps the scope's own label.
    #[must_use]
    pub fn plug_scope_label(&self, mode: PlugSelectionMode, item_hash: u32) -> String {
        match (mode, self.catalog.item(u64::from(item_hash))) {
            (PlugSelectionMode::MatchingSocketType, _) => "Same Socket Type".to_owned(),
            (mode, Some(item)) => mode.contextual_label(item, ""),
            (mode, None) => mode.label().to_owned(),
        }
    }

    /// A subclass ability's icon, from the container its node display record names
    /// ([`SubclassSummary::entry_icons`](crate::investment::SubclassSummary::entry_icons)).
    pub fn subclass_icon(
        &self,
        ctx: &egui::Context,
        container: u32,
    ) -> Option<egui::TextureHandle> {
        self.catalog.subclass_icon_texture(ctx, container)
    }

    /// The icon container of an emblem's nameplate art.
    #[must_use]
    pub fn nameplate_container(&self, emblem_hash: u32) -> Option<u32> {
        self.catalog
            .secondary_icon_container(u64::from(emblem_hash))
    }

    /// One image of an emblem's nameplate at its own size, by its icon container layer: the banner
    /// (+0x14), the overlay (+0x20) or the wide background (+0x24).
    pub fn nameplate_texture(
        &self,
        ctx: &egui::Context,
        emblem_hash: u32,
        layer_offset: usize,
    ) -> Option<egui::TextureHandle> {
        self.catalog
            .secondary_icon_texture(ctx, u64::from(emblem_hash), layer_offset)
    }

    /// A perk in a list of perks: its icon, its name, and its description's first line under it.
    pub fn draw_perk_card_row(
        &self,
        ui: &mut egui::Ui,
        hash: u32,
        name: &str,
        selected: bool,
        tooltip: PlugTooltip<'_>,
        icon: Option<IconOverride>,
    ) -> egui::Response {
        authoring_bridge::draw_perk_card_row(ui, &self.catalog, hash, name, selected, tooltip, icon)
    }

    pub fn draw_authoring_choice_row_with_icon(
        &self,
        ui: &mut egui::Ui,
        hash: Option<u32>,
        name: &str,
        description: Option<&str>,
        selected: bool,
        icon: Option<IconOverride>,
    ) -> egui::Response {
        authoring_bridge::draw_authoring_choice_row(
            ui,
            &self.catalog,
            hash,
            name,
            description,
            selected,
            icon,
        )
    }

    /// Compact native catalog row with the shared authored perk tooltip.
    pub fn draw_perk_row(
        &self,
        ui: &mut egui::Ui,
        hash: u32,
        name: &str,
        selected: bool,
        tooltip: PlugTooltip<'_>,
    ) -> egui::Response {
        authoring_bridge::draw_perk_row(ui, &self.catalog, hash, name, selected, tooltip, None)
    }

    /// The native catalog icon, sized for the surrounding authoring control.
    pub fn draw_perk_icon(&self, ui: &mut egui::Ui, hash: u32, size: f32) {
        authoring_bridge::draw_perk_icon(ui, &self.catalog, hash, size);
    }

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
            None,
        )
    }

    /// Renders a compact trigger anchored to Sundial's native compatible-plug browser.
    ///
    /// `choice_index` is part of the persistent egui identity, so multiple ordered choices for
    /// one socket can keep independent popups and search state. The caller controls only compact
    /// trigger content and an optional action drawn at the right of the popup's controls. Return
    /// true from it when the action should close the popup. Compatibility filtering, the Plugs Offered dropdown
    /// and picker rows remain shared with Sundial. A scope picked from that dropdown is written to
    /// `options.mode` and leaves the popup open.
    pub fn draw_supported_plug_choice_picker(
        &self,
        ui: &mut egui::Ui,
        query: &mut String,
        options: PlugChoicePickerOptions<'_>,
        leading_action: impl FnOnce(&mut egui::Ui) -> bool,
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
            leading_action,
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
        scope: impl Hash + std::fmt::Debug,
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
            None,
            None,
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

    /// The donor browser behind a plain dropdown instead of a card header.
    pub fn draw_weapon_donor_dropdown_picker<'a>(
        &self,
        ui: &mut egui::Ui,
        scope: impl Hash + std::fmt::Debug,
        query: &mut String,
        candidates: impl IntoIterator<Item = &'a WeaponDonorSummary>,
        options: WeaponDonorPickerOptions<'_>,
    ) -> Option<WeaponDonorPickerAction> {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        let action = authoring_bridge::draw_weapon_donor_dropdown_picker(
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

    /// The donor browser opened from `trigger`, a control the caller drew, such as a row naming
    /// the weapon that supplies one part of another.
    pub fn draw_weapon_donor_picker_from<'a>(
        &self,
        ui: &mut egui::Ui,
        trigger: egui::Response,
        scope: impl Hash + std::fmt::Debug,
        query: &mut String,
        candidates: impl IntoIterator<Item = &'a WeaponDonorSummary>,
        options: WeaponDonorPickerOptions<'_>,
    ) -> Option<WeaponDonorPickerAction> {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        authoring_bridge::draw_weapon_donor_picker_from(
            ui,
            &self.catalog,
            scope,
            query,
            &candidates,
            options,
            trigger,
        )
        .and_then(|action| match action {
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

    /// The donor pickers' weapon filter bar, over the weapon types `weapons` offer, for a list
    /// whose choices each belong to a weapon. Returns whether a control was used.
    pub fn draw_weapon_choice_filters(
        &self,
        ui: &mut egui::Ui,
        id_salt: impl Hash + std::fmt::Debug + Clone,
        weapons: &[u32],
        filter: &mut WeaponChoiceFilter,
    ) -> bool {
        authoring_bridge::draw_weapon_choice_filters(ui, &self.catalog, id_salt, weapons, filter)
    }

    /// Whether `weapon` passes `filter`.
    #[must_use]
    pub fn weapon_choice_passes(&self, weapon: u32, filter: &WeaponChoiceFilter) -> bool {
        authoring_bridge::weapon_choice_passes(&self.catalog, weapon, filter)
    }

    /// The appearance donor browser keeps inspection separate from applying a candidate.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_weapon_appearance_picker<'a>(
        &self,
        ui: &mut egui::Ui,
        scope: impl Hash + std::fmt::Debug,
        query: &mut String,
        candidates: impl IntoIterator<Item = &'a WeaponDonorSummary>,
        options: WeaponDonorPickerOptions<'_>,
        default_weapon_type: Option<&str>,
        mut preview: impl FnMut(&mut egui::Ui, Option<u32>),
    ) -> Option<WeaponDonorPickerAction> {
        let candidates = candidates.into_iter().collect::<Vec<_>>();
        authoring_bridge::draw_weapon_donor_header_picker(
            ui,
            &self.catalog,
            scope,
            query,
            &candidates,
            options,
            Some(&mut preview),
            default_weapon_type,
        )
        .and_then(|action| match action {
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
