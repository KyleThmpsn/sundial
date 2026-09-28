use super::*;

#[test]
fn key_names_refresh_with_the_catalog_without_rebuilding_on_each_frame() {
    use sundial::investment::{IngredientCatalog, PerkSource};
    let index: KeyIndex = serde_json::from_value(serde_json::json!({
        "perks": [[1, 100]],
        "actions": {"100": {"properties": [{"key": 42, "value": 1065353216,
            "target": 2, "operation": 1, "removal": 0}], "removals": []}},
        "issues": []
    }))
    .unwrap();
    let index = Arc::new(index);
    let sources = |name: &str| {
        Arc::new(IngredientCatalog {
            choices: Vec::new(),
            references: [(
                1,
                PerkSource {
                    hash: 1,
                    name: name.into(),
                    type_name: "Test".into(),
                },
            )]
            .into_iter()
            .collect(),
            sources: BTreeMap::new(),
            context: BTreeMap::new(),
        })
    };
    let original = sources("Test Perk");
    let mut keys = Keys::default();
    keys.sync(Some(&index), Some(&original));
    assert_eq!(keys.catalog.property_key(42).unwrap().perks, ["Test Perk"]);
    let address = keys.catalog.property_keys().as_ptr();
    keys.sync(Some(&index), Some(&original));
    assert_eq!(address, keys.catalog.property_keys().as_ptr());
    keys.sync(Some(&index), Some(&sources("Renamed Test Perk")));
    assert_eq!(
        keys.catalog.property_key(42).unwrap().perks,
        ["Renamed Test Perk"]
    );
    keys.sync(None, None);
    assert!(keys.catalog.property_keys().is_empty());
}

#[test]
fn native_actions_preserve_editable_retained_state() {
    use sundial::package_authoring::sandbox_perk::action::native::{Graph, fields};
    for layout in layout::EFFECT_LAYOUTS {
        let node = NativeNode::effect(layout.kind).unwrap();
        let entry = nodes::effect(node.kind).unwrap();
        let mut graph = Graph::read(&node.bytes, 0, entry.class).unwrap();
        let fields = fields::describe(entry.class).unwrap();
        let retained = fields.iter().find(|field| field.offset == 1).unwrap();
        assert!(retained.editable);
        retained
            .write(&mut graph.blocks[0], 0, &[1 - node.bytes[1]])
            .unwrap();
        let program = Program {
            actions: vec![Action::Native {
                node: NativeNode {
                    kind: node.kind,
                    bytes: graph.emit().unwrap(),
                },
            }],
            ..Program::default()
        };
        assert!(program.validate_structure().is_ok(), "kind {}", node.kind);
    }
}
