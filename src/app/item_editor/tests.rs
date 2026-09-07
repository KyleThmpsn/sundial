use super::*;

#[test]
fn responsive_item_cards_share_the_same_bounded_width_rules() {
    const STANDARD_MIN: f32 = 335.0;
    const STANDARD_MAX: f32 = 390.0;
    assert_eq!(
        responsive_item_card_layout(900.0, 8.0, 0, STANDARD_MIN, STANDARD_MAX),
        None
    );

    let (narrow_columns, narrow_width) =
        responsive_item_card_layout(660.0, 8.0, 3, STANDARD_MIN, STANDARD_MAX).unwrap();
    assert_eq!(narrow_columns, 1);
    assert!((narrow_width - STANDARD_MAX).abs() < 0.5);

    let (default_columns, default_width) =
        responsive_item_card_layout(710.0, 8.0, 8, STANDARD_MIN, STANDARD_MAX).unwrap();
    assert_eq!(default_columns, 2);
    let default_card_width = (default_width - 8.0) / 2.0;
    assert!((STANDARD_MIN..=STANDARD_MAX).contains(&default_card_width));

    let (wide_columns, wide_width) =
        responsive_item_card_layout(1_400.0, 8.0, 8, STANDARD_MIN, STANDARD_MAX).unwrap();
    assert_eq!(wide_columns, 4);
    let card_width = (wide_width - 24.0) / 4.0;
    assert!((STANDARD_MIN..=STANDARD_MAX).contains(&card_width));

    let (compact_columns, compact_width) =
        responsive_item_card_layout(950.0, 8.0, 8, 285.0, 315.0).unwrap();
    let (standard_columns, standard_width) =
        responsive_item_card_layout(950.0, 8.0, 8, STANDARD_MIN, STANDARD_MAX).unwrap();
    let (wide_columns, wide_width) =
        responsive_item_card_layout(950.0, 8.0, 8, 430.0, 520.0).unwrap();
    assert_eq!(compact_columns, 3);
    assert_eq!(standard_columns, 2);
    assert_eq!(wide_columns, 2);
    let compact_card_width = (compact_width - 16.0) / 3.0;
    let standard_card_width = (standard_width - 8.0) / 2.0;
    let wide_card_width = (wide_width - 8.0) / 2.0;
    assert!(compact_card_width < standard_card_width);
    assert!(standard_card_width < wide_card_width);
}

#[test]
fn authored_levels_display_as_in_game_power() {
    assert_eq!(displayed_item_power(0), 0);
    assert_eq!(displayed_item_power(1), 750);
    assert_eq!(displayed_item_power(74), 750);
    assert_eq!(displayed_item_power(75), 750);
    assert_eq!(displayed_item_power(106), 1_060);
    assert_eq!(
        displayed_item_power(i64::from(i32::MAX)),
        i64::from(i32::MAX) * 10
    );
}

#[test]
fn entered_power_snaps_down_and_converts_to_authored_level() {
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
fn power_input_uses_the_item_cap_or_build_fallback() {
    assert_eq!(item_power_input_max(Some(1_310)), 1_310);
    assert_eq!(item_power_input_max(Some(1_060)), 1_060);
    assert_eq!(item_power_input_max(None), 1_060);
}

#[test]
fn experimental_power_input_can_exceed_the_item_cap() {
    assert_eq!(effective_power_input_max(Some(1_310), false), 1_310);
    assert_eq!(effective_power_input_max(Some(1_310), true), 2_147_483_640);
    assert_eq!(
        authored_item_level(effective_power_input_max(Some(1_310), true)),
        Some(214_748_364)
    );
}

#[test]
fn new_inventory_items_start_at_default_power_within_their_cap() {
    assert_eq!(new_inventory_item_level(0, Some(1_310)), 106);
    assert_eq!(new_inventory_item_level(7, Some(1_360)), 106);
    assert_eq!(new_inventory_item_level(0, Some(999_990)), 106);
    assert_eq!(new_inventory_item_level(0, Some(1_010)), 101);
    assert_eq!(item_power_input_max(Some(999_990)), 999_990);
    assert_eq!(new_inventory_item_level(0, None), 106);
    assert_eq!(new_inventory_item_level(8, Some(1_360)), 0);
}
