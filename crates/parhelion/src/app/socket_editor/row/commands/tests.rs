use super::*;
use crate::app::{ActivityLog, PlugSelectionMode};
use crate::{RecipeLibrary, WeaponRecipe};
use sundial::investment::InvestmentCatalog;

#[test]
#[ignore = "requires PARHELION_SOCKET_TEST_PACKAGES with an installed Vaultbreaker Frame"]
fn native_normal_picker_imports_frame_builds_and_can_restore_stock_perk() {
    let packages =
        std::path::PathBuf::from(std::env::var_os("PARHELION_SOCKET_TEST_PACKAGES").unwrap());
    let temporary = tempfile::tempdir().unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &temporary.path().join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let library = RecipeLibrary::open(temporary.path().join("recipes")).unwrap();
    let source = WeaponRecipe::from_json_str(include_str!(
        "../../../../../recipes/vaultbreaker.parhelion.json"
    ))
    .unwrap();
    let installed_source = catalog
        .weapon_donor(source.identity.item_hash.parse_u32().unwrap())
        .unwrap();
    let hash = installed_source.sockets[0].native_default.unwrap();
    assert_ne!(hash, 0xDD5C_B37A);
    let source_effects =
        catalog.weapon_sandbox_perk_choices_from(crate::package_profile::is_stock_item_definition);
    assert!(
        source_effects
            .iter()
            .any(|effect| effect.perk_index == 1178)
    );
    let stock_effects = catalog.item_sandbox_perk_indices(0xDD5C_B37A);
    for private in catalog
        .item_sandbox_perk_indices(hash)
        .into_iter()
        .filter(|index| !stock_effects.contains(index) && *index != 405)
    {
        assert!(
            !source_effects
                .iter()
                .any(|effect| effect.perk_index == private)
        );
    }
    let donor = catalog.weapon_donor(0x4CE3_CE93).unwrap();
    let mut target = WeaponRecipe::new_weapon_for_donor(
        "parhelion.normal-picker",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let mut queries = std::collections::BTreeMap::new();
    let mut page = 0;
    let mut technical = false;
    let mut private_socket = None;
    let mut log = ActivityLog::new(crate::app::LogEntry::info("Picker test"));
    let mut context = SocketRowContext {
        catalog: &catalog,
        recipe_library: Some(&library),
        recipe: &mut target,
        queries: &mut queries,
        page: &mut page,
        plug_selection_mode: PlugSelectionMode::AnyPlug,
        donor: &donor,
        socket_index: 0,
        is_added: false,
        can_remove_added: false,
        show_experimental_options: false,
        show_technical_row: &mut technical,
        private_perk_socket: &mut private_socket,
        log: &mut log,
    };
    let choices = RowChoices::read(&mut context).unwrap();
    apply(
        &mut context,
        &choices,
        Some(RowCommand::EditChoice {
            index: 0,
            hash: Some(hash),
        }),
    );
    assert_eq!(context.recipe.overrides.socket_plug_variants.len(), 1);
    assert_eq!(
        context.recipe.overrides.socket_plug_variants[0].sandbox_perks,
        source.overrides.socket_plug_variants[0].sandbox_perks
    );
    assert_eq!(
        recipe_socket_choices(context.recipe, 0, &[]).unwrap()[0],
        0xDD5C_B37A
    );
    let selected = context.recipe.clone();
    let mut legacy = selected.clone();
    legacy.overrides.socket_plug_variants.clear();
    legacy.overrides.socket_columns[0].as_mut().unwrap().choices[0] = hash.into();
    assert_eq!(
        crate::app::custom_perks::repair_socket_picks(Some(&library), &catalog, &mut legacy)
            .unwrap(),
        1
    );
    assert_eq!(legacy, selected);
    assert_eq!(
        crate::app::custom_perks::repair_socket_picks(Some(&library), &catalog, &mut legacy)
            .unwrap(),
        0
    );
    let persisted =
        WeaponRecipe::from_json_str(&serde_json::to_string(&selected).unwrap()).unwrap();
    assert_eq!(persisted, selected);

    let choices = RowChoices::read(&mut context).unwrap();
    apply(
        &mut context,
        &choices,
        Some(RowCommand::EditChoice {
            index: 0,
            hash: Some(0xDD5C_B37A),
        }),
    );
    assert!(
        context.recipe.overrides.socket_plug_variants.is_empty(),
        "Selecting the stock perk must remove the custom edits even though they share a source hash"
    );

    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages,
        staging_root: temporary.path().join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: vec![selected],
    })
    .unwrap();
    let built = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    assert_eq!(built.weapons.len(), 1);
    assert!(built.manifest_path.is_file());
}
