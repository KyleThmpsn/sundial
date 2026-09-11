//! Candidate precedence, identity and eligibility, independent of the picker UI.
use super::*;

#[cfg(test)]
mod tests;

pub(super) fn collect(
    entries: &[Entry],
    documents: &[Document],
    templates: impl IntoIterator<Item = (PerkRecipe, String)>,
    issue: impl Fn(&PerkRecipe) -> Option<String>,
) -> Vec<Choice> {
    let mut choices = Vec::new();
    let mut add = |recipe: PerkRecipe, source: String, pending: bool| {
        let variant = recipe.at_socket(0, 0);
        if choices
            .iter()
            .any(|choice: &Choice| choice.recipe.at_socket(0, 0) == variant)
        {
            return;
        }
        let issue = if pending {
            Some("Finish editing this perk in the workbench before selecting it.".into())
        } else {
            issue(&recipe)
        };
        choices.push(Choice {
            recipe,
            source,
            issue,
        });
    };
    // Saved payloads stay usable while a document with the same identity is being edited.
    for entry in entries {
        add(entry.recipe.clone(), "My Perks".into(), false);
    }
    let empty = PerkRecipe::new().at_socket(0, 0);
    for document in documents {
        if document.baseline.is_none() && document.recipe.at_socket(0, 0) == empty {
            continue;
        }
        let saved = document
            .baseline
            .as_deref()
            .and_then(|bytes| serde_json::from_slice::<PerkRecipe>(bytes).ok())
            .is_some_and(|saved| saved == document.recipe);
        add(
            document.recipe.clone(),
            if saved { "My Perks" } else { "Workbench Draft" }.into(),
            document.pending_effect.is_some(),
        );
    }
    for (recipe, source) in templates {
        add(recipe, source, false);
    }
    choices.sort_by_cached_key(|choice| choice.recipe.name.to_lowercase());
    choices
}

impl Choice {
    pub(super) fn matches(&self, query: &str) -> bool {
        pickers::matches(
            query,
            &format!(
                "{} {} {}",
                self.recipe.name, self.recipe.description, self.source
            ),
        )
    }
}
