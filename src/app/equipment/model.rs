use super::*;
pub(in crate::app) use crate::persistence::json_account::equipment::{
    EquippedItemPlugs, EquippedItemSnapshot, EquippedPlugValue,
};

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
