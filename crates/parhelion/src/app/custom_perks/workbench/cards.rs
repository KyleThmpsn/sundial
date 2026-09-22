//! Effect-list presentation state and moves. Neither changes an effect's native identity.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Card {
    list: egui::Id,
    effect: u16,
    position: usize,
    count: usize,
}

#[derive(Clone, Copy)]
struct Drag {
    list: egui::Id,
    effect: u16,
}

#[derive(Clone, Copy)]
pub(super) struct Move {
    effect: u16,
    /// Insertion boundary in the original list, before removing the dragged effect.
    boundary: usize,
}

impl Card {
    pub(super) fn new(document: &str, effect: u16, position: usize, count: usize) -> Self {
        Self {
            list: egui::Id::new(("perk-effect-list", document)),
            effect,
            position,
            count,
        }
    }

    fn id(self) -> egui::Id {
        self.list.with(self.effect)
    }

    pub(super) fn expanded(self, ctx: &egui::Context) -> bool {
        ctx.data(|data| data.get_temp(self.id().with("expanded")).unwrap_or(true))
    }

    pub(super) fn set_expanded(self, ctx: &egui::Context, expanded: bool) {
        ctx.data_mut(|data| data.insert_temp(self.id().with("expanded"), expanded));
    }

    pub(super) fn controls(self, ui: &mut egui::Ui) {
        let expanded = self.expanded(ui.ctx());
        let label = if expanded {
            "Collapse Effect"
        } else {
            "Expand Effect"
        };
        let icon = if expanded {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };
        let toggle = ui
            .push_id(self.id().with("toggle"), |ui| {
                crate::app::style::named_control(
                    ui.add(egui::Button::new(icon).frame(false)),
                    label,
                )
                .on_hover_text(label)
            })
            .inner;
        if toggle.clicked() {
            self.set_expanded(ui.ctx(), !expanded);
        }
        let grip = ui
            .dnd_drag_source(
                self.id().with("drag"),
                Drag {
                    list: self.list,
                    effect: self.effect,
                },
                |ui| {
                    ui.add_sized(
                        [16.0, ui.spacing().interact_size.y],
                        egui::Label::new(egui_phosphor::regular::DOTS_SIX_VERTICAL)
                            .selectable(false),
                    );
                },
            )
            .response;
        crate::app::style::named_control(grip, "Reorder Effect").on_hover_text(
            "Drag to reorder effects. Move Up and Move Down are also in the effect menu.",
        );
    }

    pub(super) fn menu(self, ui: &mut egui::Ui, movement: &mut Option<Move>) {
        for (label, enabled, boundary) in [
            (
                "Move Up",
                self.position > 0,
                self.position.saturating_sub(1),
            ),
            (
                "Move Down",
                self.position + 1 < self.count,
                self.position + 2,
            ),
        ] {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                *movement = Some(Move {
                    effect: self.effect,
                    boundary,
                });
                ui.close_menu();
            }
        }
    }

    pub(super) fn drop_target(self, ui: &mut egui::Ui, rect: egui::Rect) -> Option<Move> {
        let drag = egui::DragAndDrop::payload::<Drag>(ui.ctx())?;
        if drag.list != self.list || drag.effect == self.effect {
            return None;
        }
        let response = ui.interact(
            rect.expand(2.0),
            self.id().with("drop"),
            egui::Sense::hover(),
        );
        if response.dnd_hover_payload::<Drag>().is_some() {
            let after = ui.input(|input| input.pointer.hover_pos())?.y > rect.center().y;
            let y = if after {
                rect.bottom() + 2.0
            } else {
                rect.top() - 2.0
            };
            ui.painter().hline(
                rect.x_range(),
                y,
                egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
            );
            if response.dnd_release_payload::<Drag>().is_some() {
                return Some(Move {
                    effect: drag.effect,
                    boundary: self.position + usize::from(after),
                });
            }
        }
        None
    }
}

pub(super) fn apply_move(recipe: &mut PerkRecipe, movement: Move) {
    if movement.boundary > recipe.effects.len() {
        return;
    }
    let Some(from) = recipe
        .effects
        .iter()
        .position(|effect| effect.source_perk_index == movement.effect)
    else {
        return;
    };
    let to = movement.boundary - usize::from(from < movement.boundary);
    if from != to {
        let effect = recipe.effects.remove(from);
        recipe.effects.insert(to, effect);
    }
}

/// Keep distant cards reachable while dragging inside the effect scroll viewport.
pub(super) fn scroll_during_drag(ui: &mut egui::Ui, document: &str) {
    let Some(drag) = egui::DragAndDrop::payload::<Drag>(ui.ctx()) else {
        return;
    };
    if drag.list != egui::Id::new(("perk-effect-list", document)) {
        return;
    }
    let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let viewport = ui.clip_rect();
    if !viewport.contains(pointer) {
        return;
    }
    let edge = 36.0;
    let direction = if pointer.y < viewport.top() + edge {
        (viewport.top() + edge - pointer.y) / edge
    } else if pointer.y > viewport.bottom() - edge {
        -(pointer.y - viewport.bottom() + edge) / edge
    } else {
        0.0
    };
    if direction != 0.0 {
        let dt = ui.input(|input| input.stable_dt).min(0.05);
        ui.scroll_with_delta(egui::vec2(0.0, direction * 500.0 * dt));
        ui.ctx().request_repaint();
    }
}
