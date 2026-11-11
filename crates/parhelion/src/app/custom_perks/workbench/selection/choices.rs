//! Candidate precedence, identity and eligibility, independent of the picker UI.
use super::*;

/// The choices of `entries`, `documents` and `templates`, each perk once. A template names
/// whether it was imported from a saved recipe, and an entry or document was when its id is
/// among `imports`. `catalog` tells ornaments apart.
pub(super) fn collect(
    entries: &[Entry],
    documents: &[Document],
    templates: impl IntoIterator<Item = (PerkRecipe, String, bool)>,
    (imports, catalog): (&BTreeSet<String>, &InvestmentCatalog),
    issue: impl Fn(&PerkRecipe) -> Option<String>,
) -> Vec<Choice> {
    let mut choices = Vec::new();
    // Each choice's attached form, by its plug and name. A library of imported weapons offers
    // thousands of perks, too many to compare each with every one before it.
    let mut seen = BTreeMap::<_, Vec<WeaponSocketPlugVariantRecipe>>::new();
    let mut add = |recipe: PerkRecipe, source: String, pending: bool, from_recipe: bool| {
        let variant = recipe.at_socket(0, 0);
        let same = seen
            .entry((variant.source_plug_hash.clone(), variant.name.clone()))
            .or_default();
        if same.contains(&variant) {
            return;
        }
        same.push(variant);
        let issue = if pending {
            Some("Finish editing this perk in the workbench before selecting it.".into())
        } else {
            issue(&recipe)
        };
        let imported = from_recipe || imports.contains(&recipe.id);
        let ornament = recipe
            .template_plug
            .parse_u32()
            .is_ok_and(|hash| catalog.is_ornament(hash));
        let search = format!("{} {} {source}", recipe.name, recipe.description).to_lowercase();
        choices.push(Choice {
            recipe,
            source,
            issue,
            warning: None,
            imported,
            ornament,
            search,
        });
    };
    // Saved payloads stay usable while a document with the same identity is being edited.
    for entry in entries {
        add(entry.recipe.clone(), "Custom Perks".into(), false, false);
    }
    let empty = PerkRecipe::new().at_socket(0, 0);
    for document in documents {
        if document.untouched_copy()
            || (document.baseline.is_none() && document.recipe.at_socket(0, 0) == empty)
        {
            continue;
        }
        add(
            document.recipe.clone(),
            if document.unchanged() {
                "Custom Perks"
            } else {
                "Workbench Draft"
            }
            .into(),
            document.pending_effect.is_some(),
            false,
        );
    }
    for (recipe, source, from_recipe) in templates {
        add(recipe, source, false, from_recipe);
    }
    choices.sort_by_cached_key(|choice| choice.recipe.name.to_lowercase());
    choices
}

impl Choice {
    pub(super) fn matches(&self, query: &str) -> bool {
        query
            .split_whitespace()
            .all(|word| self.search.contains(word))
    }
}
