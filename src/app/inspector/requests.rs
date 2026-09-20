use eframe::egui;

const HASH_INSPECTION_REQUEST_ID: &str = "catalog_hash_inspection_request";
const HASH_INSPECTION_CONTEXT_ID: &str = "catalog_hash_inspection_context";
const OWNED_QUANTITIES_ID: &str = "catalog_hash_inspection_owned_quantities";
const OWNED_QUANTITIES_REQUEST_ID: &str = "catalog_hash_inspection_owned_quantities_request";

/// Quantities the loaded account holds per item hash. A window that wants them asks each frame
/// it draws them; the app answers by publishing a fresh map, so no window carries the account
/// and nothing is computed while no window is showing them.
pub(in crate::app) type OwnedQuantities = std::sync::Arc<std::collections::HashMap<u64, i64>>;

pub(in crate::app) fn request_owned_quantities(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(OWNED_QUANTITIES_REQUEST_ID), true));
}

pub(in crate::app) fn take_owned_quantities_request(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new(OWNED_QUANTITIES_REQUEST_ID)))
        .unwrap_or(false)
}

pub(in crate::app) fn publish_owned_quantities(ctx: &egui::Context, quantities: OwnedQuantities) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(OWNED_QUANTITIES_ID), quantities));
}

/// Withdraws the published map, so a window shows the quantities as unavailable rather than
/// keeping a previous account's numbers.
pub(in crate::app) fn clear_owned_quantities(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove_temp::<OwnedQuantities>(egui::Id::new(OWNED_QUANTITIES_ID)));
}

pub(in crate::app) fn owned_quantities(ctx: &egui::Context) -> Option<OwnedQuantities> {
    ctx.data(|data| data.get_temp::<OwnedQuantities>(egui::Id::new(OWNED_QUANTITIES_ID)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_requests_ignore_zero_and_are_consumed() {
        let context = egui::Context::default();
        request_definition(&context, 0);
        assert_eq!(take_definition_request(&context), None);

        request_definition(&context, 0x574E_0A2A);
        assert_eq!(take_definition_request(&context), Some(0x574E_0A2A));
        assert_eq!(take_definition_request(&context), None);
    }

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
        assert!(owned_quantities(&ctx).is_none());
        publish_owned_quantities(&ctx, std::sync::Arc::new([(7_u64, 12_i64)].into()));
        assert_eq!(owned_quantities(&ctx).unwrap().get(&7), Some(&12));
        clear_owned_quantities(&ctx);
        assert!(owned_quantities(&ctx).is_none());
    }

    #[test]
    fn definition_context_is_tied_to_the_requested_hash() {
        let ui = egui::Context::default();
        let context = DefinitionInspectionContext {
            source: "Character 1 · Kinetic slot".into(),
            instance_id: Some("0x4000000000000001".into()),
            authored_level: Some(1_950),
            flags: Some(1),
            plug_count: Some(8),
            ..Default::default()
        };
        request_definition_with_context(&ui, 0xD980_2C4F, context.clone());
        assert_eq!(take_definition_request(&ui), Some(0xD980_2C4F));
        assert_eq!(take_definition_context(&ui, 0xD980_2C4F), Some(context));
        assert_eq!(take_definition_context(&ui, 0xD980_2C4F), None);
    }
}
