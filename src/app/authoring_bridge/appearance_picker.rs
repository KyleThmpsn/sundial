//! The appearance browser on the shared definition picker: the same rows, search and filters
//! as the donor pickers, with the current appearance previewed under the list. The filter
//! opens on the gameplay donor's weapon type; other types stay reachable.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    id: egui::Id,
    query: &mut String,
    candidates: &[&WeaponDonorSummary],
    options: WeaponDonorPickerOptions<'_>,
    trigger: &egui::Response,
    preview: &mut AppearancePreview<'_>,
    default_weapon_type: Option<&str>,
) -> Option<InvestmentWeaponPickerAction> {
    let default_filter = default_weapon_type.map(|weapon_type| ItemFilter {
        weapon_type: Some(weapon_type.to_owned()),
        ..ItemFilter::default()
    });
    let selected = options.selected_hash;
    let (action, _) =
        super::super::item_editor::draw_definition_picker_with_open_request_item_filter_and_footer(
            ui,
            catalog,
            id.with("picker"),
            query,
            PickerHeight {
                min: 220.0,
                max: 480.0,
            },
            (Some(trigger), false),
            default_filter,
            (
                |ui, query_text, filter| {
                    let items = candidates
                        .iter()
                        .filter_map(|donor| catalog.item(u64::from(donor.hash)))
                        .collect::<Vec<_>>();
                    let interacted = draw_item_filter_bar(
                        ui,
                        id.with("filters"),
                        ItemFilterScope::WeaponDonor,
                        &items,
                        filter,
                    );
                    let filtered = filtered_weapon_donors(catalog, candidates, filter);
                    (
                        weapon_donor_choices(catalog, query_text, &filtered, options),
                        interacted,
                    )
                },
                |ui| {
                    preview(ui, selected);
                    None::<()>
                },
            ),
        );
    match action {
        Some(ItemEditorAction::SetDefinition { hash }) => {
            Some(InvestmentWeaponPickerAction::Select(hash))
        }
        Some(ItemEditorAction::ClearDefinition) => Some(InvestmentWeaponPickerAction::Clear),
        Some(_) | None => None,
    }
}
