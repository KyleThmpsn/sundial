//! Recipe mutations shared by socket controls and the custom-perk windows.
//! A socket position alone is not an identity: a changed source plug invalidates
//! the old editor key. Keep lookup, insertion, reset and reconciliation together.
use super::*;

#[cfg(test)]
fn matches_variant(variant: &WeaponSocketPlugVariantRecipe, key: PerkEditorKey) -> bool {
    variant.socket_index == key.socket_index
        && variant.choice_index == key.choice_index
        && variant.source_plug_hash.parse_u32().ok() == Some(key.source_plug_hash)
}

#[cfg(test)]
pub(in crate::app) fn private_perk(
    recipe: &WeaponRecipe,
    key: PerkEditorKey,
) -> Option<&WeaponSandboxPerkRuntimeRecipe> {
    recipe
        .overrides
        .socket_plug_variants
        .iter()
        .find(|variant| matches_variant(variant, key))
        .and_then(|variant| {
            variant
                .sandbox_perks
                .iter()
                .find(|perk| perk.source_perk_index == key.source_perk_index)
        })
}

#[cfg(test)]
pub(in crate::app) fn private_perk_runtime_values(
    recipe: &WeaponRecipe,
    key: PerkEditorKey,
) -> Option<&Vec<WeaponRuntimeValueOverride>> {
    private_perk(recipe, key).map(|perk| &perk.runtime_values)
}

#[cfg(test)]
fn private_perk_mut(
    recipe: &mut WeaponRecipe,
    key: PerkEditorKey,
) -> Option<&mut WeaponSandboxPerkRuntimeRecipe> {
    recipe
        .overrides
        .socket_plug_variants
        .iter_mut()
        .find(|variant| matches_variant(variant, key))
        .and_then(|variant| {
            variant
                .sandbox_perks
                .iter_mut()
                .find(|perk| perk.source_perk_index == key.source_perk_index)
        })
}

#[cfg(test)]
pub(in crate::app) fn upsert_private_perk_runtime_values(
    recipe: &mut WeaponRecipe,
    key: PerkEditorKey,
    values: Vec<WeaponRuntimeValueOverride>,
) -> &mut WeaponSandboxPerkRuntimeRecipe {
    let position = (key.socket_index, key.choice_index);
    let variant_index = recipe
        .overrides
        .socket_plug_variants
        .iter()
        .position(|variant| (variant.socket_index, variant.choice_index) == position);
    let variant_index = variant_index.unwrap_or_else(|| {
        recipe
            .overrides
            .socket_plug_variants
            .push(WeaponSocketPlugVariantRecipe {
                replace_effects: false,
                investment_stats: Vec::new(),
                socket_index: key.socket_index,
                choice_index: key.choice_index,
                source_plug_hash: HexHash::new(key.source_plug_hash),
                name: None,
                classification_donor_hash: None,
                description: None,
                additional_sandbox_perks: Vec::new(),
                sandbox_perks: Vec::new(),
            });
        recipe.overrides.socket_plug_variants.len() - 1
    });
    let variant = &mut recipe.overrides.socket_plug_variants[variant_index];
    if variant.source_plug_hash.parse_u32().ok() != Some(key.source_plug_hash) {
        variant.source_plug_hash = HexHash::new(key.source_plug_hash);
        variant.sandbox_perks.clear();
    }
    if let Some(perk) = variant
        .sandbox_perks
        .iter_mut()
        .find(|perk| perk.source_perk_index == key.source_perk_index)
    {
        perk.runtime_values = values;
    } else {
        variant.sandbox_perks.push(WeaponSandboxPerkRuntimeRecipe {
            program: None,
            projectiles: Vec::new(),
            source_perk_index: key.source_perk_index,
            activation: None,
            runtime_values: values,
            action_float_values: Vec::new(),
        });
    }
    if !variant.replace_effects {
        variant
            .sandbox_perks
            .sort_unstable_by_key(|perk| perk.source_perk_index);
    }
    recipe
        .overrides
        .socket_plug_variants
        .sort_unstable_by_key(|variant| (variant.socket_index, variant.choice_index));
    private_perk_mut(recipe, key).expect("the custom perk was inserted before sorting")
}

// Legacy removal oracle retained for the existing recipe-preservation regressions.
// The UI now resets a parameter draft and explicitly applies it instead.
#[cfg(test)]
pub(in crate::app) fn remove_private_perk_runtime_values(
    recipe: &mut WeaponRecipe,
    key: PerkEditorKey,
) {
    let Some(variant_index) = recipe
        .overrides
        .socket_plug_variants
        .iter()
        .position(|variant| matches_variant(variant, key))
    else {
        return;
    };
    let variant = &mut recipe.overrides.socket_plug_variants[variant_index];
    let keep_presentation = variant.name.is_some()
        || !variant.investment_stats.is_empty()
        || variant.description.is_some()
        || variant.classification_donor_hash.is_some()
        || !variant.additional_sandbox_perks.is_empty();
    if keep_presentation && variant.sandbox_perks.len() == 1 {
        if let Some(perk) = variant
            .sandbox_perks
            .iter_mut()
            .find(|perk| perk.source_perk_index == key.source_perk_index)
        {
            perk.runtime_values.clear();
            perk.action_float_values.clear();
            perk.projectiles.clear();
            perk.activation = None;
        }
        return;
    }
    variant
        .sandbox_perks
        .retain(|perk| perk.source_perk_index != key.source_perk_index);
    if variant.sandbox_perks.is_empty() {
        recipe.overrides.socket_plug_variants.remove(variant_index);
    }
}

pub(in crate::app) fn reconcile_socket_plug_variants(
    recipe: &mut WeaponRecipe,
    socket_index: usize,
    choices: &[u32],
    removed_choice: Option<usize>,
) {
    if let Some(removed_choice) = removed_choice {
        recipe.overrides.socket_plug_variants.retain(|variant| {
            usize::from(variant.socket_index) != socket_index
                || usize::from(variant.choice_index) != removed_choice
        });
    }
    for variant in &mut recipe.overrides.socket_plug_variants {
        if usize::from(variant.socket_index) != socket_index {
            continue;
        }
        if let Some(removed_choice) = removed_choice {
            let choice_index = usize::from(variant.choice_index);
            if choice_index > removed_choice {
                variant.choice_index = variant.choice_index.saturating_sub(1);
            }
        }
    }
    recipe.overrides.socket_plug_variants.retain(|variant| {
        if usize::from(variant.socket_index) != socket_index {
            return true;
        }
        let choice_index = usize::from(variant.choice_index);
        choices.get(choice_index).copied() == variant.source_plug_hash.parse_u32().ok()
    });
}

#[cfg(test)]
mod tests;
