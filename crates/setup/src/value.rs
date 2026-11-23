//! Conversion between the maintained vault value codec and component JSON.

use careerbrew_vault::canon::Canon;
use careerbrew_vault::json_yaml;
use serde_json::{Map, Number, Value};

pub fn from_canon(value: &Canon) -> Value {
    match value {
        Canon::Null => Value::Null,
        Canon::Bool(value) => Value::Bool(*value),
        Canon::Int(value) => Value::Number(value.to_string().parse().expect("integer is JSON")),
        Canon::Float(value) => Number::from_f64(*value).map_or(Value::Null, Value::Number),
        Canon::Str(value) | Canon::Date(value) | Canon::DateTime(value) => {
            Value::String(value.clone())
        }
        Canon::List(items) => Value::Array(items.iter().map(from_canon).collect()),
        Canon::Map(items) => {
            let mut map = Map::new();
            for (key, value) in items {
                let key = match key {
                    Canon::Str(key) | Canon::Date(key) | Canon::DateTime(key) => key.clone(),
                    other => from_canon(other).to_string(),
                };
                map.insert(key, from_canon(value));
            }
            Value::Object(map)
        }
    }
}

pub fn to_canon(value: &Value) -> Canon {
    match value {
        Value::Null => Canon::Null,
        Value::Bool(value) => Canon::Bool(*value),
        Value::Number(value) => value
            .as_i64()
            .map(|number| Canon::Int(number.into()))
            .or_else(|| value.as_u64().map(|number| Canon::Int(number.into())))
            .or_else(|| value.as_f64().map(Canon::Float))
            .unwrap_or(Canon::Null),
        Value::String(value) => Canon::Str(value.clone()),
        Value::Array(items) => Canon::List(items.iter().map(to_canon).collect()),
        Value::Object(items) => Canon::Map(
            items
                .iter()
                .map(|(key, value)| (Canon::Str(key.clone()), to_canon(value)))
                .collect(),
        ),
    }
}

pub fn yaml_read(text: &str) -> Result<Value, String> {
    json_yaml::yaml_read(text).map(|value| from_canon(&value))
}

pub fn yaml_write(value: &Value) -> Result<String, String> {
    json_yaml::yaml_write(&to_canon(value))
}

pub fn json_write(value: &Value) -> Result<String, String> {
    json_yaml::json_write(&to_canon(value))
}

pub fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object()?.get(key)
}

pub fn get_mut<'a>(value: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    value.as_object_mut()?.get_mut(key)
}

pub fn set(value: &mut Value, key: &str, item: Value) {
    if let Some(map) = value.as_object_mut() {
        map.insert(key.to_owned(), item);
    }
}

pub fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => value.to_string(),
        Some(other) => python_json(other),
    }
}

pub fn bool_value(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|v| v != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
    }
}

pub fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().map(|item| text(Some(item))).collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(item) => vec![text(Some(item))],
    }
}

/// Python's typed report shape: integers stay distinguishable from strings.
pub fn typed(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => value.clone(),
        Value::Number(number) => serde_json::json!({"int": number.to_string()}),
        Value::Array(items) => Value::Array(items.iter().map(typed).collect()),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(key, value)| (key.clone(), typed(value)))
                .collect(),
        ),
    }
}

/// `json.dumps` default separators for the CLI policy projection.
pub fn python_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).expect("string JSON"),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_json).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    serde_json::to_string(key).unwrap(),
                    python_json(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
