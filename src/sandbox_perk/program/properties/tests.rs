use crate::sandbox_perk::action::{self, fixtures::Builder};

pub(super) fn payload(key: u32, value: f32) -> Vec<u8> {
    let mut out = Builder::new();
    let activation = out.event_key(99);
    out.pointer_list(
        action::PRIMARY_GROUP + action::GROUP_ACTIVATION,
        action::CONDITION_ROW_CLASS,
        &[activation],
    );
    let property = out.named_property(key, value, 2, 1, 0);
    out.pointer_list(
        action::PRIMARY_GROUP + action::GROUP_EFFECTS,
        action::EFFECT_ROW_CLASS,
        &[property, property],
    );
    let removal = out.event_key(7);
    out.pointer_list(
        action::PRIMARY_GROUP + action::GROUP_REMOVAL,
        action::CONDITION_ROW_CLASS,
        &[removal],
    );
    out.finish()
}
