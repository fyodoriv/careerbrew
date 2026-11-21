//! The typed value form both sides print, so a type difference (an int
//! against a float, a date against a string) is a visible difference.
//!
//! The JSON shape is `{"t": <type>, "v": <value>}`:
//! - `null` (no `v`), `bool`, `str`;
//! - `int`: the decimal digits as a string;
//! - `float`: the 16 hex digits of the IEEE 754 bits, big-endian;
//! - `date`: `YYYY-MM-DD`; `datetime`: Python's `isoformat()`;
//! - `list`: a list of values; `map`: a list of `[key, value]` pairs in order.
use num_bigint::BigInt;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Canon {
    Null,
    Bool(bool),
    Int(BigInt),
    Float(f64),
    Str(String),
    Date(String),
    DateTime(String),
    List(Vec<Canon>),
    Map(Vec<(Canon, Canon)>),
}

impl Canon {
    pub fn to_json(&self) -> Value {
        match self {
            Canon::Null => json!({"t": "null"}),
            Canon::Bool(v) => json!({"t": "bool", "v": v}),
            Canon::Int(v) => json!({"t": "int", "v": v.to_string()}),
            Canon::Float(v) => json!({"t": "float", "v": format!("{:016x}", v.to_bits())}),
            Canon::Str(v) => json!({"t": "str", "v": v}),
            Canon::Date(v) => json!({"t": "date", "v": v}),
            Canon::DateTime(v) => json!({"t": "datetime", "v": v}),
            Canon::List(items) => {
                json!({"t": "list", "v": items.iter().map(Canon::to_json).collect::<Vec<_>>()})
            }
            Canon::Map(pairs) => json!({"t": "map", "v": pairs
                .iter()
                .map(|(k, v)| json!([k.to_json(), v.to_json()]))
                .collect::<Vec<_>>()}),
        }
    }

    pub fn from_json(value: &Value) -> Result<Canon, String> {
        let kind = value["t"].as_str().ok_or("value has no type")?;
        let v = &value["v"];
        let text = || v.as_str().map(str::to_owned).ok_or("value is not text");
        Ok(match kind {
            "null" => Canon::Null,
            "bool" => Canon::Bool(v.as_bool().ok_or("not a bool")?),
            "int" => Canon::Int(text()?.parse().map_err(|_| "bad int")?),
            "float" => Canon::Float(f64::from_bits(
                u64::from_str_radix(&text()?, 16).map_err(|_| "bad float bits")?,
            )),
            "str" => Canon::Str(text()?),
            "date" => Canon::Date(text()?),
            "datetime" => Canon::DateTime(text()?),
            "list" => Canon::List(
                v.as_array()
                    .ok_or("list is not a list")?
                    .iter()
                    .map(Canon::from_json)
                    .collect::<Result<_, _>>()?,
            ),
            "map" => Canon::Map(
                v.as_array()
                    .ok_or("map is not a list")?
                    .iter()
                    .map(|pair| match pair.as_array().map(Vec::as_slice) {
                        Some([k, v]) => Ok((Canon::from_json(k)?, Canon::from_json(v)?)),
                        _ => Err("map pair is not a pair".to_owned()),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            _ => return Err("unknown value type".into()),
        })
    }
}

/// Inserts a pair the way a Python dict does: a repeated key keeps its first
/// place and takes the last value.
pub fn insert(pairs: &mut Vec<(Canon, Canon)>, key: Canon, value: Canon) {
    match pairs.iter_mut().find(|(k, _)| *k == key) {
        Some(pair) => pair.1 = value,
        None => pairs.push((key, value)),
    }
}
