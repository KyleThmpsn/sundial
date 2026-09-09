//! Saved plug snapshots, kept separate from catalog defaults and live runtime.
use eframe::egui;
use serde_json::Value;

use super::super::draw_named_catalog_hash_link;
use crate::{catalog::Catalog, hash::parse_hash_hex};

pub(super) fn plug_source(value: &Value) -> &'static str {
    match value {
        Value::Null => "Native Defaults",
        Value::Array(_) => "Explicit Saved List",
        _ => "Malformed Saved Value",
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SavedPlug<'a> {
    NativeDefault,
    Empty,
    Hash(u64),
    Missing,
    Invalid(&'a Value),
}

fn saved_plug(value: &Value, index: usize) -> SavedPlug<'_> {
    match value {
        Value::Null => SavedPlug::NativeDefault,
        Value::Array(values) => match values.get(index) {
            None => SavedPlug::Missing,
            Some(Value::Null) => SavedPlug::Empty,
            Some(value) => value
                .as_u64()
                .or_else(|| value.as_str().and_then(parse_hash_hex))
                .filter(|hash| *hash > 0 && *hash <= u64::from(u32::MAX))
                .map_or(SavedPlug::Invalid(value), SavedPlug::Hash),
        },
        other => SavedPlug::Invalid(other),
    }
}

pub(super) fn draw_saved_plug(ui: &mut egui::Ui, catalog: &Catalog, plugs: &Value, index: usize) {
    let saved = saved_plug(plugs, index);
    match &saved {
        SavedPlug::Hash(hash) => draw_plug(ui, catalog, *hash),
        SavedPlug::Invalid(value) => {
            ui.colored_label(ui.visuals().error_fg_color, value.to_string());
        }
        other => {
            ui.weak(match other {
                SavedPlug::NativeDefault => "Native Defaults",
                SavedPlug::Empty => "Empty",
                _ => "Not Stored",
            });
        }
    }
}

fn draw_plug(ui: &mut egui::Ui, catalog: &Catalog, hash: u64) {
    draw_named_catalog_hash_link(
        ui,
        catalog,
        hash,
        catalog.package_item_name(hash).unwrap_or("Unresolved Plug"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_defaults_empty_missing_and_invalid_remain_distinct() {
        assert_eq!(saved_plug(&Value::Null, 8), SavedPlug::NativeDefault);
        let plugs = serde_json::json!([null, 12, "0x0000000D", 0, "bad", -1, 4294967296_u64]);
        assert_eq!(saved_plug(&plugs, 0), SavedPlug::Empty);
        assert_eq!(saved_plug(&plugs, 1), SavedPlug::Hash(12));
        assert_eq!(saved_plug(&plugs, 2), SavedPlug::Hash(13));
        for index in 3..7 {
            assert!(matches!(saved_plug(&plugs, index), SavedPlug::Invalid(_)));
        }
        assert_eq!(saved_plug(&plugs, 7), SavedPlug::Missing);
    }
}
