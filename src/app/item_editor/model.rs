use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativePlugDefault {
    Plug(u64),
    Empty,
}

impl NativePlugDefault {
    pub(crate) const fn value(self) -> Option<u64> {
        match self {
            Self::Plug(hash) => Some(hash),
            Self::Empty => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemEditorAction {
    SetDefinition {
        hash: u64,
    },
    EquipInventoryItem {
        item_index: usize,
    },
    OpenInRandomItemBuilder {
        hash: u64,
    },
    ClearDefinition,
    SetLevel {
        level: i64,
    },
    SetQuantity {
        quantity: i64,
    },
    SetPlug {
        socket_index: usize,
        hash: Option<u64>,
    },
}

pub(crate) enum DefinitionSummary<'a> {
    Empty,
    Known {
        name: &'a str,
        hash_display_text: &'a str,
        type_name: &'a str,
    },
    Unknown {
        hash_display_text: &'a str,
    },
}

pub(crate) struct ItemHeader<'a> {
    pub label: Option<&'a str>,
    pub soid: Option<&'a str>,
    pub definition: DefinitionSummary<'a>,
    pub icon: Option<egui::TextureHandle>,
    pub fill: egui::Color32,
    pub valid: bool,
    pub invalid_message: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DefinitionChoice {
    pub hash: u64,
    pub name: String,
    pub type_name: String,
    /// Optional browse grouping. Callers keep equal groups adjacent.
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExistingInventoryChoice {
    pub item_index: usize,
    pub hash: u64,
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ClearDefinitionChoice {
    pub label: String,
    pub tooltip: String,
    pub selected: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct DefinitionPickerChoices {
    pub definitions: Vec<DefinitionChoice>,
    pub existing_inventory: Vec<ExistingInventoryChoice>,
    pub clear: Option<ClearDefinitionChoice>,
    pub random_item_builder_hash: Option<u64>,
    pub empty_message: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PickerHeight {
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NumericItemFields {
    pub level: Option<i64>,
    pub power_max: Option<i64>,
    pub allow_power_above_cap: bool,
    pub quantity: Option<i64>,
    pub quantity_max: Option<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct PlugChoice {
    pub hash: u64,
    pub label: String,
    pub type_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PlugPickerSnapshot {
    pub socket_index: usize,
    pub socket_label: String,
    pub current_hash: Option<u64>,
    pub current_label: String,
    pub native_default: Option<NativePlugDefault>,
    pub native_default_label: Option<String>,
    pub choices: Vec<PlugChoice>,
    pub show_types: bool,
}
