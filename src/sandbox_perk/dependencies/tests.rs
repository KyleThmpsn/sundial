use super::*;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES"]
fn installed_dependency_inventory_retains_marker_and_shared_pattern_evidence() {
    let packages = crate::test_support::stock_packages();
    let manager =
        crate::package_authoring::open_shadowkeep_package_manager(std::path::Path::new(&packages))
            .unwrap();
    let mut last = (0, 0);
    let index = inspect(&manager, |done, total| {
        assert!(done > last.0);
        last = (done, total);
    })
    .unwrap();
    let errors = index
        .perks
        .iter()
        .filter_map(|perk| {
            perk.error
                .as_ref()
                .map(|error| format!("{}: {error}", perk.index))
        })
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    assert_eq!(last.0, last.1);
    assert_eq!(index.patterns.len() + index.perks.len(), last.0);
    assert_eq!(index.perks[2002].action, None);
    assert_eq!(index.perks[1778].action, Some(0x8157_978A));
    let wave = index.perks[1778]
        .behavior
        .as_ref()
        .expect("Wave Frame decodes");
    assert!(wave.effect_kinds.contains(&26), "{wave:?}");
    assert!(!wave.headline.is_empty());
    assert!(
        index.perks[1778]
            .graphs
            .iter()
            .any(|graph| graph.tag == 0x8161_F4DE)
    );
    for pattern in [108, 285, 367] {
        assert_eq!(
            index.patterns[pattern].entity.as_ref().unwrap().tag,
            0x80BB_B80C
        );
    }
    assert!(
        index.patterns[395]
            .entity
            .as_ref()
            .unwrap()
            .components
            .iter()
            .any(|component| component.binding == 0x2D8A_944C && component.owner == 0x81A6_ABFB)
    );
    assert_eq!(
        index
            .caster
            .as_ref()
            .unwrap()
            .projectile_graphs
            .iter()
            .map(|graph| graph.tag)
            .collect::<Vec<_>>(),
        [0x81A6_AB70, 0x81A6_ABA5]
    );
}
