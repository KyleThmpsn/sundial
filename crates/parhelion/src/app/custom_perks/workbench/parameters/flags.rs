//! The Use Retirement Delay tile: the one established bit of an invisibility attachment's
//! flag word, read and written through the same helpers as every native field, so the edit
//! is one override on the word. The word itself waits under More Properties.
use super::{PrivatePerkRuntimeGraph, native};
use sundial::package_authoring::runtime::{
    WeaponRuntimeField, WeaponRuntimeValue, WeaponRuntimeValueOverride,
};

/// The invisibility attachment record and its flag word.
const SCHEMA: u32 = 0x8080_43EC;
const OFFSET: u32 = 0x298;
/// The bit the retirement path tests before it starts the Retirement Delay timer.
const BIT: u64 = 0x20;

/// A flag word with the graph and owner it is on.
pub(in crate::app::custom_perks) struct Flag<'a> {
    pub graph: u32,
    pub owner: u32,
    pub field: &'a WeaponRuntimeField,
}

pub(in crate::app::custom_perks) fn discover(loaded: &PrivatePerkRuntimeGraph) -> Vec<Flag<'_>> {
    let mut flags = Vec::<Flag<'_>>::new();
    for (tag, graph) in &loaded.graphs {
        let roots = graph
            .resources
            .iter()
            .flat_map(|resource| {
                std::iter::once(&resource.instance)
                    .chain(resource.definition.iter())
                    .map(move |root| (resource.owner_tag, root))
            })
            .chain(
                graph
                    .owners
                    .iter()
                    .flat_map(|owner| owner.roots.iter().map(move |root| (owner.owner_tag, root))),
            );
        for (owner, root) in roots {
            for field in &root.fields {
                let word = field.locator.type_handle.get() == SCHEMA
                    && field.locator.value_offset == OFFSET
                    && matches!(field.value, WeaponRuntimeValue::Unsigned(_));
                if word
                    && !flags
                        .iter()
                        .any(|flag| flag.graph == *tag && flag.field.locator == field.locator)
                {
                    flags.push(Flag {
                        graph: *tag,
                        owner,
                        field,
                    });
                }
            }
        }
    }
    flags
}

impl Flag<'_> {
    fn carrier<'a>(&self, loaded: &'a PrivatePerkRuntimeGraph) -> Option<&'a WeaponRuntimeField> {
        let graph = &loaded.graphs.iter().find(|(tag, _)| *tag == self.graph)?.1;
        native::carrier(graph, self.owner, self.field)
    }

    fn stock(&self) -> u64 {
        match self.field.value {
            WeaponRuntimeValue::Unsigned(word) => word,
            _ => 0,
        }
    }

    fn word(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<u64, String> {
        match native::current_value(loaded, self.field, self.carrier(loaded), draft)? {
            WeaponRuntimeValue::Unsigned(word) => Ok(word),
            _ => Err(
                "Attachment Flags is not a flag word here. Remove the edit before continuing."
                    .into(),
            ),
        }
    }

    pub(super) fn value(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> Result<bool, String> {
        Ok(self.word(loaded, draft)? & BIT != 0)
    }

    pub(super) fn is_modified(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> bool {
        self.word(loaded, draft)
            .is_ok_and(|word| word & BIT != self.stock() & BIT)
    }

    /// Whether the word differs from stock in this bit alone, so the tile reads the whole
    /// change and the change list needs no line for the word.
    pub(super) fn covers(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &[WeaponRuntimeValueOverride],
    ) -> bool {
        self.word(loaded, draft)
            .is_ok_and(|word| (word ^ self.stock()) & !BIT == 0)
    }

    /// Whether this is the field the tile edits, so the change list names it once.
    pub(super) fn targets_field(&self, owner: u32, field: &WeaponRuntimeField) -> bool {
        owner == self.owner && field.locator == self.field.locator
    }

    fn write(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        word: u64,
    ) -> Result<(), String> {
        native::write_value(
            loaded,
            self.field,
            self.carrier(loaded),
            draft,
            &WeaponRuntimeValue::Unsigned(word),
        )
    }

    pub(super) fn set(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        on: bool,
    ) -> Result<(), String> {
        let word = self.word(loaded, draft)?;
        self.write(loaded, draft, if on { word | BIT } else { word & !BIT })
    }

    pub(super) fn reset(
        &self,
        loaded: &PrivatePerkRuntimeGraph,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
    ) -> Result<(), String> {
        let word = self.word(loaded, draft)?;
        self.write(loaded, draft, (word & !BIT) | (self.stock() & BIT))
    }
}

/// The tile. Returns the result of an edit, or nothing when the reader left it alone.
pub(in crate::app::custom_perks) fn draw(
    ui: &mut egui::Ui,
    width: f32,
    loaded: &PrivatePerkRuntimeGraph,
    flag: &Flag<'_>,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
) -> Option<Result<(), String>> {
    let current = flag.value(loaded, draft);
    let stock = flag.stock() & BIT != 0;
    let mut on = current.clone().unwrap_or(stock);
    let label = "Use Retirement Delay";
    let hint = format!(
        "Retires the attachment after Retirement Delay instead of at once. Stock {}.",
        if stock { "on" } else { "off" }
    );
    let (changed, reset) = crate::app::style::tile(
        ui,
        width,
        ("attachment-flag", flag.graph, flag.owner),
        label,
        &hint,
        flag.is_modified(loaded, draft),
        |ui| {
            let response = ui.checkbox(&mut on, "");
            let changed = response.changed();
            crate::app::style::named_control(response, label);
            if let Err(error) = &current {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            changed
        },
    );
    if reset {
        Some(flag.reset(loaded, draft))
    } else if changed {
        Some(flag.set(loaded, draft, on))
    } else {
        None
    }
}
