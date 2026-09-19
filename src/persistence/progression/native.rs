use super::*;
pub(crate) fn hidden_count(document: &Value, table: InvestmentTable) -> usize {
    let kind = match table {
        InvestmentTable::FlagOverrides => 0,
        InvestmentTable::ValueOverrides => 1,
    };
    document["_native_progression"]["hidden_family_counts"][kind]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}
