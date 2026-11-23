//! JSON read with every object's keys required to be different. A plain read keeps the last of
//! two equal keys and says nothing, which is how a setting written twice in one file could
//! quietly mean the opposite of its first line. Every file a person edits is read this way.

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_str<E>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(Value::from(v)))
            }
            fn visit_unit<E>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = Vec::new();
                while let Some(Strict(x)) = a.next_element()? {
                    v.push(x);
                }
                Ok(Strict(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut m = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    let Strict(v) = a.next_value()?;
                    if m.insert(k.clone(), v).is_some() {
                        let twice = format!("the key {k:?} comes twice");
                        return Err(serde::de::Error::custom(twice));
                    }
                }
                Ok(Strict(Value::Object(m)))
            }
        }
        d.deserialize_any(V)
    }
}

/// JSON text as a value; an object's key coming twice, at any depth, is an error.
pub fn json(text: &str) -> Result<Value, String> {
    serde_json::from_str::<Strict>(text)
        .map(|s| s.0)
        .map_err(|e| format!("not JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::json;

    #[test]
    fn a_key_that_comes_twice_is_an_error_at_any_depth() {
        assert_eq!(
            json(r#"{"a": 1, "b": [true, null, "x", 1.5]}"#).unwrap()["b"][2],
            "x"
        );
        assert!(json(r#"{"a": "off", "a": "on"}"#)
            .unwrap_err()
            .contains("twice"));
        assert!(json(r#"{"x": {"a": 1, "a": 2}}"#)
            .unwrap_err()
            .contains("twice"));
        assert!(json(r#"[{"a": 1, "a": 2}]"#).unwrap_err().contains("twice"));
        assert!(json("{").is_err());
    }
}
