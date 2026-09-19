use super::*;

#[test]
fn a_shared_attachment_does_not_inherit_its_owner_as_a_direct_identity() {
    let mut asset = entry(12, Kind::Entity, 18);
    asset.contexts = vec![context(
        30,
        "content/characters/cabal/ultra_emperor_decoy.pattern.tft",
        None,
        2,
    )];
    assert!(asset.discovery_name().is_some());
    assert!(asset.direct_name().is_none());
    asset
        .native_paths
        .push("content/objects/d2_soccer_ball/d2_soccer_ball.pattern.tft".into());
    assert!(asset.direct_name().unwrap().contains("Soccer Ball"));
}

#[test]
fn naming_follows_deep_owners_and_terminates_reference_cycles() {
    let mut parents = (1..80)
        .map(|tag| (tag, vec![tag + 1]))
        .collect::<HashMap<_, _>>();
    parents.get_mut(&40).unwrap().push(1);
    let found = climb(1, &parents, &HashMap::new(), |tag| {
        (tag == 80)
            .then(|| Named::Path("content/vehicles/cabal/cabal_interceptor.pattern.tft".into()))
    });
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].depth, 79);
    assert_eq!(found[0].graph, 80);
    assert!(climb(1, &parents, &HashMap::new(), |_| None).is_empty());
}

#[test]
fn anonymous_perk_references_do_not_hide_a_native_ancestor() {
    let parents = HashMap::from([(1, vec![2, 3]), (3, vec![4])]);
    let contexts = climb(1, &parents, &HashMap::new(), |tag| match tag {
        2 => Some(Named::Perks(vec![2481])),
        4 => Some(Named::Path(
            "content/sandbox/vehicles/cabal/cabal_interceptor/cabal_interceptor.pattern.tft".into(),
        )),
        _ => None,
    });
    assert!(
        contexts
            .iter()
            .any(|context| context.graph == 4 && context.depth == 2)
    );
    assert!(contexts.iter().any(|context| context.perk == Some(2481)));
    let entry = Entry {
        graph: 1,
        kind: Kind::Projectile,
        object_type: 18,
        owners: vec![],
        package: String::new(),
        native_name: None,
        native_paths: vec![],
        contexts,
        perk_indices: vec![2481],
        source_hint: None,
    };
    for perk_name in ["Effect 2481", "A Custom Perk"] {
        assert_eq!(
            entry
                .discovery_name_with(|_| Some(perk_name.into()), |_| None)
                .as_deref(),
            Some("Cabal Interceptor Projectile")
        );
    }
}

#[test]
fn a_legacy_name_on_an_ancestor_does_not_stop_the_climb() {
    // Asset 1 is bound by 2, which only a Destiny 1 template name covers, and 2 is bound by
    // 3, which an installed path names. The installed name must still be found, and the
    // legacy one must rank as a fallback below it.
    let parents = HashMap::from([(1, vec![2]), (2, vec![3])]);
    let contexts = climb(1, &parents, &HashMap::new(), |tag| match tag {
        2 => Some(Named::Symbol(NameEvidence {
            name: "frag_grenade".into(),
            hash: 0xDAD7E57E,
            source: "Destiny 1 PS4 alpha wwise event frag_grenade_throw".into(),
            legacy: true,
        })),
        3 => Some(Named::Path(
            "content/sandbox/vehicles/cabal/cabal_interceptor/cabal_interceptor.pattern.tft".into(),
        )),
        _ => None,
    });
    let installed = contexts
        .iter()
        .find(|context| context.graph == 3)
        .expect("the installed ancestor is still reached");
    assert_eq!(installed.depth, 2);
    let legacy = contexts
        .iter()
        .find(|context| context.graph == 2)
        .expect("the legacy name is kept as a fallback");
    assert_eq!(legacy.depth, LEGACY_DEPTH);
    assert!(legacy.name_evidence.as_ref().is_some_and(|e| e.legacy));
    // An installed symbol on the ancestor still stops the climb as before.
    let direct = climb(1, &parents, &HashMap::new(), |tag| match tag {
        2 => Some(Named::Symbol(NameEvidence {
            name: "cabal_interceptor".into(),
            hash: 1,
            source: String::new(),
            legacy: false,
        })),
        _ => None,
    });
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].depth, 1);
}

#[test]
fn shared_ancestor_keeps_competing_paths_instead_of_naming_the_first() {
    let contexts = climb(1, &HashMap::from([(1, vec![2])]), &HashMap::new(), |_| {
        Some(Named::Paths(vec![
            "fallen_shank.pattern.tft".into(),
            "fallen_captain.pattern.tft".into(),
        ]))
    });
    assert_eq!(contexts.len(), 2);
    let mut asset = entry(1, Kind::Projectile, 18);
    asset.contexts = contexts;
    // Neither path wins. The label states both, which is what the resource records.
    assert_eq!(
        asset.discovery_name().as_deref(),
        Some("Shared by Fallen Captain, Fallen Shank Projectile")
    );
}

#[test]
fn pickup_roles_require_direct_source_names_and_preserve_related_operations() {
    let mut asset = entry(7, Kind::Emitter, 17);
    asset
        .native_paths
        .push("content/prophecy/collectable_bauble_spawner_dark.pattern.tft".into());
    assert_eq!(asset.pickup_role(), Some("Pickup Spawner"));
    asset.native_paths = vec!["dark_bauble_pickup_feedback_hopon.pattern.tft".into()];
    assert_eq!(asset.pickup_role(), Some("Pickup Feedback"));
    asset.native_paths.clear();
    asset.contexts = vec![context(1, "health_orb_pickup.pattern.tft", None, 1)];
    assert_eq!(
        asset.pickup_role(),
        None,
        "a parent pickup does not identify its nested effect"
    );
}

#[test]
fn perk_referenced_pickups_get_common_source_names_without_ancestry_paths() {
    let mut asset = entry(7, Kind::Emitter, 17);
    asset.perk_indices = vec![10, 11];
    assert_eq!(
        asset
            .discovery_name_with(|_| Some("Chosen of the Warmind".into()), |_| None)
            .as_deref(),
        Some("Chosen of the Warmind Emitter")
    );
    assert!(
        asset
            .discovery_name_with(|index| Some(format!("Unrelated{index}")), |_| None)
            .is_none()
    );
}

#[test]
fn default_discovery_hides_catch_all_and_unmapped_entities_but_keeps_named_enemy_effects() {
    let mut asset = entry(7, Kind::Projectile, 18);
    assert!(!asset.has_discovery_identity());
    asset.source_hint = Some("Spawned Entity".into());
    assert!(
        !asset.has_discovery_identity(),
        "a generic operation does not name an asset"
    );
    asset.source_hint = None;
    asset
        .native_paths
        .push("content/common/native/sandbox/label_globals.label_globals.tft".into());
    assert!(!asset.has_discovery_identity());
    assert_ne!(
        asset.label_rank(),
        0,
        "global metadata must not stop the ancestor walk"
    );
    asset
        .native_paths
        .push("content/sandbox/characters/taken/instant_detonate.pattern.tft".into());
    assert!(asset.has_discovery_identity());
    asset.kind = Kind::Entity;
    assert!(!asset.has_discovery_identity());
    asset.source_hint = Some("Attached Entity While Drawn".into());
    assert!(asset.has_discovery_identity());
    asset.source_hint = Some("Shared Across Different Operations".into());
    assert!(!asset.has_discovery_identity());
}

#[test]
fn item_named_entities_need_decoded_usage_to_appear_in_default_discovery() {
    let mut asset = entry(7, Kind::Entity, 0);
    asset.contexts = vec![context(1, "", Some(10), 1)];
    let item_name = |_| Some(ItemName::new("Sunshot", "Hand Cannon"));
    assert!(!asset.has_discovery_identity_with(|_| None, item_name));
    asset.source_hint = Some("Attached Entity While Drawn".into());
    assert!(asset.has_discovery_identity_with(|_| None, item_name));
    assert!(!asset.has_discovery_identity());
    asset.source_hint = Some("Shared Across Different Operations".into());
    assert!(!asset.has_discovery_identity_with(|_| None, item_name));
}

fn entry(graph: u32, kind: Kind, object_type: u8) -> Entry {
    Entry {
        graph,
        kind,
        object_type,
        owners: Vec::new(),
        package: "test".into(),
        native_name: None,
        native_paths: Vec::new(),
        contexts: Vec::new(),
        perk_indices: Vec::new(),
        source_hint: None,
    }
}

fn context(graph: u32, path: &str, item: Option<u32>, depth: usize) -> Context {
    Context {
        graph,
        owner: graph,
        offset: 0,
        path: path.into(),
        name_evidence: None,
        item,
        perk: None,
        depth,
    }
}

#[test]
fn labels_distinguish_native_identity_from_parent_and_perk_context() {
    let mut entry = entry(7, Kind::Projectile, 18);
    assert_eq!(entry.label(), "Projectile 0x00000007");
    assert_eq!(entry.label_rank(), 3);
    let entity = Entry {
        kind: Kind::Entity,
        object_type: 28,
        ..entry.clone()
    };
    assert_eq!(entity.label(), "Entity · system 0x00000007");
    assert_eq!(
        Entry {
            object_type: 23,
            ..entity.clone()
        }
        .kind_label(),
        "Entity · hop_on"
    );
    assert!(!Kind::Entity.spawnable() && Kind::Emitter.spawnable());
    entry.perk_indices = vec![1178];
    assert_eq!(
        entry.label_with_perks(|_| Some("Micro-Missile".into())),
        "Projectile · Used by Micro-Missile"
    );
    assert_eq!(entry.label(), "Projectile · Used by Effect 1178");
    entry
        .contexts
        .push(context(10, "content/solar_strike.pattern.tft", None, 2));
    assert_eq!(entry.label(), "Projectile · From solar_strike.pattern.tft");
    assert_eq!(entry.label_rank(), 1);
    // A direct reference outranks a deeper ancestor and is worded as one.
    entry.contexts.push(context(
        12,
        "content/solar_strike_muzzle.pattern.tft",
        None,
        1,
    ));
    assert_eq!(
        entry.label(),
        "Projectile · Referenced by solar_strike_muzzle.pattern.tft"
    );
    // Repeated native references do not imply multiple distinct identities.
    entry.contexts.push(entry.contexts[1].clone());
    assert!(!entry.label().contains("(+"));
    entry.native_name = Some("Engine Asset".into());
    assert_eq!(entry.label(), "Engine Asset");
    entry.native_paths = vec!["content/solar_strike_projectile.pattern.tft".into()];
    assert_eq!(entry.label(), "solar_strike_projectile.pattern.tft");
    assert_eq!(entry.label_rank(), 0);
}

#[test]
fn shared_label_registry_does_not_hide_real_asset_uses() {
    let mut asset = entry(0x80BC_58B1, Kind::Entity, 28);
    asset.perk_indices = vec![405, 406];
    asset.contexts.push(context(
        2,
        "content/common/native/sandbox/label_globals.label_globals.tft",
        None,
        1,
    ));
    let names = |index| {
        Some(
            if index == 405 {
                "Lightweight Frame"
            } else {
                "Second Frame"
            }
            .to_owned(),
        )
    };
    assert_eq!(asset.label_with_perks(names), "Entity · Shared Perk Asset");
    assert_eq!(asset.source_group(names, |_| None), "Shared Perk Asset");
    assert_eq!(asset.label_rank(), 2);
    asset.contexts.push(context(3, "", Some(10), 2));
    assert_eq!(
        asset.label_with(names, |_| Some(ItemName::new("Example Weapon", "Sidearm"))),
        "Entity · From Example Weapon"
    );
    assert!(
        asset.contexts[0]
            .path
            .ends_with("label_globals.label_globals.tft")
    );
}

#[test]
fn reference_walk_passes_shared_metadata_and_keeps_all_perk_owners() {
    let parents = HashMap::from([(1, vec![2]), (2, vec![3])]);
    let found = climb(1, &parents, &HashMap::new(), |tag| match tag {
        2 => Some(Named::Path(
            "content\\common\\native\\sandbox\\label_globals.label_globals.tft".into(),
        )),
        3 => Some(Named::Perks(vec![405, 406])),
        _ => None,
    });
    assert_eq!(
        found
            .iter()
            .filter_map(|context| context.perk)
            .collect::<Vec<_>>(),
        vec![405, 406]
    );
    assert!(
        found
            .iter()
            .all(|context| context.depth == 2 && context.path.is_empty())
    );
}

#[test]
fn a_weapon_pattern_ancestor_names_the_projectile_after_the_weapon() {
    let mut projectile = entry(7, Kind::Projectile, 18);
    projectile
        .contexts
        .push(context(30, "", Some(0x2B50_ED7D), 2));
    assert_eq!(projectile.label_rank(), 1);
    assert_eq!(
        projectile.label_with(
            |_| None,
            |item| (item == 0x2B50_ED7D).then(|| ItemName::new("The Mountaintop", ""))
        ),
        "Projectile · From The Mountaintop"
    );
    // Without a catalog the item is still identified rather than dropped.
    assert_eq!(projectile.label(), "Projectile · From weapon 0x2B50ED7D");
    // An ability projectile reaches the ability's own perk through its unnamed graph.
    let mut grenade = entry(9, Kind::Projectile, 18);
    grenade.contexts.push(Context {
        perk: Some(612),
        ..context(40, "", None, 2)
    });
    assert_eq!(grenade.label(), "Projectile · From Effect 612");
    assert_eq!(
        grenade.label_with(
            |perk| (perk == 612).then(|| "Fusion Grenade".to_owned()),
            |_| None
        ),
        "Projectile · From Fusion Grenade"
    );
    // A context with neither a path nor an item names nothing.
    let mut blank = entry(8, Kind::Emitter, 17);
    blank.contexts.push(context(9, "", None, 1));
    assert_eq!(blank.label_rank(), 3);
}

#[test]
fn source_groups_follow_the_same_evidence_as_labels() {
    let mut asset = entry(7, Kind::Projectile, 18);
    assert_eq!(asset.source_group(|_| None, |_| None), "Unnamed · test");
    asset.perk_indices = vec![1178];
    assert_eq!(
        asset.source_group(|_| Some("Micro-Missile".into()), |_| None),
        "Used by Micro-Missile"
    );
    asset.contexts.push(context(30, "", Some(0x2B50_ED7D), 2));
    assert_eq!(
        asset.source_group(
            |_| None,
            |_| Some(ItemName::new("The Mountaintop", "Grenade Launcher"))
        ),
        "From The Mountaintop"
    );
    asset
        .contexts
        .push(context(12, "content/sandbox/muzzle.pattern.tft", None, 1));
    assert_eq!(
        asset.source_group(|_| None, |_| None),
        "Referenced by muzzle.pattern.tft"
    );
    asset.native_paths = vec!["content\\sandbox\\weapons\\player\\demo.pattern.tft".into()];
    assert_eq!(
        asset.source_group(|_| None, |_| None),
        "sandbox / weapons / player"
    );
}

#[test]
fn weapons_sharing_a_pattern_entity_group_under_their_weapon_type() {
    let weapons = [
        (0x1, "Witherhoard", "Grenade Launcher"),
        (0x2, "The Mountaintop", "Grenade Launcher"),
        (0x3, "Fighting Lion", "Grenade Launcher"),
        (0x4, "Martyr's Retribution", "Grenade Launcher"),
        (0x5, "Sunshot", "Hand Cannon"),
    ];
    let item_name = |item: u32| {
        weapons
            .iter()
            .find(|(hash, ..)| *hash == item)
            .map(|(_, name, kind)| ItemName::new(*name, *kind))
    };
    let mut shell = entry(7, Kind::Projectile, 18);
    for item in [0x1, 0x2, 0x3, 0x4] {
        shell.contexts.push(context(30, "", Some(item), 2));
    }
    // The label spells out the first few weapons alphabetically and counts the rest.
    assert_eq!(
        shell.label_with(|_| None, item_name),
        "Projectile · Shared by Grenade Launchers"
    );
    assert_eq!(
        shell.source_group(|_| None, item_name),
        "Shared by Grenade Launchers"
    );
    // A weapon the caller cannot name still counts, and sorts after the named ones.
    shell.contexts.push(context(30, "", Some(0x9), 2));
    assert_eq!(
        shell.label_with(|_| None, item_name),
        "Projectile · Shared Asset, Role Unmapped"
    );
    assert_eq!(
        shell.source_group(|_| None, item_name),
        "Shared Asset, Role Unmapped"
    );
    // Weapons of different types share no heading, so the first name leads instead.
    shell.contexts.push(context(30, "", Some(0x5), 2));
    assert_eq!(
        shell.source_group(|_| None, item_name),
        "Shared Asset, Role Unmapped"
    );
    // Without any weapon types the heading also falls back to the first name.
    assert_eq!(
        shell.source_group(
            |_| None,
            |item| { item_name(item).map(|item| ItemName::new(item.name, "")) }
        ),
        "Shared Asset, Role Unmapped"
    );
    // Perk lists follow the same wording.
    let mut spark = entry(8, Kind::Emitter, 17);
    spark.perk_indices = vec![1, 2, 3, 4];
    assert_eq!(
        spark.label_with(|index| Some(format!("Perk {index}")), |_| None),
        "Emitter · Shared Perk Asset"
    );
    assert_eq!(
        spark.source_group(|index| Some(format!("Perk {index}")), |_| None),
        "Shared Perk Asset"
    );
}

#[test]
fn the_reference_walk_stops_at_the_nearest_named_level() {
    // 1 is bound by resource 20, which graph 30 owns. Graph 30 is referenced by 40, a
    // named resource, and by 41, an unnamed one whose own parent 50 is also named.
    // Graph 3 is bound by resource 21, which a pattern entity 31 owns. Two weapon items
    // share that entity, as a reissued weapon shares the original's pattern.
    let parents = HashMap::from([(30, vec![40, 41]), (41, vec![50]), (2, vec![60])]);
    let resource_graph =
        HashMap::from([(20, vec![30]), (1, vec![20]), (3, vec![21]), (21, vec![31])]);
    let names = HashMap::from([
        (40, Named::Path("content/a.pattern.tft".to_owned())),
        (50, Named::Path("content/b.pattern.tft".to_owned())),
        (60, Named::Path("cine_intro".to_owned())),
        (31, Named::Items(vec![0xEE06_B019, 0x2B50_ED7D])),
    ]);
    let name_of = |tag: u32| names.get(&tag).cloned();
    let found = climb(1, &parents, &resource_graph, name_of);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].graph, 40);
    assert_eq!(found[0].owner, 30);
    assert_eq!(found[0].depth, 3);
    assert_eq!(found[0].path, "content/a.pattern.tft");
    let direct = climb(2, &parents, &resource_graph, name_of);
    assert_eq!(direct[0].depth, 1);
    assert_eq!(direct[0].path, "cine_intro");
    let fired = climb(3, &parents, &resource_graph, name_of);
    assert_eq!(
        fired
            .iter()
            .map(|context| (context.graph, context.item, context.depth))
            .collect::<Vec<_>>(),
        vec![(31, Some(0x2B50_ED7D), 2), (31, Some(0xEE06_B019), 2)]
    );
    assert!(fired.iter().all(|context| context.path.is_empty()));
    assert!(climb(99, &parents, &resource_graph, name_of).is_empty());
    let catalog = Catalog {
        entries: vec![
            Entry {
                package: "sandbox".into(),
                perk_indices: vec![3],
                ..entry(1, Kind::Emitter, 17)
            },
            Entry {
                package: "cinematics".into(),
                ..entry(2, Kind::Emitter, 17)
            },
        ],
        errors: Vec::new(),
        ..Catalog::default()
    };
    assert_eq!(
        catalog.perk_packages(),
        BTreeSet::from(["sandbox".to_owned()])
    );
}

#[test]
fn native_discovery_uses_common_ancestry_without_promoting_gameplay_reports() {
    let familiar = entry(0x80BAA9B8, Kind::Projectile, 18);
    assert!(knowledge::get(familiar.graph).is_some());
    assert_eq!(
        familiar.discovery_name().as_deref(),
        Some("Hammer of Sol A")
    );
    assert!(familiar.discovery_summary().is_empty());
    let mut asset = entry(1, Kind::Projectile, 18);
    assert!(asset.discovery_name().is_none());
    assert!(
        !asset.has_discovery_identity(),
        "an asset without source evidence or an assigned display name stays unidentified"
    );
    asset.contexts = vec![
        context(
            1,
            "content/characters/taken/taken_wizard.pattern.tft",
            None,
            2,
        ),
        context(
            2,
            "content/characters/taken/taken_wizard_v400.pattern.tft",
            None,
            2,
        ),
    ];
    asset.source_hint = Some("Shared Asset, Role Unmapped".into());
    assert_eq!(
        asset.discovery_name().as_deref(),
        Some("Taken Wizard Projectile")
    );
    assert!(asset.has_discovery_identity());
    assert!(!asset.discovery_summary().contains("Solar"));
    asset.contexts.push(context(
        3,
        "content/characters/taken/taken_captain.pattern.tft",
        None,
        2,
    ));
    // Unrelated families never merge into one invented family. The label lists them.
    assert_eq!(
        asset.discovery_name().as_deref(),
        Some("Shared by Taken Captain, Taken Wizard Projectile")
    );
}

#[test]
fn discovery_variants_are_stable_across_catalog_order_and_keep_shared_weapon_types() {
    let mut first = entry(8, Kind::Projectile, 18);
    first.contexts = vec![context(
        1,
        "content/characters/hive/shrieker.pattern.tft",
        None,
        2,
    )];
    let mut second = first.clone();
    second.graph = 7;
    let mut catalog = Catalog {
        entries: vec![first, second],
        errors: vec![],
        ..Catalog::default()
    };
    let labels = catalog.discovery_labels_with(|_| None, |_| None);
    assert_eq!(labels[&7], "Hive Shrieker Projectile · Variant 1");
    assert_eq!(labels[&8], "Hive Shrieker Projectile · Variant 2");
    catalog.entries.reverse();
    assert_eq!(labels, catalog.discovery_labels_with(|_| None, |_| None));
    let mut shared = entry(9, Kind::Projectile, 18);
    shared.contexts = vec![context(1, "", Some(10), 2), context(2, "", Some(20), 2)];
    assert_eq!(
        shared.discovery_name_with(
            |_| None,
            |hash| Some(ItemName::new(format!("Weapon {hash}"), "Hand Cannon"))
        ),
        Some("Shared Hand Cannon Projectile".into())
    );
    shared.contexts.push(context(3, "", Some(30), 2));
    assert_eq!(
        shared.discovery_name_with(
            |_| None,
            |hash| Some(ItemName::new(
                format!("Weapon {hash}"),
                if hash == 30 { "" } else { "Hand Cannon" }
            ))
        ),
        Some("Shared Hand Cannon Projectile".into())
    );
    // Weapons of different types share no family, so the weapons themselves are listed.
    assert_eq!(
        shared
            .discovery_name_with(
                |_| None,
                |hash| Some(ItemName::new(
                    format!("Weapon {hash}"),
                    if hash == 30 {
                        "Machine Gun"
                    } else {
                        "Hand Cannon"
                    }
                ))
            )
            .as_deref(),
        Some("Shared by Weapon 10, Weapon 20, Weapon 30 Projectile")
    );
    shared.contexts.pop();
    assert_eq!(
        shared.discovery_name_with(
            |_| None,
            |hash| Some(ItemName::new(
                if hash == 10 { "Ace" } else { "Rose" },
                "Hand Cannon"
            ))
        ),
        Some("Shared Hand Cannon Projectile".into())
    );
}

#[test]
#[ignore = "requires PARHELION_PROJECTILE_CATALOG_INSTALL"]
fn installed_discovery_identifies_enemy_and_guardian_projectiles_from_native_sources() {
    let install =
        std::path::PathBuf::from(std::env::var_os("PARHELION_PROJECTILE_CATALOG_INSTALL").unwrap());
    let manager = crate::package_runtime::open_shadowkeep_packages(&install).unwrap();
    let catalog = cached(&install.join("packages"), &manager).unwrap();
    assert!(
        catalog.errors.is_empty(),
        "Catalog read errors: {:?}",
        catalog.errors
    );
    for (graph, family) in [
        (0x80BDE759, "Taken Wizard"),
        (0x80BF9602, "Hive Shrieker"),
        (0x80BAA9B8, "Hammer"),
    ] {
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.graph == graph)
            .expect("installed graph");
        let name = entry
            .discovery_name()
            .unwrap_or_else(|| panic!("0x{graph:08X} remains unidentified: {:?}", entry.contexts));
        assert!(name.contains(family), "0x{graph:08X}: {name}");
        assert!(entry.has_discovery_identity());
        println!("0x{graph:08X}: {name}");
    }
    let visible = catalog
        .entries
        .iter()
        .filter(|entry| entry.has_discovery_identity())
        .count();
    println!(
        "Default discovery: {visible} of {} assets",
        catalog.entries.len()
    );
    assert!(
        catalog
            .entries
            .iter()
            .filter(|entry| entry.has_discovery_identity())
            .all(|entry| !entry
                .discovery_label_with(|_| None, |_| None)
                .to_lowercase()
                .contains("debug"))
    );
}
