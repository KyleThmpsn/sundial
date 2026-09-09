use super::*;

fn recipe() -> WeaponRecipe {
    WeaponRecipe::from_json_str(include_str!(
        "../../../../recipes/good-company.parhelion.json"
    ))
    .unwrap()
}

#[test]
fn good_company_has_three_exotic_defaults_and_four_choices_per_trait() {
    let recipe = recipe();
    assert_eq!(
        recipe.to_spec().unwrap().identity,
        WeaponCloneIdentity::from_namespace("parhelion.good-company").unwrap()
    );
    assert_eq!(recipe.donor.item_hash.parse_u32().unwrap(), 0xD84E_04AB);
    assert!(recipe.runtime_component_donors.is_empty());
    assert!(recipe.overrides.socket_columns[0].is_none());
    let mut choices = BTreeSet::new();
    for (socket, default) in [(3, 0xEEB6_9A10), (4, 0x8E18_595D), (8, 0xF72A_1183)] {
        let column = recipe.overrides.socket_columns[socket].as_ref().unwrap();
        assert_eq!(column.socket_type, Some(92));
        assert_eq!(column.choices.len(), 4);
        assert_eq!(column.choices[0].parse_u32().unwrap(), default);
        for choice in &column.choices {
            assert!(choices.insert(choice.parse_u32().unwrap()));
        }
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_COMBINATION_ROOT"]
fn native_good_company_trait_choices_preserve_intrinsic_and_stock() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let stock = open_manager(&packages).unwrap();
    let source_tables = Tables::read(&stock);
    let recipes = (0..4)
        .map(|selection| {
            let mut recipe = recipe();
            if selection != 0 {
                recipe
                    .rename_authored_item(format!("Good Company Choice {selection}"))
                    .unwrap();
            }
            for socket in [3, 4, 8] {
                recipe.overrides.socket_columns[socket]
                    .as_mut()
                    .unwrap()
                    .choices
                    .rotate_left(selection);
            }
            recipe.overrides.socket_plug_variants[0].choice_index = ((4 - selection) % 4) as u16;
            recipe
        })
        .collect::<Vec<_>>();
    let mut identities = private::PrivateIdentities::default();
    let (build, _) = run_batch(
        &packages,
        &output,
        "good-company",
        &recipes,
        |manager, tables, recipe| {
            let ammo = verify_fields(manager, tables, recipe);
            private::verify_private(
                manager,
                tables,
                &stock,
                &source_tables.globals,
                recipe,
                &mut identities,
            );
            let mut spec = recipe.to_spec().unwrap();
            let (definition, _) = tables.load(manager, spec.identity.item_hash);
            let resource =
                relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
            let (_, _, rows, _) = array_at(&definition, resource).unwrap();
            let variant = &spec.overrides.socket_plug_variants[0];
            let row = rows + usize::from(variant.socket_index) * ITEM_ORDINARY_SOCKET_ROW_SIZE;
            let (_, _, choices, _) = array_at(
                &definition,
                row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET,
            )
            .unwrap();
            let private_hash = tables.plug_hash(
                read_u16(
                    &definition,
                    choices
                        + usize::from(variant.choice_index)
                            * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE,
                )
                .unwrap(),
            );
            spec.overrides.socket_columns[3].as_mut().unwrap().choices
                [usize::from(variant.choice_index)] = private_hash;
            spec.overrides.socket_plug_variants.clear();
            verify_sockets(manager, tables, &definition, &spec);
            let defaults = weapon_default_plug_indices(&definition).unwrap();
            assert_eq!(tables.plug_hash(defaults[0]), 0x8984_35DF);
            let (source, _) = source_tables.load(&stock, 0xD84E_04AB);
            let source_defaults = weapon_default_plug_indices(&source).unwrap();
            for socket in [0, 1, 2, 6, 7] {
                assert_eq!(
                    tables.plug_hash(defaults[socket]),
                    source_tables.plug_hash(source_defaults[socket])
                );
            }
            ammo
        },
    );
    eprintln!("GOOD_COMPANY_PASS stage={}", build.run_directory.display());
}
