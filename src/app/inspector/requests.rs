use eframe::egui;

const HASH_INSPECTION_REQUEST_ID: &str = "catalog_hash_inspection_request";
const HASH_INSPECTION_CONTEXT_ID: &str = "catalog_hash_inspection_context";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct DefinitionInspectionContext {
    pub source: String,
    pub instance_id: Option<String>,
    pub authored_level: Option<i64>,
    pub flags: Option<u8>,
    pub plug_count: Option<usize>,
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
    fn definition_context_is_tied_to_the_requested_hash() {
        let ui = egui::Context::default();
        let context = DefinitionInspectionContext {
            source: "Character 1 · Kinetic slot".into(),
            instance_id: Some("0x4000000000000001".into()),
            authored_level: Some(1_950),
            flags: Some(1),
            plug_count: Some(8),
        };
        request_definition_with_context(&ui, 0xD980_2C4F, context.clone());
        assert_eq!(take_definition_request(&ui), Some(0xD980_2C4F));
        assert_eq!(take_definition_context(&ui, 0xD980_2C4F), Some(context));
        assert_eq!(take_definition_context(&ui, 0xD980_2C4F), None);
    }
}
