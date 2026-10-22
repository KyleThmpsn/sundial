//! Recipe mutation regressions, including stale identities and preservation of sibling effects.
use super::*;

fn runtime_override(seed: u32) -> WeaponRuntimeValueOverride {
    WeaponRuntimeValueOverride {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: (0x1000_0000 | seed).into(),
            resource_index: 0,
            root: sundial::package_authoring::runtime::WeaponRuntimeRootKind::Instance,
            root_schema: (0x2000_0000 | seed).into(),
            path: Vec::new(),
            type_handle: (0x3000_0000 | seed).into(),
            value_offset: seed,
            byte_size: 4,
        },
        value: WeaponRuntimeValue::Unsigned(u64::from(seed)),
    }
}

#[test]
fn custom_perk_lookup_and_removal_reject_a_stale_source_plug() {
    let mut recipe = PackageAuthoringApp::default().recipe;
    let key = PerkEditorKey {
        socket_index: 2,
        choice_index: 0,
        source_plug_hash: 10,
        source_perk_index: 7,
    };
    upsert_private_perk_runtime_values(&mut recipe, key, vec![runtime_override(1)]);
    let stale = PerkEditorKey {
        source_plug_hash: 20,
        ..key
    };
    assert!(private_perk(&recipe, stale).is_none());
    remove_private_perk_runtime_values(&mut recipe, stale);
    assert_eq!(private_perk(&recipe, key).unwrap().runtime_values.len(), 1);
}
