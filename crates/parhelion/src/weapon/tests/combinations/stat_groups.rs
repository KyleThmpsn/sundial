use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_COMBINATION_ROOT, optional PARHELION_STAT_GROUP_CASE"]
fn native_stat_group_combinations_round_trip() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("stat-catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let groups = DONORS
        .iter()
        .filter_map(|&(hash, name, _, _)| {
            catalog
                .weapon_donor(hash)
                .unwrap()
                .summary
                .stat_group_index
                .map(|group| (group, (hash, name)))
        })
        .collect::<BTreeMap<_, _>>();
    assert!(
        groups.len() >= 8,
        "the matrix must exercise at least eight distinct native stat groups"
    );
    let selected = std::env::var("PARHELION_STAT_GROUP_CASE")
        .ok()
        .map(|value| value.parse::<usize>().unwrap());
    if let Some(selected) = selected {
        assert!(selected < groups.len());
    }
    fs::write(
        output.join("stat-groups.json"),
        serde_json::to_vec_pretty(&groups).unwrap(),
    )
    .unwrap();
    for (case, (&group, &(source_hash, source_name))) in groups.iter().enumerate() {
        if selected.is_some_and(|selected| selected != case) {
            continue;
        }
        let mut recipes = recipes(&catalog, case % 36);
        for (index, recipe) in recipes.iter_mut().enumerate() {
            recipe
                .rename_authored_item(format!("Stat Group {group} {}", DONORS[index].1))
                .unwrap();
            recipe.overrides.stat_group_index = Some(group);
            recipe.overrides.stat_group_donor_hash = Some(source_hash.into());
            recipe.overrides.investment_stats.clear();
            let effective = catalog
                .weapon_donor_with_stat_group_index(DONORS[index].0, Some(group))
                .unwrap();
            if let Some(stat) = effective.investment_stats.iter().find(|stat| {
                stat.minimum_value
                    .zip(stat.maximum_value)
                    .is_some_and(|(low, high)| low <= high)
            }) {
                recipe.overrides.investment_stats.push(WeaponStatOverride {
                    definition_index: stat.definition_index,
                    value: if (case + index) % 2 == 0 {
                        stat.minimum_value.unwrap()
                    } else {
                        stat.maximum_value.unwrap()
                    },
                });
            }
            recipe.validate().unwrap();
        }
        eprintln!(
            "Stat group {group} from {source_name}, {} weapon families",
            recipes.len()
        );
        let (build, variants) = run_batch(
            &packages,
            &output,
            &format!("stat-group-{group}"),
            &recipes,
            verify_weapon,
        );
        eprintln!(
            "STAT_GROUP_PASS case={case} group={group} weapons={} ammo_variants={variants} stage={}",
            recipes.len(),
            build.run_directory.display()
        );
    }
}
