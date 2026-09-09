//! Invalid inputs must fail before a staging run becomes installable.
use super::*;

fn mutation(recipe: &mut WeaponRecipe, case: usize, sockets: usize) {
    let column = |choices| {
        Some(WeaponSocketColumnRecipe {
            choices,
            ..Default::default()
        })
    };
    match case {
        0 => recipe.namespace = "outside.the.authoring.namespace".to_owned(),
        1 => recipe.name.push('\0'),
        2 => recipe.identity.item_hash = recipe.donor.item_hash.clone(),
        3 => recipe.overrides.max_stack_size = Some(0),
        4 => recipe.overrides.max_stack_size = Some(u32::MAX),
        5 => {
            recipe.overrides.socket_columns =
                vec![None; sundial::investment::MAX_WEAPON_SOCKETS + 1]
        }
        6 => {
            recipe.overrides.investment_stats = vec![
                WeaponStatOverride {
                    definition_index: 13,
                    value: 1
                };
                2
            ]
        }
        7 => {
            recipe.overrides.investment_stats = vec![WeaponStatOverride {
                definition_index: u16::MAX - 1,
                value: 1,
            }]
        }
        8 => recipe.overrides.stat_group_index = Some(u16::MAX - 1),
        9 => recipe.overrides.weapon_pattern_index = Some(u16::MAX - 1),
        10 => recipe.overrides.socket_columns = vec![column(vec![0.into()])],
        11 => recipe.overrides.socket_columns = vec![column(vec![0xDD5C_B37A.into(); 2])],
        12 | 13 => {
            recipe.overrides.socket_columns = vec![Some(WeaponSocketColumnRecipe {
                choices: vec![0xDD5C_B37A.into()],
                choice_weight_bits: vec![if case == 12 { f32::NAN.to_bits() } else { 0 }],
                ..Default::default()
            })]
        }
        14 => {
            recipe.overrides.socket_columns = vec![None; sockets];
            recipe
                .overrides
                .socket_columns
                .push(column(vec![0xDD5C_B37A.into()]));
        }
        15 => recipe.overrides.trait_indices = Some(vec![u16::MAX - 1]),
        16 => recipe.overrides.base_sandbox_perks = Some(vec![u16::MAX - 1]),
        17 => {
            recipe.presentation_donor = Some(WeaponDonorReference {
                item_hash: if recipe.donor.item_hash.parse_u32().unwrap() == 0xA25B_8F8F {
                    0x5038_4F33.into()
                } else {
                    0xA25B_8F8F.into()
                },
                expected_name: None,
            })
        }
        _ => unreachable!(),
    }
}

fn expect_rejected(packages: &Path, output: &Path, label: &str, recipe: WeaponRecipe) {
    let staging_root = output.join(label);
    assert!(
        !staging_root.exists(),
        "invalid test needs an unused staging directory"
    );
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.to_path_buf(),
        staging_root: staging_root.clone(),
        ignore_installed_authored_overlays: true,
        recipes: vec![recipe],
    });
    let error = match snapshot {
        Err(error) => error,
        Ok(snapshot) => {
            crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap_err()
        }
    };
    assert!(!error.is_empty());
    assert!(
        !staging_root.exists(),
        "rejected input created staging output: {label}"
    );
    eprintln!("EXPECTED_REJECTION {label}: {error}");
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_COMBINATION_ROOT"]
fn native_invalid_combinations_leave_no_staging_output() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let temporary = tempfile::tempdir_in(&output).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("negative-catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let mut rejected = 0;
    for (index, &(hash, name, _, _)) in DONORS.iter().enumerate() {
        let donor = catalog.weapon_donor(hash).unwrap();
        for case in 0..18 {
            let label = format!("rejected-{index}-{case}");
            let mut recipe = WeaponRecipe::new_named_weapon_for_donor(
                format!("Rejected {index} {case}"),
                hash,
                name,
            )
            .unwrap();
            mutation(&mut recipe, case, donor.sockets.len());
            expect_rejected(&packages, temporary.path(), &label, recipe);
            rejected += 1;
        }
    }
    for (index, rarity) in [
        RecipeRarity::Common,
        RecipeRarity::Uncommon,
        RecipeRarity::Rare,
        RecipeRarity::Legendary,
    ]
    .into_iter()
    .enumerate()
    {
        let mut trace = WeaponRecipe::new_named_weapon_for_donor(
            format!("Rejected Trace {index}"),
            0x5038_4F33,
            "Coldheart",
        )
        .unwrap();
        trace.overrides.rarity = Some(rarity);
        expect_rejected(
            &packages,
            temporary.path(),
            &format!("trace-{index}"),
            trace,
        );
        rejected += 1;
    }
    assert_eq!(rejected, 292);
    eprintln!("INVALID_COMBINATIONS_PASS rejected={rejected}");
}
