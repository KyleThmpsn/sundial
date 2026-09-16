//! Mapped field values read out of one native action node.
//!
//! A fact records what a native field stores together with a label for the traced role
//! of that field. It does not assert the gameplay meaning of the value.

/// One mapped field of a condition or effect node.
#[derive(Clone, Debug, PartialEq)]
pub struct Fact {
    /// Title-case label for the field.
    pub label: &'static str,
    /// Decoded value.
    pub value: FactValue,
}

impl Fact {
    pub(super) const fn new(label: &'static str, value: FactValue) -> Self {
        Self { label, value }
    }

    /// `Label: value` for a detail list.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}: {}", self.label, self.value.render())
    }
}

/// A decoded native value, kept in the representation its field uses.
#[derive(Clone, Debug, PartialEq)]
pub enum FactValue {
    /// A duration in seconds.
    Seconds(f32),
    /// A plain number.
    Number(f32),
    /// A signed native 32-bit integer, preserved without floating-point rounding.
    Integer(i32),
    /// A whole number.
    Count(u64),
    /// A boolean flag.
    Flag(bool),
    /// A 32-bit name key, normally an FNV-1 hash.
    Key(u32),
    /// A package tag reference.
    Tag(u32),
    /// A raw selector or enumeration byte whose names are not recovered.
    Selector(u8),
    /// A bit mask.
    Mask(u64),
    /// Source label hashes from a compiled filter.
    Labels(Vec<u32>),
    /// An inclusive numeric range.
    Range(f32, f32),
}

impl FactValue {
    /// The value on its own, without its label.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Seconds(value) => format!("{} s", trim_number(*value)),
            Self::Number(value) => trim_number(*value),
            Self::Integer(value) => value.to_string(),
            Self::Count(value) => value.to_string(),
            Self::Flag(value) => if *value { "Yes" } else { "No" }.to_owned(),
            Self::Key(value) => format!("0x{value:08X}"),
            Self::Tag(value) => format!("0x{value:08X}"),
            Self::Selector(value) => value.to_string(),
            Self::Mask(value) => format!("0x{value:X}"),
            Self::Labels(values) => render_labels(values),
            Self::Range(low, high) => format!("{} to {}", trim_number(*low), trim_number(*high)),
        }
    }
}

fn render_labels(values: &[u32]) -> String {
    if values.is_empty() {
        return "none".to_owned();
    }
    values
        .iter()
        .map(|value| label_name(*value).map_or_else(|| format!("0x{value:08X}"), str::to_owned))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Recovered name for a source label hash.
#[must_use]
pub fn label_name(hash: u32) -> Option<&'static str> {
    crate::package_runtime::labels::name(hash)
}

pub(super) fn trim_number(value: f32) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    if (value - value.round()).abs() < f32::EPSILON {
        return format!("{}", value.round());
    }
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_render_without_trailing_zeros_or_placeholder_names() {
        assert_eq!(FactValue::Seconds(5.0).render(), "5 s");
        assert_eq!(FactValue::Seconds(2.5).render(), "2.5 s");
        assert_eq!(FactValue::Number(0.35).render(), "0.35");
        assert_eq!(FactValue::Flag(true).render(), "Yes");
        assert_eq!(FactValue::Key(0x1234_5678).render(), "0x12345678");
        assert_eq!(FactValue::Labels(Vec::new()).render(), "none");
        assert_eq!(
            FactValue::Labels(vec![0x962E_A19B, 0x0000_0001]).render(),
            "precision, 0x00000001"
        );
        assert_eq!(FactValue::Range(0.0, 1.5).render(), "0 to 1.5");
        assert_eq!(
            Fact::new("Duration", FactValue::Seconds(1.0)).render(),
            "Duration: 1 s"
        );
    }
}
