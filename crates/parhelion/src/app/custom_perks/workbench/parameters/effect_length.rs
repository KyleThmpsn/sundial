//! The Attachment Length tile: an attached effect's length, read and written through the same
//! helpers as every native field, so the edit is one override on the timer's constant row.
//! Unlimited is written as the stock timers that never end are: -1, the timer's Unlimited flag,
//! and zero for the scale of a timer that scales an input.
use super::{PerkEditor, PrivatePerkRuntimeGraph, guided, native};
use crate::app::style;
use sundial::package_authoring::{
    runtime::{
        WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeValue,
        WeaponRuntimeValueOverride,
    },
    sandbox_perk::entity::effect_length::{self, EffectLength, UNLIMITED, UnlimitedFlag},
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

/// A length as the tile reads it: seconds, or Unlimited for none.
pub(super) fn text(length: Option<f32>) -> String {
    style::length_text(f64::from(length.unwrap_or(UNLIMITED)), length.is_none())
}

impl Length {
    fn carrier<'a>(
        &self,
        loaded: &'a PrivatePerkRuntimeGraph,
        field: &WeaponRuntimeField,
    ) -> Option<&'a WeaponRuntimeField> {
        let graph = &loaded.graphs.iter().find(|(tag, _)| *tag == self.graph)?.1;
        native::carrier(graph, self.length.owner_tag, field)
    }

    fn read(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        field: &WeaponRuntimeField,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<WeaponRuntimeValue, String> {
        native::current_value(loaded, field, self.carrier(loaded, field), draft)
    }

    fn write(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        field: &WeaponRuntimeField,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        value: &WeaponRuntimeValue,
    ) -> Result<(), String> {
        native::write_value(loaded, field, self.carrier(loaded, field), draft, value)
    }

    fn seconds(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        field: &WeaponRuntimeField,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<f32, String> {
        effect_length::seconds(&self.read(loaded, field, draft)?).ok_or_else(|| {
            "Attachment Length lanes disagree. Remove the edit before continuing.".into()
        })
    }

    /// The seconds the attachment lasts, or nothing when it never ends.
    pub(super) fn value(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<Option<f32>, String> {
        let seconds = self.seconds(loaded, &self.length.field, draft)?;
        let scale = self
            .length
            .input_scale
            .as_ref()
            .map(|scale| self.seconds(loaded, scale, draft))
            .transpose()?;
        let flag = match &self.length.unlimited {
            Some(flag) => flag.is_set(&self.read(loaded, &flag.field, draft)?),
            None => false,
        };
        Ok((!effect_length::is_unlimited(seconds, scale, flag)).then_some(seconds))
    }

    /// The stock seconds, or nothing when the stock timer never ends.
    fn stock(&self) -> Option<f32> {
        (!self.length.stock_unlimited()).then(|| self.length.stock())
    }

    fn reads_unlimited(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> bool {
        matches!(self.value(loaded, draft), Ok(None))
    }

    pub(super) fn is_modified(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> bool {
        self.value(loaded, draft)
            .is_ok_and(|value| value != self.stock())
    }

    /// Sets the length, or Unlimited when `seconds` is below zero.
    pub(super) fn set(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        seconds: f32,
    ) -> Result<(), String> {
        if !seconds.is_finite() || seconds == 0.0 {
            return Err("Attachment Length must be more than zero seconds.".into());
        }
        if seconds < 0.0 {
            let flag = self
                .length
                .unlimited
                .as_ref()
                .ok_or("This attachment cannot be Unlimited.")?;
            if let Some(scale) = &self.length.input_scale {
                self.write(loaded, scale, draft, &EffectLength::encode(0.0))?;
            }
            self.write_flag(loaded, flag, draft, true)?;
            return self.write(
                loaded,
                &self.length.field,
                draft,
                &EffectLength::encode(UNLIMITED),
            );
        }
        if self.reads_unlimited(loaded, draft) {
            self.leave_unlimited(loaded, draft)?;
        }
        self.write(
            loaded,
            &self.length.field,
            draft,
            &EffectLength::encode(seconds),
        )
    }

    pub(super) fn reset(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
    ) -> Result<(), String> {
        if self.reads_unlimited(loaded, draft) {
            self.leave_unlimited(loaded, draft)?;
        }
        self.write(loaded, &self.length.field, draft, &self.length.field.value)
    }

    /// Takes back the stock input scale and flag that Unlimited changes.
    fn leave_unlimited(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
    ) -> Result<(), String> {
        if let Some(scale) = &self.length.input_scale {
            self.write(loaded, scale, draft, &scale.value)?;
        }
        if let Some(flag) = &self.length.unlimited {
            self.write_flag(loaded, flag, draft, flag.stock())?;
        }
        Ok(())
    }

    fn write_flag(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        flag: &UnlimitedFlag,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        set: bool,
    ) -> Result<(), String> {
        let value = flag
            .with(&self.read(loaded, &flag.field, draft)?, set)
            .ok_or("The Unlimited flag is not in its bytes here.")?;
        self.write(loaded, &flag.field, draft, &value)
    }

    /// Whether this is a field the tile edits, so the change list names it once: the length,
    /// and while the attachment is Unlimited, its input's scale.
    pub(super) fn targets_field(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
        owner: u32,
        field: &WeaponRuntimeField,
    ) -> bool {
        owner == self.length.owner_tag
            && (field.locator == self.length.field.locator
                || self
                    .length
                    .input_scale
                    .as_ref()
                    .is_some_and(|scale| field.locator == scale.locator)
                    && self.reads_unlimited(loaded, draft))
    }

    /// The Unlimited flag while the attachment is Unlimited, so the change list leaves its
    /// byte to this tile.
    pub(super) fn unlimited_flag(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Option<&UnlimitedFlag> {
        self.length
            .unlimited
            .as_ref()
            .filter(|_| self.reads_unlimited(loaded, draft))
    }

    /// Whether an edit under `locator` is this length's, directly or through its carrier.
    pub(super) fn contains(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        locator: &WeaponRuntimeFieldLocator,
    ) -> bool {
        std::iter::once(&self.length.field)
            .chain(self.length.input_scale.as_ref())
            .chain(self.length.unlimited.as_ref().map(|flag| &flag.field))
            .any(|field| {
                guided::equivalent(loaded, &field.locator, locator)
                    || self.carrier(loaded, field).is_some_and(|carrier| {
                        guided::equivalent(loaded, &carrier.locator, locator)
                    })
            })
    }

    fn hint(&self) -> String {
        let stock = text(self.stock());
        let unlimited = if self.length.unlimited.is_some() {
            ", or Unlimited"
        } else {
            ""
        };
        match self.length.per_input {
            None => {
                format!("Seconds until the attachment ends on its own{unlimited}. Stock {stock}.")
            }
            Some(scale) => format!(
                "Seconds until the attachment ends on its own, before its input adds {scale} s per unit{unlimited}. Stock {stock}."
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
    let mut value = f64::from(
        current
            .clone()
            .unwrap_or(length.stock())
            .unwrap_or(UNLIMITED),
    );
    // Below zero is Unlimited, for a timer that can be.
    let unlimited = length.length.unlimited.is_some();
    let floor = if unlimited {
        -1.0
    } else {
        style::SHORTEST_LENGTH
    };
    let label = "Attachment Length";
    let hint = length.hint();
    let (changed, reset) = style::tile(
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
                        .range(floor..=3600.0)
                        .clamp_existing_to_range(false)
                        .custom_formatter(move |value, _| {
                            style::length_text(value, unlimited && value < 0.0)
                        })
                        .custom_parser(style::parse_length),
                )
                .on_hover_text(format!("Original: {}", text(length.stock())));
            let changed = response.changed();
            style::named_control(response, label);
            if let Err(error) = &current {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            changed
        },
    );
    if reset {
        Some(length.reset(loaded, draft))
    } else if changed {
        let seconds = if value < 0.0 {
            UNLIMITED
        } else {
            value.max(style::SHORTEST_LENGTH) as f32
        };
        Some(length.set(loaded, draft, seconds))
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
        style::tiles(ui, |ui, width| {
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
