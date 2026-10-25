//! Lossless inspection of native rows alongside the intentionally bounded authoring view.
#[cfg(test)]
use super::state::InvestmentTable;
#[cfg(test)]
use super::*;
pub(super) use crate::persistence::progression::native::hidden_count;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_objectives_accept_values_that_legacy_json_reserves() {
        let mut document =
            json!({"state":{"unlocks":{"character_objective_values":[[443,7],[502,9]]}}});
        assert!(parse(&document).is_err());
        document["_native_progression"] = json!({});
        assert!(parse(&document).is_ok());
        assert!(
            super::super::mutations::set_unlock_value(
                &mut document,
                "character_object_objective_values",
                443,
                8
            )
            .changed()
        );
        assert!(
            super::super::mutations::remove_unlock_value(
                &mut document,
                "character_object_objective_values",
                502
            )
            .changed()
        );
        assert_eq!(
            parse(&document).unwrap().unlocks.character_objective_values,
            vec![IndexedValue {
                index: 443,
                value: 8
            }]
        );
    }

    #[test]
    fn hidden_native_rows_count_against_authoring_capacity() {
        let mut document = json!({"_native_progression":{"hidden_family_counts":[100,0]}});
        assert!(
            !super::super::mutations::set_investment_override(
                &mut document,
                InvestmentTable::FlagOverrides,
                10,
                2
            )
            .changed()
        );
        assert!(
            super::super::mutations::set_investment_override(
                &mut document,
                InvestmentTable::ValueOverrides,
                10,
                42
            )
            .changed()
        );
    }
}
