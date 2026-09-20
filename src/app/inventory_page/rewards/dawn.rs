//! Account-wide currency queue. Delivery identities stay in the persistence layer.
mod composer;
mod ledger;

use super::{Edit, RewardDraft};
use crate::app::SundialApp;
use eframe::egui;

const GAP: f32 = 12.0;
const QUANTITY_WIDTH: f32 = 84.0;
const ACTION_WIDTH: f32 = 112.0;

impl SundialApp {
    pub(super) fn draw_dawn_reward_debts(&mut self, ui: &mut egui::Ui) {
        let Some(document) = self.document.dawn_account() else {
            return;
        };
        let character_count = document.characters().characters().len();
        let id = ui.make_persistent_id("dawn-reward-draft");
        let mut draft = ui
            .data(|data| data.get_temp::<RewardDraft>(id))
            .unwrap_or_else(|| RewardDraft {
                character_slot: self.selected_character,
                kind: 1,
                definition_hash: None,
                quantity: 1,
                query: String::new(),
                item_type: None,
            });
        // Dawn records a character identity, but delivery credits the shared account.
        draft.character_slot = self
            .selected_character
            .min(character_count.saturating_sub(1));
        let mut edit = None;
        ui.scope(|ui| {
            ui.set_max_width(ui.available_width().min(640.0));
            ui.spacing_mut().item_spacing.x = GAP;
            ui.add_space(6.0);
            ui.add_enabled_ui(character_count > 0, |ui| {
                if composer::draw(ui, &self.manifest, &mut draft) {
                    edit = Some(Edit::Add(draft.clone()));
                }
            });
            ui.add_space(16.0);
            ledger::draw(ui, &self.manifest, document, &mut edit);
        });
        ui.data_mut(|data| data.insert_temp(id, draft));
        if ui.is_enabled()
            && let Some(edit) = edit
        {
            match self.apply_dawn_reward_edit(edit) {
                Ok(()) => {
                    self.report_edit("Reward Queue Updated");
                    self.set_status("Reward queue updated. Click Save to write it", false);
                }
                Err(error) => self.set_status(error, true),
            }
        }
    }

    fn apply_dawn_reward_edit(&mut self, edit: Edit) -> Result<(), String> {
        let hash = match &edit {
            Edit::Add(draft) => draft.definition_hash,
            Edit::Quantity(id, _) => self
                .document
                .dawn_account()
                .and_then(|doc| doc.reward_debts().iter().find(|debt| debt.id == *id))
                .map(|debt| debt.definition_hash),
            Edit::Remove(_) => None,
        };
        let metadata = hash
            .and_then(|hash| self.manifest.inventory_definition(u64::from(hash)))
            .map(|definition| *definition.metadata);
        let document = self
            .document
            .dawn_account_mut()
            .ok_or("The Dawn account is unavailable")?;
        match edit {
            Edit::Add(draft) => document.queue_currency(
                draft.character_slot,
                hash.ok_or("Choose a currency")?,
                draft.quantity,
                &metadata.ok_or("The installed currency definition is unavailable")?,
            ),
            Edit::Quantity(id, quantity) => document.set_debt_quantity(
                id,
                quantity,
                &metadata.ok_or("The installed currency definition is unavailable")?,
            ),
            Edit::Remove(id) => document.cancel_debt(id),
        }
    }
}

fn currency_width(ui: &egui::Ui) -> f32 {
    (ui.available_width() - QUANTITY_WIDTH - ACTION_WIDTH - 2.0 * GAP).max(120.0)
}
