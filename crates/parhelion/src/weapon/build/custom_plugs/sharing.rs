//! Share complete, identical definitions while keeping edits private to their variant.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Eq, PartialEq)]
pub(super) struct Key {
    variant: WeaponSocketPlugVariantOverride,
    classification: Option<crate::plug_classification::PlugClassification>,
    tooltip_category: Option<u32>,
    presentation_type: Option<u32>,
}

impl Key {
    pub(super) fn new(
        sources: &sources::ProjectSources,
        variant: &WeaponSocketPlugVariantOverride,
        classification: Option<crate::plug_classification::PlugClassification>,
        classification_perk_index: Option<usize>,
    ) -> AuthoringResult<Self> {
        let tooltip_category = classification_perk_index
            .map(|index| {
                let detail = finished_sandbox_perk_at(&sources.stock_finished_sandbox_perks, index)
                    .map_err(invalid)?
                    .detail
                    .ok_or_else(|| invalid("Tooltip classification donor has no detail row"))?;
                read_u32(&detail, 20)
            })
            .transpose()?;
        let presentation_type = variant
            .classification_donor_hash
            .map(|hash| {
                let index = sources.stock_item_rows_by_hash[&hash][0];
                let presentations = dense_item_presentation_arrays(&sources.stock_dense)?[3];
                if index >= presentations.count {
                    return Err(invalid(
                        "Classification donor is outside the dense presentation table",
                    ));
                }
                read_u32(
                    &sources.stock_dense,
                    presentations.rows
                        + index * ITEM_DENSE_PRESENTATION_ROW_SIZE
                        + ITEM_DENSE_PRESENTATION_TYPE_OFFSET,
                )
            })
            .transpose()?;
        let mut variant = variant.clone();
        variant.socket_index = 0;
        variant.choice_index = 0;
        // Compare every field that classification copying writes, not the donor's identity.
        variant.classification_donor_hash = None;
        Ok(Self {
            variant,
            classification,
            tooltip_category,
            presentation_type,
        })
    }
}
