use super::*;

fn donor(hash: u32, name: &str, type_name: &str) -> WeaponDonorSummary {
    WeaponDonorSummary {
        hash,
        name: name.into(),
        type_name: type_name.into(),
        bucket_hash: 0,
        collection_backed: true,
        power_cap: None,
        damage_type: None,
        inventory_slot: None,
        ammo_type: None,
        weapon_pattern_index: None,
        weapon_translation_group: None,
        stat_group_index: None,
        damage_profile: WeaponDamageProfile::Unknown,
        rarity: WeaponRarity::Legendary,
    }
}

fn entry(name: &str, path: &str, donor_hash: u32) -> RecipeLibraryEntry {
    RecipeLibraryEntry {
        collection_destination: None,
        badge: None,
        corner_icon: None,
        path: path.into(),
        name: name.into(),
        namespace: format!("parhelion.{}", name.to_ascii_lowercase()),
        bundled: false,
        donor_hash,
        type_name: None,
        ammo_type: None,
        damage_type: None,
        rarity: None,
        icon_hash: donor_hash,
        icon_edit: crate::WeaponIconEdit::default(),
    }
}

#[test]
fn donor_cache_keeps_first_duplicate_and_refreshes_replacements() {
    let mut state = LibraryState::default();
    let entries = vec![entry("Alpha", "alpha.json", 7)];
    let donors = vec![
        donor(7, "First", "First Type"),
        donor(7, "Second", "Second Type"),
    ];

    state.refresh_donors(&donors);
    let matches = state.matching_entries(&entries, &donors, "first type");
    assert_eq!(matches.len(), 1);
    assert!(
        state
            .matching_entries(&entries, &donors, "second type")
            .is_empty()
    );
    let unrelated = vec![donor(8, "Unrelated", "Unrelated Type")];
    assert!(
        state
            .matching_entries(&entries, &unrelated, "first type")
            .is_empty()
    );

    let replacement = vec![donor(7, "Replacement", "Replacement Type")];
    state.refresh_donors(&replacement);
    assert_eq!(
        state
            .matching_entries(&entries, &replacement, "replacement type")
            .len(),
        1
    );
}

#[test]
fn clearing_donor_cache_preserves_recipe_metadata_for_filter_and_sort() {
    let mut state = LibraryState::default();
    let mut authored = entry("Authored", "authored.json", 7);
    authored.type_name = Some("Authored Type".into());
    let fallback = entry("Fallback", "fallback.json", 8);
    let entries = vec![authored, fallback];
    let donors = vec![donor(7, "A", "Donor Type"), donor(8, "B", "Other Type")];

    state.refresh_donors(&donors);
    state.sort = SortOrder::WeaponType;
    let mut shown = state.matching_entries(&entries, &donors, "");
    state.sort_entries(&mut shown, &donors);
    assert_eq!(shown[0].0.name, "Authored");
    assert_eq!(shown[1].0.name, "Fallback");

    state.refresh_donors(&[]);
    assert_eq!(
        state.matching_entries(&entries, &[], "authored type").len(),
        1
    );
    assert!(
        state
            .matching_entries(&entries, &[], "donor type")
            .is_empty()
    );
}
