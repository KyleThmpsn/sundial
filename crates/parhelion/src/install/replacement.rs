//! Reviewed, exact-identity account cleanup for a replacement generation.
use super::*;
use std::sync::Arc;
use sundial::package_authoring::account::{
    AuthoredAccountCleanup, AuthoredClientSettings, AuthoredMoveOutcome, AuthoredSlotReplacement,
    AuthoredSocketChange, preview_authored_account_replacement_for_runtime,
    preview_authored_client_settings_for_runtime,
};

/// A staged recipe's private copy of a stock plug in one socket of its item. The copy takes the
/// stock plug's place among that socket's choices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StockPlugVariant {
    pub item_hash: u32,
    pub lane: usize,
    pub source: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplacementReview {
    validated: Option<Arc<ValidatedManifest>>,
    packages: Vec<CheckedPackage>,
    mods: Vec<crate::ModReport>,
    members: Vec<crate::mod_family::Member>,
    installed: Vec<ArtifactMetadata>,
    installed_family: Option<BTreeMap<String, loader::Folder>>,
    incoming: Vec<ArtifactMetadata>,
    removed_hashes: BTreeSet<u32>,
    incoming_definitions: Option<BTreeSet<u32>>,
    removed_unlocks: Vec<AuthoredCollectionUnlock>,
    socket_changes: Vec<AuthoredSocketChange>,
    slots: Option<AuthoredSlotReplacement>,
    runtime: RuntimeSnapshot,
    cleanup: Option<AuthoredAccountCleanup>,
    client_settings: Option<AuthoredClientSettings>,
}

impl ReplacementReview {
    pub(super) fn validated(&self) -> Option<&Arc<ValidatedManifest>> {
        self.validated.as_ref()
    }
    pub(super) fn packages(&self) -> &[CheckedPackage] {
        &self.packages
    }
    pub(super) fn verify_family(
        &self,
        current: &BTreeMap<String, loader::Folder>,
    ) -> Result<(), String> {
        if self
            .installed_family
            .as_ref()
            .is_some_and(|expected| expected != current)
        {
            return Err(
                "The installed mod folders changed after review. Review installation again".into(),
            );
        }
        Ok(())
    }
    pub(super) fn verify_runtime(&self, current: &RuntimeSnapshot) -> Result<(), String> {
        if self.runtime != *current {
            return Err(
                "The installed runtime changed since review. Review installation again".into(),
            );
        }
        Ok(())
    }
    pub fn mods(&self) -> &[crate::ModReport] {
        &self.mods
    }
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

/// Read-only proposal with the package and account checks reported as they run.
pub fn preview_replacement_with_progress(
    target: &Path,
    staged: &Path,
    mut progress: impl FnMut(InstallProgress),
) -> Result<ReplacementReview, String> {
    preview_with_progress(target, staged, &mut progress)
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
    let target = canonical_packages_directory(target).map_err(|e| e.to_string())?;
    let install = target.parent().ok_or("Missing game root")?;
    let runtime = sundial::package_authoring::installed_runtime(install)?;
    preview_for_runtime_with_progress(&target, staged, &runtime, progress)
}

fn preview_for_runtime_with_progress(
    target: &Path,
    staged: &Path,
    runtime: &RuntimeSnapshot,
    progress: progress::Observer<'_>,
) -> Result<ReplacementReview, String> {
    report(progress, "Checking Review Paths", 0);
    let target = canonical_packages_directory(target).map_err(|e| e.to_string())?;
    let staged = canonical_directory(staged, "staged run").map_err(|e| e.to_string())?;
    let _staged_run_lease = crate::workflow::staging_retention::lease_for_read(&staged)?;
    report(progress, "Verifying Staged Packages and Recipes", 1);
    let incoming =
        Arc::new(validate_manifest_and_staged_files(&staged, &target).map_err(|e| e.to_string())?);
    preview_validated(&target, &staged, runtime, &incoming, &[], progress)
}

/// Installed packages, account data and settings remain live checks. The staged generation's
/// structure and recipe meaning come from the content already verified by this operation.
fn preview_validated(
    target: &Path,
    staged: &Path,
    runtime: &RuntimeSnapshot,
    incoming: &Arc<ValidatedManifest>,
    known: &[CheckedPackage],
    progress: progress::Observer<'_>,
) -> Result<ReplacementReview, String> {
    let install = target.parent().ok_or("Missing game root")?;
    let variants = &incoming.plug_variants;
    report(progress, "Checking Installed Packages", 2);
    let known = incoming
        .packages
        .iter()
        .chain(known)
        .cloned()
        .collect::<Vec<_>>();
    let (installed, packages) =
        loader::preview_uninstall_with_packages(target, &known).map_err(|e| e.to_string())?;
    let mut installed_family = installed.mod_family.clone();
    if let Some(main) = &installed.mod_folder {
        installed_family.insert(
            sundial::package_authoring::mod_content::ID.into(),
            main.clone(),
        );
    }
    report(progress, "Reading Client Settings", 3);
    let client_settings = preview_authored_client_settings_for_runtime(install, runtime)?;
    let mut review = ReplacementReview {
        validated: Some(Arc::clone(incoming)),
        packages,
        mods: if runtime.uses_mod_loader()? {
            crate::mod_family::reports(&incoming.mods, install, &incoming.artifacts)?
        } else {
            Vec::new()
        },
        members: incoming.mods.clone(),
        installed: installed.artifacts().to_vec(),
        installed_family: Some(installed_family),
        incoming: incoming.artifacts.clone(),
        removed_hashes: BTreeSet::new(),
        incoming_definitions: None,
        removed_unlocks: vec![],
        socket_changes: vec![],
        slots: None,
        runtime: runtime.clone(),
        cleanup: None,
        client_settings,
    };
    report(progress, "Reading Staged Item Identities", 4);
    let incoming = identities::Generation::open(target, staged)?;
    let compared = (|| {
        review.incoming_definitions = Some(if runtime.uses_mod_loader()? {
            incoming_mod_definitions(target, staged, &review)?
        } else {
            identities::read_available_definitions(&incoming)?
        });
        if !review.installed.is_empty() {
            report(progress, "Reading Installed Item Identities", 5);
            let installed = identities::Generation::open(target, target)?;
            let compared = identities::read_identities(&installed).and_then(|old| {
                compare_generations(&installed, &incoming, old, variants, &mut review, progress)
            });
            installed.finish(compared)?;
        }
        Ok(())
    })();
    incoming.finish(compared)?;
    report(progress, "Checking Account Items and References", 9);
    review.cleanup = Some(account_references(target, &review)?);
    if let (Some(settings), Some(cleanup)) = (&review.client_settings, &mut review.cleanup)
        && settings.merge_account_change(cleanup)?
    {
        review.client_settings = None;
    }
    Ok(review)
}

fn incoming_mod_definitions(
    target: &Path,
    staged: &Path,
    review: &ReplacementReview,
) -> Result<BTreeSet<u32>, String> {
    let sources = review
        .incoming
        .iter()
        .map(|artifact| {
            crate::mod_family::source(&review.members, staged, target, &artifact.file_name)
                .map(|path| (artifact.clone(), path))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let retired = review
        .installed
        .iter()
        .filter(|artifact| target.join(&artifact.file_name).is_file())
        .map(|artifact| artifact.file_name.clone())
        .collect();
    let incoming = crate::mod_family::incoming(&review.members, &sources, retired)?;
    let manager =
        sundial::package_authoring::open_incoming_shadowkeep_package_manager(target, &incoming)?;
    identities::available_definitions(&manager)
}

/// Compares the installed and staged generations, each opened once for every comparison.
fn compare_generations(
    installed: &identities::Generation,
    staged: &identities::Generation,
    (old, old_unlocks): (BTreeSet<u32>, Vec<AuthoredCollectionUnlock>),
    variants: &[StockPlugVariant],
    review: &mut ReplacementReview,
    progress: progress::Observer<'_>,
) -> Result<(), String> {
    let (new, new_unlocks) = identities::read_identities(staged)?;
    review.removed_hashes = old.difference(&new).copied().collect();
    let retained = old.intersection(&new).copied().collect();
    report(progress, "Reading Installed Socket Choices", 6);
    let previous = identities::socket_defaults(installed, &retained)?;
    report(progress, "Reading Staged Socket Choices", 7);
    let incoming = identities::socket_defaults(staged, &retained)?;
    review.socket_changes = socket_changes(&previous, &incoming, &new, variants)?;
    report(progress, "Comparing Equipment Slots", 8);
    review.slots = identities::slot_replacement(installed, staged, &retained)?;
    review.removed_unlocks = old_unlocks
        .into_iter()
        .filter(|old| {
            !new_unlocks
                .iter()
                .any(|new| (new.bank, new.slot) == (old.bank, old.slot))
        })
        .collect();
    Ok(())
}

fn socket_changes(
    previous: &BTreeMap<u32, Vec<Option<u32>>>,
    incoming: &BTreeMap<u32, Vec<Option<u32>>>,
    authored: &BTreeSet<u32>,
    variants: &[StockPlugVariant],
) -> Result<Vec<AuthoredSocketChange>, String> {
    if previous.keys().ne(incoming.keys()) {
        return Err(
            "The retained native socket definitions do not match between generations".into(),
        );
    }
    Ok(incoming
        .iter()
        .filter_map(|(&definition_hash, default_plugs)| {
            let installed = &previous[&definition_hash];
            // A lane both generations have whose default changed, such as a private plug taking
            // the place of a stock default, carries saved selections of the old default along.
            let mut replaced_defaults = installed
                .iter()
                .zip(default_plugs)
                .enumerate()
                .filter_map(|(lane, (old, new))| match old {
                    Some(old) if Some(*old) != *new && *old != 0 && *old != u32::MAX => {
                        Some((lane, *old))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            // A lane whose default is a recipe's private copy of a stock plug carries saved
            // selections of that stock plug along too, however many installs ago the copy took
            // its place. A lane with more than one copy leaves its saved choices alone.
            let item_variants = variants
                .iter()
                .filter(|variant| variant.item_hash == definition_hash)
                .collect::<Vec<_>>();
            for variant in &item_variants {
                let lane = variant.lane;
                let alone = item_variants.iter().filter(|v| v.lane == lane).count() == 1;
                let private_default = default_plugs
                    .get(lane)
                    .copied()
                    .flatten()
                    .is_some_and(|plug| plug != variant.source && authored.contains(&plug));
                let unclaimed = replaced_defaults.iter().all(|(other, _)| *other != lane);
                if alone
                    && private_default
                    && unclaimed
                    && lane < installed.len()
                    && variant.source != 0
                    && variant.source != u32::MAX
                {
                    replaced_defaults.push((lane, variant.source));
                }
            }
            replaced_defaults.sort_unstable();
            (installed.len() != default_plugs.len() || !replaced_defaults.is_empty()).then(|| {
                AuthoredSocketChange {
                    definition_hash,
                    previous_socket_count: installed.len(),
                    default_plugs: default_plugs.clone(),
                    replaced_defaults,
                }
            })
        })
        .collect())
}

pub(super) fn review_request(
    request: &InstallRequest,
    target: &Path,
    staged: &Path,
    runtime: &RuntimeSnapshot,
    incoming: &Arc<ValidatedManifest>,
    progress: progress::Observer<'_>,
) -> Result<Option<ReplacementReview>, String> {
    #[cfg(test)]
    if request.skip_replacement_review {
        return Ok(request.confirmed_replacement.clone());
    }
    let known = request
        .confirmed_replacement
        .as_ref()
        .map_or(&[][..], |review| review.packages());
    let current = preview_validated(target, staged, runtime, incoming, known, progress)?;
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
    let install = target.parent().ok_or("Missing game root")?;
    let preview = |hashes: &BTreeSet<u32>, slots: Option<&AuthoredSlotReplacement>| {
        preview_authored_account_replacement_for_runtime(
            install,
            &review.runtime,
            hashes,
            &review.removed_unlocks,
            &review.socket_changes,
            slots,
        )
        .map_err(|error| {
            format!("Cannot review account changes before replacing custom packages: {error}")
        })
    };
    let slots = review.slots.as_ref();
    let mut cleanup = match &review.incoming_definitions {
        None => preview(&review.removed_hashes, slots)?,
        // Saved items and plugs whose definitions the incoming packages lack, such as ones left
        // from an older build, are removed the same way as the items this build leaves out.
        // Slot moves size inventories from incoming definitions, so those are found without them.
        Some(definitions) => {
            let unmoved = preview(&review.removed_hashes, None)?;
            let missing = unmoved.missing_incoming_definitions(definitions)?;
            if missing.is_empty() && slots.is_none() {
                unmoved
            } else {
                let removed: BTreeSet<u32> =
                    review.removed_hashes.union(&missing).copied().collect();
                let cleanup = preview(&removed, slots)?;
                cleanup.validate_incoming_definitions(definitions)?;
                cleanup
            }
        }
    };
    if let Some(settings) = preview_authored_client_settings_for_runtime(install, &review.runtime)?
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
        let current = preview_authored_client_settings_for_runtime(
            target.parent().ok_or("Missing game root")?,
            &review.runtime,
        )?;
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

/// The review installation computes for this runtime, for a test to confirm.
#[cfg(test)]
pub(crate) fn test_review_for_runtime(
    target: &Path,
    staged: &Path,
    runtime: &RuntimeSnapshot,
) -> Result<ReplacementReview, String> {
    preview_for_runtime_with_progress(target, staged, runtime, &mut |_| {})
}

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
    let install = target.parent().unwrap();
    let module = install.join("bin/x64/steam_api64.dll");
    fs::create_dir_all(module.parent().unwrap()).unwrap();
    if !module.exists() {
        fs::write(&module, b"test runtime").unwrap();
    }
    let runtime = RuntimeSnapshot::from_verified_module(RuntimeBrand::Sunrise, &module).unwrap();
    let client_settings = preview_authored_client_settings_for_runtime(install, &runtime).unwrap();
    let mut review = ReplacementReview {
        validated: None,
        packages: Vec::new(),
        mods: Vec::new(),
        members: Vec::new(),
        installed: vec![],
        installed_family: None,
        incoming: vec![],
        removed_hashes: hashes,
        incoming_definitions: None,
        removed_unlocks: vec![],
        socket_changes,
        slots,
        runtime,
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
