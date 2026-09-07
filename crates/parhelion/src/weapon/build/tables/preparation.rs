//! Transfer stock tables once and retain canonical per-weapon authoring order.
use super::*;

pub(in crate::weapon::build) const METADATA_LAYOUT: KeyedAuxiliaryLayout = KeyedAuxiliaryLayout {
    row_size: ITEM_METADATA_ROW_SIZE,
    row_class: ITEM_METADATA_ROW_CLASS,
    nested_offset: ITEM_METADATA_NESTED_OFFSET,
    nested_class: ITEM_METADATA_NESTED_CLASS,
    secondary_class: Some(ITEM_METADATA_SECONDARY_CLASS),
    index_row_size: ITEM_METADATA_INDEX_ROW_SIZE,
    index_row_class: ITEM_METADATA_INDEX_ROW_CLASS,
    description: "item-metadata",
};
pub(in crate::weapon::build) const SANDBOX_PATTERN_LAYOUT: KeyedAuxiliaryLayout =
    KeyedAuxiliaryLayout {
        row_size: SANDBOX_PATTERN_ROW_SIZE,
        row_class: SANDBOX_PATTERN_ROW_CLASS,
        nested_offset: SANDBOX_PATTERN_NESTED_OFFSET,
        nested_class: SANDBOX_PATTERN_NESTED_CLASS,
        secondary_class: None,
        index_row_size: SANDBOX_PATTERN_INDEX_ROW_SIZE,
        index_row_class: SANDBOX_PATTERN_INDEX_ROW_CLASS,
        description: "sandbox-pattern",
    };

impl WeaponTables {
    pub(in crate::weapon::build) fn take_stock(
        sources: &mut sources::ProjectSources,
        weapon_count: usize,
    ) -> Self {
        Self {
            item_hash_index: std::mem::take(&mut sources.stock_item_hash_index),
            item_table: std::mem::take(&mut sources.stock_item_table),
            item_strings: std::mem::take(&mut sources.stock_item_strings),
            item_metadata: std::mem::take(&mut sources.stock_item_metadata),
            item_metadata_index: std::mem::take(&mut sources.stock_metadata_index),
            sandbox_patterns: std::mem::take(&mut sources.stock_sandbox_patterns),
            sandbox_pattern_index: std::mem::take(&mut sources.stock_sandbox_pattern_index),
            dense: std::mem::take(&mut sources.stock_dense),
            collectibles: std::mem::take(&mut sources.stock_collectibles),
            collectible_displays: std::mem::take(&mut sources.stock_collectible_displays),
            unlocks: std::mem::take(&mut sources.stock_unlocks),
            unlock_banks: std::mem::take(&mut sources.stock_unlock_banks),
            unlock_displays: std::mem::take(&mut sources.stock_unlock_displays),
            definitions: Vec::with_capacity(weapon_count),
            authored_strings: Vec::with_capacity(weapon_count),
            plans: Vec::with_capacity(weapon_count),
            project_rows: Vec::with_capacity(weapon_count),
            any_sandbox_pattern: false,
        }
    }

    pub(in crate::weapon::build) fn author_weapons(
        &mut self,
        context: &WeaponBuildContext<'_>,
        resolved: &[resolve::ResolvedWeapon],
    ) -> AuthoringResult<()> {
        for (ordinal, donor) in resolved.iter().enumerate() {
            self.author_weapon(context, ordinal, donor)
                .map_err(|error| {
                    error.context(format!(
                        "Weapon {:?} ({})",
                        donor.weapon.text.name, donor.weapon.namespace
                    ))
                })?;
        }
        Ok(())
    }
}
