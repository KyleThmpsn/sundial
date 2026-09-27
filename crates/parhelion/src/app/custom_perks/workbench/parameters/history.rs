//! Property drafts use the same session history mechanism as effect cards.
use super::*;

#[derive(Clone, PartialEq)]
pub(in crate::app::custom_perks) struct Snapshot {
    values: Vec<WeaponRuntimeValueOverride>,
    actions: Vec<crate::WeaponSandboxPerkActionFloatRecipe>,
    projectiles: Vec<ProjectileSelection>,
    movement: Option<(u32, Vec<(projectile::parameters::Kind, u32)>)>,
}

impl PerkEditor {
    pub(in crate::app::custom_perks) fn snapshot(&self) -> Snapshot {
        Snapshot {
            values: self.draft.clone(),
            actions: self.action_draft.clone(),
            projectiles: self.projectile_draft.clone(),
            movement: self.pending_movement.clone(),
        }
    }
    pub(in crate::app::custom_perks) fn record_history(
        &mut self,
        before: Snapshot,
        ctx: &egui::Context,
    ) {
        if before != self.snapshot() {
            self.history.record(before, ctx);
        }
    }
    pub(in crate::app::custom_perks) fn history_available(&self, redo: bool) -> bool {
        !self.is_loading() && self.history.available(redo)
    }
    pub(in crate::app::custom_perks) fn restore_history(&mut self, redo: bool) {
        if !self.history_available(redo) {
            return;
        }
        let mut snapshot = self.snapshot();
        if self.history.restore(&mut snapshot, redo) {
            let reload = snapshot.projectiles != self.projectile_draft;
            self.draft = snapshot.values;
            self.action_draft = snapshot.actions;
            self.projectile_draft = snapshot.projectiles;
            self.pending_movement = snapshot.movement;
            self.value_text.clear();
            self.parameter_error = None;
            self.preview = None;
            self.conversion = None;
            if reload {
                self.graph = None;
                self.error = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_redo_and_reset_keep_property_drafts_isolated_until_apply() {
        let ctx = egui::Context::default();
        let loaded = super::super::tests::fixture();
        let field = loaded.graphs[0].1.fields().next().unwrap();
        let value = WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: field.value.clone(),
        };
        let mut editor = super::super::tests::editor(loaded);
        let before = editor.snapshot();
        editor.draft.push(value);
        editor.record_history(before, &ctx);
        let edited = editor.snapshot();
        editor.restore_history(false);
        assert!(editor.draft.is_empty());
        editor.restore_history(true);
        assert!(editor.snapshot() == edited);
        let before = editor.snapshot();
        editor.reset_all();
        editor.record_history(before, &ctx);
        assert!(editor.draft.is_empty());
        editor.restore_history(false);
        assert!(editor.snapshot() == edited);
        assert!(editor.original_draft.is_empty());
    }
}
