//! Author runtime payloads without interleaving host and private-perk tag allocation.
use super::*;

pub(in crate::weapon::build) struct Payloads {
    pub entity_assignments: Vec<u8>,
    pub finished_sandbox_perks: Vec<u8>,
    pub sandbox_perk_indices: Vec<u8>,
    pub pattern_global_ids: Vec<Option<u32>>,
    pub weapon_tags: Vec<NewTagSpec>,
    pub private_perk_tags: Vec<NewTagSpec>,
    pub private_perk_append_start: usize,
    weapon_allocator: AppendedTagAllocator,
    private_perk_allocator: AppendedTagAllocator,
}

pub(in crate::weapon::build) fn author(
    package_directory: &Path,
    sources: &mut sources::ProjectSources,
    resolved: &[resolve::ResolvedWeapon],
    custom_plugs: &[ResolvedCustomPlug],
    templates: &PerkTemplates,
    weapon_runtime_start: usize,
) -> AuthoringResult<(Payloads, custom_plugs::CustomPlugPayloads)> {
    let weapon_runtime_tag_allocator =
        AppendedTagAllocator::new(HOST_PACKAGE_ID, weapon_runtime_start);
    let mut weapon_runtime_new_tags = Vec::new();
    let private_perk_runtime_append_start =
        extended_overlay_append_start(package_directory, PRIVATE_PERK_RUNTIME_PACKAGE_ID)?;
    if private_perk_runtime_append_start < PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT {
        return Err(validation(format!(
            "Private perk-runtime package historical high-water mark \
             {private_perk_runtime_append_start} precedes its latest stock count \
             {PRIVATE_PERK_RUNTIME_EXPECTED_ENTRY_COUNT}"
        )));
    }
    let private_perk_runtime_tag_allocator = AppendedTagAllocator::new(
        PRIVATE_PERK_RUNTIME_PACKAGE_ID,
        private_perk_runtime_append_start,
    );
    let mut private_perk_runtime_new_tags = Vec::new();
    let mut entity_assignments = sources.stock_entity_assignments.clone();
    let mut finished_sandbox_perks = std::mem::take(&mut sources.stock_finished_sandbox_perks);
    let mut sandbox_perk_indices = std::mem::take(&mut sources.stock_sandbox_perk_indices);
    let authored_pattern_global_ids = super::author_entities(
        &sources.manager,
        &sources.stock_sandbox_patterns,
        &sources.stock_entity_assignments,
        resolved,
        weapon_runtime_tag_allocator,
        &mut entity_assignments,
        &mut weapon_runtime_new_tags,
    )?;
    let custom_payloads = custom_plugs::author_payloads(
        &sources.manager,
        custom_plugs,
        &templates.definition,
        &templates.strings,
        custom_plugs::PerkCatalog {
            entity_assignments: &mut entity_assignments,
            finished_sandbox_perks: &mut finished_sandbox_perks,
            sandbox_perk_indices: &mut sandbox_perk_indices,
            private_perk_runtime_new_tags: &mut private_perk_runtime_new_tags,
            private_perk_runtime_tag_allocator,
        },
    )?;
    Ok((
        Payloads {
            entity_assignments,
            finished_sandbox_perks,
            sandbox_perk_indices,
            pattern_global_ids: authored_pattern_global_ids,
            weapon_tags: weapon_runtime_new_tags,
            private_perk_tags: private_perk_runtime_new_tags,
            private_perk_append_start: private_perk_runtime_append_start,
            weapon_allocator: weapon_runtime_tag_allocator,
            private_perk_allocator: private_perk_runtime_tag_allocator,
        },
        custom_payloads,
    ))
}

impl Payloads {
    pub(in crate::weapon::build) fn dependencies(
        &self,
        manager: &PackageManager,
        assets: &assets::Plan,
        entity_assignment_tag: TagHash,
    ) -> AuthoringResult<Option<Vec<u8>>> {
        if self.private_perk_tags.is_empty() && self.weapon_tags.is_empty() {
            return Ok(None);
        }
        let root_entry = manager
            .get_entry(RUNTIME_DEPENDENCY_ROOT)
            .ok_or_else(|| invalid("Investment runtime dependency root is missing"))?;
        let companion_entry = manager
            .get_entry(RUNTIME_DEPENDENCY_COMPANION)
            .ok_or_else(|| invalid("Investment runtime dependency companion is missing"))?;
        let root_payload = read_tag(
            manager,
            RUNTIME_DEPENDENCY_ROOT,
            "investment runtime dependency root",
        )?;
        if root_entry.reference != 0x8080_56A6
            || root_entry.file_type != 16
            || companion_entry.reference != 0x8080_9EF9
            || companion_entry.file_type != 8
            || read_u32(&root_payload, 0x10)? != entity_assignment_tag.0
        {
            return Err(invalid(
                "Investment runtime loading root no longer matches its audited source",
            ));
        }
        let source = read_tag(
            manager,
            RUNTIME_DEPENDENCY_COMPANION,
            "investment runtime dependency index",
        )?;
        let mut additions = Vec::new();
        for index in assets.hud_asset_start..assets.badge.new_tags.len() {
            additions.push(
                AppendedTagAllocator::new(PARHELION_ASSET_PACKAGE_ID, 0).assigned_tag(
                    index,
                    "HUD dependency",
                    "HUD icon",
                )?,
            );
        }
        for index in 0..self.private_perk_tags.len() {
            additions.push(self.private_perk_allocator.assigned_tag(
                index,
                "Runtime dependency",
                "private perk",
            )?);
        }
        for index in 0..self.weapon_tags.len() {
            additions.push(self.weapon_allocator.assigned_tag(
                index,
                "Runtime dependency",
                "weapon runtime",
            )?);
        }
        Ok(Some(
            crate::shared_tag_dependency_index::enroll_dependencies(
                &source,
                RUNTIME_DEPENDENCY_COMPANION,
                RUNTIME_DEPENDENCY_ROOT,
                &additions,
            )?,
        ))
    }
}
