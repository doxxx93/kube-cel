//! Conversion from `serde_json::Value` to `cel::Value`.
//!
//! This module provides [`json_to_cel`] which recursively converts a JSON value
//! into the CEL value representation used by the `cel` crate. The converted
//! values can then be bound as variables (e.g. `self`, `oldSelf`) in a CEL
//! evaluation context.

use std::collections::HashMap;
use std::sync::Arc;

use cel::objects::{Key, Map};
use cel::Value;

/// Convert a [`serde_json::Value`] into a [`cel::Value`].
///
/// # Number conversion priority
///
/// JSON numbers are converted using the following priority:
/// 1. `i64` — if the number fits in a signed 64-bit integer
/// 2. `u64` — if the number fits in an unsigned 64-bit integer (but not `i64`)
/// 3. `f64` — for all other numeric values (floating-point)
pub fn json_to_cel(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => convert_number(n),
        serde_json::Value::String(s) => Value::String(Arc::new(s.clone())),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(json_to_cel).collect();
            Value::List(Arc::new(items))
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::with_capacity(obj.len());
            for (k, v) in obj {
                map.insert(Key::String(Arc::new(k.clone())), json_to_cel(v));
            }
            Value::Map(Map {
                map: Arc::new(map),
            })
        }
    }
}

fn convert_number(n: &serde_json::Number) -> Value {
    if let Some(i) = n.as_i64() {
        Value::Int(i)
    } else if let Some(u) = n.as_u64() {
        Value::UInt(u)
    } else {
        Value::Float(n.as_f64().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_null() {
        assert_eq!(json_to_cel(&json!(null)), Value::Null);
    }

    #[test]
    fn test_bool() {
        assert_eq!(json_to_cel(&json!(true)), Value::Bool(true));
        assert_eq!(json_to_cel(&json!(false)), Value::Bool(false));
    }

    #[test]
    fn test_i64() {
        assert_eq!(json_to_cel(&json!(42)), Value::Int(42));
        assert_eq!(json_to_cel(&json!(-1)), Value::Int(-1));
        assert_eq!(json_to_cel(&json!(0)), Value::Int(0));
    }

    #[test]
    fn test_u64_beyond_i64() {
        let big: u64 = (i64::MAX as u64) + 1;
        let v = json_to_cel(&serde_json::Value::Number(
            serde_json::Number::from(big),
        ));
        assert_eq!(v, Value::UInt(big));
    }

    #[test]
    fn test_float() {
        assert_eq!(json_to_cel(&json!(3.14)), Value::Float(3.14));
        assert_eq!(json_to_cel(&json!(0.0)), Value::Float(0.0));
    }

    #[test]
    fn test_string() {
        assert_eq!(
            json_to_cel(&json!("hello")),
            Value::String(Arc::new("hello".into()))
        );
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(
            json_to_cel(&json!("")),
            Value::String(Arc::new(String::new()))
        );
    }

    #[test]
    fn test_array_mixed() {
        let v = json_to_cel(&json!([1, "two", true, null]));
        let expected = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::String(Arc::new("two".into())),
            Value::Bool(true),
            Value::Null,
        ]));
        assert_eq!(v, expected);
    }

    #[test]
    fn test_empty_array() {
        assert_eq!(json_to_cel(&json!([])), Value::List(Arc::new(vec![])));
    }

    #[test]
    fn test_object() {
        let v = json_to_cel(&json!({"name": "test", "count": 5}));
        if let Value::Map(map) = v {
            assert_eq!(
                map.map.get(&Key::String(Arc::new("name".into()))),
                Some(&Value::String(Arc::new("test".into())))
            );
            assert_eq!(
                map.map.get(&Key::String(Arc::new("count".into()))),
                Some(&Value::Int(5))
            );
        } else {
            panic!("expected Map");
        }
    }

    #[test]
    fn test_empty_object() {
        let v = json_to_cel(&json!({}));
        if let Value::Map(map) = v {
            assert!(map.map.is_empty());
        } else {
            panic!("expected Map");
        }
    }

    #[test]
    fn test_nested_structure() {
        let v = json_to_cel(&json!({
            "spec": {
                "replicas": 3,
                "items": [1, 2, 3]
            }
        }));
        if let Value::Map(outer) = v {
            let spec = outer.map.get(&Key::String(Arc::new("spec".into()))).unwrap();
            if let Value::Map(inner) = spec {
                assert_eq!(
                    inner.map.get(&Key::String(Arc::new("replicas".into()))),
                    Some(&Value::Int(3))
                );
                assert_eq!(
                    inner.map.get(&Key::String(Arc::new("items".into()))),
                    Some(&Value::List(Arc::new(vec![
                        Value::Int(1),
                        Value::Int(2),
                        Value::Int(3),
                    ])))
                );
            } else {
                panic!("expected inner Map");
            }
        } else {
            panic!("expected outer Map");
        }
    }

    #[test]
    fn test_number_priority() {
        // i64 range → Int
        assert_eq!(json_to_cel(&json!(42)), Value::Int(42));
        // u64 beyond i64 → UInt
        let big: u64 = (i64::MAX as u64) + 1;
        assert_eq!(
            json_to_cel(&serde_json::Value::Number(serde_json::Number::from(big))),
            Value::UInt(big)
        );
        // float → Float
        assert_eq!(json_to_cel(&json!(1.5)), Value::Float(1.5));
    }
}
