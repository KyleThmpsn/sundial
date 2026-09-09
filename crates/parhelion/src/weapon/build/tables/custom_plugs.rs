//! Append custom plug rows without changing weapon or plug allocation order.
use super::*;

impl WeaponTables {
    pub(in crate::weapon::build) fn append_custom_plugs(
        &mut self,
        custom_plugs: &[ResolvedCustomPlug],
        stock_item_count: usize,
        weapon_count: usize,
        metadata_layout: KeyedAuxiliaryLayout,
    ) -> AuthoringResult<()> {
        for (custom_ordinal, custom_plug) in custom_plugs.iter().enumerate() {
            self.item_table = append_index_row(
                std::mem::take(&mut self.item_table),
                custom_plug.source_item_index,
                custom_plug.authored_item_hash,
                custom_plug.authored_definition_tag,
                ITEM_DEFINITION_INDEX_ROW_CLASS,
                "item-definition index",
            )?;
            self.item_strings = append_index_row(
                std::mem::take(&mut self.item_strings),
                custom_plug.source_item_index,
                custom_plug.authored_item_hash,
                custom_plug.authored_string_tag,
                ITEM_STRING_INDEX_ROW_CLASS,
                "item-string index",
            )?;
            self.item_hash_index = append_item_hash_index_row(
                std::mem::take(&mut self.item_hash_index),
                custom_plug.source_item_hash,
                u16::try_from(custom_plug.source_item_index)
                    .map_err(|_| invalid("Private socket-plug donor index does not fit 16 bits"))?,
                custom_plug.authored_item_hash,
                custom_plug.authored_item_index,
            )?;
            let expected_item_count = stock_item_count
                .checked_add(weapon_count)
                .and_then(|count| count.checked_add(custom_ordinal))
                .ok_or_else(|| invalid("Private socket-plug item count overflowed"))?;
            self.dense = append_dense_item_presentation(
                std::mem::take(&mut self.dense),
                custom_plug.source_item_index,
                expected_item_count,
                custom_plug.source_icon_container,
                custom_plug.source_icon_container,
            )?;
            validate_dense_item_presentation(
                &self.dense,
                custom_plug.source_item_index,
                expected_item_count + 1,
                custom_plug.source_icon_container,
                custom_plug.source_icon_container,
            )?;
            if let Some(classification_index) = custom_plug.classification_item_index {
                set_dense_item_presentation_type(
                    &mut self.dense,
                    expected_item_count,
                    classification_index,
                )?;
            }
            if matches!(
                classify_keyed_auxiliary_donor(
                    &self.item_metadata,
                    &self.item_metadata_index,
                    custom_plug.source_item_hash,
                    custom_plug.authored_item_hash,
                    metadata_layout,
                )?,
                KeyedAuxiliaryDonorPresence::Present(_)
            ) {
                (self.item_metadata, self.item_metadata_index) = append_keyed_auxiliary_pair(
                    std::mem::take(&mut self.item_metadata),
                    std::mem::take(&mut self.item_metadata_index),
                    custom_plug.source_item_hash,
                    custom_plug.authored_item_hash,
                    metadata_layout,
                )?;
            }
            validate_keyed_auxiliary_structure(
                &self.item_metadata,
                &self.item_metadata_index,
                metadata_layout,
            )?;
            let (item_count, _, item_rows) = terminal_index_table_layout(
                &self.item_table,
                ITEM_DEFINITION_INDEX_ROW_CLASS,
                "item-definition index",
            )?;
            let (string_count, _, string_rows) = terminal_index_table_layout(
                &self.item_strings,
                ITEM_STRING_INDEX_ROW_CLASS,
                "item-string index",
            )?;
            if item_count != expected_item_count + 1
                || string_count != item_count
                || usize::from(custom_plug.authored_item_index) != item_count - 1
                || read_u32(
                    &self.item_table,
                    item_rows + (item_count - 1) * ITEM_ROW_SIZE,
                )? != custom_plug.authored_item_hash
                || read_u32(
                    &self.item_table,
                    item_rows + (item_count - 1) * ITEM_ROW_SIZE + 16,
                )? != custom_plug.authored_definition_tag.0
                || read_u32(
                    &self.item_strings,
                    string_rows + (item_count - 1) * ITEM_ROW_SIZE,
                )? != custom_plug.authored_item_hash
                || read_u32(
                    &self.item_strings,
                    string_rows + (item_count - 1) * ITEM_ROW_SIZE + 16,
                )? != custom_plug.authored_string_tag.0
            {
                return Err(validation(
                    "Private socket-plug item tables did not remain aligned",
                ));
            }
        }
        Ok(())
    }
}
