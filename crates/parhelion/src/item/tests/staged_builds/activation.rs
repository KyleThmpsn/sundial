use super::*;
use sundial::package_authoring::sandbox_perk::activation::PerkActivation;
use sundial::package_authoring::sandbox_perk::sandbox_perk_runtime_graph_sources;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn native_activation_clone_keeps_stock_effects_and_residency() {
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
    let globals = manager.read_tag(globals_tag).unwrap();
    let stock = load_sandbox_perk_runtime_action(&manager, &globals, 421).unwrap();
    let before = stock.action_payload.clone();
    let allocator = AppendedTagAllocator::new(PRIVATE_PERK_RUNTIME_PACKAGE_ID, 6469);
    for condition in PerkActivation::ALL {
        let mut tags = Vec::new();
        let authored = clone_private_sandbox_perk_runtime(
            &manager,
            &stock,
            custom_runtime::PrivateRuntimeEdits {
                activation: Some(condition),
                ..Default::default()
            },
            allocator,
            &mut tags,
        )
        .unwrap();
        assert_ne!(authored, stock.action_tag);
        assert_eq!(tags.len(), 5, "action plus four residency records");
        assert_eq!(tags[0].template_tag, stock.action_tag);
        let (labels, requires_weapon): (&[u32], u8) = match condition {
            PerkActivation::WeaponKill => (&[], 1),
            PerkActivation::PrecisionWeaponKill => (&[0x962E_A19B], 1),
            PerkActivation::MeleeKill => (&[0xBF39_E12B, 0xE175_76C9, 0x5D3A_7C84], 0),
            PerkActivation::GrenadeKill => (&[0xC20D_D425], 0),
            PerkActivation::AnyKill => (&[], 0),
        };
        verify_activation_gate(&tags[0].payload, labels, requires_weapon);
        sundial::package_authoring::sandbox_perk::action::decode(&tags[0].payload).unwrap();
        let graphs = sandbox_perk_runtime_graph_sources(&manager, &tags[0].payload).unwrap();
        assert_eq!(graphs.len(), stock.graphs.len());
        for (actual, expected) in graphs.iter().zip(&stock.graphs) {
            assert_eq!(actual.tag, expected.tag);
            assert_eq!(actual.payload, expected.payload);
            assert_eq!(actual.action_offsets, expected.action_offsets);
        }
        assert_eq!(manager.read_tag(stock.action_tag).unwrap(), before);
    }
}

fn verify_activation_gate(payload: &[u8], labels: &[u32], requires_weapon: u8) {
    for start in [0x100, 0x410] {
        assert_eq!(payload[start + 0x141], requires_weapon);
        let descriptor = start + 0xD0;
        if labels.is_empty() {
            assert_eq!(&payload[descriptor..descriptor + 16], &[0; 16]);
        } else {
            let (count, _, rows, class) = array_at(payload, descriptor).unwrap();
            assert_eq!(class, 0x8080_94B3);
            assert_eq!(count, labels.len());
            for (index, expected) in labels.iter().enumerate() {
                assert_eq!(read_u32(payload, rows + index * 24).unwrap(), *expected);
            }
        }
    }
}
