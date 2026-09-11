//! Reviewed, exact-identity account cleanup for a replacement generation.
use super::*;
use sundial::investment::{
    AuthoredAccountCleanup, AuthoredClientSettings, AuthoredMoveOutcome, AuthoredSlotReplacement,
    AuthoredSocketChange, preview_authored_account_replacement_with_slots,
    preview_authored_client_settings,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementReview {
    installed: Vec<ArtifactMetadata>,
    incoming: Vec<ArtifactMetadata>,
    removed_hashes: BTreeSet<u32>,
    removed_unlocks: Vec<AuthoredCollectionUnlock>,
    socket_changes: Vec<AuthoredSocketChange>,
    slots: Option<AuthoredSlotReplacement>,
    cleanup: Option<AuthoredAccountCleanup>,
    client_settings: Option<AuthoredClientSettings>,
}

impl ReplacementReview {
    pub(super) fn client_settings(&self) -> Option<&AuthoredClientSettings> {
        self.client_settings.as_ref()
    }
    pub fn account_cleanup(&self) -> Option<&AuthoredAccountCleanup> {
        self.cleanup.as_ref()
    }
    pub fn changes_account(&self) -> bool {
        self.cleanup
            .as_ref()
            .is_some_and(|p| p.original_bytes != p.cleaned_bytes)
    }
    pub fn socket_changes(&self) -> &[AuthoredSocketChange] {
        &self.socket_changes
    }
    pub fn removes_account_data(&self) -> bool {
        self.cleanup.as_ref().is_some_and(|p| {
            !p.removed_items.is_empty()
                || p.slot_moves
                    .iter()
                    .any(|movement| movement.outcome == AuthoredMoveOutcome::DeletedInventoryFull)
                || p.cleared_plugs > 0
                || p.cleared_unlocks > 0
                || p.removed_reward_rules > 0
        })
    }
}

/// Read-only proposal. Installation recomputes it before accepting consent.
pub fn preview_replacement(target: &Path, staged: &Path) -> Result<ReplacementReview, String> {
    preview_with_progress(target, staged, &mut |_| {})
}

const REVIEW_OPERATIONS: usize = 11;

fn report(progress: progress::Observer<'_>, label: &str, completed: usize) {
    progress(InstallProgress::item(
        InstallPhase::ReviewingAccount,
        label,
        completed,
        REVIEW_OPERATIONS,
    ));
}

fn preview_with_progress(
    target: &Path,
    staged: &Path,
    progress: progress::Observer<'_>,
) -> Result<ReplacementReview, String> {
    report(progress, "Checking Review Paths", 0);
    let target = canonical_directory(target, "target packages").map_err(|e| e.to_string())?;
    let staged = canonical_directory(staged, "staged run").map_err(|e| e.to_string())?;
    let _staged_run_lease = crate::workflow::staging_retention::lease_for_read(&staged)?;
    report(progress, "Verifying Staged Packages and Recipes", 1);
    let incoming =
        validate_manifest_and_staged_files(&staged, &target).map_err(|e| e.to_string())?;
    report(progress, "Checking Installed Packages", 2);
    let installed = preview_uninstall(&target).map_err(|e| e.to_string())?;
    report(progress, "Reading Client Settings", 3);
    let client_settings =
        preview_authored_client_settings(target.parent().ok_or("Missing game root")?)?;
    let mut review = ReplacementReview {
        installed: installed.artifacts().to_vec(),
        incoming: incoming.artifacts,
        removed_hashes: BTreeSet::new(),
        removed_unlocks: vec![],
        socket_changes: vec![],
        slots: None,
        cleanup: None,
        client_settings,
    };
    if review.installed.is_empty() {
        return Ok(review);
    }
    report(progress, "Reading Installed Weapon Identities", 4);
    let (old, old_unlocks) = identities::installed_identities(&target)?;
    report(progress, "Reading Staged Weapon Identities", 5);
    let (new, new_unlocks) = identities::generation_identities(&target, &staged)?;
    review.removed_hashes = old.difference(&new).copied().collect();
    let retained = old.intersection(&new).copied().collect();
    report(progress, "Reading Installed Socket Choices", 6);
    let previous = identities::generation_socket_defaults(&target, &target, &retained)?;
    report(progress, "Reading Staged Socket Choices", 7);
    let incoming = identities::generation_socket_defaults(&target, &staged, &retained)?;
    review.socket_changes = socket_changes(previous, incoming)?;
    report(progress, "Comparing Equipment Slots", 8);
    review.slots = identities::slot_replacement(&target, &staged, &retained)?;
    review.removed_unlocks = old_unlocks
        .into_iter()
        .filter(|old| {
            !new_unlocks
                .iter()
                .any(|new| (new.bank, new.slot) == (old.bank, old.slot))
        })
        .collect();
    if !review.removed_hashes.is_empty()
        || !review.removed_unlocks.is_empty()
        || !review.socket_changes.is_empty()
        || review.slots.is_some()
    {
        report(progress, "Checking Account Items and References", 9);
        review.cleanup = Some(account_references(&target, &review)?);
        if let (Some(settings), Some(cleanup)) = (&review.client_settings, &mut review.cleanup)
            && settings.merge_account_change(cleanup)?
        {
            review.client_settings = None;
        }
    }
    Ok(review)
}

fn socket_changes(
    previous: BTreeMap<u32, Vec<Option<u32>>>,
    incoming: BTreeMap<u32, Vec<Option<u32>>>,
) -> Result<Vec<AuthoredSocketChange>, String> {
    if previous.keys().ne(incoming.keys()) {
        return Err(
            "The retained native socket definitions do not match between generations".into(),
        );
    }
    Ok(incoming
        .into_iter()
        .filter_map(|(definition_hash, default_plugs)| {
            let previous_socket_count = previous[&definition_hash].len();
            (previous_socket_count != default_plugs.len()).then_some(AuthoredSocketChange {
                definition_hash,
                previous_socket_count,
                default_plugs,
            })
        })
        .collect())
}

pub(super) fn review_request(
    request: &InstallRequest,
    target: &Path,
    staged: &Path,
    progress: progress::Observer<'_>,
) -> Result<Option<ReplacementReview>, String> {
    #[cfg(test)]
    if request.skip_replacement_review {
        return Ok(request.confirmed_replacement.clone());
    }
    let current = preview_with_progress(target, staged, progress)?;
    report(progress, "Confirming Reviewed Changes", 10);
    validate_consent(&current, request.confirmed_replacement.as_ref())?;
    report(progress, "Account Review Complete", REVIEW_OPERATIONS);
    Ok(Some(current))
}

fn validate_consent(
    current: &ReplacementReview,
    confirmed: Option<&ReplacementReview>,
) -> Result<(), String> {
    if let Some(confirmed) = confirmed {
        if confirmed != current {
            return Err("The package set or selected account changed since review. Review installation again before confirming installation.".into());
        }
    } else if current.changes_account() {
        return Err("This build updates saved custom items, socket selections, or references. Review Account Changes and confirm installation first. No packages or account data were changed.".into());
    }
    Ok(())
}

fn account_references(
    target: &Path,
    review: &ReplacementReview,
) -> Result<AuthoredAccountCleanup, String> {
    let target = fs::canonicalize(target).map_err(|error| error.to_string())?;
    let mut cleanup = preview_authored_account_replacement_with_slots(
        target.parent().ok_or("Missing game root")?,
        &review.removed_hashes,
        &review.removed_unlocks,
        &review.socket_changes,
        review.slots.as_ref(),
    )
    .map_err(|error| {
        format!("Cannot review account changes before replacing custom packages: {error}")
    })?;
    if let Some(settings) =
        preview_authored_client_settings(target.parent().ok_or("Missing game root")?)?
    {
        settings.merge_account_change(&mut cleanup)?;
    }
    Ok(cleanup)
}

pub(super) fn verify_account(
    target: &Path,
    review: Option<&ReplacementReview>,
) -> Result<(), String> {
    let target = fs::canonicalize(target).map_err(|error| error.to_string())?;
    let target = target.as_path();
    if let Some(review) = review {
        let current =
            preview_authored_client_settings(target.parent().ok_or("Missing game root")?)?;
        let merged = review.cleanup.as_ref().is_some_and(|cleanup| {
            current.as_ref().is_some_and(|settings| {
                paths_equal(&settings.settings_path, &cleanup.settings_path)
            })
        });
        if !merged && current != review.client_settings {
            return Err("Sunrise settings changed after installation review".into());
        }
    }
    if let Some(review) = review
        && let Some(expected) = &review.cleanup
        && account_references(target, review)? != *expected
    {
        return Err("The selected account changed after replacement review. Close other account editors and review installation again. No packages were replaced.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn test_review(target: &Path, hashes: BTreeSet<u32>) -> ReplacementReview {
    test_review_with_sockets(target, hashes, vec![])
}

#[cfg(test)]
pub(crate) fn test_review_with_sockets(
    target: &Path,
    hashes: BTreeSet<u32>,
    socket_changes: Vec<AuthoredSocketChange>,
) -> ReplacementReview {
    test_review_with_slots(target, hashes, socket_changes, None)
}

#[cfg(test)]
pub(crate) fn test_review_with_slots(
    target: &Path,
    hashes: BTreeSet<u32>,
    socket_changes: Vec<AuthoredSocketChange>,
    slots: Option<AuthoredSlotReplacement>,
) -> ReplacementReview {
    let target = fs::canonicalize(target).unwrap();
    let client_settings = preview_authored_client_settings(target.parent().unwrap()).unwrap();
    let mut review = ReplacementReview {
        installed: vec![],
        incoming: vec![],
        removed_hashes: hashes,
        removed_unlocks: vec![],
        socket_changes,
        slots,
        cleanup: None,
        client_settings,
    };
    review.cleanup = Some(account_references(&target, &review).unwrap());
    if let (Some(settings), Some(cleanup)) = (&review.client_settings, &mut review.cleanup)
        && settings.merge_account_change(cleanup).unwrap()
    {
        review.client_settings = None;
    }
    review
}
