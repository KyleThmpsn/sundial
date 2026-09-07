//! Reviewed, exact-identity account cleanup for a replacement generation.
use super::*;
use sundial::investment::{AuthoredAccountCleanup, preview_authored_account_cleanup};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementReview {
    installed: Vec<ArtifactMetadata>,
    incoming: Vec<ArtifactMetadata>,
    removed_hashes: BTreeSet<u32>,
    removed_unlocks: Vec<AuthoredCollectionUnlock>,
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
}

/// Read-only proposal. Installation recomputes it before accepting consent.
pub fn preview_replacement(target: &Path, staged: &Path) -> Result<ReplacementReview, String> {
    let target = canonical_directory(target, "target packages").map_err(|e| e.to_string())?;
    let staged = canonical_directory(staged, "staged run").map_err(|e| e.to_string())?;
    let incoming =
        validate_manifest_and_staged_files(&staged, &target).map_err(|e| e.to_string())?;
    let installed = preview_uninstall(&target).map_err(|e| e.to_string())?;
    let mut review = ReplacementReview {
        installed: installed.artifacts().to_vec(),
        incoming: incoming.artifacts,
        removed_hashes: BTreeSet::new(),
        removed_unlocks: vec![],
        cleanup: None,
    };
    if review.installed.is_empty() {
        return Ok(review);
    }
    let (old, old_unlocks) = identities::installed_identities(&target)?;
    let (new, new_unlocks) = identities::generation_identities(&target, &staged)?;
    review.removed_hashes = old.difference(&new).copied().collect();
    review.removed_unlocks = old_unlocks
        .into_iter()
        .filter(|old| {
            !new_unlocks
                .iter()
                .any(|new| (new.bank, new.slot) == (old.bank, old.slot))
        })
        .collect();
    if !review.removed_hashes.is_empty() || !review.removed_unlocks.is_empty() {
        review.cleanup = Some(account_references(&target, &review)?);
    }
    Ok(review)
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
            return Err("The package set or selected account changed since review. Review installation again before confirming removal.".into());
        }
    } else if current.changes_account() {
        return Err("This build removes saved custom items or references. Review installation and confirm Back Up, Remove & Install first. No packages or account data were changed.".into());
    }
    Ok(())
}

fn account_references(
    target: &Path,
    review: &ReplacementReview,
) -> Result<AuthoredAccountCleanup, String> {
    let target = fs::canonicalize(target).map_err(|error| error.to_string())?;
    preview_authored_account_cleanup(
        target.parent().ok_or("Missing game root")?,
        &review.removed_hashes,
        &review.removed_unlocks,
    )
    .map_err(|error| {
        format!("Cannot review account cleanup before replacing custom packages: {error}")
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
    let target = fs::canonicalize(target).unwrap();
    let mut review = ReplacementReview {
        installed: vec![],
        incoming: vec![],
        removed_hashes: hashes,
        removed_unlocks: vec![],
        cleanup: None,
    };
    review.cleanup = Some(account_references(&target, &review).unwrap());
    review
}
