use super::*;

fn item_payloads(manager: &PackageManager, globals: &[u8], hash: u32) -> (Vec<u8>, Vec<u8>) {
    let root = manager
        .read_tag(globals_child_tag(globals, 0).unwrap())
        .unwrap();
    let items = manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let strings = manager
        .read_tag(globals_child_tag(globals, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap())
        .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let index = find_u32_row_key(&items, rows, count, ITEM_ROW_SIZE, hash)
        .unwrap()
        .unwrap();
    let string_rows = array_at(&strings, 8).unwrap().2;
    (
        manager
            .read_tag(TagHash(
                read_u32(&items, rows + index * ITEM_ROW_SIZE + 16).unwrap(),
            ))
            .unwrap(),
        manager
            .read_tag(TagHash(
                read_u32(&strings, string_rows + index * ITEM_ROW_SIZE + 16).unwrap(),
            ))
            .unwrap(),
    )
}

fn assert_preserved_companions(source: &[u8], authored: &[u8]) {
    let source_segment = item_string_sandbox_perk_segment(source).unwrap().unwrap();
    let authored_segment = item_string_sandbox_perk_segment(authored).unwrap().unwrap();
    let source_rows = array_at(source, item_string_sandbox_perk_descriptor(source).unwrap())
        .unwrap()
        .2;
    let authored_rows = array_at(
        authored,
        item_string_sandbox_perk_descriptor(authored).unwrap(),
    )
    .unwrap()
    .2;
    for index in 0..item_string_sandbox_perk_count(source).unwrap() {
        let offset = 32 + index * 32;
        assert_eq!(
            &source_segment[offset..offset + 16],
            &authored_segment[offset..offset + 16]
        );
        assert_eq!(
            &source_segment[offset + 24..offset + 32],
            &authored_segment[offset + 24..offset + 32]
        );
        let source_pointer = source_rows + index * 32 + 16;
        if read_i64(source, source_pointer).unwrap() != 0 {
            let old = relative_target(source, source_pointer).unwrap();
            let new = relative_target(authored, authored_rows + index * 32 + 16).unwrap();
            let length = 16 + read_u64(source, old).unwrap() as usize * 8;
            assert_eq!(&source[old..old + length], &authored[new..new + length]);
        }
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES"]
fn private_companion_variants_pack_and_preserve_stock_data() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let manager = open_manager(&packages).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    // Conditional, localized, presentation-only and multi-row perk donors.
    let donors = [
        0x87AE_999A,
        0xD861_CA14,
        0xBD4A_512D,
        0x6350_4E6A,
        0x6AA6_DA7D,
        0xD816_8754,
        0x587E_823F, // Powerful Friends has a conditional stat bonus.
        0x86E7_8BF3, // Charge Harvester also has conditional stat contributions.
    ];
    let mut cases = Vec::new();
    let mut specs = Vec::new();
    let (weapon_donor, _) = item_payloads(&manager, &globals, 0x6876_536E);
    let socket_count = weapon_default_plug_indices(&weapon_donor).unwrap().len();
    for hash in donors {
        let (definition, strings) = item_payloads(&manager, &globals, hash);
        let source_perks = weapon_sandbox_perks(&definition).unwrap();
        assert!(!source_perks.contains(&1178));
        for mode in 0..3 {
            let recipe = crate::WeaponRecipe::new_weapon_for_donor(
                format!("parhelion.companion.{hash:08x}.{mode}"),
                0x6876_536E,
                "BrayTech Winter Wolf",
            )
            .unwrap();
            let mut spec = recipe.to_spec().unwrap();
            spec.overrides.socket_columns = vec![None; socket_count];
            spec.overrides.socket_columns[4] = Some(WeaponSocketColumnOverride {
                choices: vec![hash],
                ..Default::default()
            });
            let edits = if mode != 2 {
                vec![WeaponSandboxPerkRuntimeOverride {
                    source_perk_index: source_perks[0],
                    program: None,
                    projectiles: Vec::new(),
                    activation: None,
                    runtime_values: Vec::new(),
                    action_float_values: Vec::new(),
                }]
            } else {
                Vec::new()
            };
            spec.overrides.socket_plug_variants = vec![WeaponSocketPlugVariantOverride {
                replace_effects: mode == 2,
                investment_stats: stat_edits(&definition, mode),
                socket_index: 4,
                choice_index: 0,
                source_plug_hash: hash,
                name: Some(format!("Private Companion {hash:08X} {mode}")),
                classification_donor_hash: None,
                icon: None,
                description: None,
                additional_sandbox_perks: if mode == 0 { Vec::new() } else { vec![1178] },
                sandbox_perks: edits,
            }];
            cases.push((
                spec.identity.item_hash,
                hash,
                mode,
                definition.clone(),
                strings.clone(),
            ));
            specs.push(spec);
        }
    }
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec { weapons: specs },
    )
    .unwrap();
    let view = staged_view(&packages, "private-companions-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let authored_globals = staged
        .read_tag(resolve_live_named_tag(&staged, "investment_globals", None).unwrap())
        .unwrap();
    let root = staged
        .read_tag(globals_child_tag(&authored_globals, 0).unwrap())
        .unwrap();
    let items = staged
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let mut private_hashes = BTreeSet::new();
    for case in cases {
        let hash = verify_case(&manager, &globals, &staged, &authored_globals, &items, case);
        assert!(private_hashes.insert(hash));
    }
    assert_eq!(private_hashes.len(), 24);
    eprintln!("Packed and verified 24 private companion variants without changing stock donors");
}

type Case = (u32, u32, u8, Vec<u8>, Vec<u8>);

fn verify_case(
    manager: &PackageManager,
    globals: &[u8],
    staged: &PackageManager,
    authored_globals: &[u8],
    items: &[u8],
    (weapon_hash, source_hash, mode, source_definition, source_strings): Case,
) -> u32 {
    let item_rows = array_at(items, 8).unwrap().2;
    let (weapon, _) = item_payloads(staged, authored_globals, weapon_hash);
    let plug_index = weapon_default_plug_indices(&weapon).unwrap()[4];
    let plug_hash = read_u32(items, item_rows + usize::from(plug_index) * ITEM_ROW_SIZE).unwrap();
    assert_ne!(plug_hash, source_hash);
    let (definition, strings) = item_payloads(staged, authored_globals, plug_hash);
    assert_eq!(read_u64(&definition, 0).unwrap() as usize, definition.len());
    assert_eq!(read_u64(&strings, 0).unwrap() as usize, strings.len());
    let perks = weapon_sandbox_perks(&definition).unwrap();
    assert_stat_programs(&source_definition, &definition, mode == 2);
    assert_eq!(
        item_string_sandbox_perk_count(&strings).unwrap(),
        perks.len()
    );
    if mode == 2 {
        assert_eq!(perks.len(), 1);
        assert!(
            weapon_sandbox_perk_rows(&definition).unwrap()[0][8..]
                .iter()
                .all(|byte| *byte == 0)
        );
        validate_item_string_sandbox_perk_template(
            item_string_sandbox_perk_segment(&strings).unwrap().unwrap(),
        )
        .unwrap();
    } else {
        assert_preserved_companions(&source_strings, &strings);
        assert_preserved_gameplay_conditions(&source_definition, &definition);
        assert_eq!(
            perks.len(),
            weapon_sandbox_perks(&source_definition).unwrap().len() + usize::from(mode == 1)
        );
    }
    if mode != 0 {
        let private = *perks.last().unwrap();
        assert_ne!(private, 1178);
        assert_private_runtime(manager, globals, staged, authored_globals, private, 1178);
    }
    if mode != 2 {
        let source_perk = weapon_sandbox_perks(&source_definition).unwrap()[0];
        assert_ne!(perks[0], source_perk);
        assert_private_runtime(
            manager,
            globals,
            staged,
            authored_globals,
            perks[0],
            source_perk,
        );
    }
    let (stock_definition, stock_strings) = item_payloads(staged, authored_globals, source_hash);
    assert_eq!(stock_definition, source_definition);
    assert_eq!(stock_strings, source_strings);
    plug_hash
}

fn assert_private_runtime(
    manager: &PackageManager,
    globals: &[u8],
    staged: &PackageManager,
    authored_globals: &[u8],
    private: u16,
    source: u16,
) {
    assert_ne!(private, source);
    let action =
        load_sandbox_perk_runtime_action(staged, authored_globals, usize::from(private)).unwrap();
    let stock = load_sandbox_perk_runtime_action(manager, globals, usize::from(source)).unwrap();
    assert_eq!(action.action_payload, stock.action_payload);
}

fn assert_preserved_gameplay_conditions(source: &[u8], authored: &[u8]) {
    let original = weapon_sandbox_perk_rows(source).unwrap();
    let copied = weapon_sandbox_perk_rows(authored).unwrap();
    let source_resource = relative_target(source, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let authored_resource = relative_target(authored, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    let source_rows = array_at(
        source,
        source_resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET,
    )
    .unwrap()
    .2;
    let authored_rows = array_at(
        authored,
        authored_resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET,
    )
    .unwrap()
    .2;
    for (index, row) in original.iter().enumerate() {
        assert_eq!(&row[2..16], &copied[index][2..16]);
        if read_i64(row, 16).unwrap() != 0 {
            let source_pointer = source_rows + index * ITEM_SANDBOX_PERK_ROW_SIZE + 16;
            let authored_pointer = authored_rows + index * ITEM_SANDBOX_PERK_ROW_SIZE + 16;
            let old = relative_target(source, source_pointer).unwrap();
            let new = relative_target(authored, authored_pointer).unwrap();
            let length = 16 + read_u64(source, old).unwrap() as usize * 8;
            assert_eq!(&source[old..old + length], &authored[new..new + length]);
        }
    }
}

fn stat_layout(data: &[u8]) -> Option<(usize, usize)> {
    let resource = relative_target(data, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
    if read_u64(data, resource).unwrap() == 0 {
        return None;
    }
    let (count, _, rows, _) = array_at(data, resource).unwrap();
    Some((count, rows))
}

fn stat_edits(data: &[u8], mode: u8) -> Vec<(u16, i32)> {
    let stats = stat_layout(data)
        .map(|(count, rows)| {
            (0..count)
                .map(|i| {
                    u16::from(read_u8(data, rows + i * ITEM_INVESTMENT_STAT_ROW_SIZE).unwrap())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match mode {
        0 => Vec::new(),
        1 => vec![(
            [13, 16, 20, 2]
                .into_iter()
                .find(|stat| !stats.contains(stat))
                .unwrap(),
            7,
        )],
        _ => vec![(stats.first().copied().unwrap_or(13), 7)],
    }
}

fn assert_stat_programs(source: &[u8], authored: &[u8], replaced: bool) {
    let Some((count, rows)) = stat_layout(authored) else {
        return;
    };
    let copied = stat_program_targets(authored, rows, count).unwrap();
    if replaced {
        assert_eq!(count, 1);
        assert!(copied.values().flatten().all(Option::is_none));
        return;
    }
    let Some((count, rows)) = stat_layout(source) else {
        return;
    };
    for (stat, targets) in stat_program_targets(source, rows, count).unwrap() {
        for (old, new) in targets.into_iter().zip(copied[&stat]) {
            assert_eq!(old.is_some(), new.is_some());
            if let (Some(old), Some(new)) = (old, new) {
                let length = 16 + read_u64(source, old).unwrap() as usize * 8;
                assert_eq!(&source[old..old + length], &authored[new..new + length]);
            }
        }
    }
}
