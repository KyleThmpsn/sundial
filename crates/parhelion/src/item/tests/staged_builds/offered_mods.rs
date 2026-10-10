//! A custom weapon mod offered everywhere, built the way the Custom Perk Workbench applies one:
//! Type Weapon Mod, whose plug is Boss Spec, in Breachlight's mod socket. The staged packages
//! must offer it in every shared plug set that offers Boss Spec, always available, with every
//! stock member as it shipped, and a fresh catalog scan must find it in those sets.
use super::*;
use crate::perk::PerkRecipe;
use crate::recipe::WeaponSocketColumnRecipe;
use sundial::package_authoring::investment_schema::ROOT_REUSABLE_PLUG_SET_TABLE_SLOT;

const BREACHLIGHT: u32 = 0x4CE3_CE93;
const BOSS_SPEC: u32 = 0xA63B_627D;

fn investment_root(manager: &sundial::package_authoring::PackageManager) -> Vec<u8> {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    manager
        .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
        .unwrap()
}

fn item_index(manager: &sundial::package_authoring::PackageManager, hash: u32) -> u16 {
    let items = manager
        .read_tag(
            root_child_tag(&investment_root(manager), ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        )
        .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let index = find_u32_row_key(&items, rows, count, ITEM_ROW_SIZE, hash)
        .unwrap()
        .unwrap();
    u16::try_from(index).unwrap()
}

/// Each shared plug set's members in package order: the item index and the condition
/// instructions, read straight from the table.
fn plug_sets(manager: &sundial::package_authoring::PackageManager) -> Vec<Vec<(u16, Vec<u64>)>> {
    let tag = root_child_tag(&investment_root(manager), ROOT_REUSABLE_PLUG_SET_TABLE_SLOT).unwrap();
    let table = manager.read_tag(tag).unwrap();
    assert_eq!(read_u64(&table, 0).unwrap() as usize, table.len());
    let (sets, _, rows, _) = array_at(&table, 8).unwrap();
    (0..sets)
        .map(|set| {
            let (members, _, member_rows, _) = array_at(&table, rows + set * 0x18 + 8).unwrap();
            (0..members)
                .map(|member| {
                    let at = member_rows + member * 0x20;
                    let (conditions, _, condition_rows, _) = array_at(&table, at + 8).unwrap();
                    let condition = (0..conditions)
                        .map(|index| read_u64(&table, condition_rows + index * 8).unwrap())
                        .collect();
                    (read_u16(&table, at).unwrap(), condition)
                })
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_offered_mod_joins_every_plug_set_that_offers_its_type_plug() {
    let packages = crate::test_support::stock_packages();
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache_path = temporary.path().join("catalog.json");
    let baseline =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let donor = baseline.weapon_donor(BREACHLIGHT).unwrap();
    let socket = donor
        .sockets
        .iter()
        .find(|socket| socket.label.ends_with("Weapon Mod"))
        .unwrap();
    let stock_counts = baseline.reusable_set_counts();
    assert!(stock_counts[&BOSS_SPEC] > 1);
    let source = open_manager(view.path()).unwrap();
    let boss_spec = item_index(&source, BOSS_SPEC);
    let stock_sets = plug_sets(&source);
    drop(source);
    let offering = stock_sets
        .iter()
        .enumerate()
        .filter(|(_, members)| members.iter().any(|(index, _)| *index == boss_spec))
        .map(|(set, _)| u16::try_from(set).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(offering.len(), stock_counts[&BOSS_SPEC]);

    let mut perk = PerkRecipe::new();
    perk.name = "Offered Spec".into();
    perk.classification = Some(BOSS_SPEC.into());
    perk.effects = baseline
        .item_sandbox_perk_indices(BOSS_SPEC)
        .into_iter()
        .map(PerkRecipe::effect)
        .collect();
    perk.offer_everywhere = true;
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.offered-mod.integration",
        BREACHLIGHT,
        "Breachlight",
    )
    .unwrap();
    recipe.name = "Offered Mod".into();
    recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
    recipe.overrides.socket_columns[socket.index] = Some(WeaponSocketColumnRecipe {
        choices: vec![perk.template_plug.clone()],
        reusable_plug_set_index: socket.reusable_plug_set_index,
        ..Default::default()
    });
    recipe.overrides.socket_plug_variants = vec![perk.at_socket(socket.index as u16, 0)];
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: temporary.path().join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe],
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let [weapon] = build.weapons.as_slice() else {
        panic!("one weapon was built");
    };
    let [plug] = weapon.custom_plugs.as_slice() else {
        panic!("one private plug was built");
    };
    assert_eq!(plug.offered_sets, offering);
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }

    // Each offering set gains the private plug, unconditioned, after its stock members.
    let staged = open_manager(view.path()).unwrap();
    let staged_sets = plug_sets(&staged);
    drop(staged);
    assert_eq!(staged_sets.len(), stock_sets.len());
    for (set, (stock, staged)) in stock_sets.iter().zip(&staged_sets).enumerate() {
        let mut expected = stock.clone();
        if offering.contains(&u16::try_from(set).unwrap()) {
            expected.push((plug.item_index, Vec::new()));
        }
        assert_eq!(staged, &expected, "plug set {set}");
    }

    // The catalog reads the sockets the way the app and Dawn do.
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let counts = catalog.reusable_set_counts();
    assert_eq!(counts.get(&plug.item_hash), Some(&offering.len()));
    for (hash, count) in &stock_counts {
        assert_eq!(counts.get(hash), Some(count), "plug 0x{hash:08X}");
    }
    let authored = catalog
        .weapon_donor(weapon.item_hash)
        .unwrap()
        .sockets
        .swap_remove(socket.index);
    assert_eq!(
        authored.reusable_plug_set_index,
        socket.reusable_plug_set_index
    );
    assert_eq!(authored.native_default, Some(plug.item_hash));
    crate::test_support::artifact(
        "offered-mod.json",
        &serde_json::json!({
            "plug": format!("0x{:08X}", plug.item_hash),
            "item_index": plug.item_index,
            "offered_like": format!("0x{BOSS_SPEC:08X}"),
            "offered_sets": plug.offered_sets,
            "staging": build.run_directory,
        }),
    );
}

/// A mod item built beside a weapon whose own perk is also offered everywhere. The mod's perk
/// takes the mod's item place rather than a host socket, so both must join the sets that offer
/// Boss Spec, unconditioned, with the weapon's private plug still after the items and every
/// stock member unchanged. A fresh catalog scan must name the mod by its own text and type.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn real_mod_item_is_offered_everywhere_without_a_host() {
    let packages = crate::test_support::stock_packages();
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let cache_path = temporary.path().join("catalog.json");
    let baseline =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let donor = baseline.weapon_donor(BREACHLIGHT).unwrap();
    let socket = donor
        .sockets
        .iter()
        .find(|socket| socket.label.ends_with("Weapon Mod"))
        .unwrap();
    let stock_counts = baseline.reusable_set_counts();
    let source = open_manager(view.path()).unwrap();
    let boss_spec = item_index(&source, BOSS_SPEC);
    let stock_sets = plug_sets(&source);
    drop(source);
    let offering = stock_sets
        .iter()
        .enumerate()
        .filter(|(_, members)| members.iter().any(|(index, _)| *index == boss_spec))
        .map(|(set, _)| u16::try_from(set).unwrap())
        .collect::<Vec<_>>();
    let perk = |name: &str| {
        let mut perk = PerkRecipe::new();
        perk.name = name.into();
        perk.description = format!("{name} description.");
        perk.classification = Some(BOSS_SPEC.into());
        perk.effects = baseline
            .item_sandbox_perk_indices(BOSS_SPEC)
            .into_iter()
            .map(PerkRecipe::effect)
            .collect();
        perk.offer_everywhere = true;
        perk
    };

    // The mod, as Apply to Mod writes it.
    let mod_perk = perk("Standalone Spec");
    let mut item = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Mod).unwrap();
    item.set_donor(mod_perk.template_plug.parse_u32().unwrap(), String::new());
    item.overrides.socket_plug_variants = vec![mod_perk.at_socket(0, 0)];
    item.flavor.clone_from(&mod_perk.description);
    item.rename_authored_item(mod_perk.name.clone()).unwrap();
    // A weapon whose own socket perk is offered everywhere too.
    let socket_perk = perk("Socket Spec");
    let mut weapon = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.offered-mod.weapon",
        BREACHLIGHT,
        "Breachlight",
    )
    .unwrap();
    weapon.name = "Socket Spec Host".into();
    weapon.overrides.socket_columns = vec![None; donor.sockets.len()];
    weapon.overrides.socket_columns[socket.index] = Some(WeaponSocketColumnRecipe {
        choices: vec![socket_perk.template_plug.clone()],
        reusable_plug_set_index: socket.reusable_plug_set_index,
        ..Default::default()
    });
    weapon.overrides.socket_plug_variants = vec![socket_perk.at_socket(socket.index as u16, 0)];
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: temporary.path().join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![item, weapon],
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    assert_eq!(build.weapons.len(), 2, "a mod and a weapon were built");
    let built = |kind| {
        build
            .weapons
            .iter()
            .find(|built| built.kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?} was built"))
    };
    let (built_mod, built_weapon) = (built(crate::ItemKind::Mod), built(crate::ItemKind::Weapon));
    assert!(
        built_mod.collection.is_none(),
        "a mod has no Collections entry"
    );
    let [mod_plug] = built_mod.custom_plugs.as_slice() else {
        panic!("the mod reports its one perk");
    };
    let [socket_plug] = built_weapon.custom_plugs.as_slice() else {
        panic!("the weapon has one private plug");
    };
    // The mod's perk is the mod's own item, and the socket plug follows the items.
    assert_eq!(mod_plug.item_hash, built_mod.item_hash);
    assert_eq!(mod_plug.item_index, built_mod.item_index);
    assert_eq!(mod_plug.definition_hash, built_mod.item_definition_hash);
    assert!(socket_plug.item_index > built_mod.item_index.max(built_weapon.item_index));
    assert_eq!(mod_plug.offered_sets, offering);
    assert_eq!(socket_plug.offered_sets, offering);
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }

    // Each offering set gains both plugs, unconditioned, after its stock members.
    let staged = open_manager(view.path()).unwrap();
    let staged_sets = plug_sets(&staged);
    assert_eq!(
        item_index(&staged, built_mod.item_hash),
        built_mod.item_index
    );
    drop(staged);
    assert_eq!(staged_sets.len(), stock_sets.len());
    for (set, (stock, staged)) in stock_sets.iter().zip(&staged_sets).enumerate() {
        let joined = &staged[stock.len().min(staged.len())..];
        assert_eq!(&staged[..stock.len()], stock.as_slice(), "plug set {set}");
        if offering.contains(&u16::try_from(set).unwrap()) {
            let mut indices = joined.iter().map(|(index, _)| *index).collect::<Vec<_>>();
            indices.sort_unstable();
            let mut expected = vec![mod_plug.item_index, socket_plug.item_index];
            expected.sort_unstable();
            assert_eq!(indices, expected, "plug set {set} members");
            assert!(joined.iter().all(|(_, condition)| condition.is_empty()));
        } else {
            assert!(joined.is_empty(), "plug set {set} gained members");
        }
    }

    // The catalog reads the mod by its own text and its type's, offered where Boss Spec is.
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let counts = catalog.reusable_set_counts();
    assert_eq!(counts.get(&built_mod.item_hash), Some(&offering.len()));
    for (hash, count) in &stock_counts {
        assert_eq!(counts.get(hash), Some(count), "plug 0x{hash:08X}");
    }
    let name = catalog.plug_label(built_mod.item_hash, false);
    assert_eq!(name, "Standalone Spec");
    assert_eq!(
        catalog.item_type_name(built_mod.item_hash),
        catalog.item_type_name(BOSS_SPEC)
    );
    crate::test_support::artifact(
        "mod-item.json",
        &serde_json::json!({
            "mod": format!("0x{:08X}", built_mod.item_hash),
            "mod_item_index": built_mod.item_index,
            "mod_name": name,
            "mod_type": catalog.item_type_name(built_mod.item_hash),
            "socket_plug": format!("0x{:08X}", socket_plug.item_hash),
            "socket_plug_item_index": socket_plug.item_index,
            "offered_like": format!("0x{BOSS_SPEC:08X}"),
            "offered_sets": mod_plug.offered_sets,
            "staging": build.run_directory,
        }),
    );
}
