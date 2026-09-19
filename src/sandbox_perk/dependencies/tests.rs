use super::*;

#[test]
fn discovery_recognizes_a_stock_action_that_fits_the_program_model() {
    let payload = crate::sandbox_perk::action::fixtures::drawn_pattern_action();
    let behavior = Behavior::read(&payload).unwrap();
    assert!(behavior.editable);
    assert_eq!(
        behavior.support,
        crate::sandbox_perk::nodes::Support::Authorable
    );
}

#[test]
fn cached_reading_retains_asset_paths_values_and_owned_timer_conditions() {
    let drawn =
        Behavior::read(&crate::sandbox_perk::action::fixtures::drawn_pattern_action()).unwrap();
    let effects = drawn
        .details
        .iter()
        .find(|section| section.heading == "Then")
        .unwrap();
    assert_eq!(effects.lines[0].asset, Some(0x8161_F73A));
    assert!(
        effects.lines[0]
            .fields
            .iter()
            .any(|field| field.contains("content/sandbox/weapons/demo/demo.pattern.tft"))
    );
    let precision =
        Behavior::read(&crate::sandbox_perk::action::fixtures::precision_kill_action()).unwrap();
    assert_eq!(
        precision
            .details
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>(),
        ["Starts When", "Then", "Ends When", "Ready Again When"]
    );
    let effects = precision
        .details
        .iter()
        .find(|section| section.heading == "Then")
        .unwrap();
    assert!(
        effects
            .lines
            .iter()
            .flat_map(|line| &line.fields)
            .any(|field| field.contains("precision"))
    );
    assert!(
        precision
            .details
            .iter()
            .flat_map(|section| &section.lines)
            .flat_map(|line| &line.fields)
            .any(|field| field.contains("2.5"))
    );
    let json = serde_json::to_vec(&precision).unwrap();
    assert_eq!(
        serde_json::from_slice::<Behavior>(&json).unwrap(),
        precision
    );
}

#[test]
fn unresolved_markers_are_not_reported_as_working_actions() {
    let mut perk = Perk {
        index: 2002,
        hash: 1,
        runtime_key: 0x811C_9DC5,
        action: None,
        graphs: Vec::new(),
        error: None,
        behavior: None,
    };
    assert_eq!(perk.status(), "No Standalone Action");
    perk.action = Some(1);
    assert_eq!(perk.status(), "Action With No Direct Entity Graph");
    perk.graphs.push(Entity {
        tag: 2,
        components: Vec::new(),
    });
    assert_eq!(perk.status(), "Action With Entity Graphs");
    perk.error = Some("Bad graph".into());
    assert_eq!(perk.status(), "Inspection Failed");
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn installed_dependency_inventory_retains_marker_and_shared_pattern_evidence() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("clean packages");
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
