use super::*;

#[test]
fn package_probe_uses_only_package_files() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("directory.pkg")).unwrap();
    fs::write(directory.path().join("notes.txt"), b"not a package").unwrap();
    assert!(
        first_package_file(directory.path())
            .unwrap_err()
            .contains("no .pkg files")
    );
    let package = directory.path().join("actual.PKG");
    fs::write(&package, b"probe source").unwrap();
    assert_eq!(first_package_file(directory.path()).unwrap(), package);
}

#[test]
fn package_probe_preserves_directory_errors() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing");
    let error = first_package_file(&missing).unwrap_err();
    assert!(error.contains("Could not list"));
    assert!(error.contains(&missing.display().to_string()));
}

#[test]
fn artifact_validation_progress_tracks_each_real_file() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.pkg");
    let second = directory.path().join("second.pkg");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();
    let mut events = Vec::new();

    let reports = artifact_reports_with_progress(&[first, second], &mut |event| {
        events.push(event);
    })
    .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].phase, BuildPhase::ValidatingPackages);
    assert_eq!(events[0].current_artifact.as_deref(), Some("first.pkg"));
    assert_eq!((events[0].completed, events[0].total), (0, 2));
    assert_eq!((events[1].completed, events[1].total), (1, 2));
    assert_eq!(events[2].current_artifact.as_deref(), Some("second.pkg"));
    assert_eq!((events[3].completed, events[3].total), (2, 2));
}

fn package(path: &Path, package_id: u16, patch: u16, signature: u64) {
    let bytes = PackageHeaderPrefix {
        version: SHADOWKEEP_HEADER_VERSION,
        package_id,
        build_signature: signature,
        patch_id: patch,
    }
    .encode();
    fs::write(path, bytes).expect("test package header should be written");
}

fn stock_source(root: &Path) -> PathBuf {
    let packages = root.join("packages");
    fs::create_dir(&packages).expect("package directory should be created");
    for profile in CANONICAL_PACKAGES {
        package(
            &packages.join(profile.stock_file_name(profile.stock_patch_id)),
            profile.package_id,
            profile.stock_patch_id,
            0xF6F1_7D76_7F79_5A39,
        );
    }
    packages
}

fn snapshot(package_directory: PathBuf, staging_root: PathBuf) -> BatchBuildSnapshot {
    snapshot_with_recipe(package_directory, staging_root, WeaponRecipe::every_end())
}

fn snapshot_with_recipe(
    package_directory: PathBuf,
    staging_root: PathBuf,
    recipe: WeaponRecipe,
) -> BatchBuildSnapshot {
    BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory,
        staging_root,
        ignore_installed_authored_overlays: true,
        recipes: vec![recipe],
    })
    .expect("single-recipe test snapshot should be valid")
}

fn inspect_request(snapshot: &BatchBuildSnapshot) -> Result<SourceInspection, String> {
    inspect_snapshot(snapshot)
}

fn configured_real_packages() -> Option<PathBuf> {
    std::env::var_os("SUNDIAL_TEST_PACKAGES")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

#[test]
fn source_inspection_accepts_the_stock_profile() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let request = snapshot(packages, directory.path().join("staging"));

    let report = inspect_request(&request).expect("stock package profile should pass");

    assert!(report.ignored_authored_files.is_empty());
}

#[test]
fn source_inspection_recognizes_a_complete_installed_authored_set() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    for profile in CANONICAL_PACKAGES {
        package(
            &packages.join(profile.authored_file_name),
            profile.package_id,
            profile.authored_patch_id,
            SUNDIAL_BUILD_SIGNATURE,
        );
    }
    let asset = authored_package(PARHELION_ASSET_PACKAGE_ID)
        .expect("the canonical asset package profile must exist");
    package(
        &packages.join(asset.file_name),
        asset.package_id,
        asset.patch_id,
        SUNDIAL_BUILD_SIGNATURE,
    );
    let request = snapshot(packages, directory.path().join("staging"));

    let report = inspect_request(&request).expect("complete authored set should be ignored");

    let mut expected = CANONICAL_ARTIFACT_FILE_NAMES.map(str::to_owned);
    expected.sort();
    assert_eq!(report.ignored_authored_files, expected);
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn semantic_preflight_accepts_a_valid_project_without_creating_a_staging_run() {
    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let staging = directory.path().join("staging");
    let request = snapshot(packages, staging.clone());

    preflight_snapshot(&request).expect("valid project should compile during preflight");
    assert!(
        !staging.exists(),
        "preflight must not create staging output"
    );
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES, PARHELION_TEST_RECIPE_DIRECTORY, and PARHELION_TEST_STAGING_ROOT"]
fn configured_recipe_directory_builds_and_stages_the_normal_batch_workflow() {
    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let recipe_directory = std::env::var_os("PARHELION_TEST_RECIPE_DIRECTORY")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_RECIPE_DIRECTORY must point to a recipe directory");
    let staging_root = std::env::var_os("PARHELION_TEST_STAGING_ROOT")
        .map(PathBuf::from)
        .expect("PARHELION_TEST_STAGING_ROOT must be configured");
    let mut recipe_paths = fs::read_dir(&recipe_directory)
        .expect("configured recipe directory should be readable")
        .map(|entry| {
            entry
                .expect("recipe directory entry should be readable")
                .path()
        })
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".parhelion.json"))
        })
        .collect::<Vec<_>>();
    recipe_paths.sort();
    let recipes = recipe_paths
        .iter()
        .map(|path| {
            WeaponRecipe::load_json(path)
                .unwrap_or_else(|error| panic!("{} should load: {error}", path.display()))
        })
        .collect::<Vec<_>>();
    assert!(!recipes.is_empty(), "configured recipe directory is empty");
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: packages,
        staging_root,
        ignore_installed_authored_overlays: true,
        recipes,
    })
    .expect("configured recipe batch should snapshot");

    let report = build_and_stage_snapshot(&snapshot)
        .expect("configured recipe batch should build through the normal workflow");

    assert_eq!(report.weapons.len(), snapshot.request.recipes.len());
    authored_packages_for_file_names(
        report
            .artifacts
            .iter()
            .map(|artifact| artifact.file_name.as_str()),
    )
    .expect("configured build should emit a valid recipe-selected artifact set");
    assert!(report.manifest_path.is_file());
    println!("PARHELION_STAGED_RUN={}", report.run_directory.display());
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES and PARHELION_TEST_STAGING_ROOT"]
fn configured_additional_weapon_family_matrix_builds_and_stages() {
    use crate::recipe::{RecipeDamageType, RecipeInventorySlot};
    use crate::{
        HexHash, RecipeRarity, WeaponDonorReference, WeaponIconEdit, WeaponSocketColumnRecipe,
        WeaponStatOverride,
    };

    struct Case {
        name: &'static str,
        donor_hash: u32,
        donor_name: &'static str,
        presentation_hash: u32,
        presentation_name: &'static str,
        inventory_slot: Option<RecipeInventorySlot>,
        damage_type: Option<RecipeDamageType>,
        rarity: RecipeRarity,
        edit_icon: bool,
    }

    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let staging_root = std::env::var_os("PARHELION_TEST_STAGING_ROOT")
        .map(PathBuf::from)
        .expect("PARHELION_TEST_STAGING_ROOT must be configured");
    let install = packages
        .parent()
        .expect("configured packages need an install root");
    let catalog = sundial::investment::InvestmentCatalog::load(install, false, |_| {})
        .expect("configured clean-stock catalog should load");
    let cases = [
        Case {
            name: "Matrix Hand Cannon",
            donor_hash: 0x53D5_1E72,
            donor_name: "Agamid",
            presentation_hash: 0x26F9_5A03,
            presentation_name: "Allegro-34",
            inventory_slot: Some(RecipeInventorySlot::Energy),
            damage_type: Some(RecipeDamageType::Solar),
            rarity: RecipeRarity::Rare,
            edit_icon: false,
        },
        Case {
            name: "Matrix Machine Gun",
            donor_hash: 0xC63B_5A50,
            donor_name: "A Fine Memorial",
            presentation_hash: 0x23F4_BF01,
            presentation_name: "Hammerhead",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Void),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Rocket Launcher",
            donor_hash: 0x97B2_E5DE,
            donor_name: "Apex Predator",
            presentation_hash: 0x3B16_442C,
            presentation_name: "Bad Omens",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Arc),
            rarity: RecipeRarity::Uncommon,
            edit_icon: false,
        },
        Case {
            name: "Matrix Shotgun",
            donor_hash: 0x25F6_83B0,
            donor_name: "Dust Rock Blues",
            presentation_hash: 0x4F1C_712E,
            presentation_name: "Badlander",
            inventory_slot: Some(RecipeInventorySlot::Energy),
            damage_type: Some(RecipeDamageType::Void),
            rarity: RecipeRarity::Common,
            edit_icon: false,
        },
        Case {
            name: "Matrix Fusion Rifle",
            donor_hash: 0xAEC2_1E34,
            donor_name: "Dream Breaker",
            presentation_hash: 0x9527_F0F4,
            presentation_name: "Cartesian Coordinate",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Arc),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Sniper Rifle",
            donor_hash: 0x8102_DDBD,
            donor_name: "Apostate",
            presentation_hash: 0xC56A_395C,
            presentation_name: "A Single Clap",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Solar),
            rarity: RecipeRarity::Legendary,
            edit_icon: true,
        },
        Case {
            name: "Matrix Sword",
            donor_hash: 0x61FF_E61D,
            donor_name: "Abide the Return",
            presentation_hash: 0x249F_67B4,
            presentation_name: "Falling Guillotine",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Arc),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Exotic Sword",
            donor_hash: 0xE079_4C51,
            donor_name: "Black Talon",
            presentation_hash: 0xE079_4C51,
            presentation_name: "Black Talon",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Arc),
            rarity: RecipeRarity::Exotic,
            edit_icon: false,
        },
        Case {
            name: "Matrix Scout Rifle",
            donor_hash: 0x7066_4F86,
            donor_name: "Call to Serve",
            presentation_hash: 0x1F56_4FF7,
            presentation_name: "Black Scorpion-4sr",
            inventory_slot: Some(RecipeInventorySlot::Energy),
            damage_type: Some(RecipeDamageType::Arc),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Submachine Gun",
            donor_hash: 0x7CDE_3A31,
            donor_name: "Adjudicator",
            presentation_hash: 0x7D84_5F1B,
            presentation_name: "Bad Reputation",
            inventory_slot: Some(RecipeInventorySlot::Energy),
            damage_type: Some(RecipeDamageType::Solar),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Combat Bow",
            donor_hash: 0xF422_6A09,
            donor_name: "Accrued Redemption",
            presentation_hash: 0x2AEF_B232,
            presentation_name: "No Turning Back",
            inventory_slot: None,
            damage_type: None,
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Pulse Rifle",
            donor_hash: 0xE516_CF40,
            donor_name: "Blast Furnace",
            presentation_hash: 0x4591_5B1E,
            presentation_name: "Adhortative",
            inventory_slot: Some(RecipeInventorySlot::Energy),
            damage_type: Some(RecipeDamageType::Solar),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Linear Fusion Rifle",
            donor_hash: 0x3869_9403,
            donor_name: "Line in the Sand",
            presentation_hash: 0xA0C1_DA62,
            presentation_name: "Komodo-4FR",
            inventory_slot: None,
            damage_type: Some(RecipeDamageType::Solar),
            rarity: RecipeRarity::Legendary,
            edit_icon: false,
        },
        Case {
            name: "Matrix Exotic Trace Rifle",
            donor_hash: 0x5038_4F33,
            donor_name: "Coldheart",
            presentation_hash: 0x5038_4F33,
            presentation_name: "Coldheart",
            inventory_slot: None,
            damage_type: None,
            rarity: RecipeRarity::Exotic,
            edit_icon: false,
        },
    ];

    let recipes = cases
        .into_iter()
        .map(|case| {
            let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
                case.name,
                case.donor_hash,
                case.donor_name,
            )
            .expect("matrix recipe identity should allocate");
            recipe.presentation_donor = Some(WeaponDonorReference {
                item_hash: HexHash::new(case.presentation_hash),
                expected_name: Some(case.presentation_name.to_owned()),
            });
            recipe.overrides.inventory_slot = case.inventory_slot;
            recipe.overrides.modern_damage_type = case.damage_type;
            recipe.overrides.power_cap_group = Some(11);
            recipe.overrides.rarity = Some(case.rarity);
            if case.edit_icon {
                recipe.overrides.icon_edit = WeaponIconEdit {
                    hue_shift_degrees: 24,
                    brightness: 6,
                    invert: false,
                    ..WeaponIconEdit::default()
                };
            }

            let donor = catalog
                .weapon_donor(case.donor_hash)
                .expect("matrix gameplay donor should decode");
            if let Some(stat) = donor.investment_stats.iter().find(|stat| {
                stat.minimum_value
                    .zip(stat.maximum_value)
                    .is_some_and(|(minimum, maximum)| minimum <= maximum)
            }) {
                let (minimum, maximum) = stat
                    .minimum_value
                    .zip(stat.maximum_value)
                    .expect("bounded stat should retain both bounds");
                recipe.overrides.investment_stats.push(WeaponStatOverride {
                    definition_index: stat.definition_index,
                    value: minimum + (maximum - minimum) / 2,
                });
            }

            let supported = catalog
                .weapon_supported_plug_sets(case.donor_hash)
                .expect("matrix donor compatible plugs should decode");
            recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
            if let Some(set) = supported.iter().find(|set| {
                donor
                    .sockets
                    .get(set.socket_index)
                    .is_some_and(|socket| socket.max_authored_choices >= 2)
                    && set.plug_hashes.iter().filter(|hash| **hash != 0).count() >= 2
            }) {
                recipe.overrides.socket_columns[set.socket_index] =
                    Some(WeaponSocketColumnRecipe {
                        choices: set
                            .plug_hashes
                            .iter()
                            .copied()
                            .filter(|hash| *hash != 0)
                            .take(2)
                            .map(HexHash::new)
                            .collect(),
                        ..WeaponSocketColumnRecipe::default()
                    });
            }
            recipe
                .validate()
                .unwrap_or_else(|error| panic!("{} recipe is invalid: {error}", case.name));
            recipe
        })
        .collect::<Vec<_>>();

    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: packages,
        staging_root,
        ignore_installed_authored_overlays: true,
        recipes,
    })
    .expect("additional family matrix should snapshot");
    let report = build_and_stage_snapshot(&snapshot)
        .expect("additional family matrix should build and stage");
    assert_eq!(report.weapons.len(), 14);
    authored_packages_for_file_names(
        report
            .artifacts
            .iter()
            .map(|artifact| artifact.file_name.as_str()),
    )
    .expect("family matrix should emit a valid recipe-selected artifact set");
    println!(
        "PARHELION_MATRIX_STAGED_RUN={}",
        report.run_directory.display()
    );
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES and PARHELION_TEST_STAGED_RUN"]
fn configured_staged_run_reopens_with_complete_stock_shaped_icon_graphs() {
    use tiger_pkg::{DestinyVersion, GameVersion, PackageManager, TagHash};

    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let staged_run = std::env::var_os("PARHELION_TEST_STAGED_RUN")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .expect("PARHELION_TEST_STAGED_RUN must point to a staged run");
    let ignored = CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let source = PackageSource::prepare(&packages, &ignored)
        .expect("temporary stock package view should be created");
    for name in CANONICAL_ARTIFACT_FILE_NAMES {
        let staged = staged_run.join(name);
        assert!(staged.is_file(), "{} is missing", staged.display());
        fs::hard_link(&staged, source.path().join(name))
            .expect("staged package should hard-link into the isolated package view");
    }

    let manager = PackageManager::new(
        source.path(),
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .expect("isolated authored package view should open");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(staged_run.join(MANIFEST_FILE_NAME))
            .expect("staged manifest should read"),
    )
    .expect("staged manifest should decode");
    let sunrise = &manifest["project"]["sunrise"];
    let parse_tag = |value: &serde_json::Value| {
        let encoded = value.as_str().expect("manifest tag should be a string");
        let raw = u32::from_str_radix(encoded.trim_start_matches("0x"), 16)
            .expect("manifest tag should be hexadecimal");
        TagHash(raw)
    };
    let mut definitions = sunrise["watermarked_icon_containers"]
        .as_array()
        .expect("manifest should list watermarked icon definitions")
        .iter()
        .map(&parse_tag)
        .collect::<BTreeSet<_>>();
    definitions.insert(parse_tag(&sunrise["badge_icon_tag"]));
    assert!(
        definitions.len() >= 2,
        "manifest contains no authored icon graph"
    );

    for definition in definitions {
        crate::watermark::validate_icon_definition_graph(&manager, definition)
            .unwrap_or_else(|error| panic!("authored icon {definition} is invalid: {error}"));
    }
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn semantic_preflight_authors_three_ordered_choices_in_one_gameplay_donor_column() {
    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let install = packages
        .parent()
        .expect("configured package directory should have an install root");
    let catalog = sundial::investment::InvestmentCatalog::load(install, false, |_| {})
        .expect("configured donor catalog should load");
    let barrel_choices = catalog
        .weapon_supported_plug_sets(crate::ARC_LOGIC_DONOR_HASH)
        .expect("Gameplay donor compatible plug sets should decode")
        .into_iter()
        .find(|set| set.socket_index == 1)
        .expect("Gameplay donor should have a barrel socket")
        .plug_hashes
        .into_iter()
        .take(3)
        .collect::<Vec<_>>();
    assert_eq!(barrel_choices.len(), 3);

    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let staging = directory.path().join("staging");
    let mut recipe = WeaponRecipe::every_end();
    recipe.overrides.socket_columns[1] = Some(crate::WeaponSocketColumnRecipe {
        choices: barrel_choices
            .into_iter()
            .map(crate::HexHash::new)
            .collect(),
        ..crate::WeaponSocketColumnRecipe::default()
    });
    let request = snapshot_with_recipe(packages, staging.clone(), recipe);

    preflight_snapshot(&request).expect("three-choice column should compile during preflight");
    assert!(
        !staging.exists(),
        "preflight must not create staging output"
    );
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn semantic_preflight_rejects_an_override_absent_from_the_donor() {
    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let staging = directory.path().join("staging");
    let mut recipe = WeaponRecipe::every_end();
    recipe
        .overrides
        .investment_stats
        .push(crate::WeaponStatOverride {
            definition_index: u16::MAX,
            value: 50,
        });
    let request = snapshot_with_recipe(packages, staging.clone(), recipe);

    let error = preflight_snapshot(&request).expect_err("donor-incompatible override must fail");

    assert!(
        error.contains("is not present in"),
        "unexpected error: {error}"
    );
    assert!(
        !staging.exists(),
        "failed preflight must not create staging output"
    );
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn semantic_preflight_allows_a_forced_plug_outside_the_donor_socket_pool() {
    let packages = configured_real_packages()
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let staging = directory.path().join("staging");
    let mut recipe = WeaponRecipe::every_end();
    recipe.overrides.socket_columns[0]
        .as_mut()
        .expect("Every End has an explicit first socket")
        .choices[0] = crate::HexHash::new(crate::ARC_LOGIC_DONOR_HASH);
    let request = snapshot_with_recipe(packages, staging.clone(), recipe);

    preflight_snapshot(&request).expect("an explicitly forced plug should pass preflight");
    assert!(
        !staging.exists(),
        "preflight must not create staging output"
    );
}

#[test]
fn source_inspection_ignores_a_recognized_partial_authored_set() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let profile = CANONICAL_PACKAGES[0];
    package(
        &packages.join(CANONICAL_ARTIFACT_FILE_NAMES[0]),
        profile.package_id,
        profile.authored_patch_id,
        SUNDIAL_BUILD_SIGNATURE,
    );
    let request = snapshot(packages, directory.path().join("staging"));

    let report =
        inspect_request(&request).expect("a recognized partial prior generation should be ignored");

    assert_eq!(
        report.ignored_authored_files,
        vec![CANONICAL_ARTIFACT_FILE_NAMES[0].to_owned()]
    );
}

#[test]
fn preflight_rejects_staging_inside_packages() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let request = snapshot(packages.clone(), packages.join("output"));

    let error = preflight_snapshot(&request).expect_err("live package output must fail");

    assert!(error.contains("outside the live packages directory"));
}

#[test]
fn preflight_rejects_a_normalized_nonexistent_staging_path_inside_packages() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let staging = packages
        .join("not-created")
        .join("..")
        .join("normalized-output");
    let request = snapshot(packages, staging);

    let error = preflight_snapshot(&request).expect_err("normalized live package output must fail");

    assert!(error.contains("outside the live packages directory"));
}

#[cfg(windows)]
#[test]
fn preflight_rejects_a_case_aliased_windows_staging_path_inside_packages() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let case_alias = PathBuf::from(packages.to_string_lossy().to_uppercase()).join("output");
    let request = snapshot(packages, case_alias);

    let error =
        preflight_snapshot(&request).expect_err("case-aliased live package output must fail");

    assert!(error.contains("outside the live packages directory"));
}

#[test]
fn unique_run_directories_never_reuse_an_existing_path() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let first = create_unique_run_directory(directory.path(), "custom-weapon")
        .expect("first run directory should be created");
    let second = create_unique_run_directory(directory.path(), "custom-weapon")
        .expect("second run directory should be created");

    assert_ne!(first, second);
    assert!(first.is_dir());
    assert!(second.is_dir());
    assert!(
        first
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("custom-weapon-"))
    );
}

#[test]
fn source_fingerprint_covers_sparse_stock_generations_and_detects_changes() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let packages = stock_source(directory.path());
    let sparse = packages.join(CANONICAL_PACKAGES[0].stock_file_name(1));
    package(&sparse, CANONICAL_PACKAGE_IDS[0], 1, 0xF6F1_7D76_7F79_5A39);

    let before = source_artifact_reports(&packages).unwrap();
    assert_eq!(before.len(), CANONICAL_PACKAGE_IDS.len() + 1);
    assert!(
        before
            .iter()
            .any(|artifact| artifact.file_name.ends_with("_1.pkg"))
    );

    let mut changed = fs::read(&sparse).unwrap();
    changed.extend_from_slice(b"changed after compilation started");
    fs::write(&sparse, changed).unwrap();
    let after = source_artifact_reports(&packages).unwrap();
    assert_ne!(before, after);
    assert!(
        validate_source_artifacts_unchanged(&before, &after)
            .unwrap_err()
            .contains("changed while")
    );
}

#[test]
fn batch_request_keeps_every_enabled_recipe_explicit() {
    let recipes = vec![
        WeaponRecipe::every_end(),
        WeaponRecipe::new_weapon("parhelion.second").unwrap(),
    ];
    let request = BatchBuildRequest {
        package_directory: PathBuf::from("packages"),
        staging_root: PathBuf::from("staging"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    };

    assert_eq!(request.recipes, recipes);
}

#[test]
fn batch_snapshot_is_sorted_stable_and_sensitive_to_dirty_edits() {
    let second = WeaponRecipe::second_sun().unwrap();
    let every = WeaponRecipe::every_end();
    let request = BatchBuildRequest {
        package_directory: PathBuf::from("packages"),
        staging_root: PathBuf::from("staging"),
        ignore_installed_authored_overlays: true,
        recipes: vec![second.clone(), every.clone()],
    };
    let snapshot = BatchBuildSnapshot::new(request.clone()).unwrap();
    let reversed = BatchBuildSnapshot::new(BatchBuildRequest {
        recipes: vec![every, second],
        ..request
    })
    .unwrap();
    let mut dirty_request = reversed.request.clone();
    dirty_request.recipes[0].flavor.push_str(" edited");
    let dirty = BatchBuildSnapshot::new(dirty_request).unwrap();

    assert_eq!(snapshot.fingerprint, reversed.fingerprint);
    assert_ne!(snapshot.fingerprint, dirty.fingerprint);
    assert!(
        snapshot
            .request
            .recipes
            .windows(2)
            .all(|pair| pair[0].namespace < pair[1].namespace)
    );
}

#[test]
fn project_spec_maps_every_frozen_recipe_in_snapshot_order() {
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: PathBuf::from("packages"),
        staging_root: PathBuf::from("staging"),
        ignore_installed_authored_overlays: true,
        recipes: vec![
            WeaponRecipe::second_sun().unwrap(),
            WeaponRecipe::every_end(),
        ],
    })
    .unwrap();

    let project = project_spec(&snapshot).unwrap();

    assert_eq!(project.weapons.len(), 2);
    assert!(
        project
            .weapons
            .windows(2)
            .all(|pair| pair[0].namespace < pair[1].namespace)
    );
    for (weapon, recipe) in project.weapons.iter().zip(&snapshot.request.recipes) {
        assert_eq!(weapon.namespace, recipe.namespace);
        assert_eq!(
            weapon.identity.item_hash,
            recipe.identity.item_hash.parse_u32().unwrap()
        );
    }
}

#[test]
fn staged_recipe_snapshot_round_trips_normalized_json() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: PathBuf::from("packages"),
        staging_root: PathBuf::from("staging"),
        ignore_installed_authored_overlays: true,
        recipes: vec![
            WeaponRecipe::second_sun().unwrap(),
            WeaponRecipe::every_end(),
        ],
    })
    .unwrap();

    let paths = stage_recipe_snapshot(directory.path(), &snapshot).unwrap();

    assert_eq!(paths.len(), 2);
    let loaded = paths
        .iter()
        .map(|path| WeaponRecipe::load_json(path).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(loaded, snapshot.request.recipes);
}
