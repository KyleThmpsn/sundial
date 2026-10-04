//! Class selection from the armor editor through saved recipes and staged native records.
use super::*;

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn pointer(data: &[u8], at: usize) -> usize {
    (at as i64 + i64::from_le_bytes(data[at..at + 8].try_into().unwrap())) as usize
}

fn rows(data: &[u8], at: usize, stride: usize) -> Vec<usize> {
    let count = u64::from_le_bytes(data[at..at + 8].try_into().unwrap()) as usize;
    if count == 0 {
        return Vec::new();
    }
    let header = pointer(data, at + 8);
    assert_eq!(
        u64::from_le_bytes(data[header..header + 8].try_into().unwrap()) as usize,
        count
    );
    (0..count).map(|i| header + 16 + i * stride).collect()
}

fn requirements(data: &[u8]) -> Vec<Vec<[u32; 2]>> {
    let equipment = pointer(data, 0x10);
    assert_eq!(word(data, equipment - 4), 0x8080_7C02);
    rows(data, equipment, 16)
        .into_iter()
        .map(|group| {
            rows(data, group, 8)
                .into_iter()
                .map(|token| [word(data, token), word(data, token + 4)])
                .collect()
        })
        .collect()
}

fn class_flag(program: &[[u32; 2]]) -> bool {
    program
        .iter()
        .any(|&[op, flag]| op == 1 && matches!(flag, 0xF0..=0xF4 | 0x109..=0x10D | 0x110..=0x114))
}

#[test]
#[ignore = "Requires PARHELION_DEFAULT_WEAPONS_PACKAGES and a fresh PARHELION_GEAR_ARTIFACTS directory"]
#[allow(
    clippy::cognitive_complexity,
    reason = "Keep the editor workflow and independent native readback together"
)]
fn armor_class_picker_controls_equip_collections_and_badges() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_GEAR_ARTIFACTS").unwrap());
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(output.join("recipes")).unwrap();
    let stock = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let source = open_shadowkeep_package_manager(&packages).unwrap();
    let definition = |hash| {
        source
            .read_tag(TagHash(stock.item_definition_tag(hash).unwrap()))
            .unwrap()
    };
    let donors = stock.gear_donors(ItemKind::Armor.bucket_hashes());
    // Cover all five native armor slots, plus an Exotic and a class-neutral donor.
    let mut bases = (3..=7)
        .map(|slot| {
            donors
                .iter()
                .find(|donor| {
                    donor.collection_backed
                        && stock.item_class_type(donor.hash) == Some(0)
                        && donor.rarity == WeaponRarity::Legendary
                        && definition(donor.hash)[0xB8] == slot
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    bases.push(
        donors
            .iter()
            .find(|donor| {
                donor.collection_backed
                    && stock.item_class_type(donor.hash) == Some(0)
                    && donor.rarity == WeaponRarity::Exotic
            })
            .unwrap(),
    );
    bases.push(
        donors
            .iter()
            .find(|donor| donor.collection_backed && stock.item_class_type(donor.hash) == Some(3))
            .unwrap(),
    );
    // Retain a separate Festival predicate when removing only the class restriction.
    bases.push(
        donors
            .iter()
            .find(|donor| {
                donor.collection_backed
                    && stock.item_class_type(donor.hash) == Some(0)
                    && requirements(&definition(donor.hash)).len() > 1
            })
            .unwrap(),
    );
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        ..Default::default()
    };
    // The app takes its own catalog, a second read of the test cache, and `stock` stays for
    // the checks below.
    app.install_catalog(crate::test_support::catalog(packages.parent().unwrap()).unwrap());
    let ctx = context();
    let choices = [
        ("Base Class", None),
        ("Titan", Some(crate::ArmorClass::Titan)),
        ("Hunter", Some(crate::ArmorClass::Hunter)),
        ("Warlock", Some(crate::ArmorClass::Warlock)),
        ("Any Class", Some(crate::ArmorClass::Any)),
    ];
    let mut cases = Vec::new();
    for (ordinal, base) in bases.iter().enumerate() {
        for &(label, choice) in &choices {
            new_from_menu(&ctx, &mut app, ItemKind::Armor);
            app.recipe.set_donor(base.hash, base.name.clone());
            app.edit_weapon_name(format!("Armor Class Picker {ordinal} {label}"));
            let page = settle(&ctx, &mut app);
            let inherited = stock.item_class_type(base.hash).unwrap();
            let default_label = format!(
                "{} (Base Armor)",
                class_label(inherited).unwrap_or("Any Class")
            );
            assert!(texts(&page).iter().any(|(text, _)| text == "Class"));
            assert!(app.recipe.overrides.armor_class.is_none());
            if choice.is_some() {
                click(
                    &ctx,
                    &mut app,
                    find(&page, &default_label, |text, _| text == default_label),
                );
                let menu = settle(&ctx, &mut app);
                click(&ctx, &mut app, find(&menu, label, |text, _| text == label));
            }
            assert_eq!(app.recipe.overrides.armor_class, choice);
            let page = settle(&ctx, &mut app);
            capture::write(&ctx, &page, &format!("armor-class-{ordinal}-{label}"));
            let path = output
                .join("recipes")
                .join(format!("{}.json", app.recipe.namespace));
            fs::write(&path, app.recipe.to_json_pretty().unwrap()).unwrap();
            let recipe = WeaponRecipe::load_json(path).unwrap();
            assert_eq!(recipe.overrides.armor_class, choice);
            let expected = match choice {
                None => inherited,
                Some(crate::ArmorClass::Titan) => 0,
                Some(crate::ArmorClass::Hunter) => 1,
                Some(crate::ArmorClass::Warlock) => 2,
                Some(crate::ArmorClass::Any) => 3,
            };
            cases.push((
                recipe,
                expected,
                definition(base.hash),
                base.rarity == WeaponRarity::Exotic,
            ));
        }
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: cases.iter().map(|(r, ..)| r.clone()).collect(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = output.join("view");
    staged_view(&packages, &build, &view);
    let staged =
        InvestmentCatalog::load_with_cache_path(&view, &output.join("catalog.json"), true, |_| {})
            .unwrap();
    let manager = open_shadowkeep_package_manager(&view.join("packages")).unwrap();
    let mut receipt = Vec::new();
    for (recipe, expected, original, exotic) in cases {
        let hash = recipe.identity.item_hash.parse_u32().unwrap();
        let actual = manager
            .read_tag(TagHash(staged.item_definition_tag(hash).unwrap()))
            .unwrap();
        assert_eq!(staged.item_class_type(hash), Some(expected));
        assert_eq!(actual[0xB8], original[0xB8]);
        let conditions = requirements(&actual);
        if recipe.overrides.armor_class.is_none() {
            assert_eq!(conditions, requirements(&original));
        } else {
            assert_eq!(
                conditions
                    .iter()
                    .filter(|p| !class_flag(p))
                    .collect::<Vec<_>>(),
                requirements(&original)
                    .iter()
                    .filter(|p| !class_flag(p))
                    .collect::<Vec<_>>()
            );
            let selected = conditions
                .iter()
                .filter(|p| class_flag(p))
                .collect::<Vec<_>>();
            if expected == 3 {
                assert!(selected.is_empty());
            } else {
                let flags = [
                    [0x10C, 0x109, 0x10A, 0x10D, 0x10B],
                    [0xF3, 0xF0, 0xF1, 0xF4, 0xF2],
                    [0x113, 0x110, 0x111, 0x114, 0x112],
                ];
                assert_eq!(
                    selected,
                    [&vec![[
                        1,
                        flags[usize::from(expected)][usize::from(actual[0xB8] - 3)]
                    ]]]
                );
            }
        }
        let paths = staged.item_collection_paths(hash);
        let parents = staged.item_collection_parents(hash);
        for (class, badge) in [0x5355_4E54, 0x5355_4E48, 0x5355_4E57]
            .into_iter()
            .enumerate()
        {
            let supported = expected == 3 || usize::from(expected) == class;
            assert_eq!(parents.contains(&badge), supported);
            let class_name = ["Titan", "Hunter", "Warlock"][class];
            assert_eq!(
                paths.iter().any(|p| p.len() >= 4
                    && p[p.len() - 3] == class_name
                    && p[p.len() - 2] == "Armor"
                    && p.last().is_some_and(|s| s == "Items")),
                supported && !exotic
            );
            assert_eq!(
                paths.iter().any(|p| p
                    .iter()
                    .map(String::as_str)
                    .eq([class_name, "Armor", "Exotic", "Items"])),
                supported && exotic
            );
        }
        // Reading the original tags from the staged overlay must still return stock bytes.
        let donor = recipe.donor.item_hash.parse_u32().unwrap();
        assert_eq!(
            manager
                .read_tag(TagHash(stock.item_definition_tag(donor).unwrap()))
                .unwrap(),
            original
        );
        receipt.push(serde_json::json!({"item":hash,"class":expected,"exotic":exotic,"equip_conditions":conditions,"paths":paths,"parents":parents}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(view).unwrap();
}
