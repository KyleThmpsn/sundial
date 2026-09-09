//! Strict JSON parsing shared by settings and authored documents.

use std::{collections::HashSet, fmt, io::Read};

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

const MAX_CONTAINER_DEPTH: usize = 16;

pub(crate) fn from_str<T: DeserializeOwned>(input: &str) -> Result<T, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let value = StrictValueSeed { depth: 0 }.deserialize(&mut deserializer)?;
    deserializer.end()?;
    serde_json::from_value(value)
}

pub(crate) fn from_reader<R: Read, T: DeserializeOwned>(reader: R) -> Result<T, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let value = StrictValueSeed { depth: 0 }.deserialize(&mut deserializer)?;
    deserializer.end()?;
    serde_json::from_value(value)
}

#[derive(Clone, Copy)]
struct StrictValueSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for StrictValueSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // The runtime rejects any value reached at depth 16. An empty container at depth 15 is
        // still valid because parsing it never descends to another value.
        if self.depth >= MAX_CONTAINER_DEPTH {
            return Err(de::Error::custom(format!(
                "JSON nesting exceeds the supported {MAX_CONTAINER_DEPTH}-container limit"
            )));
        }
        deserializer.deserialize_any(StrictValueVisitor { depth: self.depth })
    }
}

struct StrictValueVisitor {
    depth: usize,
}

impl StrictValueVisitor {
    fn nested<E: de::Error>(&self) -> Result<StrictValueSeed, E> {
        if self.depth >= MAX_CONTAINER_DEPTH {
            return Err(E::custom(format!(
                "JSON nesting exceeds the supported {MAX_CONTAINER_DEPTH}-container limit"
            )));
        }
        Ok(StrictValueSeed {
            depth: self.depth + 1,
        })
    }
}

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("JSON numbers must be finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let nested = self.nested()?;
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or_default());
        while let Some(value) = sequence.next_element_seed(nested)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let nested = self.nested()?;
        let mut values = Map::new();
        let mut names = HashSet::with_capacity(object.size_hint().unwrap_or_default());
        while let Some(name) = object.next_key::<String>()? {
            if !names.insert(name.clone()) {
                return Err(de::Error::custom(format!(
                    "duplicate object member {name:?}"
                )));
            }
            values.insert(name, object.next_value_seed(nested)?);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_object_members_before_value_conversion() {
        let error = from_str::<Value>(r#"{"definition_hash": 1, "definition_hash": 2}"#)
            .expect_err("duplicate members must be rejected");
        assert!(error.to_string().contains("duplicate object member"));
    }

    #[test]
    fn rejects_documents_beyond_the_runtime_depth_limit() {
        let mut encoded = "0".to_owned();
        for _ in 0..MAX_CONTAINER_DEPTH {
            encoded = format!("[{encoded}]");
        }
        let error = from_str::<Value>(&encoded).expect_err("overly deep JSON must be rejected");
        assert!(error.to_string().contains("nesting exceeds"));

        let mut deepest_empty_container = "[]".to_owned();
        for _ in 1..MAX_CONTAINER_DEPTH {
            deepest_empty_container = format!("[{deepest_empty_container}]");
        }
        assert!(from_str::<Value>(&deepest_empty_container).is_ok());
    }

    #[test]
    fn preserves_normal_json_values() {
        assert_eq!(
            from_str::<Value>(r#"{"a":[null,true,-3,4.5,"ok"]}"#).unwrap(),
            serde_json::json!({"a": [null, true, -3, 4.5, "ok"]})
        );
    }
}
