use crate::{WeaponLocaleTextRecipe, WeaponRecipe};

// Localization-header payloads follow PackageLanguage order, excluding None.
pub(super) const LANGUAGES: [&str; 13] = [
    "English",
    "French",
    "Italian",
    "German",
    "Spanish (Spain)",
    "Japanese",
    "Portuguese (Brazil)",
    "Russian",
    "Polish",
    "Chinese (Simplified)",
    "Chinese (Traditional)",
    "Spanish (Latin America)",
    "Korean",
];
pub(super) fn language(index: u8) -> &'static str {
    LANGUAGES
        .get(usize::from(index))
        .copied()
        .unwrap_or("Unknown Language")
}

#[derive(Clone, Copy, Hash)]
pub(super) enum TextSection {
    Weapon,
    Collections,
}

impl TextSection {
    pub(super) fn includes(self, field: usize) -> bool {
        let collections = matches!(field, 2 | 4 | 5 | 7);
        matches!(self, Self::Collections) == collections
    }

    pub(super) fn clear_label(self) -> &'static str {
        match self {
            Self::Weapon => "Clear Weapon Translations",
            Self::Collections => "Clear Collections Translations",
        }
    }
}

pub(super) fn clear_locale(recipe: &mut WeaponRecipe, index: usize, section: TextSection) {
    let locale = &mut recipe.locale_overrides[index];
    let fields = [
        &mut locale.name,
        &mut locale.flavor,
        &mut locale.source,
        &mut locale.type_name,
        &mut locale.collection_name,
        &mut locale.collection_description,
        &mut locale.inventory_hint,
        &mut locale.collection_requirement,
    ];
    let mut empty = true;
    for (field, value) in fields.into_iter().enumerate() {
        if section.includes(field) {
            *value = None;
        }
        empty &= value.is_none();
    }
    if empty {
        recipe.locale_overrides.remove(index);
    }
}

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
    fn clearing_one_tab_keeps_translations_from_the_other_tab() {
        let mut recipe = WeaponRecipe::every_end();
        recipe.locale_overrides = vec![WeaponLocaleTextRecipe {
            locale_index: 2,
            name: Some("Weapon translation".into()),
            source: Some("Source translation".into()),
            collection_name: Some("Collections translation".into()),
            ..Default::default()
        }];
        clear_locale(&mut recipe, 0, TextSection::Weapon);
        assert_eq!(recipe.locale_overrides.len(), 1);
        assert!(recipe.locale_overrides[0].name.is_none());
        assert_eq!(
            recipe.locale_overrides[0].source.as_deref(),
            Some("Source translation")
        );
        assert_eq!(
            recipe.locale_overrides[0].collection_name.as_deref(),
            Some("Collections translation")
        );
        clear_locale(&mut recipe, 0, TextSection::Collections);
        assert!(recipe.locale_overrides.is_empty());
    }

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
