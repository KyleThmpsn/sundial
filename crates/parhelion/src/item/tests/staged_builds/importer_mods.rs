//! Configured native-content lifecycle. Inputs are original library recipe paths, never a
//! copied recipe tree. Output packages are installed only into a temporary stock view.
//!
//! Before implementation, the failure inventory covered mixed ownership, shifted tags, stale
//! input pins and converter stamps, conversion on a hit, rewritten kept files, missing
//! dependencies, partial family recovery, foreign-mod interference and legacy layout drift.
use super::*;
use serde::Deserialize;
use serde_json::json;
use std::time::Instant;
use sundial::package_authoring::mod_content;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    /// At least one native item and items from at least two importers.
    recipes: Vec<PathBuf>,
    /// Another library item from one of those importers.
    added_recipe: PathBuf,
    #[serde(default)]
    maps: Vec<PathBuf>,
    #[serde(default)]
    legacy: Vec<LegacyBaseline>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBaseline {
    runtime: PathBuf,
    manifest: PathBuf,
}

fn origin(recipe: &crate::WeaponRecipe) -> Option<String> {
    let reference = recipe.overrides.imported_graph.as_ref()?;
    let graph: serde_json::Value =
        serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json")).unwrap())
            .unwrap();
    Some(
        graph["source"]
            .as_str()
            .expect("imported graph source")
            .to_owned(),
    )
}

fn manifests(build: &crate::BuildReport) -> serde_json::Value {
    serde_json::from_slice(&fs::read(&build.manifest_path).unwrap()).unwrap()
}

fn importer_files(
    root: &Path,
) -> BTreeMap<String, (same_file::Handle, std::time::SystemTime, String)> {
    mod_content::files(root)
        .unwrap()
        .into_iter()
        .filter(|path| {
            path.components().any(|part| {
                part.as_os_str()
                    .to_str()
                    .is_some_and(|name| name.starts_with("dev.kylethmpsn.parhelion-"))
            })
        })
        .map(|path| {
            let key = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let metadata = fs::metadata(&path).unwrap();
            (
                key,
                (
                    same_file::Handle::from_path(&path).unwrap(),
                    metadata.modified().unwrap(),
                    crate::artifact::digest_file(&path).unwrap().sha256,
                ),
            )
        })
        .collect()
}

fn account_tables(path: &Path) -> BTreeMap<String, serde_json::Value> {
    use rusqlite::types::ValueRef;
    let connection = rusqlite::Connection::open(path).unwrap();
    let names = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    names
        .into_iter()
        .map(|name| {
            let quoted = name.replace('"', "\"\"");
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{quoted}\""))
                .unwrap();
            let columns = statement
                .column_names()
                .iter()
                .map(|column| (*column).to_owned())
                .collect::<Vec<_>>();
            let width = columns.len();
            let mut rows = statement
                .query_map([], |row| {
                    (0..width)
                        .map(|index| {
                            Ok(match row.get_ref(index)? {
                                ValueRef::Null => serde_json::Value::Null,
                                ValueRef::Integer(value) => json!(value),
                                ValueRef::Real(value) => json!(value),
                                ValueRef::Text(value) => json!(String::from_utf8_lossy(value)),
                                ValueRef::Blob(value) => json!({"blob":hex::encode(value)}),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map(|row| json!(row))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows.sort_by_cached_key(|row| serde_json::to_string(row).unwrap());
            (name, json!({"columns":columns,"rows":rows}))
        })
        .collect()
}

fn install(
    build: &crate::BuildReport,
    packages: &Path,
    backups: &Path,
) -> (crate::InstallReport, f64) {
    let review = crate::install::preview_replacement(packages, &build.run_directory).unwrap();
    let mut request = crate::InstallRequest::new(
        build.run_directory.clone(),
        packages.to_path_buf(),
        backups.to_path_buf(),
    );
    request.confirmed_replacement = Some(review);
    // The installation is a new temporary fixture, with no executable game.
    request.game_running_check = || Ok(false);
    let started = Instant::now();
    let report = crate::install_staged_packages(&request).unwrap();
    (report, started.elapsed().as_secs_f64())
}

fn readback(packages: &Path, build: &crate::BuildReport) -> serde_json::Value {
    use tiger_pkg::{Package, PackageD2PreBL};
    let manager = sundial::package_authoring::open_shadowkeep_package_manager(packages).unwrap();
    let mut items = Vec::new();
    for weapon in &build.weapons {
        let bytes = manager
            .read_tag(TagHash(weapon.item_definition_hash))
            .unwrap();
        assert!(!bytes.is_empty());
        items.push(
            json!({"item":weapon.item_hash,"definition":weapon.item_definition_hash,
            "sha256":crate::artifact::digest_bytes(&bytes).sha256}),
        );
    }
    let root = packages.parent().unwrap();
    let mut decoded = Vec::new();
    for path in mod_content::files(root)
        .unwrap()
        .into_iter()
        .filter(|path| {
            path.to_string_lossy().contains("parhelion-")
                && path.extension().is_some_and(|extension| extension == "pkg")
        })
    {
        let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
        // Filename and file header are checked by the loader port. Decode every added entry
        // independently, then read the same address through Sundial's mod-aware manager.
        let parsed = tiger_pkg::manager::PackagePath::parse(path.to_str().unwrap()).unwrap();
        let id = u16::from_str_radix(&parsed.id, 16).unwrap();
        let mut entries = Vec::new();
        for index in 0..package.entries().len() {
            let address = TagHash::new(id, index.try_into().unwrap());
            let payload = package.read_entry(index).unwrap();
            assert_eq!(manager.read_tag(address).unwrap(), payload);
            entries.push(json!({
                "tag":address.0,
                "reference":package.entries()[index].reference,
                "sha256":crate::artifact::digest_bytes(&payload).sha256,
            }));
        }
        assert!(!entries.is_empty());
        decoded.push(json!({"file":path.file_name().unwrap(),"entries":entries}));
    }
    assert!(
        !decoded.is_empty(),
        "the installed importer corpus is empty"
    );
    let notices = mod_content::notices(root).unwrap().unwrap_or_default();
    assert!(
        !notices.iter().any(|notice| notice.contains("was left out")),
        "{notices:?}"
    );
    json!({"items":items,"packages":decoded,"loader_notices":notices})
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNRISE_MOD_LOADER_DLL, SUNDIAL_IMPORTER_MOD_SCENARIO and SUNDIAL_TEST_ARTIFACTS"]
#[allow(
    clippy::cognitive_complexity,
    reason = "One ordered native-content lifecycle with independent readback"
)]
fn importer_mods_reuse_changed_content_and_retire_as_one_generation() {
    let scenario_path = PathBuf::from(
        std::env::var_os("SUNDIAL_IMPORTER_MOD_SCENARIO")
            .expect("SUNDIAL_IMPORTER_MOD_SCENARIO names the original library recipe selection"),
    );
    let scenario: Scenario = serde_json::from_slice(&fs::read(&scenario_path).unwrap()).unwrap();
    let mut recipes = scenario
        .recipes
        .iter()
        .map(|path| crate::WeaponRecipe::load_json(path).unwrap())
        .collect::<Vec<_>>();
    let legacy_recipes = recipes.clone();
    assert!(
        recipes.iter().any(|recipe| origin(recipe).is_none()),
        "native content is required"
    );
    let origins = recipes.iter().filter_map(origin).collect::<BTreeSet<_>>();
    assert!(
        origins.len() >= 2,
        "at least two importer origins are required"
    );
    let added = crate::WeaponRecipe::load_json(&scenario.added_recipe).unwrap();
    let changed_origin = origin(&added).expect("the added recipe is imported");
    assert!(origins.contains(&changed_origin));
    assert!(
        !recipes
            .iter()
            .any(|recipe| recipe.identity.item_hash == added.identity.item_hash)
    );
    let maps = scenario
        .maps
        .iter()
        .map(|path| crate::map::load_recipe(path).unwrap())
        .collect::<Vec<_>>();
    let output = crate::test_support::artifact_dir("importer-mod-family");
    fs::create_dir_all(&output).unwrap();
    let source = crate::test_support::stock_packages();
    let (_view, packages) = stock_view(&source, "importer-mod-family-", Oodle::Required);
    let root = packages.parent().unwrap();
    fs::write(root.join("destiny2.exe"), []).unwrap();
    fs::create_dir_all(root.join("bin/x64/Sunrise")).unwrap();
    fs::copy(
        crate::test_support::mod_loader_runtime(),
        root.join("bin/x64/steam_api64.dll"),
    )
    .unwrap();
    fs::write(
        root.join("bin/x64/Sunrise/settings.json"),
        br#"{"version":18,"unknown":{"keep":true},"client":{"reveal_lore_books":false}}"#,
    )
    .unwrap();
    let data = root.join("bin/x64/Sunrise/data");
    fs::create_dir(&data).unwrap();
    let account_path = data.join("investment.sqlite3");
    let account = rusqlite::Connection::open(&account_path).unwrap();
    account.execute_batch("BEGIN").unwrap();
    for sql in [
        include_str!(
            "../../../../../../src/persistence/sqlite_account/fixtures/investment_schema.sql"
        ),
        include_str!(
            "../../../../../../src/persistence/sqlite_account/fixtures/account_settings_schema.sql"
        ),
        include_str!(
            "../../../../../../src/persistence/sqlite_account/fixtures/investment_defaults.sql"
        ),
        include_str!(
            "../../../../../../src/persistence/sqlite_account/fixtures/account_settings_defaults.sql"
        ),
    ] {
        account.execute_batch(sql).unwrap();
    }
    account.execute_batch("COMMIT").unwrap();
    drop(account);
    let account_before = account_tables(&account_path);
    let settings = fs::read(root.join("bin/x64/Sunrise/settings.json")).unwrap();
    let mut events = Vec::new();
    let mut run =
        |recipes: Vec<crate::WeaponRecipe>, maps: Vec<crate::map::MapRecipe>, name: &str| {
            let request = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
                package_directory: packages.clone(),
                staging_root: output.join(name),
                ignore_installed_authored_overlays: true,
                recipes,
                maps,
            })
            .unwrap();
            let started = Instant::now();
            let result = crate::build_and_stage_snapshot_with_progress(&request, |progress| {
                events.push(json!({
                    "run":name,"seconds":started.elapsed().as_secs_f64(),
                    "phase":format!("{:?}",progress.phase),"label":progress.current_artifact,
                    "activity":format!("{:?}",progress.activity),
                }));
            });
            fs::write(
                output.join(format!("{name}-events.json")),
                serde_json::to_vec_pretty(&events).unwrap(),
            )
            .unwrap();
            let build = result.unwrap();
            eprintln!("{name} stage: {:.3}s", started.elapsed().as_secs_f64());
            (build, started.elapsed().as_secs_f64())
        };
    let (first, first_seconds) = run(recipes.clone(), maps.clone(), "first");
    let staged = manifests(&first);
    let mods = staged["mods"].as_array().expect("split mod manifests");
    assert_eq!(mods.len(), origins.len() + 1);
    for member in mods {
        let id = member["manifest"]["id"].as_str().unwrap();
        let files = member["receipt"]["artifacts"].as_array().unwrap();
        if id == mod_content::ID {
            assert_eq!(
                member["manifest"]["dependencies"].as_array().unwrap().len(),
                origins.len()
            );
            assert!(files.iter().any(|file| {
                file["path"]
                    .as_str()
                    .unwrap()
                    .contains("parhelion_investment")
            }));
        } else {
            assert!(
                files
                    .iter()
                    .all(|file| file["path"].as_str().unwrap().starts_with("packages/"))
            );
            assert!(files.iter().all(|file| {
                !file["path"]
                    .as_str()
                    .unwrap()
                    .contains("parhelion_investment")
            }));
        }
    }
    let (_, first_install_seconds) = install(&first, &packages, &output.join("backups"));
    let first_readback = readback(&packages, &first);
    let mut before = importer_files(root);
    let (second, second_seconds) = run(recipes.clone(), maps.clone(), "unchanged");
    let repeated = manifests(&second);
    for member in repeated["mods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|member| member["manifest"]["id"] != mod_content::ID)
    {
        assert_eq!(member["kept"], true);
        for artifact in member["receipt"]["artifacts"].as_array().unwrap() {
            let name = Path::new(artifact["path"].as_str().unwrap())
                .file_name()
                .unwrap();
            assert!(
                !second.run_directory.join(name).exists(),
                "kept import was rewritten"
            );
        }
    }
    assert!(second.import_conversions.values().all(|count| *count == 0));
    let (_, second_install_seconds) = install(&second, &packages, &output.join("backups"));
    assert_eq!(
        before,
        importer_files(root),
        "kept files preserve identities, times and digests"
    );
    let second_readback = readback(&packages, &second);
    // Release identity handles before replacing this importer on Windows.
    before.retain(|path, _| !path.contains(&format!("parhelion-{changed_origin}/")));
    recipes.push(added);
    let (changed, changed_seconds) = run(recipes.clone(), maps.clone(), "changed");
    let changed_manifest = manifests(&changed);
    for member in changed_manifest["mods"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|member| member["manifest"]["id"] != mod_content::ID)
    {
        let id = member["manifest"]["id"].as_str().unwrap();
        assert_eq!(
            member["kept"].as_bool().unwrap(),
            !id.ends_with(&changed_origin)
        );
    }
    install(&changed, &packages, &output.join("backups"));
    for (path, identity) in &before {
        if !path.contains(&format!("parhelion-{changed_origin}/")) {
            assert_eq!(importer_files(root).get(path), Some(identity));
        }
    }
    let removed_origin = origins
        .iter()
        .find(|origin| **origin != "halo_reach" || maps.is_empty())
        .expect("an importer without a selected map");
    recipes.retain(|recipe| origin(recipe).as_ref() != Some(removed_origin));
    let removed_id = format!("{}-{removed_origin}", mod_content::ID);
    before.retain(|path, _| !path.contains(&format!("parhelion-{removed_origin}/")));
    let (removed, _) = run(recipes, maps, "retired");
    let (retired, _) = install(&removed, &packages, &output.join("backups"));
    assert!(
        !mod_content::runtime_directory(root)
            .join("mods")
            .join(&removed_id)
            .exists()
    );
    assert!(
        retired
            .backup_directory
            .join("mods")
            .join(&removed_id)
            .is_dir()
    );
    let last_readback = readback(&packages, &removed);
    assert_eq!(
        fs::read(root.join("bin/x64/Sunrise/settings.json")).unwrap(),
        settings
    );
    let account_after = account_tables(&account_path);
    let mut expected_account = account_before.clone();
    let unlocks = expected_account.get_mut("unlocks").unwrap()["rows"]
        .as_array_mut()
        .unwrap();
    for weapon in manifests(&removed)["project"]["weapons"]
        .as_array()
        .unwrap()
    {
        if !weapon["unlock"].is_null() {
            // Native account-flag bank 1 persists in SQLite bank 0, profile slot -1.
            // Installed Collections flags use lane 0 and enabled value 2.
            assert_eq!(weapon["unlock"]["bank"], 1);
            let row = json!([-1, 0, weapon["unlock"]["slot"], 0, 2]);
            assert!(
                !unlocks.contains(&row),
                "fixture already owns the authored flag"
            );
            unlocks.push(row);
        }
    }
    unlocks.sort_by_cached_key(|row| serde_json::to_string(row).unwrap());
    assert_eq!(account_after, expected_account);
    let mut legacy_readback = Vec::new();
    for (index, baseline) in scenario.legacy.iter().enumerate() {
        fs::copy(&baseline.runtime, root.join("bin/x64/steam_api64.dll")).unwrap();
        let (legacy, _) = run(
            legacy_recipes.clone(),
            Vec::new(),
            &format!("legacy-{index}"),
        );
        let expected: serde_json::Value =
            serde_json::from_slice(&fs::read(&baseline.manifest).unwrap()).unwrap();
        let actual = manifests(&legacy);
        let inventory = |manifest: &serde_json::Value| {
            manifest["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|artifact| {
                    (
                        artifact["file_name"].as_str().unwrap().to_owned(),
                        (
                            artifact["byte_length"].as_u64().unwrap(),
                            artifact["sha256"].as_str().unwrap().to_owned(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        let before = inventory(&expected);
        assert!(!before.is_empty(), "the legacy baseline corpus is empty");
        assert_eq!(before, inventory(&actual), "legacy package bytes changed");
        assert!(actual.get("mods").is_none());
        assert!(legacy.mods.is_empty());
        assert!(legacy.import_conversions.is_empty());
        legacy_readback.push(json!({
            "runtime":baseline.runtime,"baseline":baseline.manifest,
            "artifacts":actual["artifacts"],"byte_identical":true,
        }));
    }
    let source_revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        .unwrap();
    fs::write(output.join("receipt.json"), serde_json::to_vec_pretty(&json!({
        "source_revision":source_revision,
        "executable":crate::artifact::digest_file(&std::env::current_exe().unwrap()).unwrap().sha256,
        "inputs":scenario_path,"first":staged,"unchanged":repeated,"changed":changed_manifest,
        "retired":manifests(&removed),
        "readback":[first_readback,second_readback,last_readback],
        "account_readback":{"before":account_before,"after":account_after},
        "legacy_readback":legacy_readback,"events":events,
        "timing_seconds":{"first_stage":first_seconds,"unchanged_stage":second_seconds,
            "changed_stage":changed_seconds,"first_install":first_install_seconds,
            "unchanged_install":second_install_seconds},
        "limits":["Offline loader port only","No loader log or Mods page capture","No weapon or map gameplay"]
    })).unwrap()).unwrap();
}
