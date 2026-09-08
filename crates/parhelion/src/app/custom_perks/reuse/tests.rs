use super::*;

fn saved_perk(hash: u32) -> WeaponSocketPlugVariantRecipe {
    serde_json::from_value(serde_json::json!({
        "socket_index": 3, "choice_index": 1,
        "source_plug_hash": format!("0x{hash:08X}"),
        "name": "Saved Custom Perk", "description": "Keep this description.",
        "additional_sandbox_perks": [421],
        "sandbox_perks": [{"source_perk_index": 421, "runtime_values": []}]
    }))
    .unwrap()
}

#[test]
fn release_gate_keeps_saved_perks_and_deduplicates_reuse_choices() {
    assert!(!authoring_available());
    let mut recipe = WeaponRecipe::new_weapon("parhelion.saved-perk").unwrap();
    recipe
        .overrides
        .socket_plug_variants
        .push(saved_perk(0x12345678));
    let before = recipe.clone();
    let encoded = serde_json::to_string(&recipe).unwrap();
    let reloaded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(before, reloaded);
    let mut picker = ReusePicker::load(None, &recipe);
    picker.add_recipe(&reloaded);
    assert_eq!(picker.entries.len(), 1);
    assert_eq!(picker.entries[0].variant.socket_index, 0);
    assert_eq!(recipe, before);
    assert_eq!(
        picker.entries[0].variant.sandbox_perks,
        before.overrides.socket_plug_variants[0].sandbox_perks
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES; read-only native catalog"]
fn native_saved_custom_perk_reuse_is_independent_and_keeps_recipe_data() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let donor = catalog
        .weapon_donor_with_stat_group_index(0x23DB_942F, None)
        .unwrap();
    let socket = &donor.sockets[0];
    let inherited = inherited_socket_choices(
        socket.native_default,
        &socket.ordered_embedded_choices,
        authored_socket_choice_limit(socket.socket_type),
    );
    let source = saved_perk(inherited[0]);
    let before = source.clone();
    let mut target =
        WeaponRecipe::new_weapon_for_donor("parhelion.reused-perk", 0x23DB_942F, "Age-Old Bond")
            .unwrap();
    apply_saved_perk(&mut target, &donor, 0, &source).unwrap();
    let mut expected = source.clone();
    expected.socket_index = 0;
    expected.choice_index = 0;
    assert_eq!(target.overrides.socket_plug_variants, vec![expected]);
    assert_eq!(source, before);
    assert_eq!(
        recipe_socket_choices(&target, 0, &inherited).unwrap()[0],
        inherited[0]
    );
    let roundtrip = WeaponRecipe::from_json_str(&serde_json::to_string(&target).unwrap()).unwrap();
    assert_eq!(roundtrip, target);
    let unchanged = target.clone();
    assert!(apply_saved_perk(&mut target, &donor, usize::MAX, &source).is_err());
    assert_eq!(target, unchanged);

    let added_index = donor.sockets.len();
    assert!(added_index < sundial::investment::MAX_WEAPON_SOCKETS);
    target
        .overrides
        .socket_columns
        .resize_with(added_index, || None);
    target
        .overrides
        .socket_columns
        .push(Some(crate::WeaponSocketColumnRecipe {
            socket_type: Some(socket.socket_type),
            ..Default::default()
        }));
    apply_saved_perk(&mut target, &donor, added_index, &source).unwrap();
    assert_eq!(
        target.overrides.socket_columns[added_index]
            .as_ref()
            .unwrap()
            .socket_type,
        Some(socket.socket_type)
    );
    assert_eq!(
        recipe_socket_choices(&target, added_index, &[]).unwrap(),
        vec![inherited[0]]
    );
    assert_eq!(
        target.overrides.socket_plug_variants[1].socket_index as usize,
        added_index
    );
    assert_eq!(
        target.overrides.socket_plug_variants[1].sandbox_perks,
        source.sandbox_perks
    );
    assert_eq!(source, before);
}
