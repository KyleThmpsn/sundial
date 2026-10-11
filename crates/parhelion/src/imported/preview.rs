//! Adapt workbench kinds to the shared pinned-graph preview reader.
use crate::ItemKind;
use parhelion_import::GraphReference;
use sundial::ui::model_preview::LocalAppearance;

pub(crate) fn appearance(reference: &GraphReference, kind: ItemKind) -> LocalAppearance {
    parhelion_import::preview::appearance(reference, super::kind(kind).unwrap_or("weapon"))
}
