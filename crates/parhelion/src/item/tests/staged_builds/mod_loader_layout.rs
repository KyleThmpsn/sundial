//! A build for Sunrise 0.6's mod loader, read back from its staged packages beside the same
//! recipe built for an earlier runtime. The loader merges a mod patch entry by entry and refuses
//! one whose entry table differs in length from its base, so the build's new investment records
//! go to a package of their own there, while Dawn and earlier Sunrise keep them appended.
//!
//! Failure inventory, written before the layout change:
//! - a stock patch adds entries, the investment host's or the private perk runtime's
//! - the records' package is missing from the build, or is not registered, so the client never
//!   finds it
//! - an item row names its definition at an index the records' package does not hold
//! - the records' package holds a different set of records than the earlier layout appended
//! - Sundial's reader cannot find the authored item or its private perk in the packages
//! - a build for Dawn or an earlier Sunrise stops appending its records to the stock host
use super::*;
use crate::perk::PerkRecipe;
use crate::recipe::WeaponSocketColumnRecipe;
use crate::workflow::FilteredPackageView;

const BREACHLIGHT: u32 = 0x4CE3_CE93;
const BOSS_SPEC: u32 = 0xA63B_627D;
const RECORDS_PREFIX: &str = "w64_parhelion_investment_";

/// A staged package's file name, id and entry count, and the entry count of the stock generation
/// it patches.
type Staged = (String, u16, usize, Option<usize>);

/// A temporary installation over the stock packages whose `steam_api64.dll` is `runtime`.
fn installation(packages: &Path, runtime: Option<&Path>) -> FilteredPackageView {
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = FilteredPackageView::create(packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    if let Some(runtime) = runtime {
        fs::create_dir_all(install.join("bin/x64")).unwrap();
        fs::copy(runtime, install.join("bin/x64/steam_api64.dll")).unwrap();
    }
    view
}

/// Builds `recipe` for `view`'s installation and adds the staged packages to the view.
fn build(
    view: &FilteredPackageView,
    staging: &Path,
    recipe: crate::WeaponRecipe,
) -> crate::BuildReport {
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: staging.to_path_buf(),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe],
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    build
}

/// Each staged package as its file reads, beside the stock generation it patches.
fn layout(view: &FilteredPackageView, build: &crate::BuildReport) -> Vec<Staged> {
    let header =
        |path: &Path| crate::format::PackageLayout::parse(&fs::read(path).unwrap()).unwrap();
    build
        .artifacts
        .iter()
        .map(|artifact| {
            let staged = header(&build.run_directory.join(&artifact.file_name));
            let stock = crate::package_profile::canonical_package(staged.package_id).map(|stock| {
                header(
                    &view
                        .path()
                        .join(stock.stock_file_name(stock.stock_patch_id)),
                )
                .entry_count
            });
            (
                artifact.file_name.clone(),
                staged.package_id,
                staged.entry_count,
                stock,
            )
        })
        .collect()
}

/// The authored item's definition tag, read from the item table the packages now hold.
fn definition(manager: &sundial::package_authoring::PackageManager, hash: u32) -> TagHash {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
        .unwrap();
    let items = manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let index = find_u32_row_key(&items, rows, count, ITEM_ROW_SIZE, hash)
        .unwrap()
        .unwrap();
    TagHash(read_u32(&items, rows + index * ITEM_ROW_SIZE + 16).unwrap())
}

/// Builds a renamed Breachlight with a private perk offered everywhere twice: for an installation
/// without a mod loader, and for one whose `steam_api64.dll` is the configured Sunrise 0.6 or later
/// runtime. Both staged layouts are read back against the stock packages they extend. The loader
/// build's packages are copied to the artifacts folder for an in-game check and are never
/// installed here.
#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNRISE_MOD_LOADER_DLL naming a Sunrise 0.6 or later steam_api64.dll"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn mod_loader_build_moves_its_records_to_their_own_registered_package() {
    let packages = crate::test_support::stock_packages();
    let runtime = crate::test_support::mod_loader_runtime();
    let earlier = installation(&packages, None);
    let loader = installation(&packages, Some(runtime.as_path()));
    assert!(!crate::install::uses_mod_loader(earlier.path()));
    assert!(
        crate::install::uses_mod_loader(loader.path()),
        "the configured runtime loads Sunrise mods"
    );
    let temporary = tempfile::tempdir().unwrap();
    let cache_path = temporary.path().join("catalog.json");
    let install = loader.path().parent().unwrap();
    let baseline =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let donor = baseline.weapon_donor(BREACHLIGHT).unwrap();
    let socket = donor
        .sockets
        .iter()
        .find(|socket| socket.label.ends_with("Weapon Mod"))
        .unwrap();
    let mut perk = PerkRecipe::new();
    perk.name = "Loader Spec".into();
    perk.classification = Some(BOSS_SPEC.into());
    perk.effects = baseline
        .item_sandbox_perk_indices(BOSS_SPEC)
        .into_iter()
        .map(PerkRecipe::effect)
        .collect();
    perk.offer_everywhere = true;
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.mod-loader-layout.integration",
        BREACHLIGHT,
        "Breachlight",
    )
    .unwrap();
    recipe.name = "Mod Loader Layout".into();
    recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
    recipe.overrides.socket_columns[socket.index] = Some(WeaponSocketColumnRecipe {
        choices: vec![perk.template_plug.clone()],
        reusable_plug_set_index: socket.reusable_plug_set_index,
        ..Default::default()
    });
    recipe.overrides.socket_plug_variants = vec![perk.at_socket(socket.index as u16, 0)];

    // Without a mod loader the records follow the stock host's entries, as they always have.
    let host = crate::package_profile::HOST_PACKAGE_ID;
    let stock_entries = crate::package_profile::HOST_EXPECTED_ENTRY_COUNT;
    let earlier_build = build(&earlier, &temporary.path().join("earlier"), recipe.clone());
    let earlier_layout = layout(&earlier, &earlier_build);
    assert!(
        earlier_layout
            .iter()
            .all(|(name, ..)| !name.starts_with(RECORDS_PREFIX))
    );
    let appended = earlier_layout
        .iter()
        .find(|(_, id, ..)| *id == host)
        .map(|(_, _, entries, stock)| entries - stock.unwrap())
        .expect("the earlier layout patches the stock host");
    assert!(appended > 0);
    let manager = open_manager(earlier.path()).unwrap();
    let earlier_definition = definition(&manager, earlier_build.weapons[0].item_hash);
    drop(manager);
    assert_eq!(earlier_definition.pkg_id(), host);
    assert!(usize::from(earlier_definition.entry_index()) >= stock_entries);

    // With the loader, every stock patch keeps its stock entry table and the same records take a
    // package of their own, in the same order.
    let loader_build = build(&loader, &temporary.path().join("loader"), recipe);
    let [weapon] = loader_build.weapons.as_slice() else {
        panic!("one weapon was built");
    };
    let loader_layout = layout(&loader, &loader_build);
    for (name, _, entries, stock) in &loader_layout {
        if let Some(stock) = stock {
            assert_eq!(entries, stock, "{name} keeps its stock entry count");
        }
    }
    let records = loader_layout
        .iter()
        .filter(|(name, ..)| name.starts_with(RECORDS_PREFIX))
        .map(|&(_, id, entries, _)| (id, entries))
        .collect::<Vec<_>>();
    let [(records_id, records_count)] = records[..] else {
        panic!("the build stages one package of investment records");
    };
    assert!(crate::package_profile::is_authored_standalone_package_id(
        records_id
    ));
    assert_eq!(records_count, appended);

    // The item row names a definition in the records' package, and the investment loading index
    // names every record, so the client registers the package.
    let manager = open_manager(loader.path()).unwrap();
    let loader_definition = definition(&manager, weapon.item_hash);
    assert_eq!(loader_definition.pkg_id(), records_id);
    assert_eq!(
        usize::from(loader_definition.entry_index()),
        usize::from(earlier_definition.entry_index()) - stock_entries
    );
    let enrolled = crate::shared_tag_dependency_index::dependency_entries(
        &manager.read_tag(RUNTIME_DEPENDENCY_COMPANION).unwrap(),
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
    )
    .unwrap();
    for index in 0..u16::try_from(records_count).unwrap() {
        let record = TagHash::new(records_id, index);
        assert!(
            manager.read_tag(record).is_ok(),
            "record {record} reads back"
        );
        assert!(
            enrolled.contains(&(records_id, index)),
            "record {record} is in the investment loading index"
        );
    }
    drop(manager);

    // Sundial reads the authored item and its private perk from the same packages.
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &cache_path, true, |_| {}).unwrap();
    let authored = catalog.weapon_donor(weapon.item_hash).unwrap();
    assert_eq!(authored.summary.name, "Mod Loader Layout");
    let [plug] = weapon.custom_plugs.as_slice() else {
        panic!("one private plug was built");
    };
    assert_eq!(
        authored.sockets[socket.index].native_default,
        Some(plug.item_hash)
    );
    let plug_name = catalog.plug_label(plug.item_hash, false);
    assert_eq!(plug_name, "Loader Spec");

    if let Some(output) = crate::test_support::artifacts("mod-loader-layout") {
        fs::create_dir_all(&output).unwrap();
        for artifact in &loader_build.artifacts {
            fs::copy(
                loader_build.run_directory.join(&artifact.file_name),
                output.join(&artifact.file_name),
            )
            .unwrap();
        }
    }
    let files = |build: &crate::BuildReport, layout: &[Staged]| {
        layout
            .iter()
            .map(|(name, id, entries, stock)| {
                let digest = crate::artifact::digest_file(&build.run_directory.join(name)).unwrap();
                serde_json::json!({"file": name, "package_id": format!("{id:04X}"),
                    "entries": entries, "stock_entries": stock, "sha256": digest.sha256})
            })
            .collect::<Vec<_>>()
    };
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    crate::test_support::artifact(
        "mod-loader-layout.json",
        &serde_json::json!({
            "revision": revision,
            "stock_packages": packages,
            "runtime_sha256": crate::artifact::digest_file(&runtime).unwrap().sha256,
            "item": format!("0x{:08X}", weapon.item_hash),
            "private_plug": format!("0x{:08X}", plug.item_hash),
            "earlier": {"definition": earlier_definition.to_string(), "appended_records": appended,
                "packages": files(&earlier_build, &earlier_layout)},
            "loader": {"definition": loader_definition.to_string(),
                "records_package": format!("{records_id:04X}"), "records": records_count,
                "enrolled": true, "packages": files(&loader_build, &loader_layout)},
            "catalog": {"item": authored.summary.name, "private_plug": plug_name},
            "installed": false,
            "limits": "Offline package readback. Whether Sunrise accepts the mod and the client loads the records in game is unverified."
        }),
    );
    // A view keeps files it does not recognize, so the copied runtime leaves before it closes.
    fs::remove_file(install.join("bin/x64/steam_api64.dll")).unwrap();
}
