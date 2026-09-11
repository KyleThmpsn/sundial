use super::*;

/// A tolerant, read-only view of one non-null character equipment slot.
///
/// Unlike the editable equipment UI, this snapshot deliberately retains malformed
/// rows so callers can still show the authored data and its issues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct EquippedItemSnapshot {
    pub slot: &'static str,
    pub slot_label: &'static str,
    pub bucket_hash: u64,
    pub raw_item_text: String,
    pub definition_hash: Option<u64>,
    pub definition_text: String,
    pub instance_soid: Option<u64>,
    pub instance_soid_text: String,
    pub level: Option<i64>,
    pub quantity: Option<i64>,
    pub flags: Option<u8>,
    pub plugs: EquippedItemPlugs,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum EquippedItemPlugs {
    NativeDefaults,
    Authored(Vec<EquippedPlugValue>),
    Missing,
    Malformed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum EquippedPlugValue {
    Empty,
    Hash(u64),
    Malformed(String),
}

impl EquippedItemPlugs {
    /// Preserves malformed plug values for the read-only editor display.
    pub(in crate::app) fn display_value(&self) -> Option<Value> {
        match self {
            Self::NativeDefaults => Some(Value::Null),
            Self::Authored(plugs) => Some(Value::Array(
                plugs
                    .iter()
                    .map(|plug| match plug {
                        EquippedPlugValue::Empty => Value::Null,
                        EquippedPlugValue::Hash(hash) => Value::from(*hash),
                        EquippedPlugValue::Malformed(value) => Value::String(value.clone()),
                    })
                    .collect(),
            )),
            Self::Missing | Self::Malformed(_) => None,
        }
    }
}

pub(in crate::app) struct EquipmentSlotCard<'a> {
    pub id_scope: &'static str,
    pub slot: &'static str,
    pub label: &'a str,
    pub bucket_hash: u64,
    pub class_type: u64,
    pub editable: bool,
    pub header_fill: Option<egui::Color32>,
    pub snapshot: Option<&'a EquippedItemSnapshot>,
}
