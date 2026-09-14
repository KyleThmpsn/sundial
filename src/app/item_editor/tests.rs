use super::*;

#[test]
fn power_conversions_preserve_native_scaling_and_floor() {
    assert_eq!(displayed_item_power(0), 0);
    assert_eq!(displayed_item_power(1), 750);
    assert_eq!(displayed_item_power(74), 750);
    assert_eq!(displayed_item_power(75), 750);
    assert_eq!(displayed_item_power(106), 1_060);
    assert_eq!(
        displayed_item_power(i64::from(i32::MAX)),
        i64::from(i32::MAX) * 10
    );
    assert_eq!(authored_item_level(-1), None);
    assert_eq!(authored_item_level(0), Some(0));
    assert_eq!(authored_item_level(1), Some(75));
    assert_eq!(authored_item_level(749), Some(75));
    assert_eq!(authored_item_level(750), Some(75));
    assert_eq!(authored_item_level(759), Some(75));
    assert_eq!(authored_item_level(760), Some(76));
    assert_eq!(authored_item_level(1_060), Some(106));
}

#[test]
fn drawing_power_does_not_rewrite_existing_out_of_range_values() {
    egui::__run_test_ui(|ui| {
        let actions = draw_level_and_quantity(
            ui,
            "out-of-range-power",
            NumericItemFields {
                level: Some(200),
                power_max: Some(1_310),
                allow_power_above_cap: false,
                quantity: None,
                quantity_max: None,
            },
        );
        assert!(actions.is_empty());
    });
}

#[test]
fn new_item_defaults_and_power_input_respect_normal_and_experimental_caps() {
    assert_eq!(new_inventory_item_level(0, Some(1_310)), 106);
    assert_eq!(new_inventory_item_level(7, Some(1_360)), 106);
    assert_eq!(new_inventory_item_level(0, Some(999_990)), 106);
    assert_eq!(new_inventory_item_level(0, Some(1_010)), 101);
    assert_eq!(item_power_input_max(Some(999_990)), 999_990);
    assert_eq!(new_inventory_item_level(0, None), 106);
    assert_eq!(new_inventory_item_level(8, Some(1_360)), 0);
    assert_eq!(effective_power_input_max(Some(1_310), false), 1_310);
    assert_eq!(effective_power_input_max(Some(1_310), true), 2_147_483_640);
    assert_eq!(
        authored_item_level(effective_power_input_max(Some(1_310), true)),
        Some(214_748_364)
    );
}
