//! Part donors and moved markers, built and read back through a real package manager.
//!
//! One hand cannon is built on Austringer wearing Luna's Howl. It plays Ancient Gospel's
//! first-person animations, carries Dire Promise's type markers and has its trigger and muzzle markers
//! moved. Every claim is read from the staged packages: the attachment row and content block the
//! authored weapon's row selects, and the marker sets its appearance resolves to. When
//! `PARHELION_PARTS_REPORT` names a file, the read-back is written there as JSON.
use super::*;
use sundial::package_authoring::gear_markers::{MarkerSet, read_appearance};
use sundial::package_authoring::runtime::{
    load_weapon_runtime_entity_at_pattern_index_with_manager,
    load_weapon_runtime_entity_with_manager,
};

const AUSTRINGER: u32 = 0x90D4_2801;
const LUNAS_HOWL: u32 = 0x092D_8A04;
const ANCIENT_GOSPEL: u32 = 0x02E6_3C72;
const DIRE_PROMISE: u32 = 0x22B5_BC70;

/// Every position each marker name holds across an appearance's sets, sorted so two reads compare
/// as multisets.
fn positions(sets: &[MarkerSet]) -> BTreeMap<u32, Vec<[u32; 3]>> {
    let mut named = BTreeMap::<u32, Vec<[u32; 3]>>::new();
    for marker in sets.iter().flat_map(|set| &set.markers) {
        named
            .entry(marker.name)
            .or_default()
            .push(marker.position.map(f32::to_bits));
    }
    for rows in named.values_mut() {
        rows.sort_unstable();
    }
    named
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "One staged build is read back part by part in sequence"
)]
fn real_part_donors_and_moved_markers_reach_the_staged_packages() {
    use crate::weapon::animations::profile;
    use crate::weapon::behavior::{block_for_group, content, first_triple};
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let trigger = sundial::package_authoring::fnv1_name_hash("primary_trigger");
    let primary_fire = sundial::package_authoring::fnv1_name_hash("primary_fire");
    let raised = [0.0, 0.0, 0.005];
    let muzzle = [0.01, 0.0, 0.0];
    // The whole model 2 cm toward the muzzle and 1 cm down in the hand.
    let held = [0.02, 0.0, -0.01];
    let namespace = "parhelion.part-donors.integration";
    let spec = WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.to_owned(),
        donor_item_hash: AUSTRINGER,
        expected_donor_name: Some("Austringer".to_owned()),
        presentation_donor: Some(WeaponPresentationDonorReference {
            item_hash: LUNAS_HOWL,
            expected_name: Some("Luna's Howl".to_owned()),
        }),
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: "True Howl".to_owned(),
            flavor: "Part donor integration test.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            animation_donor: Some(ANCIENT_GOSPEL),
            type_marker_donor: Some(DIRE_PROMISE),
            marker_offsets: vec![(trigger, raised), (primary_fire, muzzle)],
            held_offset: Some(held),
            ..WeaponCloneOverrides::default()
        },
    };
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("a weapon with part donors and moved markers should build");
    let view = staged_view(&packages, ".parhelion-part-donors-test-", &bundle);
    let view_packages = view.path().join("packages");
    let plan = &bundle.plan.weapons[0];
    let manager = open_manager(&view_packages).unwrap();
    let stock = open_manager(&packages).unwrap();
    let authored = load_weapon_runtime_entity_with_manager(&manager, plan.item_hash).unwrap();
    // Several of these items share another item's pattern row, so each is read through its own.
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let summaries = catalog.weapon_donors();
    let stock_entity = |hash: u32| {
        let pattern = summaries
            .iter()
            .find(|donor| donor.hash == hash)
            .and_then(|donor| donor.weapon_pattern_index)
            .unwrap();
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern).unwrap()
    };
    let luna = stock_entity(LUNAS_HOWL);
    let austringer = stock_entity(AUSTRINGER);
    let gospel = stock_entity(ANCIENT_GOSPEL);
    let promise = stock_entity(DIRE_PROMISE);
    assert_eq!(
        authored.weapon_content_group_hash, luna.weapon_content_group_hash,
        "the authored row should select Luna's Howl's block and attachment row"
    );

    // Animations: the selected attachment row now carries Ancient Gospel's keys.
    let built = profile(
        &manager,
        &authored.payload,
        Some(authored.weapon_content_group_hash),
    )
    .unwrap();
    let lent = profile(
        &stock,
        &gospel.payload,
        Some(gospel.weapon_content_group_hash),
    )
    .unwrap();
    let before = profile(&stock, &luna.payload, Some(luna.weapon_content_group_hash)).unwrap();
    assert_ne!(
        before.keys, lent.keys,
        "Luna's Howl and Ancient Gospel should play different animations in stock"
    );
    assert_eq!(
        built.keys, lent.keys,
        "the weapon should play Ancient Gospel's animations"
    );
    assert!(
        stock.get_entry(TagHash(built.owner)).is_none(),
        "the edited attachment owner should be a private copy"
    );

    // Type markers: Dire Promise's, over both Luna's Howl's and Austringer's own.
    let built_content = content(&manager, &authored.payload).unwrap();
    let block = block_for_group(&built_content, authored.weapon_content_group_hash).unwrap();
    let markers_of = |entity: &[u8], group: u32| {
        let owner = content(&stock, entity).unwrap();
        let block = block_for_group(&owner, group).unwrap();
        [0x18, 0x30].map(|offset| read_u32(&owner.owner, block + offset).unwrap())
    };
    let lent_markers = markers_of(&promise.payload, promise.weapon_content_group_hash);
    let built_markers =
        [0x18, 0x30].map(|offset| read_u32(&built_content.owner, block + offset).unwrap());
    assert_ne!(
        markers_of(&luna.payload, luna.weapon_content_group_hash),
        lent_markers
    );
    assert_ne!(
        markers_of(&austringer.payload, austringer.weapon_content_group_hash),
        lent_markers
    );
    assert_eq!(
        built_markers, lent_markers,
        "the selected block should carry Dire Promise's type markers"
    );
    let label = |owner: &[u8], block: usize| {
        let triple = first_triple(owner, block).unwrap();
        let relative = i64::from_le_bytes(owner[triple..triple + 8].try_into().unwrap());
        let target = triple
            .checked_add_signed(isize::try_from(relative).unwrap())
            .unwrap();
        read_u32(owner, target + 16).unwrap()
    };
    let promise_content = content(&stock, &promise.payload).unwrap();
    let promise_block =
        block_for_group(&promise_content, promise.weapon_content_group_hash).unwrap();
    assert_eq!(
        label(&built_content.owner, block),
        label(&promise_content.owner, promise_block),
        "the type label should be Dire Promise's"
    );

    // Markers: every marker moved with the model in the hand, the trigger and muzzle markers by
    // their own offsets as well.
    let stock_arrangement =
        catalog.weapon_donor(LUNAS_HOWL).unwrap().art_arrangements[0].arrangement;
    let definition = read_tag(&manager, plan.definition_tag, "authored definition").unwrap();
    let arrangement = weapon_art_arrangements(&definition).unwrap()[0].arrangement;
    assert_ne!(
        arrangement, stock_arrangement,
        "moved markers need a private gear-art row"
    );
    let was = positions(&read_appearance(&packages, stock_arrangement).unwrap());
    let now = positions(&read_appearance(&view_packages, arrangement).unwrap());
    assert_eq!(
        was.keys().collect::<Vec<_>>(),
        now.keys().collect::<Vec<_>>(),
        "the appearance should carry the same marker names"
    );
    let moved = |rows: &[[u32; 3]], offset: [f32; 3]| {
        let mut rows = rows
            .iter()
            .map(|row| {
                std::array::from_fn(|axis| (f32::from_bits(row[axis]) + offset[axis]).to_bits())
            })
            .collect::<Vec<[u32; 3]>>();
        rows.sort_unstable();
        rows
    };
    for (name, rows) in &was {
        let expected = if *name == trigger {
            moved(&moved(rows, raised), held)
        } else if *name == primary_fire {
            moved(&moved(rows, muzzle), held)
        } else {
            moved(rows, held)
        };
        assert_eq!(now[name], expected, "marker 0x{name:08X}");
    }
    assert!(was.contains_key(&trigger) && was.contains_key(&primary_fire));

    // The model: every part's position offset moved by the same amount as its markers, so the
    // sights stay on it while it moves in the hand.
    let offsets = |manager: &PackageManager, arrangement: u16, owner: Option<u32>| {
        let mut offsets = super::donors::arrangement_models(manager, arrangement, owner)
            .into_iter()
            .map(|(_, model)| {
                std::array::from_fn(|axis| read_u32(&model, 0x60 + axis * 4).unwrap())
            })
            .collect::<Vec<[u32; 3]>>();
        offsets.sort_unstable();
        offsets
    };
    let stock_models = offsets(&stock, stock_arrangement, None);
    assert!(!stock_models.is_empty());
    assert_eq!(
        offsets(&manager, arrangement, Some(plan.item_hash)),
        moved(&stock_models, held),
        "every part model should move by the held offset"
    );

    if let Some(path) = std::env::var_os("PARHELION_PARTS_REPORT") {
        let hex = |value: u32| format!("0x{value:08X}");
        let report = serde_json::json!({
            "item": hex(plan.item_hash),
            "content_group": hex(authored.weapon_content_group_hash),
            "animations": {
                "stock": before.keys.map(hex),
                "lent": lent.keys.map(hex),
                "built": built.keys.map(hex),
                "private_owner": hex(built.owner),
            },
            "type_markers": {
                "lent": lent_markers.map(hex),
                "built": built_markers.map(hex),
            },
            "markers": was.iter().map(|(name, rows)| {
                let read = |rows: &Vec<[u32; 3]>| rows.iter().map(|row| row.map(f32::from_bits)).collect::<Vec<_>>();
                (hex(*name), serde_json::json!({ "stock": read(rows), "built": read(&now[name]) }))
            }).collect::<serde_json::Map<_, _>>(),
        });
        fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

/// Fate of All Fools: a scout rifle wearing a pulse rifle, whose moved rig fired the pulse
/// rifle's burst. Animations from the base weapon's own family keep the scout rig, so the build
/// pins the pulse rifle's parts to it and the weapon resolves the scout's first-person rig.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_base_family_animations_keep_the_base_rig() {
    const JADE_RABBIT: u32 = 0xE529_6126;
    const MACHINA_DEI_4: u32 = 0x09A0_DE64;
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let namespace = "parhelion.kept-rig.integration";
    let spec = WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.to_owned(),
        donor_item_hash: JADE_RABBIT,
        expected_donor_name: Some("The Jade Rabbit".to_owned()),
        presentation_donor: Some(WeaponPresentationDonorReference {
            item_hash: MACHINA_DEI_4,
            expected_name: Some("Machina Dei 4".to_owned()),
        }),
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: "Fate of All Fools".to_owned(),
            flavor: "Kept rig integration test.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            animation_donor: Some(JADE_RABBIT),
            ..WeaponCloneOverrides::default()
        },
    };
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("a cross-family appearance with base animations should build");
    let view = staged_view(&packages, ".parhelion-kept-rig-test-", &bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let pattern = |hash: u32| {
        catalog
            .weapon_donors()
            .iter()
            .find(|donor| donor.hash == hash)
            .and_then(|donor| donor.weapon_pattern_index)
            .unwrap()
    };
    let authored =
        load_weapon_runtime_entity_with_manager(&manager, bundle.plan.weapons[0].item_hash)
            .unwrap();
    let jade =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern(JADE_RABBIT))
            .unwrap();
    let machina =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern(MACHINA_DEI_4))
            .unwrap();
    assert_eq!(
        authored.weapon_content_group_hash, jade.weapon_content_group_hash,
        "the row should keep the scout rifle's content group"
    );
    // The attachment is a private copy, so it is compared by its rows and hold below.
    let rig = |entity: &[u8]| {
        [0x1C80_DD4A_u32, 0x681C_2C0D, 0x8983_4B2B].map(|binding| {
            sundial::package_authoring::entity::weapon_component_bindings(entity, binding).unwrap()
                [0]
            .owner_tag
        })
    };
    assert_ne!(rig(&jade.payload), rig(&machina.payload));
    assert_eq!(
        rig(&authored.payload),
        rig(&jade.payload),
        "the weapon should resolve the scout rifle's own rig"
    );
    use crate::weapon::animations::{hold, profile};
    let group = Some(jade.weapon_content_group_hash);
    assert_eq!(
        profile(&manager, &authored.payload, group).unwrap().keys,
        profile(&stock, &jade.payload, group).unwrap().keys,
        "the row should play the scout rifle's animations"
    );
    let machina_hold = hold(&stock, &machina.payload).unwrap();
    assert_ne!(hold(&stock, &jade.payload).unwrap(), machina_hold);
    let built = hold(&manager, &authored.payload).unwrap();
    assert_eq!(
        built,
        machina_hold,
        "the scout rig should hold the pulse rifle model where the pulse rig does: built {:?}, pulse {:?}",
        built.translation(),
        machina_hold.translation()
    );
    // Every part rides the scout rig's root bone, the path a pinned appearance takes.
    super::donors::assert_parts_pinned(&bundle, &view, &packages);
    let details = bundle.plan.weapons[0].details.as_ref().unwrap();
    assert_eq!(details.runtime_source, Some(pattern(JADE_RABBIT)));
    assert_eq!(details.rig_donor, None);
    assert_eq!(details.pinned_appearance, Some(pattern(MACHINA_DEI_4)));
    assert_eq!(details.animation_donor, Some(pattern(JADE_RABBIT)));
    let artifacts = bundle
        .artifacts
        .iter()
        .map(|artifact| {
            let name = &artifact.plan.output_file_name;
            let digest =
                crate::artifact::digest_file(&view.path().join("packages").join(name)).unwrap();
            crate::ArtifactMetadata {
                file_name: name.clone(),
                byte_length: digest.byte_length,
                sha256: digest.sha256,
            }
        })
        .collect::<Vec<_>>();
    crate::test_support::artifact(
        "kept-rig-build.json",
        &serde_json::json!({
            "runtime_source": details.runtime_source,
            "pinned_appearance": details.pinned_appearance,
            "animation_donor": details.animation_donor,
            "entity": authored.entity_tag,
            "rig_owners": rig(&authored.payload),
            "packages": artifacts,
            "client_build": sundial::package_authoring::sandbox_perk::nodes::CLIENT_BUILD,
            "gameplay_verified": false,
        }),
    );
}

/// One component's values in its owner: the resource record and the concrete object its prefix
/// names, each up to the next object, with every owner reference blanked and every array
/// descriptor reduced to its count. What is left is what a component holds, not how it is wired.
fn component_values(manager: &PackageManager, entity: &[u8], binding: u32) -> Vec<u8> {
    use sundial::package_authoring::entity::{
        weapon_component_binding_hashes, weapon_component_bindings,
    };
    let resource = weapon_component_bindings(entity, binding).unwrap()[0];
    let tag = resource.owner_tag;
    let owner = manager.read_tag(TagHash(tag)).unwrap();
    let u32_at = |at: usize| read_u32(&owner, at).unwrap();
    let u64_at = |at: usize| crate::tag_payload::read_u64(&owner, at).unwrap();
    let mut resources = BTreeSet::new();
    for hash in weapon_component_binding_hashes(entity).unwrap() {
        for bound in weapon_component_bindings(entity, hash).unwrap() {
            if bound.owner_tag == tag {
                resources.insert(bound.resource_offset as usize);
            }
        }
    }
    let reference = |at: usize| {
        (u32_at(at) == tag && u32_at(at + 4) & 0xFFFF_0000 == 0x8080_0000)
            .then(|| u64_at(at + 8) as usize)
    };
    let array = |at: usize| {
        let count = u64_at(at);
        let relative = i64::from_le_bytes(owner[at + 8..at + 16].try_into().unwrap());
        let header = (at as i64 + 8 + relative) as usize;
        (relative > 0
            && count < 100_000
            && header + 16 <= owner.len()
            && u64_at(header) == count
            && u32_at(header + 8) & 0xFFFF_0000 == 0x8080_0000)
            .then_some((header, count))
    };
    let mut starts = resources.clone();
    let mut at = 0;
    while at + 16 <= owner.len() {
        if let Some(target) = reference(at) {
            starts.insert(target);
        } else if let Some((header, _)) = array(at) {
            starts.insert(header);
        }
        at += 8;
    }
    let start = resource.resource_offset as usize;
    let concrete = u64_at(start + 8) as usize;
    let mut values = Vec::new();
    for object in [start, concrete] {
        let end = if object == start {
            resources.range(start + 1..).next().copied().unwrap()
        } else {
            starts
                .range(object + 1..)
                .next()
                .copied()
                .unwrap_or(owner.len())
        };
        let mut at = object;
        while at < end {
            if at + 16 <= end {
                if reference(at).is_some() {
                    values.extend_from_slice(&[0; 16]);
                    at += 16;
                    continue;
                }
                if let Some((_, count)) = array(at) {
                    values.extend_from_slice(&count.to_le_bytes());
                    at += 16;
                    continue;
                }
            }
            values.push(owner[at]);
            at += 1;
        }
    }
    values
}

/// Every event row, with each endpoint's owner tag left out: what connects to which object.
fn event_wiring(entity: &[u8]) -> Vec<[u8; 0x48]> {
    let count = crate::tag_payload::read_u64(entity, 0x20).unwrap() as usize;
    let header = crate::tag_payload::relative_target(entity, 0x28).unwrap();
    (0..count)
        .map(|index| {
            let mut row: [u8; 0x48] = entity[header + 16 + index * 0x48..][..0x48]
                .try_into()
                .unwrap();
            row[8..12].fill(0);
            row[0x28..0x2C].fill(0);
            row
        })
        .collect()
}

/// Austringer with The Jade Rabbit's barrel and Sweet Business's magazine. Each component takes
/// its donor's values while the hand cannon keeps its own trigger, its own objects and its own
/// event wiring, so the three come from three weapons at once.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_component_splices_take_one_component_each() {
    use sundial::package_authoring::entity::{
        WEAPON_BARREL_COMPONENT_KEY, WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_TRIGGER_COMPONENT_KEY,
        weapon_component_bindings,
    };
    const JADE_RABBIT: u32 = 0xE529_6126;
    const SWEET_BUSINESS: u32 = 0x5038_4F32;
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let namespace = "parhelion.component-splices.integration";
    let spec = WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.to_owned(),
        donor_item_hash: AUSTRINGER,
        expected_donor_name: Some("Austringer".to_owned()),
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: "Mixed Parts".to_owned(),
            flavor: "Component splice integration test.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            component_splices: vec![
                (WEAPON_BARREL_COMPONENT_KEY, JADE_RABBIT),
                (WEAPON_MAGAZINE_COMPONENT_KEY, SWEET_BUSINESS),
            ],
            ..WeaponCloneOverrides::default()
        },
    };
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("a weapon with components from two other weapons should build");
    let view = staged_view(&packages, ".parhelion-component-splices-test-", &bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let authored =
        load_weapon_runtime_entity_with_manager(&manager, bundle.plan.weapons[0].item_hash)
            .unwrap();
    let base = load_weapon_runtime_entity_with_manager(&stock, AUSTRINGER).unwrap();
    let jade = load_weapon_runtime_entity_with_manager(&stock, JADE_RABBIT).unwrap();
    let sweet = load_weapon_runtime_entity_with_manager(&stock, SWEET_BUSINESS).unwrap();
    let owner = weapon_component_bindings(&authored.payload, WEAPON_BARREL_COMPONENT_KEY).unwrap()
        [0]
    .owner_tag;
    assert!(
        stock.get_entry(TagHash(owner)).is_none(),
        "the spliced gameplay owner should be a private copy"
    );
    for (binding, donor, name) in [
        (WEAPON_BARREL_COMPONENT_KEY, &jade, "barrel"),
        (WEAPON_MAGAZINE_COMPONENT_KEY, &sweet, "magazine"),
    ] {
        let before = component_values(&stock, &base.payload, binding);
        let lent = component_values(&stock, &donor.payload, binding);
        assert_ne!(before, lent, "the stock {name}s should differ");
        assert_eq!(
            component_values(&manager, &authored.payload, binding),
            lent,
            "the {name} should hold its donor's values"
        );
    }
    assert_eq!(
        component_values(&manager, &authored.payload, WEAPON_TRIGGER_COMPONENT_KEY),
        component_values(&stock, &base.payload, WEAPON_TRIGGER_COMPONENT_KEY),
        "the trigger should stay Austringer's"
    );
    assert_eq!(
        event_wiring(&authored.payload),
        event_wiring(&base.payload),
        "every event row should reach the same objects as Austringer's"
    );
}

/// Each weapon type's Rounds Per Minute curve in a stat translator owner, keyed by the type's
/// translation group hash: for each stat point from 0 to 100, the outputs it writes, the first
/// being shots per second.
fn rate_curves_by_type(owner: &[u8]) -> BTreeMap<u32, Vec<Vec<f32>>> {
    use sundial::package_authoring::native_payload::native_array_at;
    let array = |class: u32| {
        (0..owner.len() - 16)
            .step_by(8)
            .find_map(|descriptor| {
                let (count, _, rows, found) = native_array_at(owner, descriptor).ok()?;
                (found == class && count > 0).then_some((count, rows))
            })
            .unwrap()
    };
    let (count, keys) = array(0x8080_38C7);
    let (table_count, tables) = array(0x8080_3975);
    assert_eq!(count, table_count, "every type key should have a table");
    (0..count)
        .map(|index| {
            let hash = read_u32(owner, keys + index * 0x28 + 0x10).unwrap();
            let (rows, _, start, _) = native_array_at(owner, tables + index * 0x30).unwrap();
            // A translation row names its stat lane at +0x20 (0 is Rounds Per Minute) and holds
            // its curve at +0x10, one array of output values per stat point.
            let row = (0..rows)
                .map(|row| start + row * 0x38)
                .find(|row| read_u32(owner, row + 0x20).unwrap() == 0)
                .expect("every type converts Rounds Per Minute");
            let (points, _, first, _) = native_array_at(owner, row + 0x10).unwrap();
            let curve = (0..points)
                .map(|point| {
                    let (values, _, at, _) = native_array_at(owner, first + point * 16).unwrap();
                    (0..values)
                        .map(|value| f32::from_bits(read_u32(owner, at + value * 4).unwrap()))
                        .collect()
                })
                .collect();
            (hash, curve)
        })
        .collect()
}

/// A hand cannon wearing a sidearm's look, with its damage socket given a Trait's role, as a
/// user built it. Both types share one rig and the row names the sidearm's type, yet the
/// stat translator must still convert as a hand cannon, or Rounds Per Minute 40 fires about
/// twice as fast. And with the Arc Damage Mod's socket gone, the weapon must carry Arc on itself
/// rather than turning Kinetic. When `PARHELION_RATE_DAMAGE_REPORT` names a file, the read-back
/// is written there as JSON.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn real_sidearm_look_keeps_hand_cannon_rates_and_arc_damage() {
    use sundial::package_authoring::entity::{
        WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, weapon_component_bindings,
    };
    const NATURE_OF_THE_BEAST: u32 = 0x1BE9_5651;
    const LAST_HOPE: u32 = 0x71F4_6BCF;
    const MULLIGAN: u32 = 0xD170_34D3;
    const HAND_CANNON: u32 = 0xC8CC_993A;
    const SIDEARM: u32 = 0x3EB0_2F1A;
    const DAMAGE_SOCKET: usize = 4;
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let base = catalog.weapon_donor(NATURE_OF_THE_BEAST).unwrap();
    let mut socket_columns = vec![None; base.sockets.len()];
    socket_columns[DAMAGE_SOCKET] = Some(crate::recipe::WeaponSocketColumnRecipe {
        choices: vec![MULLIGAN.into()],
        socket_type: Some(92),
        ..crate::recipe::WeaponSocketColumnRecipe::default()
    });
    let namespace = "parhelion.sidearm-look-rates.integration";
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        namespace,
        NATURE_OF_THE_BEAST,
        "Nature of the Beast",
    )
    .unwrap();
    recipe.name = "Zeus-SI5".into();
    recipe.set_presentation_donor(Some(crate::WeaponDonorReference {
        item_hash: LAST_HOPE.into(),
        expected_name: Some("Last Hope".into()),
    }));
    recipe.overrides.socket_columns = socket_columns;
    let staging = tempfile::tempdir().unwrap();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: staging.path().into(),
        ignore_installed_authored_overlays: true,
        recipes: vec![recipe.clone()],
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {})
        .expect("a sidearm look on a hand cannon with a retyped damage socket should build");
    let plan = &build.weapons[0];
    let view = crate::workflow::FilteredPackageView::create(&packages, &[]).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let manager = open_manager(view.path()).unwrap();
    let stock = open_manager(&packages).unwrap();

    // The translator the authored weapon reads, against the stock one both weapons share.
    let translator = |manager: &PackageManager, entity: &[u8]| {
        let [binding] =
            weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).unwrap()[..]
        else {
            panic!("one stat translator");
        };
        rate_curves_by_type(&read_tag(manager, TagHash(binding.owner_tag), "translator").unwrap())
    };
    let authored = load_weapon_runtime_entity_with_manager(&manager, plan.item_hash).unwrap();
    let pattern = base.summary.weapon_pattern_index.unwrap();
    let stock_entity =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern).unwrap();
    let sidearm_pattern = catalog
        .weapon_donor(LAST_HOPE)
        .unwrap()
        .summary
        .weapon_pattern_index
        .unwrap();
    let sidearm =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, sidearm_pattern).unwrap();
    let rig = |entity: &[u8]| {
        [0x1C80_DD4A_u32, 0x681C_2C0D, 0x8983_4B2B]
            .map(|binding| weapon_component_bindings(entity, binding).unwrap()[0].owner_tag)
    };
    // Hand cannons and sidearms share one rig, so only the row's translation group tells the
    // translator which type's table to read.
    assert_eq!(rig(&stock_entity.payload), rig(&sidearm.payload));
    let built = translator(&manager, &authored.payload);
    let original = translator(&stock, &stock_entity.payload);
    // Stat 40, the fifth point: a hand cannon fires about 2.3 shots a second, a sidearm 5.
    assert!(
        original[&HAND_CANNON][4][0] < 3.0,
        "{:?}",
        original[&HAND_CANNON][4]
    );
    assert!(
        original[&SIDEARM][4][0] > 4.0,
        "{:?}",
        original[&SIDEARM][4]
    );
    assert_eq!(
        built[&SIDEARM], original[&HAND_CANNON],
        "the sidearm type the moved rig names should convert as a hand cannon"
    );
    assert_eq!(
        built[&HAND_CANNON], original[&HAND_CANNON],
        "the hand cannon's own conversion should be untouched"
    );

    // It shows the appearance's type, as the inventory reads it, while firing as a hand cannon.
    assert_eq!(
        sundial::package_authoring::resolve_item_type_name(
            &manager,
            TagHash(plan.item_string_hash)
        )
        .unwrap(),
        "Sidearm"
    );

    // The Arc Damage Mod's socket is a Trait now, so Arc is the weapon's own marker.
    let definition = read_tag(
        &manager,
        TagHash(plan.item_definition_hash),
        "authored definition",
    )
    .unwrap();
    assert!(
        weapon_damage_socket_lanes(&definition).unwrap().is_empty(),
        "the retyped socket should no longer carry damage"
    );
    let carrier = weapon_damage_carrier(&definition).unwrap();
    assert_eq!(
        carrier,
        WeaponDamageCarrier::Fixed {
            family: WeaponDamageCarrierFamily::ModernFixed,
            damage_type: ModernDamageType::Arc,
        },
        "the weapon should keep Arc without its damage socket"
    );

    let details = plan.details.as_ref().unwrap();
    assert_eq!(details.runtime_source, Some(pattern));
    // Last Hope shares its pattern row, so the rig is named by the row's own item.
    assert_eq!(details.rig_donor, Some(sidearm.item_hash));
    let rig_donor = format!("0x{:08X}", sidearm.item_hash);
    assert_eq!(details.pinned_appearance, None);
    assert_eq!(details.damage_carrier, carrier);
    let report = crate::app::technical_build_report(Some(&build), &recipe, None, "", "");
    let field = |text: &str, label: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(label))
            .unwrap_or_else(|| panic!("missing {label}:\n{text}"))
            .trim()
            .to_owned()
    };
    assert_eq!(field(&report, "Rig Moved From"), rig_donor);
    assert_eq!(field(&report, "Damage Carrier"), "Arc on the weapon");
    assert!(
        report.contains("Current recipe matches this staged build."),
        "{report}"
    );
    let mut edited = recipe.clone();
    edited.overrides.animation_donor = Some(crate::WeaponDonorReference {
        item_hash: NATURE_OF_THE_BEAST.into(),
        expected_name: Some("Nature of the Beast".into()),
    });
    let changed = crate::app::technical_build_report(Some(&build), &edited, None, "", "");
    assert!(
        changed.contains("Current recipe differs from this staged build."),
        "{changed}"
    );
    assert_eq!(field(&changed, "Rig Moved From"), rig_donor);

    if let Some(path) = std::env::var_os("PARHELION_RATE_DAMAGE_REPORT") {
        let report = serde_json::json!({
            "item": format!("0x{:08X}", plan.item_hash),
            "sidearm_converts_as_hand_cannon": built[&SIDEARM] == original[&HAND_CANNON],
            "shots_per_second_at_40": {
                "built": built[&SIDEARM][4][0],
                "stock_hand_cannon": original[&HAND_CANNON][4][0],
                "stock_sidearm": original[&SIDEARM][4][0],
            },
            "damage_socket_lanes": weapon_damage_socket_lanes(&definition).unwrap().len(),
            "damage_carrier": format!("{carrier:?}"),
            "recipe": recipe,
            "packages": build.artifacts,
            "rig_owners": rig(&authored.payload),
            "client_build": sundial::package_authoring::sandbox_perk::nodes::CLIENT_BUILD,
            "gameplay_verified": false,
        });
        let path = PathBuf::from(path);
        fs::write(path.with_extension("txt"), &changed).unwrap();
        fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}

/// Luna's Howl, a Precision Frame hand cannon, takes its hip and aim fire from Ancient Gospel's
/// Adaptive Frame animations and keeps every other action. The build gives it a private copy of
/// the arms rig whose state table answers those two actions as Ancient Gospel's profile would,
/// while every other hand cannon keeps the shared rig. When `PARHELION_ACTIONS_REPORT` names a
/// file, the read-back is written there as JSON.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_single_actions_play_another_frames_animations() {
    use crate::recipe::AnimationAction;
    use crate::weapon::animations::{actions::Machine, arms_rig, profile};
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let namespace = "parhelion.single-actions.integration";
    let spec = WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.to_owned(),
        donor_item_hash: LUNAS_HOWL,
        expected_donor_name: Some("Luna's Howl".to_owned()),
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: "Mixed Howl".to_owned(),
            flavor: "Single actions integration test.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            animation_actions: vec![
                (AnimationAction::Fire, ANCIENT_GOSPEL),
                (AnimationAction::AimFire, ANCIENT_GOSPEL),
            ],
            ..WeaponCloneOverrides::default()
        },
    };
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("single actions from another frame should build");
    let view = staged_view(&packages, ".parhelion-single-actions-test-", &bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let authored =
        load_weapon_runtime_entity_with_manager(&manager, bundle.plan.weapons[0].item_hash)
            .unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let pattern = |hash: u32| {
        catalog
            .weapon_donor(hash)
            .unwrap()
            .summary
            .weapon_pattern_index
            .unwrap()
    };
    let luna =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern(LUNAS_HOWL))
            .unwrap();
    let gospel =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern(ANCIENT_GOSPEL))
            .unwrap();
    let own = profile(&stock, &luna.payload, Some(luna.weapon_content_group_hash))
        .unwrap()
        .keys[1];
    let lent = profile(
        &stock,
        &gospel.payload,
        Some(gospel.weapon_content_group_hash),
    )
    .unwrap()
    .keys[1];
    assert_ne!(own, lent, "the two weapons should play different frames");

    let stock_rig = arms_rig(&stock, &luna.payload, Some(luna.weapon_content_group_hash)).unwrap();
    let authored_rig = arms_rig(
        &manager,
        &authored.payload,
        Some(authored.weapon_content_group_hash),
    )
    .unwrap();
    assert_ne!(
        authored_rig.entity_tag, stock_rig.entity_tag,
        "the weapon should play its own copy of the arms rig"
    );
    assert_ne!(authored_rig.states_tag, stock_rig.states_tag);
    let original = Machine::read(stock_rig.states.clone(), &stock_rig.parameters).unwrap();
    let built = Machine::read(authored_rig.states.clone(), &authored_rig.parameters).unwrap();
    let mut report = serde_json::Map::new();
    for action in AnimationAction::ALL {
        let plays = built.outputs(action, own).unwrap();
        let its_own = original.outputs(action, own).unwrap();
        let borrowed = original.outputs(action, lent).unwrap();
        let mixed = matches!(action, AnimationAction::Fire | AnimationAction::AimFire);
        if mixed {
            assert_ne!(
                its_own, borrowed,
                "{action:?} should differ between the frames"
            );
            assert_eq!(
                plays, borrowed,
                "{action:?} should play Ancient Gospel's branch"
            );
        } else {
            assert_eq!(
                plays, its_own,
                "{action:?} should keep Luna's Howl's own branch"
            );
        }
        report.insert(
            format!("{action:?}"),
            serde_json::json!({ "plays": plays, "own": its_own, "lent": borrowed }),
        );
    }

    // Other hand cannons still play the shared rig.
    let staged_gospel =
        load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern(ANCIENT_GOSPEL))
            .unwrap();
    assert_eq!(
        arms_rig(
            &manager,
            &staged_gospel.payload,
            Some(staged_gospel.weapon_content_group_hash),
        )
        .unwrap()
        .entity_tag,
        stock_rig.entity_tag,
        "every other weapon should keep the stock arms rig"
    );
    if let Some(path) = std::env::var_os("PARHELION_ACTIONS_REPORT") {
        let report = serde_json::json!({
            "item": format!("0x{:08X}", bundle.plan.weapons[0].item_hash),
            "arms_rig": format!("0x{:08X}", authored_rig.entity_tag),
            "stock_arms_rig": format!("0x{:08X}", stock_rig.entity_tag),
            "actions": report,
        });
        fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}

/// Borrows a lightweight Reload on an exotic submachine gun and reads its packed private rig.
/// PARHELION_TEST_ARTIFACTS retains the read-back and staged package hashes.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
#[allow(
    clippy::cognitive_complexity,
    reason = "One staged build is read back part by part in sequence"
)]
fn real_single_actions_reload_between_submachine_gun_profiles() {
    use crate::recipe::AnimationAction;
    use crate::weapon::animations::{actions::Machine, arms_rig, profile};

    const DEATH_ADDER: u32 = 0x960F_8322;
    const HUCKLEBERRY: u32 = 0x8843_C72A;
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let cases = [(
        HUCKLEBERRY,
        "The Huckleberry",
        DEATH_ADDER,
        "parhelion.single-actions.reload.exotic",
        -3,
        -2,
    )];
    let project = WeaponProjectSpec {
        weapons: cases
            .iter()
            .map(|&(base, name, donor, namespace, _, _)| WeaponCloneSpec {
                kind: crate::ItemKind::Weapon,
                namespace: namespace.to_owned(),
                donor_item_hash: base,
                expected_donor_name: Some(name.to_owned()),
                presentation_donor: None,
                icon_donor: None,
                render_gear_donor: None,
                runtime_component_donors: Vec::new(),
                identity: WeaponCloneIdentity::from_namespace(namespace).unwrap(),
                text: WeaponCloneText {
                    name: format!("Mixed {name}"),
                    flavor: "Single reload integration test.".to_owned(),
                    source: "Source: integration test".to_owned(),
                    ..WeaponCloneText::default()
                },
                overrides: WeaponCloneOverrides {
                    animation_actions: vec![(AnimationAction::Reload, donor)],
                    ..WeaponCloneOverrides::default()
                },
            })
            .collect(),
    };
    let bundle = build_weapon_project(&packages, &project)
        .expect("an exotic submachine gun borrowing a lightweight reload should build");
    let view = staged_view(&packages, ".parhelion-single-actions-reload-test-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let pattern = |hash| {
        catalog
            .weapon_donor(hash)
            .unwrap()
            .summary
            .weapon_pattern_index
            .unwrap()
    };
    let mut receipts = Vec::new();
    for &(base, name, donor, namespace, expected_own, expected_lent) in &cases {
        let base_entity =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern(base))
                .unwrap();
        let donor_entity =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, pattern(donor))
                .unwrap();
        let own = profile(
            &stock,
            &base_entity.payload,
            Some(base_entity.weapon_content_group_hash),
        )
        .unwrap();
        let lent = profile(
            &stock,
            &donor_entity.payload,
            Some(donor_entity.weapon_content_group_hash),
        )
        .unwrap();
        assert_eq!(
            own.owner, lent.owner,
            "the profiles must fit the same attachment"
        );
        assert_ne!(own.keys[1], lent.keys[1]);
        let stock_rig = arms_rig(
            &stock,
            &base_entity.payload,
            Some(base_entity.weapon_content_group_hash),
        )
        .unwrap();
        let donor_rig = arms_rig(
            &stock,
            &donor_entity.payload,
            Some(donor_entity.weapon_content_group_hash),
        )
        .unwrap();
        assert_eq!(stock_rig.entity_tag, donor_rig.entity_tag);
        let original = Machine::read(stock_rig.states.clone(), &stock_rig.parameters).unwrap();
        // Independent census values: full and empty reload share node 5, hence one output.
        assert_eq!(
            original
                .outputs(AnimationAction::Reload, own.keys[1])
                .unwrap(),
            Some(vec![expected_own])
        );
        assert_eq!(
            original
                .outputs(AnimationAction::Reload, lent.keys[1])
                .unwrap(),
            Some(vec![expected_lent])
        );

        let item = project
            .weapons
            .iter()
            .find(|spec| spec.namespace == namespace)
            .unwrap()
            .identity
            .item_hash;
        let authored = load_weapon_runtime_entity_with_manager(&staged, item).unwrap();
        let authored_rig = arms_rig(
            &staged,
            &authored.payload,
            Some(authored.weapon_content_group_hash),
        )
        .unwrap();
        assert_ne!(authored_rig.entity_tag, stock_rig.entity_tag);
        assert_ne!(authored_rig.states_tag, stock_rig.states_tag);
        let built = Machine::read(authored_rig.states.clone(), &authored_rig.parameters).unwrap();
        assert_eq!(
            profile(
                &staged,
                &authored.payload,
                Some(authored.weapon_content_group_hash)
            )
            .unwrap()
            .keys,
            own.keys,
            "one action must not replace the attachment profile"
        );
        let mut actions = serde_json::Map::new();
        for action in AnimationAction::ALL {
            let plays = built.outputs(action, own.keys[1]).unwrap();
            let its_own = original.outputs(action, own.keys[1]).unwrap();
            let borrowed = original.outputs(action, lent.keys[1]).unwrap();
            if action == AnimationAction::Reload {
                assert_eq!(
                    plays,
                    Some(vec![expected_lent]),
                    "{name} must take the donor's reload"
                );
                assert_eq!(
                    built.outputs(action, lent.keys[1]).unwrap(),
                    plays,
                    "the private reload must answer the frozen choice for either profile"
                );
            } else {
                assert_eq!(plays, its_own, "{name} must keep its own {action:?}");
            }
            actions.insert(
                format!("{action:?}"),
                serde_json::json!({"plays": plays, "own": its_own, "lent": borrowed}),
            );
        }
        // Read both stock rows through the staged manager, not just the source manager.
        for hash in [base, donor] {
            let entity =
                load_weapon_runtime_entity_at_pattern_index_with_manager(&staged, pattern(hash))
                    .unwrap();
            let untouched = arms_rig(
                &staged,
                &entity.payload,
                Some(entity.weapon_content_group_hash),
            )
            .unwrap();
            assert_eq!(
                untouched.entity_tag, stock_rig.entity_tag,
                "stock weapons must keep their shared rig"
            );
            assert_eq!(
                untouched.states, stock_rig.states,
                "stock selectors must remain unchanged"
            );
        }
        receipts.push(serde_json::json!({
            "namespace": namespace, "base": format!("0x{base:08X}"), "donor": format!("0x{donor:08X}"),
            "item": format!("0x{item:08X}"), "own_profile": format!("0x{:08X}", own.keys[1]),
            "lent_profile": format!("0x{:08X}", lent.keys[1]),
            "stock_arms_rig": format!("0x{:08X}", stock_rig.entity_tag),
            "arms_rig": format!("0x{:08X}", authored_rig.entity_tag),
            "states": format!("0x{:08X}", authored_rig.states_tag), "actions": actions,
        }));
    }
    let package_hashes = bundle
        .artifacts
        .iter()
        .map(|artifact| {
            let name = &artifact.plan.output_file_name;
            let digest =
                crate::artifact::digest_file(&view.path().join("packages").join(name)).unwrap();
            serde_json::json!({"file": name, "sha256": digest.sha256, "bytes": digest.byte_length})
        })
        .collect::<Vec<_>>();
    crate::test_support::artifact(
        "single-actions-reload.json",
        &serde_json::json!({
            "cases": receipts, "packages": package_hashes,
            "verification": "Staged package read-back. Untested in game.",
        }),
    );
}
