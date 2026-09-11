use super::Titles;
use crate::{
    app::account_workspace::WorkspaceDocument, catalog::Catalog, investment::titles::Title,
    persistence::sqlite_account::SqliteAccountDocument,
};

impl Titles {
    pub(in crate::app::account_details) fn validate_changes(
        &self,
        candidate: &WorkspaceDocument,
        persisted: &WorkspaceDocument,
        catalog: &Catalog,
    ) -> Result<(), String> {
        let (Some(candidate), Some(persisted)) =
            (candidate.native_account(), persisted.native_account())
        else {
            return Ok(());
        };
        let after = selected_titles(candidate);
        let before = selected_titles(persisted);
        if after.iter().all(|&index| index == u64::from(u16::MAX))
            || (after == before && !candidate.account_flags_changed_from(persisted))
        {
            return Ok(());
        }
        let access = catalog.inspection_access();
        let source = (catalog.install_path().join("packages"), access.generation());
        let loaded;
        let titles = if self.source.as_ref() == Some(&source) {
            self.result.as_ref().and_then(|result| result.as_ref().ok())
        } else {
            None
        };
        let titles = match titles {
            Some(titles) => titles,
            None => {
                loaded = access.read(|| crate::investment::titles::load(&source.0))?;
                &loaded
            }
        };
        validate_claims(candidate, persisted, &after, &before, titles)
    }
}

fn selected_titles(document: &SqliteAccountDocument) -> Vec<u64> {
    document.runtime()["characters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|character| character["equipped_title"].as_u64().unwrap_or(u64::MAX))
        .collect()
}

fn validate_claims(
    candidate: &SqliteAccountDocument,
    persisted: &SqliteAccountDocument,
    after: &[u64],
    before: &[u64],
    titles: &[Title],
) -> Result<(), String> {
    for (character, &index) in after.iter().enumerate() {
        if index == u64::from(u16::MAX) {
            continue;
        }
        let unchanged = before.get(character) == Some(&index);
        let Some(title) = titles.iter().find(|title| u64::from(title.index) == index) else {
            if unchanged {
                continue;
            }
            return Err(format!(
                "Character {} references an unavailable title. Select an installed title or None",
                character + 1
            ));
        };
        let unlock = match &title.unlock {
            Ok(unlock) => unlock,
            Err(_) if unchanged => continue,
            Err(reason) => return Err(format!("{}: {reason}", title.name)),
        };
        // Preserve an existing invalid selection on unrelated edits, but reject a new locked title.
        let was_claimed = persisted.account_flag_is_set(unlock.definition_index, unlock.slot);
        if (!unchanged || was_claimed)
            && !candidate.account_flag_is_set(unlock.definition_index, unlock.slot)
        {
            return Err(format!(
                "{} is equipped on Character {} but its claim flag is locked. Select the title again to unlock it, or select None",
                title.name,
                character + 1
            ));
        }
    }
    Ok(())
}
