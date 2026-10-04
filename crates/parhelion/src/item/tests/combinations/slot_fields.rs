//! Stock Trust and Polaris Lance keep an Energy bucket with a Kinetic equipment slot and item
//! string. Slot, ammo and damage type must still author independently, and every output must
//! name one slot in all three fields. Cerberus+1 has no elemental carrier and no stock elemental
//! peer on its runtime entity, so its element can only come from the presentation donor.
use super::*;

const TRUST_BLANK_AMMO: u32 = 0x9E02_3464;
const TRUST: u32 = 0x2979_48F4;
const POLARIS_LANCE: u32 = 0x3735_6D87;
const WINTER_WOLF: u32 = 0x6876_536E;
const CERBERUS_PLUS_ONE: u32 = 0x5BDB_CC56;

/// A stock Energy weapon of Cerberus+1's type whose element is a type-68 socket plug.
fn socket_driven_peer(stock: &sundial::package_authoring::PackageManager, source: &Tables) -> u32 {
    let (_, cerberus) = source.load(stock, CERBERUS_PLUS_ONE);
    let kind = &cerberus[ITEM_TYPE_REFERENCE_OFFSET..ITEM_TYPE_REFERENCE_OFFSET + 8];
    source
        .indices
        .iter()
        .find_map(|(&hash, &index)| {
            let read = |table: &[u8], rows: usize| {
                let tag = read_u32(table, rows + index * ITEM_ROW_SIZE + 16).ok()?;
                stock.read_tag(TagHash(tag)).ok()
            };
            let strings = read(&source.strings, source.string_rows)?;
            if strings.get(ITEM_TYPE_REFERENCE_OFFSET..ITEM_TYPE_REFERENCE_OFFSET + 8) != Some(kind)
            {
                return None;
            }
            let definition = read(&source.items, source.item_rows)?;
            (weapon_inventory_slot(&definition).ok()? == WeaponInventorySlot::Energy
                && weapon_damage_carrier(&definition).ok()?.family()
                    == Some(WeaponDamageCarrierFamily::PlugDriven))
            .then_some(hash)
        })
        .expect("the stock set has an Energy weapon of Cerberus+1's type with a type-68 carrier")
}

const SLOTS: [RecipeInventorySlot; 3] = [
    RecipeInventorySlot::Kinetic,
    RecipeInventorySlot::Energy,
    RecipeInventorySlot::Power,
];
const AMMO: [RecipeAmmoType; 3] = [
    RecipeAmmoType::Primary,
    RecipeAmmoType::Special,
    RecipeAmmoType::Heavy,
];
const DAMAGE: [RecipeDamageType; 3] = [
    RecipeDamageType::Kinetic,
    RecipeDamageType::Solar,
    RecipeDamageType::Void,
];

fn recipe(name: &str, donor: u32, presentation: Option<u32>) -> WeaponRecipe {
    let mut recipe = WeaponRecipe::from_json_str(include_str!(
        "../../../../recipes/good-company.parhelion.json"
    ))
    .unwrap();
    recipe.overrides = Default::default();
    recipe.donor.item_hash = format!("0x{donor:08X}").parse().unwrap();
    recipe.donor.expected_name = None;
    recipe.presentation_donor = presentation.map(|hash| crate::recipe::WeaponDonorReference {
        item_hash: format!("0x{hash:08X}").parse().unwrap(),
        expected_name: None,
    });
    recipe.rename_authored_item(name).unwrap();
    recipe
}

fn author(
    mut recipe: WeaponRecipe,
    slot: RecipeInventorySlot,
    ammo: RecipeAmmoType,
    damage: RecipeDamageType,
    rarity: RecipeRarity,
) -> WeaponRecipe {
    recipe.overrides.inventory_slot = Some(slot);
    recipe.overrides.ammo_type = Some(ammo);
    recipe.overrides.modern_damage_type = Some(damage);
    recipe.overrides.rarity = Some(rarity);
    recipe
}

fn assert_one_slot(definition: &[u8], strings: &[u8], slot: WeaponInventorySlot, name: &str) {
    assert_eq!(
        weapon_inventory_slot(definition).unwrap(),
        slot,
        "{name} bucket"
    );
    assert_eq!(
        weapon_equipment_slot(definition).unwrap(),
        slot,
        "{name} equipment slot"
    );
    item_string_client_classification(strings, slot)
        .unwrap_or_else(|error| panic!("{name} item string: {error}"));
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_COMBINATION_ROOT"]
fn native_split_slot_donors_author_slot_ammo_and_damage_independently() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_COMBINATION_ROOT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let stock = open_manager(&packages).unwrap();
    let source = Tables::read(&stock);
    for donor in [TRUST_BLANK_AMMO, TRUST, POLARIS_LANCE] {
        let (definition, _) = source.load(&stock, donor);
        assert_ne!(
            weapon_inventory_slot(&definition).unwrap(),
            weapon_equipment_slot(&definition).unwrap(),
            "0x{donor:08X} is no longer a split-slot donor"
        );
    }

    // Every slot/ammo and slot/damage pair appears once.
    let mut recipes = (0..9)
        .map(|index| {
            let (slot, ammo) = (index / 3, index % 3);
            author(
                recipe(&format!("Split Slot Trust {index}"), TRUST_BLANK_AMMO, None),
                SLOTS[slot],
                AMMO[ammo],
                DAMAGE[(slot + ammo) % 3],
                RecipeRarity::Legendary,
            )
        })
        .collect::<Vec<_>>();
    recipes.push(author(
        recipe("Split Slot Polaris", POLARIS_LANCE, None),
        RecipeInventorySlot::Kinetic,
        RecipeAmmoType::Primary,
        RecipeDamageType::Solar,
        RecipeRarity::Exotic,
    ));
    recipes.push(author(
        recipe("Split Slot Appearance", WINTER_WOLF, Some(POLARIS_LANCE)),
        // Appearance policy keeps Energy appearances out of Power. Kinetic still moves the tuple.
        RecipeInventorySlot::Kinetic,
        RecipeAmmoType::Heavy,
        RecipeDamageType::Void,
        RecipeRarity::Legendary,
    ));
    // No overrides: the donor's bucket names the slot and its native ammo stays.
    let untouched = recipe("Split Slot Native", TRUST, None);
    let untouched_hash = untouched.to_spec().unwrap().identity.item_hash;
    recipes.push(untouched);
    // A socket-driven presentation donor proves the element for a Kinetic exotic behavior
    // donor; the built definition takes the modern fixed marker, since no type-68 lane can be
    // added to a definition without one.
    let socket_driven = socket_driven_peer(&stock, &source);
    eprintln!("SPLIT_SLOT_SOCKET_DRIVEN_DONOR 0x{socket_driven:08X}");
    let cerberus = author(
        recipe(
            "Split Slot Cerberus",
            CERBERUS_PLUS_ONE,
            Some(socket_driven),
        ),
        RecipeInventorySlot::Energy,
        RecipeAmmoType::Primary,
        RecipeDamageType::Solar,
        RecipeRarity::Exotic,
    );
    let cerberus_hash = cerberus.to_spec().unwrap().identity.item_hash;
    recipes.push(cerberus);

    let (build, variants) = run_batch(
        &packages,
        &output,
        "split-slot",
        &recipes,
        |manager, tables, recipe| {
            let spec = recipe.to_spec().unwrap();
            let (definition, strings) = tables.load(manager, spec.identity.item_hash);
            if spec.identity.item_hash == cerberus_hash {
                assert_eq!(
                    weapon_damage_carrier(&definition).unwrap(),
                    WeaponDamageCarrier::Fixed {
                        family: WeaponDamageCarrierFamily::ModernFixed,
                        damage_type: ModernDamageType::Solar,
                    },
                    "{} carrier",
                    recipe.name
                );
            }
            if spec.identity.item_hash == untouched_hash {
                let (donor, donor_strings) = source.load(&stock, TRUST);
                let bucket = weapon_inventory_slot(&donor).unwrap();
                assert_one_slot(&definition, &strings, bucket, &recipe.name);
                assert_eq!(
                    item_string_ammo_type(&strings).unwrap(),
                    item_string_ammo_type(&donor_strings).unwrap()
                );
                assert_eq!(
                    weapon_damage_descriptor(&definition).unwrap(),
                    weapon_damage_descriptor(&donor).unwrap()
                );
                return 0;
            }
            let slot = spec.overrides.inventory_slot.unwrap();
            assert_one_slot(&definition, &strings, slot, &recipe.name);
            verify_fields(manager, tables, recipe)
        },
    );
    eprintln!(
        "SPLIT_SLOT_PASS weapons={} ammo_variants={variants} stage={}",
        recipes.len(),
        build.run_directory.display()
    );
}
