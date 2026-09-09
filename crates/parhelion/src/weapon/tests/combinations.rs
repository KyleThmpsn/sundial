//! Native combination coverage. Passing package checks is not gameplay acceptance.
use super::*;
use crate::recipe::{RecipeAmmoType, RecipeDamageType, RecipeInventorySlot, RecipeRarity};
use crate::{
    HexHash, WeaponDonorReference, WeaponRecipe, WeaponSocketColumnRecipe, WeaponStatOverride,
};
use sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager;
mod good_company;
mod invalid;
mod private;
mod reclamation_order;
mod stat_groups;
mod stress;

// Same-family presentation donors include old and newer damage-carrier layouts.
const DONORS: &[(u32, &str, u32, &str)] = &[
    (0xA25B_8F8F, "Arc Logic", 0xA25B_8F8F, "Arc Logic"),
    (0x4CE3_CE93, "Breachlight", 0x0D90_1ED7, "Interregnum XVI"),
    (0x53D5_1E72, "Agamid", 0x26F9_5A03, "Allegro-34"),
    (0xC63B_5A50, "A Fine Memorial", 0x23F4_BF01, "Hammerhead"),
    (0x97B2_E5DE, "Apex Predator", 0x3B16_442C, "Bad Omens"),
    (0x25F6_83B0, "Dust Rock Blues", 0x4F1C_712E, "Badlander"),
    (
        0xAEC2_1E34,
        "Dream Breaker",
        0x9527_F0F4,
        "Cartesian Coordinate",
    ),
    (0x8102_DDBD, "Apostate", 0xC56A_395C, "A Single Clap"),
    (0xE079_4C51, "Black Talon", 0xE079_4C51, "Black Talon"),
    (
        0x7066_4F86,
        "Call to Serve",
        0x1F56_4FF7,
        "Black Scorpion-4sr",
    ),
    (0x7CDE_3A31, "Adjudicator", 0x7D84_5F1B, "Bad Reputation"),
    (
        0xF422_6A09,
        "Accrued Redemption",
        0x2AEF_B232,
        "No Turning Back",
    ),
    (0xE516_CF40, "Blast Furnace", 0x4591_5B1E, "Adhortative"),
    (0x3869_9403, "Line in the Sand", 0xA0C1_DA62, "Komodo-4FR"),
    (0x5038_4F33, "Coldheart", 0x5038_4F33, "Coldheart"),
    (
        0xEE06_B019,
        "The Mountaintop",
        0xEE06_B019,
        "The Mountaintop",
    ),
];

fn recipes(catalog: &InvestmentCatalog, profile: usize) -> Vec<WeaponRecipe> {
    assert!(profile < 36);
    DONORS
        .iter()
        .enumerate()
        .map(
            |(index, &(hash, donor_name, appearance, appearance_name))| {
                let name = format!("Combination {profile:02} {donor_name}");
                let mut recipe =
                    WeaponRecipe::new_named_weapon_for_donor(name, hash, donor_name).unwrap();
                recipe.overrides.ammo_type = Some(
                    [
                        RecipeAmmoType::Primary,
                        RecipeAmmoType::Special,
                        RecipeAmmoType::Heavy,
                    ][profile % 3],
                );
                recipe.overrides.modern_damage_type = Some(
                    [
                        RecipeDamageType::Kinetic,
                        RecipeDamageType::Arc,
                        RecipeDamageType::Solar,
                        RecipeDamageType::Void,
                    ][profile / 3 % 4],
                );
                recipe.overrides.inventory_slot = Some(
                    [
                        RecipeInventorySlot::Kinetic,
                        RecipeInventorySlot::Energy,
                        RecipeInventorySlot::Power,
                    ][profile / 12],
                );
                recipe.overrides.rarity = Some(
                    [
                        RecipeRarity::Common,
                        RecipeRarity::Uncommon,
                        RecipeRarity::Rare,
                        RecipeRarity::Legendary,
                        RecipeRarity::Exotic,
                    ][(profile + index) % 5],
                );
                // Shadowkeep has no ordinary Trace Rifle Collections page.
                if hash == 0x5038_4F33 {
                    recipe.overrides.rarity = Some(RecipeRarity::Exotic);
                }
                if (profile + index) % 2 == 1 {
                    let source = catalog.weapon_donor(hash).unwrap();
                    let target = [
                        sundial::investment::WeaponInventorySlot::Kinetic,
                        sundial::investment::WeaponInventorySlot::Energy,
                        sundial::investment::WeaponInventorySlot::Power,
                    ][profile / 12];
                    let candidate = catalog.weapon_donor(appearance).unwrap();
                    if crate::capabilities::appearance_compatibility(
                        &candidate.summary,
                        &source.summary,
                        target,
                    ) == crate::capabilities::AppearanceCompatibility::Compatible
                    {
                        recipe.presentation_donor = Some(WeaponDonorReference {
                            item_hash: appearance.into(),
                            expected_name: Some(appearance_name.to_owned()),
                        });
                    }
                    recipe.overrides.icon_edit.hue_shift_degrees = 37;
                    recipe.overrides.icon_edit.brightness = 8;
                }
                let donor = catalog.weapon_donor(hash).expect("matrix donor must exist");
                let stat_mode = (profile / 3 + index) % 3;
                if stat_mode != 0 {
                    if let Some(stat) = donor.investment_stats.iter().find(|stat| {
                        stat.minimum_value
                            .zip(stat.maximum_value)
                            .is_some_and(|(low, high)| low <= high)
                    }) {
                        recipe.overrides.investment_stats.push(WeaponStatOverride {
                            definition_index: stat.definition_index,
                            value: if stat_mode == 1 {
                                stat.minimum_value.unwrap()
                            } else {
                                stat.maximum_value.unwrap()
                            },
                        });
                    }
                }
                match (profile + index) % 3 {
                    1 => {
                        let supported = catalog.weapon_supported_plug_sets(hash).unwrap();
                        if let Some(set) = supported.iter().find(|set| {
                            donor
                                .sockets
                                .get(set.socket_index)
                                .is_some_and(|socket| socket.max_authored_choices >= 2)
                                && set.plug_hashes.iter().filter(|hash| **hash != 0).count() >= 2
                        }) {
                            recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
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
                                    ..Default::default()
                                });
                        }
                    }
                    2 if donor.sockets.len() < sundial::investment::MAX_WEAPON_SOCKETS => {
                        recipe.overrides.socket_columns = vec![None; donor.sockets.len()];
                        recipe
                            .overrides
                            .socket_columns
                            .push(Some(WeaponSocketColumnRecipe {
                                choices: vec![0xDD5C_B37A.into()],
                                socket_type: Some(92),
                                ..Default::default()
                            }));
                        recipe.overrides.socket_plug_variants.push(
                            crate::WeaponSocketPlugVariantRecipe {
                                socket_index: donor.sockets.len() as u16,
                                choice_index: 0,
                                source_plug_hash: 0xDD5C_B37A.into(),
                                name: Some(format!("Private Matrix Perk {profile} {index}")),
                                description: Some("A private combination test perk.".to_owned()),
                                classification_donor_hash: None,
                                investment_stats: Vec::new(),
                                additional_sandbox_perks: Vec::new(),
                                sandbox_perks: vec![crate::WeaponSandboxPerkRuntimeRecipe {
                                    source_perk_index: 1178,
                                    activation: None,
                                    runtime_values: Vec::new(),
                                    action_float_values: Vec::new(),
                                }],
                            },
                        );
                    }
                    _ => {}
                }
                let encoded = recipe.to_json_pretty().unwrap();
                let reloaded = WeaponRecipe::from_json_str(&encoded).unwrap();
                assert_eq!(recipe, reloaded);
                recipe
            },
        )
        .collect()
}

struct Tables {
    globals: Vec<u8>,
    items: Vec<u8>,
    strings: Vec<u8>,
    indices: BTreeMap<u32, usize>,
    item_rows: usize,
    string_rows: usize,
}

impl Tables {
    fn read(manager: &tiger_pkg::PackageManager) -> Self {
        let globals = manager
            .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
            .unwrap();
        let root = manager
            .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
            .unwrap();
        let items = manager
            .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
            .unwrap();
        let strings = manager
            .read_tag(globals_child_tag(&globals, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap())
            .unwrap();
        let (count, _, item_rows, _) = array_at(&items, 8).unwrap();
        let (_, _, string_rows, _) = array_at(&strings, 8).unwrap();
        let indices = (0..count)
            .map(|index| {
                (
                    read_u32(&items, item_rows + index * ITEM_ROW_SIZE).unwrap(),
                    index,
                )
            })
            .collect();
        Self {
            globals,
            items,
            strings,
            indices,
            item_rows,
            string_rows,
        }
    }

    fn load(&self, manager: &tiger_pkg::PackageManager, hash: u32) -> (Vec<u8>, Vec<u8>) {
        let index = self.indices[&hash];
        let read = |table: &[u8], rows| {
            manager
                .read_tag(TagHash(
                    read_u32(table, rows + index * ITEM_ROW_SIZE + 16).unwrap(),
                ))
                .unwrap()
        };
        (
            read(&self.items, self.item_rows),
            read(&self.strings, self.string_rows),
        )
    }

    fn plug_hash(&self, index: u16) -> u32 {
        read_u32(
            &self.items,
            self.item_rows + usize::from(index) * ITEM_ROW_SIZE,
        )
        .unwrap()
    }
}

fn verify_ammo(manager: &tiger_pkg::PackageManager, hash: u32, ammo: WeaponAmmoType) -> usize {
    let entity = load_weapon_runtime_entity_with_manager(manager, hash).unwrap();
    let bindings = weapon_component_bindings(&entity.payload, 0x5F0D_D954).unwrap();
    assert_eq!(bindings.len(), 1);
    let binding = bindings[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let instance = binding.resource_offset as usize;
    let definition = read_u64(&owner, instance + 8).unwrap() as usize;
    let count = read_u64(&owner, definition + 0x240).unwrap() as usize;
    assert!(count <= 4096);
    let mut properties = vec![definition + 0x80];
    if count != 0 {
        let header = relative_target(&owner, definition + 0x248).unwrap();
        assert_eq!(read_u64(&owner, header).unwrap() as usize, count);
        assert_eq!(read_u32(&owner, header + 8).unwrap(), 0x8080_3ACF);
        properties.extend((0..count).map(|index| header + 16 + index * 0x1C0));
    }
    for property in &properties {
        assert_eq!(
            &owner[property + 0x34..property + 0x36],
            &[1, ammo as u8 - 1]
        );
    }
    properties.len()
}

fn verify_private_default(
    manager: &tiger_pkg::PackageManager,
    tables: &Tables,
    hash: u32,
    source: u32,
) {
    assert_ne!(hash, source);
    let (plug, _) = tables.load(manager, hash);
    let (stock_plug, _) = tables.load(manager, source);
    let private = weapon_sandbox_perks(&plug).unwrap();
    let stock = weapon_sandbox_perks(&stock_plug).unwrap();
    assert_eq!(private.len(), stock.len());
    for (&private_index, &stock_index) in private.iter().zip(&stock) {
        if stock_index == 1178 {
            assert_ne!(private_index, stock_index);
            let authored_action = load_sandbox_perk_runtime_action(
                manager,
                &tables.globals,
                usize::from(private_index),
            )
            .unwrap();
            let stock_action = load_sandbox_perk_runtime_action(
                manager,
                &tables.globals,
                usize::from(stock_index),
            )
            .unwrap();
            assert_eq!(authored_action.action_tag, stock_action.action_tag);
            assert_eq!(authored_action.action_payload, stock_action.action_payload);
        } else {
            assert_eq!(private_index, stock_index);
        }
    }
}

fn verify_sockets(
    manager: &tiger_pkg::PackageManager,
    tables: &Tables,
    definition: &[u8],
    spec: &WeaponCloneSpec,
) {
    let defaults = weapon_default_plug_indices(definition).unwrap();
    if spec.overrides.socket_columns.is_empty() {
        return;
    }
    assert_eq!(defaults.len(), spec.overrides.socket_columns.len());
    let resource = relative_target(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
    let (_, _, rows, _) = array_at(definition, resource).unwrap();
    for (socket, column) in spec.overrides.socket_columns.iter().enumerate() {
        let Some(column) = column else { continue };
        let hash = tables.plug_hash(defaults[socket]);
        let variant = spec
            .overrides
            .socket_plug_variants
            .iter()
            .find(|variant| usize::from(variant.socket_index) == socket);
        let expected_hash = if let Some(variant) = variant {
            verify_private_default(manager, tables, hash, variant.source_plug_hash);
            hash
        } else {
            column.choices[0]
        };
        assert_eq!(hash, expected_hash);
        let row = rows + socket * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        if let Some(kind) = column.socket_type {
            assert_eq!(read_u16(definition, row).unwrap(), kind);
        }
        let (count, _, choices, _) =
            array_at(definition, row + ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET).unwrap();
        assert_eq!(count, column.choices.len());
        for (choice, &expected) in column.choices.iter().enumerate() {
            let actual = tables.plug_hash(
                read_u16(
                    definition,
                    choices + choice * ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE,
                )
                .unwrap(),
            );
            assert_eq!(actual, if choice == 0 { expected_hash } else { expected });
        }
    }
}

fn verify_weapon(
    manager: &tiger_pkg::PackageManager,
    tables: &Tables,
    recipe: &WeaponRecipe,
) -> usize {
    let variants = verify_fields(manager, tables, recipe);
    let spec = recipe.to_spec().unwrap();
    let (definition, _) = tables.load(manager, spec.identity.item_hash);
    verify_sockets(manager, tables, &definition, &spec);
    variants
}

fn verify_fields(
    manager: &tiger_pkg::PackageManager,
    tables: &Tables,
    recipe: &WeaponRecipe,
) -> usize {
    let spec = recipe.to_spec().unwrap();
    let (definition, strings) = tables.load(manager, spec.identity.item_hash);
    assert_eq!(
        read_u32(&definition, ITEM_DEFINITION_HASH_OFFSET).unwrap(),
        spec.identity.item_hash
    );
    assert_eq!(
        weapon_inventory_slot(&definition).unwrap(),
        spec.overrides.inventory_slot.unwrap()
    );
    assert_eq!(
        weapon_rarity(&definition).unwrap(),
        spec.overrides.rarity.unwrap()
    );
    let damage = spec.overrides.modern_damage_type.unwrap();
    assert_eq!(
        weapon_damage_descriptor(&definition).unwrap(),
        match damage {
            ModernDamageType::Kinetic => WeaponDamageDescriptor::Empty,
            damage => WeaponDamageDescriptor::Elemental(damage),
        }
    );
    let ammo = spec.overrides.ammo_type.unwrap();
    assert_eq!(item_string_ammo_type(&strings).unwrap(), Some(ammo));
    if let Some(group) = spec.overrides.stat_group_index {
        assert_eq!(item_string_stat_group_index(&strings).unwrap(), group);
    }
    for &(index, value) in &spec.overrides.investment_stats {
        let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET).unwrap();
        let (count, _, rows, _) = array_at(&definition, resource).unwrap();
        let row = (0..count)
            .map(|row| rows + row * ITEM_INVESTMENT_STAT_ROW_SIZE)
            .find(|row| read_u16(&definition, *row).unwrap() == index)
            .unwrap();
        assert_eq!(read_u32(&definition, row + 4).unwrap() as i32, value);
    }
    verify_ammo(manager, spec.identity.item_hash, ammo)
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES, PARHELION_COMBINATION_ROOT, and PARHELION_COMBINATION_PROFILE (0..35)"]
fn native_combat_combinations_round_trip() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    let profile: usize = std::env::var("PARHELION_COMBINATION_PROFILE")
        .unwrap()
        .parse()
        .unwrap();
    fs::create_dir_all(&output).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let recipes = recipes(&catalog, profile);
    let (build, variants) = run_batch(
        &packages,
        &output,
        &format!("profile-{profile:02}"),
        &recipes,
        verify_weapon,
    );
    eprintln!(
        "COMBINATION_PASS profile={profile} weapons={} ammo_variants={variants} stage={}",
        recipes.len(),
        build.run_directory.display()
    );
}

fn run_batch(
    packages: &Path,
    output: &Path,
    label: &str,
    recipes: &[WeaponRecipe],
    mut verify: impl FnMut(&tiger_pkg::PackageManager, &Tables, &WeaponRecipe) -> usize,
) -> (crate::BuildReport, usize) {
    fs::write(
        output.join(format!("{label}.json")),
        serde_json::to_vec_pretty(recipes).unwrap(),
    )
    .unwrap();
    let stock = open_manager(packages).unwrap();
    let before = Tables::read(&stock);
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.to_path_buf(),
        staging_root: output.join(label),
        ignore_installed_authored_overlays: true,
        recipes: recipes.to_vec(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |progress| {
        if progress.current_artifact.is_none() {
            eprintln!("{label}: {}", progress.phase.label());
        }
    })
    .unwrap();
    assert_eq!(build.weapons.len(), recipes.len());
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(packages, &ignored).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let manager = open_manager(view.path()).unwrap();
    let tables = Tables::read(&manager);
    let (count, _, rows, _) = array_at(&before.items, 8).unwrap();
    assert_eq!(
        &before.items[rows..rows + count * ITEM_ROW_SIZE],
        &tables.items[tables.item_rows..tables.item_rows + count * ITEM_ROW_SIZE]
    );
    let mut variants = 0;
    for recipe in recipes {
        eprintln!("Verifying {}", recipe.name);
        variants += verify(&manager, &tables, recipe);
        let hash = recipe.donor.item_hash.parse_u32().unwrap();
        assert_eq!(
            before.load(&stock, hash),
            tables.load(&manager, hash),
            "stock donor changed"
        );
    }
    let manifest: crate::manifest::ManifestDocument =
        serde_json::from_slice(&fs::read(&build.manifest_path).unwrap()).unwrap();
    manifest.validate().unwrap();
    for artifact in &manifest.source_artifacts {
        let digest = crate::artifact::digest_file(&packages.join(&artifact.file_name)).unwrap();
        assert_eq!(digest.byte_length, artifact.byte_length);
        assert_eq!(digest.sha256, artifact.sha256);
    }
    eprintln!(
        "BATCH_PASS label={label} weapons={} ammo_variants={variants} stage={}",
        recipes.len(),
        build.run_directory.display()
    );
    (build, variants)
}
