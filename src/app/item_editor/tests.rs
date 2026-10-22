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
