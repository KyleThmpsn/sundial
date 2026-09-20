//! Reload-hold element switching: The Fundamentals grafted onto another body.
//!
//! Proven in game on 2026-09-16. A SUROS Regime body with Hard Light as its appearance donor and
//! Hard Light's Fundamentals plug in its trait socket stepped Void, Arc, Solar under a held Reload
//! exactly like stock Hard Light: the plug's three action-state records flipped and the HUD pip
//! followed. Nothing else was needed, not Hard Light's item hash, runtime entity, pattern
//! selector, base perks or its declaration-only intrinsic perk 479. The hold follows the gear-art
//! row the presentation donor supplies, and the element change is the plug's three
//! predicate-gated Set Host Mode effects.
use super::*;

/// Hard Light, the weapon whose record drives the reload hold.
#[cfg(test)]
pub(crate) const HARD_LIGHT_ITEM_HASH: u32 = 0xF5DE_4480;
/// Hard Light's Fundamentals plug: stock rows 462 (Arc), 463 (Solar) and 464 (Void).
pub(crate) const FUNDAMENTALS_PLUG_HASH: u32 = 0x9C33_04DA;
/// The native socket type of weapon trait sockets, the lane the plug is pinned into.
pub(crate) const TRAIT_SOCKET_TYPE: u16 = 92;
/// The elements in the order the hold steps through them: selector 0, 1, then 2.
pub(crate) const SELECTOR_ORDER: [ModernDamageType; 3] = [
    ModernDamageType::Void,
    ModernDamageType::Arc,
    ModernDamageType::Solar,
];

/// The stock finished-perk row on the plug that sets one element.
#[must_use]
pub(crate) const fn element_perk_index(element: ModernDamageType) -> Option<u16> {
    match element {
        ModernDamageType::Void => Some(464),
        ModernDamageType::Arc => Some(462),
        ModernDamageType::Solar => Some(463),
        ModernDamageType::Kinetic => None,
    }
}

#[must_use]
pub(crate) const fn element_label(element: ModernDamageType) -> &'static str {
    match element {
        ModernDamageType::Kinetic => "Kinetic",
        ModernDamageType::Arc => "Arc",
        ModernDamageType::Solar => "Solar",
        ModernDamageType::Void => "Void",
    }
}

/// The element the weapon rests on: the first chosen element in selector order.
#[must_use]
pub(crate) fn resting_element(elements: &[ModernDamageType]) -> Option<ModernDamageType> {
    SELECTOR_ORDER
        .into_iter()
        .find(|element| elements.contains(element))
}

/// The chosen elements in selector order, without repeats.
#[must_use]
pub(crate) fn ordered_elements(elements: &[ModernDamageType]) -> Vec<ModernDamageType> {
    SELECTOR_ORDER
        .into_iter()
        .filter(|element| elements.contains(element))
        .collect()
}

pub(crate) fn validate_elements(elements: &[ModernDamageType]) -> AuthoringResult<()> {
    if elements
        .iter()
        .any(|element| element_perk_index(*element).is_none())
    {
        return Err(invalid(
            "This Fundamentals-based switcher cycles Arc, Solar and Void. Use a Perk Workbench Set Damage Type action to author Kinetic damage.",
        ));
    }
    let unique = ordered_elements(elements);
    if unique.len() != elements.len() {
        return Err(invalid("Variable damage lists an element twice."));
    }
    if unique.len() < 2 {
        return Err(invalid(
            "Variable damage needs at least two elements. One element is a fixed damage type.",
        ));
    }
    Ok(())
}

pub(super) fn validate_spec(spec: &WeaponCloneSpec) -> AuthoringResult<()> {
    let Some(variable) = &spec.overrides.variable_damage else {
        return Ok(());
    };
    validate_elements(&variable.elements)?;
    if let Some(resting) = spec.overrides.modern_damage_type
        && !variable.elements.contains(&resting)
    {
        return Err(invalid(format!(
            "Variable damage rests on {}, which is not one of its elements.",
            element_label(resting)
        )));
    }
    Ok(())
}

/// The expanded spec when the weapon switches elements, or `None` when nothing changes.
pub(super) fn expand_spec(
    spec: &WeaponCloneSpec,
    definition: &[u8],
) -> AuthoringResult<Option<WeaponCloneSpec>> {
    if spec.overrides.variable_damage.is_none() {
        return Ok(None);
    }
    let mut expanded = spec.clone();
    expanded.overrides = expand_overrides(&spec.overrides, &weapon_socket_types(definition)?)?;
    Ok(Some(expanded))
}

/// Pins The Fundamentals into the first trait socket and, for a subset of elements, adds a
/// private variant of the plug carrying only the chosen stock rows.
pub(crate) fn expand_overrides(
    overrides: &WeaponCloneOverrides,
    socket_types: &[u16],
) -> AuthoringResult<WeaponCloneOverrides> {
    let Some(variable) = &overrides.variable_damage else {
        return Ok(overrides.clone());
    };
    validate_elements(&variable.elements)?;
    // An author can turn one of the donor's columns into a trait socket, or append one, and can
    // put The Fundamentals there themselves. Read the lanes the way the socket list draws them
    // and use the socket they chose, so the build does not lead a second lane with the same perk.
    let lanes = crate::weapon_behavior::effective_socket_types(
        &crate::weapon_behavior::authored_socket_roles(overrides),
        socket_types,
    );
    let trait_lanes = || {
        lanes
            .iter()
            .enumerate()
            .filter(|(_, socket_type)| **socket_type == TRAIT_SOCKET_TYPE)
            .map(|(lane, _)| lane)
    };
    let placed = crate::weapon_behavior::authored_socket_choices(overrides);
    let lane = trait_lanes()
        .find(|lane| {
            placed
                .get(*lane)
                .is_some_and(|choices| choices.contains(&FUNDAMENTALS_PLUG_HASH))
        })
        .or_else(|| trait_lanes().next())
        .ok_or_else(|| {
            invalid("Variable damage needs a trait socket, and this weapon has none.")
        })?;
    let socket_index = u16::try_from(lane)
        .map_err(|_| invalid("Variable damage trait socket index does not fit 16 bits"))?;
    let mut expanded = overrides.clone();
    // The reload hold is carried by a behavior record in the weapon family's shared content
    // owner, not by the carrier's gear art. Request it by family so the compiler picks Hard
    // Light's record for rifles and Borealis's for snipers.
    if !expanded
        .additional_behaviors
        .iter()
        .any(|id| id == crate::weapon_behavior::ELEMENT_SWITCH)
    {
        expanded
            .additional_behaviors
            .push(crate::weapon_behavior::ELEMENT_SWITCH.to_owned());
    }
    if expanded.socket_columns.is_empty() {
        expanded.socket_columns = vec![None; socket_types.len()];
    }
    if expanded.socket_columns.len() < socket_types.len() {
        return Err(invalid(format!(
            "The recipe lists {} socket columns but the base weapon has {} sockets.",
            expanded.socket_columns.len(),
            socket_types.len()
        )));
    }
    // The Fundamentals leads the lane rather than emptying it, so an author's own choices for
    // that socket stay behind it and the element still steps from the first plug.
    let column = expanded.socket_columns[lane].get_or_insert_with(|| WeaponSocketColumnOverride {
        choices: Vec::new(),
        socket_type: None,
        choice_weight_bits: Vec::new(),
        choice_conditions: Vec::new(),
        reusable_plug_set_index: None,
        randomized_plug_set_index: None,
        randomized_selection_program: Vec::new(),
    });
    column
        .choices
        .retain(|choice| *choice != FUNDAMENTALS_PLUG_HASH);
    column.choices.insert(0, FUNDAMENTALS_PLUG_HASH);
    if expanded
        .socket_plug_variants
        .iter()
        .any(|variant| variant.socket_index == socket_index && variant.choice_index == 0)
    {
        return Err(invalid(format!(
            "Variable damage owns socket {} (Trait), but the recipe also has a private plug variant there. Remove that variant or turn off variable damage.",
            lane + 1
        )));
    }
    let elements = ordered_elements(&variable.elements);
    if elements.len() < SELECTOR_ORDER.len() {
        let labels = elements
            .iter()
            .map(|element| element_label(*element))
            .collect::<Vec<_>>();
        expanded
            .socket_plug_variants
            .push(WeaponSocketPlugVariantOverride {
                replace_effects: true,
                investment_stats: Vec::new(),
                socket_index,
                choice_index: 0,
                source_plug_hash: FUNDAMENTALS_PLUG_HASH,
                name: Some(format!("The Fundamentals ({})", labels.join(" / "))),
                icon: None,
                classification_donor_hash: None,
                description: Some(format!(
                    "Hold Reload to step the weapon's damage type through {}.",
                    labels.join(", ")
                )),
                additional_sandbox_perks: Vec::new(),
                sandbox_perks: elements
                    .iter()
                    .map(|element| WeaponSandboxPerkRuntimeOverride {
                        program: None,
                        projectiles: Vec::new(),
                        source_perk_index: element_perk_index(*element)
                            .expect("validated elemental rows"),
                        activation: None,
                        runtime_values: Vec::new(),
                        action_float_values: Vec::new(),
                    })
                    .collect(),
            });
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOCKET_TYPES: [u16; 8] = [176, 65, 177, TRAIT_SOCKET_TYPE, 183, u16::MAX, 454, 448];

    fn overrides(elements: &[ModernDamageType]) -> WeaponCloneOverrides {
        WeaponCloneOverrides {
            variable_damage: Some(WeaponVariableDamage {
                elements: elements.to_vec(),
            }),
            ..WeaponCloneOverrides::default()
        }
    }

    fn column(choice: u32) -> Option<WeaponSocketColumnOverride> {
        Some(WeaponSocketColumnOverride {
            choices: vec![choice],
            socket_type: None,
            choice_weight_bits: Vec::new(),
            choice_conditions: Vec::new(),
            reusable_plug_set_index: None,
            randomized_plug_set_index: None,
            randomized_selection_program: Vec::new(),
        })
    }

    #[test]
    fn every_element_pins_the_stock_plug_without_a_variant() {
        let expanded = expand_overrides(&overrides(&SELECTOR_ORDER), &SOCKET_TYPES).unwrap();
        assert_eq!(expanded.socket_columns.len(), SOCKET_TYPES.len());
        assert_eq!(expanded.socket_columns[3], column(FUNDAMENTALS_PLUG_HASH));
        assert!(
            expanded
                .socket_columns
                .iter()
                .enumerate()
                .all(|(lane, column)| lane == 3 || column.is_none())
        );
        assert!(expanded.socket_plug_variants.is_empty());
    }

    #[test]
    fn a_subset_adds_a_private_variant_in_selector_order() {
        let expanded = expand_overrides(
            &overrides(&[ModernDamageType::Solar, ModernDamageType::Void]),
            &SOCKET_TYPES,
        )
        .unwrap();
        let [variant] = expanded.socket_plug_variants.as_slice() else {
            panic!("one variant");
        };
        assert!(variant.replace_effects);
        assert_eq!((variant.socket_index, variant.choice_index), (3, 0));
        assert_eq!(variant.source_plug_hash, FUNDAMENTALS_PLUG_HASH);
        assert_eq!(
            variant.name.as_deref(),
            Some("The Fundamentals (Void / Solar)")
        );
        assert_eq!(
            variant
                .sandbox_perks
                .iter()
                .map(|perk| perk.source_perk_index)
                .collect::<Vec<_>>(),
            [464, 463]
        );
        assert!(
            variant
                .sandbox_perks
                .iter()
                .all(|perk| perk.program.is_none())
        );
    }

    #[test]
    fn existing_columns_keep_their_other_lanes() {
        let mut source = overrides(&SELECTOR_ORDER);
        source.socket_columns = vec![None; SOCKET_TYPES.len()];
        source.socket_columns[1] = column(0x1234_5678);
        let expanded = expand_overrides(&source, &SOCKET_TYPES).unwrap();
        assert_eq!(expanded.socket_columns[1], source.socket_columns[1]);
        assert_eq!(expanded.socket_columns[3], column(FUNDAMENTALS_PLUG_HASH));
    }

    #[test]
    fn a_chosen_trait_lane_keeps_its_choices_behind_the_fundamentals() {
        let mut chosen = overrides(&SELECTOR_ORDER);
        chosen.socket_columns = vec![None; SOCKET_TYPES.len()];
        chosen.socket_columns[3] = column(0x1234_5678);
        let expanded = expand_overrides(&chosen, &SOCKET_TYPES).unwrap();
        assert_eq!(
            expanded.socket_columns[3]
                .as_ref()
                .map(|column| column.choices.as_slice()),
            Some([FUNDAMENTALS_PLUG_HASH, 0x1234_5678].as_slice())
        );
    }

    #[test]
    fn a_missing_trait_socket_is_rejected() {
        let mut occupied = overrides(&SELECTOR_ORDER);
        occupied
            .socket_plug_variants
            .push(WeaponSocketPlugVariantOverride {
                replace_effects: false,
                investment_stats: Vec::new(),
                socket_index: 3,
                choice_index: 0,
                source_plug_hash: 0x1234_5678,
                name: None,
                icon: None,
                classification_donor_hash: None,
                description: None,
                additional_sandbox_perks: Vec::new(),
                sandbox_perks: Vec::new(),
            });
        assert!(expand_overrides(&occupied, &SOCKET_TYPES).is_err());

        assert!(expand_overrides(&overrides(&SELECTOR_ORDER), &[176, 65, 177]).is_err());

        let mut narrow = overrides(&SELECTOR_ORDER);
        narrow.socket_columns = vec![None; 2];
        assert!(expand_overrides(&narrow, &SOCKET_TYPES).is_err());
    }

    #[test]
    fn element_sets_are_validated() {
        assert!(validate_elements(&SELECTOR_ORDER).is_ok());
        assert!(validate_elements(&[ModernDamageType::Arc, ModernDamageType::Void]).is_ok());
        assert!(validate_elements(&[ModernDamageType::Arc]).is_err());
        assert!(validate_elements(&[ModernDamageType::Arc, ModernDamageType::Arc]).is_err());
        assert!(validate_elements(&[ModernDamageType::Kinetic, ModernDamageType::Arc]).is_err());
        assert_eq!(
            resting_element(&[ModernDamageType::Solar, ModernDamageType::Arc]),
            Some(ModernDamageType::Arc)
        );
        assert_eq!(resting_element(&[]), None);
    }
}
