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

#[derive(Debug, Clone, Copy, Hash)]
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
