//! Configured hosting on Sunrise 0.6. When an importer's items add rows to a table Sunrise can't
//! merge that another member also changes, the main mod's build writes the importer's rows there
//! and the importer keeps its items, their assets and its badge. The scenario holds a main mod
//! recipe and an importer recipe that both add crosshair rows, another recipe of that importer and
//! a recipe of another importer.
//!
//! Before implementation, the failure inventory covered: the importer or one of its items built
//! into the main mod, the importer still shipping the shared table so the later mod's copy drops
//! the other's rows, the main mod's table missing the importer's rows, the importer loading
//! without the main mod it needs, the main mod needing another member, removing the importer's
//! folder taking the main mod's items or rows naming the main mod's assets with it, the build
//! status silent about what Parhelion holds, and an unchanged rebuild rewriting any folder.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipes {
    recipes: Vec<PathBuf>,
}

/// Every row of the crosshair table the loader serves, with whether a loaded package holds the
/// pair it names.
fn crosshair_rows(packages: &Path) -> Vec<(crate::weapon::crosshair::Row, bool)> {
    let manager = sundial::package_authoring::open_shadowkeep_package_manager(packages).unwrap();
    let table = crate::weapon::crosshair::Table::parse(
        &manager.read_tag(crate::weapon::crosshair::TABLE).unwrap(),
    )
    .unwrap();
    table
        .rows()
        .iter()
        .map(|row| (row.clone(), manager.read_tag(TagHash(row.pair)).is_ok()))
        .collect()
}

/// Whether the Collections paths `identity` records pass through a node named `name`.
fn shown_under(identity: &serde_json::Value, name: &str) -> bool {
    identity["collections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_array().unwrap().iter().any(|node| node == name))
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNRISE_MOD_LOADER_DLL, SUNDIAL_COUPLED_MOD_SCENARIO and SUNDIAL_TEST_ARTIFACTS"]
#[allow(
    clippy::cognitive_complexity,
    reason = "One ordered hosting lifecycle with independent readback"
)]
fn parhelion_holds_shared_rows_and_the_source_keeps_its_items() {
    let scenario_path = PathBuf::from(
        std::env::var_os("SUNDIAL_COUPLED_MOD_SCENARIO")
            .expect("SUNDIAL_COUPLED_MOD_SCENARIO names the library recipes that share a table"),
    );
    let scenario: Recipes = serde_json::from_slice(&fs::read(&scenario_path).unwrap()).unwrap();
    let recipes = scenario
        .recipes
        .iter()
        .map(|path| crate::WeaponRecipe::load_json(path).unwrap())
        .collect::<Vec<_>>();
    let output = crate::test_support::artifact_dir("importer-mod-hosting");
    fs::create_dir_all(&output).unwrap();
    let installation = installation("importer-mod-hosting-");
    let root = installation.packages.parent().unwrap().to_path_buf();
    let cache = output.join("catalogs");
    fs::create_dir_all(&cache).unwrap();
    let backups = output.join("backups");
    let build = |name: &str| {
        let request = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: installation.packages.clone(),
            staging_root: output.join(name),
            ignore_installed_authored_overlays: true,
            recipes: recipes.clone(),
            maps: Vec::new(),
        })
        .unwrap();
        crate::build_and_stage_snapshot_with_progress(&request, |_| {}).unwrap()
    };
    let shared_by = |build: &crate::BuildReport| {
        build
            .mods
            .iter()
            .filter(|report| !report.shared.is_empty())
            .map(|report| (report.id.clone(), report.shared.clone()))
            .collect::<BTreeMap<_, _>>()
    };

    let first = build("first");
    let staged = manifests(&first);
    let staged_mods = staged["mods"].as_array().unwrap();
    assert!(
        first
            .mods
            .iter()
            .all(|report| report.status != crate::ModStatus::InParhelion),
        "a source builds into Parhelion: {:?}",
        first.mods
    );
    let shared = shared_by(&first);
    assert!(
        shared
            .values()
            .any(|tables| tables.iter().any(|table| table == "the crosshair table")),
        "Parhelion holds no source's crosshair rows: {:?}",
        first.mods
    );
    let dependencies = |id: &str| {
        staged_mods
            .iter()
            .find(|member| member["manifest"]["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not staged"))["manifest"]["dependencies"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|dependency| dependency["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(
        dependencies(mod_content::ID).is_empty(),
        "the main mod needs another member to load"
    );
    for id in shared.keys() {
        let report = first.mods.iter().find(|report| report.id == *id).unwrap();
        assert!(
            report
                .note
                .as_ref()
                .is_some_and(|note| note.contains("needs Parhelion to load")),
            "{id} doesn't say it needs Parhelion: {:?}",
            report.note
        );
        assert_eq!(
            dependencies(id),
            vec![mod_content::ID.to_owned()],
            "{id} loads without the main mod that holds its rows"
        );
    }
    install(&first, &installation.packages, &backups);
    let owned = items(&recipes, &first);
    let packages = packages_of(&root);
    let account_installed = account(&installation.account);
    let (expected, state) = observe(&installation, &cache, "on", &owned, &packages, None);
    let rows = crosshair_rows(&installation.packages);
    assert!(
        rows.iter().all(|(_, resolves)| *resolves),
        "a crosshair row names a pair no package holds"
    );
    let mut states = vec![json!({"observed": state, "crosshair_rows": rows.len()})];
    // Each source's items stay in its own badge.
    for (id, hashes) in &owned {
        let Some(importer) = mod_content::Importer::from_id(id) else {
            continue;
        };
        assert!(
            hashes
                .iter()
                .any(|hash| shown_under(&expected[hash], importer.name())),
            "no item of {id} shows in the {} badge",
            importer.name()
        );
    }

    // Removing a source's folder keeps the main mod and the other sources whole. Parhelion still
    // serves the removed source's crosshair rows, which name scenes no loaded package holds and
    // which no loaded item names.
    let independent = staged_mods
        .iter()
        .filter_map(|member| member["manifest"]["id"].as_str())
        .filter(|id| *id != mod_content::ID)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for id in &independent {
        let name = format!("without-{id}");
        turn_off(&root, id);
        let (_, state) = observe(
            &installation,
            &cache,
            &name,
            &owned,
            &packages,
            Some(&expected),
        );
        let rows = crosshair_rows(&installation.packages);
        let off = &packages[id];
        let orphaned = rows
            .iter()
            .filter(|(row, _)| off.contains(&TagHash(row.pair).pkg_id()))
            .collect::<Vec<_>>();
        assert!(
            rows.iter()
                .filter(|(row, _)| !off.contains(&TagHash(row.pair).pkg_id()))
                .all(|(_, resolves)| *resolves),
            "{name}: a crosshair row of a loaded member names a pair no package holds"
        );
        assert!(
            orphaned.iter().all(|(_, resolves)| !*resolves),
            "{name}: a pair of the removed member still resolves"
        );
        if shared
            .get(id)
            .is_some_and(|tables| tables.iter().any(|table| table == "the crosshair table"))
        {
            assert!(
                !orphaned.is_empty(),
                "{name}: Parhelion's table lost the removed member's rows, so the member shipped them"
            );
        }
        states.push(json!({
            "observed": state,
            "crosshair_rows": rows.len(),
            "orphaned_rows": orphaned.len(),
        }));
        turn_on(&root, id);
    }
    assert_eq!(account(&installation.account), account_installed);

    // The next build holds the same rows in Parhelion and writes nothing.
    let before = written(&root);
    let again = build("unchanged");
    assert_eq!(shared_by(&again), shared);
    assert_only_rebuilt(&again, "");
    assert!(again.import_conversions.values().all(|count| *count == 0));
    install(&again, &installation.packages, &backups);
    assert_eq!(
        written(&root),
        before,
        "an unchanged build rewrote a folder"
    );
    let (_, state) = observe(
        &installation,
        &cache,
        "unchanged",
        &owned,
        &packages_of(&root),
        Some(&expected),
    );
    states.push(json!({"observed": state}));

    let source_revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|result| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        .unwrap();
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "source_revision": source_revision,
            "executable": crate::artifact::digest_file(&std::env::current_exe().unwrap())
                .unwrap()
                .sha256,
            "inputs": scenario_path,
            "mods": first.mods,
            "first": staged,
            "states": states,
            "limits": [
                "Offline loader port and catalog only",
                "No Sunrise loader log or Mods page capture",
                "Orphaned rows not loaded in the game",
                "No weapon, inspect or HUD gameplay",
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}
