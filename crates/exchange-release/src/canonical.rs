//! Strict canonical JSON decoding.

use serde::de::{DeserializeOwned, Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::fmt;

use crate::{Error, Result, JCS_SAFE_INTEGER};

struct NoDuplicateValue(Value);

impl<'de> Deserialize<'de> for NoDuplicateValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = Value;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value")
            }

            fn visit_bool<E: serde::de::Error>(self, value: bool) -> std::result::Result<Value, E> {
                Ok(Value::Bool(value))
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> std::result::Result<Value, E> {
                Ok(Value::Number(value.into()))
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> std::result::Result<Value, E> {
                Ok(Value::Number(value.into()))
            }

            fn visit_f64<E: serde::de::Error>(self, value: f64) -> std::result::Result<Value, E> {
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Value, E> {
                Ok(Value::String(value.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Value, E> {
                Ok(Value::String(value))
            }

            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
                Ok(Value::Null)
            }

            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
                Ok(Value::Null)
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Value, A::Error> {
                let mut values = Vec::new();
                while let Some(NoDuplicateValue(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(Value::Array(values))
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut object: A,
            ) -> std::result::Result<Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, NoDuplicateValue(value))) =
                    object.next_entry::<String, NoDuplicateValue>()?
                {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(A::Error::custom(format!("duplicate member {key:?}")));
                    }
                }
                Ok(Value::Object(values))
            }
        }
        deserializer.deserialize_any(ValueVisitor).map(Self)
    }
}

/// Parse one byte-identical RFC 8785 object after enforcing the declared body limit.
pub fn parse_value(bytes: &[u8], limit: usize) -> Result<Value> {
    if bytes.len() > limit {
        return Err(Error::Bounds(format!(
            "JSON body is {} bytes; maximum is {limit}",
            bytes.len()
        )));
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(Error::Canonical("UTF-8 BOM is forbidden".into()));
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let NoDuplicateValue(value) = NoDuplicateValue::deserialize(&mut deserializer)
        .map_err(|error| Error::Json(error.to_string()))?;
    deserializer
        .end()
        .map_err(|error| Error::Json(error.to_string()))?;
    if !value.is_object() {
        return Err(Error::Schema(
            "top-level JSON value must be an object".into(),
        ));
    }
    validate_numbers(&value)?;
    let encoded = serde_json_canonicalizer::to_vec(&value)
        .map_err(|error| Error::Canonical(error.to_string()))?;
    if encoded != bytes {
        return Err(Error::Canonical(
            "body is not byte-identical RFC 8785 JSON".into(),
        ));
    }
    Ok(value)
}

/// Parse canonical JSON into a closed Serde type.
pub fn parse<T: DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T> {
    let value = parse_value(bytes, limit)?;
    serde_json::from_value(value).map_err(|error| Error::Schema(error.to_string()))
}

/// Serialize a value using RFC 8785 with no trailing newline.
pub fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value).map_err(|error| Error::Canonical(error.to_string()))
}

fn validate_numbers(value: &Value) -> Result<()> {
    match value {
        Value::Number(number) => {
            let integer = number.as_u64().ok_or_else(|| {
                Error::Bounds(
                    "JSON numbers must be nonnegative integers in the JCS-safe domain".into(),
                )
            })?;
            if integer > JCS_SAFE_INTEGER {
                return Err(Error::Bounds(format!(
                    "JSON integer {integer} exceeds {JCS_SAFE_INTEGER}"
                )));
            }
        }
        Value::Array(values) => values.iter().try_for_each(validate_numbers)?,
        Value::Object(values) => values.values().try_for_each(validate_numbers)?,
        _ => {}
    }
    Ok(())
}
