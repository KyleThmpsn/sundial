//! Turn installed custom-plug selections into portable recipe data before saving a socket.
use super::*;

pub(in crate::app) fn resolve_picked_perk(
    library: Option<&RecipeLibrary>,
    draft: Option<&WeaponRecipe>,
    catalog: &InvestmentCatalog,
    hash: u32,
) -> Result<Option<WeaponSocketPlugVariantRecipe>, String> {
    let Some(tag) = catalog.item_definition_tag(hash) else {
        return Ok(None);
    };
    if crate::package_profile::is_stock_item_definition(tag) {
        return Ok(None);
    }
    let mut recipes = draft.cloned().into_iter().collect::<Vec<_>>();
    if let Some(library) = library {
        let scan = library.scan()?;
        for entry in scan.entries {
            let recipe = WeaponRecipe::load_json(&entry.path).map_err(|error| error.to_string())?;
            if draft.is_none_or(|draft| recipe.namespace != draft.namespace) {
                recipes.push(recipe);
            }
        }
    }
    let mut donors = BTreeMap::new();
    find_picked_perk(&recipes, hash, |item_hash, socket_index, choice_index| {
        let donor = donors
            .entry(item_hash)
            .or_insert_with(|| catalog.weapon_donor(item_hash))
            .as_ref()?;
        let socket = donor.sockets.get(usize::from(socket_index))?;
        inherited_socket_choices(
            socket.native_default,
            &socket.ordered_embedded_choices,
            authored_socket_choice_limit(socket.socket_type),
        )
        .get(usize::from(choice_index))
        .copied()
    })
    .map(Some)
}

fn find_picked_perk(
    recipes: &[WeaponRecipe],
    hash: u32,
    mut installed_choice: impl FnMut(u32, u16, u16) -> Option<u32>,
) -> Result<WeaponSocketPlugVariantRecipe, String> {
    // Follow native socket bindings instead of guessing a name or an allocation nonce.
    // A shared plug remains reusable through any installed weapon that supplies it.
    let mut found: Option<WeaponSocketPlugVariantRecipe> = None;
    for recipe in recipes {
        let item_hash = recipe
            .identity
            .item_hash
            .parse_u32()
            .map_err(|error| error.to_string())?;
        for variant in &recipe.overrides.socket_plug_variants {
            if installed_choice(item_hash, variant.socket_index, variant.choice_index) == Some(hash)
            {
                let mut candidate = variant.clone();
                candidate.socket_index = 0;
                candidate.choice_index = 0;
                if found.as_ref().is_some_and(|previous| {
                    let mut previous = previous.clone();
                    previous.classification_donor_hash =
                        candidate.classification_donor_hash.clone();
                    previous != candidate
                }) {
                    return Err(format!(
                        "Custom perk 0x{hash:08X} matches conflicting recipe definitions. Resolve the source recipes before selecting it."
                    ));
                }
                found.get_or_insert(candidate);
            }
        }
    }
    if let Some(found) = found {
        return Ok(found);
    }
    Err(format!(
        "The recipe data for custom perk 0x{hash:08X} is unavailable. Import its source recipe, then select the perk again."
    ))
}

pub(in crate::app) fn attach_picked_perk(
    recipe: &mut WeaponRecipe,
    socket_index: usize,
    choice_index: usize,
    mut variant: WeaponSocketPlugVariantRecipe,
) {
    variant.socket_index = socket_index as u16;
    variant.choice_index = choice_index as u16;
    recipe.overrides.socket_plug_variants.retain(|existing| {
        usize::from(existing.socket_index) != socket_index
            || usize::from(existing.choice_index) != choice_index
    });
    recipe.overrides.socket_plug_variants.push(variant);
}

pub(in crate::app) fn choice_conflicts(
    recipe: &WeaponRecipe,
    socket: usize,
    choice: usize,
    hash: u32,
    private: Option<&WeaponSocketPlugVariantRecipe>,
    choices: &[u32],
) -> bool {
    choices.iter().copied().enumerate().any(|(index, current)| {
        if index == choice || current != hash {
            return false;
        }
        let existing = recipe
            .overrides
            .socket_plug_variants
            .iter()
            .find(|variant| {
                usize::from(variant.socket_index) == socket
                    && usize::from(variant.choice_index) == index
                    && variant.source_plug_hash.parse_u32().ok() == Some(hash)
            });
        match (existing, private) {
            (None, None) => true,
            (Some(existing), Some(private)) => {
                let mut private = private.clone();
                private.socket_index = existing.socket_index;
                private.choice_index = existing.choice_index;
                *existing == private
            }
            _ => false,
        }
    })
}

/// Recover socket choices saved by older pickers without overwriting any authored edits.
pub(in crate::app) fn repair_socket_picks(
    library: Option<&RecipeLibrary>,
    catalog: &InvestmentCatalog,
    recipe: &mut WeaponRecipe,
) -> Result<usize, String> {
    let mut repaired = recipe.clone();
    let mut count = 0;
    for (socket, column) in recipe.overrides.socket_columns.iter().enumerate() {
        let Some(column) = column else {
            continue;
        };
        for (choice, hash) in column.choices.iter().enumerate() {
            let hash = hash.parse_u32().map_err(|error| error.to_string())?;
            if catalog
                .item_definition_tag(hash)
                .is_none_or(crate::package_profile::is_stock_item_definition)
            {
                continue;
            }
            if recipe.overrides.socket_plug_variants.iter().any(|variant| {
                usize::from(variant.socket_index) == socket
                    && usize::from(variant.choice_index) == choice
            }) {
                return Err(format!(
                    "Socket {} choice {} customizes an installed custom plug directly. Reselect its source perk before applying new edits.",
                    socket + 1,
                    choice + 1
                ));
            }
            let private = resolve_picked_perk(library, Some(recipe), catalog, hash)?
                .ok_or_else(|| format!("Could not resolve custom perk 0x{hash:08X}"))?;
            repaired.overrides.socket_columns[socket]
                .as_mut()
                .expect("existing column")
                .choices[choice] = private.source_plug_hash.clone();
            attach_picked_perk(&mut repaired, socket, choice, private);
            count += 1;
        }
    }
    if count != 0 {
        *recipe = repaired;
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
