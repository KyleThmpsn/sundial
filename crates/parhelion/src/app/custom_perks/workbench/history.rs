//! Session-only document history. Pending parameter editors keep their own transaction.
use super::*;

#[derive(Clone)]
pub(in crate::app::custom_perks) struct History<T = PerkRecipe> {
    past: Vec<T>,
    future: Vec<T>,
    gesture: Option<(egui::Id, f64)>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            gesture: None,
        }
    }
}

impl<T> History<T> {
    pub(in crate::app::custom_perks) fn record_step(&mut self, before: T) {
        self.past.push(before);
        if self.past.len() > 64 {
            self.past.remove(0);
        }
        self.future.clear();
        self.gesture = None;
    }

    pub(in crate::app::custom_perks) fn record(&mut self, before: T, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        let gesture = ctx.dragged_id().or_else(|| {
            ctx.wants_keyboard_input()
                .then(|| ctx.memory(|memory| memory.focused()))
                .flatten()
        });
        let same = gesture
            .zip(self.gesture)
            .is_some_and(|(id, (previous, time))| {
                id == previous
                    && now - time < 0.75
                    && !ctx.input(|input| input.pointer.any_pressed())
            });
        if !same {
            self.record_step(before);
        }
        self.future.clear();
        self.gesture = gesture.map(|id| (id, now));
    }

    pub(in crate::app::custom_perks) fn available(&self, redo: bool) -> bool {
        if redo {
            !self.future.is_empty()
        } else {
            !self.past.is_empty()
        }
    }

    pub(in crate::app::custom_perks) fn restore(&mut self, recipe: &mut T, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.future, &mut self.past)
        } else {
            (&mut self.past, &mut self.future)
        };
        let Some(previous) = from.pop() else {
            return false;
        };
        to.push(std::mem::replace(recipe, previous));
        self.gesture = None;
        true
    }
}

impl Workbench {
    pub(in crate::app::custom_perks) fn restore_history(&mut self, redo: bool) {
        if let Some(editor) = &mut self.editor {
            editor.restore_history(redo);
            return;
        }
        let Some(document) = self.documents.get_mut(self.selected) else {
            return;
        };
        if document.pending_effect.is_some() {
            return;
        }
        if document.history.restore(&mut document.recipe, redo) {
            document.modified = Some(SystemTime::now());
            self.message = None;
            self.error = None;
            self.persist_drafts();
        }
    }

    pub(in crate::app::custom_perks) fn history_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input()
            || ctx.memory(|m| m.top_modal_layer().is_some() || m.any_popup_open())
        {
            return;
        }
        if ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
        }) {
            self.restore_history(true);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.restore_history(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_whole_effects_and_redo_is_invalidated_by_another_edit() {
        let ctx = egui::Context::default();
        let mut recipe = PerkRecipe::new();
        recipe.effects.push(PerkRecipe::effect(421));
        recipe.effects[0].activation =
            Some(sundial::package_authoring::sandbox_perk::activation::PerkActivation::GrenadeKill);
        let mut doc = Document::new(recipe.clone(), None);
        doc.history.record(doc.recipe.clone(), &ctx);
        doc.recipe.effects.clear();
        let deleted = doc.recipe.clone();
        assert!(doc.history.restore(&mut doc.recipe, false));
        assert_eq!(doc.recipe, recipe);
        assert!(doc.history.restore(&mut doc.recipe, true));
        assert_eq!(doc.recipe, deleted);
        assert!(doc.history.restore(&mut doc.recipe, false));
        doc.history.record(doc.recipe.clone(), &ctx);
        doc.recipe.name = "Changed".into();
        assert!(!doc.history.available(true));
        let restored: Document =
            serde_json::from_slice(&serde_json::to_vec(&doc).unwrap()).unwrap();
        assert!(!restored.history.available(false));
        assert!(!restored.history.available(true));
        assert_eq!(restored.recipe, doc.recipe);
    }
}
