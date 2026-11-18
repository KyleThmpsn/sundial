use super::*;

#[test]
fn hosted_key_conflicts_leave_the_served_scene_unchanged() {
    let first = Row {
        hash: 1,
        bucket: 2,
        key: 3,
        pair: TagHash::new(0x0CFB, 0).0,
        subs: Vec::new(),
    };
    let mut table = Table {
        rows: Vec::new(),
        by_hash: Vec::new(),
        by_key: Vec::new(),
    };
    table.insert(first.clone()).unwrap();
    let saved = table.serialize().unwrap();
    let mut restored = Table::parse(&saved).unwrap();
    let mut other = first.clone();
    other.pair = TagHash::new(0x0CFA, 0).0;
    assert!(restored.insert_hosted(other).is_err());
    assert_eq!(restored.serialize().unwrap(), saved);
    assert!(!restored.insert_hosted(first.clone()).unwrap());
    let mut independent = first;
    independent.key = 4;
    independent.hash = 5;
    independent.pair = TagHash::new(0x0CFA, 0).0;
    assert!(restored.insert_hosted(independent.clone()).unwrap());
    let readback = Table::parse(&restored.serialize().unwrap()).unwrap();
    assert_eq!(readback.row(2, 3).unwrap().pair, TagHash::new(0x0CFB, 0).0);
    assert_eq!(readback.row(2, 4).unwrap(), &independent);
}
