use crate::{WeaponLocaleTextRecipe, WeaponRecipe};

#[derive(Clone, Copy)]
pub(super) enum OptionalText {
    TypeName,
    CollectionName,
    CollectionDescription,
    InventoryHint,
    CollectionRequirement,
}

type LocaleField = fn(&mut WeaponLocaleTextRecipe) -> &mut Option<String>;

/// Only called on an explicit edit, never while rendering or loading.
pub(super) fn set(recipe: &mut WeaponRecipe, field: OptionalText, value: Option<String>) {
    let (primary, localized): (&mut Option<String>, LocaleField) = match field {
        OptionalText::TypeName => (&mut recipe.type_name, |l| &mut l.type_name),
        OptionalText::CollectionName => (&mut recipe.collection_name, |l| &mut l.collection_name),
        OptionalText::CollectionDescription => (&mut recipe.collection_description, |l| {
            &mut l.collection_description
        }),
        OptionalText::InventoryHint => (&mut recipe.inventory_hint, |l| &mut l.inventory_hint),
        OptionalText::CollectionRequirement => (&mut recipe.collection_requirement, |l| {
            &mut l.collection_requirement
        }),
    };
    *primary = value;
    if primary.is_none() {
        for locale in &mut recipe.locale_overrides {
            *localized(locale) = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_optional_text_clears_only_its_translations_and_remains_valid() {
        for field in [
            OptionalText::TypeName,
            OptionalText::CollectionName,
            OptionalText::CollectionDescription,
            OptionalText::InventoryHint,
            OptionalText::CollectionRequirement,
        ] {
            let mut recipe = WeaponRecipe::new_weapon("parhelion.optional-text-test").unwrap();
            recipe.type_name = Some("Type".into());
            recipe.collection_name = Some("Collection".into());
            recipe.collection_description = Some("Description".into());
            recipe.inventory_hint = Some("Hint".into());
            recipe.collection_requirement = Some("Requirement".into());
            recipe.locale_overrides = [1, 2, 3]
                .into_iter()
                .map(|locale_index| WeaponLocaleTextRecipe {
                    locale_index,
                    name: Some("Name translation".into()),
                    type_name: Some("Type translation".into()),
                    flavor: Some("Flavor translation".into()),
                    source: Some("Source translation".into()),
                    collection_name: Some("Collection translation".into()),
                    collection_description: Some("Description translation".into()),
                    inventory_hint: Some("Hint translation".into()),
                    collection_requirement: Some("Requirement translation".into()),
                })
                .collect();
            recipe.validate().unwrap();
            let before = recipe.clone();
            set(&mut recipe, field, Some("Edited primary text".into()));
            assert_eq!(recipe.locale_overrides, before.locale_overrides);
            recipe.validate().unwrap();

            let mut expected = before;
            match field {
                OptionalText::TypeName => expected.type_name = None,
                OptionalText::CollectionName => expected.collection_name = None,
                OptionalText::CollectionDescription => expected.collection_description = None,
                OptionalText::InventoryHint => expected.inventory_hint = None,
                OptionalText::CollectionRequirement => expected.collection_requirement = None,
            }
            for locale in &mut expected.locale_overrides {
                match field {
                    OptionalText::TypeName => locale.type_name = None,
                    OptionalText::CollectionName => locale.collection_name = None,
                    OptionalText::CollectionDescription => locale.collection_description = None,
                    OptionalText::InventoryHint => locale.inventory_hint = None,
                    OptionalText::CollectionRequirement => locale.collection_requirement = None,
                }
            }
            set(&mut recipe, field, None);
            assert_eq!(recipe, expected);
            recipe.validate().unwrap();
            let round_trip =
                WeaponRecipe::from_json_str(&serde_json::to_string(&recipe).unwrap()).unwrap();
            assert_eq!(round_trip, recipe);
        }
    }
}
