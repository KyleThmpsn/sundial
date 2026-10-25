//! The weapon donor pickers Parhelion draws through Sundial's catalog.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_weapon_donor_header_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
    query: &mut String,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
    preview: Option<&mut AppearancePreview<'_>>,
    default_weapon_type: Option<&str>,
) -> Option<InvestmentWeaponPickerAction> {
    let scope = ui.make_persistent_id(scope);
    let selected = options
        .selected_hash
        .and_then(|hash| catalog.item(u64::from(hash)));
    let hash_text = options
        .selected_hash
        .map(|hash| format_hash_hex(u64::from(hash)));
    // Ornaments and other plugs are named installed items without a socketed item definition.
    // Describing them from the catalog's name and type maps keeps a selected appearance source
    // readable instead of reporting it as missing.
    let plug = options
        .selected_hash
        .filter(|_| selected.is_none())
        .and_then(|hash| {
            let hash = u64::from(hash);
            let name = catalog.display_name(hash)?;
            Some((name, catalog.plug_type_name(hash).unwrap_or_default()))
        });
    // A known item shows its name and hash, and a right-click copies the hash. An item the
    // catalog cannot name keeps the hash as all there is to tell it by.
    let definition = match (selected, hash_text.as_deref()) {
        (Some(item), Some(hash_text)) => crate::app::item_editor::DefinitionSummary::Known {
            name: &item.name,
            hash_display_text: hash_text,
            type_name: options.selected_detail.unwrap_or(&item.type_name),
        },
        (None, Some(hash_text)) => {
            crate::app::item_editor::DefinitionSummary::from_name_and_type(hash_text, plug)
        }
        // Nothing selected: name the choice that leads here rather than the generic Empty, so a
        // header like Unique Weapon Behavior does not read as two ways of saying nothing.
        (_, None) => options.clear.as_ref().map_or(
            crate::app::item_editor::DefinitionSummary::Empty,
            |choice| crate::app::item_editor::DefinitionSummary::Known {
                name: choice.label,
                hash_display_text: "",
                type_name: "",
            },
        ),
    };
    let header = ItemHeader {
        label: options.header_label,
        soid: None,
        definition,
        icon: options.selected_icon_override.cloned().or_else(|| {
            options
                .selected_hash
                .and_then(|hash| catalog.icon_texture(ui.ctx(), u64::from(hash)))
        }),
        fill: muted_item_header_fill(ui),
        valid: options.selected_hash.is_none() || selected.is_some() || plug.is_some(),
        invalid_message: "Not in the loaded catalog",
    };
    let mut action_button = None;
    let mut secondary_button = None;
    let action_width = authoring_button_width(ui, options.action_label)
        + options.secondary_action_label.map_or(0.0, |label| {
            authoring_button_width(ui, label) + ui.spacing().item_spacing.x
        });
    let trigger = draw_item_header_with_trailing_at_icon_size(
        ui,
        header,
        DONOR_HEADER_ICON_SIZE,
        action_width,
        |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    action_button = Some(ui.button(options.action_label));
                    if let Some(label) = options.secondary_action_label {
                        secondary_button = Some(ui.button(label));
                    }
                });
            });
        },
    );
    let action_button = action_button.expect("a donor header always draws its action button");
    if let Some(hash) = options.selected_hash {
        let buttons_left = secondary_button
            .as_ref()
            .map_or(action_button.rect.left(), |secondary| {
                secondary.rect.left().min(action_button.rect.left())
            });
        copy_hash_menu(ui, &trigger, buttons_left, hash);
        drop(catalog_item_tooltip(trigger, catalog, u64::from(hash)));
    }
    if let Some(preview) = preview {
        return appearance_picker::draw(
            ui,
            catalog,
            scope,
            query,
            candidates,
            options,
            &action_button,
            preview,
            default_weapon_type,
        );
    }
    // Armor, Sparrows, Ships and Ghost Shells have no weapon type, damage or ammo to filter by.
    let filter_scope = if candidates
        .iter()
        .any(|donor| crate::catalog::is_weapon_bucket(donor.bucket_hash))
    {
        ItemFilterScope::WeaponDonor
    } else {
        ItemFilterScope::Armor
    };
    let action = draw_weapon_donor_picker_popup(
        ui,
        catalog,
        scope,
        query,
        candidates,
        options,
        &action_button,
        filter_scope,
    );
    if secondary_button.is_some_and(|button| button.clicked()) {
        return Some(InvestmentWeaponPickerAction::Secondary);
    }
    action
}

/// The same donor browser as the card header, opened from a plain dropdown that sits in a
/// column of other dropdowns without a card around it.
pub(crate) fn draw_weapon_donor_dropdown_picker(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
    query: &mut String,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
) -> Option<InvestmentWeaponPickerAction> {
    let trigger = dropdown_button(ui, options.selected_label);
    draw_weapon_donor_picker_from(ui, catalog, scope, query, candidates, options, trigger)
}

/// The same donor browser opened from a trigger the caller drew, such as a row naming the weapon
/// that supplies one part of another.
pub(crate) fn draw_weapon_donor_picker_from(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: impl Hash + std::fmt::Debug,
    query: &mut String,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
    trigger: egui::Response,
) -> Option<InvestmentWeaponPickerAction> {
    let scope = ui.make_persistent_id(scope);
    let trigger = match options.selected_hash {
        Some(hash) => catalog_item_tooltip(trigger, catalog, u64::from(hash)),
        None => trigger,
    };
    // Weapon type and damage filters without the dummy-weapon toggle, since the caller already
    // chose which weapons are offered. Emblems and other gear have only rarity to filter by.
    let filter_scope = if candidates
        .iter()
        .any(|donor| crate::catalog::is_weapon_bucket(donor.bucket_hash))
    {
        ItemFilterScope::Weapon
    } else {
        ItemFilterScope::Armor
    };
    draw_weapon_donor_picker_popup(
        ui,
        catalog,
        scope,
        query,
        candidates,
        options,
        &trigger,
        filter_scope,
    )
}

/// A full-width button drawn like a combo box: framed, text on the left, caret on the right.
fn dropdown_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let padding = ui.spacing().button_padding;
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, text));
    if ui.is_rect_visible(rect) {
        let visuals = *ui.style().interact(&response);
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let icon = egui::Rect::from_center_size(
            egui::pos2(
                rect.right() - padding.x - ui.spacing().icon_width * 0.5,
                rect.center().y,
            ),
            egui::Vec2::splat(ui.spacing().icon_width),
        );
        let triangle = egui::Rect::from_center_size(
            icon.center(),
            egui::vec2(icon.width() * 0.7, icon.height() * 0.45),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                triangle.left_top(),
                triangle.right_top(),
                triangle.center_bottom(),
            ],
            visuals.fg_stroke.color,
            egui::Stroke::NONE,
        ));
        let text_rect = egui::Rect::from_min_max(
            rect.min + padding,
            egui::pos2(icon.left() - padding.x, rect.max.y - padding.y),
        );
        ui.new_child(
            egui::UiBuilder::new()
                .max_rect(text_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        )
        .add(
            egui::Label::new(egui::RichText::new(text).color(visuals.text_color()))
                .truncate()
                .selectable(false),
        );
    }
    response
}

#[allow(clippy::too_many_arguments)]
fn draw_weapon_donor_picker_popup(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    scope: egui::Id,
    query: &mut String,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
    trigger: &egui::Response,
    filter_scope: ItemFilterScope,
) -> Option<InvestmentWeaponPickerAction> {
    let action = draw_definition_picker_with_open_request_and_item_filter(
        ui,
        catalog,
        scope.with("picker"),
        query,
        PickerHeight {
            min: 220.0,
            max: 480.0,
        },
        (Some(trigger), false),
        |ui, query_text, filter| {
            let items = candidates
                .iter()
                .filter_map(|donor| catalog.item(u64::from(donor.hash)))
                .collect::<Vec<_>>();
            let interacted =
                draw_item_filter_bar(ui, scope.with("filters"), filter_scope, &items, filter);
            let filtered = filtered_weapon_donors(catalog, candidates, filter);
            (
                weapon_donor_choices(catalog, query_text, &filtered, options),
                interacted,
            )
        },
    );
    match action {
        Some(ItemEditorAction::SetDefinition { hash }) => {
            Some(InvestmentWeaponPickerAction::Select(hash))
        }
        Some(ItemEditorAction::ClearDefinition) => Some(InvestmentWeaponPickerAction::Clear),
        Some(_) | None => None,
    }
}

/// The donor pickers' weapon filters, for a list whose choices each belong to a weapon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponChoiceFilter(ItemFilter);

impl WeaponChoiceFilter {
    /// Only weapons of `weapon_type`, as the appearance picker opens.
    #[must_use]
    pub fn of_weapon_type(weapon_type: &str) -> Self {
        Self(ItemFilter {
            weapon_type: Some(weapon_type.to_owned()),
            ..ItemFilter::default()
        })
    }
}

/// The donor pickers' filter bar over the weapon types `weapons` offer. Returns whether a
/// control was used.
pub(crate) fn draw_weapon_choice_filters(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id_salt: impl Hash + std::fmt::Debug + Clone,
    weapons: &[u32],
    filter: &mut WeaponChoiceFilter,
) -> bool {
    let items = weapons
        .iter()
        .filter_map(|hash| catalog.item(u64::from(*hash)))
        .collect::<Vec<_>>();
    draw_item_filter_bar(ui, id_salt, ItemFilterScope::Weapon, &items, &mut filter.0)
}

/// Whether `weapon` passes `filter`.
pub(crate) fn weapon_choice_passes(
    catalog: &Catalog,
    weapon: u32,
    filter: &WeaponChoiceFilter,
) -> bool {
    catalog
        .item(u64::from(weapon))
        .is_some_and(|item| filter.0.matches(catalog, item))
}

pub(super) fn filtered_weapon_donors<'a>(
    catalog: &Catalog,
    candidates: &[&'a WeaponDonorSummary],
    filter: &ItemFilter,
) -> Vec<&'a WeaponDonorSummary> {
    candidates
        .iter()
        .copied()
        .filter(|donor| {
            if !filter.include_dummy_weapons
                && crate::catalog::dummy_items::contains(u64::from(donor.hash))
            {
                return false;
            }
            catalog
                .item(u64::from(donor.hash))
                .is_some_and(|item| filter.matches(catalog, item))
        })
        .collect()
}

pub(super) fn weapon_donor_choices(
    catalog: &Catalog,
    query_text: &str,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
) -> DefinitionPickerChoices {
    let query = CatalogSearchQuery::new(query_text);
    let mut matches = candidates
        .iter()
        .copied()
        .filter(|donor| {
            query.matches(
                catalog,
                u64::from(donor.hash),
                &[donor.name.as_str(), donor.type_name.as_str()],
            )
        })
        .collect::<Vec<_>>();
    // One list. Whether a weapon has a Collections row changes how it is built, not how it is
    // browsed, so ordering follows the search, then the weapon type and name. An empty query
    // scores every candidate zero and falls through to that ordering.
    matches.sort_by_cached_key(|donor| {
        (
            Reverse(query.name_match_count(&donor.name)),
            donor.type_name.to_lowercase(),
            donor.name.to_lowercase(),
            donor.hash,
        )
    });
    DefinitionPickerChoices {
        definitions: matches
            .into_iter()
            .map(|donor| DefinitionChoice {
                hash: u64::from(donor.hash),
                name: donor.name.clone(),
                // The second line is what choosing the row brings when the caller says so,
                // otherwise the weapon type.
                type_name: options
                    .row_detail
                    .and_then(|detail| detail(donor.hash))
                    .unwrap_or_else(|| donor.type_name.clone()),
                group: None,
            })
            .collect(),
        existing_inventory: Vec::new(),
        clear: options.clear.map(|choice| ClearDefinitionChoice {
            label: choice.label.to_owned(),
            tooltip: choice.tooltip.to_owned(),
            selected: choice.selected,
        }),
        random_item_builder_hash: None,
        empty_message: "No compatible installed weapons found".to_owned(),
    }
}
