use super::*;

#[test]
fn shared_keys_cannot_select_a_removable_members_private_profiles() {
    let row = Row {
        key: 0xF123_4567,
        value: Modifier::Melee {
            damage: TagHash::new(0x0CFB, 0).0,
            responses: None,
        }
        .into(),
        mode: None,
    };
    let mut filed = Filed::default();
    assert!(filed.first(0x80BC_32F8, &row, "Destiny 1").unwrap());
    let mut other = row.clone();
    other.value = Modifier::Melee {
        damage: TagHash::new(0x0CFA, 0).0,
        responses: None,
    }
    .into();
    assert!(!filed.first(0x80BC_32F8, &other, "Destiny 1").unwrap());
    let error = filed
        .first(0x80BC_32F8, &other, "Halo: Reach")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Destiny 1") && error.contains("Halo: Reach") && error.contains("F1234567")
    );
    assert!(filed.first(0x80BC_32F8, &row, "Halo: Reach").is_err());
    let mut safe = Filed::default();
    let charge = Row {
        value: Modifier::Charges(1).into(),
        ..row
    };
    assert!(safe.first(0x80BC_32F8, &charge, "Destiny 1").unwrap());
    assert!(!safe.first(0x80BC_32F8, &charge, "Halo: Reach").unwrap());
}
