//! Reviewed, exact-identity account cleanup for a replacement generation.
use super::*;
use sundial::investment::{
    AuthoredAccountCleanup, AuthoredSocketChange, preview_authored_account_replacement,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementReview {
    installed: Vec<ArtifactMetadata>,
    incoming: Vec<ArtifactMetadata>,
    removed_hashes: BTreeSet<u32>,
    removed_unlocks: Vec<AuthoredCollectionUnlock>,
    socket_changes: Vec<AuthoredSocketChange>,
    cleanup: Option<AuthoredAccountCleanup>,
}

impl ReplacementReview {
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
                || p.cleared_plugs > 0
                || p.cleared_unlocks > 0
                || p.removed_reward_rules > 0
        })
    }
}

/// Read-only proposal. Installation recomputes it before accepting consent.
pub fn preview_replacement(target: &Path, staged: &Path) -> Result<ReplacementReview, String> {
    let target = canonical_directory(target, "target packages").map_err(|e| e.to_string())?;
    let staged = canonical_directory(staged, "staged run").map_err(|e| e.to_string())?;
    let _staged_run_lease = crate::workflow::staging_retention::lease_for_read(&staged)?;
    let incoming =
        validate_manifest_and_staged_files(&staged, &target).map_err(|e| e.to_string())?;
    let installed = preview_uninstall(&target).map_err(|e| e.to_string())?;
    let mut review = ReplacementReview {
        installed: installed.artifacts().to_vec(),
        incoming: incoming.artifacts,
        removed_hashes: BTreeSet::new(),
        removed_unlocks: vec![],
        socket_changes: vec![],
        cleanup: None,
    };
    if review.installed.is_empty() {
        return Ok(review);
    }
    let (old, old_unlocks) = identities::installed_identities(&target)?;
    let (new, new_unlocks) = identities::generation_identities(&target, &staged)?;
    review.removed_hashes = old.difference(&new).copied().collect();
    let retained = old.intersection(&new).copied().collect();
    let previous = identities::generation_socket_defaults(&target, &target, &retained)?;
    let incoming = identities::generation_socket_defaults(&target, &staged, &retained)?;
    review.socket_changes = socket_changes(previous, incoming)?;
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
    {
        review.cleanup = Some(account_references(&target, &review)?);
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
) -> Result<Option<ReplacementReview>, String> {
    #[cfg(test)]
    if request.skip_replacement_review {
        return Ok(request.confirmed_replacement.clone());
    }
    let current = preview_replacement(target, staged)?;
    validate_consent(&current, request.confirmed_replacement.as_ref())?;
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
    preview_authored_account_replacement(
        target.parent().ok_or("Missing game root")?,
        &review.removed_hashes,
        &review.removed_unlocks,
        &review.socket_changes,
    )
    .map_err(|error| {
        format!("Cannot review account changes before replacing custom packages: {error}")
    })
}

pub(super) fn verify_account(
    target: &Path,
    review: Option<&ReplacementReview>,
) -> Result<(), String> {
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
    let target = fs::canonicalize(target).unwrap();
    let mut review = ReplacementReview {
        installed: vec![],
        incoming: vec![],
        removed_hashes: hashes,
        removed_unlocks: vec![],
        socket_changes,
        cleanup: None,
    };
    review.cleanup = Some(account_references(&target, &review).unwrap());
    review
}
