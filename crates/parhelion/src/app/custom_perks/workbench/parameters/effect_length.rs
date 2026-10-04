//! The Attachment Length tile: an attached effect's length, read and written through the same
//! helpers as every native field, so the edit is one override on the timer's constant row.
use super::{PerkEditor, PrivatePerkRuntimeGraph, guided, native};
use sundial::package_authoring::{
    runtime::{WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeValueOverride},
    sandbox_perk::entity::effect_length::{self, EffectLength},
};

/// An effect length with the graph it is on.
pub(in crate::app::custom_perks) struct Length {
    pub graph: u32,
    pub length: EffectLength,
}

pub(in crate::app::custom_perks) fn discover(loaded: &PrivatePerkRuntimeGraph) -> Vec<Length> {
    loaded
        .graphs
        .iter()
        .flat_map(|(tag, graph)| {
            effect_length::discover(graph)
                .into_iter()
                .map(move |length| Length {
                    graph: *tag,
                    length,
                })
        })
        .collect()
}

impl Length {
    fn carrier<'a>(&self, loaded: &'a PrivatePerkRuntimeGraph) -> Option<&'a WeaponRuntimeField> {
        let graph = &loaded.graphs.iter().find(|(tag, _)| *tag == self.graph)?.1;
        native::carrier(graph, self.length.owner_tag, &self.length.field)
    }

    pub(super) fn value(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<f32, String> {
        let value = native::current_value(loaded, &self.length.field, self.carrier(loaded), draft)?;
        effect_length::seconds(&value).ok_or_else(|| {
            "Attachment Length lanes disagree. Remove the edit before continuing.".into()
        })
    }

    pub(super) fn is_modified(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> bool {
        self.value(loaded, draft)
            .is_ok_and(|value| value != self.length.stock())
    }

    pub(super) fn set(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        seconds: f32,
    ) -> Result<(), String> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err("Attachment Length must be more than zero seconds.".into());
        }
        native::write_value(
            loaded,
            &self.length.field,
            self.carrier(loaded),
            draft,
            &EffectLength::encode(seconds),
        )
    }

    pub(super) fn reset(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
    ) -> Result<(), String> {
        native::write_value(
            loaded,
            &self.length.field,
            self.carrier(loaded),
            draft,
            &self.length.field.value,
        )
    }

    /// Whether this is the field the tile edits, so the change list names it once.
    pub(super) fn targets_field(&self, owner: u32, field: &WeaponRuntimeField) -> bool {
        owner == self.length.owner_tag && field.locator == self.length.field.locator
    }

    /// Whether an edit under `locator` is this length's, directly or through its carrier.
    pub(super) fn contains(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        locator: &WeaponRuntimeFieldLocator,
    ) -> bool {
        guided::equivalent(loaded, &self.length.field.locator, locator)
            || self
                .carrier(loaded)
                .is_some_and(|carrier| guided::equivalent(loaded, &carrier.locator, locator))
    }

    fn hint(&self) -> String {
        let stock = self.length.stock();
        match self.length.per_input {
            None => format!("Seconds until the attachment ends on its own. Stock {stock} s."),
            Some(scale) => format!(
                "Seconds until the attachment ends on its own, before its input adds {scale} s per unit. Stock {stock} s."
            ),
        }
    }
}

/// The tile. Returns the result of an edit, or nothing when the reader left it alone.
pub(in crate::app::custom_perks) fn draw(
    ui: &mut egui::Ui,
    width: f32,
    loaded: &PrivatePerkRuntimeGraph,
    length: &Length,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
) -> Option<Result<(), String>> {
    let current = length.value(loaded, draft);
    let mut value = current.clone().unwrap_or(length.length.stock());
    let label = "Attachment Length";
    let hint = length.hint();
    let (changed, reset) = crate::app::style::tile(
        ui,
        width,
        ("effect-length", length.graph, length.length.owner_tag),
        label,
        &hint,
        length.is_modified(loaded, draft),
        |ui| {
            let response = ui
                .add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut value)
                        .speed(0.1)
                        .max_decimals(2)
                        .range(0.05..=3600.0)
                        .clamp_existing_to_range(false)
                        .suffix(" s"),
                )
                .on_hover_text(format!("Original: {} s", length.length.stock()));
            let changed = response.changed();
            crate::app::style::named_control(response, label);
            if let Err(error) = &current {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            changed
        },
    );
    if reset {
        Some(length.reset(loaded, draft))
    } else if changed {
        Some(length.set(loaded, draft, value))
    } else {
        None
    }
}

impl PerkEditor {
    /// Draws every effect length of an independently opened entity. Returns whether any was.
    pub(super) fn draw_effect_lengths(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
    ) -> bool {
        let lengths = discover(loaded);
        if lengths.is_empty() {
            return false;
        }
        ui.add_space(8.0);
        ui.strong("Attachment Properties");
        crate::app::style::tiles(ui, |ui, width| {
            for length in &lengths {
                if let Some(result) = draw(ui, width, loaded, length, &mut self.draft) {
                    self.parameter_error = result.err();
                    self.value_text
                        .retain(|(locator, _), _| !length.contains(loaded, locator));
                }
            }
        });
        true
    }
}
