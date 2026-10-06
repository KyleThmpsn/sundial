use eframe::egui;

use super::progression::MetadataSelection;

const HASH_INSPECTION_REQUEST_ID: &str = "catalog_hash_inspection_request";
const HASH_INSPECTION_CONTEXT_ID: &str = "catalog_hash_inspection_context";
const PROGRESSION_SELECTION_ID: &str = "catalog_hash_inspection_progression_selection";
const OWNED_QUANTITIES_ID: &str = "catalog_hash_inspection_owned_quantities";
const OWNED_QUANTITIES_REQUEST_ID: &str = "catalog_hash_inspection_owned_quantities_request";

/// Quantities the loaded account holds per item hash. A window that wants them asks each frame
/// it draws them; the app answers by publishing a fresh map, so no window carries the account
/// and nothing is computed while no window is showing them.
pub(in crate::app) type OwnedQuantities = std::sync::Arc<std::collections::HashMap<u64, i64>>;

/// The app's last answer: `None` when the account cannot be read.
pub(in crate::app) type OwnedQuantitiesAnswer = Option<OwnedQuantities>;

pub(in crate::app) fn request_owned_quantities(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(OWNED_QUANTITIES_REQUEST_ID), true));
}

pub(in crate::app) fn take_owned_quantities_request(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new(OWNED_QUANTITIES_REQUEST_ID)))
        .unwrap_or(false)
}

pub(in crate::app) fn publish_owned_quantities(ctx: &egui::Context, quantities: OwnedQuantities) {
    let answer: OwnedQuantitiesAnswer = Some(quantities);
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(OWNED_QUANTITIES_ID), answer));
}

/// Withdraws the published map, so a window shows the quantities as unavailable rather than
/// keeping a previous account's numbers.
pub(in crate::app) fn clear_owned_quantities(ctx: &egui::Context) {
    let answer: OwnedQuantitiesAnswer = None;
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(OWNED_QUANTITIES_ID), answer));
}

/// `None` until the app first answers, `Some(None)` while the account cannot be read.
pub(in crate::app) fn owned_quantities(ctx: &egui::Context) -> Option<OwnedQuantitiesAnswer> {
    ctx.data(|data| data.get_temp::<OwnedQuantitiesAnswer>(egui::Id::new(OWNED_QUANTITIES_ID)))
}

const ADD_TARGETS_ID: &str = "catalog_hash_inspection_add_targets";
const ADD_TARGETS_REQUEST_ID: &str = "catalog_hash_inspection_add_targets_request";
const ADD_REQUEST_ID: &str = "catalog_hash_inspection_add_request";

/// Where the loaded account can take a new item, answered the same way as owned quantities.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct AddTargets {
    /// Each character's class: 0 Titan, 1 Hunter, 2 Warlock, or `None` when unread.
    pub(in crate::app) characters: Vec<Option<u8>>,
    /// Whether a character may hold another class's subclass.
    pub(in crate::app) cross_class_subclasses: bool,
}

/// Where an item from the inspector goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum AddDestination {
    Character(usize),
    Profile,
}

pub(in crate::app) fn request_add_targets(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(ADD_TARGETS_REQUEST_ID), true));
}

pub(in crate::app) fn take_add_targets_request(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new(ADD_TARGETS_REQUEST_ID)))
        .unwrap_or(false)
}

/// Publishes the targets, or `None` while the account cannot be edited.
pub(in crate::app) fn publish_add_targets(
    ctx: &egui::Context,
    targets: Option<std::sync::Arc<AddTargets>>,
) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(ADD_TARGETS_ID), targets));
}

/// `None` until the app first answers, `Some(None)` while the account cannot be edited.
pub(in crate::app) fn add_targets(
    ctx: &egui::Context,
) -> Option<Option<std::sync::Arc<AddTargets>>> {
    ctx.data(|data| {
        data.get_temp::<Option<std::sync::Arc<AddTargets>>>(egui::Id::new(ADD_TARGETS_ID))
    })
}

/// Asks the app to add one of an item to a character's inventory or the profile.
pub(in crate::app) fn request_add(ctx: &egui::Context, hash: u64, destination: AddDestination) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(ADD_REQUEST_ID), (hash, destination)));
    ctx.request_repaint_of(egui::ViewportId::ROOT);
}

pub(in crate::app) fn take_add_request(ctx: &egui::Context) -> Option<(u64, AddDestination)> {
    let id = egui::Id::new(ADD_REQUEST_ID);
    ctx.data_mut(|data| {
        let request = data.get_temp::<(u64, AddDestination)>(id);
        data.remove::<(u64, AddDestination)>(id);
        request
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(in crate::app) struct DefinitionInspectionContext {
    pub source: String,
    pub instance_id: Option<String>,
    pub authored_level: Option<i64>,
    pub flags: Option<u8>,
    pub plug_count: Option<usize>,
    /// Opening-time authored value: null means native defaults, an array is explicit.
    pub plugs: Option<serde_json::Value>,
    pub quantity: Option<i64>,
}

pub(in crate::app) fn request_definition(ctx: &egui::Context, hash: u64) {
    if hash != 0 {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(HASH_INSPECTION_REQUEST_ID), hash));
    }
}

pub(in crate::app) fn request_definition_with_context(
    ctx: &egui::Context,
    hash: u64,
    context: DefinitionInspectionContext,
) {
    if hash == 0 {
        return;
    }
    request_definition(ctx, hash);
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new(HASH_INSPECTION_CONTEXT_ID), (hash, context));
    });
}

pub(in crate::app) fn take_definition_request(ctx: &egui::Context) -> Option<u64> {
    ctx.data_mut(|data| data.remove_temp(egui::Id::new(HASH_INSPECTION_REQUEST_ID)))
}

pub(in crate::app) fn take_definition_context(
    ctx: &egui::Context,
    hash: u64,
) -> Option<DefinitionInspectionContext> {
    ctx.data_mut(|data| {
        data.remove_temp::<(u64, DefinitionInspectionContext)>(egui::Id::new(
            HASH_INSPECTION_CONTEXT_ID,
        ))
        .and_then(|(requested_hash, context)| (requested_hash == hash).then_some(context))
    })
}

/// Asks the app to show an unlock definition on the Progression page's Unlocks view.
pub(in crate::app) fn request_progression_selection(
    ctx: &egui::Context,
    selection: MetadataSelection,
) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(PROGRESSION_SELECTION_ID), selection));
}

pub(in crate::app) fn take_progression_selection(ctx: &egui::Context) -> Option<MetadataSelection> {
    ctx.data_mut(|data| data.remove_temp(egui::Id::new(PROGRESSION_SELECTION_ID)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_quantities_are_published_on_request_and_readable_by_any_window() {
        let ctx = egui::Context::default();
        assert!(!take_owned_quantities_request(&ctx));
        request_owned_quantities(&ctx);
        assert!(take_owned_quantities_request(&ctx));
        assert!(
            !take_owned_quantities_request(&ctx),
            "a request is consumed once"
        );
        assert!(owned_quantities(&ctx).is_none(), "no answer yet");
        publish_owned_quantities(&ctx, std::sync::Arc::new([(7_u64, 12_i64)].into()));
        assert_eq!(owned_quantities(&ctx).flatten().unwrap().get(&7), Some(&12));
        clear_owned_quantities(&ctx);
        assert!(
            matches!(owned_quantities(&ctx), Some(None)),
            "an unreadable account is answered, not pending"
        );
    }
}
