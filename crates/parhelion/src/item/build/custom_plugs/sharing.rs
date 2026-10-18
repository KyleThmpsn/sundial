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
    /// The owner item and content pin of an imported appearance bound to this socket choice.
    /// Emission binds that appearance's art rows, dyes and icon to the plug item, so the plug
    /// carries one owner's appearance and is never shared, even with an identical variant.
    appearance: Option<(u32, String)>,
}

/// An imported appearance attached to an owner's recipe, read from its ornament link. It binds
/// the plug in this socket choice of this owner.
pub(super) struct BoundAppearance {
    socket_index: u16,
    choice_index: u16,
    native_plug: u32,
    sha256: String,
}

impl BoundAppearance {
    /// Whether this appearance binds `variant`, by the same socket, choice and native plug the
    /// emission binding follows.
    pub(super) fn binds(&self, variant: &WeaponSocketPlugVariantOverride) -> bool {
        self.socket_index == variant.socket_index
            && self.choice_index == variant.choice_index
            && self.native_plug == variant.source_plug_hash
    }

    pub(super) fn pin(&self) -> &str {
        &self.sha256
    }
}

/// The appearance attachments of one recipe. A recipe without an imported graph, or whose graph
/// has no attachments, binds none.
#[cfg(feature = "d2-model-importer")]
pub(super) fn bound_appearances(spec: &WeaponCloneSpec) -> AuthoringResult<Vec<BoundAppearance>> {
    let Some(graph) = &spec.overrides.imported_graph else {
        return Ok(Vec::new());
    };
    graph
        .attachments
        .iter()
        .map(|attachment| {
            let path = attachment.directory.join("asset-graph.json");
            let document: serde_json::Value = std::fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
                .map_err(|error| {
                    invalid(format!(
                        "Imported appearance {} could not be read: {error}",
                        attachment.sha256
                    ))
                })?;
            let link = &document["ornament"];
            let word = |key: &str| {
                link[key]
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| {
                        invalid(format!(
                            "Imported appearance {} has no ornament {key}",
                            attachment.sha256
                        ))
                    })
            };
            let index = |key: &str| {
                u16::try_from(word(key)?).map_err(|_| {
                    invalid(format!(
                        "Imported appearance {} ornament {key} does not fit 16 bits",
                        attachment.sha256
                    ))
                })
            };
            let socket_index = index("target_socket")?;
            let native_plug = word("native_plug")?;
            // Weapon ornament links name the socket and native plug but not the choice. Each
            // native plug fills one choice of its socket, so the owner's variants identify it.
            let choice_index = if link.get("choice_index").is_some() {
                index("choice_index")?
            } else {
                let mut choices = spec
                    .overrides
                    .socket_plug_variants
                    .iter()
                    .filter(|variant| {
                        variant.socket_index == socket_index
                            && variant.source_plug_hash == native_plug
                    })
                    .map(|variant| variant.choice_index);
                match (choices.next(), choices.next()) {
                    (Some(choice), None) => choice,
                    _ => {
                        return Err(invalid(format!(
                            "Imported appearance {} needs exactly one private choice in socket {} for its native plug",
                            attachment.sha256,
                            usize::from(socket_index) + 1
                        )));
                    }
                }
            };
            Ok(BoundAppearance {
                socket_index,
                choice_index,
                native_plug,
                sha256: attachment.sha256.clone(),
            })
        })
        .collect()
}

#[cfg(not(feature = "d2-model-importer"))]
pub(super) fn bound_appearances(_spec: &WeaponCloneSpec) -> AuthoringResult<Vec<BoundAppearance>> {
    Ok(Vec::new())
}

impl Key {
    pub(super) fn new(
        sources: &sources::ProjectSources,
        variant: &WeaponSocketPlugVariantOverride,
        classification: Option<crate::plug_classification::PlugClassification>,
        classification_perk_index: Option<usize>,
        appearance: Option<(u32, String)>,
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
            appearance,
        })
    }
}
