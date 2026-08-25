use eframe::egui;

const HASH_INSPECTION_REQUEST_ID: &str = "catalog_hash_inspection_request";

pub(in crate::app) fn request_definition(ctx: &egui::Context, hash: u64) {
    if hash != 0 {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new(HASH_INSPECTION_REQUEST_ID), hash));
    }
}

pub(in crate::app) fn take_definition_request(ctx: &egui::Context) -> Option<u64> {
    ctx.data_mut(|data| data.remove_temp(egui::Id::new(HASH_INSPECTION_REQUEST_ID)))
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
}
