use super::*;
use crate::catalog::{InventoryMetadata, InventoryScope, ItemStackability};

#[test]
fn opening_an_over_cap_reward_does_not_edit_its_quantity() {
    let hash = 3159615086;
    let catalog = Catalog::for_test_with_inventory(
        vec![crate::catalog::ItemDef {
            hash,
            name: "Glimmer".into(),
            type_name: "Currency".into(),
            bucket_hash: 0,
            class_type: 3,
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        }],
        Default::default(),
        [(
            hash,
            InventoryMetadata {
                scope: InventoryScope::Profile,
                native_bucket_id: 0,
                stackability: ItemStackability::Stackable,
                max_stack_size: Some(250000),
                bucket_capacity: Some(1),
            },
        )]
        .into(),
    );
    assert!(
        catalog
            .inventory_definition(hash)
            .is_some_and(|d| supports_currency(d.metadata))
    );
    let debt = RewardDebt {
        id: 1,
        account_soid: 1,
        character_soid: 2,
        runtime_epoch: 1,
        session_id: 3,
        run_id: 4,
        mission_hash: 5,
        definition_hash: hash as u32,
        quantity: 300000,
        credited: 0,
        delivered: false,
    };
    let ctx = egui::Context::default();
    for _ in 0..3 {
        let mut edit = None;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| quantity(ui, &catalog, &debt, true, &mut edit));
        });
        assert!(edit.is_none());
    }
}
