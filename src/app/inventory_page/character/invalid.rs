//! Keep unknown stored definitions accessible from both loadout layouts.
use super::*;

impl SundialApp {
    pub(in crate::app) fn draw_invalid_inventory_items(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
    ) {
        let sources = self.character_inventory_sources(character_index);
        let identities = inventory_item_ui_identities(&sources.items);
        let invalid = sources
            .items
            .iter()
            .zip(identities)
            .filter(|(item, _)| {
                self.manifest
                    .inventory_definition(u64::from(item.definition_hash))
                    .is_none()
            })
            .map(|(item, identity)| (item.clone(), identity))
            .collect::<Vec<_>>();
        if invalid.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.heading("Invalid Items");
        ui.label("These stored items have no installed definition. Their hashes identify them. Replace or remove them to resolve unknown inventory placement.");
        let usage = self.inventory_bucket_usage(
            &sources.items,
            sources
                .equipment_error
                .is_none()
                .then_some(sources.equipped_items.as_slice()),
        );
        let occupied = sources
            .equipped_items
            .iter()
            .map(|item| item.slot)
            .collect::<Vec<_>>();
        let editable = account::can_mutate_character_inventory(&self.document);
        let (minimum, maximum) = self.preferences.item_card_width.dimensions();
        let mut pending = None;
        item_editor::draw_responsive_item_cards(
            ui,
            &invalid,
            minimum,
            maximum,
            |ui, (item, identity)| {
                if let Some(request) = self.draw_inventory_item_card(
                    ui,
                    item,
                    *identity,
                    editable,
                    sources.class_type,
                    CharacterInventoryCardContext {
                        bucket_usage: &usage,
                        transfer_targets: &[],
                        occupied_equipment_slots: &occupied,
                    },
                ) && pending.is_none()
                {
                    pending = Some((item.clone(), *identity, request));
                }
            },
        );
        if let Some((item, identity, request)) = pending {
            self.apply_character_inventory_item_request(&item, identity, request);
        }
    }
}
