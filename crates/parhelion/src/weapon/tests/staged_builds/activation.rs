use super::*;
use sundial::package_authoring::sandbox_perk::activation::{PerkActivation, with_activation};
use sundial::package_authoring::sandbox_perk::sandbox_perk_runtime_graph_sources;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn native_activation_clone_keeps_stock_effects_and_residency() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
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
        assert_eq!(
            tags[0].payload,
            with_activation(stock.action_tag.0, &before, condition).unwrap()
        );
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
