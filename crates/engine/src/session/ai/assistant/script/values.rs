//! What crosses between a script and MailCraft: JSON (what the tools take and give) as Rhai's
//! values, and back. Both ways are bounded in depth, as a script can nest an array in itself
//! as deep as its operations let it.

use rhai::{Array, Dynamic, Map};
use serde_json::{Number, Value};

/// The deepest a value is taken across; deeper, it's left out (`()`, `null`).
const DEPTH_MOST: usize = 32;

/// `v` as a script's value: objects as maps, arrays as arrays, whole numbers as integers.
pub(super) fn to_script(v: &Value) -> Dynamic {
    to_script_at(v, 0)
}

fn to_script_at(v: &Value, depth: usize) -> Dynamic {
    if depth >= DEPTH_MOST {
        return Dynamic::UNIT;
    }
    match v {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from_bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Dynamic::from_int(i),
            None => n.as_f64().map_or(Dynamic::UNIT, Dynamic::from_float),
        },
        Value::String(s) => Dynamic::from(s.clone()),
        Value::Array(a) => Dynamic::from_array(a.iter().map(|v| to_script_at(v, depth + 1)).collect::<Array>()),
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, v) in o {
                m.insert(k.as_str().into(), to_script_at(v, depth + 1));
            }
            Dynamic::from_map(m)
        }
    }
}

/// A script's value as JSON: maps as objects, `()` as null, what has no JSON form (a function's
/// pointer) as its text.
pub(super) fn to_json(d: &Dynamic) -> Value {
    to_json_at(d, 0)
}

fn to_json_at(d: &Dynamic, depth: usize) -> Value {
    if depth >= DEPTH_MOST || d.is_unit() {
        return Value::Null;
    }
    if let Ok(b) = d.as_bool() {
        return Value::Bool(b);
    }
    if let Ok(i) = d.as_int() {
        return Value::Number(i.into());
    }
    if let Ok(f) = d.as_float() {
        return Number::from_f64(f).map_or(Value::Null, Value::Number);
    }
    if let Ok(c) = d.as_char() {
        return Value::String(c.to_string());
    }
    if let Ok(a) = d.as_array_ref() {
        return Value::Array(a.iter().map(|v| to_json_at(v, depth + 1)).collect());
    }
    if let Ok(m) = d.as_map_ref() {
        return Value::Object(m.iter().map(|(k, v)| (k.to_string(), to_json_at(v, depth + 1))).collect());
    }
    match d.clone().into_immutable_string() {
        Ok(s) => Value::String(s.to_string()),
        Err(_) => Value::String(d.to_string()),
    }
}
