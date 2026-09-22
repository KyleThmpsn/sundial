use super::*;

#[test]
fn link_index_keeps_direction_and_groups_unique_resources_by_nonzero_type() {
    let link = tft::Reference {
        source: 1,
        source_class: 10,
        target: 2,
        target_class: 20,
        offset: 8,
        path: "asset.tft".into(),
    };
    let index = tft::Index {
        references: vec![
            link.clone(),
            tft::Reference {
                offset: 16,
                ..link.clone()
            },
            tft::Reference {
                source: 2,
                source_class: 20,
                target: 3,
                target_class: 0,
                ..link
            },
        ],
        ..Default::default()
    };
    let navigation = Navigation::new(&index);
    assert_eq!(navigation.outgoing[&1], [0, 1]);
    assert_eq!(navigation.incoming[&2], [0, 1]);
    assert_eq!(navigation.classes[&20], BTreeSet::from([2]));
    assert!(!navigation.classes.contains_key(&0));
    assert_eq!(navigation.types[&2], BTreeSet::from([20]));
}

#[test]
fn resource_and_type_history_returns_to_the_original_results() {
    let mut navigation = Navigation::default();
    navigation.open(Destination::Resource(1));
    navigation.open(Destination::Resource(1));
    navigation.open(Destination::Class(20));
    navigation.open(Destination::Resource(2));
    assert_eq!(navigation.history.len(), 3);
    navigation.history.pop();
    assert_eq!(navigation.history.last(), Some(&Destination::Class(20)));
    navigation.history.pop();
    assert_eq!(navigation.history.last(), Some(&Destination::Resource(1)));
    navigation.clear();
    assert!(navigation.history.is_empty());
}
