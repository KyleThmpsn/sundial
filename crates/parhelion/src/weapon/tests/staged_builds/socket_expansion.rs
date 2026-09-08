use super::*;
use crate::recipe::{
    WeaponSandboxPerkRuntimeRecipe, WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe,
};

const BREACHLIGHT: u32 = 0x4CE3_CE93;
const MICRO_MISSILE: u32 = 0xDD5C_B37A;

fn expansion_recipe(count: usize, alternative: u32, second_plug: u32) -> crate::WeaponRecipe {
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        format!("parhelion.added-sockets.{count}.integration"),
        BREACHLIGHT,
        "Breachlight",
    )
    .unwrap();
    recipe.name = format!("Added Sockets {count}");
    recipe.overrides.socket_columns = vec![None; count];
    recipe.overrides.socket_columns[10] = Some(WeaponSocketColumnRecipe {
        choices: vec![MICRO_MISSILE.into(), alternative.into()],
        socket_type: Some(92),
        ..Default::default()
    });
    if count == 12 {
        recipe.overrides.socket_columns[11] = Some(WeaponSocketColumnRecipe {
            choices: vec![second_plug.into()],
            socket_type: Some(92),
            ..Default::default()
        });
    }
    recipe.overrides.socket_plug_variants = vec![WeaponSocketPlugVariantRecipe {
        investment_stats: Vec::new(),
        socket_index: 10,
        choice_index: 0,
        source_plug_hash: MICRO_MISSILE.into(),
        name: Some(format!("Added Socket Private Perk {count}")),
        classification_donor_hash: None,
        description: None,
        additional_sandbox_perks: Vec::new(),
        sandbox_perks: vec![WeaponSandboxPerkRuntimeRecipe {
            source_perk_index: 1178,
            activation: None,
            runtime_values: Vec::new(),
            action_float_values: Vec::new(),
        }],
    }];
    recipe
}

fn item_table(manager: &tiger_pkg::PackageManager) -> Vec<u8> {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
        .unwrap();
    manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap()
}

fn item_definition(
    manager: &tiger_pkg::PackageManager,
    items: &[u8],
    hash: u32,
) -> (TagHash, Vec<u8>) {
    let (count, _, rows, _) = array_at(items, 8).unwrap();
    let index = find_u32_row_key(items, rows, count, ITEM_ROW_SIZE, hash)
        .unwrap()
        .unwrap();
    let tag = TagHash(read_u32(items, rows + index * ITEM_ROW_SIZE + 16).unwrap());
    (tag, manager.read_tag(tag).unwrap())
}

fn default_hashes(items: &[u8], definition: &[u8]) -> Vec<Option<u32>> {
    let (_, _, rows, _) = array_at(items, 8).unwrap();
    weapon_default_plug_indices(definition)
        .unwrap()
        .into_iter()
        .map(|index| {
            (index != u16::MAX)
                .then(|| read_u32(items, rows + usize::from(index) * ITEM_ROW_SIZE).unwrap())
        })
        .collect()
}

fn verify_private_effect(
    manager: &tiger_pkg::PackageManager,
    catalog: &InvestmentCatalog,
    private_hash: u32,
    source_perks: &[u16],
) {
    let private_perks = catalog.item_sandbox_perk_indices(private_hash);
    assert_eq!(private_perks.len(), source_perks.len());
    assert!(!private_perks.contains(&1178));
    let private_perk = *private_perks
        .iter()
        .find(|index| !source_perks.contains(index))
        .unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let stock_runtime = load_sandbox_perk_runtime_action(manager, &globals, 1178).unwrap();
    let private_runtime =
        load_sandbox_perk_runtime_action(manager, &globals, usize::from(private_perk)).unwrap();
    assert_eq!(private_runtime.action_tag, stock_runtime.action_tag);
    assert_eq!(private_runtime.action_payload, stock_runtime.action_payload);
}

fn verify_expanded_weapon(
    manager: &tiger_pkg::PackageManager,
    catalog: &InvestmentCatalog,
    items: &[u8],
    recipe: &crate::WeaponRecipe,
    native_defaults: &[Option<u32>],
    source_perks: &[u16],
) {
    let hash = recipe.identity.item_hash.parse_u32().unwrap();
    let count = recipe.overrides.socket_columns.len();
    let (_, definition) = item_definition(manager, items, hash);
    let defaults = default_hashes(items, &definition);
    let donor = catalog.weapon_donor(hash).unwrap();
    assert_eq!(defaults.len(), count);
    assert_eq!(donor.sockets.len(), count);
    assert_eq!(&defaults[..10], native_defaults);
    assert_eq!(read_u64(&definition, 0).unwrap() as usize, definition.len());
    assert_eq!(
        donor
            .sockets
            .iter()
            .map(|socket| socket.native_default)
            .collect::<Vec<_>>(),
        defaults
    );
    let private_hash = defaults[10].unwrap();
    assert_ne!(private_hash, MICRO_MISSILE);
    verify_private_effect(manager, catalog, private_hash, source_perks);
    let column = recipe.overrides.socket_columns[10].as_ref().unwrap();
    assert_eq!(donor.sockets[10].socket_type, 92);
    assert_eq!(
        donor.sockets[10].ordered_embedded_choices,
        vec![private_hash, column.choices[1].parse_u32().unwrap()]
    );
    assert_eq!(
        catalog
            .private_plug_tooltip(private_hash, None, None, None)
            .lines()
            .next(),
        Some(format!("Added Socket Private Perk {count}").as_str())
    );
    if count == 12 {
        let column = recipe.overrides.socket_columns[11].as_ref().unwrap();
        assert_eq!(donor.sockets[11].socket_type, 92);
        assert_eq!(defaults[11], Some(column.choices[0].parse_u32().unwrap()));
    }
}

#[test]
#[ignore = "requires PARHELION_SOCKET_TEST_PACKAGES pointing to Shadowkeep packages"]
fn real_added_sockets_stage_and_rescan_with_private_plugs_and_unchanged_stock() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_SOCKET_TEST_PACKAGES").unwrap());
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
    assert_eq!(donor.sockets.len(), 10);
    let alternative = donor.sockets[3].native_default.unwrap();
    let second_plug = donor.sockets[4].native_default.unwrap();
    let source_perks = baseline.item_sandbox_perk_indices(MICRO_MISSILE);
    assert!(!source_perks.is_empty());
    let source = open_manager(view.path()).unwrap();
    let source_items = item_table(&source);
    let (stock_tag, stock_definition) = item_definition(&source, &source_items, BREACHLIGHT);
    let (plug_tag, source_plug) = item_definition(&source, &source_items, MICRO_MISSILE);
    let native_defaults = default_hashes(&source_items, &stock_definition);
    drop(source);
    let recipes = vec![
        expansion_recipe(11, alternative, second_plug),
        expansion_recipe(12, alternative, second_plug),
    ];
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: temporary.path().join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: recipes.clone(),
    })
    .unwrap();
    let mut completed = false;
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |progress| {
        eprintln!(
            "Added sockets: {} {}/{}",
            progress.phase.label(),
            progress.completed,
            progress.total
        );
        completed |= progress.phase == crate::workflow::BuildPhase::Complete;
    })
    .unwrap();
    assert!(
        completed,
        "staged package artifact validation must complete"
    );
    assert_eq!(build.weapons.len(), 2);
    assert!(build.manifest_path.is_file());
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(view.path()).unwrap();
    let staged_items = item_table(&staged);
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    for recipe in &recipes {
        verify_expanded_weapon(
            &staged,
            &catalog,
            &staged_items,
            recipe,
            &native_defaults,
            &source_perks,
        );
    }
    assert_eq!(staged.read_tag(stock_tag).unwrap(), stock_definition);
    assert_eq!(staged.read_tag(plug_tag).unwrap(), source_plug);
}
