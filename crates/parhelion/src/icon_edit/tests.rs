use super::*;

#[test]
fn malformed_rgba_payload_is_rejected_without_mutation() {
    let mut pixels = [1, 2, 3];
    let before = pixels;
    assert!(
        WeaponIconEdit {
            invert: true,
            ..WeaponIconEdit::default()
        }
        .apply_to_rgba8(&mut pixels)
        .is_err()
    );
    assert_eq!(pixels, before);
}
