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
            ctx.egui_wants_keyboard_input()
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

/// Whether the workbench window owns this frame's keyboard shortcuts: the pointer is over
/// it, or the pointer is over no other window or panel and it is the top window.
pub(super) fn owns_shortcuts(ctx: &egui::Context, window: egui::LayerId) -> bool {
    let top = || {
        ctx.memory(|memory| {
            memory
                .layer_ids()
                .filter(|layer| {
                    layer.order == egui::Order::Middle
                        && memory.areas().is_visible(layer)
                        && memory.areas().parent_layer(*layer).is_none()
                })
                .last()
        }) == Some(window)
    };
    let Some(pointer) = ctx.input(|input| input.pointer.latest_pos()) else {
        return top();
    };
    match ctx.layer_id_at(pointer) {
        Some(layer) if layer == window => true,
        Some(layer) if matches!(layer.order, egui::Order::Middle | egui::Order::Background) => {
            false
        }
        Some(_) => top(),
        None => false,
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
        if ctx.egui_wants_keyboard_input()
            || ctx.memory(|m| m.top_modal_layer().is_some())
            || egui::Popup::is_any_open(ctx)
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
